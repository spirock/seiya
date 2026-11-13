//! Real ABBA harness. Never run without the current concrete operator approval.
#![cfg(target_os = "macos")]
use seiya_observe::{
    campaign::{CampaignDriver, CoordinatorPaths, MlxRecipe, ModelLoadApproval, PackageMetadata},
    campaign_summary,
    runner::launch_campaign,
};
use std::{
    io::{Read, Write},
    path::PathBuf,
    time::Duration,
};

fn path(key: &str) -> PathBuf {
    std::env::var_os(key)
        .expect("explicit operator path required")
        .into()
}

#[tokio::test]
#[ignore = "REAL MODEL: one authorized ABBA campaign, at most12 sequential loads/30min; normal preflight required"]
async fn authorized_abba_campaign() {
    assert_eq!(
        std::env::var("SEIYA_ALLOW_MODEL_CAMPAIGN").as_deref(),
        Ok("I_AUTHORIZE_ONE_ABBA_CAMPAIGN")
    );
    assert_eq!(
        std::env::var("SEIYA_EXTERNAL_RUNTIME_CLEARED").as_deref(),
        Ok("1")
    );
    let recipe = MlxRecipe {
        python: path("SEIYA_CAMPAIGN_PYTHON"),
        runtime_entrypoint: path("SEIYA_CAMPAIGN_ENTRYPOINT"),
        checkpoint: path("SEIYA_CAMPAIGN_CHECKPOINT"),
        coordination_directory: path("SEIYA_CAMPAIGN_COORDINATION"),
        minimum_free_bytes: 2 << 30,
    };
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
    let output = path("SEIYA_CAMPAIGN_OUTPUT");
    assert!(output.is_absolute() && !output.exists());
    let approval = ModelLoadApproval::confirm(&recipe, Duration::from_secs(1800)).unwrap();
    let driver =
        CampaignDriver::prepare_authorized(paths, output.clone(), recipe, packages, approval)
            .await
            .unwrap()
            .with_runtime_memory()
            .unwrap();
    let report = launch_campaign(driver).unwrap().wait().await.unwrap();
    let mut bytes = Vec::new();
    std::fs::File::open(output.join("events.jsonl"))
        .unwrap()
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= 32 * 1024 * 1024);
    let events = bytes
        .split(|b| *b == b'\n')
        .filter(|s| !s.is_empty())
        .map(serde_json::from_slice)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let summary = campaign_summary::summarize(&report, &events).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("comparison.json"))
        .unwrap()
        .write_all(&serde_json::to_vec_pretty(&summary).unwrap())
        .unwrap();
    println!("ABBA report: {report:?}; preserved {}", output.display());
    assert!(
        report.cleanup_confirmed,
        "cleanup must be reviewed before any further model"
    );
    assert!(
        report.completed,
        "incomplete campaign; inspect artifacts, do not retry automatically"
    );
}
