use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use seiya_server::app_with_scheduler;
use seiya_server::inference::InferenceAdapter;
use seiya_server::runtime::{RuntimeConfig, RuntimeFuture, RuntimeTransport, TransportError};
use seiya_server::scheduler::{Scheduler, QUEUE_FULL, QUEUE_TIMEOUT};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Semaphore};
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:8012";
const TOKEN: &str = "synthetic-fixture-token-not-a-real-secret";
type RejectionCase = (Vec<(&'static str, Option<&'static str>)>, StatusCode);

fn fixtures() -> Value {
    serde_json::from_str(include_str!(
        "../../../contracts/openai-compat/v0.1.0/scenarios.json"
    ))
    .unwrap()
}
fn validation() -> Value {
    serde_json::from_str(include_str!(
        "../../../contracts/openai-compat/v0.1.0/validation.json"
    ))
    .unwrap()
}
fn input(text: &str) -> Value {
    json!({"model":"pegassus","swarm_mode":"fast","messages":[{"role":"user","content":text}]})
}
fn reply() -> Value {
    json!({"choices":[{"message":{"role":"assistant","content":"synthetic"}}]})
}

struct Fake {
    calls: Mutex<Vec<Value>>,
    replies: Mutex<VecDeque<Result<Value, TransportError>>>,
    blocked: bool,
    release: Semaphore,
    active: AtomicUsize,
    peak: AtomicUsize,
}
impl Fake {
    fn new(blocked: bool) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            replies: Mutex::new(VecDeque::new()),
            blocked,
            release: Semaphore::new(0),
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        })
    }
    fn push(&self, result: Result<Value, TransportError>) {
        self.replies.lock().unwrap().push_back(result);
    }
    fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}
struct Active<'a>(&'a AtomicUsize);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl RuntimeTransport for Fake {
    fn complete(&self, request: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(request);
            let result = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unprepared runtime call");
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            let _active = Active(&self.active);
            if self.blocked {
                self.release.acquire().await.unwrap().forget();
            }
            result
        })
    }
}

fn scheduler(fake: Arc<Fake>, wait: Duration) -> Scheduler {
    scheduler_with_cap(fake, wait, 256)
}

