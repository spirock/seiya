//! Discovery by default; JSON chat/buffered SSE require explicit runtime config.
pub mod config;
pub mod desktop_config;
pub mod diagnostics;
pub mod image_jobs;
#[cfg(target_os = "macos")]
pub mod image_web;
pub mod inference;
mod job_store;
pub mod jobs;
#[cfg(target_os = "macos")]
pub mod launcher;
pub mod pair_trial;
pub mod prepared_web;
pub mod reader;
pub mod runtime;
pub mod scheduler;
mod sse;
#[cfg(target_os = "macos")]
pub mod web_diagnostics;

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use config::{Config, ConfigError};
use inference::{InferenceAdapter, ValidatedInput, MAX_CHAT_BYTES};
use runtime::HttpTransport;
use scheduler::Scheduler;
use serde_json::{json, Value};
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

#[derive(Clone)]
struct HttpState {
    origin_scheme: &'static str,
    allowed_hosts: [String; 2],
    authorization: Option<Arc<str>>,
    scheduler: Option<Scheduler>,
    readers: Arc<Semaphore>,
    read_timeout: Duration,
    reader: Option<Arc<reader::Reader>>,
    jobs: Option<Arc<jobs::Jobs>>,
}

fn error(status: StatusCode, message: &'static str) -> Response {
    json_response(status, json!({"error": {"message": message}}))
}

fn json_response(status: StatusCode, body: Value) -> Response {
    let mut response = (
        status,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (
                header::HeaderName::from_static("x-seiya-experimental"),
                "discovery-only",
            ),
        ],
        Json(body),
    )
        .into_response();
    if !status.is_success() {
        response
            .headers_mut()
            .insert(header::CONNECTION, "close".parse().unwrap());
    }
    response
}

fn single_header(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    value.to_str().ok()
}

fn authorize(state: &HttpState, headers: &HeaderMap) -> Option<Response> {
    let host = single_header(headers, header::HOST);
    if !host.is_some_and(|host| state.allowed_hosts.iter().any(|allowed| host == allowed)) {
        return Some(error(StatusCode::FORBIDDEN, "untrusted Host"));
    }
    if headers.contains_key(header::ORIGIN) {
        let origin = single_header(headers, header::ORIGIN);
        if !origin.is_some_and(|origin| {
            state
                .allowed_hosts
                .iter()
                .any(|allowed| origin == format!("{}://{allowed}", state.origin_scheme))
        }) {
            return Some(error(
                StatusCode::FORBIDDEN,
                "cross-origin requests are disabled",
            ));
        }
    }
    if let Some(expected) = &state.authorization {
        let supplied = single_header(headers, header::AUTHORIZATION).unwrap_or("");
        if !bool::from(supplied.as_bytes().ct_eq(expected.as_bytes())) {
            return Some(error(StatusCode::UNAUTHORIZED, "API key required"));
        }
    }
    None
}

async fn guard(State(state): State<HttpState>, request: Request, next: Next) -> Response {
    let mut response = match authorize(&state, request.headers()) {
        Some(error) => error,
        None => {
            let name = header::HeaderName::from_static("x-seiya-model-binding");
            if request.headers().contains_key(&name)
                && !single_header(request.headers(), name).is_some_and(|binding| {
                    state
                        .scheduler
                        .as_ref()
                        .is_some_and(|s| s.model_binding_matches(binding))
                })
            {
                error(StatusCode::BAD_REQUEST, "runtime model binding mismatch")
            } else {
                next.run(request).await
            }
        }
    };
    if state.scheduler.is_some() {
        response
            .headers_mut()
            .insert("x-seiya-experimental", "chat-buffered-sse".parse().unwrap());
    }
    response
}

async fn health(State(state): State<HttpState>) -> Response {
    let busy = state.scheduler.as_ref().is_some_and(Scheduler::is_busy);
    let status = state.scheduler.as_ref().and_then(Scheduler::runtime_status);
    let readiness = if state
        .scheduler
        .as_ref()
        .is_some_and(Scheduler::requires_recovery)
    {
        "unconfirmed after runtime error"
    } else {
        "not checked"
    };
    let readiness = if status
        .as_ref()
        .is_some_and(|s| s["endpoint_observation"] == "connection_not_established")
    {
        "connection not established; operator recovery required"
    } else {
        readiness
    };
    let mut body = json!({"status": "ok", "model": "pegassus", "busy": busy,
            "backend_readiness": readiness, "version": env!("CARGO_PKG_VERSION")});
    if let Some(status) = status {
        body["runtime_status"] = status;
        body["runtime_bridge"] = json!({"version":1,"model_binding":"hex-utf8-v1"});
    }
    json_response(StatusCode::OK, body)
}

