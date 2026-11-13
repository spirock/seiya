use seiya_server::config::Config;
use seiya_server::BoundServer;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::time::timeout;

#[tokio::test]
async fn ephemeral_listener_serves_discovery_and_stops_cleanly() {
    let config = Config::parse(br#"{"listen":"127.0.0.1:0"}"#).unwrap();
    let server = BoundServer::bind(&config).await.unwrap();
    let address = server.local_addr();
    assert_ne!(address.port(), 0);
    assert_ne!(address.port(), 8010);
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(server.serve(async {
        let _ = stopped.await;
    }));
    let mut stream = timeout(Duration::from_secs(2), TcpStream::connect(address))
        .await
        .unwrap()
        .unwrap();
    stream
        .write_all(
            format!("GET /v1/models HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut bytes = Vec::new();
    timeout(Duration::from_secs(2), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    let result = String::from_utf8(bytes).unwrap();
    assert!(result.starts_with("HTTP/1.1 200 OK\r\n"));
    let body: serde_json::Value =
        serde_json::from_str(result.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["data"][0]["id"], "pegassus");
    stop.send(()).unwrap();
    timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(TcpStream::connect(address).await.is_err());
}

#[tokio::test]
async fn occupied_port_fails_without_fallback_or_taking_over() {
    let existing = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = existing.local_addr().unwrap();
    let json = format!(r#"{{"listen":"{address}"}}"#);
    let config = Config::parse(json.as_bytes()).unwrap();
    assert!(BoundServer::bind(&config).await.is_err());
    assert_eq!(existing.local_addr().unwrap(), address);
}

#[tokio::test]
async fn opt_in_bound_server_drains_real_http_work_even_if_consumer_disappears() {
    use axum::extract::State;
    use axum::routing::post;
    use axum::{Json, Router};
    use std::sync::Arc;
    use tokio::sync::{Notify, Semaphore};
    struct Gate {
        entered: Notify,
        release: Semaphore,
    }
    async fn backend(
        State(gate): State<Arc<Gate>>,
        Json(body): Json<serde_json::Value>,
    ) -> Json<serde_json::Value> {
        assert_eq!(body["model"], "synthetic-checkpoint");
        assert_eq!(body["stream"], false);
        gate.entered.notify_one();
        gate.release.acquire().await.unwrap().forget();
        Json(
            serde_json::json!({"choices":[{"message":{"role":"assistant","content":"synthetic-http"}}]}),
        )
    }
    for abandon_consumer in [false, true] {
        let gate = Arc::new(Gate {
            entered: Notify::new(),
            release: Semaphore::new(0),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_address = listener.local_addr().unwrap();
        let config = Config::parse(
            &serde_json::to_vec(&serde_json::json!({
            "listen":"127.0.0.1:0","runtime":{"id":"worker","backend":"mlx",
            "model":"synthetic-checkpoint","base_url":format!("http://{backend_address}/v1"),
            "timeout_seconds":5,"max_output_tokens":256}}))
            .unwrap(),
        )
        .unwrap();
        let frontend = BoundServer::bind(&config).await.unwrap();
        assert!(frontend.chat_enabled());
        // Construction does not contact or probe the runtime.
        assert!(timeout(Duration::from_millis(20), listener.accept())
            .await
            .is_err());
        let backend_app = Router::new()
            .route("/v1/chat/completions", post(backend))
            .with_state(gate.clone());
        let (backend_stop, backend_stopped) = oneshot::channel();
        let backend_task = tokio::spawn(async move {
            axum::serve(listener, backend_app)
                .with_graceful_shutdown(async {
                    let _ = backend_stopped.await;
                })
                .await
                .unwrap();
        });
        let address = frontend.local_addr();
        let (stop, stopped) = oneshot::channel();
        let server = tokio::spawn(frontend.serve(async {
            let _ = stopped.await;
        }));
        let consumer = tokio::spawn(async move {
            reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .post(format!("http://{address}/v1/chat/completions"))
                .json(&serde_json::json!({"model":"pegassus","swarm_mode":"fast",
                    "messages":[{"role":"user","content":"synthetic"}]}))
                .send()
                .await
                .unwrap()
                .json::<serde_json::Value>()
                .await
                .unwrap()
        });
        timeout(Duration::from_secs(2), gate.entered.notified())
            .await
            .unwrap();
        let consumer = if abandon_consumer {
            consumer.abort();
            let _ = consumer.await;
            None
        } else {
            Some(consumer)
        };
        stop.send(()).unwrap();
        tokio::task::yield_now().await;
        assert!(!server.is_finished());
        gate.release.add_permits(1);
        if let Some(consumer) = consumer {
            let result = consumer.await.unwrap();
            assert_eq!(result["choices"][0]["message"]["content"], "synthetic-http");
        }
        timeout(Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        backend_stop.send(()).unwrap();
        backend_task.await.unwrap();
        assert!(TcpStream::connect(address).await.is_err());
    }
}

#[tokio::test]
async fn unconfirmed_runtime_cannot_report_clean_bound_server_shutdown() {
    use axum::routing::post;
    use axum::Router;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let (backend_stop, backend_stopped) = oneshot::channel();
    let backend = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/v1/chat/completions",
                post(|| async {
                    (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        "synthetic-private-backend-detail",
                    )
                }),
            ),
        )
        .with_graceful_shutdown(async {
            let _ = backend_stopped.await;
        })
        .await
        .unwrap();
    });
    let config=Config::parse(&serde_json::to_vec(&serde_json::json!({"listen":"127.0.0.1:0",
        "runtime":{"id":"worker","backend":"mlx","model":"synthetic","base_url":format!("http://{endpoint}/v1"),
        "timeout_seconds":5,"max_output_tokens":256}})).unwrap()).unwrap();
    let server = BoundServer::bind(&config).await.unwrap();
    let address = server.local_addr();
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(server.serve(async {
        let _ = stopped.await;
    }));
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://{address}/v1/chat/completions"))
        .json(&serde_json::json!({"messages":[{"role":"user","content":"synthetic"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
    let text = response.text().await.unwrap();
    assert!(!text.contains("synthetic-private-backend-detail"));
    stop.send(()).unwrap();
    let error = timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("unconfirmed"));
    backend_stop.send(()).unwrap();
    backend.await.unwrap();
}
