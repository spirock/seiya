//! Owned launcher/probes/HTTP core. Public fixture entry is synthetic only;
//! the separate CampaignDriver gates recipe execution and model-load approval.
mod http;
mod process;
pub use http::{serve_fixture, serve_recipe_fixture};
pub use process::Paths;

use crate::runner::{self, Arm, Driver, DriverFuture, Exchange, PhaseEvidence, RunError};
use crate::{probes, Collector, Phase, Role, Snapshot};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub struct FixtureDriver {
    paths: Paths,
    directory: PathBuf,
    assets: Vec<process::Asset>,
    // Drop closes coordinator before the synthetic runtime. Never an adopted PID.
    coordinator: Option<process::Owned>,
    runtime: Option<process::Owned>,
    proxy: Option<http::Proxy>,
    client: reqwest::Client,
    run: usize,
    arm: Arm,
    key: String,
    url: String,
    epoch: Instant,
    partial: Vec<Snapshot>,
    probe: Option<probes::Adapter<probes::NativeTable<'static>, probes::CommandExecutor>>,
    cleanup: Vec<Value>,
    diagnostics: Vec<Value>,
    system: Option<probes::SystemReadings>,
    config_hash: Option<String>,
    unowned_cleanup_handles: bool,
    campaign: Option<crate::campaign::driver::CampaignSetup>,
    completed_requests: Vec<Value>,
    pending_requests: Vec<Value>,
    trace_rejected: bool,
    runtime_memory: bool,
}

impl FixtureDriver {
    pub(crate) fn enable_runtime_memory(&mut self) -> Result<(), RunError> {
        if !cfg!(target_os = "macos") || self.campaign.as_ref().is_none_or(|s| s.is_rehearsal()) {
            return Err(RunError::Preparation);
        }
        self.runtime_memory = true;
        Ok(())
    }
    pub(crate) fn configure_calibration(
        &mut self,
        policy: crate::campaign::CalibrationPolicy,
        approval: Option<crate::campaign::CalibrationApproval>,
    ) -> Result<(), RunError> {
        self.campaign
            .as_mut()
            .ok_or(RunError::AuthorizationRequired)?
            .configure_calibration(policy, approval)
    }
    /// Operator paths only. Runs bounded --version/shasum on known code assets;
    /// does not create a campaign directory, start models or adopt any endpoint.
    pub async fn prepare(paths: Paths, directory: PathBuf) -> Result<Self, RunError> {
        Self::prepare_inner(paths, directory, None).await
    }
    pub(crate) async fn prepare_campaign(
        paths: Paths,
        directory: PathBuf,
        setup: crate::campaign::driver::CampaignSetup,
    ) -> Result<Self, RunError> {
        Self::prepare_inner(paths, directory, Some(setup)).await
    }
    async fn prepare_inner(
        paths: Paths,
        directory: PathBuf,
        campaign: Option<crate::campaign::driver::CampaignSetup>,
    ) -> Result<Self, RunError> {
        if !cfg!(target_os = "macos") {
            return Err(RunError::Driver);
        }
        let parent = directory
            .parent()
            .ok_or(RunError::Artifact)?
            .canonicalize()
            .map_err(|_| RunError::Artifact)?;
        let name = directory.file_name().ok_or(RunError::Artifact)?;
        let directory = parent.join(name);
        if directory.exists() {
            return Err(RunError::Artifact);
        }
        let (paths, assets) = if campaign.as_ref().is_none_or(|s| s.is_rehearsal()) {
            paths.inspect().await?
        } else {
            paths.inspect_mode(false).await?
        };
        Ok(Self {
            paths,
            directory,
            assets,
            coordinator: None,
            runtime: None,
            proxy: None,
            client: http::client()?,
            run: 0,
            arm: Arm::Rust,
            key: String::new(),
            url: String::new(),
            epoch: Instant::now(),
            partial: vec![],
            probe: None,
            cleanup: vec![],
            diagnostics: vec![],
            system: None,
            config_hash: None,
            unowned_cleanup_handles: false,
            campaign,
            completed_requests: vec![],
            pending_requests: vec![],
            trace_rejected: false,
            runtime_memory: false,
        })
    }
    pub fn directory(&self) -> PathBuf {
        self.directory.clone()
    }

