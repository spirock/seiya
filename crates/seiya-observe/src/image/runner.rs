//! One explicitly approved native image attempt. No Python or shell interpreter.
use super::{
    artifact,
    assets::{self, Selection},
    native,
};
use crate::probes::OwnedChild;
use seiya_contracts::image_job::{ImageRequest, MAX_PNG_BYTES};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const PROMPT: &str = "A small blue ceramic cube on a white table, clean studio lighting.";
pub const APPROVAL: &str = "one-qwen-image21-turbo-q4-512-evaluation-600s";

fn limits(elapsed: Duration, free: u64, log_bytes: u64) -> Result<(), String> {
    if elapsed >= Duration::from_secs(600) {
        return Err("image_600_second_deadline".into());
    }
    if free < 2 * 1024u64.pow(3) {
        return Err("image_disk_reserve_exhausted".into());
    }
    if log_bytes > 4 * 1024 * 1024 {
        return Err("image_log_limit".into());
    }
    Ok(())
}

pub fn argv(binary: &Path, assets: &Path, output: &Path) -> Vec<std::ffi::OsString> {
    argv_for(
        binary,
        assets,
        output,
        &ImageRequest {
            prompt: PROMPT.into(),
            width: 512,
            height: 512,
            seed: 42,
        },
    )
}
pub(super) fn argv_for(
    binary: &Path,
    assets: &Path,
    output: &Path,
    request: &ImageRequest,
) -> Vec<std::ffi::OsString> {
    let width = request.width.to_string();
    let height = request.height.to_string();
    let seed = request.seed.to_string();
    let mut a = vec![binary.as_os_str().to_owned()];
    for (flag, file) in [
        ("--diffusion-model", "diffusion-q4.gguf"),
        ("--llm", "text-encoder-q4.gguf"),
        ("--vae", "vae-bf16.safetensors"),
    ] {
        a.push(flag.into());
        a.push(assets.join(file).into_os_string());
    }
    a.extend(
        [
            "-p",
            request.prompt.as_str(),
            "-W",
            width.as_str(),
            "-H",
            height.as_str(),
            "--seed",
            seed.as_str(),
            "--batch-count",
            "1",
            "--steps",
            "8",
            "--cfg-scale",
            "1.0",
            "--sampling-method",
            "euler",
            "--sigmas",
            "qwen-image-2.1-turbo",
            "--offload-to-cpu",
            "--clip-on-cpu",
            "--vae-on-cpu",
            "--vae-tiling",
            "--disable-image-metadata",
            "--model-args",
            "qwen_image_2_1_prefix_cache=false",
            "-o",
        ]
        .into_iter()
        .map(Into::into),
    );
    a.push(output.as_os_str().to_owned());
    a
}
fn read(path: &Path, cap: u64) -> Result<Vec<u8>, String> {
    let m = fs::symlink_metadata(path).map_err(|_| "artifact_metadata")?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() > cap {
        return Err("artifact_type_or_size".into());
    }
    let mut b = Vec::new();
    File::open(path)
        .map_err(|_| "artifact_open")?
        .take(cap + 1)
        .read_to_end(&mut b)
        .map_err(|_| "artifact_read")?;
    if b.len() as u64 > cap {
        return Err("artifact_size_limit".into());
    }
    Ok(b)
}
fn record(root: &Path, name: &str, value: &Value) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))
        .map_err(|_| "evidence_open")?;
    f.write_all(
        serde_json::to_string_pretty(value)
            .map_err(|_| "evidence_json")?
            .as_bytes(),
    )
    .map_err(|_| "evidence_write".into())
}
fn private_root(root: &Path) -> Result<(), String> {
    native::require_private_new(root)?;
    fs::create_dir(root).map_err(|_| "private_root_create")?;
    fs::set_permissions(root, fs::Permissions::from_mode(0o700))
        .map_err(|_| "private_root_mode".into())
}
fn private_child_command(program: &Path, root: &Path) -> Command {
    let mut c = Command::new(program);
    c.env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LC_ALL", "C")
        .env("HOME", root.join("home"))
        .env("TMPDIR", root.join("tmp"))
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

pub fn sandbox_probe(path: &Path, alias: &Path) -> Result<(), String> {
    for p in [path, alias] {
        if !p
            .canonicalize()
            .map_err(|_| "probe_path")?
            .starts_with(Path::new(native::BASE))
        {
            return Err("probe_path_outside_private_temp".into());
        }
        // Only this synthetic fixture resolves an alias. Production artifacts
        // still reject symlinks in read(). Check the resolved sentinel itself.
        let resolved = p.canonicalize().map_err(|_| "probe_path")?;
        if read(&resolved, 64)? != b"unchanged" {
            return Err("probe_contents".into());
        }
        if !matches!(fs::write(p,b"forbidden"),Err(e) if e.kind()==std::io::ErrorKind::PermissionDenied)
        {
            return Err("sandbox_write_was_not_denied".into());
        }
    }
    if !matches!(std::net::TcpStream::connect("127.0.0.1:9"),Err(e) if e.kind()==std::io::ErrorKind::PermissionDenied)
    {
        return Err("sandbox_network_was_not_denied".into());
    }
    Ok(())
}
pub fn rehearsal(root: &Path) -> Result<Value, String> {
    private_root(root)?;
    fs::create_dir(root.join("home")).map_err(|_| "probe_home")?;
    fs::create_dir(root.join("tmp")).map_err(|_| "probe_tmp")?;
    let _lease = native::lease()?;
    let protected = root.join("protected");
    fs::create_dir(&protected).map_err(|_| "probe_dir")?;
    let file = protected.join("sentinel");
    fs::write(&file, b"unchanged").map_err(|_| "probe_file")?;
    let alias = root.join("alias");
    std::os::unix::fs::symlink(&file, &alias).map_err(|_| "probe_link")?;
    let profile = root.join("probe.sb");
    fs::write(&profile, native::child_profile(&protected)?).map_err(|_| "probe_profile")?;
    let mut c = private_child_command(Path::new("/usr/bin/sandbox-exec"), root);
    c.arg("-f")
        .arg(&profile)
        .arg(std::env::current_exe().map_err(|_| "probe_exe")?)
        .arg("--sandbox-probe")
        .arg(&file)
        .arg(&alias);
    let mut child = OwnedChild::spawn(&mut c).map_err(|e| format!("probe_spawn:{e:?}"))?;
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        child
            .refresh()
            .map_err(|e| format!("probe_identity:{e:?}"))?;
        if child.status().is_some() {
            break;
        }
        if Instant::now() >= until {
            return Err("probe_timeout".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !child.status().is_some_and(|s| s.success()) {
        return Err("sandbox_probe_failed".into());
    }
    let probe_cleanup = child.close().map_err(|e| format!("probe_cleanup:{e:?}"))?;
    if read(&file, 64)? != b"unchanged" {
        return Err("probe_modified".into());
    }
    let mut sleep = private_child_command(Path::new("/bin/sleep"), root);
    sleep.arg("30");
    let mut child = OwnedChild::spawn(&mut sleep).map_err(|e| format!("helper_spawn:{e:?}"))?;
    let observed = child
        .refresh()
        .map_err(|e| format!("helper_identity:{e:?}"))?;
    if observed != [child.pid()] {
        return Err("helper_unexpected_tree".into());
    }
    let helper_cleanup = child.close().map_err(|e| format!("helper_cleanup:{e:?}"))?;
    let result = json!({"mode":"rust-image-rehearsal-without-models","model_inference":false,
        "direct_and_symlink_writes_denied":true,"network_denied":true,"probe_cleanup":probe_cleanup,"helper_cleanup":helper_cleanup});
    record(root, "evidence.json", &result)?;
    Ok(result)
}

pub(super) async fn shared_empty() -> Result<(), String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| "inventory_client")?;
    let mut r = client
        .get("http://127.0.0.1:11434/api/ps")
        .send()
        .await
        .map_err(|_| "inventory_unavailable")?;
    if r.status() != reqwest::StatusCode::OK || r.content_length().is_some_and(|n| n > 65536) {
        return Err("inventory_response".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = r.chunk().await.map_err(|_| "inventory_read")? {
        if bytes.len() + chunk.len() > 65536 {
            return Err("inventory_size".into());
        }
        bytes.extend(chunk);
    }
    let v: Value = serde_json::from_slice(&bytes).map_err(|_| "inventory_json")?;
    if !v["models"].as_array().is_some_and(Vec::is_empty) {
        return Err("LLM_resident_no_adoption_or_unload".into());
    }
    Ok(())
}

pub async fn image(root: &Path, binary: &Path, assets_dir: &Path) -> Result<Value, String> {
    private_root(root)?;
    for name in ["home", "tmp"] {
        fs::create_dir(root.join(name)).map_err(|_| "image_private_directory")?;
    }
    let lease = native::lease()?;
    let mut report = json!({"mode":"rust-one-real-local-image-evaluation","model_inference_requested":false,
        "response_validated":false,"memory_stop_enabled":false,"spec":{"prompt":PROMPT,"width":512,"height":512,"seed":42},"samples":[],"error":null});
    let mut child = None;
    let result:Result<(),String>=async {
        let selection=Selection::selected().map_err(str::to_string)?;
        for asset in &selection.assets{assets::verify(&assets_dir.join(&asset.local_name),asset)?;}
        if !binary.canonicalize().map_err(|_|"binary_path")?.starts_with(native::BASE) || !assets_dir.canonicalize().map_err(|_|"assets_path")?.starts_with(native::BASE){return Err("private_runtime_and_assets_required".into());}
        let build_root=binary.parent().and_then(Path::parent).and_then(Path::parent).ok_or("binary_build_root")?;
        let provenance:Value=serde_json::from_slice(&read(&build_root.join("build-evidence.json"),65536)?).map_err(|_|"binary_provenance_json")?;
        let (_,hash)=assets::hash_reader(File::open(binary).map_err(|_|"binary_open")?,100*1024*1024)?;
        if hash!="171c1d1f00abac1447f526715899dd59b93c70662cad7a7c8b973a1d4647576d" || provenance["binary_sha256"].as_str()!=Some(hash.as_str()) || provenance["exit_code"]!=0{return Err("runtime_binary_provenance_changed".into());}
        report["binary_sha256"]=json!(hash);
        ImageRequest {prompt:PROMPT.into(),width:512,height:512,seed:42}.validate().map_err(str::to_string)?;
        shared_empty().await?;
        if native::disk_free(root)?<4*1024u64.pow(3){return Err("image_preflight_disk_margin".into());}
        report["preflight"]=native::memory();
        let profile=root.join("image.sb");fs::write(&profile,native::child_profile(&assets_dir.canonicalize().map_err(|_|"assets_path")?)?).map_err(|_|"image_profile")?;
        let output=root.join("image.png");let log_path=root.join("runtime.log");
        let log=OpenOptions::new().write(true).create_new(true).open(&log_path).map_err(|_|"image_log")?;
        let mut command=private_child_command(Path::new("/usr/bin/sandbox-exec"),root);
        command.arg("-f").arg(profile).args(argv(binary,assets_dir,&output)).stdout(log.try_clone().map_err(|_|"image_log_clone")?).stderr(log);
        report["model_inference_requested"]=json!(true);let started=Instant::now();
        child=Some(OwnedChild::spawn(&mut command).map_err(|e|format!("image_spawn:{e:?}"))?);
        let mut term=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).map_err(|_|"image_signal_handler")?;
        let monitor=async {
            loop {
                let c=child.as_mut().ok_or("image_child_missing")?;
                c.refresh().map_err(|e|format!("image_ownership:{e:?}"))?;
                let mut sample=native::memory();sample["elapsed_seconds"]=json!(started.elapsed().as_secs_f64());
                let free=native::disk_free(root)?;sample["free_disk_bytes"]=json!(free);
                sample["rss_bytes_by_pid_not_additive"]=serde_json::to_value(c.rss().map_err(|e|format!("image_rss:{e:?}"))?).map_err(|_|"image_rss_json")?;
                report["samples"].as_array_mut().ok_or("image_evidence_state")?.push(sample);
                limits(started.elapsed(),free,fs::metadata(&log_path).map_err(|_|"image_log_metadata")?.len())?;
                if let Some(status)=c.status(){if !status.success(){return Err(format!("image_runtime_exit:{status}"));}return Ok::<(),String>(());}
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        };
        tokio::select! {
            r=tokio::time::timeout(Duration::from_secs(600),monitor)=>r.map_err(|_|"image_600_second_deadline")??,
            _=tokio::signal::ctrl_c()=>return Err("image_cancelled".into()),
            _=term.recv()=>return Err("image_cancelled".into()),
        }
        report["elapsed_seconds"]=json!(started.elapsed().as_secs_f64());
        let png=read(&output,MAX_PNG_BYTES as u64)?;
        report["artifact"]=serde_json::to_value(artifact::decode(&png,512,512).map_err(str::to_string)?).map_err(|_|"image_artifact_json")?;
        report["response_validated"]=json!(true);Ok(())
    }.await;
    if let Err(error) = result {
        report["error"] = json!(error);
    }
    if let Some(c) = child.as_mut() {
        match c.close() {
            Ok(clean) => {
                report["cleanup"] = serde_json::to_value(clean).map_err(|_| "cleanup_json")?
            }
            Err(e) => {
                report["cleanup_uncertain"] = json!(format!("{e:?}"));
                report["error"] = json!("image_cleanup_unconfirmed");
                record(
                    &Path::new(native::BASE).join("seiya-host-model-coordination"),
                    "image-cleanup-uncertain.json",
                    &json!({"evidence_directory":root,"pid":c.pid(),"error":format!("{e:?}")}),
                )?;
            }
        }
    }
    report["memory_after_cleanup"] = native::memory();
    record(root, "evidence.json", &report)?;
    drop(child);
    drop(lease);
    let reacquired = native::lease()?;
    drop(reacquired);
    if !report["error"].is_null() {
        return Err(format!("image_attempt_failed_evidence:{}", root.display()));
    }
    Ok(report)
}

/// Revalidate the existing synthetic result after parser changes. No inference,
/// mutation, retry, adoption of external artifacts or evidence overwrite.
pub fn validate_existing(root: &Path) -> Result<Value, String> {
    if !root
        .canonicalize()
        .map_err(|_| "evidence_path")?
        .starts_with(native::BASE)
    {
        return Err("private_evidence_required".into());
    }
    let evidence: Value = serde_json::from_slice(&read(&root.join("evidence.json"), 1024 * 1024)?)
        .map_err(|_| "evidence_json")?;
    if evidence["mode"] != "rust-one-real-local-image-evaluation"
        || evidence["response_validated"] != true
        || !evidence["error"].is_null()
        || evidence["cleanup"]["tree_stopped"] != true
        || evidence["cleanup"]["group_empty"] != true
        || evidence["cleanup"]["exit_code"] != 0
        || evidence["spec"] != json!({"prompt":PROMPT,"width":512,"height":512,"seed":42})
    {
        return Err("completed_owned_synthetic_evidence_required".into());
    }
    let decoded = artifact::decode(
        &read(&root.join("image.png"), MAX_PNG_BYTES as u64)?,
        512,
        512,
    )
    .map_err(str::to_string)?;
    let actual = serde_json::to_value(decoded).map_err(|_| "artifact_json")?;
    if evidence["artifact"] != actual {
        return Err("artifact_evidence_mismatch".into());
    }
    Ok(actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deadline_disk_and_log_fail_closed_without_memory_policy_override() {
        let ample = 4 * 1024u64.pow(3);
        assert!(limits(Duration::from_secs(599), ample, 4 * 1024 * 1024).is_ok());
        assert!(limits(Duration::from_secs(600), ample, 0).is_err());
        assert!(limits(Duration::ZERO, 1024, 0).is_err());
        assert!(limits(Duration::ZERO, ample, 4 * 1024 * 1024 + 1).is_err());
    }
    #[tokio::test]
    async fn async_deadline_interrupts_a_nonresponsive_runtime_mock() {
        assert!(
            tokio::time::timeout(Duration::from_millis(5), std::future::pending::<()>())
                .await
                .is_err()
        );
    }
    #[test]
    fn arguments_are_fixed_one_image_no_shell_or_alternative_model() {
        let a = argv(
            Path::new("/private/sd-cli"),
            Path::new("/private/assets"),
            Path::new("/private/image.png"),
        );
        let words: Vec<_> = a.iter().map(|s| s.to_str().unwrap()).collect();
        for (flag, value) in [
            ("--steps", "8"),
            ("--batch-count", "1"),
            ("-W", "512"),
            ("-H", "512"),
            ("--sigmas", "qwen-image-2.1-turbo"),
        ] {
            assert_eq!(
                words[words.iter().position(|s| *s == flag).unwrap() + 1],
                value
            );
        }
        assert!(words.contains(&"--disable-image-metadata"));
        assert!(!words.contains(&"sh"));
    }
}
