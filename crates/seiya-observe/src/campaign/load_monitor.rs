//! Pure stop-only watchdog. Normal pressure is not a capacity/admission signal.
use super::Pressure;
use serde::Serialize;

pub const PRESSURE_MAX_AGE_MILLIS: u64 = 2000;

#[derive(Debug, Clone, Serialize)]
pub struct PressureSample {
    pub sampled_at_millis: u64,
    pub pressure: Pressure,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorStop {
    SupervisorStopped,
    PressureNotNormal,
    InvalidClock,
    StaleSample,
    SamplingGap,
    Unavailable,
}
#[derive(Debug, Default, Serialize)]
pub struct LoadMonitor {
    pub last: Option<PressureSample>,
    pub stop: Option<MonitorStop>,
    pub samples_checked: u64,
}
impl LoadMonitor {
    pub fn check(&mut self, sample: PressureSample, now: u64) -> Result<(), MonitorStop> {
        if let Some(stop) = self.stop {
            return Err(stop);
        }
        let failure = if sample.sampled_at_millis > now {
            Some(MonitorStop::InvalidClock)
        } else if now - sample.sampled_at_millis > PRESSURE_MAX_AGE_MILLIS {
            Some(MonitorStop::StaleSample)
        } else if let Some(previous) = &self.last {
            if sample.sampled_at_millis < previous.sampled_at_millis {
                Some(MonitorStop::InvalidClock)
            } else if sample.sampled_at_millis - previous.sampled_at_millis > 5000 {
                Some(MonitorStop::SamplingGap)
            } else {
                None
            }
        } else {
            None
        };
        self.samples_checked = self.samples_checked.saturating_add(1);
        let failure = failure.or_else(|| {
            (sample.pressure != Pressure::Normal).then_some(MonitorStop::PressureNotNormal)
        });
        self.last = Some(sample);
        if let Some(stop) = failure {
            return self.fail(stop);
        }
        Ok(())
    }
    pub fn fail(&mut self, stop: MonitorStop) -> Result<(), MonitorStop> {
        let stop = *self.stop.get_or_insert(stop);
        Err(stop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(time: u64, pressure: Pressure) -> PressureSample {
        PressureSample {
            sampled_at_millis: time,
            pressure,
        }
    }
    #[test]
    fn monitor_is_stop_only_and_terminal_even_if_pressure_recovers() {
        for pressure in [Pressure::Warning, Pressure::Critical, Pressure::Unknown] {
            let mut monitor = LoadMonitor::default();
            assert!(monitor.check(sample(0, Pressure::Normal), 0).is_ok());
            assert_eq!(
                monitor.check(sample(1000, pressure), 1000),
                Err(MonitorStop::PressureNotNormal)
            );
            assert_eq!(
                monitor.check(sample(2000, Pressure::Normal), 2000),
                Err(MonitorStop::PressureNotNormal)
            );
            assert_eq!(monitor.samples_checked, 2);
        }
    }
    #[test]
    fn clock_staleness_gaps_and_probe_failure_cannot_be_hidden() {
        for (time, now, expected) in [
            (1000, 999, MonitorStop::InvalidClock),
            (0, 2001, MonitorStop::StaleSample),
        ] {
            assert_eq!(
                LoadMonitor::default().check(sample(time, Pressure::Normal), now),
                Err(expected)
            );
        }
        let mut monitor = LoadMonitor::default();
        monitor.check(sample(1000, Pressure::Normal), 1000).unwrap();
        assert_eq!(
            monitor.check(sample(999, Pressure::Normal), 1000),
            Err(MonitorStop::InvalidClock)
        );
        let mut monitor = LoadMonitor::default();
        monitor.check(sample(0, Pressure::Normal), 0).unwrap();
        assert_eq!(
            monitor.check(sample(5001, Pressure::Normal), 5001),
            Err(MonitorStop::SamplingGap)
        );
        let mut monitor = LoadMonitor::default();
        assert_eq!(
            monitor.fail(MonitorStop::Unavailable),
            Err(MonitorStop::Unavailable)
        );
        assert_eq!(
            monitor.check(sample(0, Pressure::Normal), 0),
            Err(MonitorStop::Unavailable)
        );
    }
}