async fn models() -> Response {
    json_response(
        StatusCode::OK,
        json!({"object": "list", "data": [
            {"id": "pegassus", "object": "model", "owned_by": "seiya"}
        ]}),
    )
}

async fn not_found() -> Response {
    error(StatusCode::NOT_FOUND, "not found")
}

async fn method_not_allowed() -> Response {
    error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed")
}

fn json_content_type(headers: &HeaderMap) -> bool {
    let Some(value) = single_header(headers, header::CONTENT_TYPE) else {
        return false;
    };
    let mut parts = value.split(';');
    if !parts
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("application/json")
    {
        return false;
    }
    if let Some(parameter) = parts.next() {
        let value = parameter.trim();
        if !value.eq_ignore_ascii_case("charset=utf-8")
            && !value.eq_ignore_ascii_case("charset=\"utf-8\"")
        {
            return false;
        }
    }
    parts.next().is_none()
}

async fn chat(State(state): State<HttpState>, request: Request) -> Response {
    let Some(scheduler) = &state.scheduler else {
        return error(
            StatusCode::NOT_IMPLEMENTED,
            "chat completions is not implemented in this experimental build",
        );
    };
    if scheduler.is_closed() {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "experimental server is shutting down",
        );
    }
    let size = single_header(request.headers(), header::CONTENT_LENGTH)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| s.parse::<usize>().ok());
    if request.headers().contains_key(header::TRANSFER_ENCODING)
        || !size.is_some_and(|n| (1..=MAX_CHAT_BYTES).contains(&n))
    {
        return error(
            StatusCode::BAD_REQUEST,
            "body must be 1–262144 bytes; chunked requests unsupported",
        );
    }
    if !json_content_type(request.headers()) {
        return error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Content-Type must be application/json with optional UTF-8 charset",
        );
    }
    if request.headers().contains_key(header::CONTENT_ENCODING)
        && !single_header(request.headers(), header::CONTENT_ENCODING)
            .is_some_and(|v| v.eq_ignore_ascii_case("identity"))
    {
        return error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "compressed request bodies are unsupported",
        );
    }
    let reader = match state.readers.clone().try_acquire_owned() {
        Ok(reader) => reader,
        Err(_) => {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "request body capacity full; retry later",
            )
        }
    };
    let bytes = match tokio::time::timeout(
        state.read_timeout,
        axum::body::to_bytes(request.into_body(), MAX_CHAT_BYTES),
    )
    .await
    {
        Err(_) => return error(StatusCode::REQUEST_TIMEOUT, "request body read timed out"),
        Ok(Err(_)) => return error(StatusCode::BAD_REQUEST, "invalid JSON request body"),
        Ok(Ok(bytes)) => bytes,
    };
    if Some(bytes.len()) != size {
        return error(
            StatusCode::BAD_REQUEST,
            "request body length does not match Content-Length",
        );
    }
    let body: Value = match serde_json::from_slice(&bytes) {
        Ok(body) => body,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid JSON request body"),
    };
    drop(reader);
    let stream = body.get("stream").and_then(Value::as_bool) == Some(true);
    let include_usage = body
        .get("stream_options")
        .and_then(|options| options.get("include_usage"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let body = match ValidatedInput::new(body) {
        Ok(body) => body,
        Err(failure) => return json_response(failure.status, failure.body()),
    };
    match scheduler.complete_validated(body).await {
        Ok(result) if stream => sse::buffered(result, include_usage),
        Ok(result) => json_response(StatusCode::OK, result),
        Err(failure) => json_response(failure.status, failure.body()),
    }
}

async fn reader_request(State(state): State<HttpState>, request: Request) -> Response {
    let start = tokio::time::Instant::now();
    let cancelling = request.uri().path() == "/internal/reader/v0.1/cancel";
    let Some(reader) = &state.reader else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "reader unavailable");
    };
    let size = single_header(request.headers(), header::CONTENT_LENGTH)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| s.parse::<usize>().ok());
    if size.is_some_and(|n| n > 16 * 1024) {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "reader frame too large");
    }
    if request.headers().contains_key(header::TRANSFER_ENCODING) || size.is_none_or(|n| n == 0) {
        return error(StatusCode::BAD_REQUEST, "invalid reader frame length");
    }
    if !json_content_type(request.headers())
        || request.headers().contains_key(header::CONTENT_ENCODING)
    {
        return error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "reader requires uncompressed JSON",
        );
    }
    let permit = match state.readers.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "reader body capacity full"),
    };
    let bytes = match tokio::time::timeout(
        state.read_timeout,
        axum::body::to_bytes(request.into_body(), 16 * 1024),
    )
    .await
    {
        Ok(Ok(bytes)) => bytes,
        Err(_) => return error(StatusCode::REQUEST_TIMEOUT, "reader body deadline"),
        Ok(Err(_)) => return error(StatusCode::BAD_REQUEST, "invalid reader frame"),
    };
    drop(permit);
    if Some(bytes.len()) != size {
        return error(StatusCode::BAD_REQUEST, "reader length mismatch");
    }
    if cancelling {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Cancel {
            version: String,
            run_id: String,
        }
        let cancel: Cancel = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => return error(StatusCode::BAD_REQUEST, "invalid cancellation frame"),
        };
        if cancel.version != "0.1-draft.1" {
            return error(StatusCode::BAD_REQUEST, "invalid cancellation version");
        }
        return match reader.cancel(&cancel.run_id) {
            Ok(()) => json_response(
                StatusCode::ACCEPTED,
                json!({"status":"cancellation_requested"}),
            ),
            Err(reader::Error::Unavailable) => error(
                StatusCode::SERVICE_UNAVAILABLE,
                "cancellation capacity full",
            ),
            Err(_) => error(StatusCode::BAD_REQUEST, "invalid cancellation frame"),
        };
    }
    let frame = match seiya_contracts::reader_frame::parse_request(&bytes, true) {
        Ok(frame) => frame,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid reader frame"),
    };
    let deadline = start + Duration::from_millis(frame.remaining_ms().into());
    match reader.read_batch(&frame, deadline).await {
        Ok(value) => json_response(StatusCode::OK, value),
        Err(reader::Error::Unavailable) => {
            error(StatusCode::SERVICE_UNAVAILABLE, "reader unavailable")
        }
        Err(reader::Error::Timeout) => error(StatusCode::GATEWAY_TIMEOUT, "reader deadline"),
        Err(reader::Error::Limit) => error(StatusCode::PAYLOAD_TOO_LARGE, "reader limit"),
        Err(reader::Error::Cancelled) => error(StatusCode::CONFLICT, "reader cancelled"),
        Err(_) => error(StatusCode::BAD_REQUEST, "reader failed"),
    }
}

