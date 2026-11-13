//! Diagnostic pressure review only. Never an alternative load-admission gate.
use super::{HostObservation, Pressure};
use serde::Serialize;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewFinding {
    InvalidSampleCount,
    InvalidClock,
    StaleObservation,
    WindowTooShort,
    SamplingGap,
    MemoryUnavailable,
    PhysicalMemoryChanged,
    PressureNotNormal,
    ExternalRuntimeNotCleared,
}

#[derive(Debug, Serialize)]
pub struct AdmissionReview {
    pub mode: &'static str,
    pub findings: Vec<ReviewFinding>,
    pub sample_count: usize,
    pub window_millis: Option<u64>,
    pub normal_pressure_observed: bool,
    pub free_pages_min_bytes: Option<u64>,
    pub free_pages_max_bytes: Option<u64>,
    pub safe_additional_allocation_bytes: Option<u64>,
    pub runtime_budget_validated: bool,
    pub model_load_authorized: bool,
    pub active_admission_policy_changed: bool,
}

/// Samples are caller-supplied observations, not proof of exclusive GPU access.
/// Low free pages do not invalidate normal pressure, nor permit a model load.
pub fn review_pressure_window(samples: &[HostObservation], now_millis: u64) -> AdmissionReview {
    let mut report = AdmissionReview {
        mode: "diagnostic-only",
        findings: vec![],
        sample_count: samples.len(),
        window_millis: None,
        normal_pressure_observed: false,
        free_pages_min_bytes: None,
        free_pages_max_bytes: None,
        safe_additional_allocation_bytes: None,
        runtime_budget_validated: false,
        model_load_authorized: false,
        active_admission_policy_changed: false,
    };
    if !(3..=16).contains(&samples.len()) {
        report.findings.push(ReviewFinding::InvalidSampleCount);
        return report;
    }
    let mut finding = |value| {
        if !report.findings.contains(&value) {
            report.findings.push(value);
        }
    };
    let first = &samples[0];
    let last = &samples[samples.len() - 1];
    match last.sampled_at_millis.checked_sub(first.sampled_at_millis) {
        Some(span) => {
            report.window_millis = Some(span);
            if span < 2000 {
                finding(ReviewFinding::WindowTooShort);
            }
        }
        None => finding(ReviewFinding::InvalidClock),
    }
    match now_millis.checked_sub(last.sampled_at_millis) {
        Some(age) if age <= 5000 => {}
        Some(_) => finding(ReviewFinding::StaleObservation),
        None => finding(ReviewFinding::InvalidClock),
    }
    for pair in samples.windows(2) {
        match pair[1]
            .sampled_at_millis
            .checked_sub(pair[0].sampled_at_millis)
        {
            None | Some(0) => finding(ReviewFinding::InvalidClock),
            Some(gap) if gap > 2000 => finding(ReviewFinding::SamplingGap),
            _ => {}
        }
    }
    for sample in samples {
        if sample.sampled_at_millis > now_millis {
            finding(ReviewFinding::InvalidClock);
        }
        if sample.physical_bytes != first.physical_bytes {
            finding(ReviewFinding::PhysicalMemoryChanged);
        }
        match (sample.physical_bytes, sample.free_bytes) {
            (Some(total), Some(free)) if total > 0 && free <= total => {
                report.free_pages_min_bytes = Some(
                    report
                        .free_pages_min_bytes
                        .map_or(free, |min| min.min(free)),
                );
                report.free_pages_max_bytes = Some(
                    report
                        .free_pages_max_bytes
                        .map_or(free, |max| max.max(free)),
                );
            }
            _ => finding(ReviewFinding::MemoryUnavailable),
        }
        if sample.pressure != Pressure::Normal {
            finding(ReviewFinding::PressureNotNormal);
        }
        if !sample.external_runtime_clearance {
            finding(ReviewFinding::ExternalRuntimeNotCleared);
        }
    }
    if report.findings.contains(&ReviewFinding::MemoryUnavailable) {
        // Do not silently report a range from only the successful subset.
        report.free_pages_min_bytes = None;
        report.free_pages_max_bytes = None;
    }
    report.normal_pressure_observed = report.findings.is_empty();
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    fn samples() -> Vec<HostObservation> {
        [0, 1000, 2000]
            .into_iter()
            .map(|sampled_at_millis| HostObservation {
                sampled_at_millis,
                physical_bytes: Some(16 << 30),
                free_bytes: Some(80 << 20),
                pressure: Pressure::Normal,
                external_runtime_clearance: true,
            })
            .collect()
    }
    #[test]
    fn normal_pressure_with_low_free_pages_is_not_memory_capacity_or_authorization() {
        let report = review_pressure_window(&samples(), 2000);
        assert!(report.normal_pressure_observed);
        assert!(report.findings.is_empty());
        assert_eq!(report.window_millis, Some(2000));
        assert_eq!(report.free_pages_min_bytes, Some(80 << 20));
        assert_eq!(report.safe_additional_allocation_bytes, None);
        assert!(
            !report.model_load_authorized
                && !report.runtime_budget_validated
                && !report.active_admission_policy_changed
        );
    }
    #[test]
    fn pressure_unknown_or_warning_at_any_instant_blocks_normal_window() {
        for pressure in [Pressure::Unknown, Pressure::Warning, Pressure::Critical] {
            let mut samples = samples();
            samples[1].pressure = pressure;
            let report = review_pressure_window(&samples, 2000);
            assert!(!report.normal_pressure_observed);
            assert!(report.findings.contains(&ReviewFinding::PressureNotNormal));
        }
    }
    #[test]
    fn sample_count_and_time_validation_fail_closed() {
        assert!(review_pressure_window(&[], 2000)
            .findings
            .contains(&ReviewFinding::InvalidSampleCount));
        assert!(
            review_pressure_window(&vec![samples()[0].clone(); 17], 2000)
                .findings
                .contains(&ReviewFinding::InvalidSampleCount)
        );
        for (times, now, expected) in [
            ([0, 1000, 2000], 7001, ReviewFinding::StaleObservation),
            ([0, 1000, 2000], 1999, ReviewFinding::InvalidClock),
            ([0, 1000, 1000], 2000, ReviewFinding::InvalidClock),
            ([2000, 1000, 0], 2000, ReviewFinding::InvalidClock),
            ([0, 3000, 4000], 4000, ReviewFinding::SamplingGap),
            ([0, 500, 1000], 1000, ReviewFinding::WindowTooShort),
        ] {
            let mut samples = samples();
            for (sample, time) in samples.iter_mut().zip(times) {
                sample.sampled_at_millis = time;
            }
            assert!(review_pressure_window(&samples, now)
                .findings
                .contains(&expected));
        }
    }
    #[test]
    fn invalid_memory_missing_clearance_and_hardware_changes_are_not_hidden() {
        for (physical, free) in [
            (None, Some(1)),
            (Some(0), Some(0)),
            (Some(16 << 30), None),
            (Some(16 << 30), Some(u64::MAX)),
        ] {
            let mut samples = samples();
            samples[1].physical_bytes = physical;
            samples[1].free_bytes = free;
            let report = review_pressure_window(&samples, 2000);
            assert!(report.findings.contains(&ReviewFinding::MemoryUnavailable));
            assert_eq!(report.free_pages_min_bytes, None);
        }
        let mut samples = samples();
        samples[1].physical_bytes = Some(32 << 30);
        samples[1].external_runtime_clearance = false;
        let report = review_pressure_window(&samples, 2000);
        assert!(report
            .findings
            .contains(&ReviewFinding::PhysicalMemoryChanged));
        assert!(report
            .findings
            .contains(&ReviewFinding::ExternalRuntimeNotCleared));
        assert!(!report.normal_pressure_observed);
    }
}
