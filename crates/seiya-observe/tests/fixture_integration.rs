//! Explicit owned-process integration; ignored in normal tests. No model loading.
#![cfg(target_os = "macos")]
use seiya_observe::fixture_driver::{FixtureDriver, Paths};
use seiya_observe::runner::launch_fixture_integration;
use serde_json::Value;
use std::path::PathBuf;

fn paths() -> Paths {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    Paths {
        python: std::env::var_os("SEIYA_FIXTURE_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/opt/homebrew/bin/python3".into()),
        python_source: root.join("swarm-app"),
        rust_server: root.join("target/debug/seiya-server"),
        fixture_runtime: PathBuf::from(env!("CARGO_BIN_EXE_seiya-observe-fixture")),
    }
}
fn output(variable: &str) -> PathBuf {
    let path =
        PathBuf::from(std::env::var_os(variable).expect("explicit new output path is required"));
    assert!(path.is_absolute() && !path.exists());
    path
}

#[tokio::test]
#[ignore = "manual macOS integration: 12 owned Python/Rust runs with synthetic runtime, native probes and real idle waits; no models"]
async fn native_fixture_campaign() {
    let directory = output("SEIYA_FIXTURE_OUTPUT");
    let driver = FixtureDriver::prepare(paths(), directory.clone())
        .await
        .unwrap();
    let report = launch_fixture_integration(driver)
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(report.completed, "{:?}", report);
    assert!(report.cleanup_confirmed);
    assert_eq!(report.runs_completed, 12);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(directory.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["mode"], "fixture-integration");
    assert_eq!(manifest["real_host_measured"], true);
    assert_eq!(manifest["model_inference"], false);
    for asset in manifest["assets"]["assets"].as_array().unwrap() {
        assert_eq!(asset["sha256"].as_str().unwrap().len(), 64);
    }
    let events = std::fs::read_to_string(directory.join("events.jsonl")).unwrap();
    let events: Vec<Value> = events
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
    for event in events.iter().filter(|event| event["event"] == "phase") {
        assert!(!event["diagnostics"]["sampling"]
            .as_array()
            .unwrap()
            .is_empty());
        if event["phase"] == "listener-ready" || event["phase"] == "shutdown" {
            assert_eq!(event["diagnostics"]["system"]["scope"], "whole-host");
        }
    }
    println!(
        "owned fixture campaign: 12 runs completed, no model inference; artifacts {}",
        directory.display()
    );
}

#[tokio::test]
#[ignore = "manual owned fixture cancellation after listener startup; no models"]
async fn native_cancel_drains_and_reaps_without_starting_other_arm() {
    let directory = output("SEIYA_FIXTURE_CANCEL_OUTPUT");
    let driver = FixtureDriver::prepare(paths(), directory.clone())
        .await
        .unwrap();
    let handle = launch_fixture_integration(driver).unwrap();
    let events_path = directory.join("events.jsonl");
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let events = std::fs::read_to_string(&events_path).unwrap();
            if events
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .any(|event| event["event"] == "phase_start")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    handle.cancel();
    let report = handle.wait().await.unwrap();
    assert!(!report.completed);
    assert_eq!(report.runs_completed, 0);
    assert_eq!(
        report.error,
        Some(seiya_observe::runner::RunError::Cancelled)
    );
    assert!(report.cleanup_confirmed);
    let events = std::fs::read_to_string(events_path).unwrap();
    assert_eq!(
        events
            .lines()
            .filter(|line| serde_json::from_str::<Value>(line).unwrap()["event"] == "run_start")
            .count(),
        1
    );
}

#[tokio::test]
#[ignore = "manual native partial-start cleanup, copies and removes only a new private test binary copy; no models"]
async fn native_partial_start_failure_reaps_owned_runtime_without_other_arm() {
    let base = output("SEIYA_FIXTURE_FAILURE_OUTPUT");
    std::fs::create_dir(&base).unwrap();
    let mut paths = paths();
    let copy = base.join("rust-server-copy");
    std::fs::copy(&paths.rust_server, &copy).unwrap();
    paths.rust_server = copy.clone();
    let directory = base.join("campaign");
    let driver = FixtureDriver::prepare(paths, directory.clone())
        .await
        .unwrap();
    std::fs::remove_file(copy).unwrap(); // Own new test copy, never the real binary.
    let report = launch_fixture_integration(driver)
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(!report.completed);
    assert_eq!(report.runs_completed, 1);
    assert!(report.cleanup_confirmed);
    let events: Vec<Value> = std::fs::read_to_string(directory.join("events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event"] == "run_start")
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event"] == "run_stop")
            .count(),
        2
    );
    println!(
        "partial start: one valid run, failed second arm, own runtime cleaned; artifacts {}",
        directory.display()
    );
}
