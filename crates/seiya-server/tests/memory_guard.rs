//! Host observations/runtime behavior are injected. No host probes or models.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use seiya_observe::{
    campaign::{MonitorStop, Pressure, PressureSample},
    session_guard::{Clock, MemoryGuard},
};
use seiya_server::{
    app_with_scheduler,
    inference::InferenceAdapter,
    runtime::{RuntimeConfig, RuntimeFuture, RuntimeTransport, TransportError},
    scheduler::Scheduler,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tokio::sync::Semaphore;
use tower::ServiceExt;

struct Manual(AtomicU64);
impl Clock for Manual {
    fn now_millis(&self) -> Option<u64> {
        Some(self.0.load(Ordering::SeqCst))
    }
}
struct Fake {
    calls: AtomicUsize,
    blocked: bool,
    entered: Semaphore,
    release: Semaphore,
    result: Mutex<Result<Value, TransportError>>,
}
impl Fake {
    fn new(blocked: bool) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            blocked,
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            result: Mutex::new(Ok(
                json!({"choices":[{"message":{"role":"assistant","content":"synthetic"},"finish_reason":"stop"}]}),
            )),
        })
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}
impl RuntimeTransport for Fake {
    fn complete(&self, _: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.add_permits(1);
            if self.blocked {
                self.release.acquire().await.unwrap().forget();
            }
            self.result.lock().unwrap().clone()
        })
    }
}
fn cfg() -> RuntimeConfig {
    RuntimeConfig::parse(br#"{"id":"fake","model":"synthetic","backend":"mlx","base_url":"http://127.0.0.1:65535/v1","timeout_seconds":1,"max_output_tokens":32}"#).unwrap()
}
fn input() -> Value {
    json!({"messages":[{"role":"user","content":"synthetic"}]})
}
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../contracts/openai-compat/v0.1.0/rust-memory-guard.json"
    ))
    .unwrap()
}
fn setup(blocked: bool) -> (Arc<Manual>, MemoryGuard, Arc<Fake>, Scheduler) {
    let clock = Arc::new(Manual(AtomicU64::new(0)));
    let memory = MemoryGuard::with_clock(clock.clone());
    let fake = Fake::new(blocked);
    let adapter = InferenceAdapter::new_with_runtime_status(cfg(), fake.clone()).unwrap();
    let scheduler =
        Scheduler::new_with_memory_guard(adapter, Duration::from_secs(1), memory.clone()).unwrap();
    (clock, memory, fake, scheduler)
}
fn observe(memory: &MemoryGuard, time: u64, pressure: Pressure) {
    let _ = memory.observe(Ok(PressureSample {
        sampled_at_millis: time,
        pressure,
    }));
}
async fn admitted(s: &Scheduler, count: usize) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while s.admitted_requests() != count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn attaching_guard_requires_opt_in_and_adds_no_json_permission() {
    let adapter = InferenceAdapter::new(cfg(), Fake::new(false)).unwrap();
    assert!(
        Scheduler::new_with_memory_guard(adapter, Duration::from_secs(1), MemoryGuard::new())
            .is_err()
    );
    assert!(seiya_server::config::Config::parse(
        br#"{"listen":"127.0.0.1:0","memory_guard":true}"#
    )
    .is_err());
}

#[tokio::test]
async fn awaiting_observation_rejects_without_slot_and_health_makes_no_capacity_claim() {
    let (_, memory, fake, s) = setup(false);
    let failure = s.complete(input()).await.unwrap_err();
    assert_eq!(failure.body(), fixture()["awaiting"]["body"]);
    assert_eq!(s.admitted_requests(), 0);
    assert_eq!(fake.calls(), 0);
    let app = app_with_scheduler(
        "127.0.0.1:9998".parse().unwrap(),
        None,
        s.clone(),
        Duration::from_secs(1),
    )
    .unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .header("host", "127.0.0.1:9998")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        body["runtime_status"]["memory_admission"],
        fixture()["initial"]
    );
    assert_eq!(
        body["runtime_status"]["version"],
        fixture()["runtime_status_version"]
    );
    assert_eq!(body["runtime_status"]["admission"], "blocked");
    assert_eq!(body["runtime_status"]["model_residency"], "unknown");
    assert_eq!(fake.calls(), 0);
    observe(&memory, 0, Pressure::Normal);
    s.complete(input()).await.unwrap();
    assert_eq!(fake.calls(), 1);
}

