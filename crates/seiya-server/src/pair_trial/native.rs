//! Opt-in macOS owner for one isolated browser trial. Never called by tests.
use super::limits::{PressureWindow, TrialLimits};
use super::{NativeChat, NativeDraft};
use crate::{
    prepared_web::{HostGate, PreparedWeb, AUXILIARY, FOREGROUND},
    runtime::{Backend, RuntimeConfig, RuntimeTransport},
};
use seiya_observe::{
    campaign::{observe_host, Pressure},
    image::{assets, native},
    probes::OwnedChild,
};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub const APPROVAL: &str = "isolated-gemma-qwen-web-trial-normal-pressure";
pub const SOLO_APPROVAL: &str = "isolated-solo-gemma-real-web-trial";
pub const RECOVERY_APPROVAL: &str = "review-and-archive-stopped-pair-before-solo-gemma";
pub(crate) const OLLAMA: &str = "/Applications/Ollama.app/Contents/Resources/ollama";
pub(crate) const DIGESTS: [&str; 2] = [
    "f4a8449d17818601a45b32b22b90520b5c1c0327a27374bfb84e1974bb74104c",
    "203e30078279db51132b9e026ceb7bb21330e5b1af67ef190671b375c9770404",
];
const GIB: u64 = 1024 * 1024 * 1024;

pub async fn rehearsal(root: &Path) -> Result<Value, String> {
    native::require_private_new(root)?;
    fs::create_dir(root).map_err(|_| "fixture_root")?;
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(|_| "fixture_mode")?;
    let reserve = std::net::TcpListener::bind("127.0.0.1:0").map_err(|_| "fixture_reserve")?;
    let port = reserve.local_addr().map_err(|_| "fixture_port")?.port();
    drop(reserve);
    let foreign = std::net::TcpListener::bind("127.0.0.1:0").map_err(|_| "fixture_foreign")?;
    let foreign_port = foreign.local_addr().map_err(|_| "fixture_port")?.port();
    let mut c = Command::new(std::env::current_exe().map_err(|_| "fixture_exe")?);
    c.arg("--fixture-respawn")
        .arg(port.to_string())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = OwnedChild::spawn(&mut c).map_err(|e| format!("fixture_spawn:{e:?}"))?;
    let proof = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if child
                .listening(port)
                .map_err(|e| format!("fixture_identity:{e:?}"))?
                && child.refresh().map_err(|_| "fixture_tree")?.len() >= 2
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if child
            .listening(foreign_port)
            .map_err(|_| "fixture_foreign_query")?
        {
            return Err("foreign_listener_adopted".into());
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "fixture_deadline".to_string())
    .and_then(|r| r);
    let cleanup = child
        .close()
        .map_err(|e| format!("fixture_cleanup:{e:?}"))?;
    proof?;
    if cleanup.processes_observed < 2 {
        return Err("fixture_worker_not_witnessed".into());
    }
    if !seiya_observe::probes::exited_group_is_empty(child.pid()).map_err(|_| "fixture_review")?
        || seiya_observe::probes::exited_group_is_empty(std::process::id())
            .map_err(|_| "fixture_review_self")?
    {
        return Err("fixture_review_absence_invalid".into());
    }
    let released = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .map_err(|_| "fixture_port_not_released")?;
    drop(released);
    let evidence = json!({"model_inference":false,"respawning_parent_fixture":true,"original_listener_verified":true,"foreign_listener_not_adopted":true,"port_released":true,"cleanup":cleanup});
    save(root, "evidence.json", &evidence)?;
    Ok(evidence)
}

fn bounded(path: &Path, cap: u64) -> Result<Vec<u8>, String> {
    let m = fs::symlink_metadata(path).map_err(|_| "model_metadata")?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() > cap {
        return Err("model_file_type_or_size".into());
    }
    let mut b = Vec::new();
    File::open(path)
        .map_err(|_| "model_open")?
        .take(cap + 1)
        .read_to_end(&mut b)
        .map_err(|_| "model_read")?;
    if b.len() as u64 > cap {
        return Err("model_file_limit".into());
    }
    Ok(b)
}
fn save(root: &Path, name: &str, value: &Value) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join(name))
        .map_err(|_| "trial_evidence_create")?;
    f.write_all(
        serde_json::to_string_pretty(value)
            .map_err(|_| "trial_evidence_json")?
            .as_bytes(),
    )
    .map_err(|_| "trial_evidence_write")?;
    f.sync_all().map_err(|_| "trial_evidence_sync".into())
}

