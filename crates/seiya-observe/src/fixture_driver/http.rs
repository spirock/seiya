use crate::runner::{Exchange, ExchangeTiming, RunError};
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub(super) const MAX_RESPONSE: usize = 8 * 1024 * 1024;
pub(super) fn client() -> Result<reqwest::Client, RunError> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|_| RunError::Driver)
}
pub(super) async fn bytes(mut response: reqwest::Response) -> Result<Vec<u8>, RunError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| RunError::Driver)? {
        if chunk.len() > MAX_RESPONSE.saturating_sub(bytes.len()) {
            return Err(RunError::Limit);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn single(headers: &HeaderMap, key: header::HeaderName) -> Option<&str> {
    let mut values = headers.get_all(key).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        None
    } else {
        Some(value)
    }
}
fn error(status: StatusCode) -> Response {
    (
        status,
        Json(json!({"error":{"message":"fixture endpoint rejected request"}})),
    )
        .into_response()
}
async fn body(request: Request, host: &str) -> Result<Value, StatusCode> {
    if single(request.headers(), header::HOST) != Some(host)
        || request.headers().contains_key(header::ORIGIN)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    let size = single(request.headers(), header::CONTENT_LENGTH)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| s.parse::<usize>().ok());
    if request.headers().contains_key(header::TRANSFER_ENCODING)
        || !size.is_some_and(|n| (1..=262144).contains(&n))
        || single(request.headers(), header::CONTENT_TYPE) != Some("application/json")
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let bytes = tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), 262144),
    )
    .await
    .map_err(|_| StatusCode::REQUEST_TIMEOUT)?
    .map_err(|_| StatusCode::BAD_REQUEST)?;
    if Some(bytes.len()) != size {
        return Err(StatusCode::BAD_REQUEST);
    }
    serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)
}

pub(super) struct Trace {
    pub requests: Mutex<Vec<Value>>,
    pub unexpected: AtomicBool,
    pub busy: AtomicBool,
    pub enabled: AtomicBool,
}
impl Trace {
    fn new() -> Self {
        Self {
            requests: Mutex::new(vec![]),
            unexpected: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            enabled: AtomicBool::new(false),
        }
    }
}
struct Busy(Arc<Trace>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}
#[derive(Clone)]
struct ProxyState {
    host: String,
    upstream: String,
    client: reqwest::Client,
    trace: Arc<Trace>,
}
async fn proxy(State(state): State<ProxyState>, request: Request) -> Response {
    let input = match body(request, &state.host).await {
        Ok(body) => body,
        Err(status) => return error(status),
    };
    if !state.trace.enabled.load(Ordering::Acquire) {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    }
    if state.trace.busy.swap(true, Ordering::AcqRel) {
        state.trace.unexpected.store(true, Ordering::Release);
        return error(StatusCode::TOO_MANY_REQUESTS);
    }
    let _busy = Busy(state.trace.clone());
    {
        let mut requests = state.trace.requests.lock().unwrap();
        if !requests.is_empty() {
            state.trace.unexpected.store(true, Ordering::Release);
            return error(StatusCode::TOO_MANY_REQUESTS);
        }
        requests.push(input.clone());
    }
    let response = match state.client.post(&state.upstream).json(&input).send().await {
        Ok(response) if response.status().is_success() => response,
        _ => return error(StatusCode::BAD_GATEWAY),
    };
    match bytes(response)
        .await
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).map_err(|_| RunError::Driver))
    {
        Ok(value) => Json(value).into_response(),
        Err(_) => error(StatusCode::BAD_GATEWAY),
    }
}

