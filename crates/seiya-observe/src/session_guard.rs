//! Supplemental stop-only pressure policy. Never load authorization or a probe.
use crate::campaign::{LoadMonitor, MonitorStop, PressureSample, PRESSURE_MAX_AGE_MILLIS};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Trusted clock seam; timestamps must share this monotonic origin.
pub trait Clock: Send + Sync {
    fn now_millis(&self) -> Option<u64>;
}
struct MonotonicClock(Instant);
impl Clock for MonotonicClock {
    fn now_millis(&self) -> Option<u64> {
        self.0.elapsed().as_millis().try_into().ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    AwaitingObservation,
    Stopped(MonitorStop),
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub version: u8,
    pub scope: &'static str,
    pub state: &'static str,
    pub stop: Option<MonitorStop>,
    pub sampled_at_millis: Option<u64>,
    pub preflight: &'static str,
}
#[derive(Default)]
struct State {
    monitor: LoadMonitor,
    last_now: Option<u64>,
    stop_at_millis: Option<u64>,
}

/// Host-owned injection only. No Deserialize, model inputs, reset or process IDs.
#[derive(Clone)]
pub struct MemoryGuard {
    state: Arc<Mutex<State>>,
    clock: Arc<dyn Clock>,
    stopped: watch::Sender<bool>,
}
impl Default for MemoryGuard {
    fn default() -> Self {
        Self::new()
    }
}
impl MemoryGuard {
    pub fn new() -> Self {
        Self::with_clock(Arc::new(MonotonicClock(Instant::now())))
    }
    pub fn with_clock(clock: Arc<dyn Clock>) -> Self {
        let (stopped, _) = watch::channel(false);
        Self {
            state: Arc::new(Mutex::new(State::default())),
            clock,
            stopped,
        }
    }
    pub fn now_millis(&self) -> Option<u64> {
        self.clock.now_millis()
    }

    /// Host supervisor can only stop, never grant/rearm. Does not consult a
    /// potentially failed clock during cleanup or unwinding.
    pub fn stop_supervisor(&self) -> Block {
        match self.state.lock() {
            Ok(mut state) => self.fail(&mut state, MonitorStop::SupervisorStopped),
            Err(_) => self.poisoned(),
        }
    }

    /// Trusted host deadline in this guard's clock. Can only shorten lifetime;
    /// no grant, reset or rearm. Every admission/dispatch/observation checks it.
    pub fn stop_at_millis(&self, deadline: u64) -> Result<(), Block> {
        let mut state = self.state.lock().map_err(|_| self.poisoned())?;
        let now = self.clock(&mut state)?;
        state.stop_at_millis = Some(
            state
                .stop_at_millis
                .map_or(deadline, |old| old.min(deadline)),
        );
        if state.stop_at_millis.is_some_and(|end| now >= end) {
            return Err(self.fail(&mut state, MonitorStop::SupervisorStopped));
        }
        Ok(())
    }

    fn fail(&self, state: &mut State, cause: MonitorStop) -> Block {
        let cause = state.monitor.fail(cause).unwrap_err();
        self.stopped.send_replace(true);
        Block::Stopped(cause)
    }
    fn clock(&self, state: &mut State) -> Result<u64, Block> {
        if let Some(stop) = state.monitor.stop {
            return Err(Block::Stopped(stop));
        }
        let now = self
            .clock
            .now_millis()
            .ok_or_else(|| self.fail(state, MonitorStop::Unavailable))?;
        if state.last_now.is_some_and(|previous| now < previous) {
            return Err(self.fail(state, MonitorStop::InvalidClock));
        }
        state.last_now = Some(now);
        if state.stop_at_millis.is_some_and(|end| now >= end) {
            return Err(self.fail(state, MonitorStop::SupervisorStopped));
        }
        Ok(now)
    }
    fn poisoned(&self) -> Block {
        self.stopped.send_replace(true);
        Block::Stopped(MonitorStop::Unavailable)
    }

    /// Supplied by an approved observer, not an HTTP flag. Does not read the host.
    pub fn observe(&self, sample: Result<PressureSample, MonitorStop>) -> Result<(), Block> {
        let mut state = self.state.lock().map_err(|_| self.poisoned())?;
        let now = self.clock(&mut state)?;
        let sample = sample.map_err(|cause| self.fail(&mut state, cause))?;
        state
            .monitor
            .check(sample, now)
            .map_err(|cause| self.fail(&mut state, cause))
    }

    /// Ok only means this supplemental guard has not stopped work. All other
    /// authorization, memory-budget, reservation and admission gates still apply.
    pub fn check(&self) -> Result<(), Block> {
        let mut state = self.state.lock().map_err(|_| self.poisoned())?;
        let now = self.clock(&mut state)?;
        let Some(sample) = &state.monitor.last else {
            return Err(Block::AwaitingObservation);
        };
        if sample.sampled_at_millis > now {
            return Err(self.fail(&mut state, MonitorStop::InvalidClock));
        }
        if now - sample.sampled_at_millis > PRESSURE_MAX_AGE_MILLIS {
            return Err(self.fail(&mut state, MonitorStop::StaleSample));
        }
        Ok(())
    }

    /// No host IO. May expire a cached observation using the monotonic clock.
    pub fn status(&self) -> Status {
        let result = self.check();
        let sampled_at_millis = self.state.lock().ok().and_then(|state| {
            state
                .monitor
                .last
                .as_ref()
                .map(|sample| sample.sampled_at_millis)
        });
        let (state, stop) = match result {
            Ok(()) => ("monitoring", None),
            Err(Block::AwaitingObservation) => ("awaiting_observation", None),
            Err(Block::Stopped(cause)) => ("stopped", Some(cause)),
        };
        Status {
            version: 1,
            scope: "stop_only_pressure",
            state,
            stop,
            sampled_at_millis,
            preflight: "external_required",
        }
    }

