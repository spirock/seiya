//! Controlled web-job boundary. No infer/load on construction or denied permit.
use super::coordination::{self, Coordination, ModelLease};
use super::{
    artifact,
    assets::{Asset, Selection, CHUNK},
    native, runner,
};
use crate::probes::OwnedChild;
use seiya_contracts::image_job::{ImageRequest, ImageStage, MAX_PNG_BYTES};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;

#[derive(Clone)]
pub struct RuntimeFiles {
    pub binary: PathBuf,
    pub assets: PathBuf,
}
#[derive(Clone)]
pub struct Environment {
    work_parent: PathBuf,
    coordination: Coordination,
    portable: bool,
}
impl Environment {
    pub fn is_portable(&self) -> bool {
        self.portable
    }
    pub fn legacy() -> Self {
        Self {
            work_parent: native::BASE.into(),
            coordination: Coordination::legacy(),
            portable: false,
        }
    }
    pub fn portable(work_parent: PathBuf, coordination: Coordination) -> Result<Self, String> {
        coordination::private_directory(&work_parent)?;
        Ok(Self {
            work_parent,
            coordination,
            portable: true,
        })
    }
    pub fn lease(&self) -> Result<ModelLease, String> {
        self.coordination.acquire()
    }
    pub fn require_new(&self, path: &Path) -> Result<(), String> {
        if self.portable {
            coordination::require_new_in(path, &self.work_parent)
        } else {
            native::require_private_new(path)
        }
    }
}
pub trait Permission: Send + Sync + 'static {
    fn enabled(&self) -> bool {
        false
    }
    fn allows(&self, request: &ImageRequest) -> bool;
    fn budget(&self) -> Budget {
        Budget::default()
    }
    fn memory_blocked(&self, _: &Value) {}
}
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub max_swap_growth_mib: u64,
    pub deadline_seconds: u64,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            max_swap_growth_mib: 512,
            deadline_seconds: 600,
        }
    }
}
impl Budget {
    pub fn validate(self) -> Result<Self, Failure> {
        if !(64..=512).contains(&self.max_swap_growth_mib)
            || !(30..=600).contains(&self.deadline_seconds)
        {
            Err(Failure::Blocked)
        } else {
            Ok(self)
        }
    }
}
pub struct Denied;
impl Permission for Denied {
    fn allows(&self, _: &ImageRequest) -> bool {
        false
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Blocked,
    Cancelled,
    Failed,
    Uncertain,
}
#[derive(Clone)]
struct Control {
    budget: Budget,
    request: ImageRequest,
    permission: Arc<dyn Permission>,
    cancel: watch::Receiver<bool>,
    stop: Arc<AtomicBool>,
    deadline: Instant,
}
impl Control {
    fn memory_allowed(&self, m: &Value, baseline: u64) -> bool {
        let allowed = safe_memory(m, baseline)
            && m["swap_bytes"].as_u64().is_some_and(|n| {
                n.saturating_sub(baseline) <= self.budget.max_swap_growth_mib * 1024 * 1024
            });
        if !allowed {
            self.permission.memory_blocked(m);
        }
        allowed
    }
    fn check(&self) -> Result<(), Failure> {
        if self.stop.load(Ordering::Acquire) || *self.cancel.borrow() {
            return Err(Failure::Cancelled);
        }
        if !self.permission.enabled() || !self.permission.allows(&self.request) {
            return Err(Failure::Blocked);
        }
        if Instant::now() >= self.deadline {
            return Err(Failure::Failed);
        }
        Ok(())
    }
}
struct Lifetime(Arc<AtomicBool>);
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
fn read(path: &Path, cap: u64) -> Result<Vec<u8>, Failure> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| Failure::Failed)?;
    let m = f.metadata().map_err(|_| Failure::Failed)?;
    if !m.is_file() || m.len() > cap {
        return Err(Failure::Failed);
    }
    let mut bytes = Vec::new();
    f.take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::Failed)?;
    if bytes.len() as u64 > cap {
        return Err(Failure::Failed);
    }
    Ok(bytes)
}
fn record(root: &Path, name: &str, value: &Value) -> Result<(), Failure> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join(name))
        .map_err(|_| Failure::Failed)?;
    f.write_all(
        serde_json::to_string_pretty(value)
            .map_err(|_| Failure::Failed)?
            .as_bytes(),
    )
    .map_err(|_| Failure::Failed)?;
    f.sync_all().map_err(|_| Failure::Failed)
}
fn verify_asset(path: &Path, asset: &Asset, control: &Control) -> Result<(), Failure> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| Failure::Blocked)?;
    let m = file.metadata().map_err(|_| Failure::Blocked)?;
    if !m.is_file() || m.len() != asset.bytes {
        return Err(Failure::Blocked);
    }
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0u64;
    let mut hash = Sha256::new();
    loop {
        control.check()?;
        let n = file.read(&mut buf).map_err(|_| Failure::Blocked)?;
        if n == 0 {
            break;
        }
        total = total.checked_add(n as u64).ok_or(Failure::Blocked)?;
        if total > asset.bytes {
            return Err(Failure::Blocked);
        }
        hash.update(&buf[..n]);
    }
    if total != asset.bytes || format!("{:x}", hash.finalize()) != asset.sha256 {
        return Err(Failure::Blocked);
    }
    Ok(())
}
fn safe_memory(m: &Value, baseline: u64) -> bool {
    m["pressure"] == 1
        && m["swap_bytes"]
            .as_u64()
            .is_some_and(|s| s.saturating_sub(baseline) <= 512 * 1024 * 1024)
}
struct Prepared {
    lease: ModelLease,
    files: RuntimeFiles,
    baseline: u64,
}
fn prepare(
    files: RuntimeFiles,
    control: &Control,
    environment: &Environment,
) -> Result<Prepared, Failure> {
    control.check()?;
    let lease = environment.lease().map_err(|_| Failure::Blocked)?;
    let binary = files.binary.canonicalize().map_err(|_| Failure::Blocked)?;
    let assets = files.assets.canonicalize().map_err(|_| Failure::Blocked)?;
    if !environment.portable
        && (!binary.starts_with(native::BASE) || !assets.starts_with(native::BASE))
    {
        return Err(Failure::Blocked);
    }
    if environment.portable && (binary != files.binary || assets != files.assets) {
        return Err(Failure::Blocked);
    }
    let memory = native::memory();
    let baseline = memory["swap_bytes"].as_u64().ok_or(Failure::Blocked)?;
    if !control.memory_allowed(&memory, baseline) {
        return Err(Failure::Blocked);
    }
    for asset in Selection::selected().map_err(|_| Failure::Blocked)?.assets {
        verify_asset(&assets.join(&asset.local_name), &asset, control)?;
    }
    control.check()?;
    let parent = binary
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or(Failure::Blocked)?;
    let provenance: Value =
        serde_json::from_slice(&read(&parent.join("build-evidence.json"), 65536)?)
            .map_err(|_| Failure::Blocked)?;
    let bytes = read(&binary, 100 * 1024 * 1024)?;
    let hash = format!("{:x}", Sha256::digest(bytes));
    if hash != "171c1d1f00abac1447f526715899dd59b93c70662cad7a7c8b973a1d4647576d"
        || provenance["binary_sha256"] != hash
        || provenance["exit_code"] != 0
    {
        return Err(Failure::Blocked);
    }
    Ok(Prepared {
        lease,
        files: RuntimeFiles { binary, assets },
        baseline,
    })
}
struct Guard {
    child: Option<OwnedChild>,
    lease: Option<ModelLease>,
    root: PathBuf,
    finalized: bool,
    spawn_uncertain: bool,
    cleanup: Value,
    intent: Option<super::coordination::NativeOperation>,
}
impl Guard {
    fn finish(&mut self) -> Result<(), Failure> {
        if self.finalized {
            return if self.cleanup["confirmed"] == true {
                Ok(())
            } else {
                Err(Failure::Uncertain)
            };
        }
        let result = match self.child.as_mut() {
            Some(c) => c
                .close()
                .map(|v| json!({"confirmed":true,"detail":v}))
                .map_err(|e| format!("{e:?}")),
            None if self.spawn_uncertain => Err("spawn_ownership_uncertain".into()),
            None => Ok(json!({"confirmed":true,"spawned":false})),
        };
        let result = result.and_then(|v| {
            if let Some(intent) = self.intent.take() {
                intent.confirm()?;
            }
            Ok(v)
        });
        self.finalized = true;
        match result {
            Ok(v) => {
                self.cleanup = v;
                Ok(())
            }
            Err(e) => {
                self.cleanup = json!({"confirmed":false,"error":format!("{e:?}")});
                let marker = json!({"evidence_directory":self.root,"pid":self.child.as_ref().map(OwnedChild::pid),"error":format!("{e:?}")});
                if self.lease.as_ref().is_none_or(|lease| {
                    lease
                        .quarantine("image-cleanup-uncertain.json", &marker)
                        .is_err()
                }) {
                    // Fail closed even when persistence fails: retain kernel flock
                    // for this process lifetime, never pretend a free resource.
                    if let Some(file) = self.lease.take() {
                        std::mem::forget(file);
                    }
                }
                Err(Failure::Uncertain)
            }
        }
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.finalized {
            let _ = self.finish();
        }
    }
}

