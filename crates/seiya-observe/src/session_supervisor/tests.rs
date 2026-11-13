use super::*;
use crate::campaign::Pressure;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Copy)]
enum Plan {
    Valid,
    Missing,
    ReusedPid,
    WrongRoot,
    Duplicate,
    Untracked,
    Error,
    Hang,
    Panic,
}
struct Shared {
    plan: Mutex<Plan>,
    lease_fail: AtomicBool,
    probe_fail: AtomicBool,
    probe_hang: AtomicBool,
    held: AtomicBool,
    stops: AtomicUsize,
    samples: AtomicUsize,
    releases: AtomicUsize,
    runtime_drops: AtomicUsize,
    lease_drops: AtomicUsize,
    events: Mutex<Vec<&'static str>>,
}
impl Shared {
    fn new(plan: Plan) -> Arc<Self> {
        Arc::new(Self {
            plan: Mutex::new(plan),
            lease_fail: AtomicBool::new(false),
            probe_fail: AtomicBool::new(false),
            probe_hang: AtomicBool::new(false),
            held: AtomicBool::new(true),
            stops: AtomicUsize::new(0),
            samples: AtomicUsize::new(0),
            releases: AtomicUsize::new(0),
            runtime_drops: AtomicUsize::new(0),
            lease_drops: AtomicUsize::new(0),
            events: Mutex::new(vec![]),
        })
    }
}
fn id(pid: u32, marker: &str) -> ProcessIdentity {
    ProcessIdentity::new(pid, marker).unwrap()
}
fn ownership() -> Ownership {
    Ownership::new(id(41, "root-original"), vec![id(42, "child-original")]).unwrap()
}
struct Runtime(Arc<Shared>, Pressure);
impl Drop for Runtime {
    fn drop(&mut self) {
        self.0.runtime_drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl OwnedRuntime for Runtime {
    fn ownership(&self) -> Ownership {
        ownership()
    }
    fn sample_pressure(&mut self, time: u64) -> OwnedFuture<'_, PressureSample> {
        Box::pin(async move {
            self.0.samples.fetch_add(1, Ordering::SeqCst);
            if self.0.probe_hang.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            if self.0.probe_fail.load(Ordering::SeqCst) {
                return Err(OwnerError::Unsupported);
            }
            Ok(PressureSample {
                sampled_at_millis: time,
                pressure: self.1,
            })
        })
    }
    fn stop_and_reap(&mut self) -> OwnedFuture<'_, ReapEvidence> {
        Box::pin(async move {
            self.0.stops.fetch_add(1, Ordering::SeqCst);
            self.0.events.lock().unwrap().push("stop");
            assert!(
                self.0.held.load(Ordering::SeqCst),
                "lease must precede owned cleanup"
            );
            let plan = *self.0.plan.lock().unwrap();
            let mut proof = ReapEvidence {
                root: id(41, "root-original"),
                reaped: vec![id(42, "child-original"), id(41, "root-original")],
                coverage: Coverage::Complete,
            };
            match plan {
                Plan::Valid => {}
                Plan::Missing => {
                    proof.reaped.pop();
                }
                Plan::ReusedPid => proof.reaped[0] = id(42, "different-process"),
                Plan::WrongRoot => proof.root = id(41, "different-process"),
                Plan::Duplicate => proof.reaped[0] = proof.reaped[1].clone(),
                Plan::Untracked => proof.coverage = Coverage::UntrackedDescendants,
                Plan::Error => return Err(OwnerError::Failed),
                Plan::Hang => std::future::pending::<()>().await,
                Plan::Panic => panic!("injected owner panic"),
            }
            Ok(proof)
        })
    }
}
struct Lease(Arc<Shared>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.lease_drops.fetch_add(1, Ordering::SeqCst);
        // This models why an unverified lease Drop is unsafe: it unlocks.
        self.0.held.store(false, Ordering::SeqCst);
    }
}
impl OwnedLease for Lease {
    fn release(&mut self) -> OwnedFuture<'_, ()> {
        Box::pin(async move {
            self.0.releases.fetch_add(1, Ordering::SeqCst);
            self.0.events.lock().unwrap().push("release");
            if self.0.lease_fail.swap(false, Ordering::SeqCst) {
                return Err(OwnerError::Failed);
            }
            self.0.held.store(false, Ordering::SeqCst);
            Ok(())
        })
    }
}
fn policy() -> Policy {
    Policy::new(
        Duration::from_secs(30),
        Duration::from_millis(20),
        Duration::from_millis(20),
    )
    .unwrap()
}
fn start(shared: &Arc<Shared>, pressure: Pressure) -> (MemoryGuard, Supervisor) {
    let guard = MemoryGuard::new();
    let supervisor = Supervisor::start_injected(
        Runtime(shared.clone(), pressure),
        Lease(shared.clone()),
        guard.clone(),
        policy(),
    );
    (guard, supervisor)
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn model_capable_scope_serialization_does_not_invent_inference_evidence() {
    // Serialization with fake resources ONLY. Not evidence of a native MLX run.
    let shared = Shared::new(Plan::Valid);
    let mut supervisor = Supervisor::start_restricted(
        Runtime(shared.clone(), Pressure::Warning),
        Lease(shared),
        MemoryGuard::new(),
        policy(),
        true,
        true,
    );
    let status = supervisor.wait_terminal().await.unwrap();
    assert_eq!(status.version, 5);
    assert_eq!(status.model_inference, None);
    let wire = serde_json::to_value(status).unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../contracts/benchmarks/chat-session/v0.1.0/restricted-runtime-status.json"
    ))
    .unwrap();
    for key in ["model_inference", "runtime_origin", "containment"] {
        assert_eq!(
            wire[key],
            fixture["model_capable_inference_not_observed"][key]
        );
    }
}

