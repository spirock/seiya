//! Explicit temporary host: Gemma and images share one slot, never residency.
use crate::{
    image_jobs::{self, ImageJobs},
    pair_trial::{
        self,
        limits::{PressureWindow, TrialLimits},
        NativeChat,
    },
    prepared_web::{HostGate, PreparedWeb, AUXILIARY, FOREGROUND},
    runtime::{Backend, RuntimeConfig, RuntimeFuture, RuntimeTransport, TransportError},
};
use seiya_contracts::image_job::{ImageRequest, ImageStage};
use seiya_observe::{
    image::{
        coordination::{Coordination, ModelLease},
        job::{self, Environment, Permission, RuntimeFiles},
        native,
    },
    probes::OwnedChild,
};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};

pub const APPROVAL: &str = "temporary-real-gemma-and-images-sequential-web";
pub const GEMMA_SMOKE_APPROVAL: &str = "one-isolated-gemma-e2b-response-with-memory-guards";
pub const GEMMA_WEB_APPROVAL: &str = "temporary-manual-gemma-only-LAN-web";
pub const EVALUATION_APPROVAL: &str = "manual-local-images-and-multiagent-evaluation-web";
pub const PORTABLE_APPROVAL: &str = "manual-portable-macos-web-no-automatic-inference";
/// Single explicitly approved functional probe; never binds a web listener.
pub async fn gemma_smoke(root: &Path) -> Result<Value, String> {
    native::require_private_new(root)?;
    fs::create_dir(root).map_err(|_| "smoke_root")?;
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(|_| "smoke_mode")?;
    let mut report = json!({"mode":"one-isolated-gemma-e2b-functional-test","model":FOREGROUND,"inference_requested":false,"response_validated":false,"error":null,"rss_scope":"per owned process, sampled, not physical footprint or MLX allocator"});
    let result:Result<(),String>=async{
        let lease=native::lease()?;
        if !inventory(11434).await.map_err(|_|"shared_inventory_unavailable")?["models"].as_array().is_some_and(Vec::is_empty){return Err("shared_models_resident_no_adoption".into());}
        let mut window=Vec::new();for i in 0..9{if i>0{tokio::time::sleep(Duration::from_millis(250)).await;}window.push(native::memory());}
        let first=window[0]["swap_bytes"].as_u64().ok_or("swap_unavailable")?;
        let allowed=window.iter().all(|m|m["pressure"]==1&&m["swap_bytes"].as_u64().is_some_and(|v|v.saturating_sub(first)<=32*1024*1024))&&native::disk_free(root)? >=4*1024u64.pow(3);
        report["preflight"]=json!({"window":window,"normal_stable_swap_disk_required":true,"allowed":allowed});if !allowed{return Err("preflight_not_normal_stable_or_disk_low".into());}
        for p in ["models","models/blobs"]{fs::create_dir(root.join(p)).map_err(|_|"smoke_model_directory")?;}
        let source=Path::new(&std::env::var("HOME").map_err(|_|"smoke_home")?).join(".ollama").canonicalize().map_err(|_|"smoke_store")?;
        pair_trial::native::stage(&root.join("models"),&source,FOREGROUND,pair_trial::native::DIGESTS[0])?;
        drop(lease);
        let settings=crate::diagnostics::Diagnostics::memory(Arc::new(native::memory));report["limits"]=serde_json::to_value(settings.limits()).map_err(|_|"smoke_limits")?;
        let session=Arc::new(Session{feature_controlled:false,multiagent_configured:false,images_enabled:false,settings,until:Instant::now()+Duration::from_secs(180),enabled:AtomicBool::new(true),failed:AtomicBool::new(false),phase:AtomicU8::new(0),slot:Arc::new(Semaphore::new(1))});
        let chat=Chat{model:FOREGROUND,session:session.clone(),root:root.to_owned(),source,binary:pair_trial::native::OLLAMA.into(),environment:Environment::legacy()};
        let mut interrupt=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).map_err(|_|"smoke_signal")?;let mut term=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).map_err(|_|"smoke_signal")?;
        report["turn_attempted"]=json!(true);
        let response=tokio::select!{r=chat.turn(json!({"model":FOREGROUND,"messages":[{"role":"user","content":"Responde exactamente SEIYA_OK, sin explicaciones."}]}))=>r.map_err(|e|format!("gemma_turn:{e:?}")),_=interrupt.recv()=>Err("smoke_interrupted".into()),_=term.recv()=>Err("smoke_terminated".into()),_=tokio::time::sleep(Duration::from_secs(150))=>Err("smoke_outer_deadline".into())};
        session.stop();
        let response=response?;
        let valid=response["choices"][0]["message"]["content"].as_str().is_some_and(|s|s.trim()=="SEIYA_OK")&&response["choices"][0]["message"]["tool_calls"].as_array().is_none_or(Vec::is_empty);
        report["usage"]=response["usage"].clone();report["response_validated"]=json!(valid);if !valid{return Err("smoke_response_marker_invalid".into());}Ok(())
    }.await;
    report["error"] = json!(result.err());
    report["memory_after"] = native::memory();
    // The attempt marker is written immediately before the actual HTTP call by
    // Chat; distinguish admission failure from a requested model load.
    report["inference_requested"] = json!(fs::read_dir(root)
        .map_err(|_| "smoke_evidence_list")?
        .filter_map(Result::ok)
        .any(|e| e.path().is_dir()
            && e.file_name().to_string_lossy().starts_with("chat-")
            && e.path().join("attempt.json").is_file()));
    save(root, "result.json", &report)?;
    Ok(report)
}
fn save(root: &Path, name: &str, v: &Value) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(root.join(name))
        .map_err(|_| "host_evidence_open")?;
    f.write_all(
        serde_json::to_vec_pretty(v)
            .map_err(|_| "host_evidence_json")?
            .as_slice(),
    )
    .and_then(|_| f.sync_all())
    .map_err(|_| "host_evidence_write".into())
}
struct Session {
    feature_controlled: bool,
    multiagent_configured: bool,
    images_enabled: bool,
    settings: Arc<crate::diagnostics::Diagnostics>,
    until: Instant,
    enabled: AtomicBool,
    failed: AtomicBool,
    phase: AtomicU8,
    slot: Arc<Semaphore>,
}
impl Session {
    fn live(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
            && !self.settings.storage_failed()
            && !self.failed.load(Ordering::Acquire)
            && Instant::now() < self.until
    }
    fn stop(&self) {
        self.enabled.store(false, Ordering::Release);
    }
    fn fail(&self) {
        self.failed.store(true, Ordering::Release);
        self.slot.close();
    }
    async fn acquire(self: &Arc<Self>, phase: u8) -> Result<Turn, TransportError> {
        if !self.live() {
            return Err(TransportError::NotConnected);
        }
        let permit =
            tokio::time::timeout(Duration::from_secs(30), self.slot.clone().acquire_owned())
                .await
                .map_err(|_| TransportError::NotConnected)?
                .map_err(|_| TransportError::NotConnected)?;
        if !self.live() {
            return Err(TransportError::NotConnected);
        }
        self.phase.store(phase, Ordering::Release);
        Ok(Turn {
            session: self.clone(),
            _permit: permit,
            confirmed: false,
        })
    }
}
struct Turn {
    session: Arc<Session>,
    _permit: OwnedSemaphorePermit,
    confirmed: bool,
}
impl Turn {
    fn confirm(&mut self) {
        self.confirmed = true;
    }
}
impl Drop for Turn {
    fn drop(&mut self) {
        if !self.confirmed {
            self.session.fail();
        }
        self.session.phase.store(0, Ordering::Release);
    }
}
impl Permission for Session {
    fn budget(&self) -> job::Budget {
        let p = self.settings.limits();
        job::Budget {
            max_swap_growth_mib: p.image_swap_growth_mib,
            deadline_seconds: p.image_deadline_seconds,
        }
    }
    fn memory_blocked(&self, m: &Value) {
        let _ = self
            .settings
            .record_stop(crate::diagnostics::Stop::from_sample(
                crate::diagnostics::StopSource::Host,
                "memory_policy",
                &json!({"memory":m}),
            ));
    }
    fn enabled(&self) -> bool {
        self.live()
            && self.images_enabled
            && (!self.feature_controlled || self.settings.features().images)
    }
    fn allows(&self, _: &ImageRequest) -> bool {
        self.enabled()
    }
}
impl image_jobs::Admission for Session {
    fn allows_image(&self) -> bool {
        self.enabled() && native::memory()["pressure"] == 1
    }
}
impl HostGate for Session {
    fn multiagent_enabled(&self) -> bool {
        self.live() && self.multiagent_configured && self.settings.features().multiagent
    }
    fn admits(&self, f: &str, a: &str) -> bool {
        f == FOREGROUND
            && (a.is_empty() || a == AUXILIARY && self.multiagent_configured)
            && self.live()
            && self.phase.load(Ordering::Acquire) != 2
    }
    fn status(&self) -> Option<&'static str> {
        Some(if !self.live() {
            "Sesión finalizada o detenida; no se cargarán modelos."
        } else {
            match self.phase.load(Ordering::Acquire) {
                1 => "Gemma real: carga/consulta y descarga confirmada por turno.",
                2 => "Imagen local en curso; chat conservado, temporalmente ocupado.",
                3 => "Qwen prepara un borrador por turnos; no ejecuta código ni escribe archivos.",
                _ => {
                    if self.images_enabled {
                        "Chat Gemma e imágenes disponibles por turnos; Qwen apagado."
                    } else {
                        "Gemma real bajo demanda: escribe para cargarlo. Qwen e imágenes apagados."
                    }
                }
            }
        })
    }
}
struct Images {
    session: Arc<Session>,
    native: image_jobs::native::NativeImage,
}
impl image_jobs::Executor for Images {
    fn available(&self) -> bool {
        self.native.available()
    }
    fn generate(
        &self,
        r: ImageRequest,
        mut cancel: watch::Receiver<bool>,
        stage: watch::Sender<ImageStage>,
    ) -> image_jobs::Work {
        let session = self.session.clone();
        let work = self.native.generate(r, cancel.clone(), stage);
        Box::pin(async move {
            let mut turn = tokio::select! {t=session.acquire(2)=>t.map_err(|_|image_jobs::Failure::Blocked)?,_=cancel.changed()=>return Err(image_jobs::Failure::Cancelled)};
            if *cancel.borrow() {
                turn.confirm();
                return Err(image_jobs::Failure::Cancelled);
            }
            let result = work.await;
            if result == Err(image_jobs::Failure::Uncertain) {
                session.fail();
            }
            turn.confirm();
            drop(turn);
            result
        })
    }
}
struct Chat {
    model: &'static str,
    session: Arc<Session>,
    root: PathBuf,
    source: PathBuf,
    binary: PathBuf,
    environment: Environment,
}
struct ChatOwner {
    child: Option<OwnedChild>,
    lease: Option<ModelLease>,
    root: PathBuf,
    session: Arc<Session>,
    finished: bool,
    spawn_uncertain: bool,
}
impl ChatOwner {
    fn finish(&mut self) -> Result<(), TransportError> {
        if self.finished {
            return Err(TransportError::Unavailable);
        }
        self.finished = true;
        let result = if let Some(child) = &mut self.child {
            child
                .close()
                .map(|v| json!({"confirmed":true,"detail":v}))
                .map_err(|e| format!("{e:?}"))
        } else if self.spawn_uncertain {
            Err("spawn_uncertain".into())
        } else {
            Ok(json!({"confirmed":true,"spawned":false}))
        };
        match result {
            Ok(v) => {
                if save(&self.root, "cleanup.json", &v).is_err() {
                    self.session.fail();
                    return Err(TransportError::Unavailable);
                }
                Ok(())
            }
            Err(e) => {
                self.session.fail();
                let marker = json!({"pid":self.child.as_ref().map(OwnedChild::pid),"evidence":self.root,"error":e});
                if self.lease.as_ref().is_none_or(|lease| {
                    lease
                        .quarantine("pair-cleanup-uncertain.json", &marker)
                        .is_err()
                }) {
                    if let Some(f) = self.lease.take() {
                        std::mem::forget(f);
                    }
                }
                let _ = save(
                    &self.root,
                    "cleanup.json",
                    &json!({"confirmed":false,"error":e}),
                );
                Err(TransportError::Unavailable)
            }
        }
    }
}
impl Drop for ChatOwner {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish();
        }
    }
}
async fn inventory(port: u16) -> Result<Value, TransportError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| TransportError::NotConnected)?;
    let mut r = client
        .get(format!("http://127.0.0.1:{port}/api/ps"))
        .send()
        .await
        .map_err(|_| TransportError::NotConnected)?;
    if r.status() != reqwest::StatusCode::OK {
        return Err(TransportError::NotConnected);
    }
    let mut bytes = Vec::new();
    while let Some(c) = r.chunk().await.map_err(|_| TransportError::NotConnected)? {
        if bytes.len() + c.len() > 65536 {
            return Err(TransportError::NotConnected);
        }
        bytes.extend(c);
    }
    serde_json::from_slice(&bytes).map_err(|_| TransportError::NotConnected)
}
impl Chat {
    async fn turn(&self, input: Value) -> Result<Value, TransportError> {
        let mut turn = self
            .session
            .acquire(if self.model == AUXILIARY { 3 } else { 1 })
            .await?;
        let lease = self
            .environment
            .lease()
            .map_err(|_| TransportError::NotConnected)?;
        if self.environment.is_portable() {
            crate::desktop_config::executable(&self.binary, "llm_binary")
                .map_err(|_| TransportError::NotConnected)?;
        }
        if !inventory(11434).await?["models"]
            .as_array()
            .is_some_and(Vec::is_empty)
            || native::memory()["pressure"] != 1
        {
            return Err(TransportError::NotConnected);
        }
        let root = self.root.join(format!("chat-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).map_err(|_| TransportError::Unavailable)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .map_err(|_| TransportError::Unavailable)?;
        let mut owner = ChatOwner {
            child: None,
            lease: Some(lease),
            root: root.clone(),
            session: self.session.clone(),
            finished: false,
            spawn_uncertain: false,
        };
        let start = Instant::now();
        let baseline = native::memory()["swap_bytes"]
            .as_u64()
            .ok_or(TransportError::NotConnected)?;
        let policy = self.session.settings.limits();
        let deadline = Duration::from_secs(policy.chat_deadline_seconds);
        let limits = TrialLimits {
            warning_budget_ms: policy.chat_warning_ms,
            max_swap_growth_mib: policy.chat_swap_growth_mib,
            ..Default::default()
        };
        let mut samples = Vec::new();
        let result=async{
            for p in ["home","tmp"]{fs::create_dir(root.join(p)).map_err(|_|TransportError::Unavailable)?;}
            let reservation=std::net::TcpListener::bind("127.0.0.1:0").map_err(|_|TransportError::Unavailable)?;let port=reservation.local_addr().map_err(|_|TransportError::Unavailable)?.port();drop(reservation);
            let profile=format!("(version 1)(allow default)(deny file-write* (subpath {}))",serde_json::to_string(&self.source).map_err(|_|TransportError::Unavailable)?);
            fs::write(root.join("readonly.sb"),profile).map_err(|_|TransportError::Unavailable)?;
            let log_path=root.join("runtime.log");let log=OpenOptions::new().create_new(true).write(true).mode(0o600).open(&log_path).map_err(|_|TransportError::Unavailable)?;
            let mut command=Command::new("/usr/bin/sandbox-exec");command.arg("-f").arg(root.join("readonly.sb")).arg(&self.binary).arg("serve").env_clear()
                .env("PATH","/usr/bin:/bin:/usr/sbin:/sbin").env("LC_ALL","C").env("HOME",root.join("home")).env("TMPDIR",root.join("tmp"))
                .env("OLLAMA_MODELS",self.root.join("models")).env("OLLAMA_HOST",format!("127.0.0.1:{port}")).env("OLLAMA_NO_CLOUD","1").env("OLLAMA_NOPRUNE","1")
                .env("OLLAMA_MAX_LOADED_MODELS","1").env("OLLAMA_NUM_PARALLEL","1").env("OLLAMA_CONTEXT_LENGTH","4096").env("OLLAMA_KEEP_ALIVE","30m").env("OLLAMA_LOAD_TIMEOUT","120s").env("OLLAMA_DEBUG","0")
                .current_dir(&root).stdin(Stdio::null()).stdout(log.try_clone().map_err(|_|TransportError::Unavailable)?).stderr(log);
            if !self.session.live(){return Err(TransportError::NotConnected);}
            owner.spawn_uncertain=true;owner.child=Some(OwnedChild::spawn(&mut command).map_err(|_|TransportError::Unavailable)?);owner.spawn_uncertain=false;
            save(&root,"owner.json",&json!({"pid":owner.child.as_ref().unwrap().pid(),"port":port,"inference_requested":false})).map_err(|_|TransportError::Unavailable)?;
            let mut pressure=PressureWindow::new(0);
            loop{
                self.check(&mut owner,&mut pressure,&limits,deadline,start,baseline,&log_path,&mut samples)?;
                if owner.child.as_mut().unwrap().listening(port).map_err(|_|TransportError::Unavailable)?{break;}
                if start.elapsed()>Duration::from_secs(15){return Err(TransportError::Timeout);}
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            if !inventory(port).await?["models"].as_array().is_some_and(Vec::is_empty){return Err(TransportError::Unavailable);}
            let chat=NativeChat::new(port,self.model)?;
            save(&root,"attempt.json",&json!({"inference_requested":true,"model":self.model})).map_err(|_|TransportError::Unavailable)?;
            let work=chat.complete(input);tokio::pin!(work);
            let answer=loop{tokio::select!{
                r=&mut work=>break r,
                _=tokio::time::sleep(Duration::from_millis(250))=>self.check(&mut owner,&mut pressure,&limits,deadline,start,baseline,&log_path,&mut samples)?
            }};
            self.check(&mut owner,&mut pressure,&limits,deadline,start,baseline,&log_path,&mut samples)?;
            answer
        }.await;
        let cleanup = owner.finish();
        let saved = save(
            &root,
            "observations.json",
            &json!({"samples":samples,"response_received":result.is_ok(),"error":result.as_ref().err().map(|e|format!("{e:?}")),"usage":result.as_ref().ok().map(|v|&v["usage"])}),
        );
        cleanup?;
        turn.confirm();
        if saved.is_err() {
            self.session.fail();
            return Err(TransportError::Unavailable);
        }
        result
    }
    #[allow(clippy::too_many_arguments)]
    fn check(
        &self,
        owner: &mut ChatOwner,
        pressure: &mut PressureWindow,
        limits: &TrialLimits,
        deadline: Duration,
        start: Instant,
        baseline: u64,
        log: &Path,
        samples: &mut Vec<Value>,
    ) -> Result<(), TransportError> {
        let mut memory = native::memory();
        memory["memory_stop_enabled"] = json!(true);
        let memory_allowed = pressure.observe(
            start.elapsed().as_millis() as u64,
            &memory,
            baseline,
            limits,
        );
        let disk_allowed = native::disk_free(&owner.root).is_ok_and(|n| n >= 2 * 1024u64.pow(3));
        let log_allowed = fs::metadata(log).is_ok_and(|m| m.len() <= 4 * 1024 * 1024);
        let reason = stop_reason(
            self.session.live(),
            memory_allowed,
            start.elapsed(),
            disk_allowed,
            log_allowed,
            deadline,
        );
        samples.push(json!({"elapsed_ms":start.elapsed().as_millis() as u64,"memory":memory,"memory_allowed":memory_allowed,"warning_used_ms":pressure.warning_used_ms,"baseline_swap_bytes":baseline,"stop_reason":reason}));
        if let Some(sample) = samples.last_mut() {
            sample["owned_rss_bytes_by_pid"] = owner
                .child
                .as_ref()
                .and_then(|c| c.rss().ok())
                .map_or(Value::Null, |rss| json!(rss));
        }
        if let Some(reason) = reason {
            let _ = save(
                &owner.root,
                "stop.json",
                &json!({"reason":reason,"last_sample":samples.last()}),
            );
            if let Some(sample) = samples.last() {
                let _ = self
                    .session
                    .settings
                    .record_stop(crate::diagnostics::Stop::from_sample(
                        crate::diagnostics::StopSource::Host,
                        reason,
                        sample,
                    ));
            }
            self.session.fail();
            return Err(TransportError::Unavailable);
        }
        let child = owner.child.as_mut().ok_or(TransportError::Unavailable)?;
        if child.refresh().is_err() || child.status().is_some() {
            if let Some(sample) = samples.last() {
                let _ = self
                    .session
                    .settings
                    .record_stop(crate::diagnostics::Stop::from_sample(
                        crate::diagnostics::StopSource::Host,
                        "runtime_ownership",
                        sample,
                    ));
            }
            self.session.fail();
            return Err(TransportError::Unavailable);
        }
        Ok(())
    }
}
fn stop_reason(
    live: bool,
    memory: bool,
    elapsed: Duration,
    disk: bool,
    log: bool,
    deadline: Duration,
) -> Option<&'static str> {
    if !live {
        Some("session_revoked_or_expired")
    } else if !memory {
        Some("memory_pressure_swap_or_stale_observation")
    } else if elapsed >= deadline {
        Some("turn_deadline")
    } else if !disk {
        Some("disk_reserve")
    } else if !log {
        Some("runtime_log_limit_or_unavailable")
    } else {
        None
    }
}
impl RuntimeTransport for Chat {
    fn complete(&self, input: Value) -> RuntimeFuture<'_> {
        Box::pin(self.turn(input))
    }
}

/// TLS-only LAN listener; source addresses outside the explicitly selected LAN
/// subnet are rejected before the handshake. No wildcard or port forwarding.
pub struct LanConfig {
    pub history: PathBuf,
    pub address: SocketAddr,
    pub prefix: u8,
    pub certificate: PathBuf,
    pub private_key: PathBuf,
}
fn same_subnet(host: Ipv4Addr, peer: IpAddr, prefix: u8) -> bool {
    if !(8..=30).contains(&prefix) {
        return false;
    }
    let IpAddr::V4(peer) = peer else {
        return false;
    };
    let mask = u32::MAX << (32 - prefix);
    u32::from(host) & mask == u32::from(peer) & mask
}
pub(crate) struct TlsListener {
    pub(crate) tcp: tokio::net::TcpListener,
    pub(crate) acceptor: tokio_rustls::TlsAcceptor,
    pub(crate) host: Ipv4Addr,
    pub(crate) prefix: u8,
}
impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.tcp.accept().await {
                Ok((stream, peer)) => {
                    if !same_subnet(self.host, peer.ip(), self.prefix) {
                        continue;
                    }
                    if let Ok(Ok(tls)) =
                        tokio::time::timeout(Duration::from_secs(3), self.acceptor.accept(stream))
                            .await
                    {
                        return (tls, peer);
                    }
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }
    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.tcp.local_addr()
    }
}
pub(crate) fn tls(config: &LanConfig) -> Result<tokio_rustls::TlsAcceptor, String> {
    use tokio_rustls::rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer},
    };
    fn bounded(path: &Path, key: bool) -> Result<Vec<u8>, String> {
        use std::io::Read;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(0x100 /* O_NOFOLLOW macOS */)
            .open(path)
            .map_err(|_| "tls_file_open")?;
        let m = file.metadata().map_err(|_| "tls_file_metadata")?;
        if !m.is_file() || m.len() > 65536 || key && m.permissions().mode() & 0o077 != 0 {
            return Err("tls_regular_bounded_private_files_required".into());
        }
        let mut v = Vec::new();
        file.take(65537)
            .read_to_end(&mut v)
            .map_err(|_| "tls_file_read")?;
        if v.len() > 65536 {
            return Err("tls_file_size".into());
        }
        Ok(v)
    }
    let certificate = CertificateDer::from(bounded(&config.certificate, false)?);
    let key =
        PrivateKeyDer::try_from(bounded(&config.private_key, true)?).map_err(|_| "tls_key_der")?;
    let server = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|_| "tls_versions")?
    .with_no_client_auth()
    .with_single_cert(vec![certificate], key)
    .map_err(|_| "tls_certificate_key")?;
    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(server)))
}

