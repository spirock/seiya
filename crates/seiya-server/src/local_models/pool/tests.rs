use super::*;
mod image_cases;
use crate::jobs::Executor;
use crate::local_models::{Assignments, Model};
use crate::runtime::{Backend, RuntimeConfig};
use std::sync::Mutex;

pub(crate) struct Fixture(pub PathBuf);
impl Fixture {
    pub(crate) fn new() -> Self {
        let p = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("seiya-catalog-{}", uuid::Uuid::new_v4()));
        storage::ensure_directory(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
pub(crate) fn catalog() -> Catalog {
    Catalog {
        version: 1,
        models: [
            ("brain", "runtime-brain", 21001),
            ("worker-a", "runtime-worker-a", 21002),
            ("worker-b", "runtime-worker-b", 21003),
        ]
        .into_iter()
        .map(|(id, binding, port)| Model {
            id: id.into(),
            runtime: RuntimeConfig {
                id: id.into(),
                model: binding.into(),
                backend: Backend::OpenaiLocal,
                base_url: format!("http://127.0.0.1:{port}/v1"),
                timeout_seconds: 1,
                max_output_tokens: 256,
            },
            supports_tools: true,
            max_input_bytes: 65536,
        })
        .collect(),
        assignments: Assignments {
            brain: "brain".into(),
            workers: vec!["worker-a".into(), "worker-b".into()],
        },
        minimum_available_mib: 256,
        web_search: false,
        image_models: vec![],
        image: None,
        image_chat: false,
    }
}
struct Memory(AtomicBool);
impl Resources for Memory {
    fn admits(&self, _: u64) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
struct Fake {
    binding: String,
    log: Arc<Mutex<Vec<String>>>,
    bad: bool,
    hold: bool,
    entered: Arc<tokio::sync::Notify>,
}
impl Endpoint for Fake {
    fn advertises(&self, binding: &str) -> Probe<'_> {
        let matched = binding == self.binding;
        Box::pin(async move { Ok(matched) })
    }
    fn complete(&self, request: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            self.log
                .lock()
                .unwrap()
                .push(request["model"].as_str().unwrap().into());
            self.entered.notify_one();
            if self.hold {
                std::future::pending::<()>().await;
            }
            Ok(
                json!({"model":if self.bad{"wrong-model"}else{&self.binding},"choices":[{"message":{"role":"assistant","content":"synthetic draft"}}]}),
            )
        })
    }
}
fn endpoints(
    catalog: &Catalog,
    log: &Arc<Mutex<Vec<String>>>,
    bad: bool,
    hold: bool,
    entered: Arc<tokio::sync::Notify>,
) -> BTreeMap<String, Arc<dyn Endpoint>> {
    catalog
        .models
        .iter()
        .map(|m| {
            (
                m.id.clone(),
                Arc::new(Fake {
                    binding: m.runtime.model.clone(),
                    log: log.clone(),
                    bad,
                    hold,
                    entered: entered.clone(),
                }) as Arc<dyn Endpoint>,
            )
        })
        .collect()
}
#[tokio::test]
async fn arbitrary_bindings_and_worker_group_execute_sequentially_without_autoload() {
    let f = Fixture::new();
    let c = catalog();
    let log = Arc::new(Mutex::new(vec![]));
    let endpoints = endpoints(&c, &log, false, false, Arc::new(tokio::sync::Notify::new()));
    let p = Pool::configured(
        c.clone(),
        &f.0,
        endpoints,
        Arc::new(Memory(AtomicBool::new(true))),
    )
    .unwrap();
    assert!(log.lock().unwrap().is_empty());
    let text = p
        .bound("brain")
        .complete(
            json!({"model":"runtime-brain","messages":[{"role":"user","content":"synthetic"}]}),
        )
        .await
        .unwrap();
    assert_eq!(text["model"], "runtime-brain");
    let (_tx, rx) = watch::channel(false);
    for _ in 0..3 {
        assert_eq!(
            Workers(p.clone())
                .draft("synthetic".into(), rx.clone())
                .await
                .unwrap(),
            "synthetic draft"
        );
    }
    assert_eq!(
        *log.lock().unwrap(),
        vec![
            "runtime-brain",
            "runtime-worker-a",
            "runtime-worker-b",
            "runtime-worker-a"
        ]
    );
    assert!(!f.0.join("coordination/http-21001.inflight").exists());
    let e = endpoints_for(&c);
    assert!(Pool::configured(c, &f.0, e, Arc::new(Memory(AtomicBool::new(true)))).is_err());
}
fn endpoints_for(c: &Catalog) -> BTreeMap<String, Arc<dyn Endpoint>> {
    endpoints(
        c,
        &Arc::new(Mutex::new(vec![])),
        false,
        false,
        Arc::new(tokio::sync::Notify::new()),
    )
}
#[tokio::test]
async fn unknown_resources_wrong_model_and_ambiguous_completion_never_fallback() {
    let f = Fixture::new();
    let c = catalog();
    let log = Arc::new(Mutex::new(vec![]));
    let memory = Arc::new(Memory(AtomicBool::new(false)));
    let p = Pool::configured(
        c.clone(),
        &f.0,
        endpoints(&c, &log, true, false, Arc::new(tokio::sync::Notify::new())),
        memory.clone(),
    )
    .unwrap();
    assert!(p
        .bound("brain")
        .complete(json!({"model":"runtime-brain"}))
        .await
        .is_err());
    assert!(log.lock().unwrap().is_empty());
    memory.0.store(true, Ordering::Release);
    assert!(p
        .bound("brain")
        .complete(json!({"model":"runtime-brain"}))
        .await
        .is_err());
    assert_eq!(*log.lock().unwrap(), vec!["runtime-brain"]);
    assert!(p.uncertain());
    assert!(f.0.join("coordination/http-21001.inflight").exists());
    assert!(p
        .bound("worker-a")
        .complete(json!({"model":"runtime-worker-a"}))
        .await
        .is_err());
    drop(p);
    let result = Pool::configured(c.clone(), &f.0, endpoints_for(&c), memory);
    assert!(matches!(result,Err(e) if e=="runtime_journal_pending_operator_required"));
    let mut different = c;
    for (i, m) in different.models.iter_mut().enumerate() {
        m.runtime.base_url = format!("http://127.0.0.1:{}/v1", 22001 + i);
    }
    let result = Pool::configured(
        different.clone(),
        &f.0,
        endpoints_for(&different),
        Arc::new(Memory(AtomicBool::new(true))),
    );
    assert!(matches!(result,Err(e) if e=="runtime_journal_pending_operator_required"));
}
#[tokio::test]
async fn drop_mid_post_leaves_journal_and_exclusion_before_releasing_slot() {
    let f = Fixture::new();
    let c = catalog();
    let notify = Arc::new(tokio::sync::Notify::new());
    let p = Pool::configured(
        c.clone(),
        &f.0,
        endpoints(
            &c,
            &Arc::new(Mutex::new(vec![])),
            false,
            true,
            notify.clone(),
        ),
        Arc::new(Memory(AtomicBool::new(true))),
    )
    .unwrap();
    let bound = p.bound("brain");
    let task = tokio::spawn(async move { bound.complete(json!({"model":"runtime-brain"})).await });
    notify.notified().await;
    assert!(!p.idle());
    task.abort();
    let _ = task.await;
    assert!(p.idle());
    assert!(p.uncertain());
    assert!(f.0.join("coordination/http-21001.inflight").exists());
}

