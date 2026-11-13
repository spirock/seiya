//! Explicit host-only preflight. No MLX import, weight hashing or runtime launch.
#![cfg(target_os = "macos")]
use seiya_observe::campaign::{
    assess, observe_host, review_pressure_window, MlxRecipe, Reservation,
};
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

#[tokio::test]
#[ignore = "explicit authorized host preflight: own sockets/lease and memory probes only; never loads a model"]
async fn record_authorized_host_preflight_without_model_load() {
    assert_eq!(
        std::env::var("SEIYA_ALLOW_HOST_PREFLIGHT").as_deref(),
        Ok("1")
    );
    assert_eq!(
        std::env::var("SEIYA_KNOWN_PORTS_CLEARED").as_deref(),
        Ok("1")
    );
    let output = PathBuf::from(
        std::env::var_os("SEIYA_HOST_PREFLIGHT_OUTPUT").expect("new absolute output path"),
    );
    let coordination = PathBuf::from(
        std::env::var_os("SEIYA_CAMPAIGN_COORDINATION")
            .expect("shared existing coordination directory"),
    );
    assert!(output.is_absolute() && !output.exists());
    assert!(coordination.is_absolute() && coordination.is_dir());
    let mut builder = std::fs::DirBuilder::new();
    use std::os::unix::fs::DirBuilderExt;
    builder.mode(0o700).create(&output).unwrap();
    let recipe = MlxRecipe {
        python: std::env::var_os("SEIYA_CAMPAIGN_PYTHON")
            .expect("operator Python path")
            .into(),
        runtime_entrypoint: std::env::var_os("SEIYA_CAMPAIGN_ENTRYPOINT")
            .expect("operator MLX entrypoint path")
            .into(),
        checkpoint: std::env::var_os("SEIYA_CAMPAIGN_CHECKPOINT")
            .expect("operator checkpoint path")
            .into(),
        coordination_directory: coordination,
        minimum_free_bytes: 2 << 30,
    };
    // The recipe is metadata only in this test. None of these model/runtime paths
    // is opened, imported, hashed or executed. Readiness never authorizes a load.
    let reservation = Reservation::acquire(&recipe).unwrap();
    let ports = reservation.ports().unwrap();
    let epoch = Instant::now();
    let mut observations = Vec::new();
    for index in 0..3 {
        if index > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        observations.push(observe_host(epoch, true).await.unwrap());
    }
    let now_millis = u64::try_from(epoch.elapsed().as_millis()).unwrap();
    let review = review_pressure_window(&observations, now_millis);
    let host = observations.last().unwrap().clone();
    let readiness = assess(&recipe, host, now_millis, Some(&reservation)).unwrap();
    let document = json!({"mode":"authorized-host-preflight","model_loaded":false,
        "weight_hashing":false,"model_campaign_started":false,"recipe":recipe,"readiness":readiness,
        "pressure_window_review":review,"observations":observations,
        "cooperative_lease_acquired":true,"held_ephemeral_ports":[ports.0,ports.1],
        "known_service_ports_clearance":"operator checked 8010/8012/8081 with lsof; not universal GPU exclusivity"});
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("preflight.json"))
        .unwrap();
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(&document).unwrap())
        .unwrap();
    drop(file);
    drop(reservation);
    // Confirm the lease was released; do not unlink its persistent file.
    drop(Reservation::acquire(&recipe).unwrap());
    println!("{}", serde_json::to_string(&document).unwrap());
    println!(
        "own lease/sockets released; no model started; artifacts {}",
        output.display()
    );
}
