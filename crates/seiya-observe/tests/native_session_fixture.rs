//! Explicit native smoke: newly compiled audited helper, private locks and pipes.
//! No MLX/model, host pressure probe, user processes, shell, or operational ports.
#![cfg(target_os = "macos")]
use seiya_observe::{
    campaign::{MlxRecipe, Pressure, Reservation},
    session_guard::MemoryGuard,
    session_native_fixture::{self, Behavior, FailureKind},
    session_supervisor::{Policy, Stage, StopCause},
};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn recipe(directory: PathBuf) -> MlxRecipe {
    MlxRecipe {
        python: "/synthetic/python".into(),
        runtime_entrypoint: "/synthetic/mlx-server".into(),
        checkpoint: "/synthetic/checkpoint".into(),
        coordination_directory: directory,
        minimum_free_bytes: 2 << 30,
    }
}
fn directory(parent: &Path, name: &str) -> PathBuf {
    use std::os::unix::fs::DirBuilderExt;
    let path = parent.join(name);
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700).create(&path).unwrap();
    path.canonicalize().unwrap()
}
fn can_reacquire(recipe: &MlxRecipe) {
    assert!(recipe
        .coordination_directory
        .join("seiya-mlx-baseline.lock")
        .is_file());
    let next = Reservation::acquire(recipe).unwrap();
    drop(next);
}

#[tokio::test]
#[ignore = "explicit native Child/strong identity/EOF/SIGTERM/reaping and kernel lease fixture; no model, host-memory probe or operational service"]
async fn record_owned_fixture_child_and_lease_without_models() {
    let output = PathBuf::from(
        std::env::var_os("SEIYA_NATIVE_FIXTURE_OUTPUT").expect("new absolute output required"),
    );
    assert!(output.is_absolute() && !output.exists());
    let parent = output.parent().unwrap().canonicalize().unwrap();
    let output = directory(&parent, output.file_name().unwrap().to_str().unwrap());
    let executable = Path::new(env!("CARGO_BIN_EXE_seiya-observe-fixture"))
        .canonicalize()
        .unwrap();
    let policy = Policy::new(
        Duration::from_secs(5),
        Duration::from_millis(500),
        Duration::from_secs(2),
    )
    .unwrap();
    let mut cases = vec![];
    for (name, behavior, signals) in [
        ("eof", Behavior::Cooperative, 0),
        ("term", Behavior::IgnoreEof, 1),
    ] {
        let recipe = recipe(directory(&output, name));
        let guard = MemoryGuard::new();
        let lease = Reservation::acquire(&recipe).unwrap().into_session_lease();
        let started = session_native_fixture::start(
            &executable,
            lease,
            guard.clone(),
            policy,
            Pressure::Warning,
            behavior,
        )
        .await
        .unwrap();
        let mut supervisor = started.supervisor;
        let status = supervisor.wait_terminal().await.unwrap();
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/benchmarks/chat-session/v0.1.0/native-child-status.json"
        ))
        .unwrap();
        assert_eq!(serde_json::to_value(&status).unwrap(), fixture["stopped"]);
        assert_eq!(status.version, 3);
        assert_eq!(status.stage, Stage::Stopped);
        assert_eq!(status.mode, "native-fixture-supervisor");
        assert_eq!(status.model_inference, Some(false));
        assert_eq!(status.runtime_origin, Some("native_fixture_child"));
        assert_eq!(status.pressure_origin, Some("injected_fixture"));
        assert!(status.runtime_reaped && status.lease_release_confirmed);
        let evidence = started.evidence.snapshot();
        assert!(evidence.eof_closed && evidence.reaped && !evidence.untracked_descendants);
        assert_eq!(evidence.term_signals, signals);
        assert!(evidence.identity.pid() > 0);
        assert!(guard.check().is_err());
        can_reacquire(&recipe);
        cases.push(json!({"case":name,"status":status,"evidence":evidence}));
    }
    let recipe = recipe(directory(&output, "bad-ready"));
    let lease = Reservation::acquire(&recipe).unwrap().into_session_lease();
    let failure = match session_native_fixture::start(
        &executable,
        lease,
        MemoryGuard::new(),
        policy,
        Pressure::Normal,
        Behavior::BadHandshake,
    )
    .await
    {
        Ok(_) => panic!("invalid handshake accepted"),
        Err(error) => error,
    };
    assert_eq!(failure.kind, FailureKind::Handshake);
    assert!(failure.child_cleanup_confirmed && failure.lease_release_confirmed);
    can_reacquire(&recipe);
    let idle_recipe = crate::recipe(directory(&output, "idle-deadline"));
    let idle_policy = Policy::new(
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_secs(2),
    )
    .unwrap();
    let started = session_native_fixture::start(
        &executable,
        Reservation::acquire(&idle_recipe)
            .unwrap()
            .into_session_lease(),
        MemoryGuard::new(),
        idle_policy,
        Pressure::Normal,
        Behavior::Cooperative,
    )
    .await
    .unwrap();
    let mut idle = started.supervisor;
    let idle_status = idle.wait_terminal().await.unwrap();
    assert_eq!(idle_status.cause, Some(StopCause::Deadline));
    assert_eq!(idle_status.stage, Stage::Stopped);
    assert!(started.evidence.snapshot().reaped);
    can_reacquire(&idle_recipe);
    cases.push(
        json!({"case":"idle-deadline","status":idle_status,"evidence":started.evidence.snapshot()}),
    );
    let mut rejected_executables = vec![];
    for (name, path) in [
        ("missing", output.join("missing-program")),
        ("symlink", output.join("seiya-observe-fixture")),
    ] {
        if name == "symlink" {
            std::os::unix::fs::symlink(&executable, &path).unwrap();
        }
        let recipe = crate::recipe(directory(&output, name));
        let error = match session_native_fixture::start(
            &path,
            Reservation::acquire(&recipe).unwrap().into_session_lease(),
            MemoryGuard::new(),
            policy,
            Pressure::Normal,
            Behavior::Cooperative,
        )
        .await
        {
            Ok(_) => panic!("invalid executable accepted"),
            Err(error) => error,
        };
        assert_eq!(error.kind, FailureKind::InvalidExecutable);
        assert!(error.child_cleanup_confirmed && error.lease_release_confirmed);
        can_reacquire(&recipe);
        rejected_executables
            .push(json!({"case":name,"rejected_before_spawn":true,"lease_release_confirmed":true}));
    }
    let document = json!({"mode":"native-fixture-supervisor-smoke","model_inference":false,
        "host_pressure_probed":false,"pressure_source":"injected_fixture","cases":cases,"rejected_executables":rejected_executables,
        "invalid_handshake":{"rejected":true,"child_cleanup_confirmed":failure.child_cleanup_confirmed,
            "lease_release_confirmed":failure.lease_release_confirmed},
        "limitations":"Audited non-forking helper only, not arbitrary descendant containment, MLX shutdown or sustained chat evidence"});
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output.join("report.json"))
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(&document).unwrap())
        .unwrap();
    file.sync_all().unwrap();
    println!(
        "native helper identity/reaping and lease OK; pressure injected, no model; artifacts {}",
        output.display()
    );
}

