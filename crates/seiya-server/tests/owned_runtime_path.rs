//! Shared guard wiring; native subprocess rehearsal is explicit and model-free.
use seiya_observe::{
    campaign::{Pressure, PressureSample},
    session_guard::MemoryGuard,
};
use seiya_server::{config::Config, BoundServer};
use serde_json::{json, Value};
use std::time::Duration;

fn config(port: u16) -> Config {
    Config::parse(
        &serde_json::to_vec(&json!({
    "listen":"127.0.0.1:0","runtime_status":true,"runtime":{"id":"fixture",
    "model":"synthetic-checkpoint","backend":"mlx","base_url":format!("http://127.0.0.1:{port}/v1"),
    "timeout_seconds":1,"max_output_tokens":256}}))
        .unwrap(),
    )
    .unwrap()
}
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap()
}

#[tokio::test]
async fn bound_server_guard_requires_opt_in_and_blocks_before_any_backend_connection() {
    let discovery = Config::parse(br#"{"listen":"127.0.0.1:0"}"#).unwrap();
    assert!(
        BoundServer::bind_with_memory_guard(&discovery, MemoryGuard::new())
            .await
            .is_err()
    );
    let guard = MemoryGuard::new();
    let server = BoundServer::bind_with_memory_guard(&config(65535), guard.clone())
        .await
        .unwrap();
    let base = format!("http://{}", server.local_addr());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(server.serve(async move {
        let _ = rx.await;
    }));
    let client = client();
    let health: Value = client
        .get(format!("{base}/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["runtime_status"]["version"], 2);
    assert_eq!(
        health["runtime_status"]["memory_admission"]["state"],
        "awaiting_observation"
    );
    let response = client
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({"messages":[{"role":"user","content":"fixture"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"]["code"],
        "memory_observation_unavailable"
    );
    let _ = guard.observe(Ok(PressureSample {
        sampled_at_millis: guard.now_millis().unwrap(),
        pressure: Pressure::Warning,
    }));
    let response = client
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({"messages":[{"role":"user","content":"fixture"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"]["code"],
        "runtime_memory_blocked"
    );
    tx.send(()).unwrap();
    task.await.unwrap().unwrap();
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "explicit owned no-fork HTTP runtime + shared native lease/guard + Rust chat, no model, external network or operational ports"]
async fn native_restricted_chat_pressure_stop_and_lease_release_are_integrated() {
    use seiya_observe::{
        campaign::{MlxRecipe, Reservation},
        session_runtime,
        session_supervisor::{Policy, Stage},
    };
    use std::{
        io::Write,
        os::unix::fs::{DirBuilderExt, OpenOptionsExt},
        path::PathBuf,
    };
    let bootstrap = PathBuf::from(
        std::env::var_os("SEIYA_RESTRICTED_BOOTSTRAP").expect("fresh repository bootstrap"),
    );
    let fixture = PathBuf::from(
        std::env::var_os("SEIYA_RESTRICTED_FIXTURE").expect("fresh repository runtime fixture"),
    );
    let output = PathBuf::from(
        std::env::var_os("SEIYA_RESTRICTED_CHAT_OUTPUT").expect("new absolute report path"),
    );
    assert!(output.is_absolute() && !output.exists());
    let directory = output.with_extension("coordination");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let recipe = MlxRecipe {
        python: "/synthetic/python".into(),
        runtime_entrypoint: "/synthetic/mlx.py".into(),
        checkpoint: "/synthetic/checkpoint".into(),
        coordination_directory: directory.canonicalize().unwrap(),
        minimum_free_bytes: 2 << 30,
    };
    let guard = MemoryGuard::new();
    let policy = Policy::new(
        Duration::from_secs(10),
        Duration::from_millis(500),
        Duration::from_secs(2),
    )
    .unwrap();
    let started = session_runtime::start_rehearsal(
        Reservation::acquire(&recipe).unwrap(),
        &bootstrap,
        &fixture,
        guard.clone(),
        policy,
        Some(Pressure::Normal),
    )
    .await
    .unwrap();
    let mut supervisor = started.supervisor;
    let server = BoundServer::bind_with_memory_guard(&config(started.port), guard.clone())
        .await
        .unwrap();
    let base = format!("http://{}", server.local_addr());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(server.serve(async move {
        let _ = rx.await;
    }));
    let client = client();
    let initial: Value = client
        .get(format!("{base}/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        initial["runtime_status"]["memory_admission"]["state"],
        "monitoring"
    );
    let body = json!({"model":"pegassus","messages":[{"role":"user","content":"fixture"}],"max_tokens":256});
    let answer: Value = client
        .post(format!("{base}/v1/chat/completions"))
        .json(&body)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        answer["choices"][0]["message"]["content"],
        "MEMORY_BASELINE_OK"
    );
    let mut tools = body.clone();
    tools["tools"] = json!([{"type":"function","function":{"name":"read","parameters":{"type":"object","properties":{"path":{"type":"string"}}}}}]);
    let answer: Value = client
        .post(format!("{base}/v1/chat/completions"))
        .json(&tools)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        answer["choices"][0]["message"]["tool_calls"][0]["id"],
        "fixture-call"
    );
    assert!(Reservation::acquire(&recipe).is_err());
    let _ = guard.observe(Ok(PressureSample {
        sampled_at_millis: guard.now_millis().unwrap(),
        pressure: Pressure::Warning,
    }));
    let stopped = supervisor.wait_terminal().await.unwrap();
    assert_eq!(stopped.stage, Stage::Stopped);
    assert!(stopped.runtime_reaped && stopped.lease_release_confirmed);
    let response = client
        .post(format!("{base}/v1/chat/completions"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    assert!(!response.headers().contains_key("retry-after"));
    let failure: Value = response.json().await.unwrap();
    assert_eq!(failure["error"]["code"], "runtime_memory_blocked");
    let health: Value = client
        .get(format!("{base}/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        health["runtime_status"]["memory_admission"]["state"],
        "stopped"
    );
    tx.send(()).unwrap();
    task.await.unwrap().unwrap();
    drop(Reservation::acquire(&recipe).unwrap());
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&output)
        .unwrap();
    file.write_all(
        &serde_json::to_vec_pretty(
            &json!({"mode":"restricted-owned-chat-fixture","model_inference":false,
        "text_and_tools_preserved":true,"tools_executed":0,"pressure_source":"injected_fixture",
        "supervisor":stopped,"health":health["runtime_status"],"error":failure}),
        )
        .unwrap(),
    )
    .unwrap();
    file.sync_all().unwrap();
}
