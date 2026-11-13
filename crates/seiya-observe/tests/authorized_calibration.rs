//! Operator-invoked real model calibration. Ignored; never run by make test.
#![cfg(target_os = "macos")]
use seiya_observe::campaign::{
    CalibrationApproval, CalibrationPolicy, CampaignDriver, CoordinatorPaths, MlxRecipe,
    NativeProvenance, PackageMetadata,
};
use seiya_observe::runner::launch_calibration;
use std::path::PathBuf;
use std::time::Duration;

fn path(key: &str) -> PathBuf {
    std::env::var_os(key)
        .expect("explicit operator-approved path required")
        .into()
}

#[tokio::test]
#[ignore = "explicit approved candidate-file hashes/version metadata only; never imports MLX, allocates tensors or starts inference"]
async fn authorized_candidate_provenance_without_model_load() {
    assert_eq!(
        std::env::var("SEIYA_ALLOW_CANDIDATE_PROVENANCE").as_deref(),
        Ok("1")
    );
    let output = path("SEIYA_PROVENANCE_OUTPUT");
    assert!(output.is_absolute() && !output.exists());
    let recipe = MlxRecipe {
        python: path("SEIYA_CAMPAIGN_PYTHON"),
        runtime_entrypoint: path("SEIYA_CAMPAIGN_ENTRYPOINT"),
        checkpoint: path("SEIYA_CAMPAIGN_CHECKPOINT"),
        coordination_directory: path("SEIYA_CAMPAIGN_COORDINATION"),
        minimum_free_bytes: 2 << 30,
    };
    let packages = PackageMetadata {
        mlx: path("SEIYA_MLX_METADATA"),
        mlx_lm: path("SEIYA_MLX_LM_METADATA"),
        transformers: path("SEIYA_TRANSFORMERS_METADATA"),
    };
    let mut builder = std::fs::DirBuilder::new();
    use std::os::unix::fs::DirBuilderExt;
    builder.mode(0o700).create(&output).unwrap();
    let provenance = NativeProvenance::inspect(&recipe, &packages).await;
    let document = match &provenance {
        Ok(provenance) => {
            serde_json::json!({"mode":"native-candidate-provenance-only","model_loaded":false,"mlx_imported":false,"provenance":provenance})
        }
        Err(error) => {
            serde_json::json!({"mode":"native-candidate-provenance-only","model_loaded":false,"error":error})
        }
    };
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("provenance.json"))
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(&document).unwrap())
        .unwrap();
    assert!(
        provenance.is_ok(),
        "provenance failed; evidence preserved: {provenance:?}"
    );
    println!(
        "native provenance verified without loading model; artifacts {}",
        output.display()
    );
}

#[tokio::test]
#[ignore = "REAL MODEL LOAD: requires fresh explicit calibration authorization, external-runtime clearance and all operator paths; not authorized by preparing policy"]
async fn authorized_single_model_calibration() {
    // Check confirmations before version probes, file reads, hashing or startup.
    assert_eq!(
        std::env::var("SEIYA_ALLOW_MODEL_CALIBRATION").as_deref(),
        Ok("I_AUTHORIZE_ONE_EXPERIMENTAL_LOAD")
    );
    assert_eq!(
        std::env::var("SEIYA_EXTERNAL_RUNTIME_CLEARED").as_deref(),
        Ok("1")
    );
    let memory = match std::env::var("SEIYA_CALIBRATION_RUNTIME_MEMORY").as_deref() {
        Ok("1") => true,
        Err(std::env::VarError::NotPresent) => false,
        _ => panic!("runtime memory opt-in must be absent or exactly 1"),
    };
    let seconds = std::env::var("SEIYA_CALIBRATION_RUNTIME_SECONDS")
        .expect("explicit runtime limit required")
        .parse::<u32>()
        .unwrap();
    let policy = CalibrationPolicy {
        runtime_seconds: seconds,
    };
    policy.validate().unwrap();
    let recipe = MlxRecipe {
        python: path("SEIYA_CAMPAIGN_PYTHON"),
        runtime_entrypoint: path("SEIYA_CAMPAIGN_ENTRYPOINT"),
        checkpoint: path("SEIYA_CAMPAIGN_CHECKPOINT"),
        coordination_directory: path("SEIYA_CAMPAIGN_COORDINATION"),
        minimum_free_bytes: 2 << 30,
    };
    let output = path("SEIYA_CALIBRATION_OUTPUT");
    assert!(output.is_absolute() && !output.exists());
    let paths = CoordinatorPaths {
        python: path("SEIYA_COORDINATOR_PYTHON"),
        python_source: path("SEIYA_COORDINATOR_SOURCE"),
        rust_server: path("SEIYA_COORDINATOR_RUST"),
    };
    let packages = PackageMetadata {
        mlx: path("SEIYA_MLX_METADATA"),
        mlx_lm: path("SEIYA_MLX_LM_METADATA"),
        transformers: path("SEIYA_TRANSFORMERS_METADATA"),
    };
    // This constructor may only be reached after real current user approval and
    // external clearance, not because a model supplied the confirmation string.
    let approval =
        CalibrationApproval::confirm(&recipe, &policy, Duration::from_secs(900)).unwrap();
    let driver = CampaignDriver::prepare_calibration_authorized(
        paths,
        output.clone(),
        recipe,
        packages,
        approval,
    )
    .await
    .unwrap();
    let driver = if memory {
        driver.with_runtime_memory().unwrap()
    } else {
        driver
    };
    let report = launch_calibration(driver).unwrap().wait().await.unwrap();
    println!(
        "calibration report: {report:?}; preserved artifacts {}",
        output.display()
    );
    assert!(
        report.cleanup_confirmed,
        "owned-resource cleanup not confirmed: {report:?}"
    );
    assert!(
        report.completed,
        "calibration incomplete; inspect preserved artifacts: {report:?}"
    );
    assert_eq!(report.mode, "model-calibration");
    assert_eq!(report.runs_completed, 1);
}
