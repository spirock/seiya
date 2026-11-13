//! Kernel/process fixtures only; never MLX, user processes or operational locks.
#![cfg(target_os = "macos")]
use seiya_observe::{
    campaign::{LoadBudget, MlxRecipe, ModelLoadApproval, PackageMetadata, Pressure, Reservation},
    session_guard::MemoryGuard,
    session_runtime::{self, Error, MlxLaunch},
    session_supervisor::{Policy, Stage, StopCause},
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn directory() -> PathBuf {
    use std::os::unix::fs::DirBuilderExt;
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/restricted-runtime-tests");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join(uuid::Uuid::new_v4().to_string());
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .unwrap();
    path.canonicalize().unwrap()
}
fn recipe() -> MlxRecipe {
    MlxRecipe {
        python: "/synthetic/python".into(),
        runtime_entrypoint: "/synthetic/mlx.py".into(),
        checkpoint: "/synthetic/checkpoint".into(),
        coordination_directory: directory(),
        minimum_free_bytes: 2 << 30,
    }
}
fn policy() -> Policy {
    Policy::new(
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_secs(2),
    )
    .unwrap()
}
fn bootstrap() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_seiya-session-bootstrap"))
        .canonicalize()
        .unwrap()
}
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_seiya-observe-fixture"))
        .canonicalize()
        .unwrap()
}

#[tokio::test]
async fn pressure_before_spawn_closes_guard_and_releases_unused_lease() {
    for pressure in [Pressure::Warning, Pressure::Critical, Pressure::Unknown] {
        let recipe = recipe();
        let guard = MemoryGuard::new();
        let error = match session_runtime::start_rehearsal(
            Reservation::acquire(&recipe).unwrap(),
            &bootstrap(),
            &fixture(),
            guard.clone(),
            policy(),
            Some(pressure),
        )
        .await
        {
            Ok(_) => panic!("pressure admitted"),
            Err(e) => e,
        };
        assert_eq!(error.cause, Error::Pressure);
        assert!(error.cleanup_confirmed && error.lease_released);
        assert!(guard.check().is_err());
        drop(Reservation::acquire(&recipe).unwrap());
    }
}

#[tokio::test]
async fn invalid_fixture_or_bootstrap_never_starts_and_leaves_no_lease() {
    for invalid_fixture in [true, false] {
        let recipe = recipe();
        let guard = MemoryGuard::new();
        let missing = recipe.coordination_directory.join("missing");
        let bootstrap = bootstrap();
        let fixture = fixture();
        let error = match session_runtime::start_rehearsal(
            Reservation::acquire(&recipe).unwrap(),
            if invalid_fixture {
                &bootstrap
            } else {
                &missing
            },
            if invalid_fixture { &missing } else { &fixture },
            guard.clone(),
            policy(),
            Some(Pressure::Normal),
        )
        .await
        {
            Ok(_) => panic!("missing executable accepted"),
            Err(e) => e,
        };
        assert_eq!(error.cause, Error::Executable);
        assert!(error.cleanup_confirmed && error.lease_released);
        assert!(guard.check().is_err());
        drop(Reservation::acquire(&recipe).unwrap());
    }
}

#[tokio::test]
async fn authorization_and_incomplete_budget_block_before_any_model_files_or_start() {
    let recipe = recipe();
    let other = MlxRecipe {
        checkpoint: "/synthetic/other".into(),
        ..recipe.clone()
    };
    let packages = PackageMetadata {
        mlx: "/synthetic/mlx".into(),
        mlx_lm: "/synthetic/mlx-lm".into(),
        transformers: "/synthetic/transformers".into(),
    };
    let budget = LoadBudget {
        manifest_sha256: "a".repeat(64),
        context_tokens: 4096,
        output_tokens: 256,
        resident_weights: None,
        active_kv: None,
        transient_activations: None,
        disposable_caches: None,
        runtime_overhead: None,
        coordinator: None,
        host_margin: None,
    };
    // Synthetic approvals only: neither recipe points to real runtime/model files.
    for (approved, expected) in [(&other, Error::Authorization), (&recipe, Error::Budget)] {
        let approval = ModelLoadApproval::confirm(approved, Duration::from_secs(1)).unwrap();
        let guard = MemoryGuard::new();
        let error = match session_runtime::start_mlx(
            MlxLaunch {
                recipe: &recipe,
                packages: &packages,
                approval: &approval,
                budget: &budget,
                bootstrap: &bootstrap(),
            },
            guard.clone(),
            policy(),
        )
        .await
        {
            Ok(_) => panic!("unreviewed model admitted"),
            Err(e) => e,
        };
        assert_eq!(error.cause, expected);
        assert!(guard.check().is_err());
        assert!(!recipe
            .coordination_directory
            .join("seiya-mlx-baseline.lock")
            .exists());
    }
}

