use super::{
    assess, assess_calibration, observe_host, observe_pressure, CalibrationApproval,
    CalibrationPolicy, HostObservation, LoadMonitor, MlxRecipe, MonitorStop, NativeProvenance,
    PackageMetadata, PreparationError, Pressure, PressureSample, Reservation,
};
use crate::fixture_driver::{FixtureDriver, Paths};
use crate::runner::{Driver, DriverFuture, PhaseEvidence, RunError};
use crate::{Phase, Snapshot};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct CoordinatorPaths {
    pub python: PathBuf,
    pub python_source: PathBuf,
    pub rust_server: PathBuf,
}

/// Explicit trusted-operator confirmation, never parsed from a request or model.
/// Creating it requires an actual current user approval and external-runtime
/// clearance; it is not something an agent should mint to bypass that approval.
pub struct ModelLoadApproval {
    recipe: MlxRecipe,
    expires: Instant,
}
impl ModelLoadApproval {
    pub fn confirm(recipe: &MlxRecipe, duration: Duration) -> Result<Self, PreparationError> {
        recipe.validate()?;
        if !(Duration::from_secs(1)..=Duration::from_secs(3600)).contains(&duration) {
            return Err(PreparationError::InvalidRecipe);
        }
        Ok(Self {
            recipe: recipe.clone(),
            expires: Instant::now() + duration,
        })
    }
    pub(crate) fn valid(&self, recipe: &MlxRecipe) -> bool {
        &self.recipe == recipe && Instant::now() < self.expires
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn deadline_for(&self, recipe: &MlxRecipe) -> Option<Instant> {
        self.valid(recipe).then_some(self.expires)
    }
}

#[derive(Clone, Copy)]
pub enum RehearsalCondition {
    Ready,
    LowMemory,
    CriticalPressure,
    MissingClearance,
    Stale,
    PressureDuringLoad,
    PressureDuringRequest,
}

pub(crate) struct CampaignSetup {
    pub(crate) recipe: MlxRecipe,
    packages: PackageMetadata,
    pub(crate) rehearsal_binary: Option<PathBuf>,
    condition: RehearsalCondition,
    approval: Option<ModelLoadApproval>,
    reservation: Option<Reservation>,
    reference: Option<Value>,
    provenance: Option<Value>,
    preflight: Option<Value>,
    error: Option<PreparationError>,
    clock: Instant,
    cancellation: Option<tokio::sync::watch::Receiver<bool>>,
    monitor: LoadMonitor,
    last_monitor_probe: Option<Instant>,
    calibration: Option<CalibrationPolicy>,
    calibration_approval: Option<CalibrationApproval>,
    calibration_used: bool,
    runtime_started: Option<Instant>,
}
impl CampaignSetup {
    fn new(
        recipe: MlxRecipe,
        packages: PackageMetadata,
        rehearsal_binary: Option<PathBuf>,
        condition: RehearsalCondition,
        approval: Option<ModelLoadApproval>,
    ) -> Self {
        Self {
            recipe,
            packages,
            rehearsal_binary,
            condition,
            approval,
            reservation: None,
            reference: None,
            provenance: None,
            preflight: None,
            error: None,
            clock: Instant::now(),
            cancellation: None,
            monitor: LoadMonitor::default(),
            last_monitor_probe: None,
            calibration: None,
            calibration_approval: None,
            calibration_used: false,
            runtime_started: None,
        }
    }
    pub(crate) fn is_rehearsal(&self) -> bool {
        self.rehearsal_binary.is_some()
    }
    fn authorized(&self) -> bool {
        if let Some(policy) = &self.calibration {
            return self.is_rehearsal()
                || self
                    .calibration_approval
                    .as_ref()
                    .is_some_and(|a| a.valid(&self.recipe, policy));
        }
        self.is_rehearsal()
            || self
                .approval
                .as_ref()
                .is_some_and(|a| a.valid(&self.recipe))
    }
    pub(crate) fn ensure_current(&self) -> Result<(), RunError> {
        if self
            .cancellation
            .as_ref()
            .is_some_and(|receiver| *receiver.borrow())
        {
            return Err(RunError::Cancelled);
        }
        if self.authorized() {
            Ok(())
        } else {
            Err(RunError::AuthorizationRequired)
        }
    }
    fn preparation(&mut self, error: PreparationError) -> RunError {
        self.error = Some(error);
        RunError::Preparation
    }
    pub(crate) async fn prepare_run(&mut self) -> Result<(), RunError> {
        self.error = None;
        self.preflight = None;
        self.monitor = LoadMonitor::default();
        self.last_monitor_probe = None;
        self.runtime_started = None;
        self.ensure_current()?;
        if self.calibration.is_some() {
            if self.calibration_used {
                return Err(RunError::AuthorizationRequired);
            }
            self.calibration_used = true;
        }
        if self.reservation.is_none() {
            self.reservation =
                Some(Reservation::acquire(&self.recipe).map_err(|e| self.preparation(e))?);
        }
        self.reservation
            .as_mut()
            .unwrap()
            .rearm()
            .map_err(|e| self.preparation(e))?;
        let provenance = NativeProvenance::inspect(&self.recipe, &self.packages)
            .await
            .map_err(|e| self.preparation(e))?;
        let value = serde_json::to_value(provenance).map_err(|_| RunError::InvalidEvidence)?;
        self.provenance = Some(value.clone());
        if self
            .reference
            .as_ref()
            .is_some_and(|previous| previous != &value)
        {
            return Err(RunError::ProvenanceChanged);
        }
        self.reference = Some(value);
        self.ensure_current()?;
        let clock = self.clock;
        let now = || u64::try_from(clock.elapsed().as_millis()).map_err(|_| RunError::Limit);
        let observation = self.host_observation().await?;
        if let Some(policy) = self.calibration.clone() {
            let mut observations = vec![observation];
            for _ in 0..2 {
                tokio::time::sleep(Duration::from_secs(1)).await;
                self.ensure_current()?;
                observations.push(self.host_observation().await?);
            }
            let ready = assess_calibration(
                &self.recipe,
                &policy,
                &observations,
                now()?,
                self.reservation.as_ref(),
            )
            .map_err(|e| self.preparation(e))?;
            let blocked = !ready.eligible_for_experimental_calibration;
            self.preflight = Some(
                json!({"source":if self.is_rehearsal() {"injected-rehearsal"} else {"native-host"},
                "policy":"experimental-single-calibration","readiness":ready,"observations":observations}),
            );
            if blocked {
                return Err(RunError::PreflightBlocked);
            }
            self.ensure_current()?;
            let last = observations.last().ok_or(RunError::InvalidEvidence)?;
            self.monitor
                .check(
                    PressureSample {
                        sampled_at_millis: last.sampled_at_millis,
                        pressure: last.pressure,
                    },
                    now()?,
                )
                .map_err(|_| RunError::LoadMonitorStopped)?;
            return Ok(());
        }
        let ready = assess(&self.recipe, observation, now()?, self.reservation.as_ref())
            .map_err(|e| self.preparation(e))?;
        let blocked = !ready.blockers.is_empty();
        if !blocked {
            self.monitor
                .check(
                    PressureSample {
                        sampled_at_millis: ready.observation.sampled_at_millis,
                        pressure: ready.observation.pressure,
                    },
                    now()?,
                )
                .map_err(|_| RunError::LoadMonitorStopped)?;
        }
        self.preflight = Some(
            json!({"source":if self.is_rehearsal() {"injected-rehearsal"} else {"native-host"},"readiness":ready}),
        );
        if blocked {
            return Err(RunError::PreflightBlocked);
        }
        Ok(())
    }
    async fn host_observation(&mut self) -> Result<HostObservation, RunError> {
        let clock = self.clock;
        let now = || u64::try_from(clock.elapsed().as_millis()).map_err(|_| RunError::Limit);
        let observation = if self.is_rehearsal() {
            let mut host = HostObservation {
                sampled_at_millis: now()?,
                physical_bytes: Some(16 << 30),
                free_bytes: Some(8 << 30),
                pressure: Pressure::Normal,
                external_runtime_clearance: true,
            };
            match self.condition {
                RehearsalCondition::Ready
                | RehearsalCondition::PressureDuringLoad
                | RehearsalCondition::PressureDuringRequest => {}
                RehearsalCondition::LowMemory => host.free_bytes = Some(0),
                RehearsalCondition::CriticalPressure => host.pressure = Pressure::Critical,
                RehearsalCondition::MissingClearance => host.external_runtime_clearance = false,
                RehearsalCondition::Stale => host.sampled_at_millis = u64::MAX,
            }
            host
        } else {
            observe_host(self.clock, true)
                .await
                .map_err(|e| self.preparation(e))?
        };
        Ok(observation)
    }
    /// Watchdog errors abort to runner cleanup; this never admits a new load.
    pub(crate) fn remaining_runtime(&self) -> Result<Option<Duration>, RunError> {
        self.calibration
            .as_ref()
            .zip(self.runtime_started)
            .map(|(policy, started)| {
                Duration::from_secs(u64::from(policy.runtime_seconds))
                    .checked_sub(started.elapsed())
                    .filter(|remaining| !remaining.is_zero())
                    .ok_or(RunError::CalibrationDeadline)
            })
            .transpose()
    }
    pub(crate) async fn monitor_pressure(
        &mut self,
        request: bool,
        force: bool,
    ) -> Result<(), RunError> {
        self.ensure_current()?;
        self.remaining_runtime()?;
        if self.monitor.stop.is_some() {
            return Err(RunError::LoadMonitorStopped);
        }
        if !force
            && self
                .last_monitor_probe
                .is_some_and(|last| last.elapsed() < Duration::from_secs(1))
        {
            return Ok(());
        }
        let clock = self.clock;
        let now = || u64::try_from(clock.elapsed().as_millis()).map_err(|_| RunError::Limit);
        let sample = if self.is_rehearsal() {
            let bad = matches!(self.condition, RehearsalCondition::PressureDuringLoad) && !request
                || matches!(self.condition, RehearsalCondition::PressureDuringRequest) && request;
            PressureSample {
                sampled_at_millis: now()?,
                pressure: if bad {
                    Pressure::Warning
                } else {
                    Pressure::Normal
                },
            }
        } else {
            match observe_pressure(self.clock).await {
                Ok(sample) => sample,
                Err(error) => {
                    self.error = Some(error);
                    let _ = self.monitor.fail(MonitorStop::Unavailable);
                    return Err(RunError::LoadMonitorStopped);
                }
            }
        };
        self.last_monitor_probe = Some(Instant::now());
        self.monitor
            .check(sample, now()?)
            .map_err(|_| RunError::LoadMonitorStopped)
    }
    pub(crate) fn release_runtime(&mut self) -> Result<u16, RunError> {
        self.ensure_current()?;
        self.runtime_started = Some(Instant::now());
        self.reservation
            .as_mut()
            .ok_or(RunError::Unconfirmed)?
            .release_runtime()
            .map_err(|e| self.preparation(e))
    }
    pub(crate) fn release_coordinator(&mut self) -> Result<u16, RunError> {
        self.reservation
            .as_mut()
            .ok_or(RunError::Unconfirmed)?
            .release_coordinator()
            .map_err(|e| self.preparation(e))
    }
    pub(crate) fn state(&self) -> Value {
        json!({"recipe":self.recipe,"lease_held":self.reservation.is_some(),"provenance":self.provenance,
            "calibration_policy":self.calibration,"runtime_elapsed_millis":self.runtime_started.map(|s|s.elapsed().as_millis()),
            "preflight":self.preflight,"load_monitor":self.monitor,"monitor_source":if self.is_rehearsal() {"injected-rehearsal"} else {"native-pressure-probe"},
            "preparation_error":self.error,"model_load_approval_current":!self.is_rehearsal() && self.authorized()})
    }
    pub(crate) fn set_cancellation(&mut self, receiver: tokio::sync::watch::Receiver<bool>) {
        self.cancellation = Some(receiver);
    }
    pub(crate) fn configure_calibration(
        &mut self,
        policy: CalibrationPolicy,
        approval: Option<CalibrationApproval>,
    ) -> Result<(), RunError> {
        policy.validate().map_err(|_| RunError::Preparation)?;
        if !self.is_rehearsal()
            && !approval
                .as_ref()
                .is_some_and(|a| a.valid(&self.recipe, &policy))
        {
            return Err(RunError::AuthorizationRequired);
        }
        self.calibration = Some(policy);
        self.calibration_approval = approval;
        self.approval = None;
        Ok(())
    }
}

pub struct CampaignDriver {
    core: FixtureDriver,
    rehearsal: bool,
    calibration: bool,
}
impl CampaignDriver {
    /// Explicit opt-in only. Does not load a model or grant load approval.
    /// Incompatible with the exec-based synthetic campaign rehearsal.
    pub fn with_runtime_memory(mut self) -> Result<Self, RunError> {
        self.core.enable_runtime_memory()?;
        Ok(self)
    }
    /// Builder for fixed-fixture rehearsal only; never converts a real campaign's
    /// normal approval into a calibration approval.
    pub fn into_calibration_rehearsal(
        mut self,
        policy: CalibrationPolicy,
    ) -> Result<Self, RunError> {
        if !self.rehearsal {
            return Err(RunError::AuthorizationRequired);
        }
        self.core.configure_calibration(policy, None)?;
        self.calibration = true;
        Ok(self)
    }
    pub async fn prepare_calibration_authorized(
        paths: CoordinatorPaths,
        directory: PathBuf,
        recipe: MlxRecipe,
        packages: PackageMetadata,
        approval: CalibrationApproval,
    ) -> Result<Self, RunError> {
        let policy = approval.policy();
        if !approval.valid(&recipe, &policy) {
            return Err(RunError::AuthorizationRequired);
        }
        let mut setup = CampaignSetup::new(recipe, packages, None, RehearsalCondition::Ready, None);
        setup.configure_calibration(policy, Some(approval))?;
        let mut driver = Self::prepare(paths, directory, setup).await?;
        driver.calibration = true;
        Ok(driver)
    }
    pub(crate) fn is_calibration(&self) -> bool {
        self.calibration
    }
    /// Only the repository's fixed Python exec shim and compiled fixture binary.
    /// `recipe.checkpoint` and METADATA must be synthetic assets under output's
    /// parent. This method never imports MLX or manufactures load authorization.
    pub async fn prepare_rehearsal(
        paths: CoordinatorPaths,
        directory: PathBuf,
        recipe: MlxRecipe,
        packages: PackageMetadata,
        fixture_binary: PathBuf,
        condition: RehearsalCondition,
    ) -> Result<Self, RunError> {
        recipe.validate().map_err(|_| RunError::Preparation)?;
        let shim = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mlx_entrypoint.py")
            .canonicalize()
            .map_err(|_| RunError::Preparation)?;
        let parent = directory
            .parent()
            .ok_or(RunError::Artifact)?
            .canonicalize()
            .map_err(|_| RunError::Artifact)?;
        if recipe
            .runtime_entrypoint
            .canonicalize()
            .map_err(|_| RunError::Preparation)?
            != shim
            || [
                &recipe.checkpoint,
                &packages.mlx,
                &packages.mlx_lm,
                &packages.transformers,
            ]
            .iter()
            .any(|path| {
                path.canonicalize()
                    .map_or(true, |p| p == parent || !p.starts_with(&parent))
            })
        {
            return Err(RunError::Preparation);
        }
        let fixture_binary = fixture_binary
            .canonicalize()
            .map_err(|_| RunError::Preparation)?;
        Self::prepare(
            paths,
            directory,
            CampaignSetup::new(recipe, packages, Some(fixture_binary), condition, None),
        )
        .await
    }
    /// Does not start inference. The returned driver's start hook will hash approved
    /// assets, acquire/hold the lease and enforce preflight before each owned run.
    pub async fn prepare_authorized(
        paths: CoordinatorPaths,
        directory: PathBuf,
        recipe: MlxRecipe,
        packages: PackageMetadata,
        approval: ModelLoadApproval,
    ) -> Result<Self, RunError> {
        if !approval.valid(&recipe) {
            return Err(RunError::AuthorizationRequired);
        }
        Self::prepare(
            paths,
            directory,
            CampaignSetup::new(
                recipe,
                packages,
                None,
                RehearsalCondition::Ready,
                Some(approval),
            ),
        )
        .await
    }
    async fn prepare(
        paths: CoordinatorPaths,
        directory: PathBuf,
        setup: CampaignSetup,
    ) -> Result<Self, RunError> {
        setup.recipe.validate().map_err(|_| RunError::Preparation)?;
        let rehearsal = setup.is_rehearsal();
        let paths = Paths {
            python: paths.python,
            python_source: paths.python_source,
            rust_server: paths.rust_server,
            fixture_runtime: setup.rehearsal_binary.clone().unwrap_or_default(),
        };
        let core = FixtureDriver::prepare_campaign(paths, directory, setup).await?;
        Ok(Self {
            core,
            rehearsal,
            calibration: false,
        })
    }
    pub fn directory(&self) -> PathBuf {
        self.core.directory()
    }
    pub(crate) fn mode(&self) -> &'static str {
        if self.calibration {
            return if self.rehearsal {
                "calibration-rehearsal"
            } else {
                "model-calibration"
            };
        }
        if self.rehearsal {
            "campaign-rehearsal"
        } else {
            "model-campaign"
        }
    }
}
impl Driver for CampaignDriver {
    fn set_cancellation(&mut self, receiver: tokio::sync::watch::Receiver<bool>) {
        self.core.set_cancellation(receiver);
    }
    fn is_simulated(&self) -> bool {
        false
    }
    fn backend_model(&self) -> &str {
        self.core.backend_model()
    }
    fn startup_deadline(&self) -> Duration {
        Duration::from_secs(720)
    }
    fn artifact_metadata(&self) -> Value {
        self.core.artifact_metadata()
    }
    fn resource_state(&self) -> Value {
        self.core.resource_state()
    }
    fn phase_metadata(&self) -> Value {
        self.core.phase_metadata()
    }
    fn partial_snapshots(&mut self) -> Vec<Snapshot> {
        self.core.partial_snapshots()
    }
    fn start_run(&mut self, arm: crate::runner::Arm, plan: Value) -> DriverFuture<'_, ()> {
        self.core.start_run(arm, plan)
    }
    fn phase(&mut self, phase: Phase) -> DriverFuture<'_, PhaseEvidence> {
        self.core.phase(phase)
    }
    fn stop_run(&mut self) -> DriverFuture<'_, bool> {
        self.core.stop_run()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_is_scoped_to_exact_recipe_and_expires_without_a_json_permission() {
        let recipe = MlxRecipe {
            python: "/synthetic/python".into(),
            runtime_entrypoint: "/synthetic/entry".into(),
            checkpoint: "/synthetic/model".into(),
            coordination_directory: "/synthetic/shared".into(),
            minimum_free_bytes: 1 << 30,
        };
        let approval = ModelLoadApproval::confirm(&recipe, Duration::from_secs(1)).unwrap();
        assert!(approval.valid(&recipe));
        let mut other = recipe.clone();
        other.checkpoint = "/synthetic/other".into();
        assert!(!approval.valid(&other));
        let expired = ModelLoadApproval {
            recipe: recipe.clone(),
            expires: Instant::now() - Duration::from_secs(1),
        };
        assert!(!expired.valid(&recipe));
        assert!(ModelLoadApproval::confirm(&recipe, Duration::ZERO).is_err());
        assert!(ModelLoadApproval::confirm(&recipe, Duration::from_secs(3601)).is_err());
    }

    #[tokio::test]
    async fn canceled_preparation_does_not_hash_reserve_or_start_any_runtime() {
        let recipe = MlxRecipe {
            python: "/nonexistent/python".into(),
            runtime_entrypoint: "/nonexistent/entry".into(),
            checkpoint: "/nonexistent/model".into(),
            coordination_directory: "/nonexistent/shared".into(),
            minimum_free_bytes: 1 << 30,
        };
        let packages = PackageMetadata {
            mlx: "/nonexistent/mlx".into(),
            mlx_lm: "/nonexistent/mlx-lm".into(),
            transformers: "/nonexistent/transformers".into(),
        };
        let mut setup = CampaignSetup::new(
            recipe,
            packages,
            Some("/nonexistent/fixture".into()),
            RehearsalCondition::Ready,
            None,
        );
        let (_sender, receiver) = tokio::sync::watch::channel(true);
        setup.set_cancellation(receiver);
        assert_eq!(setup.prepare_run().await, Err(RunError::Cancelled));
        assert!(setup.reservation.is_none());
        assert!(setup.provenance.is_none());
        assert_eq!(setup.release_runtime(), Err(RunError::Cancelled));
    }

    #[tokio::test]
    async fn normal_campaign_expiry_and_cancel_precede_probe_and_throttle() {
        for cancelled in [false, true] {
            let recipe = MlxRecipe {
                python: "/nonexistent/python".into(),
                runtime_entrypoint: "/nonexistent/entry".into(),
                checkpoint: "/nonexistent/model".into(),
                coordination_directory: "/nonexistent/shared".into(),
                minimum_free_bytes: 2 << 30,
            };
            let approval = ModelLoadApproval {
                recipe: recipe.clone(),
                expires: if cancelled {
                    Instant::now() + Duration::from_secs(60)
                } else {
                    Instant::now() - Duration::from_secs(1)
                },
            };
            let packages = PackageMetadata {
                mlx: "/nonexistent/mlx".into(),
                mlx_lm: "/nonexistent/mlx-lm".into(),
                transformers: "/nonexistent/transformers".into(),
            };
            let mut setup = CampaignSetup::new(
                recipe,
                packages,
                None,
                RehearsalCondition::Ready,
                Some(approval),
            );
            let (_sender, receiver) = tokio::sync::watch::channel(cancelled);
            setup.set_cancellation(receiver);
            setup.last_monitor_probe = Some(Instant::now());
            let expected = if cancelled {
                RunError::Cancelled
            } else {
                RunError::AuthorizationRequired
            };
            for request in [false, true] {
                assert_eq!(setup.monitor_pressure(request, false).await, Err(expected));
                assert_eq!(setup.monitor_pressure(request, true).await, Err(expected));
            }
            assert_eq!(setup.monitor.samples_checked, 0);
            assert!(setup.reservation.is_none());
        }
    }

    #[tokio::test]
    async fn calibration_deadline_and_single_use_stop_before_another_probe_or_load() {
        let recipe = MlxRecipe {
            python: "/fixture/python".into(),
            runtime_entrypoint: "/fixture/shim".into(),
            checkpoint: "/fixture/model".into(),
            coordination_directory: "/fixture/shared".into(),
            minimum_free_bytes: 2 << 30,
        };
        let packages = PackageMetadata {
            mlx: "/fixture/mlx".into(),
            mlx_lm: "/fixture/mlx-lm".into(),
            transformers: "/fixture/transformers".into(),
        };
        let mut setup = CampaignSetup::new(
            recipe,
            packages,
            Some("/fixture/runtime".into()),
            RehearsalCondition::Ready,
            None,
        );
        setup
            .configure_calibration(
                CalibrationPolicy {
                    runtime_seconds: 30,
                },
                None,
            )
            .unwrap();
        setup.runtime_started = Some(Instant::now() - Duration::from_secs(31));
        assert_eq!(
            setup.monitor_pressure(true, true).await,
            Err(RunError::CalibrationDeadline)
        );
        assert_eq!(setup.monitor.samples_checked, 0);
        setup.calibration_used = true;
        assert_eq!(
            setup.prepare_run().await,
            Err(RunError::AuthorizationRequired)
        );
        assert!(setup.reservation.is_none());
    }
}
