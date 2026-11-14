//! Sequential owned-runtime lifecycle, not a VRAM measurement or remote unload.
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Role {
    Brain,
    Worker,
    Image,
}
impl Role {
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Brain => 1,
            Self::Image => 2,
            Self::Worker => 3,
        }
    }
    pub(crate) fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Brain),
            2 => Some(Self::Image),
            3 => Some(Self::Worker),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Idle,
    Preparing,
    Loading,
    Running,
    Unloading,
    Stopped,
    Quarantined,
}
#[derive(Clone, Copy, Serialize)]
pub(crate) struct Snapshot {
    pub phase: Phase,
    pub role: Option<Role>,
    pub admission: bool,
    pub owned_cleanup_confirmed: bool,
    pub physical_memory_release_measured: bool,
}
struct State {
    phase: Phase,
    role: Option<Role>,
    enabled: bool,
    cleanup: bool,
}
pub(crate) struct Driver {
    state: Mutex<State>,
    slot: Arc<Semaphore>,
}
impl Driver {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                phase: Phase::Idle,
                role: None,
                enabled: true,
                cleanup: true,
            }),
            slot: Arc::new(Semaphore::new(1)),
        })
    }
    pub(crate) fn snapshot(&self) -> Snapshot {
        let s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        Snapshot {
            phase: s.phase,
            role: s.role,
            admission: s.enabled && s.phase == Phase::Idle,
            owned_cleanup_confirmed: s.cleanup,
            physical_memory_release_measured: false,
        }
    }
    pub(crate) fn role_code(&self) -> u8 {
        self.snapshot().role.map_or(0, Role::code)
    }
    pub(crate) fn idle(&self) -> bool {
        self.slot.available_permits() == 1
    }
    pub(crate) fn stop(&self) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        s.enabled = false;
        if s.role.is_none() && s.phase != Phase::Quarantined {
            s.phase = Phase::Stopped;
        }
        self.slot.close();
    }
    pub(crate) fn quarantine(&self) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        s.enabled = false;
        s.phase = Phase::Quarantined;
        s.cleanup = false;
        self.slot.close();
    }
    pub(crate) async fn acquire(self: &Arc<Self>, role: Role) -> Result<Lease, ()> {
        let permit = self.slot.clone().acquire_owned().await.map_err(|_| ())?;
        let mut s = self.state.lock().map_err(|_| ())?;
        if !s.enabled || s.phase != Phase::Idle {
            return Err(());
        }
        s.phase = Phase::Preparing;
        s.role = Some(role);
        s.cleanup = false;
        Ok(Lease {
            driver: self.clone(),
            _permit: permit,
            finished: false,
        })
    }
}
pub(crate) struct Lease {
    driver: Arc<Driver>,
    _permit: OwnedSemaphorePermit,
    finished: bool,
}
impl Lease {
    pub(crate) fn advance(&self, to: Phase) -> Result<(), ()> {
        let mut s = self.driver.state.lock().map_err(|_| ())?;
        let valid = matches!(
            (s.phase, to),
            (Phase::Preparing, Phase::Loading)
                | (Phase::Loading, Phase::Running)
                | (
                    Phase::Preparing | Phase::Loading | Phase::Running,
                    Phase::Unloading
                )
        );
        if !valid {
            drop(s);
            self.driver.quarantine();
            return Err(());
        }
        s.phase = to;
        Ok(())
    }
    /// Host-owned process adapter must confirm closure; this is never inferred
    /// from a response body, timeout or model-authored tool result.
    pub(crate) fn confirm_cleanup(mut self) {
        let mut s = self.driver.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.phase != Phase::Quarantined {
            if !matches!(s.phase, Phase::Preparing | Phase::Unloading) {
                drop(s);
                self.driver.quarantine();
                return;
            }
            s.cleanup = true;
            s.role = None;
            s.phase = if s.enabled {
                Phase::Idle
            } else {
                Phase::Stopped
            };
        }
        self.finished = true;
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if !self.finished {
            self.driver.quarantine();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn brain_worker_and_image_require_cleanup_before_switching() {
        let d = Driver::new();
        assert!(d.snapshot().owned_cleanup_confirmed);
        for role in [Role::Brain, Role::Worker, Role::Image] {
            let turn = d.acquire(role).await.unwrap();
            assert_eq!(d.role_code(), role.code());
            turn.advance(Phase::Loading).unwrap();
            turn.advance(Phase::Running).unwrap();
            let next = d.clone();
            let wait = tokio::spawn(async move { next.acquire(Role::Brain).await });
            tokio::task::yield_now().await;
            assert!(!wait.is_finished());
            assert!(!d.snapshot().owned_cleanup_confirmed);
            turn.advance(Phase::Unloading).unwrap();
            assert!(!wait.is_finished());
            turn.confirm_cleanup();
            wait.await.unwrap().unwrap().confirm_cleanup();
            assert!(d.idle());
            assert_eq!(d.snapshot().phase, Phase::Idle);
        }
    }
    #[tokio::test]
    async fn stop_drains_owned_work_without_claiming_unload_and_unknown_is_terminal() {
        let d = Driver::new();
        let turn = d.acquire(Role::Image).await.unwrap();
        turn.advance(Phase::Loading).unwrap();
        d.stop();
        assert!(!d.idle());
        assert!(!d.snapshot().owned_cleanup_confirmed);
        assert!(d.acquire(Role::Brain).await.is_err());
        turn.advance(Phase::Unloading).unwrap();
        turn.confirm_cleanup();
        assert!(d.snapshot().owned_cleanup_confirmed);
        assert_eq!(d.snapshot().phase, Phase::Stopped);
        let d = Driver::new();
        let turn = d.acquire(Role::Brain).await.unwrap();
        drop(turn);
        assert_eq!(d.snapshot().phase, Phase::Quarantined);
        assert!(d.acquire(Role::Image).await.is_err());
        assert!(!d.snapshot().physical_memory_release_measured);
    }
    #[tokio::test]
    async fn invalid_transition_or_premature_confirmation_cannot_report_idle() {
        let d = Driver::new();
        let turn = d.acquire(Role::Brain).await.unwrap();
        assert!(turn.advance(Phase::Running).is_err());
        turn.confirm_cleanup();
        assert_eq!(d.snapshot().phase, Phase::Quarantined);
        let d = Driver::new();
        let turn = d.acquire(Role::Worker).await.unwrap();
        turn.advance(Phase::Loading).unwrap();
        turn.confirm_cleanup();
        assert_eq!(d.snapshot().phase, Phase::Quarantined);
    }
    #[tokio::test]
    async fn cancelling_a_queued_request_does_not_release_or_quarantine_the_active_model() {
        let driver = Driver::new();
        let active = driver.acquire(Role::Brain).await.unwrap();
        active.advance(Phase::Loading).unwrap();
        let d = driver.clone();
        let queued = tokio::spawn(async move { d.acquire(Role::Image).await });
        tokio::task::yield_now().await;
        queued.abort();
        let _ = queued.await;
        assert!(!driver.idle());
        assert_eq!(driver.snapshot().phase, Phase::Loading);
        active.advance(Phase::Unloading).unwrap();
        active.confirm_cleanup();
        assert!(driver.snapshot().admission);
        driver.acquire(Role::Image).await.unwrap().confirm_cleanup();
    }
}