#[tokio::test]
async fn pressure_unknown_failure_and_staleness_never_reach_transport_or_rearm() {
    for cause in 0..5 {
        let (clock, memory, fake, s) = setup(false);
        observe(&memory, 0, Pressure::Normal);
        match cause {
            0 => observe(&memory, 0, Pressure::Warning),
            1 => observe(&memory, 0, Pressure::Critical),
            2 => observe(&memory, 0, Pressure::Unknown),
            3 => {
                let _ = memory.observe(Err(MonitorStop::Unavailable));
            }
            _ => clock.0.store(2001, Ordering::SeqCst),
        }
        assert_eq!(
            s.complete(input()).await.unwrap_err().body(),
            fixture()["stopped"]["body"]
        );
        observe(&memory, clock.0.load(Ordering::SeqCst), Pressure::Normal);
        assert_eq!(
            s.complete(input()).await.unwrap_err().body(),
            fixture()["stopped"]["body"]
        );
        assert_eq!(fake.calls(), 0);
        assert_eq!(s.admitted_requests(), 0);
    }
}

#[tokio::test]
async fn pressure_stop_wakes_waiter_but_holds_active_slot_until_backend_finishes() {
    let (_, memory, fake, s) = setup(true);
    observe(&memory, 0, Pressure::Normal);
    let run = s.clone();
    let active = tokio::spawn(async move { run.complete(input()).await });
    fake.entered.acquire().await.unwrap().forget();
    let run = s.clone();
    let queued = tokio::spawn(async move { run.complete(input()).await });
    admitted(&s, 2).await;
    observe(&memory, 0, Pressure::Warning);
    assert_eq!(
        queued.await.unwrap().unwrap_err().body(),
        fixture()["stopped"]["body"]
    );
    assert_eq!(s.admitted_requests(), 1);
    assert!(s.is_busy());
    assert!(!active.is_finished());
    assert_eq!(fake.calls(), 1);
    fake.release.add_permits(1);
    assert_eq!(
        active.await.unwrap().unwrap_err().body(),
        fixture()["stopped"]["body"]
    );
    assert_eq!(s.admitted_requests(), 0);
    assert!(!s.requires_recovery());
    assert_eq!(s.runtime_status().unwrap()["admission"], "blocked");
}

#[tokio::test]
async fn stale_sample_is_detected_while_waiting_and_suppresses_late_success() {
    let (clock, memory, fake, s) = setup(true);
    observe(&memory, 0, Pressure::Normal);
    let run = s.clone();
    let active = tokio::spawn(async move { run.complete(input()).await });
    fake.entered.acquire().await.unwrap().forget();
    let run = s.clone();
    let queued = tokio::spawn(async move { run.complete(input()).await });
    admitted(&s, 2).await;
    clock.0.store(2001, Ordering::SeqCst);
    assert_eq!(
        queued.await.unwrap().unwrap_err().code,
        Some("runtime_memory_blocked")
    );
    assert_eq!(memory.status().stop, Some(MonitorStop::StaleSample));
    assert!(!active.is_finished());
    fake.release.add_permits(1);
    assert_eq!(
        active.await.unwrap().unwrap_err().code,
        Some("runtime_memory_blocked")
    );
    assert_eq!(fake.calls(), 1);
}

