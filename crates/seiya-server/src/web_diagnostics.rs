//! Temporary LAN connectivity host; deliberately never calls model preparation.
use crate::{
    image_web::{LanConfig, TlsListener},
    pair_trial,
    prepared_web::PreparedWeb,
};
use seiya_observe::image::native;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::IpAddr,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    time::Duration,
};
pub const APPROVAL: &str = "temporary-LAN-TLS-diagnostics-without-inference";
pub async fn run(
    root: &Path,
    config: LanConfig,
    previous_stop: Option<&Path>,
) -> Result<(), String> {
    let IpAddr::V4(host) = config.address.ip() else {
        return Err("private_ipv4_required".into());
    };
    if !host.is_private() || config.address.port() != 8016 || !(8..=30).contains(&config.prefix) {
        return Err("private_LAN_explicit_port8016_and_subnet_required".into());
    }
    let acceptor = crate::image_web::tls(&config)?;
    native::require_private_new(root)?;
    fs::create_dir(root).map_err(|_| "diagnostics_root")?;
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(|_| "diagnostics_mode")?;
    let diagnostics = crate::diagnostics::Diagnostics::persistent(
        &config.history,
        std::sync::Arc::new(native::memory),
    )?;
    if let Some(path) = previous_stop {
        diagnostics.import_stop(path)?;
    }
    let state = PreparedWeb::diagnostics_only()
        .map_err(|_| "diagnostics_state")?
        .with_diagnostics(diagnostics)
        .map_err(|_| "diagnostics_attach")?;
    let tcp = tokio::net::TcpListener::bind(config.address)
        .await
        .map_err(|_| "diagnostics_port_busy_no_adoption")?;
    let (app, user, probe) = pair_trial::access::lan_tls_probe_app(config.address, state)
        .map_err(|_| "diagnostics_auth")?;
    for (name, contents) in [("access.url", user), ("probe.url", probe)] {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(root.join(name))
            .map_err(|_| "diagnostics_access_file")?;
        f.write_all(contents.as_bytes())
            .map_err(|_| "diagnostics_access_write")?;
    }
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| "diagnostics_signal")?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| "diagnostics_signal")?;
    let listener = TlsListener {
        tcp,
        acceptor,
        host,
        prefix: config.prefix,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "diagnostics_clock")?
        .as_secs();
    let report = serde_json::json!({"ready":true,"mode":"no-inference-diagnostics","address":config.address,"pid":std::process::id(),"access_file":root.join("access.url"),"expires_seconds":1800,"started_at_unix":now,"expires_at_unix":now+1800,"model_inference_requested":false});
    let mut evidence = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(root.join("startup-evidence.json"))
        .map_err(|_| "diagnostics_evidence")?;
    evidence
        .write_all(
            serde_json::to_vec_pretty(&report)
                .map_err(|_| "diagnostics_report")?
                .as_slice(),
        )
        .map_err(|_| "diagnostics_evidence_write")?;
    println!("{report}");
    let result=axum::serve(listener,app).with_graceful_shutdown(async move{tokio::select!{_=term.recv()=>{},_=interrupt.recv()=>{},_=tokio::time::sleep(Duration::from_secs(1800))=>{}}}).await.map_err(|_|"diagnostics_serve".to_owned());
    let mut evidence = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(root.join("final-evidence.json"))
        .map_err(|_| "diagnostics_final_evidence")?;
    evidence.write_all(serde_json::to_vec_pretty(&serde_json::json!({"server_stopped":true,"no_runtime_started":true,"serve_ok":result.is_ok()})).map_err(|_|"diagnostics_final_json")?.as_slice()).map_err(|_|"diagnostics_final_write")?;
    result
}
