//! No model, host probes or external network. In-process mocks, except the
//! explicitly owned ephemeral TCP fixtures testing transport/BoundServer wiring.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use seiya_server::{
    app_with_scheduler,
    config::Config,
    inference::InferenceAdapter,
    runtime::{HttpTransport, RuntimeConfig, RuntimeFuture, RuntimeTransport, TransportError},
    scheduler::Scheduler,
    BoundServer,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tokio::{
    net::TcpListener,
    sync::{oneshot, Semaphore},
};
use tower::ServiceExt;

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../contracts/openai-compat/v0.1.0/rust-runtime-status.json"
    ))
    .unwrap()
}
fn cfg() -> RuntimeConfig {
    RuntimeConfig::parse(br#"{"id":"fixture","model":"synthetic","backend":"mlx","base_url":"http://127.0.0.1:65535/v1","timeout_seconds":1,"max_output_tokens":32}"#).unwrap()
}
fn input() -> Value {
    json!({"messages":[{"role":"user","content":"synthetic"}]})
}
fn reply() -> Value {
    json!({"choices":[{"message":{"role":"assistant","content":"synthetic"},"finish_reason":"stop"}]})
}
struct Fake {
    result: Mutex<Result<Value, TransportError>>,
    calls: AtomicUsize,
    blocked: bool,
    entered: Semaphore,
    release: Semaphore,
}
impl Fake {
    fn new(result: Result<Value, TransportError>, blocked: bool) -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(result),
            calls: AtomicUsize::new(0),
            blocked,
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        })
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}
impl RuntimeTransport for Fake {
    fn complete(&self, _: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.add_permits(1);
            if self.blocked {
                self.release.acquire().await.unwrap().forget();
            }
            self.result.lock().unwrap().clone()
        })
    }
}
fn scheduler(fake: Arc<Fake>, enabled: bool) -> Scheduler {
    let adapter = if enabled {
        InferenceAdapter::new_with_runtime_status(cfg(), fake)
    } else {
        InferenceAdapter::new(cfg(), fake)
    }
    .unwrap();
    Scheduler::new(adapter, Duration::from_secs(1)).unwrap()
}
fn router(scheduler: Scheduler) -> Router {
    app_with_scheduler(
        "127.0.0.1:9998".parse().unwrap(),
        None,
        scheduler,
        Duration::from_secs(1),
    )
    .unwrap()
}
async fn http(app: Router, body: Option<Value>) -> (StatusCode, Value) {
    let mut request = Request::builder().header("host", "127.0.0.1:9998");
    let request = if let Some(body) = body {
        let bytes = serde_json::to_vec(&body).unwrap();
        request = request
            .method("POST")
            .uri("/v1/chat/completions")
            .header("content-type", "application/json")
            .header("content-length", bytes.len());
        request.body(Body::from(bytes)).unwrap()
    } else {
        request.uri("/health").body(Body::empty()).unwrap()
    };
    let result = app.oneshot(request).await.unwrap();
    assert!(!result.headers().contains_key("retry-after"));
    let status = result.status();
    let bytes = axum::body::to_bytes(result.into_body(), 65536)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[test]
fn opt_in_is_strict_and_requires_runtime_without_changing_default() {
    assert!(
        !Config::parse(br#"{"listen":"127.0.0.1:0"}"#)
            .unwrap()
            .runtime_status
    );
    assert!(Config::parse(br#"{"listen":"127.0.0.1:0","runtime_status":true}"#).is_err());
    for value in [json!("true"), Value::Null, json!(1)] {
        let config = json!({"listen":"127.0.0.1:0","runtime_status":value});
        assert!(Config::parse(&serde_json::to_vec(&config).unwrap()).is_err());
    }
}

#[tokio::test]
async fn bridge_model_binding_is_opt_in_exact_and_checked_before_generation() {
    let example: Value = serde_json::from_str(include_str!(
        "../../../contracts/openai-compat/v0.1.0/runtime-bridge.json"
    ))
    .unwrap();
    assert!(Config::parse(&serde_json::to_vec(&example["rust_config"]).unwrap()).is_ok());
    let fake = Fake::new(Ok(reply()), false);
    let app = router(scheduler(fake.clone(), true));
    let binding: String = b"synthetic".iter().map(|b| format!("{b:02x}")).collect();
    let bytes = serde_json::to_vec(&input()).unwrap();
    for (supplied, expected) in [("00", StatusCode::BAD_REQUEST), (&binding, StatusCode::OK)] {
        let request = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("host", "127.0.0.1:9998")
            .header("content-type", "application/json")
            .header("content-length", bytes.len())
            .header("x-seiya-model-binding", supplied)
            .body(Body::from(bytes.clone()))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            expected
        );
    }
    assert_eq!(fake.calls(), 1);
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("host", "untrusted.invalid")
        .header("x-seiya-model-binding", "00")
        .body(Body::from("not-json"))
        .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    for enabled in [false, true] {
        let fake = Fake::new(Ok(reply()), false);
        let app = router(scheduler(fake.clone(), enabled));
        let request = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("host", "127.0.0.1:9998")
            .header("x-seiya-model-binding", &binding)
            .header("x-seiya-model-binding", &binding)
            .body(Body::from("not-json"))
            .unwrap();
        assert_eq!(
            app.oneshot(request).await.unwrap().status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(fake.calls(), 0);
    }
    let fake = Fake::new(Ok(reply()), false);
    let app = router(scheduler(fake.clone(), false));
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("host", "127.0.0.1:9998")
        .header("x-seiya-model-binding", &binding)
        .body(Body::from("not-json"))
        .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(fake.calls(), 0);
}

#[tokio::test]
async fn cached_health_does_not_probe_or_claim_model_residency_or_memory_admission() {
    let fake = Fake::new(Ok(reply()), false);
    let app = router(scheduler(fake.clone(), true));
    let (status, body) = http(app.clone(), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["runtime_status"], fixture()["initial"]);
    let bridge_fixture: Value = serde_json::from_str(include_str!(
        "../../../contracts/openai-compat/v0.1.0/runtime-bridge.json"
    ))
    .unwrap();
    assert_eq!(body["runtime_bridge"], bridge_fixture["handshake"]);
    assert_eq!(fake.calls(), 0);
    assert_eq!(http(app.clone(), Some(input())).await.0, StatusCode::OK);
    assert_eq!(
        http(app.clone(), None).await.1["runtime_status"],
        fixture()["completed"]
    );
    assert_eq!(fake.calls(), 1);
    assert_eq!(http(app, Some(input())).await.0, StatusCode::OK);
    assert_eq!(fake.calls(), 2);
}

#[tokio::test]
async fn not_connected_is_clear_json_and_blocks_repeated_attempts_without_dispatch() {
    let fake = Fake::new(Err(TransportError::NotConnected), false);
    let s = scheduler(fake.clone(), true);
    let app = router(s.clone());
    for _ in 0..3 {
        let (status, body) = http(app.clone(), Some(input())).await;
        assert_eq!(status.as_u16(), fixture()["not_connected"]["http_status"]);
        assert_eq!(body, fixture()["not_connected"]["body"]);
    }
    assert_eq!(fake.calls(), 1);
    assert!(!s.is_busy());
    assert!(s.requires_recovery());
    let health = http(app, None).await.1;
    assert_eq!(health["runtime_status"]["admission"], "blocked");
    assert_eq!(
        health["runtime_status"]["endpoint_observation"],
        "connection_not_established"
    );
    assert!(!health["backend_readiness"]
        .as_str()
        .unwrap()
        .contains("pressure"));
    s.close_and_drain().await.unwrap();
    assert_eq!(s.runtime_status().unwrap()["admission"], "closed");
}

#[tokio::test]
async fn legacy_unavailable_errors_and_health_shape_remain_unchanged() {
    for error in [TransportError::NotConnected, TransportError::Unavailable] {
        let fake = Fake::new(Err(error), false);
        let s = scheduler(fake.clone(), false);
        let app = router(s.clone());
        assert!(http(app.clone(), None)
            .await
            .1
            .get("runtime_status")
            .is_none());
        let (status, body) = http(app, Some(input())).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(
            body,
            json!({"error":{"message":"local model endpoint failed: URLError"}})
        );
        assert!(s.requires_recovery());
        assert!(s.is_busy());
        assert_eq!(fake.calls(), 1);
    }
}

#[tokio::test]
async fn ambiguous_transport_failures_stay_quarantined_and_are_not_claimed_offline() {
    for error in [
        TransportError::Timeout,
        TransportError::Unavailable,
        TransportError::InvalidJson,
        TransportError::HttpFailure,
        TransportError::ResponseTooLarge,
    ] {
        let fake = Fake::new(Err(error), false);
        let s = scheduler(fake.clone(), true);
        let first = s.complete(input()).await.unwrap_err();
        assert_eq!(first.status, StatusCode::BAD_GATEWAY);
        assert_eq!(first.code, Some("runtime_state_unconfirmed"));
        let next = s.complete(input()).await.unwrap_err();
        assert_eq!(next.body(), fixture()["unconfirmed"]["body"]);
        let status = s.runtime_status().unwrap();
        assert_eq!(status["endpoint_observation"], "unconfirmed");
        assert!(status["active_requests"].is_null());
        assert!(s.close_and_drain().await.is_err());
        assert_eq!(fake.calls(), 1);
    }
}

#[tokio::test]
async fn consumer_disconnect_and_waiting_requests_never_repeat_an_undispatched_failure() {
    let fake = Fake::new(Err(TransportError::NotConnected), true);
    let s = scheduler(fake.clone(), true);
    let run = s.clone();
    let consumer = tokio::spawn(async move { run.complete(input()).await });
    fake.entered.acquire().await.unwrap().forget();
    assert_eq!(s.runtime_status().unwrap()["active_requests"], 1);
    consumer.abort();
    let _ = consumer.await;
    assert!(s.is_busy());
    let run = s.clone();
    let waiter = tokio::spawn(async move { run.complete(input()).await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while s.admitted_requests() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fake.release.add_permits(1);
    assert_eq!(
        waiter.await.unwrap().unwrap_err().body(),
        fixture()["not_connected"]["body"]
    );
    assert_eq!(fake.calls(), 1);
    assert_eq!(s.admitted_requests(), 0);
    s.close_and_drain().await.unwrap();
}

#[tokio::test]
async fn outer_deadline_does_not_free_a_model_or_retry_uncertain_work() {
    let fake = Fake::new(Ok(reply()), true);
    let s = scheduler(fake.clone(), true);
    let error = s.complete(input()).await.unwrap_err();
    assert_eq!(error.code, Some("runtime_state_unconfirmed"));
    assert!(s.runtime_status().unwrap()["active_requests"].is_null());
    assert_eq!(
        s.complete(input()).await.unwrap_err().body(),
        fixture()["unconfirmed"]["body"]
    );
    assert_eq!(fake.calls(), 1);
    assert!(s.close_and_drain().await.is_err());
}

#[tokio::test]
async fn completed_but_semantically_invalid_reply_is_not_claimed_as_loaded_model() {
    let fake = Fake::new(Ok(json!({})), false);
    let s = scheduler(fake, true);
    assert_eq!(
        s.complete(input()).await.unwrap_err().status,
        StatusCode::BAD_GATEWAY
    );
    assert!(!s.requires_recovery());
    assert_eq!(s.runtime_status().unwrap(), fixture()["completed"]);
}

#[tokio::test]
async fn owned_closed_port_is_classified_before_dispatch() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = cfg();
    config.base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    drop(listener);
    let transport = HttpTransport::new(&config).unwrap();
    assert_eq!(
        transport.complete(input()).await.unwrap_err(),
        TransportError::NotConnected
    );
}

#[tokio::test]
async fn response_body_disconnect_is_not_misclassified_as_connection_failure() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = cfg();
    config.base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let backend = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 4096];
        let _ = stream.read(&mut buffer).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{}")
            .await
            .unwrap();
    });
    let adapter = InferenceAdapter::new_with_runtime_status(
        config.clone(),
        Arc::new(HttpTransport::new(&config).unwrap()),
    )
    .unwrap();
    let failure = adapter.complete(input()).await.unwrap_err();
    assert_eq!(failure.status, StatusCode::BAD_GATEWAY);
    assert_eq!(failure.code, Some("runtime_state_unconfirmed"));
    assert_eq!(
        adapter.runtime_status().unwrap()["endpoint_observation"],
        "unconfirmed"
    );
    backend.await.unwrap();
}

#[tokio::test]
async fn status_opt_in_does_not_bypass_http_guards_or_accept_client_status_fields() {
    let fake = Fake::new(Ok(reply()), false);
    let app = router(scheduler(fake.clone(), true));
    let request = Request::builder()
        .uri("/health")
        .header("host", "untrusted.invalid")
        .body(Body::empty())
        .unwrap();
    let result = app.clone().oneshot(request).await.unwrap();
    assert_eq!(result.status(), StatusCode::FORBIDDEN);
    let bytes = axum::body::to_bytes(result.into_body(), 65536)
        .await
        .unwrap();
    assert!(serde_json::from_slice::<Value>(&bytes)
        .unwrap()
        .get("runtime_status")
        .is_none());
    let mut body = input();
    body["runtime_status"] = json!(true);
    assert_eq!(http(app, Some(body)).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(fake.calls(), 0);
}

#[tokio::test]
async fn bound_server_opt_in_is_wired_and_shutdown_does_not_claim_runtime_cleanup() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let runtime_url = format!("http://{}/v1", listener.local_addr().unwrap());
    drop(listener);
    let config = Config::parse(
        &serde_json::to_vec(&json!({"listen":"127.0.0.1:0",
        "runtime_status":true,"runtime":{"id":"fake","model":"synthetic","backend":"mlx",
        "base_url":runtime_url,"timeout_seconds":1,"max_output_tokens":32}}))
        .unwrap(),
    )
    .unwrap();
    let server = BoundServer::bind(&config).await.unwrap();
    let address = server.local_addr();
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(server.serve(async {
        let _ = stopped.await;
    }));
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let health: Value = client
        .get(format!("http://{address}/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["runtime_status"], fixture()["initial"]);
    let result = client
        .post(format!("http://{address}/v1/chat/completions"))
        .json(&input())
        .send()
        .await
        .unwrap();
    assert_eq!(result.status().as_u16(), 503);
    assert_eq!(
        result.json::<Value>().await.unwrap(),
        fixture()["not_connected"]["body"]
    );
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    // The frontend drain confirms no dispatched job, not any process shutdown.
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}
