use seiya_server::inference::InferenceAdapter;
use seiya_server::runtime::{RuntimeConfig, RuntimeFuture, RuntimeTransport, TransportError};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

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
async fn accepted_chat_options_are_not_forwarded_as_backend_capabilities() {
    for case in chat_options()["valid"].as_array().unwrap() {
        let runtime = Arc::new(Scripted::default());
        runtime.replies.lock().unwrap().push_back(Ok(
            json!({"choices":[{"message":{"role":"assistant","content":"synthetic"}}]}),
        ));
        let adapter = InferenceAdapter::new(config(), runtime.clone()).unwrap();
        let mut body = json!({"messages":[{"role":"user","content":"synthetic"}]});
        for (key, value) in case["patch"].as_object().unwrap() {
            body[key] = value.clone();
        }
        let result = adapter.complete(body.clone()).await.unwrap();
        assert!(result.get("usage").is_some()); // Adapter result is always complete JSON.
        let calls = runtime.calls.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            &[
                json!({"model":"fixture-checkpoint","messages":body["messages"],
            "stream":false,"max_tokens":256,"temperature":0,"chat_template_kwargs":{"enable_thinking":false}})
            ]
        );
    }
}

#[tokio::test]
async fn unsupported_options_fail_without_runtime_or_sensitive_error_echo() {
    let fixture = chat_options();
    let runtime = Arc::new(Scripted::default());
    let adapter = InferenceAdapter::new(config(), runtime.clone()).unwrap();
    let mut cases = fixture["invalid"].as_array().unwrap().clone();
    for key in fixture["unsupported_fields"].as_array().unwrap() {
        for value in fixture["unsupported_values"].as_array().unwrap() {
            let mut patch = json!({});
            patch[key.as_str().unwrap()] = value.clone();
            cases.push(json!({"patch":patch,"error":fixture["unsupported_error"]}));
        }
    }
    for case in cases {
        let mut body = json!({"messages":[{"role":"user","content":"synthetic"}]});
        for (key, value) in case["patch"].as_object().unwrap() {
            body[key] = value.clone();
        }
        let error = adapter.complete(body).await.unwrap_err();
        assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(error.message, case["error"].as_str().unwrap());
        assert!(!error.message.contains("private"));
        assert!(!adapter.is_busy());
    }
    assert!(runtime.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn output_budget_aliases_forward_only_the_effective_backend_max_tokens() {
    let fixtures = output_budgets();
    for case in fixtures["valid"].as_array().unwrap() {
        let runtime = Arc::new(Scripted::default());
        runtime.replies.lock().unwrap().push_back(Ok(
            json!({"choices":[{"message":{"role":"assistant","content":"synthetic"}}]}),
        ));
        let mut cfg = config();
        cfg.max_output_tokens = case["config_cap"]
            .as_u64()
            .unwrap_or(fixtures["default_config_cap"].as_u64().unwrap());
        let adapter = InferenceAdapter::new(cfg, runtime.clone()).unwrap();
        let mut body = json!({"messages":[{"role":"user","content":"synthetic"}]});
        for (key, value) in case["patch"].as_object().unwrap() {
            body[key] = value.clone();
        }
        adapter.complete(body).await.unwrap();
        let calls = runtime.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["max_tokens"], case["effective"], "{}", case["id"]);
        assert!(calls[0].get("max_completion_tokens").is_none());
        assert!(!adapter.is_busy());
    }
}

#[tokio::test]
async fn invalid_or_conflicting_output_aliases_do_not_acquire_runtime() {
    let fixture = output_budgets();
    let runtime = Arc::new(Scripted::default());
    let adapter = InferenceAdapter::new(config(), runtime.clone()).unwrap();
    for (key, other) in [
        ("max_tokens", "max_completion_tokens"),
        ("max_completion_tokens", "max_tokens"),
    ] {
        for value in fixture["invalid_values"].as_array().unwrap() {
            for include_other in [false, true] {
                let mut body = json!({"messages":[{"role":"user","content":"synthetic"}]});
                body[key] = value.clone();
                if include_other {
                    body[other] = json!(32);
                }
                let error = adapter.complete(body).await.unwrap_err();
                assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);
                assert_eq!(
                    error.message,
                    format!("{key} must be an integer between 1 and 4096")
                );
                assert!(!adapter.is_busy());
            }
        }
    }
    for patch in fixture["invalid_pairs"].as_array().unwrap() {
        let mut body = patch.clone();
        body["messages"] = json!([{"role":"user","content":"synthetic"}]);
        let error = adapter.complete(body).await.unwrap_err();
        assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(error.message, fixture["conflict_error"].as_str().unwrap());
        assert!(!adapter.is_busy());
    }
    assert!(runtime.calls.lock().unwrap().is_empty());
}