#[tokio::test]
#[ignore = "explicit read-only native host pressure and audited helper only; never induces pressure, loads MLX or touches an operational process"]
async fn native_pressure_stops_only_owned_leaf_or_reaches_deadline() {
    let output = PathBuf::from(
        std::env::var_os("SEIYA_NATIVE_PRESSURE_OUTPUT").expect("new absolute output required"),
    );
    assert!(output.is_absolute() && !output.exists());
    let parent = output.parent().unwrap().canonicalize().unwrap();
    let output = directory(&parent, output.file_name().unwrap().to_str().unwrap());
    let recipe = recipe(output.clone());
    let guard = MemoryGuard::new();
    let reading = seiya_observe::session_pressure::sample(guard.now_millis().unwrap()).unwrap();
    let policy = Policy::new(
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_secs(2),
    )
    .unwrap();
    let executable = Path::new(env!("CARGO_BIN_EXE_seiya-observe-fixture"))
        .canonicalize()
        .unwrap();
    let started = session_native_fixture::start_with_native_pressure(
        &executable,
        Reservation::acquire(&recipe).unwrap().into_session_lease(),
        guard.clone(),
        policy,
        Behavior::Cooperative,
    )
    .await
    .unwrap();
    let mut supervisor = started.supervisor;
    let status = supervisor.wait_terminal().await.unwrap();
    assert_eq!(status.version, 4);
    assert_eq!(status.pressure_origin, Some("native_macos_sysctl"));
    assert_eq!(status.runtime_origin, Some("native_fixture_child"));
    assert_eq!(status.model_inference, Some(false));
    assert_eq!(status.stage, Stage::Stopped);
    assert!(status.runtime_reaped && status.lease_release_confirmed);
    if status.cause
        == Some(StopCause::Memory(
            seiya_observe::campaign::MonitorStop::PressureNotNormal,
        ))
    {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/benchmarks/chat-session/v0.1.0/native-pressure-status.json"
        ))
        .unwrap();
        assert_eq!(serde_json::to_value(&status).unwrap(), fixture["stopped"]);
    }
    let evidence = started.evidence.snapshot();
    assert!(evidence.reaped && !evidence.untracked_descendants);
    assert_eq!(evidence.term_signals, 0);
    can_reacquire(&recipe);
    let document = json!({"mode":"native-pressure-leaf-smoke", "model_inference":false,
        "status":status, "evidence":evidence, "guard":guard.status(), "initial_pressure":reading.pressure,
        "limitations":"Read-only host sample, no induced pressure or MLX cleanup/capacity/sustained inference validation"});
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output.join("report.json"))
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(&document).unwrap())
        .unwrap();
    file.sync_all().unwrap();
}
