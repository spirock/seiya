use super::*;
use crate::{Collector, Observation, Provider, Target};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let base =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/observe-runner-fixtures");
        fs::create_dir_all(&base).unwrap();
        Self(base.join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn summary(&self) -> Value {
        serde_json::from_slice(&fs::read(self.0.join("summary.json")).unwrap()).unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        // Known files in our new test-only directory; never recursive cleanup.
        for name in [
            "manifest.json",
            "events.jsonl",
            "summary.json",
            "aborted.json",
            "fixture-marker",
        ] {
            let _ = fs::remove_file(self.0.join(name));
        }
        let _ = fs::remove_dir(&self.0);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    Budget,
    Finish,
    Rss,
    Clock,
    Stop,
    Start,
    Pending,
    Role,
    Real,
    Encoding,
}
#[derive(Default)]
struct State {
    starts: Vec<Arm>,
    stops: usize,
    cold_started: bool,
}
struct Fake {
    fault: Fault,
    state: Arc<Mutex<State>>,
    millis: u64,
    gate: Option<Arc<Semaphore>>,
}
fn fake(fault: Fault) -> (Fake, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State::default()));
    (
        Fake {
            fault,
            state: state.clone(),
            millis: 0,
            gate: None,
        },
        state,
    )
}
struct Samples;
impl Provider for Samples {
    fn observe(&mut self, identity: &ProcessIdentity) -> Observation {
        Observation::Present {
            identity: identity.clone(),
            rss_kib: 12,
        }
    }
}
fn snapshots(phase: Phase, millis: &mut u64) -> Vec<Snapshot> {
    let count = match phase {
        Phase::ListenerReady => 2,
        Phase::Idle => 10,
        _ => 1,
    };
    let targets = [Role::Runtime, Role::Coordinator, Role::Collector]
        .into_iter()
        .enumerate()
        .map(|(index, role)| Target {
            identity: ProcessIdentity::new(10 + index as u32, "synthetic-start").unwrap(),
            role,
        })
        .collect();
    let mut collector = Collector::new(targets, count).unwrap();
    (0..count)
        .map(|_| {
            *millis += 1000;
            collector.collect(*millis, phase, &mut Samples).unwrap()
        })
        .collect()
}
fn completed(message: Value, finish: &str) -> Value {
    json!({"model":"pegassus","choices":[{"message":message,"finish_reason":finish}],
        "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}})
}
fn exchanges(phase: Phase, run: usize) -> Vec<Exchange> {
    let plan = plan();
    let payloads = &plan["payloads"];
    let count = match phase {
        Phase::ColdText | Phase::Resume => 1,
        Phase::ToolRoundtrip => 2,
        Phase::WarmRepeat => 3,
        _ => 0,
    };
    let mut messages = if phase == Phase::ToolRoundtrip {
        payloads["tool_messages"].clone()
    } else {
        payloads["text_messages"].clone()
    };
    let mut result = Vec::new();
    for index in 0..count {
        let mut request = json!({"model":"synthetic-checkpoint","messages":messages,"stream":false,
            "max_tokens":256,"temperature":0,"chat_template_kwargs":{"enable_thinking":false}});
        let completion = if phase == Phase::ToolRoundtrip {
            request["tools"] = payloads["tools"].clone();
            if index == 0 {
                let id = format!("call-{run}");
                let message = json!({"role":"assistant","content":null,"tool_calls":[{"id":id,
                    "type":"function","function":{"name":"read","arguments":"{\"path\":\"fixture.txt\"}"}}]});
                messages.as_array_mut().unwrap().push(message.clone());
                messages.as_array_mut().unwrap().push(json!({"role":"tool","tool_call_id":id,"content":payloads["synthetic_tool_result"]}));
                completed(message, "tool_calls")
            } else {
                completed(
                    json!({"role":"assistant","content":payloads["expected_final_text"]}),
                    "stop",
                )
            }
        } else {
            completed(
                json!({"role":"assistant","content":payloads["expected_text"]}),
                "stop",
            )
        };
        result.push(Exchange {
            backend_request: request,
            completion,
            timing: None,
        });
    }
    result
}

#[tokio::test]
async fn calibration_runner_executes_one_rust_arm_and_one_request_not_abba() {
    let directory = Directory::new();
    let (driver, state) = fake(Fault::None);
    let report = launch(directory.0.clone(), driver, "calibration-rehearsal")
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(report.completed);
    assert_eq!(report.runs_completed, 1);
    assert_eq!(state.lock().unwrap().starts, vec![Arm::Rust]);
    let manifest: Value =
        serde_json::from_slice(&fs::read(directory.0.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["model_inference"], false);
    assert_eq!(manifest["execution"]["runs"], 1);
    assert_eq!(
        manifest["execution"]["phases"],
        json!(["listener-ready", "cold-text", "shutdown"])
    );
    let events = fs::read_to_string(directory.0.join("events.jsonl")).unwrap();
    let phases: Vec<Value> = events
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|e| e["event"] == "phase")
        .collect();
    assert_eq!(phases.len(), 3);
    assert_eq!(
        phases
            .iter()
            .map(|e| e["requests"].as_array().unwrap().len())
            .sum::<usize>(),
        1
    );
}

#[test]
fn campaign_validates_the_exact_checkpoint_identity_not_the_old_synthetic_constant() {
    let model = "/synthetic/local checkpoint";
    let mut millis = 0;
    let mut phase = PhaseEvidence {
        exchanges: exchanges(Phase::ColdText, 1),
        snapshots: snapshots(Phase::ColdText, &mut millis),
    };
    assert_eq!(
        RunEvidence::default().validate_model(Phase::ColdText, &phase, model),
        Err(RunError::UnequalWork)
    );
    phase.exchanges[0].backend_request["model"] = model.into();
    assert!(RunEvidence::default()
        .validate_model(Phase::ColdText, &phase, model)
        .is_ok());
    phase.exchanges[0].backend_request["model"] = "/synthetic/other".into();
    assert_eq!(
        RunEvidence::default().validate_model(Phase::ColdText, &phase, model),
        Err(RunError::UnequalWork)
    );
}
impl Driver for Fake {
    fn is_simulated(&self) -> bool {
        self.fault != Fault::Real
    }
    fn start_run(&mut self, arm: Arm, _: Value) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            self.state.lock().unwrap().starts.push(arm);
            self.millis = 0;
            if self.fault == Fault::Start {
                Err(RunError::Driver)
            } else {
                Ok(())
            }
        })
    }
    fn phase(&mut self, phase: Phase) -> DriverFuture<'_, PhaseEvidence> {
        Box::pin(async move {
            if phase == Phase::ColdText {
                self.state.lock().unwrap().cold_started = true;
                if let Some(gate) = &self.gate {
                    gate.acquire().await.unwrap().forget();
                }
                if self.fault == Fault::Pending {
                    std::future::pending::<()>().await;
                }
            }
            let mut evidence = PhaseEvidence {
                exchanges: exchanges(phase, self.state.lock().unwrap().starts.len()),
                snapshots: snapshots(phase, &mut self.millis),
            };
            if phase == Phase::ToolRoundtrip
                && self.fault == Fault::Encoding
                && self.state.lock().unwrap().starts.len() == 2
            {
                let literal = "{ \"path\" : \"fixture.txt\" }";
                evidence.exchanges[0].completion["choices"][0]["message"]["tool_calls"][0]
                    ["function"]["arguments"] = literal.into();
                evidence.exchanges[1].backend_request["messages"][1]["tool_calls"][0]["function"]
                    ["arguments"] = literal.into();
            }
            if phase == Phase::ColdText {
                match self.fault {
                    Fault::Budget => {
                        evidence.exchanges[0].backend_request["max_tokens"] = 512.into()
                    }
                    Fault::Finish => {
                        evidence.exchanges[0].completion["choices"][0]["finish_reason"] =
                            "length".into()
                    }
                    Fault::Rss => evidence.snapshots[0].processes.clear(),
                    Fault::Clock => evidence.snapshots[0].elapsed_millis = 0,
                    Fault::Role => evidence.snapshots[0].processes[0].role = Role::Collector,
                    _ => (),
                }
            }
            Ok(evidence)
        })
    }
    fn stop_run(&mut self) -> DriverFuture<'_, bool> {
        Box::pin(async move {
            self.state.lock().unwrap().stops += 1;
            Ok(self.fault != Fault::Stop)
        })
    }
}