async fn reader_select(State(state): State<HttpState>, request: Request) -> Response {
    let size = single_header(request.headers(), header::CONTENT_LENGTH)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| s.parse::<usize>().ok());
    if size.is_some_and(|n| n > 160 * 1024) {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "selection frame too large");
    }
    if size.is_none_or(|n| n == 0)
        || request.headers().contains_key(header::TRANSFER_ENCODING)
        || request.headers().contains_key(header::CONTENT_ENCODING)
        || !json_content_type(request.headers())
    {
        return error(StatusCode::BAD_REQUEST, "invalid selection framing");
    }
    let _permit = match state.readers.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "selection capacity full"),
    };
    let bytes = match tokio::time::timeout(
        state.read_timeout,
        axum::body::to_bytes(request.into_body(), 160 * 1024),
    )
    .await
    {
        Ok(Ok(bytes)) if Some(bytes.len()) == size => bytes,
        _ => return error(StatusCode::BAD_REQUEST, "invalid selection frame"),
    };
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Selection {
        request: String,
        response: String,
        ids: Vec<String>,
    }
    let frame: Selection = match serde_json::from_slice(&bytes) {
        Ok(frame) => frame,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid selection frame"),
    };
    let original =
        match seiya_contracts::reader_frame::parse_request(frame.request.as_bytes(), true) {
            Ok(original) => original,
            Err(_) => return error(StatusCode::BAD_REQUEST, "invalid selection request"),
        };
    let ids: Vec<_> = frame.ids.iter().map(String::as_str).collect();
    match reader::selection::select(&original, frame.response.as_bytes(), &ids) {
        Ok(sources) => json_response(
            StatusCode::OK,
            json!({"sources":sources.into_iter().map(|s|
            json!({"id":s.id,"text":s.text,"text_truncated":s.text_truncated})).collect::<Vec<_>>()}),
        ),
        Err(_) => error(StatusCode::BAD_REQUEST, "invalid source selection"),
    }
}