pub async fn run(root: &Path, config: LanConfig) -> Result<(), String> {
    run_mode(root, config, false, false, None, None).await
}
pub async fn gemma_web(root: &Path, config: LanConfig) -> Result<(), String> {
    run_mode(root, config, true, false, None, None).await
}
pub async fn evaluation_web(root: &Path, config: LanConfig) -> Result<(), String> {
    run_mode(root, config, false, true, None, None).await
}
struct PortableEnvironment {
    environment: Environment,
    config: crate::desktop_config::Config,
}
fn optional_auxiliary(store: &Path, requested: bool) -> Result<bool, String> {
    let manifest = store.join("manifests-v2/ollama.com/library/qwen3.5/9b-mlx");
    match fs::symlink_metadata(manifest) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if requested {
                Err("configured_auxiliary_model_required".into())
            } else {
                Ok(false)
            }
        }
        Err(_) => Err("auxiliary_manifest_observation_failed".into()),
    }
}
/// No warmup or automatic chat. Caller must supply explicit LAN/TLS approval.
pub async fn portable(
    config_path: &Path,
    mut lan: LanConfig,
    chat_only: bool,
) -> Result<(), String> {
    let config = crate::desktop_config::load_file(config_path)
        .map_err(|e| format!("desktop_config:{}:{}", e.field, e.code))?;
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("portable_home_required")?)
        .canonicalize()
        .map_err(|_| "portable_home_required")?;
    let coordination = Coordination::portable(&home, &config.coordination_dir)?;
    let environment = Environment::portable(config.data_dir.clone(), coordination)?;
    lan.history = config.data_dir.clone();
    let root = config
        .data_dir
        .join(format!("run-{}", uuid::Uuid::new_v4()));
    run_mode(
        &root,
        lan,
        chat_only,
        !chat_only,
        Some(PortableEnvironment {
            environment,
            config,
        }),
        None,
    )
    .await
}
pub(crate) struct DesktopControl {
    pub ready: tokio::sync::oneshot::Sender<crate::launcher::Ready>,
    pub stop: watch::Receiver<bool>,
}
pub(crate) async fn desktop(
    profile: &[u8],
    chat_only: bool,
    control: DesktopControl,
) -> Result<(), String> {
    let config =
        crate::desktop_config::check_bytes(profile).map_err(|_| "desktop_profile_invalid")?;
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("desktop_home_required")?)
        .canonicalize()
        .map_err(|_| "desktop_home_required")?;
    let environment = Environment::portable(
        config.data_dir.clone(),
        Coordination::portable(&home, &config.coordination_dir)?,
    )?;
    let root = config
        .data_dir
        .join(format!("run-{}", uuid::Uuid::new_v4()));
    let lan = LanConfig {
        history: config.data_dir.clone(),
        address: "127.0.0.1:8015".parse().unwrap(),
        prefix: 32,
        certificate: PathBuf::new(),
        private_key: PathBuf::new(),
    };
    run_mode(
        &root,
        lan,
        chat_only,
        !chat_only,
        Some(PortableEnvironment {
            environment,
            config,
        }),
        Some(control),
    )
    .await
}
struct StopOnDrop(Arc<Session>);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.stop();
    }
}
struct SequentialDraft {
    chat: Arc<Chat>,
}
impl crate::jobs::Executor for SequentialDraft {
    fn draft(&self, prompt: String, cancel: watch::Receiver<bool>) -> crate::jobs::Work {
        let chat = self.chat.clone();
        Box::pin(async move {
            if *cancel.borrow() {
                return Err(crate::jobs::Failure::Confirmed);
            }
            let r=chat.turn(json!({"model":AUXILIARY,"messages":[{"role":"system","content":"Return a short source-code draft as plain text. No tools, execution, file writes, tests or private reasoning."},{"role":"user","content":prompt}]})).await;
            let r = r.map_err(|_| {
                if chat.session.failed.load(Ordering::Acquire) {
                    crate::jobs::Failure::Uncertain
                } else {
                    crate::jobs::Failure::Confirmed
                }
            })?;
            if *cancel.borrow() {
                return Err(crate::jobs::Failure::Confirmed);
            }
            let text = r["choices"][0]["message"]["content"]
                .as_str()
                .filter(|s| !s.trim().is_empty() && s.len() <= 65536 && !s.contains("<think>"))
                .ok_or(crate::jobs::Failure::Confirmed)?;
            Ok(text.into())
        })
    }
}
async fn run_mode(
    root: &Path,
    config: LanConfig,
    gemma_only: bool,
    evaluation: bool,
    portable: Option<PortableEnvironment>,
    mut desktop: Option<DesktopControl>,
) -> Result<(), String> {
    let local = desktop.is_some();
    if desktop.as_ref().is_some_and(|d| *d.stop.borrow()) {
        return Err("desktop_start_cancelled".into());
    }
    let environment = portable
        .as_ref()
        .map(|p| p.environment.clone())
        .unwrap_or_else(Environment::legacy);
    let binary = portable
        .as_ref()
        .map(|p| p.config.llm_binary.clone())
        .unwrap_or_else(|| pair_trial::native::OLLAMA.into());
    let files = if let Some(p) = &portable {
        p.config.images.as_ref().map(|i| RuntimeFiles {
            binary: i.binary.clone(),
            assets: i.assets_dir.clone(),
        })
    } else {
        Some(RuntimeFiles {
            binary: Path::new(native::BASE).join("seiya-sdcpp-build-01/build/bin/sd-cli"),
            assets: Path::new(native::BASE).join("seiya-image21-turbo-q4-assets-01"),
        })
    };
    let images_configured = !gemma_only && files.is_some();
    let auxiliary_configured = if !evaluation {
        false
    } else if let Some(p) = &portable {
        optional_auxiliary(&p.config.model_store, p.config.features.multiagent)?
    } else {
        true
    };
    let IpAddr::V4(host) = config.address.ip() else {
        return Err("explicit_private_ipv4_required".into());
    };
    if (if local {
        host != Ipv4Addr::LOCALHOST || config.prefix != 32
    } else {
        !host.is_private() || !(8..=30).contains(&config.prefix)
    }) || config.address.port() != 8015
    {
        return Err("explicit_private_LAN_and_port8015_required".into());
    }
    let acceptor = if local { None } else { Some(tls(&config)?) };
    if !config.history.is_absolute()
        || config
            .history
            .canonicalize()
            .map_err(|_| "history_directory_required")?
            != config.history
        || fs::symlink_metadata(&config.history)
            .map_err(|_| "history_directory")?
            .permissions()
            .mode()
            & 0o077
            != 0
    {
        return Err("history_private_canonical_directory_required".into());
    }
    let _profile = portable
        .as_ref()
        .map(|p| seiya_observe::image::coordination::profile_lock(&p.config.data_dir))
        .transpose()?;
    environment.require_new(root)?;
    // Admission checks quarantine even when optional image preflight is disabled.
    if portable.is_some() {
        drop(environment.lease()?);
    }
    for name in ["drafts", "settings"]
        .into_iter()
        .chain(images_configured.then_some("images"))
    {
        let path = config.history.join(name);
        match fs::create_dir(&path) {
            Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|_| "history_mode")?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("history_subdirectory".into()),
        }
    }
    fs::create_dir(root).map_err(|_| "host_root")?;
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(|_| "host_root_mode")?;
    let settings = crate::diagnostics::Diagnostics::persistent(
        &config.history.join("settings"),
        Arc::new(native::memory),
    )?;
    if let Some(p) = &portable {
        settings
            .initialize_features(p.config.features)
            .map_err(|_| "portable_settings")?;
    } else if evaluation {
        settings
            .initialize_evaluation()
            .map_err(|_| "evaluation_settings")?;
    }
    let session = Arc::new(Session {
        feature_controlled: evaluation,
        multiagent_configured: auxiliary_configured,
        images_enabled: images_configured,
        until: Instant::now() + Duration::from_secs(settings.limits().session_seconds),
        settings: settings.clone(),
        enabled: AtomicBool::new(true),
        failed: AtomicBool::new(false),
        phase: AtomicU8::new(0),
        slot: Arc::new(Semaphore::new(1)),
    });
    let _stop_on_drop = StopOnDrop(session.clone());
    let preflight_lease = if gemma_only {
        let lease = environment.lease()?;
        if !inventory(11434)
            .await
            .map_err(|_| "gemma_web_shared_inventory")?["models"]
            .as_array()
            .is_some_and(Vec::is_empty)
        {
            return Err("shared_model_resident_no_adoption".into());
        }
        let mut baseline = None;
        for i in 0..9 {
            if i > 0 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            let m = native::memory();
            let swap = m["swap_bytes"].as_u64().ok_or("gemma_web_swap_unknown")?;
            let initial = *baseline.get_or_insert(swap);
            if m["pressure"] != 1 || swap.saturating_sub(initial) > 32 * 1024 * 1024 {
                return Err("gemma_web_preflight_not_normal_stable".into());
            }
        }
        if native::disk_free(root)? < 4 * 1024u64.pow(3) {
            return Err("gemma_web_disk_reserve".into());
        }
        Some(lease)
    } else {
        None
    };
    if images_configured && session.enabled() {
        let preflight = job::preflight_with(
            files.clone().ok_or("image_files_required")?,
            session.clone(),
            environment.clone(),
        );
        if let Some(d) = &mut desktop {
            tokio::select! {r=preflight=>r.map_err(|e|format!("image_preflight:{e:?}"))?,_=d.stop.changed()=>return Err("desktop_start_cancelled".into())};
        } else {
            preflight
                .await
                .map_err(|e| format!("image_preflight:{e:?}"))?;
        }
    }
    fs::create_dir(root.join("models")).map_err(|_| "host_models")?;
    fs::create_dir(root.join("models/blobs")).map_err(|_| "host_blobs")?;
    let source = if let Some(p) = &portable {
        p.config.model_store.clone()
    } else {
        Path::new(&std::env::var("HOME").map_err(|_| "host_home")?)
            .join(".ollama")
            .canonicalize()
            .map_err(|_| "host_model_store")?
    };
    pair_trial::native::stage(
        &root.join("models"),
        &source,
        FOREGROUND,
        pair_trial::native::DIGESTS[0],
    )?;
    if auxiliary_configured {
        pair_trial::native::stage(
            &root.join("models"),
            &source,
            AUXILIARY,
            pair_trial::native::DIGESTS[1],
        )?;
    }
    drop(preflight_lease);
    let images = if !images_configured {
        None
    } else {
        Some(ImageJobs::persistent(
            Arc::new(Images {
                session: session.clone(),
                native: image_jobs::native::NativeImage::with_environment(
                    files.ok_or("image_files_required")?,
                    session.clone(),
                    environment.clone(),
                ),
            }),
            session.clone(),
            &config.history.join("images"),
        )?)
    };
    let auxiliary: Arc<dyn crate::jobs::Executor> = if auxiliary_configured {
        Arc::new(SequentialDraft {
            chat: Arc::new(Chat {
                model: AUXILIARY,
                session: session.clone(),
                root: root.to_owned(),
                source: source.clone(),
                binary: binary.clone(),
                environment: environment.clone(),
            }),
        })
    } else {
        Arc::new(NoDrafts)
    };
    let chat = Arc::new(Chat {
        model: FOREGROUND,
        session: session.clone(),
        root: root.to_owned(),
        source,
        binary,
        environment,
    });
    struct NoDrafts;
    impl image_jobs::Admission for NoDrafts {
        fn allows_image(&self) -> bool {
            false
        }
    }
    impl crate::jobs::Executor for NoDrafts {
        fn draft(&self, _: String, _: watch::Receiver<bool>) -> crate::jobs::Work {
            Box::pin(async { Err(crate::jobs::Failure::Confirmed) })
        }
    }
    let state = PreparedWeb::with_history(
        RuntimeConfig {
            id: "gemma-images-sequential".into(),
            model: FOREGROUND.into(),
            backend: Backend::Mlx,
            base_url: "http://127.0.0.1:49399/v1".into(),
            timeout_seconds: 120,
            max_output_tokens: 256,
        },
        chat,
        auxiliary,
        session.clone(),
        !auxiliary_configured,
        &config.history.join("drafts"),
    )
    .map_err(|_| "host_state")?;
    let state = if let Some(images) = &images {
        state
            .with_images(images.clone())
            .map_err(|_| "host_images")?
    } else {
        state
    };
    let state = state
        .with_diagnostics(settings)
        .map_err(|_| "host_diagnostics")?;
    let state = if evaluation {
        state
            .with_feature_settings()
            .map_err(|_| "host_feature_settings")?
    } else {
        state
    };
    let tcp = tokio::net::TcpListener::bind(config.address)
        .await
        .map_err(|_| "host_port_busy_no_adoption")?;
    let address = tcp.local_addr().map_err(|_| "host_address")?;
    let (app, link, probe) = if local {
        let (a, l) =
            pair_trial::access::app(address, state.clone()).map_err(|_| "desktop_access")?;
        (a, l, None)
    } else if gemma_only || evaluation {
        let (a, l, p) = pair_trial::access::lan_tls_probe_app(address, state.clone())
            .map_err(|_| "host_access")?;
        (a, l, Some(p))
    } else {
        let (a, l) =
            pair_trial::access::lan_tls_app(address, state.clone()).map_err(|_| "host_access")?;
        (a, l, None)
    };
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| "host_signal")?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| "host_signal")?;
    let mut access = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(root.join("access.url"))
        .map_err(|_| "host_access_file")?;
    access
        .write_all(link.as_bytes())
        .map_err(|_| "host_access_write")?;
    if let Some(probe) = probe {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(root.join("probe.url"))
            .map_err(|_| "host_probe_file")?;
        f.write_all(probe.as_bytes())
            .map_err(|_| "host_probe_write")?;
    }
    if desktop.as_ref().is_some_and(|d| *d.stop.borrow()) {
        return Err("desktop_start_cancelled".into());
    }
    let mut web = if let Some(acceptor) = acceptor {
        let listener = TlsListener {
            tcp,
            acceptor,
            host,
            prefix: config.prefix,
        };
        tokio::spawn(async move { axum::serve(listener, app).await })
    } else {
        tokio::spawn(async move { axum::serve(tcp, app).await })
    };
    struct AbortWeb(tokio::task::AbortHandle);
    impl Drop for AbortWeb {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _abort_web = AbortWeb(web.abort_handle());
    let smoke = if gemma_only || evaluation {
        Ok(())
    } else {
        state.trial_solo_smoke().await
    };
    if smoke.is_ok() {
        state.mark_real_trial();
    } else {
        session.fail();
    }
    save(
        root,
        "startup-evidence.json",
        &json!({"ready":smoke.is_ok(),"pid":std::process::id(),"chat_smoke_validated":!gemma_only&&!evaluation&&smoke.is_ok(),"automatic_chat_probe":!gemma_only&&!evaluation,"error":smoke.as_ref().err(),"image_inference_requested":false,"mode":if local{"desktop-loopback"}else if evaluation{"manual-evaluation"}else if gemma_only{"manual-gemma-web"}else{"sequential-gemma-images"},"address":address,"tls":if local{None}else{Some("1.3")},"source_subnet_prefix":config.prefix,"expires_at_unix":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|n|n.as_secs()+session.until.saturating_duration_since(Instant::now()).as_secs()),"expires_in_seconds":session.until.saturating_duration_since(Instant::now()).as_secs()}),
    )?;
    if !local {
        println!(
            "{}",
            json!({"ready":smoke.is_ok(),"access_file":root.join("access.url")})
        );
    }
    let mut desktop_stop = if let Some(d) = desktop {
        let _ = d.ready.send(crate::launcher::Ready {
            bootstrap: link,
            home: state.local_browser_home(address),
        });
        Some(d.stop)
    } else {
        None
    };
    let web_ended = if smoke.is_ok() {
        let stop = async {
            if let Some(rx) = &mut desktop_stop {
                if !*rx.borrow() {
                    let _ = rx.changed().await;
                }
            } else {
                std::future::pending::<()>().await;
            }
        };
        tokio::select! {_=&mut web=>{session.fail();true},_=term.recv()=>false,_=interrupt.recv()=>false,_=stop=>false,_=tokio::time::sleep_until(session.until.into())=>false}
    } else {
        false
    };
    session.stop();
    let drained = if let Some(images) = images {
        images.shutdown().await
    } else {
        true
    };
    let chat_drained = tokio::time::timeout(Duration::from_secs(15), async {
        while session.slot.available_permits() != 1 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .is_ok();
    if !web_ended {
        web.abort();
        let _ = web.await;
    }
    save(
        root,
        "final-evidence.json",
        &json!({"images_drained":drained,"chat_drained":chat_drained,"failed":session.failed.load(Ordering::Acquire),"web_ended_unexpectedly":web_ended}),
    )?;
    if smoke.is_err() || !drained || !chat_drained || web_ended {
        return Err("host_stopped_or_cleanup_unconfirmed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_optional_auxiliary_does_not_require_a_second_model_but_explicit_request_does() {
        let root =
            std::env::temp_dir().join(format!("seiya-optional-model-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        assert!(!optional_auxiliary(&root, false).unwrap());
        assert!(optional_auxiliary(&root, true).is_err());
        let manifest = root.join("manifests-v2/ollama.com/library/qwen3.5/9b-mlx");
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(manifest, b"not-read-by-presence-check").unwrap();
        assert!(optional_auxiliary(&root, false).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn evaluation_switches_gate_real_features_without_disabling_chat() {
        let mut s = session();
        let own = Arc::get_mut(&mut s).unwrap();
        own.feature_controlled = true;
        own.multiagent_configured = true;
        own.settings.initialize_evaluation().unwrap();
        assert!(s.enabled());
        assert!(s.multiagent_enabled());
        assert!(s.admits(FOREGROUND, AUXILIARY));
        let revision = s.settings.status()["revision"].as_u64().unwrap();
        s.settings
            .control(crate::diagnostics::Command::Features {
                revision,
                features: crate::diagnostics::Features::default(),
            })
            .unwrap();
        assert!(!s.enabled());
        assert!(!s.multiagent_enabled());
        assert!(s.admits(FOREGROUND, AUXILIARY));
    }
    #[test]
    fn settings_are_effective_for_future_image_budgets_without_granting_recovery() {
        let s = session();
        let first = s.budget();
        let limits = crate::diagnostics::Limits {
            image_swap_growth_mib: 64,
            image_deadline_seconds: 30,
            chat_warning_ms: 500,
            chat_swap_growth_mib: 256,
            ..Default::default()
        };
        s.settings
            .control(crate::diagnostics::Command::Configure {
                revision: 1,
                limits,
            })
            .unwrap();
        assert_eq!(s.budget().max_swap_growth_mib, 64);
        assert_eq!(s.budget().deadline_seconds, 30);
        assert_eq!(first.max_swap_growth_mib, 512);
        s.fail();
        let mut p = s.settings.limits();
        p.image_swap_growth_mib = 512;
        s.settings
            .control(crate::diagnostics::Command::Configure {
                revision: 2,
                limits: p,
            })
            .unwrap();
        assert!(!s.live());
        assert_eq!(s.settings.limits().chat_warning_ms, 500);
    }
    fn session() -> Arc<Session> {
        Arc::new(Session {
            feature_controlled: false,
            multiagent_configured: false,
            images_enabled: true,
            settings: crate::diagnostics::Diagnostics::memory(Arc::new(
                || json!({"pressure":1,"swap_bytes":0}),
            )),
            until: Instant::now() + Duration::from_secs(60),
            enabled: AtomicBool::new(true),
            failed: AtomicBool::new(false),
            phase: AtomicU8::new(0),
            slot: Arc::new(Semaphore::new(1)),
        })
    }
    #[tokio::test]
    async fn modalities_share_one_slot_and_release_only_after_turn() {
        let s = session();
        let mut chat = s.acquire(1).await.unwrap();
        assert!(s.slot.clone().try_acquire_owned().is_err());
        assert_eq!(s.phase.load(Ordering::Acquire), 1);
        chat.confirm();
        drop(chat);
        let mut image = s.acquire(2).await.unwrap();
        assert!(!s.admits(FOREGROUND, ""));
        assert!(s.slot.clone().try_acquire_owned().is_err());
        image.confirm();
        drop(image);
        assert!(s.admits(FOREGROUND, ""));
    }
    #[tokio::test]
    async fn expired_revoked_and_quarantined_sessions_never_acquire() {
        let s = session();
        s.stop();
        assert!(s.acquire(1).await.is_err());
        let s = session();
        s.fail();
        assert!(s.acquire(2).await.is_err());
        let s = Arc::new(Session {
            feature_controlled: false,
            multiagent_configured: false,
            images_enabled: true,
            settings: crate::diagnostics::Diagnostics::memory(Arc::new(
                || json!({"pressure":1,"swap_bytes":0}),
            )),
            until: Instant::now(),
            enabled: AtomicBool::new(true),
            failed: AtomicBool::new(false),
            phase: AtomicU8::new(0),
            slot: Arc::new(Semaphore::new(1)),
        });
        assert!(s.acquire(1).await.is_err());
    }
    #[tokio::test]
    async fn lost_turn_blocks_both_modalities() {
        let s = session();
        let turn = s.acquire(1).await.unwrap();
        drop(turn);
        assert!(!s.live());
        assert!(s.acquire(2).await.is_err());
    }
    #[test]
    fn lan_peer_filter_is_explicit_ipv4_subnet_not_all_private_networks() {
        let host = Ipv4Addr::new(192, 168, 1, 149);
        assert!(same_subnet(host, "192.168.1.22".parse().unwrap(), 24));
        for p in ["192.168.2.22", "10.1.1.1", "8.8.8.8", "::1"] {
            assert!(!same_subnet(host, p.parse().unwrap(), 24));
        }
        assert!(!same_subnet(host, "192.168.1.22".parse().unwrap(), 0));
    }
    #[test]
    fn diagnostics_distinguish_stop_conditions_without_inventing_cause() {
        assert_eq!(
            stop_reason(
                true,
                true,
                Duration::ZERO,
                true,
                true,
                Duration::from_secs(120)
            ),
            None
        );
        assert_eq!(
            stop_reason(
                true,
                false,
                Duration::ZERO,
                true,
                true,
                Duration::from_secs(120)
            ),
            Some("memory_pressure_swap_or_stale_observation")
        );
        assert_eq!(
            stop_reason(
                true,
                true,
                Duration::from_secs(120),
                true,
                true,
                Duration::from_secs(120)
            ),
            Some("turn_deadline")
        );
        assert_eq!(
            stop_reason(
                true,
                true,
                Duration::ZERO,
                false,
                true,
                Duration::from_secs(120)
            ),
            Some("disk_reserve")
        );
        assert_eq!(
            stop_reason(
                true,
                true,
                Duration::ZERO,
                true,
                false,
                Duration::from_secs(120)
            ),
            Some("runtime_log_limit_or_unavailable")
        );
    }
}