#[tokio::test]
async fn balanced_simulation_writes_auditable_artifacts_and_normalizes_only_analytic_ids() {
    let directory = Directory::new();
    let (driver, state) = fake(Fault::None);
    let report = launch_simulation(directory.0.clone(), driver)
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(report.completed && report.cleanup_confirmed);
    assert_eq!(report.runs_completed, 12);
    let state = state.lock().unwrap();
    assert_eq!(
        state.starts,
        [Arm::Python, Arm::Rust, Arm::Rust, Arm::Python].repeat(3)
    );
    assert_eq!(state.stops, 12);
    let manifest: Value =
        serde_json::from_slice(&fs::read(directory.0.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["mode"], "simulation");
    assert_eq!(manifest["real_host_measured"], false);
    let events: Vec<Value> = fs::read_to_string(directory.0.join("events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event"] == "phase")
            .count(),
        84
    );
    let tool_events: Vec<_> = events
        .iter()
        .filter(|event| event["event"] == "phase" && event["phase"] == "tool-roundtrip")
        .collect();
    assert_eq!(
        tool_events[0]["requests"][1]["messages"][2]["tool_call_id"],
        "call-1"
    );
    assert_eq!(
        tool_events[1]["requests"][1]["messages"][2]["tool_call_id"],
        "call-2"
    );
    assert_eq!(directory.summary()["completed"], true);
    assert!(!directory.0.join("aborted.json").exists());
}

