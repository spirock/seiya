//! End-to-end TCP only against owned synthetic servers. No model or host tools.
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use seiya_server::{config::Config, BoundServer};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../contracts/openai-compat/v0.1.0/rust-opencode-profile.json"
    ))
    .unwrap()
}

#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<Value>>,
    replies: Mutex<VecDeque<(StatusCode, Value)>>,
}

async fn complete(
    State(state): State<Arc<Backend>>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    state.calls.lock().unwrap().push(body);
    let (status, reply) = state
        .replies
        .lock()
        .unwrap()
        .pop_front()
        .expect("unexpected backend call");
    (status, Json(reply))
}

struct Pair {
    backend: Arc<Backend>,
    url: String,
    client: reqwest::Client,
    frontend_stop: oneshot::Sender<()>,
    frontend: JoinHandle<std::io::Result<()>>,
    backend_stop: oneshot::Sender<()>,
    backend_task: JoinHandle<()>,
}

impl Pair {
    async fn start(cap: u64, replies: Vec<(StatusCode, Value)>) -> Self {
        let backend = Arc::new(Backend::default());
        backend.replies.lock().unwrap().extend(replies);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/v1/chat/completions", post(complete))
            .with_state(backend.clone());
        let (backend_stop, stopped) = oneshot::channel();
        let backend_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let config = Config::parse(&serde_json::to_vec(&json!({"listen":"127.0.0.1:0",
            "runtime":{"id":"fixture-worker","backend":"mlx","model":"fixture-checkpoint",
                "base_url":format!("http://{address}/v1"),"timeout_seconds":5,"max_output_tokens":cap}})).unwrap()).unwrap();
        let server = BoundServer::bind(&config).await.unwrap();
        let url = format!("http://{}", server.local_addr());
        let (frontend_stop, stopped) = oneshot::channel();
        let frontend = tokio::spawn(server.serve(async {
            let _ = stopped.await;
        }));
        Self {
            backend,
            url,
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            frontend_stop,
            frontend,
            backend_stop,
            backend_task,
        }
    }

    async fn send(&self, body: &Value) -> reqwest::Response {
        self.client
            .post(format!("{}/v1/chat/completions", self.url))
            .json(body)
            .send()
            .await
            .unwrap()
    }

    async fn finish(self, uncertain: bool) {
        self.frontend_stop.send(()).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(3), self.frontend)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.is_err(), uncertain);
        self.backend_stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), self.backend_task)
            .await
            .unwrap()
            .unwrap();
        assert!(self.backend.replies.lock().unwrap().is_empty());
    }
}

fn reply(message: Value, reason: &str) -> (StatusCode, Value) {
    (
        StatusCode::OK,
        json!({"choices":[{"message":message,"finish_reason":reason}],"usage":fixture()["usage"]}),
    )
}

async fn decoded(response: reqwest::Response, stream: bool, include_usage: bool) -> Value {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        response.headers()["x-seiya-experimental"],
        "chat-buffered-sse"
    );
    let mut completion;
    if stream {
        assert_eq!(
            response.headers()["content-type"],
            "text/event-stream; charset=utf-8"
        );
        let text = response.text().await.unwrap();
        let frames: Vec<_> = text.strip_suffix("\n\n").unwrap().split("\n\n").collect();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[2], "data: [DONE]");
        let first: Value = serde_json::from_str(frames[0].strip_prefix("data: ").unwrap()).unwrap();
        let last: Value = serde_json::from_str(frames[1].strip_prefix("data: ").unwrap()).unwrap();
        for key in ["id", "created", "model", "object"] {
            assert_eq!(first[key], last[key]);
        }
        assert_eq!(first["object"], "chat.completion.chunk");
        assert_eq!(first["choices"][0]["finish_reason"], Value::Null);
        assert_eq!(last["choices"][0]["delta"], json!({}));
        assert!(first.get("usage").is_none());
        assert_eq!(last.get("usage").is_some(), include_usage);
        let mut message = first["choices"][0]["delta"].clone();
        if let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) {
            for (index, call) in calls.iter_mut().enumerate() {
                assert_eq!(call["index"], index);
                call.as_object_mut().unwrap().remove("index");
            }
        }
        completion = last;
        completion["choices"][0]["message"] = message;
    } else {
        assert_eq!(
            response.headers()["content-type"],
            "application/json; charset=utf-8"
        );
        completion = response.json::<Value>().await.unwrap();
        assert_eq!(completion["object"], "chat.completion");
    }
    assert_eq!(completion["model"], "pegassus");
    let id = completion["id"]
        .as_str()
        .unwrap()
        .strip_prefix("chatcmpl-")
        .unwrap();
    assert_eq!(id.len(), 32);
    assert!(id
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    assert!(completion["created"].as_u64().unwrap() > 0);
    assert_eq!(completion["swarm"]["selected_model"], "fixture-worker");
    assert!(completion["swarm"]["elapsed_seconds"].as_f64().unwrap() >= 0.0);
    if !stream || include_usage {
        assert_eq!(completion["usage"], fixture()["usage"]);
    }
    completion
}

fn assert_forwarded(call: &Value, body: &Value, budget: u64) {
    assert_eq!(
        call,
        &json!({"model":"fixture-checkpoint","messages":body["messages"],
        "tools":body["tools"],"stream":false,"max_tokens":budget,"temperature":0,
        "chat_template_kwargs":{"enable_thinking":false}})
    );
}