/// Explicit human-authorized review. No model construction, loads or signals.
fn recovery_report_matches(m: &Value, r: &Value) -> bool {
    m["pid"]
        .as_u64()
        .is_some_and(|p| p > 0 && p <= i32::MAX as u64)
        && m["error"] == "OwnershipChanged"
        && m["pid"] == r["runtime_pid"]
        && r["mode"] == "ADR0047-isolated-real-browser-trial"
        && r["cleanup"]["confirmed"] == false
        && r["cleanup"]["error"] == m["error"]
}
pub async fn recover_quarantine(root: &Path) -> Result<Value, String> {
    native::require_private_new(root)?;
    fs::create_dir(root).map_err(|_| "review_directory")?;
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(|_| "review_mode")?;
    let _lease = native::quarantine_review_lease()?;
    let coord = Path::new(native::BASE).join("seiya-host-model-coordination");
    if coord
        .join("image-cleanup-uncertain.json")
        .symlink_metadata()
        .is_ok()
    {
        return Err("unreviewed_image_quarantine".into());
    }
    let marker = coord.join("pair-cleanup-uncertain.json");
    let m: Value =
        serde_json::from_slice(&bounded(&marker, 4096)?).map_err(|_| "review_marker_json")?;
    let evidence = Path::new(m["evidence"].as_str().ok_or("review_evidence_path")?)
        .canonicalize()
        .map_err(|_| "review_evidence_path")?;
    if evidence.parent() != Some(Path::new(native::BASE)) {
        return Err("review_evidence_outside_private_base".into());
    }
    let original: Value = serde_json::from_slice(&bounded(
        &evidence.join("startup-evidence.json"),
        2 * 1024 * 1024,
    )?)
    .map_err(|_| "review_original_json")?;
    if !recovery_report_matches(&m, &original) {
        return Err("review_marker_report_mismatch".into());
    }
    if !get(11434, "/api/ps").await?["models"]
        .as_array()
        .is_some_and(Vec::is_empty)
        || native::memory()["pressure"] != 1
    {
        return Err("review_host_not_normal_or_shared_resident".into());
    }
    let mut checked = Vec::new();
    let mut reservations = Vec::new();
    let mut original_checked = false;
    for (index, entry) in fs::read_dir(native::BASE)
        .map_err(|_| "review_listing")?
        .take(2049)
        .enumerate()
    {
        if index >= 2048 {
            return Err("review_listing_limit".into());
        }
        let entry = entry.map_err(|_| "review_entry")?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("seiya-real-web-")
            || !entry.file_type().map_err(|_| "review_entry_type")?.is_dir()
        {
            continue;
        }
        let report = entry.path().join("startup-evidence.json");
        if !report.exists() {
            return Err("review_prior_trial_has_no_startup_report".into());
        }
        let r: Value = serde_json::from_slice(&bounded(&report, 2 * 1024 * 1024)?)
            .map_err(|_| "review_trial_json")?;
        let Some(pid) = r["runtime_pid"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
        else {
            continue;
        };
        let port = r["runtime_port"]
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p >= 49152)
            .ok_or("review_trial_port")?;
        if !seiya_observe::probes::exited_group_is_empty(pid)
            .map_err(|_| "review_process_observation")?
        {
            return Err("review_root_or_group_still_present".into());
        }
        reservations.push(
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
                .map_err(|_| "review_private_port_not_free")?,
        );
        if entry.path().canonicalize().ok().as_ref() == Some(&evidence)
            && Some(u64::from(pid)) == m["pid"].as_u64()
        {
            original_checked = true;
        }
        checked.push(json!({"pid":pid,"port":port,"root_and_group_absent":true,"port_reserved_without_adoption":true}));
    }
    if !original_checked {
        return Err("review_original_not_checked".into());
    }
    let archive = coord.join(format!(
        "pair-cleanup-reviewed-{}.json",
        uuid::Uuid::new_v4()
    ));
    let record = json!({"mode":"ADR0049-human-authorized-operational-recovery","checks":checked,"shared_empty":true,"pressure":1,"retrospective_complete_tree_proven":false,"archive":archive,"models_loaded":false,"signals_sent":false});
    save(root, "review.json", &record)?;
    if serde_json::from_slice::<Value>(&bounded(&marker, 4096)?)
        .ok()
        .as_ref()
        != Some(&m)
        || archive.exists()
    {
        return Err("review_marker_changed_or_archive_exists".into());
    }
    fs::rename(&marker, &archive).map_err(|_| "review_archive")?;
    if let Err(e) = save(
        root,
        "result.json",
        &json!({"archived":true,"review":record}),
    ) {
        if !marker.exists() {
            let _ = fs::rename(&archive, &marker);
        }
        return Err(e);
    }
    drop(reservations);
    Ok(json!({"archived":true,"models_loaded":false,"review_directory":root}))
}
fn digest_name(s: &str) -> bool {
    s.len() == 71
        && s.starts_with("sha256:")
        && s[7..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn stage(store: &Path, source: &Path, tag: &str, digest: &str) -> Result<(), String> {
    let (name, version) = tag.split_once(':').ok_or("selected_tag")?;
    let relative = Path::new("manifests-v2/ollama.com/library")
        .join(name)
        .join(version);
    let original = source.join(&relative);
    let manifest_blob = source.join("blobs").join(format!("sha256-{digest}"));
    if original
        .canonicalize()
        .map_err(|_| "selected_manifest_target")?
        != manifest_blob
    {
        return Err("selected_manifest_target_changed".into());
    }
    // Tags are existing symlinks. Read only the exact pinned regular blob, not
    // an arbitrary resolved file and never mutate the shared link.
    let raw = bounded(&manifest_blob, 1024 * 1024)?;
    let (_, actual) = assets::hash_reader(raw.as_slice(), 1024 * 1024)?;
    if actual != digest {
        return Err("selected_manifest_digest_changed".into());
    }
    let m: Value = serde_json::from_slice(&raw).map_err(|_| "selected_manifest_json")?;
    let mut refs = m["layers"]
        .as_array()
        .filter(|r| r.len() <= 4096)
        .ok_or("selected_layers_limit")?
        .clone();
    refs.push(m["config"].clone());
    refs.push(json!({"digest":format!("sha256:{digest}"),"size":raw.len()}));
    for r in refs {
        let d = r["digest"]
            .as_str()
            .filter(|s| digest_name(s))
            .ok_or("selected_blob_digest")?;
        let file = d.replace(':', "-");
        let src = source.join("blobs").join(&file);
        let meta = fs::symlink_metadata(&src).map_err(|_| "selected_blob_missing")?;
        if !meta.is_file()
            || meta.file_type().is_symlink()
            || Some(meta.len()) != r["size"].as_u64()
        {
            return Err("selected_blob_type_or_size".into());
        }
        let dst = store.join("blobs").join(file);
        if dst.symlink_metadata().is_ok() {
            if fs::read_link(&dst).ok().as_ref() != Some(&src) {
                return Err("selected_blob_link_conflict".into());
            }
        } else {
            std::os::unix::fs::symlink(src, dst).map_err(|_| "selected_blob_link")?;
        }
    }
    let dst = store.join(relative);
    fs::create_dir_all(dst.parent().ok_or("manifest_parent")?)
        .map_err(|_| "manifest_private_parent")?;
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)
        .map_err(|_| "manifest_private_copy")?;
    f.write_all(&raw)
        .map_err(|_| "manifest_private_copy".into())
}

struct Gate {
    solo: bool,
    ready: AtomicBool,
    reason: AtomicU8,
    seen: Mutex<Instant>,
}
impl Gate {
    fn stop(&self, reason: u8) {
        self.ready.store(false, Ordering::Release);
        let _ = self
            .reason
            .compare_exchange(0, reason, Ordering::AcqRel, Ordering::Acquire);
        let _ = self
            .reason
            .compare_exchange(1, reason, Ordering::AcqRel, Ordering::Acquire);
    }
    fn activate(&self) -> Result<(), String> {
        if self
            .reason
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("trial_stopped_before_ready".into());
        }
        self.ready.store(true, Ordering::Release);
        Ok(())
    }
}
impl HostGate for Gate {
    fn admits(&self, f: &str, a: &str) -> bool {
        f == FOREGROUND
            && a == if self.solo { "" } else { AUXILIARY }
            && self.ready.load(Ordering::Acquire)
            && self.reason.load(Ordering::Acquire) == 1
            && self
                .seen
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .elapsed()
                < Duration::from_secs(1)
    }
    fn status(&self) -> Option<&'static str> {
        Some(match self.reason.load(Ordering::Acquire) {
            0 => "Preparando runtime privado; aún sin admisión.",
            1 => "Prueba real temporal con guardas activas.",
            2 => "Prueba detenida por presión/swap u observación de memoria inválida.",
            3 => "Prueba detenida: ownership/listener del runtime no confirmado.",
            4 => "Prueba detenida por límite de disco/log.",
            5 => "Prueba temporal finalizada.",
            7 => "Runtime sin finalización confirmada; árbol propio detenido por seguridad.",
            _ => "Prueba no iniciada: preflight o preparación no superados; revisa evidencia.",
        })
    }
}
fn pressure_allowed(sample: &Value, baseline: u64, p: &TrialLimits) -> bool {
    (sample["pressure"] == 1 || p.warning_budget_ms > 0 && sample["pressure"] == 2)
        && sample["swap_bytes"]
            .as_u64()
            .is_some_and(|s| s.saturating_sub(baseline) <= p.max_swap_growth_mib * 1024 * 1024)
}
fn preflight_allowed(
    pressure: Pressure,
    physical: Option<u64>,
    free: Option<u64>,
    window: &[Value],
    disk: u64,
    p: &TrialLimits,
) -> bool {
    if pressure != Pressure::Normal
        || physical.is_none_or(|n| n < 16 * GIB)
        || disk < 4 * GIB
        || window.len() != 9
    {
        return false;
    }
    if p.preflight_min_free_mib > 0
        && free.is_none_or(|n| n < p.preflight_min_free_mib * 1024 * 1024)
    {
        return false;
    }
    let mut previous = None;
    let mut first = None;
    let mut lowest_swap = u64::MAX;
    for s in window {
        let (Some(time), Some(swap)) = (s["elapsed_ms"].as_u64(), s["swap_bytes"].as_u64()) else {
            return false;
        };
        if s["pressure"] != 1 {
            return false;
        }
        if previous.is_some_and(|p| time <= p || time - p > 1000) {
            return false;
        }
        first.get_or_insert(time);
        previous = Some(time);
        lowest_swap = lowest_swap.min(swap);
        if swap - lowest_swap > p.preflight_max_swap_growth_mib * 1024 * 1024 {
            return false;
        }
    }
    matches!(
        previous.zip(first).map(|(last, first)| last - first),
        Some(2000..=4000)
    )
}