#[tokio::test]
async fn mismatches_fail_before_another_arm_and_preserve_partial_events_and_summary() {
    for fault in [
        Fault::Budget,
        Fault::Finish,
        Fault::Rss,
        Fault::Clock,
        Fault::Role,
        Fault::Start,
    ] {
        let directory = Directory::new();
        let (driver, state) = fake(fault);
        let report = launch_simulation(directory.0.clone(), driver)
            .unwrap()
            .wait()
            .await
            .unwrap();
        assert!(!report.completed);
        assert!(report.error.is_some());
        assert!(report.cleanup_confirmed);
        let state = state.lock().unwrap();
        assert_eq!(state.starts.len(), 1);
        assert_eq!(state.stops, 1);
        assert_eq!(directory.summary()["completed"], false);
        assert!(directory.0.join("events.jsonl").exists());
    }
}

#[tokio::test]
async fn unconfirmed_stop_is_not_success_or_permission_for_the_other_arm() {
    let directory = Directory::new();
    let (driver, state) = fake(Fault::Stop);
    let report = launch_simulation(directory.0.clone(), driver)
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(report.error, Some(RunError::Unconfirmed));
    assert!(!report.cleanup_confirmed);
    assert_eq!(state.lock().unwrap().starts.len(), 1);
}

#[tokio::test]
async fn cross_arm_work_mismatch_is_not_normalized_away_with_call_ids() {
    let directory = Directory::new();
    let (driver, state) = fake(Fault::Encoding);
    let report = launch_simulation(directory.0.clone(), driver)
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(report.error, Some(RunError::UnequalWork));
    assert_eq!(report.runs_completed, 1);
    assert_eq!(state.lock().unwrap().starts.len(), 2);
    assert_eq!(state.lock().unwrap().stops, 2);
}

#[tokio::test(start_paused = true)]
async fn timed_out_phase_records_failure_and_requests_cleanup_without_retry() {
    let directory = Directory::new();
    let (driver, state) = fake(Fault::Pending);
    let report = launch_simulation(directory.0.clone(), driver)
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(report.error, Some(RunError::TimedOut));
    let state = state.lock().unwrap();
    assert_eq!(state.starts.len(), 1);
    assert_eq!(state.stops, 1);
}

#[tokio::test]
async fn dropping_consumer_requests_cancel_but_drains_inflight_phase_before_stop() {
    let directory = Directory::new();
    let (mut driver, state) = fake(Fault::None);
    let gate = Arc::new(Semaphore::new(0));
    driver.gate = Some(gate.clone());
    let handle = launch_simulation(directory.0.clone(), driver).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !state.lock().unwrap().cold_started {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(handle);
    assert_eq!(state.lock().unwrap().stops, 0);
    gate.add_permits(1);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !directory.0.join("summary.json").exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // File may have just been created: task yields after writing, so wait for stop.
    assert_eq!(state.lock().unwrap().stops, 1);
    let summary = directory.summary();
    assert_eq!(summary["error"], "cancelled");
    assert_eq!(summary["cleanup_confirmed"], true);
    assert_eq!(state.lock().unwrap().starts.len(), 1);
}

#[tokio::test]
async fn existing_output_and_real_driver_are_rejected_without_overwrite_or_start() {
    let directory = Directory::new();
    fs::create_dir(&directory.0).unwrap();
    fs::write(directory.0.join("fixture-marker"), b"keep").unwrap();
    let (driver, state) = fake(Fault::None);
    assert!(matches!(
        launch_simulation(directory.0.clone(), driver),
        Err(RunError::Artifact)
    ));
    assert_eq!(
        fs::read(directory.0.join("fixture-marker")).unwrap(),
        b"keep"
    );
    assert!(state.lock().unwrap().starts.is_empty());
    let fresh = Directory::new();
    let (driver, _) = fake(Fault::Real);
    assert!(matches!(
        launch_simulation(fresh.0.clone(), driver),
        Err(RunError::RealDriverForbidden)
    ));
    assert!(!fresh.0.exists());
}

#[test]
fn artifact_limits_leave_incomplete_marker_not_a_false_summary() {
    let directory = Directory::new();
    let mut artifacts = Artifacts::create(directory.0.clone()).unwrap();
    artifacts.count = MAX_EVENTS;
    assert_eq!(artifacts.event(json!({"event":"no"})), Err(RunError::Limit));
    drop(artifacts);
    assert!(directory.0.join("aborted.json").exists());
    assert!(!directory.0.join("summary.json").exists());
}

#[test]
fn plan_changes_do_not_silently_diverge_from_supported_simulation_protocol() {
    let mut document = plan();
    assert_eq!(validate_plan(&document), Ok(()));
    document["comparison"]["blocks"] = 4.into();
    assert_eq!(validate_plan(&document), Err(RunError::InvalidEvidence));
    document = plan();
    document["workload"][4]["idle_seconds"] = 20.into();
    assert_eq!(validate_plan(&document), Err(RunError::InvalidEvidence));
    document = plan();
    document["comparison"]["arms_config"]["python"]["max_output_tokens"] = 512.into();
    assert_eq!(validate_plan(&document), Err(RunError::InvalidEvidence));
}