/// Build discovery routes for an already bound loopback address.
/// This is not an inference server and must not replace the production service.
pub fn app(address: SocketAddr, api_key: Option<String>) -> Result<Router, ConfigError> {
    build_app(address, api_key, None, Duration::from_secs(30), None, None)
}

/// For an explicitly configured/injected scheduler; tests must use synthetic runtimes.
pub fn app_with_scheduler(
    address: SocketAddr,
    api_key: Option<String>,
    scheduler: Scheduler,
    read_timeout: Duration,
) -> Result<Router, ConfigError> {
    build_app(address, api_key, Some(scheduler), read_timeout, None, None)
}

/// Experimental library-only transport-stage reader. Not enabled by any CLI
/// configuration. The host must resolve its explicit secret before calling.
pub fn app_with_reader(
    address: SocketAddr,
    api_key: String,
    reader: Arc<reader::Reader>,
) -> Result<Router, ConfigError> {
    config::validate_key(&api_key)?;
    build_app(
        address,
        Some(api_key),
        None,
        Duration::from_secs(3),
        Some(reader),
        None,
    )
}

pub fn app_with_jobs(
    address: SocketAddr,
    api_key: String,
    jobs: Arc<jobs::Jobs>,
) -> Result<Router, ConfigError> {
    config::validate_key(&api_key)?;
    build_app(
        address,
        Some(api_key),
        None,
        Duration::from_secs(3),
        None,
        Some(jobs),
    )
}

async fn job_request(State(state): State<HttpState>, request: Request) -> Response {
    let size = single_header(request.headers(), header::CONTENT_LENGTH)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| s.parse::<usize>().ok());
    if size.is_none_or(|n| n == 0 || n > 16384)
        || request.headers().contains_key(header::TRANSFER_ENCODING)
        || request.headers().contains_key(header::CONTENT_ENCODING)
        || !json_content_type(request.headers())
    {
        return error(StatusCode::BAD_REQUEST, "invalid jobs framing");
    }
    let _permit = match state.readers.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "jobs body capacity full"),
    };
    let bytes = match tokio::time::timeout(
        state.read_timeout,
        axum::body::to_bytes(request.into_body(), 16384),
    )
    .await
    {
        Ok(Ok(bytes)) if Some(bytes.len()) == size => bytes,
        _ => return error(StatusCode::BAD_REQUEST, "invalid jobs body"),
    };
    let command = match serde_json::from_slice::<jobs::Command>(&bytes) {
        Ok(c) => c,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid jobs command"),
    };
    match state.jobs.as_ref().unwrap().control(command) {
        Ok(value) => json_response(StatusCode::OK, value),
        Err(jobs::Error::Unknown) => error(StatusCode::NOT_FOUND, "job/session unavailable"),
        Err(jobs::Error::Capacity) => error(StatusCode::TOO_MANY_REQUESTS, "jobs capacity full"),
        Err(jobs::Error::NotReady) => error(StatusCode::CONFLICT, "job not ready"),
        Err(jobs::Error::Storage) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "job history storage failed; no automatic retry",
        ),
        Err(_) => error(StatusCode::BAD_REQUEST, "invalid jobs command"),
    }
}