fn config() -> RuntimeConfig {
    let fixture = fixtures();
    let mut value = fixture["config"]["models"][0].clone();
    for name in ["timeout_seconds", "max_output_tokens"] {
        value[name] = fixture["config"][name].clone();
    }
    RuntimeConfig::parse(&serde_json::to_vec(&value).unwrap()).unwrap()
}

#[derive(Default)]
struct Scripted {
    replies: Mutex<VecDeque<Result<Value, TransportError>>>,
    calls: Mutex<Vec<Value>>,
}

impl RuntimeTransport for Scripted {
    fn complete(&self, request: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(request);
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unprepared runtime call")
        })
    }
}

fn normalized(mut response: Value) -> Value {
    let id = response["id"]
        .as_str()
        .unwrap()
        .strip_prefix("chatcmpl-")
        .unwrap();
    assert_eq!(id.len(), 32);
    assert!(id
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    assert!(response["created"].as_u64().unwrap() > 0);
    let elapsed = response["swarm"]["elapsed_seconds"].as_f64().unwrap();
    assert!(elapsed.is_finite() && elapsed >= 0.0);
    response["id"] = "<completion-id>".into();
    response["created"] = "<unix-seconds>".into();
    response["swarm"]["elapsed_seconds"] = "<elapsed-seconds>".into();
    response
}

fn expected_runtime(step: &Value) -> Value {
    let body = &step["body"];
    let mut expected = json!({"model":"fixture-checkpoint","messages":body["messages"],
        "stream":false,"max_tokens":256,"temperature":0,"chat_template_kwargs":{"enable_thinking":false}});
    if body["tools"].as_array().is_some_and(|t| !t.is_empty()) {
        expected["tools"] = body["tools"].clone();
    }
    if let Some(overrides) = step["runtime_request"].as_object() {
        for (key, value) in overrides {
            expected[key] = value.clone();
        }
    }
    expected
}

#[tokio::test]
async fn non_sse_chat_scenarios_match_python_golden_and_forward_exactly() {
    let document = fixtures();
    let mut tested = 0;
    for scenario in document["scenarios"].as_array().unwrap() {
        let runtime = Arc::new(Scripted::default());
        let adapter = InferenceAdapter::new(config(), runtime.clone()).unwrap();
        for step in scenario["steps"].as_array().unwrap() {
            if step["method"] != "POST" || step.get("expected_sse").is_some() {
                continue;
            }
            if step.get("runtime_error").is_some() {
                assert_eq!(step["runtime_error"], TransportError::Timeout.message());
                runtime
                    .replies
                    .lock()
                    .unwrap()
                    .push_back(Err(TransportError::Timeout));
            } else {
                runtime
                    .replies
                    .lock()
                    .unwrap()
                    .push_back(Ok(step["runtime_response"].clone()));
            }
            let result = adapter.complete(step["body"].clone()).await;
            match result {
                Ok(response) => {
                    assert_eq!(step["status"], 200, "{}", scenario["id"]);
                    assert_eq!(normalized(response), step["expected"], "{}", scenario["id"]);
                }
                Err(error) => {
                    assert_eq!(
                        error.status.as_u16() as u64,
                        step["status"].as_u64().unwrap(),
                        "{}",
                        scenario["id"]
                    );
                    assert_eq!(error.body(), step["expected"], "{}", scenario["id"]);
                }
            }
            assert_eq!(
                runtime.calls.lock().unwrap().pop().unwrap(),
                expected_runtime(step),
                "{}",
                scenario["id"]
            );
            assert!(runtime.replies.lock().unwrap().is_empty());
            tested += 1;
        }
    }
    assert!(tested >= 10);
}

#[tokio::test]
async fn shared_invalid_requests_never_call_the_runtime() {
    let cases = validation();
    let runtime = Arc::new(Scripted::default());
    let adapter = InferenceAdapter::new(config(), runtime.clone()).unwrap();
    for case in cases["invalid_requests"].as_array().unwrap() {
        let mut body = case.get("body").unwrap_or(&cases["base_request"]).clone();
        if let Some(patch) = case["patch"].as_object() {
            for (key, value) in patch {
                body[key] = value.clone();
            }
        }
        let error = adapter.complete(body).await.unwrap_err();
        assert_eq!(error.status.as_u16(), 400, "{}", case["id"]);
        assert_eq!(
            error.body(),
            json!({"error":{"message":case["error"]}}),
            "{}",
            case["id"]
        );
    }
    assert!(runtime.calls.lock().unwrap().is_empty());
    assert!(!adapter.is_busy());
}

#[tokio::test]
async fn shared_invalid_tool_calls_fail_without_execution() {
    let cases = validation();
    let runtime = Arc::new(Scripted::default());
    let adapter = InferenceAdapter::new(config(), runtime.clone()).unwrap();
    for case in cases["invalid_backend_calls"].as_array().unwrap() {
        runtime
            .replies
            .lock()
            .unwrap()
            .push_back(Ok(json!({"choices":[{"message":{
            "role":"assistant","content":null,"tool_calls":case["calls"]}}]})));
        let mut body = cases["base_request"].clone();
        body["tools"] =
            json!([{"type":"function","function":{"name":"read","parameters":{"type":"object"}}}]);
        let error = adapter.complete(body).await.unwrap_err();
        assert_eq!(error.status.as_u16(), 502);
        assert_eq!(
            error.message,
            case["error"]
                .as_str()
                .unwrap_or("invalid structured tool call; nothing executed"),
            "{}",
            case["id"]
        );
        assert!(!adapter.is_busy());
    }
}

#[tokio::test]
async fn consensus_native_requests_and_large_payload_are_rejected_before_transport() {
    let runtime = Arc::new(Scripted::default());
    let adapter = InferenceAdapter::new(config(), runtime.clone()).unwrap();
    let mut body = validation()["base_request"].clone();
    body["swarm_mode"] = "consensus".into();
    assert_eq!(
        adapter.complete(body).await.unwrap_err().message,
        "consensus is not supported in this experimental build"
    );
    let native: Value = serde_json::from_str(include_str!(
        "../../../contracts/v1/examples/session-request.json"
    ))
    .unwrap();
    assert_eq!(
        adapter.complete(native).await.unwrap_err().status.as_u16(),
        400
    );
    let large = json!({"messages":[{"role":"user","content":"x".repeat(262144)}]});
    assert_eq!(
        adapter.complete(large).await.unwrap_err().status.as_u16(),
        400
    );
    assert!(runtime.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn completed_but_malformed_backend_messages_release_the_worker() {
    let runtime = Arc::new(Scripted::default());
    let adapter = InferenceAdapter::new(config(), runtime.clone()).unwrap();
    let body = validation()["base_request"].clone();
    for reply in [
        json!({"choices":[{"message":{"role":"user","content":"bad role"}}]}),
        json!({"choices":[{"message":{"role":"assistant","content":42}}]}),
        json!({"choices":[{"message":{"role":"assistant","content":null}}]}),
        json!({"choices":[{"message":{"role":"assistant","content":"OK"}}],"usage":{"prompt_tokens":-1}}),
        json!({"choices":[{"message":{"role":"assistant","content":"OK"}}],"usage":{"prompt_tokens":true}}),
        json!({"choices":[{"message":{"role":"assistant","content":"OK"}}],"usage":{"prompt_tokens":1.5}}),
        json!({"choices":[{"message":{"role":"assistant","content":"OK"}}],"usage":{"prompt_tokens":"1"}}),
        json!({"choices":[{"message":{"role":"assistant","content":"OK"},"finish_reason":42}]}),
        json!({"choices":[{"message":{"role":"assistant","content":"OK"},"finish_reason":"content_filter"}]}),
        json!({"choices":[{"message":{"role":"assistant","content":"OK"},"finish_reason":"tool_calls"}]}),
    ] {
        runtime.replies.lock().unwrap().push_back(Ok(reply));
        assert_eq!(
            adapter
                .complete(body.clone())
                .await
                .unwrap_err()
                .status
                .as_u16(),
            502
        );
        assert!(!adapter.is_busy());
        assert!(!adapter.requires_recovery());
    }
    runtime.replies.lock().unwrap().push_back(Ok(
        json!({"choices":[{"message":{"role":"assistant","content":"OK"}}]}),
    ));
    assert_eq!(
        adapter.complete(body).await.unwrap()["choices"][0]["message"]["content"],
        "OK"
    );
}

struct Gated {
    entered: Notify,
    release: Notify,
}
impl RuntimeTransport for Gated {
    fn complete(&self, _request: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            self.entered.notify_one();
            self.release.notified().await;
            Ok(json!({"choices":[{"message":{"role":"assistant","content":"OK"}}]}))
        })
    }
}

#[tokio::test]
async fn cancelling_consumer_does_not_free_an_active_runtime() {
    let runtime = Arc::new(Gated {
        entered: Notify::new(),
        release: Notify::new(),
    });
    let adapter = InferenceAdapter::new(config(), runtime.clone()).unwrap();
    let caller = adapter.clone();
    let task =
        tokio::spawn(async move { caller.complete(validation()["base_request"].clone()).await });
    tokio::time::timeout(Duration::from_secs(2), runtime.entered.notified())
        .await
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(adapter.is_busy());
    assert_eq!(
        adapter
            .complete(validation()["base_request"].clone())
            .await
            .unwrap_err()
            .status
            .as_u16(),
        429
    );
    runtime.release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), async {
        while adapter.is_busy() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!adapter.requires_recovery());
}