    async fn start(&mut self, arm: Arm) -> Result<(), RunError> {
        if self.runtime.is_some() || self.coordinator.is_some() || self.proxy.is_some() {
            return Err(RunError::Unconfirmed);
        }
        self.run += 1;
        self.arm = arm;
        self.cleanup.clear();
        self.partial.clear();
        self.completed_requests.clear();
        self.pending_requests.clear();
        self.trace_rejected = false;
        self.config_hash = None;
        self.unowned_cleanup_handles = false;
        if let Some(setup) = &mut self.campaign {
            setup.prepare_run().await?;
        }
        let work = self.directory.join(format!("run-{:03}", self.run));
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        builder.create(&work).map_err(|_| RunError::Artifact)?;
        let home = work.join("home");
        fs::create_dir(&home).map_err(|_| RunError::Artifact)?;
        self.key = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let (runtime_port, mut command) = if let Some(setup) = &mut self.campaign {
            let port = setup.release_runtime()?;
            let mut command = process::clean(Command::new(&setup.recipe.python), &home);
            if self.runtime_memory {
                command.args(["-c", crate::runtime_memory_channel::BOOTSTRAP]);
            }
            command.args(
                setup
                    .recipe
                    .runtime_arguments(port)
                    .map_err(|_| RunError::Preparation)?,
            );
            if let Some(binary) = &setup.rehearsal_binary {
                command.env("SEIYA_REHEARSAL_BINARY", binary);
            }
            (port, command)
        } else {
            let (reservation, port) = process::reserve_port()?;
            let mut command = process::clean(Command::new(&self.paths.fixture_runtime), &home);
            command.args(["--fixture-runtime", "--port", &port.to_string()]);
            drop(reservation);
            (port, command)
        };
        command.current_dir(&work);
        self.runtime = Some(if self.runtime_memory {
            process::Owned::spawn_instrumented(&mut command)?
        } else {
            process::Owned::spawn(&mut command, Role::Runtime)?
        });
        wait_listener(
            self.runtime.as_mut().unwrap(),
            runtime_port,
            if self.campaign.is_some() { 120 } else { 10 },
            self.campaign.as_mut(),
        )
        .await?;
        self.proxy = Some(http::Proxy::start(runtime_port).await?);
        let backend = format!("{}/v1", self.proxy.as_ref().unwrap().url);
        let (reservation, coordinator_port) = if let Some(setup) = &mut self.campaign {
            (None, setup.release_coordinator()?)
        } else {
            let (listener, port) = process::reserve_port()?;
            (Some(listener), port)
        };
        let model = self.backend_model();
        let config = match arm {
            Arm::Python => {
                json!({"mode":"fast","worker":"fixture-worker","timeout_seconds":180,"max_output_tokens":256,
                "models":[{"id":"fixture-worker","model":model,"backend":"mlx","base_url":backend}]})
            }
            Arm::Rust => {
                json!({"listen":format!("127.0.0.1:{coordinator_port}"),"api_key_env":"SEIYA_BENCH_KEY",
                "runtime":{"id":"fixture-worker","model":model,"backend":"mlx","base_url":backend,"timeout_seconds":180,"max_output_tokens":256}})
            }
        };
        let config_path = work.join("config.json");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&config_path)
            .map_err(|_| RunError::Artifact)?;
        file.write_all(&serde_json::to_vec(&config).map_err(|_| RunError::Artifact)?)
            .map_err(|_| RunError::Artifact)?;
        self.config_hash = Some(process::hash(&config_path).await?);
        let mut command = match arm {
            Arm::Python => {
                let mut command = process::clean(Command::new(&self.paths.python), &home);
                command
                    .args(["-m", "seiya", "serve", "--config"])
                    .arg(&config_path)
                    .args(["--port", &coordinator_port.to_string()])
                    .env("PYTHONPATH", &self.paths.python_source)
                    .env("SEIYA_API_KEY", &self.key);
                command
            }
            Arm::Rust => {
                let mut command = process::clean(Command::new(&self.paths.rust_server), &home);
                command
                    .args(["serve", "--config"])
                    .arg(&config_path)
                    .env("SEIYA_BENCH_KEY", &self.key);
                command
            }
        };
        command.current_dir(&work);
        drop(reservation);
        self.coordinator = Some(process::Owned::spawn(&mut command, Role::Coordinator)?);
        wait_listener(
            self.coordinator.as_mut().unwrap(),
            coordinator_port,
            10,
            self.campaign.as_mut(),
        )
        .await?;
        self.url = format!("http://127.0.0.1:{coordinator_port}");
        for (route, models) in [("/health", false), ("/v1/models", true)] {
            let response = self
                .client
                .get(format!("{}{route}", self.url))
                .bearer_auth(&self.key)
                .send()
                .await
                .map_err(|_| RunError::Driver)?;
            if !response.status().is_success() {
                return Err(RunError::Driver);
            }
            let value: Value = serde_json::from_slice(&http::bytes(response).await?)
                .map_err(|_| RunError::Driver)?;
            if models && value["data"][0]["id"] != "pegassus" || !models && value["busy"] != false {
                return Err(RunError::InvalidEvidence);
            }
        }
        self.runtime.as_mut().unwrap().verify()?;
        self.coordinator.as_mut().unwrap().verify()?;
        self.proxy
            .as_ref()
            .unwrap()
            .trace
            .enabled
            .store(true, Ordering::Release);
        self.epoch = Instant::now();
        self.probe = Some(
            probes::for_current_collector(Duration::from_millis(500))
                .map_err(|_| RunError::Driver)?,
        );
        Ok(())
    }

    async fn sample(&mut self, phase: Phase) -> Result<(), RunError> {
        let started = Instant::now();
        if let Some(setup) = &mut self.campaign {
            setup
                .monitor_pressure(
                    matches!(
                        phase,
                        Phase::ColdText | Phase::ToolRoundtrip | Phase::WarmRepeat | Phase::Resume
                    ),
                    false,
                )
                .await?;
        }
        if self.partial.len() >= 128 {
            return Err(RunError::Limit);
        }
        let runtime = self.runtime.as_mut().ok_or(RunError::Driver)?;
        runtime.verify()?;
        let runtime_target = runtime.target.clone();
        let coordinator = self.coordinator.as_mut().ok_or(RunError::Driver)?;
        coordinator.verify()?;
        let coordinator_target = coordinator.target.clone();
        let probe = self.probe.as_mut().ok_or(RunError::Driver)?;
        let mut readings = probe.sample().await.map_err(|_| RunError::Driver)?;
        let mut targets = readings.targets();
        for known in [&runtime_target, &coordinator_target] {
            let target = targets
                .iter_mut()
                .find(|target| target.identity.pid() == known.identity.pid())
                .ok_or(RunError::IdentityChanged)?;
            if target.identity != known.identity {
                return Err(RunError::IdentityChanged);
            }
            target.role = known.role;
        }
        // Observation of ancestry does not grant a kill/reap handle. The fixture
        // recipe expects no extra process; if one appears, never claim full tree
        // cleanup or proceed to the next arm on the strength of root wait alone.
        self.unowned_cleanup_handles |=
            targets.iter().any(|target| target.role == Role::Descendant);
        self.runtime.as_mut().unwrap().verify()?;
        self.coordinator.as_mut().unwrap().verify()?;
        let elapsed =
            u64::try_from(self.epoch.elapsed().as_millis()).map_err(|_| RunError::Limit)?;
        let snapshot = Collector::new(targets, 1)
            .map_err(|_| RunError::InvalidEvidence)?
            .collect(elapsed, phase, &mut readings)
            .map_err(|_| RunError::InvalidEvidence)?;
        self.partial.push(snapshot);
        if self.runtime_memory {
            let started = self.epoch.elapsed().as_millis();
            let remaining = self
                .campaign
                .as_ref()
                .ok_or(RunError::Preparation)?
                .remaining_runtime()?;
            let wait = remaining.map_or(crate::runtime_memory_channel::MAX_WAIT, |r| {
                r.min(crate::runtime_memory_channel::MAX_WAIT)
            });
            let result = self
                .runtime
                .as_mut()
                .ok_or(RunError::Driver)?
                .memory_sample(wait)
                .await;
            self.diagnostics
                .push(json!({"runtime_memory":result.as_ref().ok(),
                "runtime_memory_failed":result.is_err(),"identity":runtime_target.identity,
                "phase":phase,"collector_started_millis":started,
                "collector_finished_millis":self.epoch.elapsed().as_millis()}));
            result?;
            if let Some(setup) = &self.campaign {
                setup.ensure_current()?;
                setup.remaining_runtime()?;
            }
        }
        self.diagnostics.push(
            json!({"elapsed_millis":elapsed,"collection_micros":started.elapsed().as_micros()}),
        );
        Ok(())
    }

    async fn complete(&mut self, phase: Phase, body: Value) -> Result<Exchange, RunError> {
        if let Some(setup) = &mut self.campaign {
            setup.ensure_current()?;
            setup.monitor_pressure(true, true).await?;
        }
        let trace = self.proxy.as_ref().ok_or(RunError::Driver)?.trace.clone();
        let work = http::exchange(
            self.client.clone(),
            self.url.clone(),
            self.key.clone(),
            body,
            trace,
        );
        tokio::pin!(work);
        self.sample(phase).await?;
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        let remaining = self
            .campaign
            .as_ref()
            .map(|setup| setup.remaining_runtime())
            .transpose()?
            .flatten();
        let expiry = async {
            match remaining {
                Some(remaining) => tokio::time::sleep(remaining).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::pin!(expiry);
        let result = loop {
            tokio::select! { biased;
                _ = &mut expiry => break Err(RunError::CalibrationDeadline),
                result = &mut work => break result,
                _ = interval.tick() => self.sample(phase).await?,
            }
        };
        if let Ok(exchange) = &result {
            self.completed_requests.push(json!({"backend_request":exchange.backend_request,"timing":exchange.timing,
                "finish_reason":exchange.completion["choices"][0]["finish_reason"],"usage":exchange.completion["usage"]}));
        }
        self.sample(phase).await?;
        result
    }

    async fn idle(&mut self, phase: Phase, seconds: u64) -> Result<(), RunError> {
        let end = tokio::time::Instant::now() + Duration::from_secs(seconds);
        self.sample(phase).await?;
        loop {
            let next = (tokio::time::Instant::now() + Duration::from_secs(1)).min(end);
            tokio::time::sleep_until(next).await;
            self.sample(phase).await?;
            if tokio::time::Instant::now() >= end {
                break;
            }
        }
        Ok(())
    }

    async fn perform(&mut self, phase: Phase) -> Result<PhaseEvidence, RunError> {
        self.partial.clear();
        self.completed_requests.clear();
        self.diagnostics.clear();
        self.system = None;
        self.audit_trace()?;
        if matches!(phase, Phase::ListenerReady | Phase::Shutdown) {
            self.system = Some(
                self.probe
                    .as_mut()
                    .ok_or(RunError::Driver)?
                    .sample_system()
                    .await
                    .map_err(|_| RunError::Driver)?,
            );
        }
        let payloads = runner::plan()["payloads"].clone();
        let mut exchanges = Vec::new();
        match phase {
            Phase::ListenerReady => self.idle(phase, 2).await?,
            Phase::Idle => self.idle(phase, 10).await?,
            Phase::Shutdown => self.sample(phase).await?,
            Phase::ToolRoundtrip => {
                let mut body = request(payloads["tool_messages"].clone());
                body["tools"] = payloads["tools"].clone();
                let first = self.complete(phase, body.clone()).await?;
                let message = &first.completion["choices"][0]["message"];
                let id = message["tool_calls"][0]["id"]
                    .as_str()
                    .ok_or(RunError::InvalidEvidence)?;
                body["messages"]
                    .as_array_mut()
                    .unwrap()
                    .push(message.clone());
                body["messages"].as_array_mut().unwrap().push(json!({"role":"tool","tool_call_id":id,"content":payloads["synthetic_tool_result"]}));
                exchanges.push(first);
                exchanges.push(self.complete(phase, body).await?);
            }
            _ => {
                for _ in 0..if phase == Phase::WarmRepeat { 3 } else { 1 } {
                    exchanges.push(
                        self.complete(phase, request(payloads["text_messages"].clone()))
                            .await?,
                    );
                }
            }
        }
        self.audit_trace()?;
        Ok(PhaseEvidence {
            exchanges,
            snapshots: std::mem::take(&mut self.partial),
        })
    }

    fn audit_trace(&self) -> Result<(), RunError> {
        let trace = &self.proxy.as_ref().ok_or(RunError::Driver)?.trace;
        if trace.unexpected.load(Ordering::Acquire)
            || trace.busy.load(Ordering::Acquire)
            || !trace.requests.lock().unwrap().is_empty()
        {
            return Err(RunError::UnequalWork);
        }
        Ok(())
    }
}

fn request(messages: Value) -> Value {
    json!({"model":"pegassus","swarm_mode":"fast","stream":true,"max_tokens":256,"messages":messages})
}

async fn wait_listener(
    process: &mut process::Owned,
    port: u16,
    seconds: u64,
    mut campaign: Option<&mut crate::campaign::driver::CampaignSetup>,
) -> Result<(), RunError> {
    let remaining = campaign
        .as_ref()
        .map(|setup| setup.remaining_runtime())
        .transpose()?
        .flatten();
    let timeout = remaining.map_or(Duration::from_secs(seconds), |duration| {
        duration.min(Duration::from_secs(seconds))
    });
    let calibration_deadline =
        remaining.is_some_and(|duration| duration <= Duration::from_secs(seconds));
    tokio::time::timeout(timeout, async {
        let end = tokio::time::Instant::now() + Duration::from_secs(seconds);
        loop {
            if let Some(setup) = campaign.as_mut() {
                setup.monitor_pressure(false, false).await?;
            }
            if process.port_owned(port).await? {
                return Ok(());
            }
            if tokio::time::Instant::now() >= end {
                return Err(RunError::TimedOut);
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .map_err(|_| {
        if calibration_deadline {
            RunError::CalibrationDeadline
        } else {
            RunError::TimedOut
        }
    })?
}

impl Driver for FixtureDriver {
    fn set_cancellation(&mut self, receiver: tokio::sync::watch::Receiver<bool>) {
        if let Some(setup) = &mut self.campaign {
            setup.set_cancellation(receiver);
        }
    }
    fn backend_model(&self) -> &str {
        self.campaign
            .as_ref()
            .and_then(|setup| setup.recipe.checkpoint.to_str())
            .unwrap_or("synthetic-checkpoint")
    }
    fn is_simulated(&self) -> bool {
        false
    }
    fn artifact_metadata(&self) -> Value {
        json!({"backend":if self.campaign.as_ref().is_some_and(|s| !s.is_rehearsal()) {"owned-mlx-recipe"} else {"owned-synthetic-process"},
            "model_inference":self.campaign.as_ref().is_some_and(|s| !s.is_rehearsal()),"assets":self.assets,"preparation":self.campaign.as_ref().map(|s| s.state()),
            "runtime_memory_enabled":self.runtime_memory,
            "runtime_memory_bootstrap":if self.runtime_memory { Some(crate::runtime_memory_channel::BOOTSTRAP) } else { None }})
    }
    fn resource_state(&self) -> Value {
        json!({"runtime":self.runtime.as_ref().map(|child| &child.target),"coordinator":self.coordinator.as_ref().map(|child| &child.target),
            "config_sha256":self.config_hash,"preparation":self.campaign.as_ref().map(|s| s.state()),"descendant_cleanup_unconfirmed":self.unowned_cleanup_handles,"cleanup":self.cleanup,
            "completed_requests":self.completed_requests,"pending_backend_requests":self.proxy.as_ref().map_or_else(|| self.pending_requests.clone(), |p| p.trace.requests.lock().unwrap().clone()),
            "unexpected_requests":self.trace_rejected || self.proxy.as_ref().is_some_and(|p| p.trace.unexpected.load(Ordering::Acquire))})
    }
    fn partial_snapshots(&mut self) -> Vec<Snapshot> {
        std::mem::take(&mut self.partial)
    }
    fn phase_metadata(&self) -> Value {
        json!({"sampling":self.diagnostics,"system":self.system,"completed_requests":self.completed_requests,"sampling_policy":"boundary-and-best-effort-1000ms","ttfb_is_token_latency":false})
    }
    fn start_run(&mut self, arm: Arm, _: Value) -> DriverFuture<'_, ()> {
        Box::pin(async move { self.start(arm).await })
    }
    fn phase(&mut self, phase: Phase) -> DriverFuture<'_, PhaseEvidence> {
        Box::pin(async move { self.perform(phase).await })
    }
    fn stop_run(&mut self) -> DriverFuture<'_, bool> {
        Box::pin(async move {
            let mut confirmed = !self.unowned_cleanup_handles;
            if let Some(mut child) = self.coordinator.take() {
                let result = child.stop(self.arm == Arm::Python).await;
                self.cleanup.push(json!({"target":child.target,"forced":result.as_ref().ok(),"confirmed":result.is_ok()}));
                confirmed &= result.is_ok();
            }
            if let Some(mut child) = self.runtime.take() {
                let result = child.stop(false).await;
                self.cleanup.push(json!({"target":child.target,"forced":result.as_ref().ok(),"confirmed":result.is_ok()}));
                confirmed &= result.is_ok();
            }
            if let Some(mut proxy) = self.proxy.take() {
                self.pending_requests = proxy.trace.requests.lock().unwrap().clone();
                self.trace_rejected = proxy.trace.unexpected.load(Ordering::Acquire);
                confirmed &= !proxy.trace.unexpected.load(Ordering::Acquire)
                    && proxy.trace.requests.lock().unwrap().is_empty()
                    && !proxy.trace.busy.load(Ordering::Acquire);
                confirmed &= proxy.stop().await;
            }
            self.probe = None;
            self.key.clear();
            self.url.clear();
            Ok(confirmed)
        })
    }
}

#[cfg(test)]
mod tests;