#[test]
fn ownership_and_policy_are_bounded_and_do_not_accept_recycled_duplicate_pids() {
    assert!(Ownership::new(id(1, "a"), vec![id(1, "b")]).is_err());
    assert!(Ownership::new(id(1, "a"), (2..=33).map(|pid| id(pid, "b")).collect()).is_err());
    assert!(Policy::new(
        Duration::ZERO,
        Duration::from_millis(1),
        Duration::from_millis(1)
    )
    .is_err());
    assert!(Policy::new(
        Duration::from_secs(3601),
        Duration::from_millis(1),
        Duration::from_millis(1)
    )
    .is_err());
    assert!(Policy::new(
        Duration::from_secs(1),
        Duration::from_secs(2),
        Duration::from_millis(1)
    )
    .is_err());
    assert!(Policy::new(
        Duration::from_secs(1),
        Duration::from_millis(1),
        Duration::from_secs(11)
    )
    .is_err());
}

#[tokio::test]
async fn pressure_stop_requires_complete_reaping_before_lease_release() {
    let shared = Shared::new(Plan::Valid);
    let (guard, mut supervisor) = start(&shared, Pressure::Warning);
    let status = supervisor.wait_terminal().await.unwrap();
    assert_eq!(status.stage, Stage::Stopped);
    assert_eq!(status.model_inference, Some(false));
    assert_eq!(status.mode, "injected-supervisor");
    assert_eq!(
        status.cause,
        Some(StopCause::Memory(MonitorStop::PressureNotNormal))
    );
    assert!(status.runtime_reaped && status.lease_release_confirmed);
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../contracts/benchmarks/chat-session/v0.1.0/supervisor-status.json"
    ))
    .unwrap();
    assert_eq!(serde_json::to_value(&status).unwrap(), fixture["stopped"]);
    assert_eq!(*shared.events.lock().unwrap(), vec!["stop", "release"]);
    assert!(!shared.held.load(Ordering::SeqCst));
    assert!(guard.check().is_err());
    assert_eq!(
        supervisor.retry_cleanup().await.unwrap_err(),
        Error::NotQuarantined
    );
}