/// Verify the pinned local files, exclusive lease, memory and empty shared LLM
/// inventory without loading a model or writing a generation directory.
pub async fn preflight(
    files: RuntimeFiles,
    permission: Arc<dyn Permission>,
) -> Result<(), Failure> {
    preflight_with(files, permission, Environment::legacy()).await
}
pub async fn preflight_with(
    files: RuntimeFiles,
    permission: Arc<dyn Permission>,
    environment: Environment,
) -> Result<(), Failure> {
    let budget = permission.budget().validate()?;
    let (_tx, cancel) = watch::channel(false);
    let control = Control {
        budget,
        request: ImageRequest {
            prompt: "preflight-no-generation".into(),
            width: 512,
            height: 512,
            seed: 42,
        },
        permission,
        cancel,
        stop: Arc::new(AtomicBool::new(false)),
        deadline: Instant::now() + Duration::from_secs(budget.deadline_seconds),
    };
    let ctl = control.clone();
    let env = environment.clone();
    let prepared = tokio::task::spawn_blocking(move || prepare(files, &ctl, &env))
        .await
        .map_err(|_| Failure::Uncertain)??;
    runner::shared_empty().await.map_err(|_| Failure::Blocked)?;
    if !control.memory_allowed(&native::memory(), prepared.baseline)
        || native::disk_free(&environment.work_parent).map_err(|_| Failure::Blocked)?
            < 4 * 1024u64.pow(3)
    {
        return Err(Failure::Blocked);
    }
    Ok(())
}