struct Controlled {
    binding: String,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl Endpoint for Controlled {
    fn advertises(&self, _: &str) -> Probe<'_> {
        Box::pin(async { Ok(true) })
    }
    fn complete(&self, _: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            self.entered.notify_one();
            self.release.notified().await;
            Ok(
                json!({"model":self.binding,"choices":[{"message":{"role":"assistant","content":"synthetic completed"}}]}),
            )
        })
    }
}
#[tokio::test]
async fn cancellation_waits_for_confirmed_response_instead_of_dropping_http() {
    let f = Fixture::new();
    let c = catalog();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let mut e = endpoints_for(&c);
    e.insert(
        "worker-a".into(),
        Arc::new(Controlled {
            binding: "runtime-worker-a".into(),
            entered: entered.clone(),
            release: release.clone(),
        }),
    );
    let p = Pool::configured(c, &f.0, e, Arc::new(Memory(AtomicBool::new(true)))).unwrap();
    let (cancel, rx) = watch::channel(false);
    let workers = Workers(p.clone());
    let task = tokio::spawn(async move { workers.draft("synthetic".into(), rx).await });
    entered.notified().await;
    cancel.send_replace(true);
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    assert!(!p.idle());
    assert!(f.0.join("coordination/http-21002.inflight").exists());
    release.notify_one();
    assert!(matches!(
        task.await.unwrap(),
        Err(crate::jobs::Failure::Confirmed)
    ));
    assert!(p.idle());
    assert!(!p.uncertain());
    assert!(!f.0.join("coordination/http-21002.inflight").exists());
}
#[tokio::test(start_paused = true)]
async fn timeout_never_unloads_or_reuses_a_model_with_unknown_execution_state() {
    let f = Fixture::new();
    let c = catalog();
    let entered = Arc::new(tokio::sync::Notify::new());
    let p = Pool::configured(
        c.clone(),
        &f.0,
        endpoints(
            &c,
            &Arc::new(Mutex::new(vec![])),
            false,
            true,
            entered.clone(),
        ),
        Arc::new(Memory(AtomicBool::new(true))),
    )
    .unwrap();
    let bound = p.bound("brain");
    let task = tokio::spawn(async move { bound.complete(json!({"model":"runtime-brain"})).await });
    entered.notified().await;
    tokio::time::advance(Duration::from_secs(2)).await;
    assert!(matches!(task.await.unwrap(), Err(TransportError::Timeout)));
    assert!(p.uncertain());
    assert!(f.0.join("coordination/http-21001.inflight").exists());
}
#[tokio::test]
async fn real_transport_fixture_checks_inventory_and_exact_binding_without_network_fallback() {
    use axum::{
        routing::{get, post},
        Json, Router,
    };
    let f = Fixture::new();
    let mut c = catalog();
    c.models.truncate(1);
    c.assignments.workers.clear();
    let listener = loop {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        if !matches!(
            listener.local_addr().unwrap().port(),
            8015 | 8016 | 11434 | 49399 | 49400
        ) {
            break listener;
        }
    };
    let address = listener.local_addr().unwrap();
    c.models[0].runtime.base_url = format!("http://{address}/v1");
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let router=Router::new().route("/v1/models",get(||async{Json(json!({"data":[{"id":"runtime-brain"}]}))}))
        .route("/v1/chat/completions",post(move|Json(body):Json<Value>|{let calls=observed.clone();async move{
            calls.fetch_add(1,Ordering::AcqRel);assert_eq!(body["model"],"runtime-brain");assert_eq!(body["stream"],false);
            Json(json!({"model":"runtime-brain","choices":[{"message":{"role":"assistant","content":"synthetic HTTP"}}]}))
        }}));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut e = BTreeMap::new();
    e.insert(
        "brain".into(),
        Arc::new(Http(HttpTransport::new(&c.models[0].runtime).unwrap())) as Arc<dyn Endpoint>,
    );
    let p = Pool::configured(c, &f.0, e, Arc::new(Memory(AtomicBool::new(true)))).unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 0);
    let response=p.bound("brain").complete(json!({"model":"runtime-brain","messages":[{"role":"user","content":"synthetic fixture"}]})).await.unwrap();
    assert_eq!(
        response["choices"][0]["message"]["content"],
        "synthetic HTTP"
    );
    assert_eq!(calls.load(Ordering::Acquire), 1);
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn unchanged_browser_contract_reaches_an_arbitrary_brain_and_keeps_public_identity() {
    use axum::{
        body::Body,
        http::{header, Request},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let f = Fixture::new();
    let mut c = catalog();
    c.assignments.workers.clear();
    c.models[0].runtime.max_output_tokens = 128;
    c.models[0].max_input_bytes = 4096;
    let log = Arc::new(Mutex::new(vec![]));
    let p = Pool::configured(
        c.clone(),
        &f.0,
        endpoints(&c, &log, false, false, Arc::new(tokio::sync::Notify::new())),
        Arc::new(Memory(AtomicBool::new(true))),
    )
    .unwrap();
    let state = crate::prepared_web::PreparedWeb::local_models(
        c.models[0].runtime.clone(),
        p.bound("brain"),
        Arc::new(Workers(p.clone())),
        p.clone(),
        None,
    )
    .unwrap();
    let app =
        crate::prepared_web::app("127.0.0.1:8015".parse().unwrap(), "a".repeat(32), state).unwrap();
    let body = serde_json::to_vec(
        &json!({"model":"pegassus","messages":[{"role":"user","content":"synthetic browser"}]}),
    )
    .unwrap();
    let request = |authorized: bool| {
        let mut r = Request::post("/api/chat")
            .header(header::HOST, "127.0.0.1:8015")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, body.len());
        if authorized {
            r = r.header(header::AUTHORIZATION, format!("Bearer {}", "a".repeat(32)));
        }
        r.body(Body::from(body.clone())).unwrap()
    };
    assert_eq!(
        app.clone().oneshot(request(false)).await.unwrap().status(),
        401
    );
    assert!(log.lock().unwrap().is_empty());
    let oversized = serde_json::to_vec(
        &json!({"model":"pegassus","messages":[{"role":"user","content":"x".repeat(6000)}]}),
    )
    .unwrap();
    let invalid = Request::post("/api/chat")
        .header(header::HOST, "127.0.0.1:8015")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, oversized.len())
        .header(header::AUTHORIZATION, format!("Bearer {}", "a".repeat(32)))
        .body(Body::from(oversized))
        .unwrap();
    assert_eq!(app.clone().oneshot(invalid).await.unwrap().status(), 400);
    assert!(log.lock().unwrap().is_empty());
    assert!(!p.uncertain());
    let response = app.oneshot(request(true)).await.unwrap();
    assert_eq!(response.status(), 200);
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["model"], "pegassus");
    assert_eq!(*log.lock().unwrap(), vec!["runtime-brain"]);
}