#[tokio::test]
async fn restricted_model_route_never_lowers_the_two_gib_normal_gate() {
    let mut recipe = recipe();
    recipe.minimum_free_bytes = 1 << 30;
    let packages = PackageMetadata {
        mlx: "/synthetic/mlx".into(),
        mlx_lm: "/synthetic/mlx-lm".into(),
        transformers: "/synthetic/transformers".into(),
    };
    let budget = LoadBudget {
        manifest_sha256: "a".repeat(64),
        context_tokens: 4096,
        output_tokens: 256,
        resident_weights: None,
        active_kv: None,
        transient_activations: None,
        disposable_caches: None,
        runtime_overhead: None,
        coordinator: None,
        host_margin: None,
    };
    let approval = ModelLoadApproval::confirm(&recipe, Duration::from_secs(1)).unwrap();
    let guard = MemoryGuard::new();
    let error = match session_runtime::start_mlx(
        MlxLaunch {
            recipe: &recipe,
            packages: &packages,
            approval: &approval,
            budget: &budget,
            bootstrap: &bootstrap(),
        },
        guard.clone(),
        policy(),
    )
    .await
    {
        Ok(_) => panic!("lowered gate accepted"),
        Err(error) => error,
    };
    assert_eq!(error.cause, Error::Preparation);
    assert!(guard.check().is_err());
    assert!(!recipe
        .coordination_directory
        .join("seiya-mlx-baseline.lock")
        .exists());
}

