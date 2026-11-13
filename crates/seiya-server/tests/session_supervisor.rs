//! Whole chain with injected ownership, pressure, reaping and lease. No OS
//! process/lock, native probe, model, network, or real runtime cancellation.
use seiya_observe::{
    campaign::{Pressure, PressureSample},
    session_guard::MemoryGuard,
    session_supervisor::{
        Coverage, OwnedFuture, OwnedLease, OwnedRuntime, Ownership, Policy, ReapEvidence, Stage,
        Supervisor,
    },
    ProcessIdentity,
};
use seiya_server::{
    inference::InferenceAdapter,
    runtime::{RuntimeConfig, RuntimeFuture, RuntimeTransport, TransportError},
    scheduler::Scheduler,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::Semaphore;

struct Shared {
    entered: Semaphore,
    finished: Semaphore,
    complete: AtomicBool,
    root_reaped: AtomicBool,
    child_reaped: AtomicBool,
    lease_held: AtomicBool,
    calls: AtomicUsize,
    stops: AtomicUsize,
    root_signals: AtomicUsize,
    releases: AtomicUsize,
}
struct Backend(Arc<Shared>);
impl RuntimeTransport for Backend {
    fn complete(&self, _: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            self.0.calls.fetch_add(1, Ordering::SeqCst);
            self.0.entered.add_permits(1);
            self.0.finished.acquire().await.unwrap().forget();
            Err(TransportError::Unavailable)
        })
    }
}
fn root() -> ProcessIdentity {
    ProcessIdentity::new(501, "owned-root-fixture").unwrap()
}
fn child() -> ProcessIdentity {
    ProcessIdentity::new(502, "owned-child-fixture").unwrap()
}
struct Runtime(Arc<Shared>);
impl OwnedRuntime for Runtime {
    fn ownership(&self) -> Ownership {
        Ownership::new(root(), vec![child()]).unwrap()
    }
    fn sample_pressure(&mut self, time: u64) -> OwnedFuture<'_, PressureSample> {
        Box::pin(async move {
            Ok(PressureSample {
                sampled_at_millis: time,
                pressure: Pressure::Normal,
            })
        })
    }
    fn stop_and_reap(&mut self) -> OwnedFuture<'_, ReapEvidence> {
        Box::pin(async move {
            self.0.stops.fetch_add(1, Ordering::SeqCst);
            assert!(self.0.lease_held.load(Ordering::SeqCst));
            if !self.0.root_reaped.swap(true, Ordering::SeqCst) {
                self.0.root_signals.fetch_add(1, Ordering::SeqCst);
                self.0.finished.add_permits(1);
            }
            let complete = self.0.complete.load(Ordering::SeqCst);
            self.0.child_reaped.store(complete, Ordering::SeqCst);
            Ok(ReapEvidence {
                root: root(),
                reaped: if complete {
                    vec![root(), child()]
                } else {
                    vec![root()]
                },
                coverage: if complete {
                    Coverage::Complete
                } else {
                    Coverage::UntrackedDescendants
                },
            })
        })
    }
}
struct Lease(Arc<Shared>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.lease_held.store(false, Ordering::SeqCst);
    }
}
impl OwnedLease for Lease {
    fn release(&mut self) -> OwnedFuture<'_, ()> {
        Box::pin(async move {
            assert!(self.0.root_reaped.load(Ordering::SeqCst));
            assert!(self.0.child_reaped.load(Ordering::SeqCst));
            self.0.releases.fetch_add(1, Ordering::SeqCst);
            self.0.lease_held.store(false, Ordering::SeqCst);
            Ok(())
        })
    }
}
fn input() -> Value {
    json!({"messages":[{"role":"user","content":"synthetic"}]})
}

#[tokio::test]
async fn pressure_stop_joins_queue_guard_owned_cleanup_and_lease_without_fake_recovery() {
    for complete in [true, false] {
        let shared = Arc::new(Shared {
            entered: Semaphore::new(0),
            finished: Semaphore::new(0),
            complete: AtomicBool::new(complete),
            root_reaped: AtomicBool::new(false),
            child_reaped: AtomicBool::new(false),
            lease_held: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            root_signals: AtomicUsize::new(0),
            releases: AtomicUsize::new(0),
        });
        let guard = MemoryGuard::new();
        let policy = Policy::new(
            Duration::from_secs(30),
            Duration::from_millis(100),
            Duration::from_millis(100),
        )
        .unwrap();
        let mut supervisor = Supervisor::start_injected(
            Runtime(shared.clone()),
            Lease(shared.clone()),
            guard.clone(),
            policy,
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            while guard.check().is_err() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let cfg=RuntimeConfig::parse(br#"{"id":"fake","model":"synthetic","backend":"mlx","base_url":"http://127.0.0.1:65535/v1","timeout_seconds":2,"max_output_tokens":32}"#).unwrap();
        let adapter =
            InferenceAdapter::new_with_runtime_status(cfg, Arc::new(Backend(shared.clone())))
                .unwrap();
        let s = Scheduler::new_with_memory_guard(adapter, Duration::from_secs(1), guard.clone())
            .unwrap();
        let run = s.clone();
        let active = tokio::spawn(async move { run.complete(input()).await });
        shared.entered.acquire().await.unwrap().forget();
        let run = s.clone();
        let queued = tokio::spawn(async move { run.complete(input()).await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while s.admitted_requests() != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let _ = guard.observe(Ok(PressureSample {
            sampled_at_millis: guard.now_millis().unwrap(),
            pressure: Pressure::Warning,
        }));
        let status = supervisor.wait_terminal().await.unwrap();
        assert_eq!(
            active.await.unwrap().unwrap_err().code,
            Some("runtime_memory_blocked")
        );
        assert_eq!(
            queued.await.unwrap().unwrap_err().code,
            Some("runtime_memory_blocked")
        );
        assert_eq!(shared.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            status.stage,
            if complete {
                Stage::Stopped
            } else {
                Stage::Quarantined
            }
        );
        assert_eq!(shared.lease_held.load(Ordering::SeqCst), !complete);
        assert_eq!(
            shared.releases.load(Ordering::SeqCst),
            usize::from(complete)
        );
        // A reaping proof does not silently reset the HTTP adapter's uncertainty.
        assert!(s.requires_recovery());
        assert!(s.close_and_drain().await.is_err());
        if !complete {
            shared.complete.store(true, Ordering::SeqCst);
            assert_eq!(
                supervisor.retry_cleanup().await.unwrap().stage,
                Stage::Stopped
            );
            assert_eq!(shared.root_signals.load(Ordering::SeqCst), 1);
            assert_eq!(shared.stops.load(Ordering::SeqCst), 2);
            assert_eq!(shared.releases.load(Ordering::SeqCst), 1);
        }
        assert!(!shared.lease_held.load(Ordering::SeqCst));
    }
}