pub(super) struct Proxy {
    pub url: String,
    pub trace: Arc<Trace>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<std::io::Result<()>>>,
}
impl Proxy {
    pub async fn start(runtime_port: u16) -> Result<Self, RunError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| RunError::Driver)?;
        let address = listener.local_addr().map_err(|_| RunError::Driver)?;
        let trace = Arc::new(Trace::new());
        let state = ProxyState {
            host: address.to_string(),
            upstream: format!("http://127.0.0.1:{runtime_port}/v1/chat/completions"),
            client: client()?,
            trace: trace.clone(),
        };
        let app = Router::new()
            .route("/v1/chat/completions", post(proxy))
            .with_state(state);
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        Ok(Self {
            url: format!("http://{address}"),
            trace,
            stop: Some(stop),
            task: Some(task),
        })
    }
    pub async fn stop(&mut self) -> bool {
        self.trace.enabled.store(false, Ordering::Release);
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let Some(mut task) = self.task.take() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
            Ok(Ok(Ok(()))) => true,
            _ => {
                task.abort();
                let _ = task.await;
                false
            }
        }
    }
}
impl Drop for Proxy {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

pub(super) async fn exchange(
    client: reqwest::Client,
    url: String,
    key: String,
    request: Value,
    trace: Arc<Trace>,
) -> Result<Exchange, RunError> {
    if trace.unexpected.load(Ordering::Acquire) || !trace.requests.lock().unwrap().is_empty() {
        return Err(RunError::UnequalWork);
    }
    let started = Instant::now();
    let response = client
        .post(format!("{url}/v1/chat/completions"))
        .bearer_auth(key)
        .json(&request)
        .send()
        .await
        .map_err(|_| RunError::Driver)?;
    let headers_millis =
        u64::try_from(started.elapsed().as_millis()).map_err(|_| RunError::Limit)?;
    if !response.status().is_success()
        || !sse_content_type(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok()),
        )
    {
        return Err(RunError::Driver);
    }
    let bytes = bytes(response).await?;
    let completion = decode_sse(&bytes)?;
    let mut requests = trace.requests.lock().unwrap();
    if trace.unexpected.load(Ordering::Acquire) || requests.len() != 1 {
        return Err(RunError::UnequalWork);
    }
    Ok(Exchange {
        backend_request: requests.remove(0),
        completion,
        timing: Some(ExchangeTiming {
            request_millis: u64::try_from(started.elapsed().as_millis())
                .map_err(|_| RunError::Limit)?,
            headers_millis,
            response_bytes: bytes.len(),
        }),
    })
}

pub(super) fn decode_sse(bytes: &[u8]) -> Result<Value, RunError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RunError::InvalidEvidence)?;
    let frames: Vec<_> = text
        .strip_suffix("\n\n")
        .ok_or(RunError::InvalidEvidence)?
        .split("\n\n")
        .collect();
    if frames.len() != 3 || frames[2] != "data: [DONE]" {
        return Err(RunError::InvalidEvidence);
    }
    let first: Value = serde_json::from_str(
        frames[0]
            .strip_prefix("data: ")
            .ok_or(RunError::InvalidEvidence)?,
    )
    .map_err(|_| RunError::InvalidEvidence)?;
    let last: Value = serde_json::from_str(
        frames[1]
            .strip_prefix("data: ")
            .ok_or(RunError::InvalidEvidence)?,
    )
    .map_err(|_| RunError::InvalidEvidence)?;
    for field in ["id", "created", "model", "object"] {
        if first[field] != last[field] {
            return Err(RunError::InvalidEvidence);
        }
    }
    let id = first["id"]
        .as_str()
        .and_then(|s| s.strip_prefix("chatcmpl-"))
        .ok_or(RunError::InvalidEvidence)?;
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || first["created"].as_u64().is_none_or(|n| n == 0)
        || first["model"] != "pegassus"
        || first["object"] != "chat.completion.chunk"
        || first["choices"].as_array().is_none_or(|v| v.len() != 1)
        || last["choices"].as_array().is_none_or(|v| v.len() != 1)
        || first["choices"][0]["index"] != 0
        || last["choices"][0]["index"] != 0
        || !first["choices"][0]["finish_reason"].is_null()
        || last["choices"][0]["delta"] != json!({})
        || !last["usage"].is_object()
    {
        return Err(RunError::InvalidEvidence);
    }
    let mut message = first["choices"][0]["delta"].clone();
    if let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) {
        for (index, call) in calls.iter_mut().enumerate() {
            if call["index"].as_u64() != Some(index as u64) {
                return Err(RunError::InvalidEvidence);
            }
            call.as_object_mut()
                .ok_or(RunError::InvalidEvidence)?
                .remove("index");
        }
    }
    Ok(
        json!({"id":last["id"],"created":last["created"],"model":last["model"],"object":"chat.completion",
        "choices":[{"index":0,"message":message,"finish_reason":last["choices"][0]["finish_reason"]}],"usage":last["usage"],"swarm":last["swarm"]}),
    )
}