fn scheduler_with_cap(fake: Arc<Fake>, wait: Duration, cap: u64) -> Scheduler {
    let mut cfg=RuntimeConfig::parse(br#"{"id":"fixture-worker","model":"fixture-checkpoint","backend":"mlx","base_url":"http://127.0.0.1:65535/v1","timeout_seconds":5,"max_output_tokens":256}"#).unwrap();
    cfg.max_output_tokens = cap;
    Scheduler::new(InferenceAdapter::new(cfg, fake).unwrap(), wait).unwrap()
}

fn output_budgets() -> Value {
    serde_json::from_str(include_str!(
        "../../../contracts/openai-compat/v0.1.0/rust-output-budget.json"
    ))
    .unwrap()
}

fn chat_options() -> Value {
    serde_json::from_str(include_str!(
        "../../../contracts/openai-compat/v0.1.0/rust-chat-options.json"
    ))
    .unwrap()
}

#[tokio::test]
async fn http_chat_options_preserve_json_and_control_only_sse_usage() {
    for case in chat_options()["valid"].as_array().unwrap() {
        let fake = Fake::new(false);
        fake.push(Ok(json!({"choices":[{"message":{"role":"assistant","content":"synthetic"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}})));
        let app = router(scheduler(fake.clone(), Duration::from_secs(1)));
        let mut body = input("synthetic");
        for (key, value) in case["patch"].as_object().unwrap() {
            body[key] = value.clone();
        }
        if body["stream"] == true {
            let events = sse_response(app, body).await;
            assert!(events[0].get("usage").is_none());
            assert_eq!(
                events[1].get("usage").is_some(),
                case["sse_usage"].as_bool().unwrap()
            );
            if events[1].get("usage").is_some() {
                assert_eq!(
                    events[1]["usage"],
                    json!({"prompt_tokens":10,"completion_tokens":2,"total_tokens":12})
                );
            }
            assert_eq!(events[1]["choices"][0]["finish_reason"], "stop");
            assert!(events[1].get("swarm").is_some());
        } else {
            let (status, _, result) =
                response(app, request(&serde_json::to_vec(&body).unwrap(), &[])).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(result["usage"]["total_tokens"], 12);
        }
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["stream"], false);
        assert!(calls[0].get("store").is_none());
        assert!(calls[0].get("stream_options").is_none());
    }
}

#[tokio::test]
async fn http_unsupported_chat_options_are_json_errors_before_admission() {
    let fixture = chat_options();
    let fake = Fake::new(false);
    let scheduler = scheduler(fake.clone(), Duration::from_secs(1));
    let app = router(scheduler.clone());
    let mut cases = fixture["invalid"].as_array().unwrap().clone();
    for key in fixture["unsupported_fields"].as_array().unwrap() {
        for value in fixture["unsupported_values"].as_array().unwrap() {
            let mut patch = json!({});
            patch[key.as_str().unwrap()] = value.clone();
            cases.push(json!({"patch":patch,"error":fixture["unsupported_error"]}));
        }
    }
    for case in cases {
        let mut body = input("synthetic");
        for (key, value) in case["patch"].as_object().unwrap() {
            body[key] = value.clone();
        }
        let (status, _, result) = response(
            app.clone(),
            request(&serde_json::to_vec(&body).unwrap(), &[]),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(result, json!({"error":{"message":case["error"]}}));
        assert_eq!(scheduler.admitted_requests(), 0);
    }
    assert_eq!(fake.count(), 0);
    assert!(!scheduler.is_busy());
}

#[tokio::test]
async fn json_http_output_aliases_respect_client_and_operator_caps() {
    let fixture = output_budgets();
    for case in fixture["valid"].as_array().unwrap() {
        let fake = Fake::new(false);
        fake.push(Ok(reply()));
        let cap = case["config_cap"]
            .as_u64()
            .unwrap_or(fixture["default_config_cap"].as_u64().unwrap());
        let app = router(scheduler_with_cap(
            fake.clone(),
            Duration::from_secs(1),
            cap,
        ));
        let mut body = input("synthetic");
        for (key, value) in case["patch"].as_object().unwrap() {
            body[key] = value.clone();
        }
        let (status, _, result) =
            response(app, request(&serde_json::to_vec(&body).unwrap(), &[])).await;
        assert_eq!(status, StatusCode::OK, "{}", case["id"]);
        assert_eq!(result["choices"][0]["message"]["content"], "synthetic");
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["max_tokens"], case["effective"]);
        assert!(calls[0].get("max_completion_tokens").is_none());
    }
}

#[tokio::test]
async fn json_http_invalid_output_budgets_fail_before_admission_or_generation() {
    let fixture = output_budgets();
    let fake = Fake::new(false);
    let scheduler = scheduler(fake.clone(), Duration::from_secs(1));
    let app = router(scheduler.clone());
    for key in ["max_tokens", "max_completion_tokens"] {
        for value in fixture["invalid_values"].as_array().unwrap() {
            let mut body = input("synthetic");
            body[key] = value.clone();
            let (status, _, result) = response(
                app.clone(),
                request(&serde_json::to_vec(&body).unwrap(), &[]),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(
                result,
                json!({"error":{"message":format!("{key} must be an integer between 1 and 4096")}})
            );
        }
    }
    for patch in fixture["invalid_pairs"].as_array().unwrap() {
        let mut body = input("synthetic");
        for (key, value) in patch.as_object().unwrap() {
            body[key] = value.clone();
        }
        let (status, _, result) = response(
            app.clone(),
            request(&serde_json::to_vec(&body).unwrap(), &[]),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(result["error"]["message"], fixture["conflict_error"]);
    }
    assert_eq!(fake.count(), 0);
    assert_eq!(scheduler.admitted_requests(), 0);
    assert!(!scheduler.is_busy());
}
async fn until(condition: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
fn start(
    s: Scheduler,
    text: &str,
) -> tokio::task::JoinHandle<Result<Value, seiya_server::inference::ApiError>> {
    let body = input(text);
    tokio::spawn(async move { s.complete(body).await })
}

#[tokio::test]
async fn one_active_two_waiting_are_fifo_and_fourth_is_rejected() {
    let fake = Fake::new(true);
    for _ in 0..3 {
        fake.push(Ok(reply()));
    }
    let s = scheduler(fake.clone(), Duration::from_secs(5));
    let first = start(s.clone(), "first");
    until(|| fake.count() == 1).await;
    let second = start(s.clone(), "second");
    until(|| s.admitted_requests() == 2).await;
    let third = start(s.clone(), "third");
    until(|| s.admitted_requests() == 3).await;
    let full = s.complete(input("fourth")).await.unwrap_err();
    assert_eq!(full.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(full.message, QUEUE_FULL);
    fake.release.add_permits(1);
    first.await.unwrap().unwrap();
    until(|| fake.count() == 2).await;
    fake.release.add_permits(1);
    second.await.unwrap().unwrap();
    until(|| fake.count() == 3).await;
    fake.release.add_permits(1);
    third.await.unwrap().unwrap();
    let calls = fake.calls.lock().unwrap();
    for (i, text) in ["first", "second", "third"].iter().enumerate() {
        assert_eq!(calls[i]["messages"][0]["content"], *text);
    }
    assert_eq!(fake.peak.load(Ordering::SeqCst), 1);
    assert!(!s.is_busy());
}

#[tokio::test]
async fn queue_timeout_releases_capacity_without_another_generation() {
    let fake = Fake::new(true);
    fake.push(Ok(reply()));
    let s = scheduler(fake.clone(), Duration::from_millis(25));
    let first = start(s.clone(), "first");
    until(|| fake.count() == 1).await;
    let error = s.complete(input("waiting")).await.unwrap_err();
    assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error.message, QUEUE_TIMEOUT);
    assert_eq!(s.admitted_requests(), 1);
    assert_eq!(fake.count(), 1);
    fake.release.add_permits(1);
    first.await.unwrap().unwrap();
    fake.push(Ok(reply()));
    fake.release.add_permits(1);
    s.complete(input("recovered capacity")).await.unwrap();
    assert!(!s.is_busy());
}

#[tokio::test]
async fn cancelling_waiter_releases_slot_but_active_consumer_keeps_ownership() {
    let fake = Fake::new(true);
    fake.push(Ok(reply()));
    let s = scheduler(fake.clone(), Duration::from_secs(5));
    let active = start(s.clone(), "active");
    until(|| fake.count() == 1).await;
    let waiter = start(s.clone(), "waiting");
    until(|| s.admitted_requests() == 2).await;
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    until(|| s.admitted_requests() == 1).await;
    active.abort();
    assert!(active.await.unwrap_err().is_cancelled());
    assert!(s.is_busy());
    assert_eq!(s.admitted_requests(), 1);
    let drain = tokio::spawn({
        let s = s.clone();
        async move { s.close_and_drain().await }
    });
    until(|| s.is_closed()).await;
    assert!(!drain.is_finished());
    fake.release.add_permits(1);
    drain.await.unwrap().unwrap();
    assert_eq!(fake.count(), 1);
    assert_eq!(s.admitted_requests(), 0);
}

#[tokio::test]
async fn shutdown_wakes_waiters_and_drains_active_work() {
    let fake = Fake::new(true);
    fake.push(Ok(reply()));
    let s = scheduler(fake.clone(), Duration::from_secs(5));
    let active = start(s.clone(), "active");
    until(|| fake.count() == 1).await;
    let waiter = start(s.clone(), "waiting");
    until(|| s.admitted_requests() == 2).await;
    s.close();
    assert_eq!(
        waiter.await.unwrap().unwrap_err().status,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        s.complete(input("after close")).await.unwrap_err().status,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let drain = tokio::spawn({
        let s = s.clone();
        async move { s.close_and_drain().await }
    });
    assert!(!drain.is_finished());
    fake.release.add_permits(1);
    active.await.unwrap().unwrap();
    drain.await.unwrap().unwrap();
    assert_eq!(fake.count(), 1);
}

#[tokio::test]
async fn uncertain_runtime_fails_queued_work_and_shutdown_without_reentry() {
    let fake = Fake::new(true);
    fake.push(Err(TransportError::Timeout));
    let s = scheduler(fake.clone(), Duration::from_secs(5));
    let active = start(s.clone(), "active");
    until(|| fake.count() == 1).await;
    let waiter = start(s.clone(), "waiting");
    until(|| s.admitted_requests() == 2).await;
    fake.release.add_permits(1);
    assert_eq!(
        active.await.unwrap().unwrap_err().status,
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(
        waiter.await.unwrap().unwrap_err().status,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(s.requires_recovery());
    assert_eq!(fake.count(), 1);
    assert!(s.close_and_drain().await.is_err());
}

#[tokio::test]
async fn invalid_input_does_not_consume_admission_and_defaults_match_fixture_errors() {
    let fake = Fake::new(false);
    let s = scheduler(fake.clone(), Duration::from_secs(1));
    assert_eq!(
        s.complete(json!({})).await.unwrap_err().status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(s.admitted_requests(), 0);
    assert_eq!(fake.count(), 0);
    let cases = validation();
    // Direct scheduler callers must cross the same validation boundary as HTTP,
    // even though internal validated requests are no longer checked repeatedly.
    let mut invalid: Vec<Value> = cases["invalid_requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| {
            let mut body = case.get("body").unwrap_or(&cases["base_request"]).clone();
            if let Some(patch) = case["patch"].as_object() {
                for (key, value) in patch {
                    body[key] = value.clone();
                }
            }
            body
        })
        .collect();
    invalid.push(json!({"messages":[{"role":"user","content":"x".repeat(262144)}]}));
    for body in invalid {
        assert_eq!(
            s.complete(body).await.unwrap_err().status,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(s.admitted_requests(), 0);
        assert_eq!(fake.count(), 0);
    }
    assert_eq!(cases["queue_errors"]["full"], QUEUE_FULL);
    assert_eq!(cases["queue_errors"]["timeout"], QUEUE_TIMEOUT);
}

fn router(s: Scheduler) -> Router {
    app_with_scheduler(
        HOST.parse().unwrap(),
        Some(TOKEN.into()),
        s,
        Duration::from_secs(1),
    )
    .unwrap()
}
fn request(body: &[u8], headers: &[(&str, Option<&str>)]) -> Request<Body> {
    let mut request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header(header::HOST, HOST)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, body.len())
        .body(Body::from(body.to_vec()))
        .unwrap();
    for (name, value) in headers {
        if let Some(value) = value {
            request.headers_mut().insert(
                header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        } else {
            request.headers_mut().remove(*name);
        }
    }
    request
}
async fn response(app: Router, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/json; charset=utf-8"
    );
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(headers["x-seiya-experimental"], "chat-buffered-sse");
    if !status.is_success() {
        assert_eq!(headers[header::CONNECTION], "close");
    }
    (status, headers, serde_json::from_slice(&bytes).unwrap())
}
fn normalize(mut body: Value) -> Value {
    let id = body["id"]
        .as_str()
        .unwrap()
        .strip_prefix("chatcmpl-")
        .unwrap();
    assert_eq!(id.len(), 32);
    assert!(id
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    assert!(body["created"].as_u64().unwrap() > 0);
    let elapsed = body["swarm"]["elapsed_seconds"].as_f64().unwrap();
    assert!(elapsed.is_finite() && elapsed >= 0.0);
    body["id"] = "<completion-id>".into();
    body["created"] = "<unix-seconds>".into();
    body["swarm"]["elapsed_seconds"] = "<elapsed-seconds>".into();
    body
}

fn parse_sse(bytes: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(bytes).unwrap();
    assert!(text.ends_with("data: [DONE]\n\n"));
    let frames: Vec<_> = text.strip_suffix("\n\n").unwrap().split("\n\n").collect();
    assert_eq!(frames.len(), 3);
    let mut events = Vec::new();
    for frame in frames {
        let data = frame.strip_prefix("data: ").unwrap();
        if data == "[DONE]" {
            events.push(json!(data));
        } else {
            events.push(serde_json::from_str::<Value>(data).unwrap());
        }
    }
    assert_eq!(events[0]["id"], events[1]["id"]);
    assert_eq!(events[0]["created"], events[1]["created"]);
    assert_eq!(events[0]["model"], "pegassus");
    assert_eq!(events[0]["model"], events[1]["model"]);
    events
}

async fn sse_response(app: Router, body: Value) -> Vec<Value> {
    let response = app
        .oneshot(request(&serde_json::to_vec(&body).unwrap(), &[]))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream; charset=utf-8"
    );
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        response.headers()["x-seiya-experimental"],
        "chat-buffered-sse"
    );
    assert_eq!(response.headers()["x-accel-buffering"], "no");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    parse_sse(&bytes)
}

#[tokio::test]
async fn buffered_sse_matches_shared_text_and_tool_fixtures() {
    let mut tested = 0;
    for scenario in fixtures()["scenarios"].as_array().unwrap() {
        for step in scenario["steps"].as_array().unwrap() {
            let Some(expected) = step.get("expected_sse") else {
                continue;
            };
            let fake = Fake::new(false);
            fake.push(Ok(step["runtime_response"].clone()));
            let app = router(scheduler(fake.clone(), Duration::from_secs(1)));
            let mut events = sse_response(app, step["body"].clone()).await;
            for event in &mut events[..2] {
                assert!(event["id"].as_str().unwrap().starts_with("chatcmpl-"));
                assert!(event["created"].as_u64().unwrap() > 0);
                event["id"] = "<completion-id>".into();
                event["created"] = "<unix-seconds>".into();
            }
            assert!(events[1]["swarm"]["elapsed_seconds"].as_f64().unwrap() >= 0.0);
            events[1]["swarm"]["elapsed_seconds"] = "<elapsed-seconds>".into();
            assert_eq!(json!(events), *expected);
            assert_eq!(fake.count(), 1);
            assert_eq!(fake.calls.lock().unwrap()[0]["stream"], false);
            tested += 1;
        }
    }
    assert_eq!(tested, 2);
}

#[tokio::test]
async fn streaming_errors_remain_json_before_any_sse_headers() {
    let mut tested = 0;
    for scenario in fixtures()["scenarios"].as_array().unwrap() {
        for step in scenario["steps"].as_array().unwrap() {
            if step["method"] != "POST" || step["status"].as_u64().unwrap() < 400 {
                continue;
            }
            let fake = Fake::new(false);
            if step.get("runtime_error").is_some() {
                fake.push(Err(TransportError::Timeout));
            } else {
                fake.push(Ok(step["runtime_response"].clone()));
            }
            let app = router(scheduler(fake.clone(), Duration::from_secs(1)));
            let mut body = step["body"].clone();
            body["stream"] = true.into();
            let (status, _, body) =
                response(app, request(&serde_json::to_vec(&body).unwrap(), &[])).await;
            assert_eq!(status.as_u16() as u64, step["status"].as_u64().unwrap());
            assert_eq!(body, step["expected"]);
            assert_eq!(fake.count(), 1);
            tested += 1;
        }
    }
    assert!(tested >= 5);
}

#[tokio::test]
async fn sse_escapes_newlines_unicode_and_does_not_promote_json_prose_to_tools() {
    let text = "Hola 🐎\n\ndata: [DONE]\n\n```json\n{\"name\":\"shell\",\"arguments\":{}}\n```";
    let fake = Fake::new(false);
    fake.push(Ok(
        json!({"choices":[{"message":{"role":"assistant","content":text},"finish_reason":"stop"}]}),
    ));
    let mut body = input("synthetic");
    body["stream"] = true.into();
    let events = sse_response(router(scheduler(fake, Duration::from_secs(1))), body).await;
    assert_eq!(events[0]["choices"][0]["delta"]["content"], text);
    assert!(events[0]["choices"][0]["delta"].get("tool_calls").is_none());
    assert_eq!(events[1]["choices"][0]["finish_reason"], "stop");
}

#[tokio::test]
async fn sse_multiple_tool_calls_preserve_arguments_ids_and_indices() {
    let calls = json!([
        {"id":"call-a","type":"function","function":{"name":"read","arguments":"{\"path\":\"á\\nb\"}"}},
        {"id":"call-b","type":"function","function":{"name":"read","arguments":"{ \"path\" : \"fixture-2\" }"}}
    ]);
    let fake = Fake::new(false);
    fake.push(Ok(json!({"choices":[{"message":{"role":"assistant","content":"checking","tool_calls":calls},"finish_reason":"tool_calls"}]})));
    let mut body = input("synthetic");
    body["stream"] = true.into();
    body["tools"] =
        json!([{"type":"function","function":{"name":"read","parameters":{"type":"object"}}}]);
    let events = sse_response(router(scheduler(fake, Duration::from_secs(1))), body).await;
    let mut received = events[0]["choices"][0]["delta"]["tool_calls"].clone();
    for (index, call) in received.as_array_mut().unwrap().iter_mut().enumerate() {
        assert_eq!(call["index"], index);
        call.as_object_mut().unwrap().remove("index");
    }
    assert_eq!(received, calls);
    assert_eq!(events[1]["choices"][0]["finish_reason"], "tool_calls");
}

#[tokio::test]
async fn buffered_sse_waits_for_backend_validation_before_responding() {
    let fake = Fake::new(true);
    fake.push(Ok(reply()));
    let s = scheduler(fake.clone(), Duration::from_secs(1));
    let mut body = input("synthetic");
    body["stream"] = true.into();
    let pending = tokio::spawn(sse_response(router(s.clone()), body));
    until(|| fake.count() == 1).await;
    assert!(!pending.is_finished());
    assert!(s.is_busy());
    fake.release.add_permits(1);
    assert_eq!(pending.await.unwrap().len(), 3);
    assert!(!s.is_busy());
}

#[tokio::test]
async fn stream_guards_and_queue_errors_never_open_sse_or_invoke_extra_work() {
    let fake = Fake::new(true);
    fake.push(Ok(reply()));
    let s = scheduler(fake.clone(), Duration::from_secs(5));
    let app = router(s.clone());
    let mut body = input("synthetic");
    body["stream"] = true.into();
    let bytes = serde_json::to_vec(&body).unwrap();
    for (headers, status) in [
        (vec![("Authorization", None)], StatusCode::UNAUTHORIZED),
        (vec![("Host", Some("invalid.test"))], StatusCode::FORBIDDEN),
        (
            vec![("Origin", Some("https://invalid.test"))],
            StatusCode::FORBIDDEN,
        ),
    ] {
        assert_eq!(
            response(app.clone(), request(&bytes, &headers)).await.0,
            status
        );
    }
    assert_eq!(fake.count(), 0);
    let active = start(s.clone(), "active");
    until(|| fake.count() == 1).await;
    let second = start(s.clone(), "second");
    until(|| s.admitted_requests() == 2).await;
    let third = start(s.clone(), "third");
    until(|| s.admitted_requests() == 3).await;
    let (status, _, body) = response(app.clone(), request(&bytes, &[])).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["message"], QUEUE_FULL);
    s.close();
    assert_eq!(
        response(app, request(&bytes, &[])).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(second.await.unwrap().is_err());
    assert!(third.await.unwrap().is_err());
    fake.release.add_permits(1);
    active.await.unwrap().unwrap();
    s.close_and_drain().await.unwrap();
    assert_eq!(fake.count(), 1);
}

#[tokio::test]
async fn tcp_simulated_client_roundtrip_preserves_tool_history_and_finishes_sse() {
    let scenario = fixtures()["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scenario| scenario["steps"].as_array().unwrap().len() == 2)
        .unwrap()
        .clone();
    let fake = Fake::new(false);
    for step in scenario["steps"].as_array().unwrap() {
        fake.push(Ok(step["runtime_response"].clone()));
    }
    let s = scheduler(fake.clone(), Duration::from_secs(1));
    let (address, stop, task) = wire_server(s, Duration::from_secs(1)).await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let steps = scenario["steps"].as_array().unwrap();
    let mut first = steps[0]["body"].clone();
    first["stream"] = true.into();
    let response = client
        .post(format!("http://{address}/v1/chat/completions"))
        .json(&first)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let events = parse_sse(&response.bytes().await.unwrap());
    let mut call = events[0]["choices"][0]["delta"]["tool_calls"][0].clone();
    assert_eq!(
        call.as_object_mut().unwrap().remove("index"),
        Some(json!(0))
    );
    let mut next = steps[1]["body"].clone();
    next["stream"] = true.into();
    assert_eq!(call, next["messages"][1]["tool_calls"][0]);
    // The synthetic client supplies a fixture tool result. No tool is executed.
    let response = client
        .post(format!("http://{address}/v1/chat/completions"))
        .json(&next)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let final_events = parse_sse(&response.bytes().await.unwrap());
    assert_eq!(
        final_events[0]["choices"][0]["delta"]["content"],
        steps[1]["expected"]["choices"][0]["message"]["content"]
    );
    assert_eq!(fake.calls.lock().unwrap()[1]["messages"], next["messages"]);
    assert_eq!(fake.count(), 2);
    stop.send(()).unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn json_http_chat_and_tool_roundtrip_match_non_stream_fixtures() {
    let document = fixtures();
    let mut tested = 0;
    for scenario in document["scenarios"].as_array().unwrap() {
        let fake = Fake::new(false);
        let app = router(scheduler(fake.clone(), Duration::from_secs(1)));
        for step in scenario["steps"].as_array().unwrap() {
            if step["method"] != "POST"
                || step["body"]["stream"] == true
                || step.get("expected_sse").is_some()
            {
                continue;
            }
            if step.get("runtime_error").is_some() {
                fake.push(Err(TransportError::Timeout));
            } else {
                fake.push(Ok(step["runtime_response"].clone()));
            }
            let (status, _, mut body) = response(
                app.clone(),
                request(&serde_json::to_vec(&step["body"]).unwrap(), &[]),
            )
            .await;
            assert_eq!(
                status.as_u16() as u64,
                step["status"].as_u64().unwrap(),
                "{}",
                scenario["id"]
            );
            if status == StatusCode::OK {
                body = normalize(body);
            }
            assert_eq!(body, step["expected"], "{}", scenario["id"]);
            tested += 1;
        }
    }
    assert!(tested >= 9);
}

#[tokio::test]
async fn shared_invalid_requests_fail_before_any_http_backend_call() {
    let fake = Fake::new(false);
    let app = router(scheduler(fake.clone(), Duration::from_secs(1)));
    let data = validation();
    for case in data["invalid_requests"].as_array().unwrap() {
        let mut body = case.get("body").unwrap_or(&data["base_request"]).clone();
        if let Some(patch) = case["patch"].as_object() {
            for (key, value) in patch {
                body[key] = value.clone();
            }
        }
        let (status, _, body) = response(
            app.clone(),
            request(&serde_json::to_vec(&body).unwrap(), &[]),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{}", case["id"]);
        assert_eq!(
            body,
            json!({"error":{"message":case["error"]}}),
            "{}",
            case["id"]
        );
    }
    assert_eq!(fake.count(), 0);
}

#[tokio::test]
async fn http_framing_type_encoding_auth_and_stream_rejections_are_early() {
    let fake = Fake::new(false);
    let app = router(scheduler(fake.clone(), Duration::from_secs(1)));
    let bytes = serde_json::to_vec(&input("test")).unwrap();
    let cases: Vec<RejectionCase> = vec![
        (vec![("Content-Length", None)], StatusCode::BAD_REQUEST),
        (vec![("Content-Length", Some("0"))], StatusCode::BAD_REQUEST),
        (
            vec![("Content-Length", Some("262145"))],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![("Content-Length", Some("+2"))],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![("Transfer-Encoding", Some("chunked"))],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![("Content-Type", None)],
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            vec![("Content-Type", Some("text/plain"))],
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            vec![("Content-Type", Some("application/json; charset=latin1"))],
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            vec![("Content-Encoding", Some("gzip"))],
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            vec![
                ("Authorization", None),
                ("Content-Type", Some("text/plain")),
            ],
            StatusCode::UNAUTHORIZED,
        ),
        (
            vec![("Origin", Some("https://untrusted.invalid"))],
            StatusCode::FORBIDDEN,
        ),
        (
            vec![("Host", Some("untrusted.invalid"))],
            StatusCode::FORBIDDEN,
        ),
    ];
    for (headers, expected) in cases {
        assert_eq!(
            response(app.clone(), request(&bytes, &headers)).await.0,
            expected
        );
    }
    for bytes in [b"{".as_slice(), b"\xff".as_slice()] {
        assert_eq!(
            response(app.clone(), request(bytes, &[])).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let mut duplicate = request(&bytes, &[]);
    duplicate
        .headers_mut()
        .append(header::CONTENT_LENGTH, "2".parse().unwrap());
    assert_eq!(
        response(app.clone(), duplicate).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(fake.count(), 0);
}

#[tokio::test]
async fn exact_http_body_limit_is_accepted_and_health_reports_quarantine() {
    let fake = Fake::new(false);
    fake.push(Err(TransportError::Timeout));
    let s = scheduler(fake.clone(), Duration::from_secs(1));
    let app = router(s.clone());
    let mut bytes = serde_json::to_vec(&input("test")).unwrap();
    bytes.resize(262144, b' ');
    assert_eq!(
        response(
            app.clone(),
            request(
                &bytes,
                &[("Content-Type", Some("application/json; charset=utf-8"))]
            )
        )
        .await
        .0,
        StatusCode::BAD_GATEWAY
    );
    let health = Request::builder()
        .uri("/health")
        .header(header::HOST, HOST)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (_, _, body) = response(app.clone(), health).await;
    assert_eq!(body["busy"], true);
    assert_eq!(body["backend_readiness"], "unconfirmed after runtime error");
    assert_eq!(
        response(
            app.clone(),
            request(&serde_json::to_vec(&input("next")).unwrap(), &[])
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(fake.count(), 1);
}

#[tokio::test]
async fn concurrent_http_requests_wait_then_recover_with_no_parallel_backend() {
    let fake = Fake::new(true);
    for _ in 0..3 {
        fake.push(Ok(reply()));
    }
    let s = scheduler(fake.clone(), Duration::from_secs(5));
    let app = router(s.clone());
    let submit = |text: &str| {
        let app = app.clone();
        let bytes = serde_json::to_vec(&input(text)).unwrap();
        tokio::spawn(async move { response(app, request(&bytes, &[])).await })
    };
    let first = submit("first");
    until(|| fake.count() == 1).await;
    let second = submit("second");
    until(|| s.admitted_requests() == 2).await;
    let third = submit("third");
    until(|| s.admitted_requests() == 3).await;
    let (status, _, body) = response(
        app.clone(),
        request(&serde_json::to_vec(&input("fourth")).unwrap(), &[]),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["message"], QUEUE_FULL);
    let health = || {
        Request::builder()
            .uri("/health")
            .header(header::HOST, HOST)
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(response(app.clone(), health()).await.2["busy"], true);
    fake.release.add_permits(1);
    assert_eq!(first.await.unwrap().0, StatusCode::OK);
    until(|| fake.count() == 2).await;
    fake.release.add_permits(1);
    assert_eq!(second.await.unwrap().0, StatusCode::OK);
    until(|| fake.count() == 3).await;
    fake.release.add_permits(1);
    assert_eq!(third.await.unwrap().0, StatusCode::OK);
    assert_eq!(fake.peak.load(Ordering::SeqCst), 1);
    assert_eq!(response(app, health()).await.2["busy"], false);
}

async fn wire_server(
    s: Scheduler,
    timeout: Duration,
) -> (
    std::net::SocketAddr,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = app_with_scheduler(address, None, s, timeout).unwrap();
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    (address, stop, task)
}

#[tokio::test]
async fn slow_body_times_out_without_generation_or_leaked_reader() {
    let fake = Fake::new(false);
    let s = scheduler(fake.clone(), Duration::from_secs(1));
    let (address, stop, task) = wire_server(s, Duration::from_millis(25)).await;
    let mut socket = TcpStream::connect(address).await.unwrap();
    socket.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&bytes).starts_with("HTTP/1.1 408 Request Timeout"));
    assert_eq!(fake.count(), 0);
    fake.push(Ok(reply()));
    let result = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://{address}/v1/chat/completions"))
        .json(&input("after read timeout"))
        .send()
        .await
        .unwrap();
    assert_eq!(result.status(), reqwest::StatusCode::OK);
    let _ = result.bytes().await.unwrap();
    assert_eq!(fake.count(), 1);
    stop.send(()).unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn eight_stalled_uploads_bound_readers_and_ninth_is_rejected() {
    let fake = Fake::new(false);
    let s = scheduler(fake.clone(), Duration::from_secs(1));
    let (address, stop, task) = wire_server(s, Duration::from_secs(2)).await;
    let mut uploads = Vec::new();
    for _ in 0..8 {
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: 100\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut interim = [0u8; 25];
        tokio::time::timeout(Duration::from_secs(1), socket.read_exact(&mut interim))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
        uploads.push(socket);
    }
    let mut ninth = TcpStream::connect(address).await.unwrap();
    ninth.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}").as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), ninth.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&bytes).starts_with("HTTP/1.1 429 Too Many Requests"));
    assert_eq!(fake.count(), 0);
    drop(uploads);
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
}