pub async fn generate(
    files: RuntimeFiles,
    request: ImageRequest,
    permission: Arc<dyn Permission>,
    cancel: watch::Receiver<bool>,
    stage: watch::Sender<ImageStage>,
) -> Result<Vec<u8>, Failure> {
    generate_with(
        files,
        request,
        permission,
        cancel,
        stage,
        Environment::legacy(),
    )
    .await
}
pub async fn generate_with(
    files: RuntimeFiles,
    request: ImageRequest,
    permission: Arc<dyn Permission>,
    cancel: watch::Receiver<bool>,
    stage: watch::Sender<ImageStage>,
    environment: Environment,
) -> Result<Vec<u8>, Failure> {
    request.validate().map_err(|_| Failure::Failed)?;
    if request.width != 512 || request.height != 512 {
        return Err(Failure::Failed);
    }
    let budget = permission.budget().validate()?;
    let stop = Arc::new(AtomicBool::new(false));
    let _lifetime = Lifetime(stop.clone());
    let control = Control {
        budget,
        request: request.clone(),
        permission,
        cancel: cancel.clone(),
        stop,
        deadline: Instant::now() + Duration::from_secs(budget.deadline_seconds),
    };
    control.check()?;
    stage.send_replace(ImageStage::Preparing);
    let root = environment
        .work_parent
        .join(format!("seiya-web-image-{}", uuid::Uuid::new_v4()));
    environment
        .require_new(&root)
        .map_err(|_| Failure::Blocked)?;
    fs::create_dir(&root).map_err(|_| Failure::Blocked)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).map_err(|_| Failure::Blocked)?;
    let ctl = control.clone();
    let env = environment.clone();
    let prepared = tokio::task::spawn_blocking(move || prepare(files, &ctl, &env))
        .await
        .map_err(|_| Failure::Uncertain)??;
    let mut guard = Guard {
        child: None,
        lease: Some(prepared.lease),
        root: root.clone(),
        finalized: false,
        spawn_uncertain: false,
        cleanup: Value::Null,
        intent: None,
    };
    let mut report = json!({"mode":"controlled-web-image-job","width":request.width,"height":request.height,"seed":request.seed,"prompt_bytes":request.prompt.len(),"model_inference_requested":false,"memory_stop_enabled":true,"samples":[],"error":null});
    report["limits"] = json!({"swap_growth_mib":budget.max_swap_growth_mib,"deadline_seconds":budget.deadline_seconds,"warning_allowed":false,"critical_stop":true});
    let mut result=async{
        control.check()?;runner::shared_empty().await.map_err(|_|Failure::Blocked)?;
        if !control.memory_allowed(&native::memory(),prepared.baseline)||native::disk_free(&root).map_err(|_|Failure::Blocked)?<4*1024u64.pow(3){return Err(Failure::Blocked);}
        for name in ["home","tmp"]{fs::create_dir(root.join(name)).map_err(|_|Failure::Failed)?;}
        let profile=root.join("image.sb");fs::write(&profile,native::child_profile(&prepared.files.assets).map_err(|_|Failure::Failed)?).map_err(|_|Failure::Failed)?;
        let output=root.join("image.png");let log_path=root.join("runtime.log");let log=OpenOptions::new().write(true).create_new(true).mode(0o600).open(&log_path).map_err(|_|Failure::Failed)?;
        let mut command=Command::new("/usr/bin/sandbox-exec");command.arg("-f").arg(profile).args(runner::argv_for(&prepared.files.binary,&prepared.files.assets,&output,&request))
            .env_clear().env("PATH","/usr/bin:/bin:/usr/sbin:/sbin").env("LC_ALL","C").env("HOME",root.join("home")).env("TMPDIR",root.join("tmp")).current_dir(&root).stdin(Stdio::null()).stdout(log.try_clone().map_err(|_|Failure::Failed)?).stderr(log);
        control.check()?;guard.intent=Some(guard.lease.as_ref().ok_or(Failure::Uncertain)?.begin_native("image").map_err(|_|Failure::Blocked)?);report["model_inference_requested"]=json!(true);guard.spawn_uncertain=true;guard.child=Some(OwnedChild::spawn(&mut command).map_err(|_|Failure::Uncertain)?);guard.spawn_uncertain=false;stage.send_replace(ImageStage::Running);
        let mut cancel=cancel.clone();
        loop{
            control.check()?;let c=guard.child.as_mut().ok_or(Failure::Uncertain)?;c.refresh().map_err(|_|Failure::Uncertain)?;
            let memory=native::memory();report["samples"].as_array_mut().ok_or(Failure::Failed)?.push(memory.clone());
            if !control.memory_allowed(&memory,prepared.baseline)||native::disk_free(&root).map_err(|_|Failure::Failed)?<2*1024u64.pow(3)||fs::metadata(&log_path).map_err(|_|Failure::Failed)?.len()>4*1024*1024{return Err(Failure::Blocked);}
            if let Some(status)=c.status(){if !status.success(){return Err(Failure::Failed);}break;}
            tokio::select!{_=cancel.changed()=>return Err(Failure::Cancelled),_=tokio::time::sleep(Duration::from_millis(250))=>{}}
        }
        control.check()?;stage.send_replace(ImageStage::Validating);let png=read(&output,MAX_PNG_BYTES as u64)?;
        report["artifact"]=serde_json::to_value(artifact::decode(&png,request.width,request.height).map_err(|_|Failure::Failed)?).map_err(|_|Failure::Failed)?;Ok(png)
    }.await;
    let cleanup = guard.finish();
    if cleanup.is_ok() && result.is_ok() {
        if let Err(e) = control.check() {
            result = Err(e);
        }
    }
    report["cleanup"] = guard.cleanup.clone();
    report["error"] = json!(result.as_ref().err().map(|e| format!("{e:?}")));
    report["memory_after_cleanup"] = native::memory();
    let saved = record(&root, "evidence.json", &report);
    cleanup?;
    saved?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operator_budget_can_only_reduce_guard_and_is_a_turn_snapshot() {
        assert!(Budget {
            max_swap_growth_mib: 513,
            deadline_seconds: 600
        }
        .validate()
        .is_err());
        assert!(Budget {
            max_swap_growth_mib: 512,
            deadline_seconds: 601
        }
        .validate()
        .is_err());
        let mut c = control();
        c.budget = Budget {
            max_swap_growth_mib: 64,
            deadline_seconds: 60,
        }
        .validate()
        .unwrap();
        assert!(!c.memory_allowed(&json!({"pressure":1,"swap_bytes":65*1024*1024}), 0));
        assert!(safe_memory(
            &json!({"pressure":1,"swap_bytes":65*1024*1024}),
            0
        ));
        for p in [2, 4, 0] {
            assert!(!c.memory_allowed(&json!({"pressure":p,"swap_bytes":0}), 0));
        }
    }
    struct Yes;
    impl Permission for Yes {
        fn enabled(&self) -> bool {
            true
        }
        fn allows(&self, _: &ImageRequest) -> bool {
            true
        }
    }
    fn control() -> Control {
        let (_tx, cancel) = watch::channel(false);
        Control {
            budget: Budget::default(),
            request: ImageRequest {
                prompt: "fixture".into(),
                width: 512,
                height: 512,
                seed: 42,
            },
            permission: Arc::new(Yes),
            cancel,
            stop: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + Duration::from_secs(5),
        }
    }
    #[test]
    fn asset_hash_and_cancel_are_checked_between_bounded_blocks() {
        let root = std::env::temp_dir().join(format!(
            "seiya-image-asset-fixture-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("asset");
        fs::write(&path, b"abc").unwrap();
        let mut asset = Selection::selected().unwrap().assets.remove(0);
        asset.bytes = 3;
        asset.sha256 = format!("{:x}", Sha256::digest(b"abc"));
        let c = control();
        assert!(verify_asset(&path, &asset, &c).is_ok());
        c.stop.store(true, Ordering::Release);
        assert_eq!(verify_asset(&path, &asset, &c), Err(Failure::Cancelled));
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert_eq!(
            verify_asset(&alias, &asset, &control()),
            Err(Failure::Blocked)
        );
        asset.sha256 = "0".repeat(64);
        assert_eq!(
            verify_asset(&path, &asset, &control()),
            Err(Failure::Blocked)
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unconfirmed_spawn_persists_quarantine_without_touching_global_state() {
        let root = std::env::temp_dir().join(format!(
            "seiya-image-quarantine-fixture-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let coordination = Coordination::isolated_for_test(root.canonicalize().unwrap());
        let lease = coordination.acquire().unwrap();
        let intent = lease.begin_native("image").unwrap();
        let mut g = Guard {
            child: None,
            lease: Some(lease),
            root: root.clone(),
            finalized: false,
            spawn_uncertain: true,
            cleanup: Value::Null,
            intent: Some(intent),
        };
        assert_eq!(g.finish(), Err(Failure::Uncertain));
        assert!(root.join("image-cleanup-uncertain.json").is_file());
        assert!(root
            .join(super::super::coordination::NATIVE_INFLIGHT)
            .is_file());
        assert_eq!(g.cleanup["confirmed"], false);
        drop(g);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn helper_cleanup_retains_lease_until_guard_drop_without_global_runtime() {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("seiya-image-helper-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let coordination = Coordination::isolated_for_test(root.clone());
        let lease = coordination.acquire().unwrap();
        let intent = lease.begin_native("image").unwrap();
        let mut command = Command::new("/bin/sleep");
        command.arg("30");
        let child = OwnedChild::spawn(&mut command).unwrap();
        let mut guard = Guard {
            child: Some(child),
            lease: Some(lease),
            root: root.clone(),
            finalized: false,
            spawn_uncertain: false,
            cleanup: Value::Null,
            intent: Some(intent),
        };
        assert!(coordination.acquire().is_err());
        assert!(root
            .join(super::super::coordination::NATIVE_INFLIGHT)
            .exists());
        guard.finish().unwrap();
        assert!(!root
            .join(super::super::coordination::NATIVE_INFLIGHT)
            .exists());
        assert_eq!(guard.cleanup["confirmed"], true);
        assert!(coordination.acquire().is_err());
        drop(guard);
        assert!(coordination.acquire().is_ok());
        fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn denied_or_cancelled_request_never_reads_weights_or_starts_process() {
        let (cancel, rx) = watch::channel(false);
        let (stage, _) = watch::channel(ImageStage::Preparing);
        let r = ImageRequest {
            prompt: "synthetic".into(),
            width: 512,
            height: 512,
            seed: 42,
        };
        let files = RuntimeFiles {
            binary: "/not-read".into(),
            assets: "/not-read".into(),
        };
        assert_eq!(
            generate(
                files.clone(),
                r.clone(),
                Arc::new(Denied),
                rx.clone(),
                stage.clone()
            )
            .await
            .unwrap_err(),
            Failure::Blocked
        );
        cancel.send_replace(true);
        assert_eq!(
            generate(files, r, Arc::new(Denied), rx, stage)
                .await
                .unwrap_err(),
            Failure::Cancelled
        );
    }
    #[test]
    fn memory_guard_does_not_inherit_old_image_informational_exception() {
        assert!(safe_memory(&json!({"pressure":1,"swap_bytes":100}), 100));
        for p in [2, 4, 0] {
            assert!(!safe_memory(&json!({"pressure":p,"swap_bytes":100}), 100));
        }
        assert!(!safe_memory(
            &json!({"pressure":1,"swap_bytes":513*1024*1024}),
            0
        ));
    }
    #[test]
    fn prompt_is_an_isolated_argument_not_a_command_or_output_path() {
        let r = ImageRequest {
            prompt: "-o /outside; $(not-executed)\n--steps 999".into(),
            width: 512,
            height: 512,
            seed: 7,
        };
        let a = runner::argv_for(
            Path::new("/sd-cli"),
            Path::new("/assets"),
            Path::new("/private/image.png"),
            &r,
        );
        let words: Vec<_> = a.iter().map(|v| v.to_str().unwrap()).collect();
        assert_eq!(
            words[words.iter().position(|v| *v == "-p").unwrap() + 1],
            r.prompt
        );
        assert_eq!(words.last(), Some(&"/private/image.png"));
        assert_eq!(words.iter().filter(|v| **v == "--steps").count(), 1);
    }
}