#[tokio::test]
async fn consumer_abort_and_stop_do_not_release_the_active_lease_or_fake_cleanup() {
    let (_, memory, fake, s) = setup(true);
    observe(&memory, 0, Pressure::Normal);
    let run = s.clone();
    let consumer = tokio::spawn(async move { run.complete(input()).await });
    fake.entered.acquire().await.unwrap().forget();
    consumer.abort();
    let _ = consumer.await;
    observe(&memory, 0, Pressure::Warning);
    assert_eq!(s.admitted_requests(), 1);
    assert!(s.is_busy());
    let run = s.clone();
    let drain = tokio::spawn(async move { run.close_and_drain().await });
    tokio::task::yield_now().await;
    assert!(!drain.is_finished());
    fake.release.add_permits(1);
    drain.await.unwrap().unwrap();
    assert_eq!(s.admitted_requests(), 0);
    assert_eq!(fake.calls(), 1);
    assert_eq!(s.runtime_status().unwrap()["admission"], "closed");
}

#[tokio::test]
async fn timeout_during_memory_stop_still_keeps_backend_state_uncertain() {
    let (_, memory, fake, s) = setup(true);
    observe(&memory, 0, Pressure::Normal);
    let run = s.clone();
    let active = tokio::spawn(async move { run.complete(input()).await });
    fake.entered.acquire().await.unwrap().forget();
    observe(&memory, 0, Pressure::Warning);
    assert_eq!(
        active.await.unwrap().unwrap_err().code,
        Some("runtime_memory_blocked")
    );
    assert!(s.requires_recovery());
    assert!(s.is_busy());
    assert_eq!(fake.calls(), 1);
    assert!(s.close_and_drain().await.is_err());
}

#[tokio::test]
async fn fresh_observations_preserve_serial_fifo_execution() {
    let (_, memory, fake, s) = setup(true);
    observe(&memory, 0, Pressure::Normal);
    let run = s.clone();
    let first = tokio::spawn(async move { run.complete(input()).await });
    fake.entered.acquire().await.unwrap().forget();
    let run = s.clone();
    let second = tokio::spawn(async move { run.complete(input()).await });
    admitted(&s, 2).await;
    fake.release.add_permits(1);
    first.await.unwrap().unwrap();
    fake.entered.acquire().await.unwrap().forget();
    assert_eq!(fake.calls(), 2);
    fake.release.add_permits(1);
    second.await.unwrap().unwrap();
    assert_eq!(s.admitted_requests(), 0);
}

#[tokio::test]
async fn queue_deadline_with_fresh_observation_does_not_dispatch_another_generation() {
    let memory = MemoryGuard::with_clock(Arc::new(Manual(AtomicU64::new(0))));
    observe(&memory, 0, Pressure::Normal);
    let fake = Fake::new(true);
    let adapter = InferenceAdapter::new_with_runtime_status(cfg(), fake.clone()).unwrap();
    let s = Scheduler::new_with_memory_guard(adapter, Duration::from_millis(20), memory).unwrap();
    let run = s.clone();
    let active = tokio::spawn(async move { run.complete(input()).await });
    fake.entered.acquire().await.unwrap().forget();
    assert_eq!(
        s.complete(input()).await.unwrap_err().status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(fake.calls(), 1);
    assert_eq!(s.admitted_requests(), 1);
    fake.release.add_permits(1);
    active.await.unwrap().unwrap();
    assert_eq!(s.admitted_requests(), 0);
}

struct FailBeforeDispatch(AtomicUsize);
impl Clock for FailBeforeDispatch {
    fn now_millis(&self) -> Option<u64> {
        (self.0.fetch_add(1, Ordering::SeqCst) < 2).then_some(0)
    }
}
#[tokio::test]
async fn guard_failure_between_admission_and_first_generation_poll_dispatches_nothing() {
    let clock = Arc::new(FailBeforeDispatch(AtomicUsize::new(0)));
    let memory = MemoryGuard::with_clock(clock.clone());
    observe(&memory, 0, Pressure::Normal);
    clock.0.store(0, Ordering::SeqCst);
    let fake = Fake::new(false);
    let adapter = InferenceAdapter::new_with_runtime_status(cfg(), fake.clone()).unwrap();
    let s = Scheduler::new_with_memory_guard(adapter, Duration::from_secs(1), memory).unwrap();
    let failure = s.complete(input()).await.unwrap_err();
    assert_eq!(failure.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(fake.calls(), 0);
    assert_eq!(s.admitted_requests(), 0);
}