#[tokio::test]
async fn missing_reused_duplicate_wrong_root_and_untracked_evidence_keep_lease() {
    for plan in [
        Plan::Missing,
        Plan::ReusedPid,
        Plan::WrongRoot,
        Plan::Duplicate,
        Plan::Untracked,
    ] {
        let shared = Shared::new(plan);
        let (guard, mut supervisor) = start(&shared, Pressure::Critical);
        let status = supervisor.wait_terminal().await.unwrap();
        assert_eq!(status.stage, Stage::Quarantined);
        assert_eq!(status.cleanup_failure, Some(CleanupFailure::InvalidProof));
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/benchmarks/chat-session/v0.1.0/supervisor-status.json"
        ))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            fixture["quarantined"]
        );
        assert!(!status.runtime_reaped && !status.lease_release_confirmed);
        assert!(shared.held.load(Ordering::SeqCst));
        assert!(guard.check().is_err());
        assert_eq!(shared.releases.load(Ordering::SeqCst), 0);
        assert_eq!(shared.runtime_drops.load(Ordering::SeqCst), 0);
        *shared.plan.lock().unwrap() = Plan::Valid;
        assert_eq!(
            supervisor.retry_cleanup().await.unwrap().stage,
            Stage::Stopped
        );
        assert_eq!(shared.stops.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn owner_failure_and_cleanup_timeout_do_not_unlock_or_retry_automatically() {
    for plan in [Plan::Error, Plan::Hang] {
        let shared = Shared::new(plan);
        let (_, mut supervisor) = start(&shared, Pressure::Unknown);
        let status = supervisor.wait_terminal().await.unwrap();
        assert_eq!(status.stage, Stage::Quarantined);
        assert!(shared.held.load(Ordering::SeqCst));
        assert_eq!(
            status.cleanup_failure,
            Some(if matches!(plan, Plan::Hang) {
                CleanupFailure::TimedOut
            } else {
                CleanupFailure::OwnerFailed
            })
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert_eq!(shared.stops.load(Ordering::SeqCst), 1);
        assert_eq!(shared.releases.load(Ordering::SeqCst), 0);
        *shared.plan.lock().unwrap() = Plan::Valid;
        assert!(
            supervisor
                .retry_cleanup()
                .await
                .unwrap()
                .lease_release_confirmed
        );
    }
}

#[tokio::test]
async fn lease_release_failure_retains_capability_and_never_signals_reaped_pids_again() {
    let shared = Shared::new(Plan::Valid);
    shared.lease_fail.store(true, Ordering::SeqCst);
    let (_, mut supervisor) = start(&shared, Pressure::Warning);
    let status = supervisor.wait_terminal().await.unwrap();
    assert_eq!(status.cleanup_failure, Some(CleanupFailure::LeaseRelease));
    assert!(status.runtime_reaped);
    assert!(!status.lease_release_confirmed);
    assert!(shared.held.load(Ordering::SeqCst));
    // Even changing the runtime to error must not trigger another stop operation.
    *shared.plan.lock().unwrap() = Plan::Error;
    assert_eq!(
        supervisor.retry_cleanup().await.unwrap().stage,
        Stage::Stopped
    );
    assert_eq!(shared.stops.load(Ordering::SeqCst), 1);
    assert_eq!(shared.releases.load(Ordering::SeqCst), 2);
    assert_eq!(
        *shared.events.lock().unwrap(),
        vec!["stop", "release", "release"]
    );
}

#[tokio::test]
async fn cancel_before_first_probe_stops_guard_and_cleans_only_supplied_capabilities() {
    let shared = Shared::new(Plan::Valid);
    let (guard, mut supervisor) = start(&shared, Pressure::Normal);
    supervisor.request_stop();
    let status = supervisor.wait_terminal().await.unwrap();
    assert_eq!(status.cause, Some(StopCause::Cancelled));
    assert_eq!(status.stage, Stage::Stopped);
    assert_eq!(shared.samples.load(Ordering::SeqCst), 0);
    assert_eq!(
        guard.check(),
        Err(Block::Stopped(MonitorStop::SupervisorStopped))
    );
}

#[tokio::test]
async fn failed_and_timed_out_probes_close_admission_and_trigger_owned_cleanup() {
    for hang in [false, true] {
        let shared = Shared::new(Plan::Valid);
        if hang {
            shared.probe_hang.store(true, Ordering::SeqCst);
        } else {
            shared.probe_fail.store(true, Ordering::SeqCst);
        }
        let (guard, mut supervisor) = start(&shared, Pressure::Normal);
        let status = supervisor.wait_terminal().await.unwrap();
        assert_eq!(status.cause, Some(StopCause::OwnerFailed));
        assert_eq!(status.stage, Stage::Stopped);
        assert_eq!(guard.check(), Err(Block::Stopped(MonitorStop::Unavailable)));
        assert_eq!(shared.samples.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn session_deadline_covers_idle_without_http_requests() {
    let shared = Shared::new(Plan::Valid);
    let guard = MemoryGuard::new();
    let policy = Policy::new(
        Duration::from_secs(1),
        Duration::from_millis(20),
        Duration::from_millis(20),
    )
    .unwrap();
    let mut supervisor = Supervisor::start_injected(
        Runtime(shared.clone(), Pressure::Normal),
        Lease(shared.clone()),
        guard.clone(),
        policy,
    );
    let status = supervisor.wait_terminal().await.unwrap();
    assert_eq!(status.cause, Some(StopCause::Deadline));
    assert_eq!(status.stage, Stage::Stopped);
    assert!(guard.check().is_err());
    assert!(!shared.held.load(Ordering::SeqCst));
}

#[tokio::test]
async fn consumer_drop_requests_cleanup_but_never_drops_an_unconfirmed_lease() {
    let shared = Shared::new(Plan::Hang);
    let (guard, supervisor) = start(&shared, Pressure::Normal);
    drop(supervisor);
    tokio::time::timeout(Duration::from_secs(1), async {
        while shared.stops.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert!(guard.check().is_err());
    assert!(shared.held.load(Ordering::SeqCst));
    assert_eq!(shared.lease_drops.load(Ordering::SeqCst), 0);
    assert_eq!(shared.runtime_drops.load(Ordering::SeqCst), 0);
    // Intentional bounded fake-capability retention: no native resource/FD exists.
    // Executor teardown must fail closed too, not run the fake lease's Drop.
}

#[tokio::test]
async fn retries_are_operator_driven_and_bounded_without_new_loads() {
    let shared = Shared::new(Plan::Missing);
    let (_, mut supervisor) = start(&shared, Pressure::Warning);
    assert_eq!(supervisor.wait_terminal().await.unwrap().attempts, 1);
    assert_eq!(supervisor.retry_cleanup().await.unwrap().attempts, 2);
    assert_eq!(supervisor.retry_cleanup().await.unwrap().attempts, 3);
    assert_eq!(
        supervisor.retry_cleanup().await.unwrap_err(),
        Error::AttemptsExhausted
    );
    assert_eq!(shared.stops.load(Ordering::SeqCst), 3);
    assert!(shared.held.load(Ordering::SeqCst));
    assert_eq!(shared.releases.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn owner_panic_cannot_report_success_or_unlock_unconfirmed_resources() {
    let shared = Shared::new(Plan::Panic);
    let (guard, mut supervisor) = start(&shared, Pressure::Warning);
    assert_eq!(
        supervisor.wait_terminal().await.unwrap_err(),
        Error::TaskEnded
    );
    assert!(guard.check().is_err());
    assert!(shared.held.load(Ordering::SeqCst));
    assert_eq!(shared.lease_drops.load(Ordering::SeqCst), 0);
    assert_eq!(shared.releases.load(Ordering::SeqCst), 0);
}

#[test]
fn executor_shutdown_cannot_unlock_an_unconfirmed_lease() {
    let shared = Shared::new(Plan::Hang);
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    executor.block_on(async {
        let (_, mut supervisor) = start(&shared, Pressure::Warning);
        assert_eq!(
            supervisor.wait_terminal().await.unwrap().stage,
            Stage::Quarantined
        );
    });
    drop(executor);
    assert!(shared.held.load(Ordering::SeqCst));
    assert_eq!(shared.lease_drops.load(Ordering::SeqCst), 0);
    assert_eq!(shared.runtime_drops.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cancelling_recovery_waiter_does_not_cancel_or_repeat_owned_cleanup() {
    let shared = Shared::new(Plan::Missing);
    let (_, mut supervisor) = start(&shared, Pressure::Warning);
    assert_eq!(supervisor.wait_terminal().await.unwrap().attempts, 1);
    *shared.plan.lock().unwrap() = Plan::Hang;
    assert!(
        tokio::time::timeout(Duration::from_millis(1), supervisor.retry_cleanup())
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(1), async {
        while supervisor.status().attempts < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let status = supervisor.wait_terminal().await.unwrap();
    assert_eq!(status.attempts, 2);
    assert_eq!(status.stage, Stage::Quarantined);
    assert!(shared.held.load(Ordering::SeqCst));
    *shared.plan.lock().unwrap() = Plan::Valid;
    assert_eq!(
        supervisor.retry_cleanup().await.unwrap().stage,
        Stage::Stopped
    );
    assert_eq!(shared.stops.load(Ordering::SeqCst), 3);
}

struct FailedClock;
impl crate::session_guard::Clock for FailedClock {
    fn now_millis(&self) -> Option<u64> {
        panic!("injected clock panic")
    }
}
#[tokio::test]
async fn teardown_closes_guard_without_reentering_a_failed_clock() {
    let shared = Shared::new(Plan::Valid);
    let guard = MemoryGuard::with_clock(Arc::new(FailedClock));
    let mut supervisor = Supervisor::start_injected(
        Runtime(shared.clone(), Pressure::Normal),
        Lease(shared.clone()),
        guard.clone(),
        policy(),
    );
    assert_eq!(
        supervisor.wait_terminal().await.unwrap_err(),
        Error::TaskEnded
    );
    // Panic happened under the policy mutex: poison is unavailable evidence,
    // not a known clean shutdown. Crucially, the failed clock is not called again.
    assert_eq!(guard.check(), Err(Block::Stopped(MonitorStop::Unavailable)));
    assert!(shared.held.load(Ordering::SeqCst));
    assert_eq!(shared.lease_drops.load(Ordering::SeqCst), 0);
}