async fn get(port: u16, path: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| "trial_inventory_client")?;
    let mut r = client
        .get(format!("http://127.0.0.1:{port}{path}"))
        .send()
        .await
        .map_err(|_| "trial_inventory_unavailable")?;
    if r.status() != reqwest::StatusCode::OK {
        return Err("trial_inventory_status".into());
    }
    let mut raw = Vec::new();
    while let Some(c) = r.chunk().await.map_err(|_| "trial_inventory_read")? {
        if raw.len() + c.len() > 65536 {
            return Err("trial_inventory_limit".into());
        }
        raw.extend(c);
    }
    serde_json::from_slice(&raw).map_err(|_| "trial_inventory_json".into())
}
fn close(root: &Path, owner: &Mutex<Option<OwnedChild>>, report: &Mutex<Value>) {
    let mut guard = owner.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(mut child) = guard.take() {
        let value = match child.close() {
            Ok(c) => json!({"confirmed":true,"detail":c}),
            Err(e) => {
                let q = json!({"pid":child.pid(),"evidence":root,"error":format!("{e:?}")});
                let _ = save(
                    &Path::new(native::BASE).join("seiya-host-model-coordination"),
                    "pair-cleanup-uncertain.json",
                    &q,
                );
                json!({"confirmed":false,"error":format!("{e:?}")})
            }
        };
        report.lock().unwrap_or_else(|p| p.into_inner())["cleanup"] = value;
    }
}

