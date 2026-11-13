use super::*;
use axum::body::Body;

fn input() -> Value {
    json!({"model":"synthetic-checkpoint","messages":[{"role":"user","content":"synthetic"}],"stream":false,"max_tokens":256})
}
fn request(value: &Value) -> Request {
    let bytes = serde_json::to_vec(value).unwrap();
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header(header::HOST, "127.0.0.1:9999")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .unwrap()
}
async fn value(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 262144).await.unwrap()).unwrap()
}

#[tokio::test]
async fn fixture_is_data_only_with_structured_calls_and_fixed_budget() {
    let state = FixtureState {
        host: "127.0.0.1:9999".into(),
        model: "synthetic-checkpoint".into(),
    };
    let plain = value(fixture(State(state.clone()), request(&input())).await).await;
    assert_eq!(
        plain["choices"][0]["message"]["content"],
        "MEMORY_BASELINE_OK"
    );
    let mut tools = input();
    tools["tools"] =
        json!([{"type":"function","function":{"name":"read","parameters":{"type":"object"}}}]);
    let call = value(fixture(State(state.clone()), request(&tools)).await).await;
    assert_eq!(
        call["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
        "{\"path\":\"fixture.txt\"}"
    );
    tools["messages"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"tool","content":"not executed","tool_call_id":"fixture-call"}));
    let final_reply = value(fixture(State(state.clone()), request(&tools)).await).await;
    assert_eq!(
        final_reply["choices"][0]["message"]["content"],
        "MEMORY_TOOL_OK_47"
    );
    let mut invalid = input();
    invalid["max_tokens"] = 512.into();
    assert_eq!(
        fixture(State(state), request(&invalid)).await.status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn host_origin_framing_and_json_fail_before_forwarding() {
    let mut bad = request(&input());
    bad.headers_mut()
        .insert(header::HOST, "foreign.invalid".parse().unwrap());
    assert_eq!(
        body(bad, "127.0.0.1:9999").await.unwrap_err(),
        StatusCode::FORBIDDEN
    );
    let mut bad = request(&input());
    bad.headers_mut()
        .insert(header::ORIGIN, "http://foreign.invalid".parse().unwrap());
    assert_eq!(
        body(bad, "127.0.0.1:9999").await.unwrap_err(),
        StatusCode::FORBIDDEN
    );
    let mut bad = request(&input());
    bad.headers_mut()
        .append(header::CONTENT_LENGTH, "2".parse().unwrap());
    assert_eq!(
        body(bad, "127.0.0.1:9999").await.unwrap_err(),
        StatusCode::BAD_REQUEST
    );
    let mut bad = request(&input());
    bad.headers_mut()
        .insert(header::TRANSFER_ENCODING, "chunked".parse().unwrap());
    assert_eq!(
        body(bad, "127.0.0.1:9999").await.unwrap_err(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn concurrent_proxy_request_is_rejected_without_recording_or_forwarding() {
    let trace = Arc::new(Trace::new());
    trace.enabled.store(true, Ordering::Release);
    trace.busy.store(true, Ordering::Release);
    let state = ProxyState {
        host: "127.0.0.1:9999".into(),
        upstream: "http://127.0.0.1:1/never-contact".into(),
        client: client().unwrap(),
        trace: trace.clone(),
    };
    assert_eq!(
        proxy(State(state), request(&input())).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert!(trace.unexpected.load(Ordering::Acquire));
    assert!(trace.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn proxy_records_actual_request_and_rejects_unconsumed_or_concurrent_work() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = FixtureState {
        host: address.to_string(),
        model: "synthetic-checkpoint".into(),
    };
    let app = Router::new()
        .route("/v1/chat/completions", post(fixture))
        .with_state(state);
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let mut proxy = Proxy::start(address.port()).await.unwrap();
    let client = client().unwrap();
    let url = format!("{}/v1/chat/completions", proxy.url);
    assert_eq!(
        client
            .post(&url)
            .json(&input())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(proxy.trace.requests.lock().unwrap().is_empty());
    proxy.trace.enabled.store(true, Ordering::Release);
    let response = client.post(&url).json(&input()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();
    assert_eq!(*proxy.trace.requests.lock().unwrap(), vec![input()]);
    assert_eq!(
        client
            .post(&url)
            .json(&input())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert!(proxy.trace.unexpected.load(Ordering::Acquire));
    assert!(proxy.stop().await);
    stop.send(()).unwrap();
    task.await.unwrap();
}

fn stream() -> String {
    let base = json!({"id":"chatcmpl-0123456789abcdef0123456789abcdef","created":1,"model":"pegassus","object":"chat.completion.chunk"});
    let mut first = base.clone();
    first["choices"] = json!([{"index":0,"delta":{"role":"assistant","content":null,
        "tool_calls":[{"index":0,"id":"fixture","type":"function","function":{"name":"read","arguments":"{ \"path\" : \"ñ.txt\" }"}}]},"finish_reason":null}]);
    let mut last = base;
    last["choices"] = json!([{"index":0,"delta":{},"finish_reason":"tool_calls"}]);
    last["usage"] = json!({"prompt_tokens":10,"completion_tokens":2,"total_tokens":12});
    format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n")
}

#[test]
fn sse_decode_preserves_literals_and_rejects_missing_done_ids_or_usage() {
    let text = stream();
    let decoded = decode_sse(text.as_bytes()).unwrap();
    assert_eq!(
        decoded["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
        "{ \"path\" : \"ñ.txt\" }"
    );
    assert!(decoded["choices"][0]["message"]["tool_calls"][0]
        .get("index")
        .is_none());
    for invalid in [
        text.replace("data: [DONE]\n\n", ""),
        text.replacen("0123456789abcdef0123456789abcdef", "different", 1),
        text.replace("\"usage\":", "\"not_usage\":"),
        text.replace("\"index\":0,\"type\"", "\"index\":1,\"type\""),
    ] {
        if invalid != text {
            assert!(decode_sse(invalid.as_bytes()).is_err());
        }
    }
}

#[test]
fn both_existing_coordinator_sse_content_types_are_accepted_without_changing_python() {
    assert!(sse_content_type(Some("text/event-stream")));
    assert!(sse_content_type(Some("text/event-stream; charset=utf-8")));
    assert!(!sse_content_type(Some("application/json")));
    assert!(!sse_content_type(Some("text/event-stream; charset=latin1")));
    assert!(!sse_content_type(None));
}

#[test]
fn recipe_fixture_accepts_only_the_fixed_mlx_arguments_without_opening_checkpoint() {
    let recipe = crate::campaign::MlxRecipe {
        python: "/synthetic/python".into(),
        runtime_entrypoint: "/synthetic/shim".into(),
        checkpoint: "/does-not-exist/checkpoint".into(),
        coordination_directory: "/synthetic/coordination".into(),
        minimum_free_bytes: 1 << 30,
    };
    let args = recipe.runtime_arguments(55000).unwrap()[1..].to_vec();
    assert_eq!(
        recipe_fixture_arguments(&args),
        Some((55000, "/does-not-exist/checkpoint".into()))
    );
    let mut invalid = args.clone();
    invalid.push("--trust-remote-code".into());
    assert!(recipe_fixture_arguments(&invalid).is_none());
    let mut invalid = args.clone();
    let max = invalid.iter().position(|a| a == "--max-tokens").unwrap();
    invalid[max + 1] = "512".into();
    assert!(recipe_fixture_arguments(&invalid).is_none());
    let mut invalid = args;
    invalid[1] = "remote/model".into();
    assert!(recipe_fixture_arguments(&invalid).is_none());
}
