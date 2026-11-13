//! A/B lifecycle for injected simulations and the concrete owned fixture driver.
use crate::{Availability, Phase, ProcessIdentity, Role, Snapshot, MAX_TARGETS};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;

const PLAN: &str = include_str!("../../../contracts/benchmarks/memory-baseline/v0.1.0/plan.json");
const MAX_ARTIFACT_BYTES: usize = 32 * 1024 * 1024;
const MAX_EVENTS: usize = 10_000;
const DEADLINE: Duration = Duration::from_secs(240);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    Python,
    Rust,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunError {
    RealDriverForbidden,
    Artifact,
    Limit,
    InvalidEvidence,
    IdentityChanged,
    ClockRegressed,
    UnequalWork,
    Driver,
    TimedOut,
    Cancelled,
    Unconfirmed,
    Preparation,
    AuthorizationRequired,
    PreflightBlocked,
    ProvenanceChanged,
    LoadMonitorStopped,
    CalibrationDeadline,
}

pub struct Exchange {
    pub backend_request: Value,
    pub completion: Value,
    pub timing: Option<ExchangeTiming>,
}
#[derive(Serialize)]
pub struct ExchangeTiming {
    pub request_millis: u64,
    pub headers_millis: u64,
    pub response_bytes: usize,
}
pub struct PhaseEvidence {
    pub exchanges: Vec<Exchange>,
    pub snapshots: Vec<Snapshot>,
}
pub type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RunError>> + Send + 'a>>;

/// Trusted injection seam. Declaring a driver simulated is not user authorization.
pub trait Driver: Send + 'static {
    fn is_simulated(&self) -> bool;
    fn backend_model(&self) -> &str {
        "synthetic-checkpoint"
    }
    fn startup_deadline(&self) -> Duration {
        DEADLINE
    }
    fn set_cancellation(&mut self, _: watch::Receiver<bool>) {}
    fn artifact_metadata(&self) -> Value {
        Value::Null
    }
    fn resource_state(&self) -> Value {
        Value::Null
    }
    fn phase_metadata(&self) -> Value {
        Value::Null
    }
    fn partial_snapshots(&mut self) -> Vec<Snapshot> {
        vec![]
    }
    fn start_run(&mut self, arm: Arm, plan: Value) -> DriverFuture<'_, ()>;
    fn phase(&mut self, phase: Phase) -> DriverFuture<'_, PhaseEvidence>;
    /// true means all owned resources confirmed stopped, not merely disconnected.
    fn stop_run(&mut self) -> DriverFuture<'_, bool>;
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub mode: &'static str,
    pub completed: bool,
    pub runs_completed: usize,
    pub error: Option<RunError>,
    pub cleanup_confirmed: bool,
}

pub struct RunHandle {
    cancel: watch::Sender<bool>,
    task: Option<JoinHandle<Result<Report, RunError>>>,
}
impl RunHandle {
    pub fn cancel(&self) {
        self.cancel.send_replace(true);
    }
    pub async fn wait(mut self) -> Result<Report, RunError> {
        self.task
            .take()
            .ok_or(RunError::Driver)?
            .await
            .map_err(|_| RunError::Driver)?
    }
}
impl Drop for RunHandle {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
    }
}

