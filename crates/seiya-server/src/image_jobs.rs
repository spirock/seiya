//! Opt-in image broker, separate from text jobs and model tools. No I/O on new().
#[cfg(target_os = "macos")]
pub mod native;
use axum::body::Bytes;
use seiya_contracts::image_job::{ImageRequest, ImageStage, MAX_PNG_BYTES};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::{watch, Semaphore};

pub const MAX_ARTIFACT_BYTES: usize = 32 * 1024 * 1024;
pub type Work = Pin<Box<dyn Future<Output = Result<Vec<u8>, Failure>> + Send>>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Blocked,
    Cancelled,
    Failed,
    Uncertain,
}
pub trait Executor: Send + Sync + 'static {
    fn available(&self) -> bool {
        false
    }
    /// Return bytes ONLY after owned execution/cleanup is confirmed. Uncertain
    /// transport or cleanup is Uncertain. Cancellation is a request, not proof.
    fn generate(
        &self,
        request: ImageRequest,
        cancel: watch::Receiver<bool>,
        stage: watch::Sender<ImageStage>,
    ) -> Work;
}
pub trait Admission: Send + Sync + 'static {
    fn allows_image(&self) -> bool;
}
pub struct Blocked;
impl Admission for Blocked {
    fn allows_image(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Preparing,
    Running,
    Validating,
    Cancelling,
    Completed,
    Cancelled,
    Failed,
    Blocked,
    Quarantined,
}
impl Phase {
    fn active(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Preparing | Self::Running | Self::Validating | Self::Cancelling
        )
    }
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Session {},
    Open {
        conversation_id: String,
    },
    Forget {
        session: String,
    },
    Submit {
        session: String,
        request_id: String,
        request: ImageRequest,
    },
    List {
        session: String,
    },
    Status {
        session: String,
        job_id: String,
    },
    Cancel {
        session: String,
        job_id: String,
    },
    Artifact {
        session: String,
        job_id: String,
    },
}
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unknown,
    Capacity,
    Unavailable,
    NotReady,
    Storage,
}
pub struct Artifact {
    pub png: Bytes,
    pub metadata: Value,
}
pub enum Reply {
    Json(Value),
    Png(Arc<Artifact>),
}
struct Job {
    owner: String,
    request_id: String,
    request: ImageRequest,
    phase: Phase,
    cancel: watch::Sender<bool>,
    user_cancel: bool,
    artifact: Option<Arc<Artifact>>,
    error: Option<&'static str>,
    deadline: tokio::time::Instant,
}
struct Registry {
    tasks: std::collections::BTreeSet<String>,
    conversations: BTreeMap<String, String>,
    sessions: BTreeMap<String, Instant>,
    jobs: BTreeMap<String, Job>,
    artifact_bytes: usize,
}
type Verify = fn(&[u8], &ImageRequest) -> Result<Value, ()>;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    quarantined: bool,
    sessions: Vec<String>,
    conversations: BTreeMap<String, String>,
    jobs: Vec<SavedJob>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedJob {
    id: String,
    owner: String,
    request_id: String,
    request: ImageRequest,
    phase: Phase,
    cancel: bool,
    user_cancel: bool,
    error: Option<String>,
    metadata: Value,
}
fn valid_token(t: &str) -> bool {
    t.len() == 64
        && t.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn restored_error(value: Option<&str>, interrupted: bool) -> Option<&'static str> {
    if interrupted {
        return Some("interrupted_by_restart_no_replay");
    }
    value.map(|s| match s {
        "pool_quarantined" => "pool_quarantined",
        "queue_deadline" => "queue_deadline",
        "permission_or_resources" => "permission_or_resources",
        "deadline" => "deadline",
        "artifact_capacity" => "artifact_capacity",
        "generation_or_png_validation_failed" => "generation_or_png_validation_failed",
        "cleanup_unconfirmed" => "cleanup_unconfirmed",
        "executor_dropped_or_panicked" => "executor_dropped_or_panicked",
        "host_shutdown_cleanup_unconfirmed" => "host_shutdown_cleanup_unconfirmed",
        "interrupted_by_restart_no_replay" => "interrupted_by_restart_no_replay",
        _ => "restored_failure",
    })
}
pub struct ImageJobs {
    store: Option<crate::job_store::Store>,
    storage_failed: AtomicBool,
    registry: Mutex<Registry>,
    executor: Arc<dyn Executor>,
    admission: Arc<dyn Admission>,
    slot: Arc<Semaphore>,
    quarantined: AtomicBool,
    limit: Duration,
    verify: Verify,
    decoder_supported: bool,
}
struct RunLease {
    jobs: Arc<ImageJobs>,
    id: String,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
    confirmed: bool,
}
impl Drop for RunLease {
    fn drop(&mut self) {
        if !self.confirmed {
            self.jobs
                .quarantine(&self.id, "executor_dropped_or_panicked");
        }
        self.jobs
            .registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .tasks
            .remove(&self.id);
    }
}
fn native_verify(raw: &[u8], request: &ImageRequest) -> Result<Value, ()> {
    let decoded = seiya_observe::image::artifact::decode(raw, request.width, request.height)
        .map_err(|_| ())?;
    serde_json::to_value(decoded).map_err(|_| ())
}
fn id(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok_and(|u| u.to_string() == s)
}
fn view(id: &str, j: &Job) -> Value {
    json!({"job_id":id,"request_id":j.request_id,"phase":j.phase,"cancel_requested":*j.cancel.borrow(),"artifact_available":j.phase==Phase::Completed,"error":j.error,"progress_percent":null})
}
impl ImageJobs {
    /// Stable host-owned private directory; no startup execution or recovery of grants.
    pub fn persistent(
        executor: Arc<dyn Executor>,
        admission: Arc<dyn Admission>,
        root: &std::path::Path,
    ) -> Result<Arc<Self>, String> {
        let mut jobs = Self::new(executor, admission);
        Self::attach_store(&mut jobs, root)?;
        Ok(jobs)
    }
    fn attach_store(jobs: &mut Arc<Self>, root: &std::path::Path) -> Result<(), String> {
        let store = crate::job_store::Store::open(root)?;
        let own = Arc::get_mut(jobs).ok_or("attach_store_before_sharing")?;
        if let Some((manifest, mut blobs)) = store.load()? {
            let saved: Saved =
                serde_json::from_value(manifest).map_err(|_| "image_snapshot_schema")?;
            if saved.version != 1
                || saved.jobs.len() > 8
                || saved.sessions.len() > 16
                || saved.conversations.len() > 16
            {
                return Err("image_snapshot_limits".into());
            }
            let mut state = Registry {
                tasks: Default::default(),
                sessions: BTreeMap::new(),
                conversations: saved.conversations,
                jobs: BTreeMap::new(),
                artifact_bytes: 0,
            };
            for token in saved.sessions {
                if !valid_token(&token) || state.sessions.insert(token, Instant::now()).is_some() {
                    return Err("image_snapshot_session".into());
                }
            }
            if state
                .conversations
                .iter()
                .any(|(c, t)| !id(c) || !state.sessions.contains_key(t))
            {
                return Err("image_snapshot_conversation".into());
            }
            let mut quarantine = saved.quarantined;
            for r in saved.jobs {
                r.request.validate().map_err(|_| "image_snapshot_request")?;
                if !id(&r.id)
                    || !id(&r.request_id)
                    || !state.sessions.contains_key(&r.owner)
                    || r.request.width != 512
                    || r.request.height != 512
                    || state.jobs.contains_key(&r.id)
                {
                    return Err("image_snapshot_record".into());
                }
                if state
                    .jobs
                    .values()
                    .any(|j: &Job| j.owner == r.owner && j.request_id == r.request_id)
                {
                    return Err("image_snapshot_duplicate_request".into());
                }
                let phase = if r.phase == Phase::Queued {
                    Phase::Cancelled
                } else if r.phase.active() {
                    quarantine = true;
                    Phase::Quarantined
                } else {
                    r.phase
                };
                quarantine |= phase == Phase::Quarantined;
                let artifact = if phase == Phase::Completed {
                    let bytes = blobs.remove(&r.id).ok_or("image_snapshot_missing_png")?;
                    let metadata = (own.verify)(&bytes, &r.request)
                        .map_err(|_| "image_snapshot_png_invalid")?;
                    if metadata != r.metadata {
                        return Err("image_snapshot_png_metadata".into());
                    }
                    state.artifact_bytes += bytes.len();
                    Some(Arc::new(Artifact {
                        png: Bytes::from(bytes),
                        metadata,
                    }))
                } else {
                    if !r.metadata.is_null() {
                        return Err("image_snapshot_unexpected_artifact".into());
                    }
                    None
                };
                let (cancel, _) = watch::channel(
                    r.cancel || phase == Phase::Cancelled || phase == Phase::Quarantined,
                );
                state.jobs.insert(
                    r.id,
                    Job {
                        owner: r.owner,
                        request_id: r.request_id,
                        request: r.request,
                        phase,
                        cancel,
                        user_cancel: r.user_cancel,
                        artifact,
                        error: restored_error(r.error.as_deref(), r.phase.active()),
                        deadline: tokio::time::Instant::now(),
                    },
                );
            }
            if !blobs.is_empty() || state.artifact_bytes > MAX_ARTIFACT_BYTES {
                return Err("image_snapshot_unreferenced_blobs".into());
            }
            own.registry = Mutex::new(state);
            own.quarantined.store(quarantine, Ordering::Release);
            if quarantine {
                own.slot.close();
            }
        }
        own.store = Some(store);
        own.checkpoint(&mut own.registry.lock().unwrap())
            .map_err(|_| "image_snapshot_checkpoint".to_owned())?;
        Ok(())
    }
    fn checkpoint(&self, state: &mut Registry) -> Result<(), Error> {
        if self.storage_failed.load(Ordering::Acquire) {
            return Err(Error::Storage);
        }
        let Some(store) = &self.store else {
            return Ok(());
        };
        let saved = Saved {
            version: 1,
            quarantined: self.quarantined.load(Ordering::Acquire),
            sessions: state.sessions.keys().cloned().collect(),
            conversations: state.conversations.clone(),
            jobs: state
                .jobs
                .iter()
                .map(|(id, j)| SavedJob {
                    id: id.clone(),
                    owner: j.owner.clone(),
                    request_id: j.request_id.clone(),
                    request: j.request.clone(),
                    phase: j.phase,
                    cancel: *j.cancel.borrow(),
                    user_cancel: j.user_cancel,
                    error: j.error.map(str::to_owned),
                    metadata: j
                        .artifact
                        .as_ref()
                        .map_or(Value::Null, |a| a.metadata.clone()),
                })
                .collect(),
        };
        let manifest = serde_json::to_value(saved).map_err(|_| Error::Storage)?;
        let blobs = state
            .jobs
            .iter()
            .filter_map(|(id, j)| j.artifact.as_ref().map(|a| (id.clone(), a.png.as_ref())))
            .collect();
        if store.save(manifest, blobs).is_err() {
            self.storage_failed.store(true, Ordering::Release);
            self.slot.close();
            for j in state.jobs.values_mut().filter(|j| j.phase.active()) {
                j.cancel.send_replace(true);
                j.phase = if j.phase == Phase::Queued {
                    Phase::Blocked
                } else {
                    Phase::Cancelling
                };
            }
            return Err(Error::Storage);
        }
        Ok(())
    }
    /// Host shutdown only; revocation must precede this call. HTTP has no reset.
    #[cfg(target_os = "macos")]
    pub(crate) async fn shutdown(&self) -> bool {
        {
            let mut s = self.registry.lock().unwrap();
            for j in s.jobs.values_mut().filter(|j| j.phase.active()) {
                j.user_cancel = true;
                j.cancel.send_replace(true);
                j.phase = if j.phase == Phase::Queued {
                    Phase::Cancelled
                } else {
                    Phase::Cancelling
                };
            }
        }
        let drained = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if !self
                    .registry
                    .lock()
                    .unwrap()
                    .jobs
                    .values()
                    .any(|j| j.phase.active())
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .is_ok();
        if !drained {
            let ids: Vec<_> = self
                .registry
                .lock()
                .unwrap()
                .jobs
                .iter()
                .filter(|(_, j)| j.phase.active())
                .map(|(id, _)| id.clone())
                .collect();
            for id in ids {
                self.quarantine(&id, "host_shutdown_cleanup_unconfirmed");
            }
        }
        drained && !self.quarantined.load(Ordering::Acquire)
    }
    pub fn new(executor: Arc<dyn Executor>, admission: Arc<dyn Admission>) -> Arc<Self> {
        Self::configured(
            executor,
            admission,
            Duration::from_secs(620),
            native_verify,
            cfg!(target_os = "macos"),
        )
    }
    fn configured(
        executor: Arc<dyn Executor>,
        admission: Arc<dyn Admission>,
        limit: Duration,
        verify: Verify,
        supported: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            store: None,
            storage_failed: AtomicBool::new(false),
            registry: Mutex::new(Registry {
                tasks: Default::default(),
                conversations: BTreeMap::new(),
                sessions: BTreeMap::new(),
                jobs: BTreeMap::new(),
                artifact_bytes: 0,
            }),
            executor,
            admission,
            slot: Arc::new(Semaphore::new(1)),
            quarantined: AtomicBool::new(false),
            limit,
            verify,
            decoder_supported: supported,
        })
    }
    pub fn capabilities(&self) -> Value {
        json!({"available":self.decoder_supported&&self.executor.available()&&!self.storage_failed.load(Ordering::Acquire)&&!self.quarantined.load(Ordering::Acquire)&&self.admission.allows_image(),"version":"image-jobs-v1","width":512,"height":512,"max_prompt_bytes":2048,"max_png_bytes":MAX_PNG_BYTES,"volatile":self.store.is_none(),"history_recovery":true,"storage_failed":self.storage_failed.load(Ordering::Acquire),"quarantined":self.quarantined.load(Ordering::Acquire),"progress_percent":null})
    }
    fn quarantine(&self, id: &str, error: &'static str) {
        self.quarantined.store(true, Ordering::Release);
        self.slot.close();
        let mut state = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        for (key, j) in &mut state.jobs {
            if key == id {
                j.phase = Phase::Quarantined;
                j.error = Some(error);
                j.artifact = None;
                j.cancel.send_replace(true);
            } else if j.phase.active() {
                j.cancel.send_replace(true);
                j.phase = if j.phase == Phase::Queued {
                    Phase::Blocked
                } else {
                    Phase::Cancelling
                };
            }
        }
        let _ = self.checkpoint(&mut state);
    }
    /// Session capability checked before job data/effects. No URL/path from UI.
    pub fn control(self: &Arc<Self>, command: Command) -> Result<Reply, Error> {
        if self.storage_failed.load(Ordering::Acquire) {
            return Err(Error::Storage);
        }
        let persist = matches!(
            command,
            Command::Session {}
                | Command::Open { .. }
                | Command::Forget { .. }
                | Command::Cancel { .. }
        );
        let reply = self.command(command)?;
        if persist {
            self.checkpoint(&mut self.registry.lock().unwrap())?;
        }
        if self.storage_failed.load(Ordering::Acquire) {
            return Err(Error::Storage);
        }
        Ok(reply)
    }
    fn command(self: &Arc<Self>, command: Command) -> Result<Reply, Error> {
        if let Command::Open { conversation_id } = command {
            if !id(&conversation_id) {
                return Err(Error::Invalid);
            }
            let mut s = self.registry.lock().unwrap();
            let token = if let Some(t) = s.conversations.get(&conversation_id) {
                t.clone()
            } else {
                if s.sessions.len() >= 16 {
                    return Err(Error::Capacity);
                }
                let token = format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                );
                s.conversations.insert(conversation_id, token.clone());
                token
            };
            s.sessions.insert(token.clone(), Instant::now());
            return Ok(Reply::Json(
                json!({"session":token,"volatile":self.store.is_none(),"ttl_seconds":1800}),
            ));
        }
        if matches!(command, Command::Session {}) {
            let mut s = self.registry.lock().unwrap();
            if s.sessions.len() >= 16 {
                return Err(Error::Capacity);
            }
            let token = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            s.sessions.insert(token.clone(), Instant::now());
            return Ok(Reply::Json(
                json!({"session":token,"volatile":self.store.is_none(),"ttl_seconds":1800}),
            ));
        }
        let session = match &command {
            Command::Submit { session, .. }
            | Command::List { session }
            | Command::Status { session, .. }
            | Command::Cancel { session, .. }
            | Command::Artifact { session, .. } => session,
            Command::Forget { session } => session,
            _ => unreachable!(),
        };
        let mut s = self.registry.lock().unwrap();
        if session.len() != 64
            || s.sessions
                .get(session)
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(1800))
        {
            return Err(Error::Unknown);
        }
        s.sessions.insert(session.clone(), Instant::now());
        match command {
            Command::Submit {
                session,
                request_id,
                request,
            } => {
                request.validate().map_err(|_| Error::Invalid)?;
                if !id(&request_id) || request.width != 512 || request.height != 512 {
                    return Err(Error::Invalid);
                }
                if let Some((key, j)) = s
                    .jobs
                    .iter()
                    .find(|(_, j)| j.owner == session && j.request_id == request_id)
                {
                    if j.request != request {
                        return Err(Error::Invalid);
                    }
                    return Ok(Reply::Json(view(key, j)));
                }
                // Host callback outside registry lock. Session is verified again
                // when reacquired, avoiding foreign code under the lock.
                drop(s);
                if self.capabilities()["available"] != true {
                    return Err(Error::Unavailable);
                }
                let mut s = self.registry.lock().unwrap();
                if s.sessions
                    .get(&session)
                    .is_none_or(|t| t.elapsed() >= Duration::from_secs(1800))
                {
                    return Err(Error::Unknown);
                }
                // A concurrent retry may have inserted the same request.
                if let Some((key, j)) = s
                    .jobs
                    .iter()
                    .find(|(_, j)| j.owner == session && j.request_id == request_id)
                {
                    if j.request != request {
                        return Err(Error::Invalid);
                    }
                    return Ok(Reply::Json(view(key, j)));
                }
                if s.jobs.len() >= 8 || s.jobs.values().filter(|j| j.phase.active()).count() >= 3 {
                    return Err(Error::Capacity);
                }
                let runtime =
                    tokio::runtime::Handle::try_current().map_err(|_| Error::Unavailable)?;
                let key = uuid::Uuid::new_v4().to_string();
                let (cancel, rx) = watch::channel(false);
                let j = Job {
                    owner: session,
                    request_id,
                    request,
                    phase: Phase::Queued,
                    cancel,
                    user_cancel: false,
                    artifact: None,
                    error: None,
                    deadline: tokio::time::Instant::now() + self.limit,
                };
                let value = view(&key, &j);
                s.jobs.insert(key.clone(), j);
                self.checkpoint(&mut s)?;
                s.tasks.insert(key.clone());
                let own = self.clone();
                runtime.spawn(async move {
                    own.run(key, rx).await;
                });
                Ok(Reply::Json(value))
            }
            Command::List { session } => Ok(Reply::Json(
                json!({"jobs":s.jobs.iter().filter(|(_,j)|j.owner==session).map(|(key,j)|view(key,j)).collect::<Vec<_>>(),"volatile":self.store.is_none()}),
            )),
            Command::Status { session, job_id } => {
                let j = s
                    .jobs
                    .get(&job_id)
                    .filter(|j| j.owner == session)
                    .ok_or(Error::Unknown)?;
                Ok(Reply::Json(view(&job_id, j)))
            }
            Command::Cancel { session, job_id } => {
                let j = s
                    .jobs
                    .get_mut(&job_id)
                    .filter(|j| j.owner == session)
                    .ok_or(Error::Unknown)?;
                if j.phase.active() {
                    j.user_cancel = true;
                    j.cancel.send_replace(true);
                    j.phase = if j.phase == Phase::Queued {
                        Phase::Cancelled
                    } else {
                        Phase::Cancelling
                    };
                }
                Ok(Reply::Json(view(&job_id, j)))
            }
            Command::Artifact { session, job_id } => {
                let j = s
                    .jobs
                    .get(&job_id)
                    .filter(|j| j.owner == session)
                    .ok_or(Error::Unknown)?;
                if j.phase != Phase::Completed {
                    return Err(Error::NotReady);
                }
                Ok(Reply::Png(j.artifact.clone().ok_or(Error::NotReady)?))
            }
            Command::Forget { session } => {
                if s.tasks
                    .iter()
                    .any(|id| s.jobs.get(id).is_some_and(|j| j.owner == session))
                {
                    return Err(Error::NotReady);
                }
                if s.jobs.values().any(|j| {
                    j.owner == session && (j.phase.active() || j.phase == Phase::Quarantined)
                }) {
                    return Err(Error::NotReady);
                }
                let removed: usize = s
                    .jobs
                    .values()
                    .filter(|j| j.owner == session)
                    .filter_map(|j| j.artifact.as_ref().map(|a| a.png.len()))
                    .sum();
                s.artifact_bytes -= removed;
                s.jobs.retain(|_, j| j.owner != session);
                s.sessions.remove(&session);
                s.conversations.retain(|_, t| *t != session);
                Ok(Reply::Json(json!({"forgotten":true})))
            }
            Command::Session {} | Command::Open { .. } => unreachable!(),
        }
    }
    fn stage(&self, key: &str, stage: ImageStage) {
        let mut s = self.registry.lock().unwrap();
        let j = s.jobs.get_mut(key).unwrap();
        if !j.phase.active() || j.phase == Phase::Cancelling {
            return;
        }
        let next = match stage {
            ImageStage::Preparing => Phase::Preparing,
            ImageStage::Running => Phase::Running,
            ImageStage::Validating => Phase::Validating,
        };
        let rank = |p| match p {
            Phase::Preparing => 0,
            Phase::Running => 1,
            Phase::Validating => 2,
            _ => 3,
        };
        if rank(next) >= rank(j.phase) {
            j.phase = next;
        }
        let _ = self.checkpoint(&mut s);
    }
    async fn run(self: Arc<Self>, key: String, mut cancel: watch::Receiver<bool>) {
        let mut lease = RunLease {
            jobs: self.clone(),
            id: key.clone(),
            permit: None,
            confirmed: false,
        };
        let deadline = self.registry.lock().unwrap().jobs[&key].deadline;
        let permit = if *cancel.borrow() {
            None
        } else {
            tokio::select! {p=self.slot.clone().acquire_owned()=>p.ok(),_=tokio::time::sleep_until(deadline)=>None,_=cancel.changed()=>None}
        };
        let Some(permit) = permit else {
            let mut s = self.registry.lock().unwrap();
            let j = s.jobs.get_mut(&key).unwrap();
            j.phase = if self.quarantined.load(Ordering::Acquire) {
                j.error = Some("pool_quarantined");
                Phase::Blocked
            } else if j.user_cancel {
                Phase::Cancelled
            } else {
                j.error = Some("queue_deadline");
                Phase::Failed
            };
            lease.confirmed = true;
            let _ = self.checkpoint(&mut s);
            return;
        };
        lease.permit = Some(permit);
        let admitted = self.decoder_supported
            && self.executor.available()
            && self.admission.allows_image()
            && !self.quarantined.load(Ordering::Acquire);
        let request = {
            let mut s = self.registry.lock().unwrap();
            let j = s.jobs.get_mut(&key).unwrap();
            if *cancel.borrow() || !admitted || tokio::time::Instant::now() >= deadline {
                j.phase = if j.user_cancel {
                    Phase::Cancelled
                } else if !admitted {
                    j.error = Some("permission_or_resources");
                    Phase::Blocked
                } else {
                    j.error = Some("queue_deadline");
                    Phase::Failed
                };
                lease.confirmed = true;
                let _ = self.checkpoint(&mut s);
                return;
            }
            j.phase = Phase::Preparing;
            let request = j.request.clone();
            if self.checkpoint(&mut s).is_err() {
                lease.confirmed = true;
                return;
            }
            request
        };
        let (stage_tx, mut stage_rx) = watch::channel(ImageStage::Preparing);
        let mut work = self
            .executor
            .generate(request.clone(), cancel.clone(), stage_tx);
        let mut stage_open = true;
        let mut draining = false;
        let mut timed_out = false;
        let mut until = deadline;
        let outcome = loop {
            tokio::select! {
                value=&mut work=>break Some(value),
                changed=stage_rx.changed(),if stage_open=>{if changed.is_err(){stage_open=false;}else{let observed=*stage_rx.borrow_and_update();self.stage(&key,observed);}},
                _=cancel.changed(),if !draining=>{draining=true;until=tokio::time::Instant::now()+Duration::from_secs(10);},
                _=tokio::time::sleep_until(until)=>{
                    if draining{break None;}
                    timed_out=true;draining=true;until=tokio::time::Instant::now()+Duration::from_secs(10);
                    let mut s=self.registry.lock().unwrap();let j=s.jobs.get_mut(&key).unwrap();j.cancel.send_replace(true);j.phase=Phase::Cancelling;
                }
            }
        };
        drop(work); // native guard closes/quarantines before slot can be reused
        let outcome = match outcome {
            None | Some(Err(Failure::Uncertain)) => {
                self.quarantine(&key, "cleanup_unconfirmed");
                lease.confirmed = true;
                return;
            }
            Some(value) => value,
        };
        let failure = outcome.as_ref().err().copied();
        let artifact = match outcome {
            Ok(bytes) if bytes.len() <= MAX_PNG_BYTES => {
                self.stage(&key, ImageStage::Validating);
                (self.verify)(&bytes, &request).ok().map(|metadata| {
                    Arc::new(Artifact {
                        png: Bytes::from(bytes),
                        metadata,
                    })
                })
            }
            _ => None,
        };
        let mut s = self.registry.lock().unwrap();
        let user_cancel = s.jobs[&key].user_cancel;
        let cancel_requested = *s.jobs[&key].cancel.borrow();
        let pool_quarantined = self.quarantined.load(Ordering::Acquire);
        let capacity_ok = artifact
            .as_ref()
            .is_none_or(|a| a.png.len() <= MAX_ARTIFACT_BYTES.saturating_sub(s.artifact_bytes));
        if !timed_out && !cancel_requested && !pool_quarantined && capacity_ok {
            if let Some(a) = &artifact {
                s.artifact_bytes += a.png.len();
            }
        }
        let j = s.jobs.get_mut(&key).unwrap();
        if user_cancel {
            j.phase = Phase::Cancelled;
        } else if timed_out {
            j.phase = Phase::Failed;
            j.error = Some("deadline");
        } else if pool_quarantined {
            j.phase = Phase::Blocked;
            j.error = Some("pool_quarantined");
        } else if cancel_requested {
            j.phase = Phase::Cancelled;
        } else if let Some(a) = artifact.filter(|_| capacity_ok) {
            j.phase = Phase::Completed;
            j.artifact = Some(a);
        } else {
            j.phase = match failure {
                Some(Failure::Blocked) => Phase::Blocked,
                Some(Failure::Cancelled) => Phase::Cancelled,
                _ => Phase::Failed,
            };
            j.error = if !capacity_ok {
                Some("artifact_capacity")
            } else {
                match failure {
                    Some(Failure::Blocked) => Some("permission_or_resources"),
                    Some(Failure::Cancelled) => None,
                    _ => Some("generation_or_png_validation_failed"),
                }
            };
        }
        lease.confirmed = true;
        let _ = self.checkpoint(&mut s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn queued_cancel_cannot_remove_a_record_before_its_task_drains() {
        let (j, _, calls) = fixture(false, false, Duration::from_secs(30));
        let s = session(&j);
        let key = submit(&j, &s, &uuid::Uuid::new_v4().to_string());
        value(j.control(Command::Cancel {
            session: s.clone(),
            job_id: key,
        }));
        assert!(matches!(
            j.control(Command::Forget { session: s.clone() }),
            Err(Error::NotReady)
        ));
        tokio::task::yield_now().await;
        value(j.control(Command::Forget { session: s }));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn durable_completed_png_and_binding_survive_restart_without_execution() {
        let root = crate::job_store::tests::directory();
        let (mut j, done, calls) = fixture(false, false, Duration::from_secs(30));
        ImageJobs::attach_store(&mut j, &root).unwrap();
        let conversation = uuid::Uuid::new_v4().to_string();
        let s = value(j.control(Command::Open {
            conversation_id: conversation.clone(),
        }))["session"]
            .as_str()
            .unwrap()
            .to_owned();
        let request_id = uuid::Uuid::new_v4().to_string();
        let key = submit(&j, &s, &request_id);
        phase(&j, &s, &key, "running").await;
        done.notify_one();
        phase(&j, &s, &key, "completed").await;
        let executor = j.executor.clone();
        drop(j);
        let mut reopened = ImageJobs::configured(
            executor,
            Arc::new(Ready),
            Duration::from_secs(30),
            fixture_verify,
            true,
        );
        ImageJobs::attach_store(&mut reopened, &root).unwrap();
        let restored = value(reopened.control(Command::Open {
            conversation_id: conversation,
        }))["session"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(restored, s);
        assert_eq!(submit(&reopened, &s, &request_id), key);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(
            reopened.control(Command::Artifact {
                session: s.clone(),
                job_id: key
            }),
            Ok(Reply::Png(_))
        ));
        assert_eq!(reopened.capabilities()["volatile"], false);
        value(reopened.control(Command::Forget { session: s }));
        drop(reopened);
        let store = crate::job_store::Store::open(&root).unwrap();
        let (m, b) = store.load().unwrap().unwrap();
        assert!(m["jobs"].as_array().unwrap().is_empty());
        assert!(b.is_empty());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn interrupted_restart_never_replays_and_storage_failure_never_dispatches() {
        let root = crate::job_store::tests::directory();
        let (mut j, _, calls) = fixture(false, false, Duration::from_secs(30));
        ImageJobs::attach_store(&mut j, &root).unwrap();
        let s = session(&j);
        let key = uuid::Uuid::new_v4().to_string();
        {
            let mut state = j.registry.lock().unwrap();
            let (cancel, _) = watch::channel(false);
            state.jobs.insert(
                key.clone(),
                Job {
                    owner: s.clone(),
                    request_id: uuid::Uuid::new_v4().to_string(),
                    request: request(),
                    phase: Phase::Running,
                    cancel,
                    user_cancel: false,
                    artifact: None,
                    error: None,
                    deadline: tokio::time::Instant::now(),
                },
            );
            j.checkpoint(&mut state).unwrap();
        }
        let executor = j.executor.clone();
        drop(j);
        let mut j = ImageJobs::configured(
            executor,
            Arc::new(Ready),
            Duration::from_secs(30),
            fixture_verify,
            true,
        );
        ImageJobs::attach_store(&mut j, &root).unwrap();
        assert_eq!(status(&j, &s, &key)["phase"], "quarantined");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            j.control(Command::Forget { session: s }),
            Err(Error::NotReady)
        ));
        drop(j);
        std::fs::remove_dir_all(root).unwrap();
        let root = crate::job_store::tests::directory();
        let (mut j, _, calls) = fixture(false, false, Duration::from_secs(30));
        ImageJobs::attach_store(&mut j, &root).unwrap();
        let s = session(&j);
        let moved = root.with_extension("moved");
        std::fs::rename(&root, &moved).unwrap();
        assert!(matches!(
            j.control(Command::Submit {
                session: s,
                request_id: uuid::Uuid::new_v4().to_string(),
                request: request()
            }),
            Err(Error::Storage)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(j.capabilities()["available"], false);
        drop(j);
        assert!(moved.join("snapshot.bin").is_file());
        std::fs::remove_dir_all(moved).unwrap();
    }
    fn web(jobs: Arc<ImageJobs>) -> axum::Router {
        use crate::{
            prepared_web::{PreparedWeb, FOREGROUND},
            runtime::{Backend, RuntimeConfig, RuntimeFuture, RuntimeTransport},
        };
        struct NoChat;
        impl RuntimeTransport for NoChat {
            fn complete(&self, _: Value) -> RuntimeFuture<'_> {
                panic!("image control must not dispatch Gemma")
            }
        }
        let config = RuntimeConfig {
            id: "image-fixture".into(),
            model: FOREGROUND.into(),
            backend: Backend::Mlx,
            base_url: "http://127.0.0.1:49401/v1".into(),
            timeout_seconds: 10,
            max_output_tokens: 256,
        };
        let state = PreparedWeb::solo(
            config,
            Arc::new(NoChat),
            Arc::new(crate::prepared_web::Denied),
        )
        .unwrap()
        .with_images(jobs)
        .unwrap();
        crate::prepared_web::app(
            "127.0.0.1:49400".parse().unwrap(),
            "synthetic-image-key".into(),
            state,
        )
        .unwrap()
    }
    async fn http(app: &axum::Router, value: Value) -> axum::response::Response {
        use tower::ServiceExt;
        let raw = serde_json::to_vec(&value).unwrap();
        app.clone()
            .oneshot(
                axum::http::Request::post("/api/images/v1")
                    .header("host", "127.0.0.1:49400")
                    .header("authorization", "Bearer synthetic-image-key")
                    .header("content-type", "application/json")
                    .header("content-length", raw.len())
                    .body(axum::body::Body::from(raw))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn http_json(app: &axum::Router, value: Value) -> Value {
        use http_body_util::BodyExt;
        let r = http(app, value).await;
        assert_eq!(r.status(), 200);
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }
    #[tokio::test]
    async fn http_is_authenticated_opt_in_and_serves_owned_binary_after_confirmation() {
        use http_body_util::BodyExt;
        let (j, done, _) = fixture(false, false, Duration::from_secs(30));
        let app = web(j.clone());
        let s = http_json(&app, json!({"operation":"session"})).await["session"]
            .as_str()
            .unwrap()
            .to_owned();
        let submitted=http_json(&app,json!({"operation":"submit","session":s,"request_id":uuid::Uuid::new_v4().to_string(),"request":{"prompt":"fixture","width":512,"height":512,"seed":42}})).await;
        let key = submitted["job_id"].as_str().unwrap();
        phase(&j, &s, key, "running").await;
        assert_eq!(
            http(
                &app,
                json!({"operation":"artifact","session":s,"job_id":key})
            )
            .await
            .status(),
            409
        );
        done.notify_one();
        phase(&j, &s, key, "completed").await;
        let r = http(
            &app,
            json!({"operation":"artifact","session":s,"job_id":key}),
        )
        .await;
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["content-type"], "image/png");
        assert_eq!(r.headers()["cache-control"], "no-store");
        assert_eq!(r.headers()["x-content-type-options"], "nosniff");
        assert!(r.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .contains("seiya-image.png"));
        let bytes = r.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(bytes.as_ref(), b"fixture-png-bytes-not-a-real-decoder");
        let other = session(&j);
        assert_eq!(
            http(
                &app,
                json!({"operation":"artifact","session":other,"job_id":key})
            )
            .await
            .status(),
            404
        );
    }
    #[tokio::test]
    async fn http_guard_and_closed_frames_precede_executor_and_css_is_unchanged() {
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let (j, _, calls) = fixture(false, false, Duration::from_secs(30));
        let app = web(j);
        for (host, auth, origin, expected) in [
            ("evil.example", "Bearer synthetic-image-key", None, 403),
            ("127.0.0.1:49400", "Bearer wrong", None, 401),
            (
                "127.0.0.1:49400",
                "Bearer synthetic-image-key",
                Some("https://evil.example"),
                403,
            ),
        ] {
            let mut r = axum::http::Request::post("/api/images/v1")
                .header("host", host)
                .header("authorization", auth);
            if let Some(o) = origin {
                r = r.header("origin", o);
            }
            assert_eq!(
                app.clone()
                    .oneshot(r.body(axum::body::Body::from("malformed")).unwrap())
                    .await
                    .unwrap()
                    .status()
                    .as_u16(),
                expected
            );
        }
        assert_eq!(
            http(
                &app,
                json!({"operation":"session","model":"other","allow":true})
            )
            .await
            .status(),
            400
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let page = app
            .oneshot(
                axum::http::Request::get("/")
                    .header("host", "127.0.0.1:49400")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let html = String::from_utf8(
            page.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        let original = include_str!("../../../swarm-app/seiya/ui.html");
        assert_eq!(
            html.split("<style>")
                .nth(1)
                .unwrap()
                .split("</style>")
                .next(),
            original
                .split("<style>")
                .nth(1)
                .unwrap()
                .split("</style>")
                .next()
        );
        assert!(html.contains("seiyaImageCapabilities(data.images)"));
        assert!(html.contains("/api/images/v1"));
    }
    #[tokio::test]
    async fn http_cancel_remains_available_after_resource_admission_revoked() {
        struct Switch(AtomicBool);
        impl Admission for Switch {
            fn allows_image(&self) -> bool {
                self.0.load(Ordering::Acquire)
            }
        }
        let (fixture, _, _) = fixture(false, false, Duration::from_secs(30));
        let switch = Arc::new(Switch(AtomicBool::new(true)));
        let j = ImageJobs::configured(
            fixture.executor.clone(),
            switch.clone(),
            Duration::from_secs(30),
            fixture_verify,
            true,
        );
        let app = web(j.clone());
        let s = session(&j);
        let key = submit(&j, &s, &uuid::Uuid::new_v4().to_string());
        phase(&j, &s, &key, "running").await;
        switch.0.store(false, Ordering::Release);
        assert_eq!(j.capabilities()["available"], false);
        assert_eq!(
            http(&app, json!({"operation":"cancel","session":s,"job_id":key}))
                .await
                .status(),
            200
        );
        phase(&j, &s, &key, "cancelled").await;
    }
    struct Ready;
    impl Admission for Ready {
        fn allows_image(&self) -> bool {
            true
        }
    }
    struct Fake {
        calls: Arc<std::sync::atomic::AtomicUsize>,
        finish: Arc<tokio::sync::Notify>,
        uncertain: bool,
        ignore_cancel: bool,
    }
    impl Executor for Fake {
        fn available(&self) -> bool {
            true
        }
        fn generate(
            &self,
            _: ImageRequest,
            mut cancel: watch::Receiver<bool>,
            stage: watch::Sender<ImageStage>,
        ) -> Work {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let done = self.finish.clone();
            let uncertain = self.uncertain;
            let ignore = self.ignore_cancel;
            Box::pin(async move {
                stage.send_replace(ImageStage::Running);
                if ignore {
                    std::future::pending::<()>().await;
                }
                tokio::select! {_=done.notified()=>if uncertain{Err(Failure::Uncertain)}else{Ok(b"fixture-png-bytes-not-a-real-decoder".to_vec())},_=cancel.changed()=>Err(Failure::Cancelled)}
            })
        }
    }
    fn fixture_verify(raw: &[u8], _: &ImageRequest) -> Result<Value, ()> {
        if raw == b"fixture-png-bytes-not-a-real-decoder" {
            Ok(json!({"validation":"MOCK ONLY"}))
        } else {
            Err(())
        }
    }
    fn fixture(
        uncertain: bool,
        ignore: bool,
        limit: Duration,
    ) -> (
        Arc<ImageJobs>,
        Arc<tokio::sync::Notify>,
        Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let finish = Arc::new(tokio::sync::Notify::new());
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let executor = Arc::new(Fake {
            calls: calls.clone(),
            finish: finish.clone(),
            uncertain,
            ignore_cancel: ignore,
        });
        (
            ImageJobs::configured(executor, Arc::new(Ready), limit, fixture_verify, true),
            finish,
            calls,
        )
    }
    fn value(r: Result<Reply, Error>) -> Value {
        match r.unwrap() {
            Reply::Json(v) => v,
            _ => panic!("expected JSON"),
        }
    }
    fn session(j: &Arc<ImageJobs>) -> String {
        value(j.control(Command::Session {}))["session"]
            .as_str()
            .unwrap()
            .into()
    }
    fn request() -> ImageRequest {
        ImageRequest {
            prompt: "synthetic blue cube".into(),
            width: 512,
            height: 512,
            seed: 42,
        }
    }
    fn submit(j: &Arc<ImageJobs>, s: &str, id: &str) -> String {
        value(j.control(Command::Submit {
            session: s.into(),
            request_id: id.into(),
            request: request(),
        }))["job_id"]
            .as_str()
            .unwrap()
            .into()
    }
    fn status(j: &Arc<ImageJobs>, s: &str, id: &str) -> Value {
        value(j.control(Command::Status {
            session: s.into(),
            job_id: id.into(),
        }))
    }
    async fn phase(j: &Arc<ImageJobs>, s: &str, id: &str, p: &str) {
        for _ in 0..200 {
            if status(j, s, id)["phase"] == p {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("phase {p}: {}", status(j, s, id));
    }
    #[tokio::test]
    async fn idempotent_submit_state_and_artifact_are_owned_and_bounded() {
        let (j, done, calls) = fixture(false, false, Duration::from_secs(30));
        let s = session(&j);
        let id = uuid::Uuid::new_v4().to_string();
        let a = submit(&j, &s, &id);
        assert_eq!(a, submit(&j, &s, &id));
        phase(&j, &s, &a, "running").await;
        assert_eq!(status(&j, &s, &a)["progress_percent"], Value::Null);
        let other = session(&j);
        assert!(matches!(
            j.control(Command::Artifact {
                session: other,
                job_id: a.clone()
            }),
            Err(Error::Unknown)
        ));
        let mut changed = request();
        changed.seed = 1;
        assert!(matches!(
            j.control(Command::Submit {
                session: s.clone(),
                request_id: id,
                request: changed
            }),
            Err(Error::Invalid)
        ));
        done.notify_one();
        phase(&j, &s, &a, "completed").await;
        let Reply::Png(png) = j
            .control(Command::Artifact {
                session: s,
                job_id: a,
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(png.metadata["validation"], "MOCK ONLY");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn cancelling_waits_for_confirmation_and_late_bytes_are_not_published() {
        let (j, _, _) = fixture(false, false, Duration::from_secs(30));
        let s = session(&j);
        let a = submit(&j, &s, &uuid::Uuid::new_v4().to_string());
        phase(&j, &s, &a, "running").await;
        assert_eq!(
            value(j.control(Command::Cancel {
                session: s.clone(),
                job_id: a.clone()
            }))["phase"],
            "cancelling"
        );
        phase(&j, &s, &a, "cancelled").await;
        assert!(matches!(
            j.control(Command::Artifact {
                session: s,
                job_id: a
            }),
            Err(Error::NotReady)
        ));
    }
    #[tokio::test(start_paused = true)]
    async fn unresponsive_cancel_quarantines_and_no_queued_executor_starts() {
        let (j, _, calls) = fixture(false, true, Duration::from_secs(1));
        let s = session(&j);
        let a = submit(&j, &s, &uuid::Uuid::new_v4().to_string());
        phase(&j, &s, &a, "running").await;
        let b = submit(&j, &s, &uuid::Uuid::new_v4().to_string());
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(11)).await;
        phase(&j, &s, &a, "quarantined").await;
        assert_ne!(status(&j, &s, &b)["phase"], "completed");
        assert_eq!(j.capabilities()["available"], false);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn uncertain_completion_closes_pool_and_capacity_has_no_overwrite() {
        let (j, done, calls) = fixture(true, false, Duration::from_secs(30));
        let s = session(&j);
        let a = submit(&j, &s, &uuid::Uuid::new_v4().to_string());
        phase(&j, &s, &a, "running").await;
        submit(&j, &s, &uuid::Uuid::new_v4().to_string());
        submit(&j, &s, &uuid::Uuid::new_v4().to_string());
        assert!(matches!(
            j.control(Command::Submit {
                session: s.clone(),
                request_id: uuid::Uuid::new_v4().to_string(),
                request: request()
            }),
            Err(Error::Capacity)
        ));
        done.notify_one();
        phase(&j, &s, &a, "quarantined").await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(j.capabilities()["quarantined"], true);
    }
    #[tokio::test]
    async fn blocked_and_expired_sessions_do_not_dispatch() {
        let (j, _, calls) = fixture(false, false, Duration::from_secs(30));
        let s = session(&j);
        j.registry
            .lock()
            .unwrap()
            .sessions
            .insert(s.clone(), Instant::now() - Duration::from_secs(1801));
        assert!(matches!(
            j.control(Command::List { session: s }),
            Err(Error::Unknown)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let disabled = ImageJobs::new(j.executor.clone(), Arc::new(Blocked));
        let s = session(&disabled);
        assert_eq!(disabled.capabilities()["available"], false);
        assert!(matches!(
            disabled.control(Command::Submit {
                session: s,
                request_id: uuid::Uuid::new_v4().to_string(),
                request: request()
            }),
            Err(Error::Unavailable)
        ));
    }
    #[test]
    fn image_commands_do_not_accept_paths_models_or_other_protocols() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../contracts/image-jobs/v0.1.0/commands.json"
        ))
        .unwrap();
        for v in fixture["examples"].as_array().unwrap() {
            assert!(serde_json::from_value::<Command>(v.clone()).is_ok());
        }
        for raw in [
            r#"{"operation":"session","allow":true}"#,
            r#"{"model":"pegassus","messages":[]}"#,
            r#"{"operation":"submit","session":"x","request_id":"x","request":{"prompt":"x","width":512,"height":512,"seed":42,"output":"/file"}}"#,
        ] {
            assert!(serde_json::from_str::<Command>(raw).is_err());
        }
    }
    #[tokio::test]
    async fn artifact_capacity_and_rejected_verification_do_not_publish_bytes() {
        let (j, done, _) = fixture(false, false, Duration::from_secs(30));
        j.registry.lock().unwrap().artifact_bytes = MAX_ARTIFACT_BYTES;
        let s = session(&j);
        let key = submit(&j, &s, &uuid::Uuid::new_v4().to_string());
        phase(&j, &s, &key, "running").await;
        done.notify_one();
        phase(&j, &s, &key, "failed").await;
        assert_eq!(status(&j, &s, &key)["error"], "artifact_capacity");
        assert_eq!(
            j.registry.lock().unwrap().artifact_bytes,
            MAX_ARTIFACT_BYTES
        );
        let (rejected, done, _) = fixture(false, false, Duration::from_secs(30));
        let rejected = ImageJobs::configured(
            rejected.executor.clone(),
            Arc::new(Ready),
            Duration::from_secs(30),
            |_, _| Err(()),
            true,
        );
        let s = session(&rejected);
        let key = submit(&rejected, &s, &uuid::Uuid::new_v4().to_string());
        phase(&rejected, &s, &key, "running").await;
        done.notify_one();
        phase(&rejected, &s, &key, "failed").await;
        assert!(matches!(
            rejected.control(Command::Artifact {
                session: s,
                job_id: key
            }),
            Err(Error::NotReady)
        ));
    }
    #[tokio::test]
    async fn late_success_after_user_cancel_is_discarded() {
        struct Late(Arc<tokio::sync::Notify>);
        impl Executor for Late {
            fn available(&self) -> bool {
                true
            }
            fn generate(
                &self,
                _: ImageRequest,
                _: watch::Receiver<bool>,
                stage: watch::Sender<ImageStage>,
            ) -> Work {
                let n = self.0.clone();
                Box::pin(async move {
                    stage.send_replace(ImageStage::Running);
                    n.notified().await;
                    Ok(b"fixture-png-bytes-not-a-real-decoder".to_vec())
                })
            }
        }
        let done = Arc::new(tokio::sync::Notify::new());
        let j = ImageJobs::configured(
            Arc::new(Late(done.clone())),
            Arc::new(Ready),
            Duration::from_secs(30),
            fixture_verify,
            true,
        );
        let s = session(&j);
        let key = submit(&j, &s, &uuid::Uuid::new_v4().to_string());
        phase(&j, &s, &key, "running").await;
        value(j.control(Command::Cancel {
            session: s.clone(),
            job_id: key.clone(),
        }));
        done.notify_one();
        phase(&j, &s, &key, "cancelled").await;
        assert_eq!(j.registry.lock().unwrap().artifact_bytes, 0);
        assert!(matches!(
            j.control(Command::Artifact {
                session: s,
                job_id: key
            }),
            Err(Error::NotReady)
        ));
        // A different task's drop/quarantine cancels this run too. Even a late
        // successful executor cannot publish after that pool cancellation.
        let done = Arc::new(tokio::sync::Notify::new());
        let j = ImageJobs::configured(
            Arc::new(Late(done.clone())),
            Arc::new(Ready),
            Duration::from_secs(30),
            fixture_verify,
            true,
        );
        let s = session(&j);
        let key = submit(&j, &s, &uuid::Uuid::new_v4().to_string());
        phase(&j, &s, &key, "running").await;
        j.quarantine("synthetic-other-run", "fixture_drop");
        done.notify_one();
        phase(&j, &s, &key, "blocked").await;
        assert_eq!(j.registry.lock().unwrap().artifact_bytes, 0);
        assert!(matches!(
            j.control(Command::Artifact {
                session: s,
                job_id: key
            }),
            Err(Error::NotReady)
        ));
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn actual_decoder_accepts_synthetic_png_and_rejects_corruption_without_models() {
        fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut b = (data.len() as u32).to_be_bytes().to_vec();
            b.extend(kind);
            b.extend(data);
            let mut crc = !0u32;
            for x in &b[4..] {
                crc ^= u32::from(*x);
                for _ in 0..8 {
                    crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
                }
            }
            b.extend((!crc).to_be_bytes());
            b
        }
        let mut row = vec![0u8];
        for x in 0..512 {
            row.extend(if x == 0 { [255, 0, 0] } else { [0, 0, 255] });
        }
        let raw = row.repeat(512);
        let mut z = vec![0x78, 0x01];
        let count = raw.chunks(65535).len();
        for (i, block) in raw.chunks(65535).enumerate() {
            z.push(u8::from(i + 1 == count));
            let n = block.len() as u16;
            z.extend(n.to_le_bytes());
            z.extend((!n).to_le_bytes());
            z.extend(block);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for x in raw {
            a = (a + u32::from(x)) % 65521;
            b = (b + a) % 65521;
        }
        z.extend(((b << 16) | a).to_be_bytes());
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut h = 512u32.to_be_bytes().to_vec();
        h.extend(512u32.to_be_bytes());
        h.extend([8, 2, 0, 0, 0]);
        png.extend(chunk(b"IHDR", &h));
        png.extend(chunk(b"IDAT", &z));
        png.extend(chunk(b"IEND", &[]));
        let metadata = native_verify(&png, &request()).unwrap();
        assert_eq!(metadata["width"], 512);
        assert_eq!(metadata["sha256"].as_str().unwrap().len(), 64);
        let root = crate::job_store::tests::directory();
        let (mut durable, _, _) = fixture(false, false, Duration::from_secs(30));
        Arc::get_mut(&mut durable).unwrap().verify = native_verify;
        ImageJobs::attach_store(&mut durable, &root).unwrap();
        let s = session(&durable);
        let key = uuid::Uuid::new_v4().to_string();
        {
            let mut state = durable.registry.lock().unwrap();
            let (cancel, _) = watch::channel(false);
            state.artifact_bytes = png.len();
            state.jobs.insert(
                key.clone(),
                Job {
                    owner: s.clone(),
                    request_id: uuid::Uuid::new_v4().to_string(),
                    request: request(),
                    phase: Phase::Completed,
                    cancel,
                    user_cancel: false,
                    artifact: Some(Arc::new(Artifact {
                        png: Bytes::from(png.clone()),
                        metadata: metadata.clone(),
                    })),
                    error: None,
                    deadline: tokio::time::Instant::now(),
                },
            );
            durable.checkpoint(&mut state).unwrap();
        }
        let executor = durable.executor.clone();
        drop(durable);
        let mut reopened = ImageJobs::configured(
            executor.clone(),
            Arc::new(Ready),
            Duration::from_secs(30),
            native_verify,
            true,
        );
        ImageJobs::attach_store(&mut reopened, &root).unwrap();
        assert_eq!(status(&reopened, &s, &key)["phase"], "completed");
        drop(reopened);
        let store = crate::job_store::Store::open(&root).unwrap();
        let (manifest, mut blobs) = store.load().unwrap().unwrap();
        blobs.get_mut(&key).unwrap()[20] ^= 1;
        store
            .save(
                manifest,
                blobs
                    .iter()
                    .map(|(id, b)| (id.clone(), b.as_slice()))
                    .collect(),
            )
            .unwrap();
        drop(store);
        let before = std::fs::read(root.join("snapshot.bin")).unwrap();
        let mut corrupt = ImageJobs::configured(
            executor,
            Arc::new(Ready),
            Duration::from_secs(30),
            native_verify,
            true,
        );
        assert!(ImageJobs::attach_store(&mut corrupt, &root).is_err());
        assert_eq!(std::fs::read(root.join("snapshot.bin")).unwrap(), before);
        drop(corrupt);
        std::fs::remove_dir_all(root).unwrap();
        png[20] ^= 1;
        assert!(native_verify(&png, &request()).is_err());
        assert!(native_verify(b"<svg/>", &request()).is_err());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn native_permission_denied_cannot_be_advertised_by_positive_resource_admission() {
        let executor = native::NativeImage::new(
            seiya_observe::image::job::RuntimeFiles {
                binary: "/not-read".into(),
                assets: "/not-read".into(),
            },
            Arc::new(seiya_observe::image::job::Denied),
        );
        let jobs = ImageJobs::new(Arc::new(executor), Arc::new(Ready));
        assert_eq!(jobs.capabilities()["available"], false);
    }
}
