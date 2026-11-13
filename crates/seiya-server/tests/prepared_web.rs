//! In-process HTTP + synthetic transports only. Never loads Gemma/Qwen.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use seiya_server::{
    jobs::{Executor, Failure, Work},
    prepared_web::{self, HostGate, PreparedWeb},
    runtime::{Backend, RuntimeConfig, RuntimeFuture, RuntimeTransport},
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{watch, Notify};
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:49370";
struct Gate {
    allow: AtomicBool,
    checks: AtomicUsize,
}
impl HostGate for Gate {
    fn admits(&self, foreground: &str, auxiliary: &str) -> bool {
        assert_eq!(foreground, "gemma4:e2b-mlx");
        assert_eq!(auxiliary, "qwen3.5:9b-mlx");
        self.checks.fetch_add(1, Ordering::SeqCst);
        self.allow.load(Ordering::SeqCst)
    }
}
#[derive(Default)]
struct Model {
    replies: Mutex<VecDeque<Value>>,
    requests: Mutex<Vec<Value>>,
}
impl Model {
    fn push(&self, value: Value) {
        self.replies.lock().unwrap().push_back(value);
    }
}
impl RuntimeTransport for Model {
    fn complete(&self, request: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request);
            Ok(self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected model dispatch"))
        })
    }
}
struct Auxiliary {
    calls: AtomicUsize,
    finish: Notify,
    uncertain: bool,
}
impl Auxiliary {
    fn new(uncertain: bool) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            finish: Notify::new(),
            uncertain,
        })
    }
}
struct Worker(Arc<Auxiliary>);
impl Executor for Worker {
    fn draft(&self, _: String, mut cancel: watch::Receiver<bool>) -> Work {
        let own = self.0.clone();
        own.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            tokio::select! {
                _=own.finish.notified()=>if own.uncertain{Err(Failure::Uncertain)}else{Ok("print('synthetic draft; not tested')".into())},
                _=cancel.changed()=>Err(Failure::Confirmed),
            }
        })
    }
}
fn config() -> RuntimeConfig {
    RuntimeConfig {
        id: "prepared-synthetic".into(),
        model: prepared_web::FOREGROUND.into(),
        backend: Backend::Mlx,
        base_url: "http://127.0.0.1:49371/v1".into(),
        timeout_seconds: 10,
        max_output_tokens: 256,
    }
}
fn fixtures(uncertain: bool) -> (Router, Arc<Model>, Arc<Auxiliary>, Arc<Gate>) {
    let model = Arc::new(Model::default());
    let worker = Auxiliary::new(uncertain);
    let gate = Arc::new(Gate {
        allow: AtomicBool::new(true),
        checks: AtomicUsize::new(0),
    });
    let state = PreparedWeb::with_host(
        config(),
        model.clone(),
        Arc::new(Worker(worker.clone())),
        gate.clone(),
    )
    .unwrap();
    (
        prepared_web::app(HOST.parse().unwrap(), "synthetic-key".into(), state).unwrap(),
        model,
        worker,
        gate,
    )
}
async fn send(app: &Router, path: &str, value: Value) -> (u16, Value) {
    let raw = serde_json::to_vec(&value).unwrap();
    let r = app
        .clone()
        .oneshot(
            Request::post(path)
                .header("host", HOST)
                .header("authorization", "Bearer synthetic-key")
                .header("content-type", "application/json")
                .header("content-length", raw.len())
                .body(Body::from(raw))
                .unwrap(),
        )
        .await
        .unwrap();
    let code = r.status().as_u16();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    (code, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}
async fn session(app: &Router) -> String {
    let (code, v) = send(app, "/api/jobs", json!({"operation":"session"})).await;
    assert_eq!(code, 200);
    v["session"].as_str().unwrap().into()
}
fn body(session: &str, id: &str, text: &str) -> Value {
    json!({"model":"pegassus","messages":[{"role":"user","content":text}],"job_session":session,"job_request_id":id})
}
fn call(name: &str, args: &str) -> Value {
    json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":"Invented 100% tests passed", "tool_calls":[{"id":"call1","type":"function","function":{"name":name,"arguments":args}}]}}]})
}
async fn wait_phase(app: &Router, session: &str, id: &str, phase: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let (code, v) = send(
                app,
                "/api/jobs",
                json!({"operation":"status","session":session,"job_id":id}),
            )
            .await;
            assert_eq!(code, 200);
            if v["phase"] == phase {
                return v;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("phase not observed")
}

#[tokio::test]
async fn feature_revocation_removes_tools_but_keeps_plain_chat() {
    struct Switch(AtomicBool);
    impl HostGate for Switch {
        fn admits(&self, _: &str, _: &str) -> bool {
            true
        }
        fn multiagent_enabled(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }
    let gate = Arc::new(Switch(AtomicBool::new(true)));
    let model = Arc::new(Model::default());
    let worker = Auxiliary::new(false);
    let state = PreparedWeb::with_host(
        config(),
        model.clone(),
        Arc::new(Worker(worker)),
        gate.clone(),
    )
    .unwrap();
    let app = prepared_web::app(HOST.parse().unwrap(), "synthetic-key".into(), state).unwrap();
    let s = session(&app).await;
    model.push(json!({"choices":[{"message":{"role":"assistant","content":"tool-capable fixture"},"finish_reason":"stop"}]}));
    assert_eq!(
        send(
            &app,
            "/api/chat",
            body(&s, &uuid::Uuid::new_v4().to_string(), "hello")
        )
        .await
        .0,
        200
    );
    assert!(model.requests.lock().unwrap()[0]["tools"].is_array());
    gate.0.store(false, Ordering::SeqCst);
    model.push(json!({"choices":[{"message":{"role":"assistant","content":"plain fixture"},"finish_reason":"stop"}]}));
    assert_eq!(
        send(
            &app,
            "/api/chat",
            json!({"model":"pegassus","messages":[{"role":"user","content":"hello"}]})
        )
        .await
        .0,
        200
    );
    assert!(model.requests.lock().unwrap()[1].get("tools").is_none());
}
#[tokio::test]
async fn blocked_by_default_but_controls_and_static_login_remain_available() {
    let model = Arc::new(Model::default());
    let worker = Auxiliary::new(false);
    let state =
        PreparedWeb::blocked(config(), model.clone(), Arc::new(Worker(worker.clone()))).unwrap();
    let app = prepared_web::app(HOST.parse().unwrap(), "synthetic-key".into(), state).unwrap();
    let s = session(&app).await;
    assert_eq!(
        send(
            &app,
            "/api/chat",
            body(&s, &uuid::Uuid::new_v4().to_string(), "hola")
        )
        .await
        .0,
        503
    );
    let (_, list) = send(&app, "/api/jobs", json!({"operation":"list","session":s})).await;
    assert_eq!(list["foreground_allowed"], false);
    let r = app
        .clone()
        .oneshot(
            Request::get("/")
                .header("host", HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let html =
        String::from_utf8(r.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(html.contains("Integración preparada, no activada"));
    assert!(html.contains("seiya.prepared.chats.v1"));
    assert!(!html.contains("const storageKey='seiya.chats.v1'"));
    let original = include_str!("../../../swarm-app/seiya/ui.html");
    assert_eq!(
        html.split("<style>")
            .nth(1)
            .unwrap()
            .split("</style>")
            .next(),
        original
            .split("<style>")
            .nth(1)
            .unwrap()
            .split("</style>")
            .next()
    );
    for tag in [
        "<main>",
        "<header>",
        "<aside>",
        "<section ",
        "<button ",
        "<textarea ",
    ] {
        assert_eq!(html.matches(tag).count(), original.matches(tag).count());
    }
    assert!(model.requests.lock().unwrap().is_empty());
    assert_eq!(worker.calls.load(Ordering::SeqCst), 0);
    assert_eq!(send(&app, "/v1/chat/completions", json!({})).await.0, 404);
}

#[tokio::test]
async fn chat_while_auxiliary_runs_status_ack_and_cancellation_use_actual_registry() {
    let (app, model, worker, gate) = fixtures(false);
    let s = session(&app).await;
    let request = uuid::Uuid::new_v4().to_string();
    model.push(call("code_job_submit", r#"{"task":"a synthetic draft"}"#));
    let (code, reply) = send(&app, "/api/chat", body(&s, &request, "programa")).await;
    assert_eq!(code, 200, "{reply}");
    assert_eq!(reply["swarm"]["status_rendered_by_host"], true);
    assert!(!reply["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .contains("100%"));
    let id = reply["jobs"]["jobs"][0]["job_id"].as_str().unwrap();
    wait_phase(&app, &s, id, "generating").await;
    model.push(json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"Hola sintético"}}]}));
    let (code, chat) = send(
        &app,
        "/api/chat",
        body(&s, &uuid::Uuid::new_v4().to_string(), "hola"),
    )
    .await;
    assert_eq!(code, 200, "{chat}");
    assert_eq!(chat["choices"][0]["message"]["content"], "Hola sintético");
    model.push(call("code_job_status", &json!({"job_id":id}).to_string()));
    let (code, status) = send(
        &app,
        "/api/chat",
        body(&s, &uuid::Uuid::new_v4().to_string(), "progreso"),
    )
    .await;
    assert_eq!(code, 200, "{status}");
    assert!(status["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .contains("Generando borrador"));
    assert_eq!(status["jobs"]["jobs"][0]["progress_percent"], Value::Null);
    gate.allow.store(false, Ordering::SeqCst);
    assert_eq!(
        send(
            &app,
            "/api/jobs",
            json!({"operation":"cancel","session":s,"job_id":id})
        )
        .await
        .0,
        200
    );
    wait_phase(&app, &s, id, "cancelled").await;
    assert_eq!(
        send(
            &app,
            "/api/jobs",
            json!({"operation":"artifact","session":s,"job_id":id})
        )
        .await
        .0,
        409
    );
    assert_eq!(
        send(&app, "/api/chat", body(&s, &request, "hola")).await.0,
        503
    );
    let calls = model.requests.lock().unwrap();
    assert_eq!(calls.len(), 3);
    for r in calls.iter() {
        let raw = r.to_string();
        assert!(!raw.contains(&s));
        assert!(!raw.contains(&request));
        assert_eq!(r["model"], prepared_web::FOREGROUND);
        assert_eq!(r["tools"].as_array().unwrap().len(), 3);
    }
    assert_eq!(worker.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn idempotent_submit_and_owned_download_without_execution() {
    let (app, model, worker, _) = fixtures(false);
    let s = session(&app).await;
    let request = uuid::Uuid::new_v4().to_string();
    for _ in 0..2 {
        model.push(call("code_job_submit", r#"{"task":"same task"}"#));
        let (code, _) = send(&app, "/api/chat", body(&s, &request, "draft")).await;
        assert_eq!(code, 200);
    }
    let (_, list) = send(&app, "/api/jobs", json!({"operation":"list","session":s})).await;
    assert_eq!(list["jobs"].as_array().unwrap().len(), 1);
    let id = list["jobs"][0]["job_id"].as_str().unwrap();
    wait_phase(&app, &s, id, "generating").await;
    worker.finish.notify_one();
    wait_phase(&app, &s, id, "completed").await;
    let (code, artifact) = send(
        &app,
        "/api/jobs",
        json!({"operation":"artifact","session":s,"job_id":id}),
    )
    .await;
    assert_eq!(code, 200);
    assert_eq!(artifact["tests_executed"], false);
    assert_eq!(artifact["workspace_written"], false);
    assert_eq!(artifact["filename"], "borrador.txt");
    let stranger = session(&app).await;
    assert_eq!(
        send(
            &app,
            "/api/jobs",
            json!({"operation":"artifact","session":stranger,"job_id":id})
        )
        .await
        .0,
        404
    );
    assert_eq!(worker.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn auth_origin_and_schema_precede_gate_and_model_dispatch() {
    let (app, model, _, gate) = fixtures(false);
    for (host, auth, origin, expected) in [
        (HOST, "Bearer wrong", None, 401),
        ("evil.example", "Bearer synthetic-key", None, 403),
        (
            HOST,
            "Bearer synthetic-key",
            Some("https://evil.example"),
            403,
        ),
    ] {
        let mut r = Request::post("/api/chat")
            .header("host", host)
            .header("authorization", auth);
        if let Some(origin) = origin {
            r = r.header("origin", origin);
        }
        let r = app
            .clone()
            .oneshot(r.body(Body::from("malformed")).unwrap())
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), expected);
    }
    assert_eq!(gate.checks.load(Ordering::SeqCst), 0);
    let s = session(&app).await;
    let id = uuid::Uuid::new_v4().to_string();
    for field in ["tools", "stream", "reader_run"] {
        let mut b = body(&s, &id, "x");
        b[field] = json!(true);
        assert_eq!(send(&app, "/api/chat", b).await.0, 400);
    }
    let mut b = body(&s, &id, "x");
    b["messages"][0]["role"] = json!("system");
    assert_eq!(send(&app, "/api/chat", b).await.0, 400);
    assert_eq!(send(&app,"/api/jobs",json!({"operation":"submit","session":s,"request_id":id,"prompt":"unapproved direct submit"})).await.0,400);
    assert!(model.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn malformed_tool_args_cannot_cross_session_or_execute_other_tools() {
    let (app, model, worker, _) = fixtures(false);
    let s = session(&app).await;
    let id = uuid::Uuid::new_v4().to_string();
    for raw in [
        r#"{"task":"x","session":"other"}"#,
        r#"{"task":"x","task":"y"}"#,
        r#"{"task":""}"#,
    ] {
        model.push(call("code_job_submit", raw));
        assert_eq!(send(&app, "/api/chat", body(&s, &id, "draft")).await.0, 400);
    }
    let mut parallel = call("code_job_submit", r#"{"task":"one"}"#);
    let mut second = parallel["choices"][0]["message"]["tool_calls"][0].clone();
    second["id"] = json!("call2");
    parallel["choices"][0]["message"]["tool_calls"]
        .as_array_mut()
        .unwrap()
        .push(second);
    model.push(parallel);
    assert_eq!(send(&app, "/api/chat", body(&s, &id, "draft")).await.0, 400);
    model.push(call("shell_run", r#"{"command":"not executable"}"#));
    assert!(send(&app, "/api/chat", body(&s, &id, "draft")).await.0 >= 400);
    assert_eq!(worker.calls.load(Ordering::SeqCst), 0);
    let (_, list) = send(&app, "/api/jobs", json!({"operation":"list","session":s})).await;
    assert_eq!(list["jobs"], json!([]));
}

#[tokio::test]
async fn uncertain_auxiliary_quarantines_foreground_without_status_bypass() {
    let (app, model, worker, _) = fixtures(true);
    let s = session(&app).await;
    let id = uuid::Uuid::new_v4().to_string();
    model.push(call("code_job_submit", r#"{"task":"uncertain mock"}"#));
    let (_, reply) = send(&app, "/api/chat", body(&s, &id, "draft")).await;
    let job = reply["jobs"]["jobs"][0]["job_id"].as_str().unwrap();
    wait_phase(&app, &s, job, "generating").await;
    worker.finish.notify_one();
    wait_phase(&app, &s, job, "quarantined").await;
    assert_eq!(send(&app, "/api/chat", body(&s, &id, "hello")).await.0, 503);
    let (_, state) = send(&app, "/api/jobs", json!({"operation":"list","session":s})).await;
    assert_eq!(state["foreground_allowed"], false);
    assert_eq!(model.requests.lock().unwrap().len(), 1);
    assert_eq!(
        send(
            &app,
            "/api/jobs",
            json!({"operation":"artifact","session":s,"job_id":job})
        )
        .await
        .0,
        409
    );
}

#[tokio::test]
async fn rollback_router_has_no_prepared_endpoints() {
    let app = seiya_server::app(HOST.parse().unwrap(), Some("synthetic-key".into())).unwrap();
    for path in ["/api/chat", "/api/jobs"] {
        assert_eq!(send(&app, path, json!({})).await.0, 404);
    }
}

struct SoloGate;
impl HostGate for SoloGate {
    fn admits(&self, foreground: &str, auxiliary: &str) -> bool {
        assert_eq!(foreground, prepared_web::FOREGROUND);
        assert_eq!(auxiliary, "");
        true
    }
}
#[tokio::test]
async fn explicit_solo_browser_has_transport_but_no_auxiliary_jobs_or_tools() {
    let model = Arc::new(Model::default());
    model.push(json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"hola sintético para test"}}]}));
    let state = PreparedWeb::solo(config(), model.clone(), Arc::new(SoloGate)).unwrap();
    let app = prepared_web::app(HOST.parse().unwrap(), "synthetic-key".into(), state).unwrap();
    let basic = json!({"model":"pegassus","messages":[{"role":"user","content":"hola"}]});
    let (code, answer) = send(&app, "/api/chat", basic.clone()).await;
    assert_eq!(code, 200, "{answer}");
    assert_eq!(
        answer["choices"][0]["message"]["content"],
        "hola sintético para test"
    );
    assert!(answer.get("jobs").is_none());
    assert_eq!(
        send(&app, "/api/jobs", json!({"operation":"session"}))
            .await
            .0,
        404
    );
    let mut bad = basic.clone();
    bad["job_session"] = json!("0".repeat(64));
    assert_eq!(send(&app, "/api/chat", bad).await.0, 400);
    let mut bad = basic;
    bad["tools"] = json!([]);
    assert_eq!(send(&app, "/api/chat", bad).await.0, 400);
    let request = Request::get("/health")
        .header("host", HOST)
        .header("authorization", "Bearer synthetic-key")
        .body(Body::empty())
        .unwrap();
    let r = app.oneshot(request).await.unwrap();
    let health: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(health["jobs"]["enabled"], false);
    assert_eq!(health["images"]["available"], false);
    assert_eq!(health["trial_mode"], "gemma-only");
    let requests = model.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].get("tools").is_none());
    assert_eq!(requests[0]["model"], prepared_web::FOREGROUND);
}
#[tokio::test]
async fn solo_without_gate_still_cannot_dispatch() {
    let model = Arc::new(Model::default());
    let state = PreparedWeb::solo(config(), model.clone(), Arc::new(prepared_web::Denied)).unwrap();
    let app = prepared_web::app(HOST.parse().unwrap(), "synthetic-key".into(), state).unwrap();
    assert_eq!(
        send(
            &app,
            "/api/chat",
            json!({"model":"pegassus","messages":[{"role":"user","content":"hola"}]})
        )
        .await
        .0,
        503
    );
    assert!(model.requests.lock().unwrap().is_empty());
}