struct Pending;
impl RuntimeTransport for Pending {
    fn complete(&self, _request: Value) -> RuntimeFuture<'_> {
        Box::pin(std::future::pending())
    }
}

#[tokio::test]
async fn outer_deadline_keeps_uncertain_runtime_blocked() {
    let mut cfg = config();
    cfg.timeout_seconds = 1;
    let adapter = InferenceAdapter::new(cfg, Arc::new(Pending)).unwrap();
    let error = adapter
        .complete(validation()["base_request"].clone())
        .await
        .unwrap_err();
    assert_eq!(error.message, TransportError::Timeout.message());
    assert!(adapter.is_busy());
    assert!(adapter.requires_recovery());
    assert_eq!(
        adapter
            .complete(validation()["base_request"].clone())
            .await
            .unwrap_err()
            .status
            .as_u16(),
        503
    );
}

#[tokio::test]
async fn transport_errors_are_redacted_and_block_reentry() {
    for failure in [
        TransportError::Timeout,
        TransportError::Unavailable,
        TransportError::HttpFailure,
        TransportError::InvalidJson,
        TransportError::ResponseTooLarge,
    ] {
        let runtime = Arc::new(Scripted::default());
        runtime.replies.lock().unwrap().push_back(Err(failure));
        let adapter = InferenceAdapter::new(config(), runtime.clone()).unwrap();
        let error = adapter
            .complete(validation()["base_request"].clone())
            .await
            .unwrap_err();
        assert_eq!(error.message, failure.message());
        assert!(adapter.requires_recovery());
        assert_eq!(
            adapter
                .complete(validation()["base_request"].clone())
                .await
                .unwrap_err()
                .status
                .as_u16(),
            503
        );
        assert_eq!(runtime.calls.lock().unwrap().len(), 1);
    }
}
