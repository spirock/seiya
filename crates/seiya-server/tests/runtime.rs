use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use seiya_server::inference::InferenceAdapter;
use seiya_server::runtime::{
    HttpTransport, RuntimeConfig, RuntimeTransport, TransportError, MAX_RUNTIME_RESPONSE_BYTES,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

fn config(url: &str) -> RuntimeConfig {
    RuntimeConfig::parse(
        &serde_json::to_vec(&json!({"id":"fixture-worker","model":"fixture-checkpoint",
        "backend":"mlx","base_url":url,"timeout_seconds":1,"max_output_tokens":256}))
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn runtime_config_rejects_remote_dns_redirectable_credentials_and_unsupported_backends() {
    for url in [
        "https://127.0.0.1:8081/v1",
        "http://localhost:8081/v1",
        "http://192.0.2.1:8081/v1",
        "http://127.0.0.1/v1",
        "http://127.0.0.1:0/v1",
        "http://127.0.0.1:8081/",
        "http://127.0.0.1:8081/v1?key=secret",
        "http://user:secret@127.0.0.1:8081/v1",
        "http://127.0.0.1:8081/v1#secret",
        "http://127.1:8081/v1",
        "http://2130706433:8081/v1",
        "http://[::1]:8081/v1",
        "http://127.0.0.1:8081/v1/../v1",
        " http://127.0.0.1:8081/v1",
    ] {
        let mut value = json!({"id":"worker","model":"fixture","backend":"mlx","base_url":url,
            "timeout_seconds":1,"max_output_tokens":256});
        let error = RuntimeConfig::parse(&serde_json::to_vec(&value).unwrap())
            .err()
            .unwrap();
        assert!(!error.to_string().contains("secret"));
        value["base_url"] = "http://127.0.0.1:65535/v1".into();
        value["backend"] = "ollama".into();
        assert!(RuntimeConfig::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}

#[test]
fn invalid_budgets_ids_and_models_are_rejected() {
    for (field, value) in [
        ("id", json!("bad id")),
        ("id", json!("")),
        ("model", json!("\n")),
        ("timeout_seconds", json!(0)),
        ("timeout_seconds", json!(601)),
        ("max_output_tokens", json!(0)),
        ("max_output_tokens", json!(4097)),
        ("max_output_tokens", json!(true)),
    ] {
        let mut base = json!({"id":"worker","model":"fixture","backend":"mlx","base_url":"http://127.0.0.1:65535/v1",
            "timeout_seconds":1,"max_output_tokens":256});
        base[field] = value;
        assert!(RuntimeConfig::parse(&serde_json::to_vec(&base).unwrap()).is_err());
    }
}

#[tokio::test]
async fn constructing_transport_does_not_connect() {
    let cfg = config("http://127.0.0.1:65535/v1/");
    assert_eq!(cfg.endpoint().unwrap().path(), "/v1/chat/completions");
    assert!(HttpTransport::new(&cfg).is_ok());
}

#[tokio::test]
async fn local_post_preserves_json_without_touching_a_model() {
    async fn handler(
        State(calls): State<Arc<Mutex<Vec<Value>>>>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        calls.lock().unwrap().push(body);
        Json(json!({"choices":[{"message":{"role":"assistant","content":"synthetic"}}]}))
    }
    let calls = Arc::new(Mutex::new(Vec::new()));
    let router = Router::new()
        .route("/v1/chat/completions", post(handler))
        .with_state(calls.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cfg = config(&format!("http://{}/v1", listener.local_addr().unwrap()));
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let transport = HttpTransport::new(&cfg).unwrap();
    let input = json!({"model":"fixture-checkpoint","messages":[{"role":"user","content":"synthetic"}],"stream":false});
    let reply = transport.complete(input.clone()).await.unwrap();
    assert_eq!(reply["choices"][0]["message"]["content"], "synthetic");
    assert_eq!(*calls.lock().unwrap(), vec![input]);
    let adapter = InferenceAdapter::new(cfg, Arc::new(transport)).unwrap();
    let completed = adapter
        .complete(json!({"model":"pegassus","swarm_mode":"fast",
        "messages":[{"role":"user","content":"synthetic"}]}))
        .await
        .unwrap();
    assert_eq!(completed["model"], "pegassus");
    assert_eq!(completed["choices"][0]["message"]["content"], "synthetic");
    assert_eq!(
        calls.lock().unwrap()[1],
        json!({"model":"fixture-checkpoint","stream":false,
        "messages":[{"role":"user","content":"synthetic"}],"max_tokens":256,
        "temperature":0,"chat_template_kwargs":{"enable_thinking":false}})
    );
    stop.send(()).unwrap();
    server.await.unwrap();
}

async fn raw_backend(reply: Vec<u8>) -> (RuntimeConfig, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cfg = config(&format!("http://{}/v1", listener.local_addr().unwrap()));
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await;
        // Early rejection/limits may close the client before all bytes are sent.
        let _ = socket.write_all(&reply).await;
    });
    (cfg, task)
}

#[tokio::test]
async fn redirects_http_errors_and_invalid_json_are_redacted() {
    for (wire, expected) in [
        ("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/private-secret\r\nContent-Length: 0\r\n\r\n",TransportError::HttpFailure),
        ("HTTP/1.1 500 Error\r\nContent-Length: 14\r\n\r\nprivate-secret",TransportError::HttpFailure),
        ("HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nnot-json",TransportError::InvalidJson)] {
        let (cfg,task)=raw_backend(wire.as_bytes().to_vec()).await;
        let error=HttpTransport::new(&cfg).unwrap().complete(json!({})).await.unwrap_err();
        assert_eq!(error,expected);
        assert!(!error.message().contains("private-secret"));
        task.await.unwrap();
    }
}

#[tokio::test]
async fn response_size_is_bounded_with_and_without_content_length() {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
        MAX_RUNTIME_RESPONSE_BYTES + 1
    );
    let (cfg, task) = raw_backend(header.into_bytes()).await;
    assert_eq!(
        HttpTransport::new(&cfg)
            .unwrap()
            .complete(json!({}))
            .await
            .unwrap_err(),
        TransportError::ResponseTooLarge
    );
    task.await.unwrap();
    let mut reply = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
    reply.resize(reply.len() + MAX_RUNTIME_RESPONSE_BYTES + 1, b' ');
    let (cfg, task) = raw_backend(reply).await;
    assert_eq!(
        HttpTransport::new(&cfg)
            .unwrap()
            .complete(json!({}))
            .await
            .unwrap_err(),
        TransportError::ResponseTooLarge
    );
    task.await.unwrap();
}

#[tokio::test]
async fn local_transport_timeout_is_explicit() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cfg = config(&format!("http://{}/v1", listener.local_addr().unwrap()));
    let backend = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
    });
    let error = HttpTransport::new(&cfg)
        .unwrap()
        .complete(json!({}))
        .await
        .unwrap_err();
    assert_eq!(error, TransportError::Timeout);
    backend.abort();
    let _ = backend.await;
}
