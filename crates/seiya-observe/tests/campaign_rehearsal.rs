//! Opt-in recipe lifecycle integration on synthetic assets only. Never loads MLX.
#![cfg(target_os = "macos")]
use seiya_observe::campaign::CalibrationPolicy;
use seiya_observe::campaign::{
    CampaignDriver, CoordinatorPaths, MlxRecipe, PackageMetadata, RehearsalCondition, Reservation,
};
use seiya_observe::runner::{launch_calibration, launch_campaign, RunError};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn base(variable: &str) -> PathBuf {
    let base = PathBuf::from(
        std::env::var_os(variable).expect("explicit new synthetic output directory required"),
    );
    assert!(base.is_absolute() && !base.exists());
    std::fs::create_dir(&base).unwrap();
    base
}
fn setup(base: &Path) -> (CoordinatorPaths, MlxRecipe, PackageMetadata) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let checkpoint = base.join("checkpoint");
    std::fs::create_dir(&checkpoint).unwrap();
    for (name, value) in [
        ("config.json", json!({"model_type":"synthetic"})),
        ("tokenizer.json", json!({})),
        (
            "tokenizer_config.json",
            json!({"chat_template":"fixture, not inference"}),
        ),
    ] {
        std::fs::write(checkpoint.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
    }
    std::fs::write(checkpoint.join("model.safetensors"), b"abc").unwrap(); // Deliberately not model weights.
    for name in ["mlx", "mlx-lm", "transformers"] {
        std::fs::write(
            base.join(format!("{name}-METADATA")),
            format!("Name: {name}\nVersion: 0.0.fixture\n\n"),
        )
        .unwrap();
    }
    let coordination = base.join("coordination");
    std::fs::create_dir(&coordination).unwrap();
    let python = PathBuf::from("/opt/homebrew/bin/python3");
    let recipe = MlxRecipe {
        python: python.clone(),
        runtime_entrypoint: root.join("crates/seiya-observe/tests/fixtures/mlx_entrypoint.py"),
        checkpoint,
        coordination_directory: coordination,
        minimum_free_bytes: 2 << 30,
    };
    (
        CoordinatorPaths {
            python,
            python_source: root.join("swarm-app"),
            rust_server: root.join("target/debug/seiya-server"),
        },
        recipe,
        PackageMetadata {
            mlx: base.join("mlx-METADATA"),
            mlx_lm: base.join("mlx-lm-METADATA"),
            transformers: base.join("transformers-METADATA"),
        },
    )
}
async fn driver(
    base: &Path,
    paths: CoordinatorPaths,
    recipe: MlxRecipe,
    packages: PackageMetadata,
    condition: RehearsalCondition,
) -> CampaignDriver {
    CampaignDriver::prepare_rehearsal(
        paths,
        base.join("campaign"),
        recipe,
        packages,
        PathBuf::from(env!("CARGO_BIN_EXE_seiya-observe-fixture")),
        condition,
    )
    .await
    .unwrap()
}
fn events(base: &Path) -> Vec<Value> {
    std::fs::read_to_string(base.join("campaign/events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
async fn wait_phase(base: &Path) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if events(base)
                .iter()
                .any(|event| event["event"] == "phase_start")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "manual full ABBA recipe rehearsal, synthetic checkpoint/runtime, native RSS/hash, injected memory preflight; no MLX"]
async fn native_recipe_rehearsal_abba() {
    let base = base("SEIYA_REHEARSAL_OUTPUT");
    let (paths, recipe, packages) = setup(&base);
    let model = recipe.checkpoint.to_string_lossy().into_owned();
    let handle = launch_campaign(
        driver(
            &base,
            paths,
            recipe.clone(),
            packages,
            RehearsalCondition::Ready,
        )
        .await,
    )
    .unwrap();
    let report = handle.wait().await.unwrap();
    assert!(report.completed, "{report:?}");
    assert!(report.cleanup_confirmed);
    assert_eq!(report.runs_completed, 12);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(base.join("campaign/manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["mode"], "campaign-rehearsal");
    assert_eq!(manifest["real_host_measured"], true);
    assert_eq!(manifest["model_inference"], false);
    let events = events(&base);
    assert_eq!(events.iter().filter(|e| e["event"] == "phase").count(), 84);
    for event in events.iter().filter(|e| e["event"] == "resources") {
        let prep = &event["resources"]["preparation"];
        assert_eq!(prep["lease_held"], true);
        assert_eq!(prep["preflight"]["source"], "injected-rehearsal");
        assert_eq!(
            prep["preflight"]["readiness"]["model_load_authorized"],
            false
        );
        assert_eq!(
            prep["provenance"]["checkpoint"]["verification"],
            "native-held-files-sha256-before-measurement"
        );
    }
    for event in events.iter().filter(|e| e["event"] == "phase") {
        for request in event["requests"].as_array().unwrap() {
            assert_eq!(request["model"], model);
            assert_eq!(request["max_tokens"], 256);
        }
    }
    let reservation = Reservation::acquire(&recipe).unwrap();
    drop(reservation); // Lease actually released after all runs.
    println!(
        "recipe rehearsal completed 12 runs/84 phases, no MLX: {}",
        base.display()
    );
}

#[tokio::test]
#[ignore = "manual blocked preflight recipe regression: hashes synthetic assets, never spawns runtime; no MLX"]
async fn native_recipe_preflight_blocks_before_any_child() {
    let base = base("SEIYA_REHEARSAL_BLOCKED_OUTPUT");
    for (index, condition) in [
        RehearsalCondition::LowMemory,
        RehearsalCondition::CriticalPressure,
        RehearsalCondition::MissingClearance,
        RehearsalCondition::Stale,
    ]
    .into_iter()
    .enumerate()
    {
        let work = base.join(format!("case-{index}"));
        std::fs::create_dir(&work).unwrap();
        let (paths, recipe, packages) = setup(&work);
        let report =
            launch_campaign(driver(&work, paths, recipe.clone(), packages, condition).await)
                .unwrap()
                .wait()
                .await
                .unwrap();
        assert_eq!(report.error, Some(RunError::PreflightBlocked));
        assert_eq!(report.runs_completed, 0);
        assert!(report.cleanup_confirmed);
        let events = events(&work);
        assert_eq!(events.len(), 2);
        assert!(events[1]["resources"]["runtime"].is_null());
        assert!(events[1]["resources"]["coordinator"].is_null());
        assert!(
            !events[1]["resources"]["preparation"]["preflight"]["readiness"]["blockers"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        drop(Reservation::acquire(&recipe).unwrap());
    }
}

#[tokio::test]
#[ignore = "manual recipe cancellation drains owned run and verifies cooperative lease release; no MLX"]
async fn native_recipe_cancel_holds_lease_until_cleanup() {
    let base = base("SEIYA_REHEARSAL_CANCEL_OUTPUT");
    let (paths, recipe, packages) = setup(&base);
    let handle = launch_campaign(
        driver(
            &base,
            paths,
            recipe.clone(),
            packages,
            RehearsalCondition::Ready,
        )
        .await,
    )
    .unwrap();
    wait_phase(&base).await;
    assert!(matches!(
        Reservation::acquire(&recipe),
        Err(seiya_observe::campaign::PreparationError::Busy)
    ));
    handle.cancel();
    let report = handle.wait().await.unwrap();
    assert_eq!(report.error, Some(RunError::Cancelled));
    assert!(report.cleanup_confirmed);
    assert_eq!(
        events(&base)
            .iter()
            .filter(|e| e["event"] == "run_start")
            .count(),
        1
    );
    drop(Reservation::acquire(&recipe).unwrap());
}

#[tokio::test]
#[ignore = "manual recipe provenance change regression prevents next arm startup; synthetic assets only, no MLX"]
async fn native_recipe_provenance_change_aborts_without_second_runtime() {
    let base = base("SEIYA_REHEARSAL_CHANGE_OUTPUT");
    let (paths, recipe, packages) = setup(&base);
    let handle = launch_campaign(
        driver(
            &base,
            paths,
            recipe.clone(),
            packages,
            RehearsalCondition::Ready,
        )
        .await,
    )
    .unwrap();
    wait_phase(&base).await;
    std::fs::write(
        recipe.checkpoint.join("config.json"),
        b"{\"model_type\":\"synthetic\",\"changed\":true}",
    )
    .unwrap();
    let report = handle.wait().await.unwrap();
    assert_eq!(report.error, Some(RunError::ProvenanceChanged));
    assert_eq!(report.runs_completed, 1);
    assert!(report.cleanup_confirmed);
    let events = events(&base);
    let stopped = events
        .iter()
        .filter(|e| e["event"] == "run_stop")
        .collect::<Vec<_>>();
    assert_eq!(stopped.len(), 2);
    assert!(stopped[1]["resources"]["runtime"].is_null());
    drop(Reservation::acquire(&recipe).unwrap());
}

#[tokio::test]
#[ignore = "manual recipe partial-start regression, removes only a new private coordinator binary copy; no MLX"]
async fn native_recipe_partial_start_failure_cleans_runtime_and_lease() {
    let base = base("SEIYA_REHEARSAL_FAILURE_OUTPUT");
    let (mut paths, recipe, packages) = setup(&base);
    let copy = base.join("private-rust-copy");
    std::fs::copy(&paths.rust_server, &copy).unwrap();
    paths.rust_server = copy.clone();
    let prepared = driver(
        &base,
        paths,
        recipe.clone(),
        packages,
        RehearsalCondition::Ready,
    )
    .await;
    std::fs::remove_file(copy).unwrap();
    let report = launch_campaign(prepared).unwrap().wait().await.unwrap();
    assert_eq!(report.error, Some(RunError::Driver));
    assert_eq!(report.runs_completed, 1);
    assert!(report.cleanup_confirmed);
    let events = events(&base);
    let last = events.iter().rfind(|e| e["event"] == "run_stop").unwrap();
    assert_eq!(last["resources"]["cleanup"].as_array().unwrap().len(), 1); // Runtime started, coordinator did not.
    assert_eq!(last["resources"]["cleanup"][0]["confirmed"], true);
    drop(Reservation::acquire(&recipe).unwrap());
}

#[tokio::test]
#[ignore = "manual stop-only pressure watchdog regression on owned synthetic runtime; injects readings, never causes real memory pressure or loads MLX"]
async fn native_watchdog_stops_load_and_request_admission_with_owned_cleanup() {
    let base = base("SEIYA_REHEARSAL_MONITOR_OUTPUT");
    for (name, condition) in [
        ("load", RehearsalCondition::PressureDuringLoad),
        ("request", RehearsalCondition::PressureDuringRequest),
    ] {
        let work = base.join(name);
        std::fs::create_dir(&work).unwrap();
        let (paths, recipe, packages) = setup(&work);
        let report =
            launch_campaign(driver(&work, paths, recipe.clone(), packages, condition).await)
                .unwrap()
                .wait()
                .await
                .unwrap();
        assert_eq!(report.error, Some(RunError::LoadMonitorStopped));
        assert_eq!(report.runs_completed, 0);
        assert!(report.cleanup_confirmed);
        let events = events(&work);
        assert_eq!(
            events.iter().filter(|e| e["event"] == "run_start").count(),
            1
        );
        let stopped = events.iter().find(|e| e["event"] == "run_stop").unwrap();
        assert_eq!(
            stopped["resources"]["preparation"]["load_monitor"]["stop"],
            "pressure_not_normal"
        );
        assert_eq!(
            stopped["resources"]["preparation"]["monitor_source"],
            "injected-rehearsal"
        );
        assert!(!stopped["resources"]["cleanup"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(stopped["resources"]["cleanup"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["confirmed"] == true));
        assert!(events
            .iter()
            .filter(|e| e["event"] == "phase")
            .all(|e| e["requests"].as_array().unwrap().is_empty()));
        drop(Reservation::acquire(&recipe).unwrap());
    }
}

#[tokio::test]
#[ignore = "manual calibration policy rehearsal: one owned synthetic runtime, low-free/pressure injected, no MLX or real load approval"]
async fn native_calibration_exception_is_single_use_and_separate_from_campaign() {
    let base = base("SEIYA_CALIBRATION_REHEARSAL_OUTPUT");
    for calibration in [false, true] {
        let work = base.join(if calibration {
            "wrong-normal-launch"
        } else {
            "wrong-calibration-launch"
        });
        std::fs::create_dir(&work).unwrap();
        let (paths, recipe, packages) = setup(&work);
        let prepared = driver(&work, paths, recipe, packages, RehearsalCondition::Ready).await;
        let result = if calibration {
            launch_campaign(
                prepared
                    .into_calibration_rehearsal(CalibrationPolicy {
                        runtime_seconds: 30,
                    })
                    .unwrap(),
            )
        } else {
            launch_calibration(prepared)
        };
        assert!(matches!(result, Err(RunError::InvalidEvidence)));
        assert!(!work.join("campaign").exists());
    }
    for (name, condition, calibrate, expected) in [
        (
            "normal-low-free",
            RehearsalCondition::LowMemory,
            false,
            Some(RunError::PreflightBlocked),
        ),
        (
            "calibration-low-free",
            RehearsalCondition::LowMemory,
            true,
            None,
        ),
        (
            "calibration-critical",
            RehearsalCondition::CriticalPressure,
            true,
            Some(RunError::PreflightBlocked),
        ),
        (
            "calibration-watchdog",
            RehearsalCondition::PressureDuringRequest,
            true,
            Some(RunError::LoadMonitorStopped),
        ),
    ] {
        let work = base.join(name);
        std::fs::create_dir(&work).unwrap();
        let (paths, recipe, packages) = setup(&work);
        let prepared = driver(&work, paths, recipe.clone(), packages, condition).await;
        let handle = if calibrate {
            launch_calibration(
                prepared
                    .into_calibration_rehearsal(CalibrationPolicy {
                        runtime_seconds: 30,
                    })
                    .unwrap(),
            )
        } else {
            launch_campaign(prepared)
        }
        .unwrap();
        let report = handle.wait().await.unwrap();
        assert_eq!(report.error, expected, "{name}: {report:?}");
        assert!(report.cleanup_confirmed);
        assert_eq!(report.completed, expected.is_none());
        assert_eq!(report.runs_completed, usize::from(expected.is_none()));
        let events = events(&work);
        assert_eq!(
            events.iter().filter(|e| e["event"] == "run_start").count(),
            1
        );
        if expected.is_none() {
            let manifest: Value = serde_json::from_slice(
                &std::fs::read(work.join("campaign/manifest.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest["mode"], "calibration-rehearsal");
            assert_eq!(manifest["model_inference"], false);
            assert_eq!(manifest["execution"]["runs"], 1);
            assert_eq!(events.iter().filter(|e| e["event"] == "phase").count(), 3);
            let resources = events.iter().find(|e| e["event"] == "resources").unwrap();
            assert_eq!(
                resources["resources"]["preparation"]["preflight"]["readiness"]
                    ["capacity_guaranteed"],
                false
            );
        }
        drop(Reservation::acquire(&recipe).unwrap());
    }
}