pub async fn run(root: &Path, limits: TrialLimits) -> Result<(), String> {
    run_mode(root, limits, false).await
}
pub async fn run_solo(root: &Path, limits: TrialLimits) -> Result<(), String> {
    run_mode(root, limits, true).await
}
async fn run_mode(root: &Path, limits: TrialLimits, solo: bool) -> Result<(), String> {
    let limits =
        TrialLimits::parse(&serde_json::to_vec(&limits).map_err(|_| "trial_limits_encode")?)?;
    native::require_private_new(root)?;
    fs::create_dir(root).map_err(|_| "trial_root")?;
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(|_| "trial_root_mode")?;
    for p in ["home", "tmp", "models", "models/blobs"] {
        fs::create_dir(root.join(p)).map_err(|_| "trial_directory")?;
    }
    let epoch = Instant::now();
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| "trial_signal_handler")?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| "trial_signal_handler")?;
    let gate = Arc::new(Gate {
        solo,
        ready: AtomicBool::new(false),
        reason: AtomicU8::new(0),
        seen: Mutex::new(epoch),
    });
    let owner = Arc::new(Mutex::new(None));
    let report = Arc::new(Mutex::new(
        json!({"mode":if solo{"ADR0049-solo-gemma-real-web-trial"}else{"ADR0047-isolated-real-browser-trial"},"limits":limits,"inference_requested":false,"ready":false,"samples":[],"warmups":[],"error":null}),
    ));
    let reservation =
        std::net::TcpListener::bind("127.0.0.1:0").map_err(|_| "trial_runtime_port")?;
    let port = reservation
        .local_addr()
        .map_err(|_| "trial_runtime_port")?
        .port();
    drop(reservation);
    let foreground = Arc::new(NativeChat::new(port, FOREGROUND).map_err(|_| "trial_foreground")?);
    let auxiliary = Arc::new(NativeChat::new(port, AUXILIARY).map_err(|_| "trial_auxiliary")?);
    let config = RuntimeConfig {
        id: "isolated-pair-trial".into(),
        model: FOREGROUND.into(),
        backend: Backend::Mlx,
        base_url: format!("http://127.0.0.1:{port}/v1"),
        timeout_seconds: 120,
        max_output_tokens: 256,
    };
    let state = if solo {
        PreparedWeb::solo(config, foreground.clone(), gate.clone())
    } else {
        PreparedWeb::with_host(
            config,
            foreground.clone(),
            Arc::new(NativeDraft(auxiliary.clone())),
            gate.clone(),
        )
    }
    .map_err(|_| "trial_web_state")?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, limits.web_port))
        .await
        .map_err(|_| "trial_web_port_busy_no_adoption")?;
    let address = listener.local_addr().map_err(|_| "trial_web_address")?;
    let (app, link) = super::access::app(address, state.clone()).map_err(|_| "trial_web_app")?;
    let mut access = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(root.join("access.url"))
        .map_err(|_| "trial_access_file")?;
    access
        .write_all(link.as_bytes())
        .map_err(|_| "trial_access_write")?;
    drop(access);
    let mut web = tokio::spawn(async move { axum::serve(listener, app).await });
    let mut monitor = None;
    let mut lease = None;
    let done = Arc::new(AtomicBool::new(false));
    let setup: Result<(), String> = tokio::select! {result=async{
        lease=Some(native::lease()?);
        if !get(11434,"/api/ps").await?["models"].as_array().is_some_and(Vec::is_empty){return Err("shared_runtime_resident_no_adoption".into());}
        let host=observe_host(epoch,true).await.map_err(|_|"trial_preflight_observation")?;
        let mut window=Vec::new();
        for i in 0..9 {if i>0{tokio::time::sleep(Duration::from_millis(250)).await;}let mut m=native::memory();m["memory_stop_enabled"]=json!(true);m["elapsed_ms"]=json!(epoch.elapsed().as_millis() as u64);window.push(m);}
        let memory=window.last().cloned().ok_or("trial_memory_window")?;
        let baseline=window[0]["swap_bytes"].as_u64().ok_or("trial_swap_observation")?;let disk=native::disk_free(root)?;
        {let mut r=report.lock().unwrap();r["preflight"]=json!({"method":"ADR0047-normal-pressure-stable-swap-experiment","host":host,"free_pages_diagnostic_only":limits.preflight_min_free_mib==0,"memory":memory,"window":window,"free_disk_bytes":disk,"capacity_guaranteed":false});}
        if !preflight_allowed(host.pressure,host.physical_bytes,host.free_bytes,&window,disk,&limits)||!pressure_allowed(&memory,baseline,&limits){return Err("trial_preflight_normal_stable_window_physical16GiB_disk4GiB_required".into());}
        let source=Path::new(&std::env::var("HOME").map_err(|_|"trial_home")?).join(".ollama").canonicalize().map_err(|_|"trial_existing_store")?;
        for (tag,digest) in [FOREGROUND,AUXILIARY].into_iter().zip(DIGESTS).take(if solo{1}else{2}){stage(&root.join("models"),&source,tag,digest)?;}
        let (_,hash)=assets::hash_reader(File::open(OLLAMA).map_err(|_|"trial_runtime_binary")?,512*1024*1024)?;
        report.lock().unwrap()["runtime_binary_sha256"]=json!(hash);
        if !pressure_allowed(&native::memory(),baseline,&limits){return Err("trial_fresh_preload_memory_gate".into());}
        let profile=format!("(version 1)(allow default)(deny file-write* (subpath {}))",serde_json::to_string(&source).map_err(|_|"trial_profile_path")?);
        fs::write(root.join("readonly.sb"),profile).map_err(|_|"trial_profile")?;
        let log_path=root.join("runtime.log");let log=OpenOptions::new().write(true).create_new(true).mode(0o600).open(&log_path).map_err(|_|"trial_log")?;
        let mut c=Command::new("/usr/bin/sandbox-exec");c.arg("-f").arg(root.join("readonly.sb")).arg(OLLAMA).arg("serve").env_clear()
            .env("PATH","/usr/bin:/bin:/usr/sbin:/sbin").env("LC_ALL","C").env("HOME",root.join("home")).env("TMPDIR",root.join("tmp"))
            .env("OLLAMA_MODELS",root.join("models")).env("OLLAMA_HOST",format!("127.0.0.1:{port}"))
            .env("OLLAMA_NO_CLOUD","1").env("OLLAMA_NOPRUNE","1").env("OLLAMA_MAX_LOADED_MODELS",if solo{"1"}else{"2"}).env("OLLAMA_NUM_PARALLEL","1").env("OLLAMA_CONTEXT_LENGTH","4096").env("OLLAMA_KEEP_ALIVE","30m").env("OLLAMA_LOAD_TIMEOUT","120s").env("OLLAMA_DEBUG","0")
            .current_dir(root).stdin(Stdio::null()).stdout(log.try_clone().map_err(|_|"trial_log_clone")?).stderr(log);
        let child=OwnedChild::spawn(&mut c).map_err(|e|format!("trial_runtime_spawn:{e:?}"))?;
        report.lock().unwrap()["runtime_pid"]=json!(child.pid());report.lock().unwrap()["runtime_port"]=json!(port);*owner.lock().unwrap()=Some(child);
        let (o,g,r,d,dir,s,policy)=(owner.clone(),gate.clone(),report.clone(),done.clone(),root.to_owned(),state.clone(),limits.clone());
        monitor=Some(tokio::spawn(async move{
            let mut pressure=PressureWindow::new(epoch.elapsed().as_millis() as u64);
            loop{
                if d.load(Ordering::Acquire){break;}
                let mut m=native::memory();m["memory_stop_enabled"]=json!(true);let mut code=0;
                if !pressure.observe(epoch.elapsed().as_millis() as u64,&m,baseline,&policy){code=2;}
                m["warning_used_ms"]=json!(pressure.warning_used_ms);
                let owns={let mut o=o.lock().unwrap_or_else(|p|p.into_inner());o.as_mut().ok_or(()).and_then(|c|{c.refresh().map_err(|_|())?;if c.status().is_some(){return Err(());}c.listening(port).map_err(|_|())})};
                if owns.is_err()||(epoch.elapsed()>Duration::from_secs(15)&&owns!=Ok(true)){code=3;}
                if !native::disk_free(&dir).is_ok_and(|n|n>=2*GIB)||!fs::metadata(&log_path).is_ok_and(|m|m.len()<=4*1024*1024){code=4;}
                if code==0&&s.requires_trial_stop(){code=7;}
                {let mut r=r.lock().unwrap_or_else(|p|p.into_inner());let samples=r["samples"].as_array_mut().unwrap();if samples.len()<8000{samples.push(json!({"elapsed_seconds":epoch.elapsed().as_secs_f64(),"memory":m,"listener_owned":owns==Ok(true)}));}}
                if code!=0{g.stop(code);close(&dir,&o,&r);break;}
                if owns==Ok(true){*g.seen.lock().unwrap_or_else(|p|p.into_inner())=Instant::now();}
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }));
        tokio::time::timeout(Duration::from_secs(15),async{
            loop{let listening=owner.lock().unwrap().as_mut().ok_or("trial_runtime_stopped")?.listening(port).map_err(|_|"trial_listener_identity")?;
                if listening{break Ok::<(),String>(());}tokio::time::sleep(Duration::from_millis(50)).await;}
        }).await.map_err(|_|"trial_listener_deadline")??;
        if get(port,"/api/version").await?["version"]!="0.40.14"||!get(port,"/api/ps").await?["models"].as_array().is_some_and(Vec::is_empty){return Err("trial_runtime_version_or_initial_residency".into());}
        for (model,chat) in [(FOREGROUND,foreground),(AUXILIARY,auxiliary)].into_iter().take(if solo{1}else{2}){
            if gate.reason.load(Ordering::Acquire)!=0{return Err("trial_stopped_before_warmup".into());}
            if !get(11434,"/api/ps").await?["models"].as_array().is_some_and(Vec::is_empty){return Err("shared_runtime_became_resident_no_adoption".into());}
            if !pressure_allowed(&native::memory(),baseline,&limits)||!owner.lock().unwrap().as_mut().ok_or("trial_runtime_stopped")?.listening(port).map_err(|_|"trial_warmup_listener")?{return Err("trial_fresh_warmup_gate".into());}
            report.lock().unwrap()["inference_requested"]=json!(true);
            let result=chat.complete(json!({"model":model,"messages":[{"role":"user","content":"Responde exactamente SEIYA_OK, sin explicaciones."}]})).await.map_err(|e|format!("trial_warmup:{e:?}"))?;
            if result["choices"][0]["message"]["content"].as_str().map(str::trim)!=Some("SEIYA_OK"){return Err("trial_warmup_response_invalid".into());}
            report.lock().unwrap()["warmups"].as_array_mut().unwrap().push(json!({"model":model,"marker_validated":true,"usage":result["usage"]}));
        }
        let ps=get(port,"/api/ps").await?;let models=ps["models"].as_array().ok_or("trial_residency_shape")?;
        let expected:&[&str]=if solo{&[FOREGROUND]}else{&[FOREGROUND,AUXILIARY]};
        if models.len()!=expected.len()||expected.iter().any(|name|!models.iter().any(|m|m["name"]==*name)){return Err("trial_selected_residency_not_confirmed".into());}
        report.lock().unwrap()["resident_models_at_startup"]=json!(models.iter().map(|m|json!({"name":m["name"],"context_length":m["context_length"]})).collect::<Vec<_>>());
        gate.activate()?;
        if solo{state.trial_solo_smoke().await?;}else{state.trial_tool_smoke().await?;}
        if !gate.admits(FOREGROUND,if solo{""}else{AUXILIARY}){return Err("trial_stopped_after_tool_smoke".into());}
        state.mark_real_trial();report.lock().unwrap()[if solo{"browser_chat_smoke_validated"}else{"tool_job_smoke_validated"}]=json!(true);report.lock().unwrap()["ready"]=json!(true);Ok(())
    }=>result,_=term.recv()=>Err("trial_cancelled".into()),_=interrupt.recv()=>Err("trial_cancelled".into()),_=tokio::time::sleep(Duration::from_secs(limits.max_session_seconds).saturating_sub(epoch.elapsed()))=>Err("trial_session_deadline".into())};
    let cancelled = setup.as_ref().is_err_and(|e| e == "trial_cancelled");
    if let Err(error) = setup {
        gate.stop(6);
        report.lock().unwrap()["error"] = json!(error);
        close(root, &owner, &report);
        lease.take();
    }
    {
        let r = report.lock().unwrap();
        if let Err(error) = save(root, "startup-evidence.json", &r) {
            drop(r);
            gate.stop(6);
            done.store(true, Ordering::Release);
            close(root, &owner, &report);
            web.abort();
            return Err(error);
        }
        println!(
            "{}",
            json!({"access_file":root.join("access.url"),"ready":r["ready"],"error":r["error"],"trial_status":gate.status(),"expires_after_seconds":limits.max_session_seconds.saturating_sub(epoch.elapsed().as_secs())})
        );
    }
    let remaining = if cancelled {
        Duration::ZERO
    } else {
        Duration::from_secs(limits.max_session_seconds).saturating_sub(epoch.elapsed())
    };
    let web_finished = tokio::select! {_=&mut web=>true,_=tokio::time::sleep(remaining)=>false,_=interrupt.recv()=>false,_=term.recv()=>false};
    gate.stop(5);
    done.store(true, Ordering::Release);
    if let Some(m) = monitor {
        let _ = m.await;
    }
    close(root, &owner, &report);
    if !web_finished {
        web.abort();
        let _ = web.await;
    }
    let r = report.lock().unwrap();
    save(root, "final-evidence.json", &r)?;
    drop(r);
    drop(lease);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_requires_matching_original_failure_not_just_an_arbitrary_pid() {
        let marker = json!({"pid":123,"error":"OwnershipChanged"});
        let mut report = json!({"mode":"ADR0047-isolated-real-browser-trial","runtime_pid":123,"cleanup":{"confirmed":false,"error":"OwnershipChanged"}});
        assert!(recovery_report_matches(&marker, &report));
        report["runtime_pid"] = json!(124);
        assert!(!recovery_report_matches(&marker, &report));
        assert!(!recovery_report_matches(&json!({}), &json!({})));
    }
    #[test]
    fn staging_accepts_only_pinned_tag_target_and_regular_matching_blobs() {
        let root = std::env::temp_dir().join(format!(
            "seiya-trial-store-fixture-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let source = root.join("source");
        let store = root.join("private");
        fs::create_dir_all(source.join("blobs")).unwrap();
        fs::create_dir_all(store.join("blobs")).unwrap();
        let d = format!("sha256:{}", "0".repeat(64));
        fs::write(source.join("blobs").join(d.replace(':', "-")), b"abc").unwrap();
        let raw = serde_json::to_vec(&json!({"config":{"digest":d,"size":3},"layers":[]})).unwrap();
        let (_, hash) = assets::hash_reader(raw.as_slice(), 1024).unwrap();
        let blob = source.join("blobs").join(format!("sha256-{hash}"));
        fs::write(&blob, &raw).unwrap();
        let tag = source.join("manifests-v2/ollama.com/library/gemma4/e2b-mlx");
        fs::create_dir_all(tag.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&blob, &tag).unwrap();
        stage(&store, &source, FOREGROUND, &hash).unwrap();
        assert_eq!(
            fs::read(store.join("manifests-v2/ollama.com/library/gemma4/e2b-mlx")).unwrap(),
            raw
        );
        assert!(tag.is_symlink());
        fs::remove_file(&tag).unwrap();
        let elsewhere = root.join("other");
        fs::write(&elsewhere, &raw).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &tag).unwrap();
        assert!(stage(&store, &source, FOREGROUND, &hash).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn pair_trial_does_not_inherit_warning_critical_or_swap_exceptions() {
        let limits = TrialLimits::default();
        for p in [2, 4, 0] {
            assert!(!pressure_allowed(
                &json!({"pressure":p,"swap_bytes":100}),
                100,
                &limits
            ));
        }
        assert!(pressure_allowed(
            &json!({"pressure":1,"swap_bytes":100}),
            100,
            &limits
        ));
        assert!(!pressure_allowed(
            &json!({"pressure":1,"swap_bytes":513*1024*1024}),
            0,
            &limits
        ));
        assert!(!digest_name("sha256:../escape"));
        assert!(digest_name(&format!("sha256:{}", DIGESTS[0])));
    }
    #[test]
    fn preflight_uses_stable_os_observation_not_free_cache_misclassification() {
        let limits = TrialLimits::default();
        let window: Vec<_> = (0..9)
            .map(|i| json!({"elapsed_ms":i*250,"pressure":1,"swap_bytes":100}))
            .collect();
        assert!(preflight_allowed(
            Pressure::Normal,
            Some(16 * GIB),
            Some(0),
            &window,
            4 * GIB,
            &limits
        ));
        let custom = TrialLimits {
            preflight_min_free_mib: 2048,
            ..limits.clone()
        };
        assert!(!preflight_allowed(
            Pressure::Normal,
            Some(16 * GIB),
            Some(128 * 1024 * 1024),
            &window,
            4 * GIB,
            &custom
        ));
        for index in [0, 4, 8] {
            let mut bad = window.clone();
            bad[index]["pressure"] = json!(2);
            assert!(!preflight_allowed(
                Pressure::Normal,
                Some(16 * GIB),
                Some(0),
                &bad,
                4 * GIB,
                &limits
            ));
        }
        let mut swap = window.clone();
        swap[8]["swap_bytes"] = json!(33 * 1024 * 1024);
        assert!(!preflight_allowed(
            Pressure::Normal,
            Some(16 * GIB),
            Some(0),
            &swap,
            4 * GIB,
            &limits
        ));
        let mut stale = window.clone();
        stale[8]["elapsed_ms"] = json!(5000);
        assert!(!preflight_allowed(
            Pressure::Normal,
            Some(16 * GIB),
            Some(0),
            &stale,
            4 * GIB,
            &limits
        ));
        let mut missing = window.clone();
        missing[3]["swap_bytes"] = Value::Null;
        assert!(!preflight_allowed(
            Pressure::Normal,
            Some(16 * GIB),
            Some(0),
            &missing,
            4 * GIB,
            &limits
        ));
        assert!(!preflight_allowed(
            Pressure::Normal,
            None,
            Some(0),
            &window,
            4 * GIB,
            &limits
        ));
    }
}