struct Artifacts {
    directory: PathBuf,
    events: File,
    bytes: usize,
    count: usize,
    finished: bool,
    mode: &'static str,
}
fn new_file(path: PathBuf) -> Result<File, RunError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| RunError::Artifact)
}
impl Artifacts {
    fn create(directory: PathBuf) -> Result<Self, RunError> {
        Self::create_mode(directory, "simulation", Value::Null)
    }
    fn create_mode(
        directory: PathBuf,
        mode: &'static str,
        metadata: Value,
    ) -> Result<Self, RunError> {
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        builder.create(&directory).map_err(|_| RunError::Artifact)?;
        let mut manifest = new_file(directory.join("manifest.json"))?;
        let document = json!({"mode":mode,"package_version":env!("CARGO_PKG_VERSION"),
            "real_host_measured":mode != "simulation","model_inference":matches!(mode,"model-campaign"|"model-calibration"),
            "weight_hash":null,"binary_hash":null,"assets":metadata,"plan":plan(),"execution":execution_metadata(mode)});
        let bytes = serde_json::to_vec(&document).map_err(|_| RunError::Artifact)?;
        manifest.write_all(&bytes).map_err(|_| RunError::Artifact)?;
        let events = new_file(directory.join("events.jsonl"))?;
        Ok(Self {
            directory,
            events,
            bytes: bytes.len(),
            count: 0,
            finished: false,
            mode,
        })
    }
    fn event(&mut self, value: Value) -> Result<(), RunError> {
        let mut line = serde_json::to_vec(&value).map_err(|_| RunError::Artifact)?;
        line.push(b'\n');
        if self.count >= MAX_EVENTS || line.len() > MAX_ARTIFACT_BYTES.saturating_sub(self.bytes) {
            return Err(RunError::Limit);
        }
        self.events
            .write_all(&line)
            .and_then(|_| self.events.flush())
            .map_err(|_| RunError::Artifact)?;
        self.bytes += line.len();
        self.count += 1;
        Ok(())
    }
    fn finish(&mut self, report: &Report) -> Result<(), RunError> {
        let bytes = serde_json::to_vec(report).map_err(|_| RunError::Artifact)?;
        if bytes.len() > MAX_ARTIFACT_BYTES.saturating_sub(self.bytes) {
            return Err(RunError::Limit);
        }
        new_file(self.directory.join("summary.json"))?
            .write_all(&bytes)
            .map_err(|_| RunError::Artifact)?;
        self.finished = true;
        Ok(())
    }
}
impl Drop for Artifacts {
    fn drop(&mut self) {
        if !self.finished {
            if let Ok(mut file) = new_file(self.directory.join("aborted.json")) {
                let _ = serde_json::to_writer(
                    &mut file,
                    &json!({"mode":self.mode,"completed":false,"reason":"incomplete-artifacts"}),
                );
            }
        }
    }
}

pub fn plan() -> Value {
    serde_json::from_str(PLAN).expect("checked-in baseline plan must be valid JSON")
}

pub fn launch_simulation(directory: PathBuf, driver: impl Driver) -> Result<RunHandle, RunError> {
    if !driver.is_simulated() {
        return Err(RunError::RealDriverForbidden);
    }
    launch(directory, driver, "simulation")
}

/// Concrete owned fixture driver only. No model recipe or free endpoint input.
pub fn launch_fixture_integration(
    driver: crate::fixture_driver::FixtureDriver,
) -> Result<RunHandle, RunError> {
    launch(driver.directory(), driver, "fixture-integration")
}

pub fn launch_campaign(driver: crate::campaign::CampaignDriver) -> Result<RunHandle, RunError> {
    if driver.is_calibration() {
        return Err(RunError::InvalidEvidence);
    }
    let mode = driver.mode();
    launch(driver.directory(), driver, mode)
}

pub fn launch_calibration(driver: crate::campaign::CampaignDriver) -> Result<RunHandle, RunError> {
    if !driver.is_calibration() {
        return Err(RunError::InvalidEvidence);
    }
    let mode = driver.mode();
    launch(driver.directory(), driver, mode)
}
fn is_calibration(mode: &str) -> bool {
    matches!(mode, "model-calibration" | "calibration-rehearsal")
}
fn execution_metadata(mode: &str) -> Value {
    if is_calibration(mode) {
        json!({"protocol":"single-runtime-calibration-not-baseline","arms":["rust"],"runs":1,
        "phases":["listener-ready","cold-text","shutdown"],"inference_requests":1})
    } else {
        json!({"protocol":"abba-baseline","runs":12,"phases":7})
    }
}