fn build_app(
    address: SocketAddr,
    api_key: Option<String>,
    scheduler: Option<Scheduler>,
    read_timeout: Duration,
    reader: Option<Arc<reader::Reader>>,
    jobs: Option<Arc<jobs::Jobs>>,
) -> Result<Router, ConfigError> {
    if address.ip() != Ipv4Addr::LOCALHOST || address.port() == 0 {
        return Err(ConfigError(
            "router requires a bound 127.0.0.1 address with a nonzero port",
        ));
    }
    if let Some(key) = &api_key {
        config::validate_key(key)?;
    }
    if read_timeout < Duration::from_millis(1) || read_timeout > Duration::from_secs(30) {
        return Err(ConfigError(
            "request read timeout must be between 1 and 30000 milliseconds",
        ));
    }
    let state = HttpState {
        origin_scheme: "http",
        allowed_hosts: [
            format!("127.0.0.1:{}", address.port()),
            format!("localhost:{}", address.port()),
        ],
        authorization: api_key.map(|key| Arc::from(format!("Bearer {key}"))),
        scheduler,
        readers: Arc::new(Semaphore::new(8)),
        read_timeout,
        reader,
        jobs,
    };
    let router = Router::new()
        .route("/health", get(health).fallback(method_not_allowed))
        .route("/v1/models", get(models).fallback(method_not_allowed))
        .route(
            "/v1/chat/completions",
            post(chat).fallback(method_not_allowed),
        )
        .fallback(not_found);
    let router = if state.jobs.is_some() {
        router.route(
            "/internal/jobs/v1",
            post(job_request).fallback(method_not_allowed),
        )
    } else {
        router
    };
    let router = if state.reader.is_some() {
        router
            .route(
                "/internal/reader/v0.1/read",
                post(reader_request).fallback(method_not_allowed),
            )
            .route(
                "/internal/reader/v0.1/cancel",
                post(reader_request).fallback(method_not_allowed),
            )
            .route(
                "/internal/reader/v0.1/select",
                post(reader_select).fallback(method_not_allowed),
            )
    } else {
        router
    };
    Ok(router
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, guard)))
}

pub struct BoundServer {
    listener: TcpListener,
    router: Router,
    address: SocketAddr,
    scheduler: Option<Scheduler>,
}

impl BoundServer {
    /// Resolve secrets before binding. Never probes or opens a model runtime.
    pub async fn bind(config: &Config) -> Result<Self, ConfigError> {
        Self::bind_scoped(config, None).await
    }

    /// Host-owned library integration only. Does not create an observer, acquire
    /// approval, launch a runtime or expose JSON permission to change pressure.
    pub async fn bind_with_memory_guard(
        config: &Config,
        guard: seiya_observe::session_guard::MemoryGuard,
    ) -> Result<Self, ConfigError> {
        if !config.runtime_status || config.runtime.is_none() {
            return Err(ConfigError("memory guard requires an opt-in runtime"));
        }
        Self::bind_scoped(config, Some(guard)).await
    }

    async fn bind_scoped(
        config: &Config,
        guard: Option<seiya_observe::session_guard::MemoryGuard>,
    ) -> Result<Self, ConfigError> {
        let key = config.resolve_api_key(|name| std::env::var(name).ok())?;
        let listener = TcpListener::bind(config.listen)
            .await
            .map_err(|_| ConfigError("cannot bind experimental server; address may be occupied"))?;
        let address = listener
            .local_addr()
            .map_err(|_| ConfigError("cannot determine bound address"))?;
        let scheduler = if let Some(runtime) = &config.runtime {
            if runtime.endpoint()?.port() == Some(address.port()) {
                return Err(ConfigError(
                    "runtime endpoint must not point to the frontend listener",
                ));
            }
            let transport = Arc::new(HttpTransport::new(runtime)?);
            let adapter = if config.runtime_status {
                InferenceAdapter::new_with_runtime_status(runtime.clone(), transport)?
            } else {
                InferenceAdapter::new(runtime.clone(), transport)?
            };
            let wait = Duration::from_millis(config.queue_wait_ms.unwrap_or(120000));
            Some(if let Some(guard) = guard {
                Scheduler::new_with_memory_guard(adapter, wait, guard)?
            } else {
                Scheduler::new(adapter, wait)?
            })
        } else {
            None
        };
        let router = build_app(
            address,
            key,
            scheduler.clone(),
            Duration::from_millis(config.request_read_timeout_ms.unwrap_or(30000)),
            None,
            None,
        )?;
        Ok(Self {
            listener,
            router,
            address,
            scheduler,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }

    pub fn chat_enabled(&self) -> bool {
        self.scheduler.is_some()
    }

    pub async fn serve(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> std::io::Result<()> {
        let closing = self.scheduler.clone();
        let result = axum::serve(self.listener, self.router)
            .with_graceful_shutdown(async move {
                shutdown.await;
                if let Some(scheduler) = closing {
                    scheduler.close();
                }
            })
            .await;
        if let Some(scheduler) = self.scheduler {
            scheduler.close_and_drain().await.map_err(|_| {
                std::io::Error::other(
                    "runtime state is unconfirmed after shutdown; verify backend before reuse",
                )
            })?;
        }
        result
    }
}