#[tokio::test]
#[ignore = "explicit sandbox fork/spawn/exec + owned HTTP fixture, native socket ownership and kernel lease; no model"]
async fn restricted_native_runtime_reaps_before_unlocking_on_pressure_cancel_and_idle() {
    use std::process::{Command, Stdio};
    let bootstrap = bootstrap();
    let plain = Command::new(&bootstrap)
        .arg("--probe-only")
        .env_clear()
        .stdout(Stdio::piped())
        .output()
        .unwrap();
    assert!(!plain.status.success());
    assert!(plain.stdout.is_empty());
    let inherited = Command::new("/usr/bin/sandbox-exec")
        .arg("-p")
        .arg(seiya_observe::session_containment::PROFILE)
        .arg(&bootstrap)
        .args(["--exec", bootstrap.to_str().unwrap(), "--probe-only"])
        .env_clear()
        .stdout(Stdio::piped())
        .output()
        .unwrap();
    assert!(inherited.status.success());
    // Exec target has stdout redirected to null; exit0 proves its second probe
    // also observed EPERM. Only the first bootstrap marker belongs to our pipe.
    assert_eq!(inherited.stdout, seiya_observe::session_containment::READY);
    let mut reports = vec![];
    for case in ["pressure", "cancel", "idle", "host-deadline"] {
        let recipe = recipe();
        let guard = MemoryGuard::new();
        let started = session_runtime::start_rehearsal(
            Reservation::acquire(&recipe).unwrap(),
            &bootstrap,
            &fixture(),
            guard.clone(),
            policy(),
            Some(Pressure::Normal),
        )
        .await
        .unwrap();
        assert!(Reservation::acquire(&recipe).is_err());
        let mut supervisor = started.supervisor;
        if case == "pressure" {
            let _ = guard.observe(Ok(seiya_observe::campaign::PressureSample {
                sampled_at_millis: guard.now_millis().unwrap(),
                pressure: Pressure::Warning,
            }));
        } else if case == "cancel" {
            supervisor.request_stop();
        } else if case == "host-deadline" {
            guard
                .stop_at_millis(guard.now_millis().unwrap() + 50)
                .unwrap();
        }
        let status = supervisor.wait_terminal().await.unwrap();
        assert_eq!(status.stage, Stage::Stopped);
        assert_eq!(status.version, 5);
        assert_eq!(status.mode, "native-restricted-supervisor");
        assert_eq!(status.containment, Some("macos_process_fork_denied_v1"));
        assert_eq!(
            status.runtime_origin,
            Some("native_restricted_fixture_child")
        );
        assert_eq!(status.pressure_origin, Some("injected_fixture"));
        assert_eq!(status.model_inference, Some(false));
        assert!(status.runtime_reaped && status.lease_release_confirmed);
        if case == "idle" {
            assert_eq!(status.cause, Some(StopCause::Deadline));
        }
        if case == "pressure" {
            let fixture: serde_json::Value = serde_json::from_str(include_str!(
                "../../../contracts/benchmarks/chat-session/v0.1.0/restricted-runtime-status.json"
            ))
            .unwrap();
            assert_eq!(
                serde_json::to_value(&status).unwrap(),
                fixture["fixture_pressure_stopped"]
            );
        }
        if case == "cancel" {
            assert_eq!(status.cause, Some(StopCause::Cancelled));
        }
        drop(Reservation::acquire(&recipe).unwrap());
        reports.push(serde_json::json!({"case":case,"identity":started.identity,"port":started.port,"status":status}));
    }
    let quarantine_recipe = recipe();
    let guard = MemoryGuard::new();
    let short_cleanup = Policy::new(
        Duration::from_secs(5),
        Duration::from_millis(500),
        Duration::from_millis(100),
    )
    .unwrap();
    let started = session_runtime::start_rehearsal_with_behavior(
        Reservation::acquire(&quarantine_recipe).unwrap(),
        &bootstrap,
        &fixture(),
        guard.clone(),
        short_cleanup,
        session_runtime::RehearsalOptions {
            pressure: Some(Pressure::Normal),
            behavior: session_runtime::RehearsalBehavior::IgnoreTerm,
        },
    )
    .await
    .unwrap();
    let mut supervisor = started.supervisor;
    supervisor.request_stop();
    let quarantined = supervisor.wait_terminal().await.unwrap();
    assert_eq!(quarantined.stage, Stage::Quarantined);
    assert!(!quarantined.lease_release_confirmed);
    assert!(Reservation::acquire(&quarantine_recipe).is_err());
    // Only the audited helper's own three-second self-expiry, no foreign kill.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let recovered = supervisor.retry_cleanup().await.unwrap();
    assert_eq!(recovered.stage, Stage::Stopped);
    assert_eq!(recovered.attempts, 2);
    assert!(recovered.runtime_reaped && recovered.lease_release_confirmed);
    drop(Reservation::acquire(&quarantine_recipe).unwrap());
    reports.push(serde_json::json!({"case":"ignored-term-quarantine","quarantined":quarantined,"recovered":recovered}));

    let foreign_recipe = recipe();
    let reservation = Reservation::acquire(&foreign_recipe).unwrap();
    let port = reservation.ports().unwrap().0;
    let owned_bootstrap = bootstrap.clone();
    let owned_fixture = fixture();
    let task = tokio::spawn(async move {
        session_runtime::start_rehearsal_with_behavior(
            reservation,
            &owned_bootstrap,
            &owned_fixture,
            MemoryGuard::new(),
            policy(),
            session_runtime::RehearsalOptions {
                pressure: Some(Pressure::Normal),
                behavior: session_runtime::RehearsalBehavior::NoListener,
            },
        )
        .await
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let foreign_listener = loop {
        if let Ok(listener) = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)) {
            break listener;
        }
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let failure = match task.await.unwrap() {
        Ok(_) => panic!("foreign listener accepted as own"),
        Err(e) => e,
    };
    assert_eq!(failure.cause, Error::StartupTimeout);
    assert!(failure.cleanup_confirmed && failure.lease_released);
    assert!(foreign_listener.local_addr().is_ok());
    drop(foreign_listener);
    drop(Reservation::acquire(&foreign_recipe).unwrap());
    reports.push(serde_json::json!({"case":"foreign-listener-rejected","root_cleanup":failure.cleanup_confirmed,"lease_released":failure.lease_released}));

    let native_recipe = recipe();
    match session_runtime::start_rehearsal(
        Reservation::acquire(&native_recipe).unwrap(),
        &bootstrap,
        &fixture(),
        MemoryGuard::new(),
        policy(),
        None,
    )
    .await
    {
        Ok(started) => {
            let mut supervisor = started.supervisor;
            let status = supervisor.wait_terminal().await.unwrap();
            assert_eq!(status.stage, Stage::Stopped);
            assert_eq!(status.pressure_origin, Some("native_macos_sysctl"));
            assert!(status.runtime_reaped && status.lease_release_confirmed);
            reports.push(serde_json::json!({"case":"native-pressure","status":status}));
        }
        Err(error) => {
            assert_eq!(error.cause, Error::Pressure);
            assert!(error.cleanup_confirmed && error.lease_released);
            reports.push(serde_json::json!({"case":"native-pressure","blocked":true,"reason":"pressure_guard"}));
        }
    }
    drop(Reservation::acquire(&native_recipe).unwrap());
    let output = PathBuf::from(
        std::env::var_os("SEIYA_RESTRICTED_OUTPUT").expect("new absolute report path"),
    );
    assert!(output.is_absolute() && !output.exists());
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&output)
        .unwrap();
    file.write_all(
        &serde_json::to_vec_pretty(&serde_json::json!({"mode":"restricted-native-fixture",
        "model_inference":false,"fork_spawn_denied_and_exec_inheritance":true,"cases":reports}))
        .unwrap(),
    )
    .unwrap();
    file.sync_all().unwrap();
}
