//! Owned-capability supervisor for injected and audited native fixture rehearsals.
//! No model load authorization, general runtime adapter or HTTP control surface.
use crate::campaign::{MonitorStop, PressureSample};
use crate::session_guard::{Block, MemoryGuard};
use crate::{ProcessIdentity, MAX_TARGETS};
use serde::Serialize;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};

const MAX_ATTEMPTS: u8 = 3;
pub type OwnedFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, OwnerError>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerError {
    Failed,
    IdentityChanged,
    Unsupported,
    ClockUnavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidOwnership,
    InvalidPolicy,
    NotQuarantined,
    RecoveryBusy,
    AttemptsExhausted,
    TaskEnded,
}

/// A trusted producer must establish these identities from original owned
/// handles. This validates shape/uniqueness, not OS ownership or authorization.
#[derive(Debug, Clone)]
pub struct Ownership {
    root: ProcessIdentity,
    members: Vec<ProcessIdentity>,
}
impl Ownership {
    pub fn new(
        root: ProcessIdentity,
        mut descendants: Vec<ProcessIdentity>,
    ) -> Result<Self, Error> {
        if descendants.len() >= MAX_TARGETS {
            return Err(Error::InvalidOwnership);
        }
        descendants.insert(0, root.clone());
        let mut pids = HashSet::new();
        if descendants.iter().any(|id| !pids.insert(id.pid())) {
            return Err(Error::InvalidOwnership);
        }
        Ok(Self {
            root,
            members: descendants,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    Complete,
    UntrackedDescendants,
}
pub struct ReapEvidence {
    pub root: ProcessIdentity,
    pub reaped: Vec<ProcessIdentity>,
    pub coverage: Coverage,
}
impl ReapEvidence {
    fn validates(&self, expected: &Ownership) -> bool {
        if self.root != expected.root
            || self.coverage != Coverage::Complete
            || self.reaped.len() != expected.members.len()
        {
            return false;
        }
        let mut pids = HashSet::new();
        self.reaped
            .iter()
            .all(|id| pids.insert(id.pid()) && expected.members.contains(id))
    }
}

/// Injection seam, not permission to kill supplied PIDs. Implementations must
/// retain original handles, verify identity and enumerate all owned descendants.
pub trait OwnedRuntime: Send + 'static {
    /// Immutable cached manifest, no IO; native ownership must be proved earlier.
    fn ownership(&self) -> Ownership;
    fn sample_pressure(&mut self, sampled_at_millis: u64) -> OwnedFuture<'_, PressureSample>;
    fn stop_and_reap(&mut self) -> OwnedFuture<'_, ReapEvidence>;
}
/// Release must be cancellation-safe/idempotent and never unlink a lock file.
/// It must not yield after irreversible release but before returning success.
/// NativeSessionLease owns the actual shared coordination reservation; a runtime
/// native adapter is still absent. Origin labels that mixed evidence explicitly.
pub trait OwnedLease: Send + 'static {
    /// Cached evidence scope only, not a replacement for cleanup/proof.
    fn origin(&self) -> LeaseOrigin {
        LeaseOrigin::Injected
    }
    fn release(&mut self) -> OwnedFuture<'_, ()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseOrigin {
    Injected,
    NativeCoordinationLock,
}

#[derive(Clone, Copy)]
pub struct Policy {
    session: Duration,
    probe: Duration,
    cleanup: Duration,
}
impl Policy {
    pub fn new(session: Duration, probe: Duration, cleanup: Duration) -> Result<Self, Error> {
        if !(Duration::from_secs(1)..=Duration::from_secs(3600)).contains(&session)
            || !(Duration::from_millis(1)..=Duration::from_secs(1)).contains(&probe)
            || !(Duration::from_millis(1)..=Duration::from_secs(10)).contains(&cleanup)
        {
            return Err(Error::InvalidPolicy);
        }
        Ok(Self {
            session,
            probe,
            cleanup,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Starting,
    Monitoring,
    Stopping,
    Quarantined,
    Stopped,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopCause {
    Memory(MonitorStop),
    Cancelled,
    Deadline,
    OwnerFailed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupFailure {
    OwnerFailed,
    InvalidProof,
    TimedOut,
    LeaseRelease,
}
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub version: u8,
    pub mode: &'static str,
    pub model_inference: Option<bool>,
    pub stage: Stage,
    pub cause: Option<StopCause>,
    pub cleanup_failure: Option<CleanupFailure>,
    pub attempts: u8,
    pub owned_members: usize,
    pub runtime_reaped: bool,
    pub lease_release_confirmed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_origin: Option<LeaseOrigin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_origin: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressure_origin: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub containment: Option<&'static str>,
}

struct NativeScope {
    runtime: &'static str,
    pressure: &'static str,
    restricted: bool,
    model: bool,
}
impl Status {
    fn terminal(&self) -> bool {
        matches!(self.stage, Stage::Quarantined | Stage::Stopped)
    }
}

struct Resources<R, L> {
    runtime: Option<R>,
    lease: Option<L>,
    guard: MemoryGuard,
    runtime_reaped: bool,
    lease_released: bool,
}
impl<R, L> Drop for Resources<R, L> {
    fn drop(&mut self) {
        self.guard.stop_supervisor();
        if !self.lease_released {
            // Fail closed even if the executor destroys this actor or a trusted
            // adapter panics. Deliberate last-resort retention until process exit;
            // never invoke unverified Drop that could unlock or kill by stale PID.
            if let Some(runtime) = self.runtime.take() {
                std::mem::forget(runtime);
            }
            if let Some(lease) = self.lease.take() {
                std::mem::forget(lease);
            }
        }
    }
}
impl<R: OwnedRuntime, L: OwnedLease> Resources<R, L> {
    async fn cleanup(
        &mut self,
        ownership: &Ownership,
        deadline: Duration,
    ) -> Result<(), CleanupFailure> {
        let attempt = async {
            if !self.runtime_reaped {
                let proof = self
                    .runtime
                    .as_mut()
                    .unwrap()
                    .stop_and_reap()
                    .await
                    .map_err(|_| CleanupFailure::OwnerFailed)?;
                if !proof.validates(ownership) {
                    return Err(CleanupFailure::InvalidProof);
                }
                self.runtime_reaped = true;
            }
            self.lease
                .as_mut()
                .unwrap()
                .release()
                .await
                .map_err(|_| CleanupFailure::LeaseRelease)?;
            self.lease_released = true;
            Ok(())
        };
        tokio::time::timeout(deadline, attempt)
            .await
            .unwrap_or(Err(CleanupFailure::TimedOut))
    }
}

/// Drop requests stop; the actor and its capabilities are not aborted. In
/// quarantine they remain retained even with no consumer. No automatic retry.
#[must_use]
pub struct Supervisor {
    stop: watch::Sender<bool>,
    recovery: mpsc::Sender<()>,
    status: watch::Receiver<Status>,
    // Never expose abort authority to an HTTP consumer.
    _task: JoinHandle<()>,
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}
impl Supervisor {
    /// Runtime/pressure injection; native fixture launch is separate. Native
    /// coordination leases use status version2/origin. No model runtime producer. Caller
    /// must already own resources and approval/preflight.
    pub fn start_injected<R: OwnedRuntime, L: OwnedLease>(
        runtime: R,
        lease: L,
        guard: MemoryGuard,
        policy: Policy,
    ) -> Self {
        Self::start_scoped(runtime, lease, guard, policy, None)
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn start_native_fixture<R: OwnedRuntime, L: OwnedLease>(
        runtime: R,
        lease: L,
        guard: MemoryGuard,
        policy: Policy,
        native_pressure: bool,
    ) -> Self {
        Self::start_scoped(
            runtime,
            lease,
            guard,
            policy,
            Some(NativeScope {
                runtime: "native_fixture_child",
                pressure: if native_pressure {
                    "native_macos_sysctl"
                } else {
                    "injected_fixture"
                },
                restricted: false,
                model: false,
            }),
        )
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn start_restricted<R: OwnedRuntime, L: OwnedLease>(
        runtime: R,
        lease: L,
        guard: MemoryGuard,
        policy: Policy,
        model: bool,
        native_pressure: bool,
    ) -> Self {
        Self::start_scoped(
            runtime,
            lease,
            guard,
            policy,
            Some(NativeScope {
                runtime: if model {
                    "native_restricted_mlx_child"
                } else {
                    "native_restricted_fixture_child"
                },
                pressure: if native_pressure {
                    "native_macos_sysctl"
                } else {
                    "injected_fixture"
                },
                restricted: true,
                model,
            }),
        )
    }

    fn start_scoped<R: OwnedRuntime, L: OwnedLease>(
        runtime: R,
        lease: L,
        guard: MemoryGuard,
        policy: Policy,
        scope: Option<NativeScope>,
    ) -> Self {
        let resources = Resources {
            runtime: Some(runtime),
            lease: Some(lease),
            guard,
            runtime_reaped: false,
            lease_released: false,
        };
        let ownership = resources.runtime.as_ref().unwrap().ownership();
        let origin = resources.lease.as_ref().unwrap().origin();
        let lease_origin = (origin != LeaseOrigin::Injected).then_some(origin);
        let initial = Status {
            version: if let Some(scope) = &scope {
                if scope.restricted {
                    5
                } else if scope.pressure == "native_macos_sysctl" {
                    4
                } else {
                    3
                }
            } else if lease_origin.is_some() {
                2
            } else {
                1
            },
            mode: if let Some(scope) = &scope {
                if scope.restricted {
                    "native-restricted-supervisor"
                } else {
                    "native-fixture-supervisor"
                }
            } else {
                "injected-supervisor"
            },
            model_inference: if scope.as_ref().is_some_and(|s| s.model) {
                None
            } else {
                Some(false)
            },
            stage: Stage::Starting,
            cause: None,
            cleanup_failure: None,
            attempts: 0,
            owned_members: ownership.members.len(),
            runtime_reaped: false,
            lease_release_confirmed: false,
            lease_origin,
            runtime_origin: scope.as_ref().map(|s| s.runtime),
            pressure_origin: scope.as_ref().map(|s| s.pressure),
            containment: scope
                .as_ref()
                .and_then(|s| s.restricted.then_some("macos_process_fork_denied_v1")),
        };
        let (status_tx, status) = watch::channel(initial);
        let (stop, stop_rx) = watch::channel(false);
        let (recovery, recovery_rx) = mpsc::channel(1);
        let deadline = Instant::now() + policy.session;
        let task = tokio::spawn(run(
            resources,
            ownership,
            policy,
            deadline,
            stop_rx,
            recovery_rx,
            status_tx,
        ));
        Self {
            stop,
            recovery,
            status,
            _task: task,
        }
    }
    pub fn status(&self) -> Status {
        self.status.borrow().clone()
    }
    pub fn request_stop(&self) {
        self.stop.send_replace(true);
    }

    pub async fn wait_terminal(&mut self) -> Result<Status, Error> {
        loop {
            let status = self.status();
            if status.terminal() {
                return Ok(status);
            }
            self.status.changed().await.map_err(|_| Error::TaskEnded)?;
        }
    }
    /// Explicit operator-owned cleanup retry only. Never restarts inference.
    pub async fn retry_cleanup(&mut self) -> Result<Status, Error> {
        let previous = self.status();
        if previous.stage != Stage::Quarantined {
            return Err(Error::NotQuarantined);
        }
        if previous.attempts >= MAX_ATTEMPTS {
            return Err(Error::AttemptsExhausted);
        }
        self.recovery
            .try_send(())
            .map_err(|_| Error::RecoveryBusy)?;
        loop {
            let status = self.status();
            if status.attempts > previous.attempts && status.terminal() {
                return Ok(status);
            }
            self.status.changed().await.map_err(|_| Error::TaskEnded)?;
        }
    }
}

async fn run<R: OwnedRuntime, L: OwnedLease>(
    mut resources: Resources<R, L>,
    ownership: Ownership,
    policy: Policy,
    deadline: Instant,
    mut stop: watch::Receiver<bool>,
    mut recovery: mpsc::Receiver<()>,
    status_tx: watch::Sender<Status>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut watch_guard = !matches!(resources.guard.check(), Err(Block::AwaitingObservation));
    let cause = loop {
        if *stop.borrow() {
            break StopCause::Cancelled;
        }
        let observation = async {
            interval.tick().await;
            let now = resources
                .guard
                .now_millis()
                .ok_or(OwnerError::ClockUnavailable)?;
            tokio::time::timeout(
                policy.probe,
                resources.runtime.as_mut().unwrap().sample_pressure(now),
            )
            .await
            .unwrap_or(Err(OwnerError::Failed))
        };
        tokio::select! {
            biased;
            _ = stop.changed() => break StopCause::Cancelled,
            _ = tokio::time::sleep_until(deadline) => break StopCause::Deadline,
            block = resources.guard.wait_blocked(), if watch_guard => {
                break match block { Block::Stopped(cause) => StopCause::Memory(cause),
                    Block::AwaitingObservation => StopCause::Memory(MonitorStop::Unavailable) };
            }
            result = observation => {
                match result {
                    Ok(sample) => match resources.guard.observe(Ok(sample)) {
                        Ok(()) => {
                            watch_guard = true;
                            status_tx.send_modify(|status| status.stage = Stage::Monitoring);
                        }
                        Err(Block::Stopped(cause)) => break StopCause::Memory(cause),
                        Err(Block::AwaitingObservation) => break StopCause::OwnerFailed,
                    },
                    Err(_) => {
                        let _ = resources.guard.observe(Err(MonitorStop::Unavailable));
                        break StopCause::OwnerFailed;
                    }
                }
            }
        }
    };
    resources.guard.stop_supervisor();
    let mut attempts = 0;
    loop {
        attempts += 1;
        status_tx.send_modify(|status| {
            status.stage = Stage::Stopping;
            status.cause = Some(cause);
            status.attempts = attempts;
            status.cleanup_failure = None;
        });
        let result = resources.cleanup(&ownership, policy.cleanup).await;
        status_tx.send_modify(|status| {
            status.stage = if result.is_ok() {
                Stage::Stopped
            } else {
                Stage::Quarantined
            };
            status.runtime_reaped = resources.runtime_reaped;
            status.lease_release_confirmed = resources.lease_released;
            status.cleanup_failure = result.err();
        });
        if resources.lease_released {
            return;
        }
        // No automatic retries, no busy loop when every consumer disappeared.
        if attempts >= MAX_ATTEMPTS || recovery.recv().await.is_none() {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests;
