//! Operator configuration, never model/browser job arguments or approval.
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TrialLimits {
    pub version: u32,
    pub web_port: u16,
    pub preflight_min_free_mib: u64,
    pub preflight_max_swap_growth_mib: u64,
    pub max_swap_growth_mib: u64,
    pub warning_budget_ms: u64,
    pub max_session_seconds: u64,
}
impl Default for TrialLimits {
    fn default() -> Self {
        Self {
            version: 1,
            web_port: 8011,
            preflight_min_free_mib: 0,
            preflight_max_swap_growth_mib: 32,
            max_swap_growth_mib: 512,
            warning_budget_ms: 0,
            max_session_seconds: 1800,
        }
    }
}
impl TrialLimits {
    pub fn parse(raw: &[u8]) -> Result<Self, String> {
        if raw.len() > 4096 {
            return Err("trial_limits_size".into());
        }
        let p: Self =
            serde_json::from_slice(raw).map_err(|_| "trial_limits_invalid_json_or_fields")?;
        if p.version != 1
            || p.web_port < 1024
            || matches!(p.web_port, 8010 | 11434)
            || p.preflight_min_free_mib > 8192
            || p.preflight_max_swap_growth_mib > 128
            || !(64..=2048).contains(&p.max_swap_growth_mib)
            || p.warning_budget_ms > 10000
            || !(60..=1800).contains(&p.max_session_seconds)
        {
            return Err("trial_limits_out_of_bounds".into());
        }
        Ok(p)
    }
    pub fn read(path: &std::path::Path) -> Result<Self, String> {
        let m = path
            .symlink_metadata()
            .map_err(|_| "trial_limits_metadata")?;
        if !m.is_file() || m.file_type().is_symlink() || m.len() > 4096 {
            return Err("trial_limits_regular_small_file_required".into());
        }
        use std::io::Read;
        let mut raw = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| "trial_limits_open")?
            .take(4097)
            .read_to_end(&mut raw)
            .map_err(|_| "trial_limits_read")?;
        Self::parse(&raw)
    }
}

/// Charges the entire interval at either warning endpoint, never resets at
/// normal pressure. Critical/unknown/stale readings always stop the trial.
pub struct PressureWindow {
    previous_ms: u64,
    previous_pressure: u64,
    pub warning_used_ms: u64,
}
impl PressureWindow {
    pub fn new(now: u64) -> Self {
        Self {
            previous_ms: now,
            previous_pressure: 1,
            warning_used_ms: 0,
        }
    }
    pub fn observe(&mut self, now: u64, sample: &Value, baseline: u64, p: &TrialLimits) -> bool {
        let Some(delta) = now.checked_sub(self.previous_ms).filter(|n| *n <= 1000) else {
            return false;
        };
        let Some(pressure) = sample["pressure"].as_u64().filter(|n| *n == 1 || *n == 2) else {
            return false;
        };
        if pressure == 2 || self.previous_pressure == 2 {
            let Some(used) = self.warning_used_ms.checked_add(delta.max(1)) else {
                return false;
            };
            self.warning_used_ms = used;
        }
        self.previous_ms = now;
        self.previous_pressure = pressure;
        self.warning_used_ms <= p.warning_budget_ms
            && sample["swap_bytes"]
                .as_u64()
                .is_some_and(|s| s.saturating_sub(baseline) <= p.max_swap_growth_mib * 1024 * 1024)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn config_is_bounded_closed_and_does_not_silently_disable_monitor() {
        let example = TrialLimits::parse(include_bytes!(
            "../../../../contracts/benchmarks/background-jobs/v0.1.0/web-trial-limits.example.json"
        ))
        .unwrap();
        assert_eq!(example.warning_budget_ms, 0);
        assert_eq!(example.preflight_min_free_mib, 0);
        let p = TrialLimits::parse(br#"{"preflight_min_free_mib":0,"warning_budget_ms":1000}"#)
            .unwrap();
        assert_eq!(p.max_swap_growth_mib, 512);
        for raw in [
            br#"{"warning_budget_ms":10001}"#.as_slice(),
            br#"{"max_swap_growth_mib":0}"#,
            br#"{"allow_critical":true}"#,
            br#"{"version":2}"#,
            br#"{"web_port":8010}"#,
            br#"{"web_port":11434}"#,
            br#"{"web_port":0}"#,
            br#"{"warning_budget_ms":1,"warning_budget_ms":2}"#,
        ] {
            assert!(TrialLimits::parse(raw).is_err());
        }
    }
    #[test]
    fn pressure_window_accumulates_warning_and_never_allows_critical_or_unknown() {
        let p = TrialLimits {
            warning_budget_ms: 500,
            ..TrialLimits::default()
        };
        let mut w = PressureWindow::new(0);
        assert!(w.observe(100, &json!({"pressure":2,"swap_bytes":0}), 0, &p));
        assert!(w.observe(300, &json!({"pressure":1,"swap_bytes":0}), 0, &p));
        assert_eq!(w.warning_used_ms, 300);
        assert!(!w.observe(600, &json!({"pressure":2,"swap_bytes":0}), 0, &p));
        for v in [
            json!({"pressure":4,"swap_bytes":0}),
            json!({"pressure":null,"swap_bytes":0}),
            json!({"pressure":1,"swap_bytes":513*1024*1024}),
        ] {
            assert!(!PressureWindow::new(0).observe(100, &v, 0, &TrialLimits::default()));
        }
        assert!(!PressureWindow::new(0).observe(
            1001,
            &json!({"pressure":1,"swap_bytes":0}),
            0,
            &p
        ));
        assert!(!PressureWindow::new(0).observe(
            1,
            &json!({"pressure":2,"swap_bytes":0}),
            0,
            &TrialLimits::default()
        ));
    }
}