    /// Wake immediately on observed stop; catch missing/stale probes every250ms
    /// only while this future is polled. No autonomous monitor or native cleanup.
    pub async fn wait_blocked(&self) -> Block {
        let mut stopped = self.stopped.subscribe();
        loop {
            if let Err(block) = self.check() {
                return block;
            }
            tokio::select! {
                _ = stopped.changed() => {},
                _ = tokio::time::sleep(Duration::from_millis(250)) => {},
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::campaign::Pressure;
    use std::sync::atomic::{AtomicU64, Ordering};
    struct Manual(AtomicU64);
    impl Clock for Manual {
        fn now_millis(&self) -> Option<u64> {
            let now = self.0.load(Ordering::SeqCst);
            (now != u64::MAX).then_some(now)
        }
    }
    fn sample(time: u64, pressure: Pressure) -> PressureSample {
        PressureSample {
            sampled_at_millis: time,
            pressure,
        }
    }
    #[test]
    fn no_sample_is_not_permission_and_normal_pressure_does_not_claim_capacity() {
        let clock = Arc::new(Manual(AtomicU64::new(0)));
        let guard = MemoryGuard::with_clock(clock);
        assert_eq!(guard.check(), Err(Block::AwaitingObservation));
        assert_eq!(guard.status().preflight, "external_required");
        guard.observe(Ok(sample(0, Pressure::Normal))).unwrap();
        assert!(guard.check().is_ok());
        assert_eq!(guard.status().scope, "stop_only_pressure");
    }

    #[test]
    fn host_deadline_only_shortens_and_fresh_pressure_never_rearms_it() {
        let clock = Arc::new(Manual(AtomicU64::new(0)));
        let guard = MemoryGuard::with_clock(clock.clone());
        guard.stop_at_millis(100).unwrap();
        assert_eq!(guard.check(), Err(Block::AwaitingObservation));
        guard.observe(Ok(sample(0, Pressure::Normal))).unwrap();
        guard.stop_at_millis(50).unwrap();
        guard.stop_at_millis(200).unwrap();
        clock.0.store(49, Ordering::SeqCst);
        assert!(guard.check().is_ok());
        clock.0.store(50, Ordering::SeqCst);
        assert_eq!(
            guard.check(),
            Err(Block::Stopped(MonitorStop::SupervisorStopped))
        );
        assert!(guard.observe(Ok(sample(50, Pressure::Normal))).is_err());
        assert!(guard.stop_at_millis(1000).is_err());
    }
    #[test]
    fn pressure_failure_is_terminal_even_if_a_new_normal_sample_arrives() {
        for pressure in [Pressure::Warning, Pressure::Critical, Pressure::Unknown] {
            let guard = MemoryGuard::with_clock(Arc::new(Manual(AtomicU64::new(0))));
            guard.observe(Ok(sample(0, Pressure::Normal))).unwrap();
            let stopped = Err(Block::Stopped(MonitorStop::PressureNotNormal));
            assert_eq!(guard.observe(Ok(sample(0, pressure))), stopped);
            assert_eq!(guard.observe(Ok(sample(0, Pressure::Normal))), stopped);
            assert_eq!(guard.check(), stopped);
        }
    }
    #[test]
    fn stale_observations_replayed_samples_and_probe_errors_stop_work() {
        let clock = Arc::new(Manual(AtomicU64::new(0)));
        let guard = MemoryGuard::with_clock(clock.clone());
        guard.observe(Ok(sample(0, Pressure::Normal))).unwrap();
        clock.0.store(2000, Ordering::SeqCst);
        assert!(guard.check().is_ok());
        // A replay cannot extend freshness beyond the original sample's time.
        guard.observe(Ok(sample(0, Pressure::Normal))).unwrap();
        clock.0.store(2001, Ordering::SeqCst);
        assert_eq!(guard.check(), Err(Block::Stopped(MonitorStop::StaleSample)));
        let guard = MemoryGuard::new();
        assert_eq!(
            guard.observe(Err(MonitorStop::Unavailable)),
            Err(Block::Stopped(MonitorStop::Unavailable))
        );
    }
    #[test]
    fn future_regressing_or_missing_clocks_and_sampling_gaps_fail_closed() {
        for (first, next, sampled, expected) in [
            (1000, 999, 999, MonitorStop::InvalidClock),
            (0, 5001, 5001, MonitorStop::SamplingGap),
            (0, 0, 1, MonitorStop::InvalidClock),
            (0, u64::MAX, 0, MonitorStop::Unavailable),
        ] {
            let clock = Arc::new(Manual(AtomicU64::new(first)));
            let guard = MemoryGuard::with_clock(clock.clone());
            guard.observe(Ok(sample(first, Pressure::Normal))).unwrap();
            clock.0.store(next, Ordering::SeqCst);
            assert_eq!(
                guard.observe(Ok(sample(sampled, Pressure::Normal))),
                Err(Block::Stopped(expected))
            );
        }
    }
    #[tokio::test]
    async fn waiting_monitor_is_woken_by_stop_without_a_host_probe() {
        let guard = MemoryGuard::with_clock(Arc::new(Manual(AtomicU64::new(0))));
        guard.observe(Ok(sample(0, Pressure::Normal))).unwrap();
        let running = guard.clone();
        let task = tokio::spawn(async move { running.wait_blocked().await });
        let _ = guard.observe(Ok(sample(0, Pressure::Warning)));
        assert_eq!(
            task.await.unwrap(),
            Block::Stopped(MonitorStop::PressureNotNormal)
        );
    }
}