fn sse_content_type(value: Option<&str>) -> bool {
    // Python's existing buffered SSE omits charset; Rust explicitly uses UTF-8.
    matches!(
        value,
        Some("text/event-stream" | "text/event-stream; charset=utf-8")
    )
}

#[derive(Clone)]
struct FixtureState {
    host: String,
    model: String,
}
async fn fixture(State(state): State<FixtureState>, request: Request) -> Response {
    let input = match body(request, &state.host).await {
        Ok(body) => body,
        Err(status) => return error(status),
    };
    if input["model"] != state.model || input["stream"] != false || input["max_tokens"] != 256 {
        return error(StatusCode::BAD_REQUEST);
    }
    let has_tools = input["tools"]
        .as_array()
        .is_some_and(|tools| !tools.is_empty());
    let (message, finish) = if input["messages"]
        .as_array()
        .and_then(|v| v.last())
        .is_some_and(|m| m["role"] == "tool")
    {
        (
            json!({"role":"assistant","content":"MEMORY_TOOL_OK_47"}),
            "stop",
        )
    } else if has_tools {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"fixture-call","type":"function","function":{"name":"read","arguments":"{\"path\":\"fixture.txt\"}"}}]}),
            "tool_calls",
        )
    } else {
        (
            json!({"role":"assistant","content":"MEMORY_BASELINE_OK"}),
            "stop",
        )
    };
    Json(json!({"choices":[{"message":message,"finish_reason":finish}],"usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}})).into_response()
}

/// Synthetic backend binary only: no model loading or host tools.
pub async fn serve_fixture(port: u16) -> std::io::Result<()> {
    serve_fixture_model(port, "synthetic-checkpoint".into()).await
}

pub(super) fn recipe_fixture_arguments(args: &[String]) -> Option<(u16, String)> {
    let model = args.get(1)?.clone();
    let port = args
        .get(args.iter().position(|a| a == "--port")? + 1)?
        .parse()
        .ok()?;
    let recipe = crate::campaign::MlxRecipe {
        python: "/fixture/python".into(),
        runtime_entrypoint: "/fixture/entrypoint".into(),
        checkpoint: model.clone().into(),
        coordination_directory: "/fixture/coordination".into(),
        minimum_free_bytes: 1 << 30,
    };
    let expected = recipe.runtime_arguments(port).ok()?;
    if expected[1..] != *args {
        return None;
    }
    Some((port, model))
}

/// Recipe-compatible fixture only: argv validated but no checkpoint is opened.
pub async fn serve_recipe_fixture(args: Vec<String>) -> std::io::Result<()> {
    let (port, model) = recipe_fixture_arguments(&args)
        .ok_or_else(|| std::io::Error::other("invalid synthetic recipe argv"))?;
    serve_fixture_model(port, model).await
}
async fn serve_fixture_model(port: u16, model: String) -> std::io::Result<()> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let state = FixtureState {
        host: listener.local_addr()?.to_string(),
        model: model.clone(),
    };
    let app = Router::new()
        .route("/v1/chat/completions", post(fixture))
        .route(
            "/v1/models",
            get(move || {
                let model = model.clone();
                async move { Json(json!({"data":[{"id":model}]})) }
            }),
        )
        .with_state(state);
    axum::serve(listener, app).await
}

#[cfg(test)]
mod tests;