#[tokio::test]
async fn observed_profile_shape_roundtrip_preserves_tools_usage_and_effective_budget_over_tcp() {
    let fixture = fixture();
    for (stream, usage) in [(false, true), (true, true), (true, false)] {
        for (requested, cap) in [(32, 256), (2048, 64)] {
            let tool_message =
                json!({"role":"assistant","content":null,"tool_calls":[fixture["call"]]});
            let final_message = json!({"role":"assistant","content":fixture["tool_result"]});
            let pair = Pair::start(
                cap,
                vec![
                    reply(tool_message.clone(), "tool_calls"),
                    reply(final_message.clone(), "stop"),
                ],
            )
            .await;
            let mut body = fixture["body"].clone();
            body["stream"] = stream.into();
            body["max_completion_tokens"] = requested.into();
            if stream {
                body["stream_options"]["include_usage"] = usage.into();
            } else {
                body.as_object_mut().unwrap().remove("stream_options");
            }
            let first = decoded(pair.send(&body).await, stream, usage).await;
            assert_eq!(first["choices"][0]["message"], tool_message);
            assert_eq!(first["choices"][0]["finish_reason"], "tool_calls");
            assert_eq!(first["swarm"]["mode"], "tools");
            let mut next = body.clone();
            next["messages"]
                .as_array_mut()
                .unwrap()
                .push(first["choices"][0]["message"].clone());
            // A simulated client supplies data. No read/shell/edit tool is executed.
            next["messages"]
                .as_array_mut()
                .unwrap()
                .push(json!({"role":"tool",
                "tool_call_id":fixture["call"]["id"],"content":fixture["tool_result"]}));
            let final_reply = decoded(pair.send(&next).await, stream, usage).await;
            assert_eq!(final_reply["choices"][0]["message"], final_message);
            assert_eq!(final_reply["choices"][0]["finish_reason"], "stop");
            let calls = pair.backend.calls.lock().unwrap().clone();
            assert_eq!(calls.len(), 2);
            assert_forwarded(&calls[0], &body, requested.min(cap));
            assert_forwarded(&calls[1], &next, requested.min(cap));
            assert_eq!(next["messages"][1]["tool_calls"][0], fixture["call"]);
            pair.finish(false).await;
        }
    }
}

#[tokio::test]
async fn combined_profile_truncation_and_invalid_backend_responses_never_become_sse_success() {
    let cases = [
        (
            json!({"choices":[{"message":{"role":"assistant","content":"partial"},"finish_reason":"length"}]}),
            "fixture-worker: output truncated; increase output budget",
        ),
        (
            json!({"choices":[]}),
            "fixture-worker: malformed model response",
        ),
        (
            json!({"choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":{"prompt_tokens":false}}),
            "invalid token usage object",
        ),
        (
            json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"bad","type":"function","function":{"name":"not_offered","arguments":"{}"}}]}}]}),
            "invalid structured tool call; nothing executed",
        ),
    ];
    for (stream, usage) in [(false, true), (true, true), (true, false)] {
        for (backend_reply, error) in &cases {
            let pair = Pair::start(
                64,
                vec![
                    (StatusCode::OK, backend_reply.clone()),
                    reply(
                        json!({"role":"assistant","content":"capacity recovered"}),
                        "stop",
                    ),
                ],
            )
            .await;
            let mut body = fixture()["body"].clone();
            body["stream"] = stream.into();
            if stream {
                body["stream_options"]["include_usage"] = usage.into();
            } else {
                body.as_object_mut().unwrap().remove("stream_options");
            }
            let response = pair.send(&body).await;
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
            assert_eq!(
                response.headers()["content-type"],
                "application/json; charset=utf-8"
            );
            assert_eq!(
                response.json::<Value>().await.unwrap(),
                json!({"error":{"message":error}})
            );
            let recovered = decoded(pair.send(&body).await, stream, usage).await;
            assert_eq!(
                recovered["choices"][0]["message"]["content"],
                "capacity recovered"
            );
            for call in pair.backend.calls.lock().unwrap().iter() {
                assert_forwarded(call, &body, 64);
            }
            pair.finish(false).await;
        }
    }
}

#[tokio::test]
async fn combined_invalid_options_and_budgets_never_reach_http_backend() {
    let pair = Pair::start(64, vec![]).await;
    for patch in [
        json!({"max_completion_tokens":null}),
        json!({"max_tokens":32,"max_completion_tokens":64}),
        json!({"store":true}),
        json!({"stream_options":{"include_usage":null}}),
        json!({"stop":"private"}),
    ] {
        let mut body = fixture()["body"].clone();
        for (key, value) in patch.as_object().unwrap() {
            body[key] = value.clone();
        }
        let response = pair.send(&body).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers()["content-type"],
            "application/json; charset=utf-8"
        );
        let text = response.text().await.unwrap();
        assert!(serde_json::from_str::<Value>(&text).unwrap()["error"]["message"].is_string());
        assert!(!text.contains("private"));
    }
    assert!(pair.backend.calls.lock().unwrap().is_empty());
    pair.finish(false).await;
}

#[tokio::test]
async fn combined_profile_transport_failure_quarantines_without_retry_or_fake_done() {
    let pair = Pair::start(
        64,
        vec![(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"private":"backend-detail"}),
        )],
    )
    .await;
    let body = fixture()["body"].clone();
    for expected in [StatusCode::BAD_GATEWAY, StatusCode::SERVICE_UNAVAILABLE] {
        let response = pair.send(&body).await;
        assert_eq!(response.status(), expected);
        assert_eq!(
            response.headers()["content-type"],
            "application/json; charset=utf-8"
        );
        let text = response.text().await.unwrap();
        assert!(!text.contains("backend-detail") && !text.contains("[DONE]"));
        assert!(serde_json::from_str::<Value>(&text).unwrap()["error"]["message"].is_string());
    }
    assert_eq!(pair.backend.calls.lock().unwrap().len(), 1);
    pair.finish(true).await;
}
