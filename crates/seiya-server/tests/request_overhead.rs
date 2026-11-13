//! Synthetic in-process HTTP overhead only; never model/runtime network work.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use seiya_server::{
    app_with_scheduler,
    inference::InferenceAdapter,
    runtime::{RuntimeConfig, RuntimeFuture, RuntimeTransport},
    scheduler::Scheduler,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tower::ServiceExt;

struct Immediate(AtomicUsize);
impl RuntimeTransport for Immediate {
    fn complete(&self, body: Value) -> RuntimeFuture<'_> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move {
            assert_eq!(body["max_tokens"], 256);
            assert_eq!(body["stream"], false);
            assert!(body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .starts_with("synthetic"));
            Ok(
                json!({"choices":[{"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}),
            )
        })
    }
}

#[tokio::test]
#[ignore = "explicit CPU microbenchmark with fixed synthetic in-process HTTP and immediate fake runtime; no network, host probes or models"]
async fn synthetic_http_request_overhead() {
    let output = std::path::PathBuf::from(
        std::env::var_os("SEIYA_OVERHEAD_OUTPUT").expect("new absolute evidence file required"),
    );
    assert!(output.is_absolute() && !output.exists());
    let runtime = Arc::new(Immediate(AtomicUsize::new(0)));
    let cfg=RuntimeConfig::parse(br#"{"id":"fake","model":"synthetic-checkpoint","backend":"mlx","base_url":"http://127.0.0.1:9999/v1","timeout_seconds":10,"max_output_tokens":256}"#).unwrap();
    let scheduler = Scheduler::new(
        InferenceAdapter::new(cfg, runtime.clone()).unwrap(),
        Duration::from_secs(1),
    )
    .unwrap();
    let app = app_with_scheduler(
        "127.0.0.1:9998".parse().unwrap(),
        None,
        scheduler,
        Duration::from_secs(1),
    )
    .unwrap();
    let mut cases = Vec::new();
    for characters in [4096, 240 * 1024] {
        let bytes=serde_json::to_vec(&json!({"model":"pegassus","messages":[{"role":"user","content":format!("synthetic{}","x".repeat(characters))}],"max_tokens":256})).unwrap();
        let mut batches = Vec::new();
        for batch in 0..6 {
            let started = Instant::now();
            for _ in 0..100 {
                let request = Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("host", "127.0.0.1:9998")
                    .header("content-type", "application/json")
                    .header("content-length", bytes.len())
                    .body(Body::from(bytes.clone()))
                    .unwrap();
                let response = app.clone().oneshot(request).await.unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let body = axum::body::to_bytes(response.into_body(), 65536)
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::from_slice::<Value>(&body).unwrap()["choices"][0]["message"]
                        ["content"],
                    "OK"
                );
            }
            if batch > 0 {
                batches.push(started.elapsed().as_micros() as u64);
            }
        }
        let mut sorted = batches.clone();
        sorted.sort();
        cases.push(
            json!({"request_bytes":bytes.len(),"requests_per_batch":100,"measured_batches":5,
            "batch_micros":batches,"median_microseconds_per_request":sorted[2] as f64/100.0}),
        );
    }
    assert_eq!(runtime.0.load(Ordering::Relaxed), 1200);
    let document = json!({"mode":"synthetic-in-process-http-overhead","fake_runtime":true,"model_inference":false,"network":false,
        "cases":cases,"caveats":"CPU-only microbenchmark, not LLM latency/tokens-per-second or memory baseline"});
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(&document).unwrap())
        .unwrap();
    println!("{document}");
}