fn launch(
    directory: PathBuf,
    mut driver: impl Driver,
    mode: &'static str,
) -> Result<RunHandle, RunError> {
    validate_plan(&plan())?;
    let backend_model = driver.backend_model().to_owned();
    if backend_model.is_empty()
        || backend_model.len() > 4096
        || backend_model.chars().any(char::is_control)
    {
        return Err(RunError::InvalidEvidence);
    }
    let runtime = tokio::runtime::Handle::try_current().map_err(|_| RunError::Driver)?;
    let mut artifacts = if mode == "simulation" {
        Artifacts::create(directory)?
    } else {
        Artifacts::create_mode(directory, mode, driver.artifact_metadata())?
    };
    let (cancel, cancelled) = watch::channel(false);
    driver.set_cancellation(cancelled.clone());
    let arms = if is_calibration(mode) {
        vec![Arm::Rust]
    } else {
        [Arm::Python, Arm::Rust, Arm::Rust, Arm::Python].repeat(3)
    };
    let target_runs = arms.len();
    let phases = if is_calibration(mode) {
        vec![Phase::ListenerReady, Phase::ColdText, Phase::Shutdown]
    } else {
        vec![
            Phase::ListenerReady,
            Phase::ColdText,
            Phase::ToolRoundtrip,
            Phase::WarmRepeat,
            Phase::Idle,
            Phase::Resume,
            Phase::Shutdown,
        ]
    };
    let task = runtime.spawn(async move {
        let mut report = Report { mode, completed: false, runs_completed: 0, error: None, cleanup_confirmed: true };
        let mut reference = None;
        'runs: for arm in arms {
            if *cancelled.borrow() { report.error = Some(RunError::Cancelled); break; }
            let result = async {
                artifacts.event(json!({"event":"run_start","run":report.runs_completed,"arm":arm}))?;
                let startup_deadline = driver.startup_deadline().min(Duration::from_secs(720));
                tokio::time::timeout(startup_deadline, driver.start_run(arm, plan())).await.map_err(|_| RunError::TimedOut)??;
                artifacts.event(json!({"event":"resources","run":report.runs_completed,"resources":driver.resource_state()}))?;
                let mut evidence = RunEvidence::default();
                for phase in phases.iter().copied() {
                    if *cancelled.borrow() { return Err(RunError::Cancelled); }
                    artifacts.event(json!({"event":"phase_start","run":report.runs_completed,"arm":arm,"phase":phase}))?;
                    let phase_evidence = bounded(driver.phase(phase)).await?;
                    if let Err(error) = evidence.validate_model(phase, &phase_evidence, &backend_model) {
                        if phase_evidence.snapshots.len() <= 128
                            && phase_evidence.snapshots.iter().all(|snapshot| snapshot.processes.len() <= MAX_TARGETS)
                            && phase_evidence.exchanges.len() <= 3 {
                            let _ = artifacts.event(json!({"event":"rejected_phase","run":report.runs_completed,"phase":phase,
                                "source":mode,"validated_run":false,"error":error,"snapshots":phase_evidence.snapshots,
                                "requests":phase_evidence.exchanges.iter().map(|exchange| &exchange.backend_request).collect::<Vec<_>>(),
                                "diagnostics":driver.phase_metadata()}));
                        }
                        return Err(error);
                    }
                    artifacts.event(json!({"event":"phase","run":report.runs_completed,"arm":arm,"phase":phase,
                        "source":mode,"snapshots":phase_evidence.snapshots,"diagnostics":driver.phase_metadata(),
                        "results":phase_evidence.exchanges.iter().map(|exchange| json!({
                            "message":exchange.completion["choices"][0]["message"],
                            "finish_reason":exchange.completion["choices"][0]["finish_reason"],
                            "usage":exchange.completion["usage"],"timing":exchange.timing})).collect::<Vec<_>>(),
                        "requests":phase_evidence.exchanges.iter().map(|exchange| &exchange.backend_request).collect::<Vec<_>>()}))?;
                    if *cancelled.borrow() { return Err(RunError::Cancelled); }
                }
                if let Some(expected) = &reference {
                    if expected != &evidence.requests { return Err(RunError::UnequalWork); }
                } else { reference = Some(evidence.requests); }
                Ok(())
            }.await;
            if result.is_err() {
                let partial = driver.partial_snapshots();
                if !partial.is_empty() && partial.len() <= 128 && partial.iter().all(|snapshot| snapshot.processes.len() <= MAX_TARGETS) {
                    // Best effort; an artifact failure must not skip driver cleanup.
                    let _ = artifacts.event(json!({"event":"partial_snapshots","run":report.runs_completed,"source":mode,"validated_run":false,"snapshots":partial,"diagnostics":driver.phase_metadata()}));
                }
            }
            // Even failed starts/validation/cancel need explicit driver cleanup.
            let cleanup = bounded(driver.stop_run()).await;
            let confirmed = matches!(cleanup, Ok(true));
            report.cleanup_confirmed &= confirmed;
            artifacts.event(json!({"event":"run_stop","run":report.runs_completed,"arm":arm,"confirmed":confirmed,
                "error":result.as_ref().err().or_else(|| cleanup.as_ref().err()),"resources":driver.resource_state()}))?;
            if let Err(error) = result { report.error = Some(error); break 'runs; }
            if !confirmed { report.error = Some(RunError::Unconfirmed); break 'runs; }
            report.runs_completed += 1;
        }
        report.completed = report.error.is_none() && report.runs_completed == target_runs && report.cleanup_confirmed;
        artifacts.finish(&report)?;
        Ok(report)
    });
    Ok(RunHandle {
        cancel,
        task: Some(task),
    })
}

fn validate_plan(plan: &Value) -> Result<(), RunError> {
    let comparison = &plan["comparison"];
    let phases = &plan["workload"];
    if plan["version"] != 1
        || plan["runner_implemented"] != true
        || plan["authorization_required"] != true
        || comparison["block_order"] != json!(["python", "rust", "rust", "python"])
        || comparison["blocks"] != 3
        || comparison["request"]
            != json!({"model":"pegassus","swarm_mode":"fast","max_tokens":256,"stream":true})
        || comparison["runtime_settings"]["max_output_tokens"] != 256
        || comparison["arms_config"]["python"]["max_output_tokens"] != 256
        || comparison["arms_config"]["rust"]["max_output_tokens"] != 256
        || comparison["arms_config"]["python"]["mode"] != "fast"
        || phases.as_array().is_none_or(|phases| phases.len() != 7)
        || phases[0]["idle_seconds"] != 2
        || phases[3]["repetitions"] != 3
        || phases[4]["idle_seconds"] != 10
        || plan["sampling"]["interval_ms"] != 1000
    {
        return Err(RunError::InvalidEvidence);
    }
    Ok(())
}

async fn bounded<T>(future: DriverFuture<'_, T>) -> Result<T, RunError> {
    tokio::time::timeout(DEADLINE, future)
        .await
        .map_err(|_| RunError::TimedOut)?
}

#[derive(Default)]
struct RunEvidence {
    last_millis: Option<u64>,
    identities: BTreeMap<u32, (ProcessIdentity, Role)>,
    requests: Vec<Value>,
}
impl RunEvidence {
    fn validate_model(
        &mut self,
        phase: Phase,
        evidence: &PhaseEvidence,
        model: &str,
    ) -> Result<(), RunError> {
        let expected_count = match phase {
            Phase::ColdText | Phase::Resume => 1,
            Phase::ToolRoundtrip => 2,
            Phase::WarmRepeat => 3,
            _ => 0,
        };
        if evidence.exchanges.len() != expected_count
            || evidence.snapshots.is_empty()
            || evidence.snapshots.len() > 128
        {
            return Err(RunError::InvalidEvidence);
        }
        let before = self.last_millis.unwrap_or(0);
        for snapshot in &evidence.snapshots {
            if snapshot.phase != phase
                || snapshot.diagnostic_version != 1
                || snapshot.processes.is_empty()
                || snapshot.processes.len() > MAX_TARGETS
            {
                return Err(RunError::InvalidEvidence);
            }
            if self
                .last_millis
                .is_some_and(|last| snapshot.elapsed_millis < last)
            {
                return Err(RunError::ClockRegressed);
            }
            self.last_millis = Some(snapshot.elapsed_millis);
            let mut seen = HashSet::new();
            for process in &snapshot.processes {
                if !seen.insert(process.identity.pid())
                    || process.rss.availability() != Availability::Available
                {
                    return Err(RunError::InvalidEvidence);
                }
                if let Some(previous) = self.identities.insert(
                    process.identity.pid(),
                    (process.identity.clone(), process.role),
                ) {
                    if previous != (process.identity.clone(), process.role) {
                        return Err(RunError::IdentityChanged);
                    }
                }
            }
            if self.identities.len() > MAX_TARGETS {
                return Err(RunError::Limit);
            }
            for role in [Role::Runtime, Role::Coordinator, Role::Collector] {
                if !snapshot
                    .processes
                    .iter()
                    .any(|process| process.role == role)
                {
                    return Err(RunError::InvalidEvidence);
                }
            }
        }
        let minimum = match phase {
            Phase::ListenerReady => 2000,
            Phase::Idle => 10000,
            _ => 0,
        };
        if self.last_millis.unwrap_or(0).saturating_sub(before) < minimum {
            return Err(RunError::InvalidEvidence);
        }
        let plan = plan();
        let payloads = &plan["payloads"];
        let mut messages = if phase == Phase::ToolRoundtrip {
            payloads["tool_messages"].clone()
        } else {
            payloads["text_messages"].clone()
        };
        for (index, exchange) in evidence.exchanges.iter().enumerate() {
            let mut expected = json!({"model":model,"messages":messages,"stream":false,
                "max_tokens":plan["comparison"]["request"]["max_tokens"],"temperature":0,"chat_template_kwargs":{"enable_thinking":false}});
            if phase == Phase::ToolRoundtrip {
                expected["tools"] = payloads["tools"].clone();
            }
            if exchange.backend_request != expected {
                return Err(RunError::UnequalWork);
            }
            let response = &exchange.completion;
            if response["choices"]
                .as_array()
                .is_none_or(|choices| choices.len() != 1)
                || response["model"] != "pegassus"
            {
                return Err(RunError::InvalidEvidence);
            }
            let message = &response["choices"][0]["message"];
            if message["role"] != "assistant" {
                return Err(RunError::InvalidEvidence);
            }
            for key in ["prompt_tokens", "completion_tokens", "total_tokens"] {
                if response["usage"][key].as_u64().is_none() {
                    return Err(RunError::InvalidEvidence);
                }
            }
            let prompt = response["usage"]["prompt_tokens"].as_u64().unwrap();
            let output = response["usage"]["completion_tokens"].as_u64().unwrap();
            if output > 256
                || prompt.checked_add(output) != response["usage"]["total_tokens"].as_u64()
            {
                return Err(RunError::InvalidEvidence);
            }
            if phase == Phase::ToolRoundtrip && index == 0 {
                let calls = message["tool_calls"]
                    .as_array()
                    .filter(|calls| calls.len() == 1)
                    .ok_or(RunError::InvalidEvidence)?;
                let call = &calls[0];
                let id = call["id"]
                    .as_str()
                    .filter(|id| {
                        !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
                    })
                    .ok_or(RunError::InvalidEvidence)?;
                let arguments: Value = serde_json::from_str(
                    call["function"]["arguments"]
                        .as_str()
                        .filter(|text| text.len() <= 1024)
                        .ok_or(RunError::InvalidEvidence)?,
                )
                .map_err(|_| RunError::InvalidEvidence)?;
                if call["type"] != "function"
                    || call["function"]["name"] != "read"
                    || arguments != payloads["expected_tool_arguments"]
                    || response["choices"][0]["finish_reason"] != "tool_calls"
                {
                    return Err(RunError::InvalidEvidence);
                }
                messages
                    .as_array_mut()
                    .ok_or(RunError::InvalidEvidence)?
                    .push(message.clone());
                messages.as_array_mut().unwrap().push(json!({"role":"tool","tool_call_id":id,"content":payloads["synthetic_tool_result"]}));
            } else {
                let expected_text = if phase == Phase::ToolRoundtrip {
                    &payloads["expected_final_text"]
                } else {
                    &payloads["expected_text"]
                };
                if message["content"].as_str().map(str::trim) != expected_text.as_str()
                    || message
                        .get("tool_calls")
                        .is_some_and(|calls| calls != &Value::Null && calls != &json!([]))
                    || response["choices"][0]["finish_reason"] != "stop"
                {
                    return Err(RunError::InvalidEvidence);
                }
            }
            let mut canonical = exchange.backend_request.clone();
            if phase == Phase::ToolRoundtrip && index == 1 {
                canonical["messages"][1]["tool_calls"][0]["id"] = "<call-0>".into();
                canonical["messages"][2]["tool_call_id"] = "<call-0>".into();
            }
            self.requests.push(canonical);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
