//! Opt-in campaign preparation and owned driver; rehearsal does not load MLX.
mod admission_review;
mod calibration;
pub use calibration::{
    assess_calibration, CalibrationApproval, CalibrationPolicy, CalibrationReadiness,
};
mod load_budget;
mod load_monitor;
pub use load_budget::{
    logical_kv_bytes, Basis, BudgetAmount, BudgetError, BudgetReport, KvLayerShape, LoadBudget,
};
pub use load_monitor::{LoadMonitor, MonitorStop, PressureSample, PRESSURE_MAX_AGE_MILLIS};
pub(crate) mod driver;
pub use admission_review::{review_pressure_window, AdmissionReview, ReviewFinding};
mod native;
mod provenance;
mod reservation;
pub use driver::{CampaignDriver, CoordinatorPaths, ModelLoadApproval, RehearsalCondition};
pub use native::{observe_host, observe_pressure, NativeProvenance, PackageMetadata};
pub use provenance::{Asset, AssetKind, Provenance, ProvenanceSource};
pub use reservation::Reservation;

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparationError {
    InvalidRecipe,
    InvalidProvenance,
    Limit,
    Busy,
    Unsupported,
    Io,
    TimedOut,
}

/// Trusted operator configuration, not a model-generated command or endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MlxRecipe {
    pub python: PathBuf,
    pub runtime_entrypoint: PathBuf,
    pub checkpoint: PathBuf,
    pub coordination_directory: PathBuf,
    /// Observation threshold selected for this host by the operator, not a RAM estimate.
    pub minimum_free_bytes: u64,
}

fn absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .to_str()
            .is_some_and(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
        && path != Path::new("/")
}
impl MlxRecipe {
    pub fn validate(&self) -> Result<(), PreparationError> {
        if [
            &self.python,
            &self.runtime_entrypoint,
            &self.checkpoint,
            &self.coordination_directory,
        ]
        .iter()
        .any(|path| !absolute(path))
            || self.python == self.runtime_entrypoint
            || !(1024 * 1024 * 1024..=1024 * 1024 * 1024 * 1024).contains(&self.minimum_free_bytes)
        {
            return Err(PreparationError::InvalidRecipe);
        }
        Ok(())
    }

    /// Arguments only, never executes. Port must belong to an owned reservation;
    /// a number passing validation is NOT proof of a listener or permission.
    pub fn runtime_arguments(&self, port: u16) -> Result<Vec<String>, PreparationError> {
        self.validate()?;
        if port < 49152 || [8010, 8012, 8081].contains(&port) {
            return Err(PreparationError::InvalidRecipe);
        }
        Ok(vec![
            self.runtime_entrypoint.to_string_lossy().into_owned(),
            "--model".into(),
            self.checkpoint.to_string_lossy().into_owned(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
            "--allowed-origins".into(),
            format!("http://127.0.0.1:{port}"),
            "--decode-concurrency".into(),
            "1".into(),
            "--prompt-concurrency".into(),
            "1".into(),
            "--prefill-step-size".into(),
            "256".into(),
            "--prompt-cache-size".into(),
            "1".into(),
            "--prompt-cache-bytes".into(),
            "128MB".into(),
            "--max-tokens".into(),
            "256".into(),
            "--chat-template-args".into(),
            "{\"enable_thinking\":false}".into(),
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pressure {
    Normal,
    Warning,
    Critical,
    Unknown,
}

/// Host observations supplied by the explicit preflight mechanism. No PID inputs.
#[derive(Debug, Clone, Serialize)]
pub struct HostObservation {
    pub sampled_at_millis: u64,
    pub physical_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    pub pressure: Pressure,
    /// Operator's current clearance, not inferred from closed ports or a lock.
    pub external_runtime_clearance: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Blocker {
    StaleObservation,
    MemoryUnavailable,
    BelowThreshold,
    PressureNotNormal,
    ExternalRuntimeNotCleared,
    NoReservation,
}
#[derive(Debug, Serialize)]
pub struct Readiness {
    pub memory_method: &'static str,
    pub memory_threshold_is_capacity_guarantee: bool,
    pub blockers: Vec<Blocker>,
    pub observation: HostObservation,
    pub minimum_free_bytes: u64,
    /// Preparation deliberately does not grant model-load authorization.
    pub model_load_authorized: bool,
}

/// Pure policy, also usable with synthetic observations. Never an execution token.
pub fn assess(
    recipe: &MlxRecipe,
    observation: HostObservation,
    now_millis: u64,
    reservation: Option<&Reservation>,
) -> Result<Readiness, PreparationError> {
    recipe.validate()?;
    let mut blockers = Vec::new();
    if now_millis
        .checked_sub(observation.sampled_at_millis)
        .is_none_or(|age| age > 5000)
    {
        blockers.push(Blocker::StaleObservation);
    }
    match (observation.physical_bytes, observation.free_bytes) {
        (Some(total), Some(free)) if total > 0 && free <= total => {
            if free < recipe.minimum_free_bytes {
                blockers.push(Blocker::BelowThreshold);
            }
        }
        _ => blockers.push(Blocker::MemoryUnavailable),
    }
    if observation.pressure != Pressure::Normal {
        blockers.push(Blocker::PressureNotNormal);
    }
    if !observation.external_runtime_clearance {
        blockers.push(Blocker::ExternalRuntimeNotCleared);
    }
    if reservation.is_none_or(|r| !r.matches(recipe)) {
        blockers.push(Blocker::NoReservation);
    }
    Ok(Readiness {
        memory_method: "vm-stat-free-pages-only",
        memory_threshold_is_capacity_guarantee: false,
        blockers,
        observation,
        minimum_free_bytes: recipe.minimum_free_bytes,
        model_load_authorized: false,
    })
}

#[cfg(test)]
mod tests;
