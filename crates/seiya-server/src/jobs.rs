//! Opt-in code drafts only. No shell, workspace writes, model loading or
//! authority from prompts. A real executor needs an owned, admitted runtime.
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
use uuid::Uuid;

pub type Work = Pin<Box<dyn Future<Output = Result<String, Failure>> + Send>>;
#[derive(Debug)]
pub enum Failure {
    Confirmed,
    Uncertain,
}
pub trait Executor: Send + Sync + 'static {
    /// Cancellation is a request; only returning confirms that work ended.
    /// An uncertain transport must return Uncertain, never Confirmed.
    fn draft(&self, prompt: String, cancel: watch::Receiver<bool>) -> Work;
}
pub trait Admission: Send + Sync + 'static {
    fn allows_pool(&self, auxiliaries: usize) -> bool;
}
pub struct Blocked;
impl Admission for Blocked {
    fn allows_pool(&self, _: usize) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Generating,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
    Blocked,
    Quarantined,
}
impl Phase {
    fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Generating | Self::Cancelling)
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
        prompt: String,
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
struct Job {
    owner: String,
    request: String,
    prompt: String,
    phase: Phase,
    artifact: Option<String>,
    cancel: watch::Sender<bool>,
    deadline: tokio::time::Instant,
}
struct Registry {
    tasks: std::collections::BTreeSet<String>,
    conversations: BTreeMap<String, String>,
    sessions: BTreeMap<String, Instant>,
    jobs: BTreeMap<String, Job>,
}
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
    request: String,
    prompt: String,
    phase: Phase,
    artifact: Option<String>,
    cancel: bool,
}
pub struct Jobs {
    store: Option<crate::job_store::Store>,
    storage_failed: AtomicBool,
    registry: Mutex<Registry>,
    executor: Arc<dyn Executor>,
    admission: Arc<dyn Admission>,
    auxiliary: Arc<Semaphore>,
    quarantined: AtomicBool,
    limit: Duration,
    parallelism: usize,
}

// Own the slot inside this guard: panic/drop closes admission BEFORE a permit
// could become reusable. There is no orphaned "generating forever" state.
struct RunLease {
    jobs: Arc<Jobs>,
    id: String,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
    confirmed: bool,
}
impl Drop for RunLease {
    fn drop(&mut self) {
        if self.confirmed {
            self.jobs
                .registry
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .tasks
                .remove(&self.id);
            return;
        }
        self.jobs.quarantined.store(true, Ordering::Release);
        self.jobs.auxiliary.close();
        let mut state = self.jobs.registry.lock().unwrap_or_else(|p| p.into_inner());
        for (id, job) in state.jobs.iter_mut() {
            if id == &self.id {
                job.phase = Phase::Quarantined;
                job.artifact = None;
            } else if job.phase.active() {
                job.cancel.send_replace(true);
                job.phase = if job.phase == Phase::Queued {
                    Phase::Blocked
                } else {
                    Phase::Cancelling
                };
            }
        }
        let _ = self.jobs.checkpoint(&mut state);
        state.tasks.remove(&self.id);
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unknown,
    Capacity,
    NotReady,
    Storage,
}
/// Candidate capacity from disjoint OPERATOR reservations, not RSS/Metal sums.
/// This calculation never grants admission, probes memory or loads a model.
pub fn planned_parallelism(
    physical: u64,
    host: u64,
    foreground: u64,
    per_auxiliary: u64,
    runtime_cap: usize,
) -> Result<usize, Error> {
    if physical == 0
        || host == 0
        || foreground == 0
        || per_auxiliary == 0
        || !(1..=4).contains(&runtime_cap)
    {
        return Err(Error::Invalid);
    }
    let available = physical.saturating_sub(host).saturating_sub(foreground);
    Ok((available / per_auxiliary).min(runtime_cap as u64) as usize)
}
fn uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
fn view(id: &str, job: &Job) -> Value {
    json!({"job_id":id,"request_id":job.request,"phase":job.phase,"cancel_requested":*job.cancel.borrow(),
        "artifact_available":job.phase == Phase::Completed,"tests_executed":false,
        "workspace_written":false,"progress_percent":null})
}
impl Jobs {
    pub(crate) fn foreground_allowed(&self) -> bool {
        !self.quarantined.load(Ordering::Acquire) && !self.storage_failed.load(Ordering::Acquire)
    }
    pub fn persistent(
        executor: Arc<dyn Executor>,
        admission: Arc<dyn Admission>,
        limit: Duration,
        root: &std::path::Path,
    ) -> Result<Arc<Self>, String> {
        let mut jobs = Self::new(executor, admission, limit).map_err(|_| "job_config")?;
        Self::attach_store(&mut jobs, root)?;
        Ok(jobs)
    }
    pub(crate) fn attach_store(jobs: &mut Arc<Self>, root: &std::path::Path) -> Result<(), String> {
        let store = crate::job_store::Store::open(root)?;
        let own = Arc::get_mut(jobs).ok_or("attach_jobs_before_sharing")?;
        if let Some((manifest, blobs)) = store.load()? {
            if !blobs.is_empty() {
                return Err("draft_snapshot_unexpected_blobs".into());
            }
            let saved: Saved =
                serde_json::from_value(manifest).map_err(|_| "draft_snapshot_schema")?;
            if saved.version != 1
                || saved.jobs.len() > 64
                || saved.sessions.len() > 32
                || saved.conversations.len() > 32
            {
                return Err("draft_snapshot_limits".into());
            }
            let mut state = Registry {
                tasks: Default::default(),
                sessions: BTreeMap::new(),
                conversations: saved.conversations,
                jobs: BTreeMap::new(),
            };
            for t in saved.sessions {
                if t.len() != 64
                    || !t
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                    || state.sessions.insert(t, Instant::now()).is_some()
                {
                    return Err("draft_snapshot_session".into());
                }
            }
            if state
                .conversations
                .iter()
                .any(|(id, t)| !uuid(id) || !state.sessions.contains_key(t))
            {
                return Err("draft_snapshot_conversation".into());
            }
            let mut quarantine = saved.quarantined;
            for r in saved.jobs {
                if !uuid(&r.id)
                    || !uuid(&r.request)
                    || !state.sessions.contains_key(&r.owner)
                    || r.prompt.trim().is_empty()
                    || r.prompt.len() > 8192
                    || state.jobs.contains_key(&r.id)
                    || state
                        .jobs
                        .values()
                        .any(|j: &Job| j.owner == r.owner && j.request == r.request)
                {
                    return Err("draft_snapshot_record".into());
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
                if phase == Phase::Completed
                    && r.artifact
                        .as_ref()
                        .is_none_or(|t| t.trim().is_empty() || t.len() > 65536)
                    || phase != Phase::Completed && r.artifact.is_some()
                {
                    return Err("draft_snapshot_artifact".into());
                }
                let (cancel, _) = watch::channel(
                    r.cancel || phase == Phase::Cancelled || phase == Phase::Quarantined,
                );
                state.jobs.insert(
                    r.id,
                    Job {
                        owner: r.owner,
                        request: r.request,
                        prompt: r.prompt,
                        phase,
                        artifact: r.artifact,
                        cancel,
                        deadline: tokio::time::Instant::now(),
                    },
                );
            }
            own.registry = Mutex::new(state);
            own.quarantined.store(quarantine, Ordering::Release);
            if quarantine {
                own.auxiliary.close();
            }
        }
        own.store = Some(store);
        own.checkpoint(&mut own.registry.lock().unwrap())
            .map_err(|_| "draft_snapshot_checkpoint".to_owned())?;
        Ok(())
    }
    pub(crate) fn durable(&self) -> bool {
        self.store.is_some()
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
                    request: j.request.clone(),
                    prompt: j.prompt.clone(),
                    phase: j.phase,
                    artifact: j.artifact.clone(),
                    cancel: *j.cancel.borrow(),
                })
                .collect(),
        };
        if store
            .save(
                serde_json::to_value(saved).map_err(|_| Error::Storage)?,
                vec![],
            )
            .is_err()
        {
            self.storage_failed.store(true, Ordering::Release);
            self.auxiliary.close();
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
    pub fn new(
        executor: Arc<dyn Executor>,
        admission: Arc<dyn Admission>,
        limit: Duration,
    ) -> Result<Arc<Self>, Error> {
        Self::with_parallelism(executor, admission, limit, 1)
    }
    /// Operator limit, not a model/browser permission or inference that RAM fits.
    /// Admission must validate the entire foreground + auxiliary pool.
    pub fn with_parallelism(
        executor: Arc<dyn Executor>,
        admission: Arc<dyn Admission>,
        limit: Duration,
        parallelism: usize,
    ) -> Result<Arc<Self>, Error> {
        if limit.is_zero() || limit > Duration::from_secs(180) {
            return Err(Error::Invalid);
        }
        if !(1..=4).contains(&parallelism) {
            return Err(Error::Invalid);
        }
        Ok(Arc::new(Self {
            store: None,
            storage_failed: AtomicBool::new(false),
            registry: Mutex::new(Registry {
                tasks: Default::default(),
                conversations: BTreeMap::new(),
                sessions: BTreeMap::new(),
                jobs: BTreeMap::new(),
            }),
            executor,
            admission,
            auxiliary: Arc::new(Semaphore::new(parallelism)),
            quarantined: AtomicBool::new(false),
            limit,
            parallelism,
        }))
    }
    fn command(self: &Arc<Self>, command: Command) -> Result<Value, Error> {
        let mut state = self.registry.lock().unwrap();
        if let Command::Open { conversation_id } = command {
            if !uuid(&conversation_id) {
                return Err(Error::Invalid);
            }
            let token = if let Some(t) = state.conversations.get(&conversation_id) {
                t.clone()
            } else {
                if state.sessions.len() >= 32 {
                    return Err(Error::Capacity);
                }
                let t = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
                state.conversations.insert(conversation_id, t.clone());
                t
            };
            state.sessions.insert(token.clone(), Instant::now());
            return Ok(json!({"session":token,"volatile":self.store.is_none(),"ttl_seconds":1800}));
        }
        if matches!(command, Command::Session { .. }) {
            if state.sessions.len() >= 32 {
                return Err(Error::Capacity);
            }
            let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
            state.sessions.insert(token.clone(), Instant::now());
            return Ok(json!({"session":token,"volatile":self.store.is_none(),"ttl_seconds":1800}));
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
        if session.len() != 64
            || state
                .sessions
                .get(session)
                .is_none_or(|used| used.elapsed() >= Duration::from_secs(1800))
        {
            return Err(Error::Unknown);
        }
        // An active page keeps its capability alive. A newly submitted job
        // cannot lose its status/cancel capability halfway through its <=180s.
        state.sessions.insert(session.clone(), Instant::now());
        match command {
            Command::Submit {
                session,
                request_id,
                prompt,
            } => {
                if !uuid(&request_id) || prompt.trim().is_empty() || prompt.len() > 8192 {
                    return Err(Error::Invalid);
                }
                if let Some((id, job)) = state
                    .jobs
                    .iter()
                    .find(|(_, j)| j.owner == session && j.request == request_id)
                {
                    if job.prompt != prompt {
                        return Err(Error::Invalid);
                    }
                    return Ok(view(id, job));
                }
                if state.jobs.len() >= 64
                    || state.jobs.values().filter(|j| j.phase.active()).count()
                        >= self.parallelism + 4
                {
                    return Err(Error::Capacity);
                }
                let id = Uuid::new_v4().to_string();
                let (cancel, receiver) = watch::channel(false);
                let job = Job {
                    owner: session,
                    request: request_id,
                    prompt,
                    phase: Phase::Queued,
                    artifact: None,
                    cancel,
                    deadline: tokio::time::Instant::now() + self.limit,
                };
                let result = view(&id, &job);
                let runtime = tokio::runtime::Handle::try_current().map_err(|_| Error::NotReady)?;
                state.jobs.insert(id.clone(), job);
                self.checkpoint(&mut state)?;
                state.tasks.insert(id.clone());
                let service = self.clone();
                runtime.spawn(async move {
                    service.run(id, receiver).await;
                });
                Ok(result)
            }
            Command::List { session } => Ok(
                json!({"jobs":state.jobs.iter().filter(|(_,j)| j.owner == session).map(|(id,j)| view(id,j)).collect::<Vec<_>>(),
                "foreground_allowed":!self.quarantined.load(Ordering::Acquire),"auxiliary_limit":self.parallelism,"volatile":self.store.is_none()}),
            ),
            Command::Status { session, job_id }
            | Command::Cancel { session, job_id }
            | Command::Artifact { session, job_id } => {
                let job = state
                    .jobs
                    .get_mut(&job_id)
                    .filter(|j| j.owner == session)
                    .ok_or(Error::Unknown)?;
                // Actual action dispatch below uses a helper to keep owner checks identical.
                Ok(view(&job_id, job))
            }
            Command::Forget { session } => {
                if state
                    .tasks
                    .iter()
                    .any(|id| state.jobs.get(id).is_some_and(|j| j.owner == session))
                {
                    return Err(Error::NotReady);
                }
                if state.jobs.values().any(|j| {
                    j.owner == session && (j.phase.active() || j.phase == Phase::Quarantined)
                }) {
                    return Err(Error::NotReady);
                }
                state.jobs.retain(|_, j| j.owner != session);
                state.sessions.remove(&session);
                state.conversations.retain(|_, t| *t != session);
                Ok(json!({"forgotten":true}))
            }
            Command::Session { .. } | Command::Open { .. } => unreachable!(),
        }
    }
    pub fn control(self: &Arc<Self>, command: Command) -> Result<Value, Error> {
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
        let result = self.control_inner(command)?;
        if persist {
            self.checkpoint(&mut self.registry.lock().unwrap())?;
        }
        if self.storage_failed.load(Ordering::Acquire) {
            return Err(Error::Storage);
        }
        Ok(result)
    }
    fn control_inner(self: &Arc<Self>, command: Command) -> Result<Value, Error> {
        let action = match &command {
            Command::Cancel { .. } => 1,
            Command::Artifact { .. } => 2,
            _ => 0,
        };
        if action == 0 {
            return self.command(command);
        }
        let (session, id) = match command {
            Command::Cancel { session, job_id } | Command::Artifact { session, job_id } => {
                (session, job_id)
            }
            _ => unreachable!(),
        };
        // Verify capability/expiry before looking at any data or effects.
        self.command(Command::Status {
            session: session.clone(),
            job_id: id.clone(),
        })?;
        let mut state = self.registry.lock().unwrap();
        let job = state
            .jobs
            .get_mut(&id)
            .filter(|j| j.owner == session)
            .ok_or(Error::Unknown)?;
        if action == 2 {
            if job.phase != Phase::Completed {
                return Err(Error::NotReady);
            }
            return Ok(
                json!({"job_id":id,"filename":"borrador.txt","text":job.artifact,
                "untrusted":true,"tests_executed":false,"workspace_written":false}),
            );
        }
        if job.phase.active() {
            job.cancel.send_replace(true);
            job.phase = if job.phase == Phase::Queued {
                Phase::Cancelled
            } else {
                Phase::Cancelling
            };
        }
        Ok(view(&id, job))
    }
    async fn run(self: Arc<Self>, id: String, mut cancel: watch::Receiver<bool>) {
        let mut lease = RunLease {
            jobs: self.clone(),
            id: id.clone(),
            permit: None,
            confirmed: false,
        };
        let deadline = self.registry.lock().unwrap().jobs[&id].deadline;
        let permit = tokio::select! {
            result = self.auxiliary.clone().acquire_owned() => result.ok(),
            _ = tokio::time::sleep_until(deadline) => None,
            _ = cancel.changed() => None,
        };
        let Some(permit) = permit else {
            let mut state = self.registry.lock().unwrap();
            let job = state.jobs.get_mut(&id).unwrap();
            job.phase = if self.quarantined.load(Ordering::Acquire) {
                Phase::Blocked
            } else if *cancel.borrow() {
                Phase::Cancelled
            } else {
                Phase::Failed
            };
            lease.confirmed = true;
            let _ = self.checkpoint(&mut state);
            return;
        };
        lease.permit = Some(permit);
        // A host admission callback must be a quick snapshot, never a blocking
        // probe. Do not invoke foreign code while holding registry state.
        let admitted = self.admission.allows_pool(self.parallelism);
        let prompt = {
            let mut state = self.registry.lock().unwrap();
            let job = state.jobs.get_mut(&id).unwrap();
            if *cancel.borrow() {
                job.phase = Phase::Cancelled;
                lease.confirmed = true;
                let _ = self.checkpoint(&mut state);
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                job.phase = Phase::Failed;
                lease.confirmed = true;
                let _ = self.checkpoint(&mut state);
                return;
            }
            if self.quarantined.load(Ordering::Acquire) || !admitted {
                job.phase = Phase::Blocked;
                lease.confirmed = true;
                let _ = self.checkpoint(&mut state);
                return;
            }
            job.phase = Phase::Generating;
            let prompt = job.prompt.clone();
            if self.checkpoint(&mut state).is_err() {
                lease.confirmed = true;
                return;
            }
            prompt
        };
        let outcome =
            tokio::time::timeout_at(deadline, self.executor.draft(prompt, cancel.clone())).await;
        let mut state = self.registry.lock().unwrap();
        let job = state.jobs.get_mut(&id).unwrap();
        match outcome {
            Ok(Ok(text)) if text.len() <= 65536 && !text.trim().is_empty() => {
                if *cancel.borrow() {
                    job.phase = Phase::Cancelled;
                } else {
                    job.phase = Phase::Completed;
                    job.artifact = Some(text);
                }
            }
            Ok(Ok(_)) | Ok(Err(Failure::Confirmed)) => {
                job.phase = if *cancel.borrow() {
                    Phase::Cancelled
                } else {
                    Phase::Failed
                }
            }
            _ => {
                job.phase = Phase::Quarantined;
                self.quarantined.store(true, Ordering::Release);
                self.auxiliary.close();
                // Keep the pool closed and the uncertain slot unreleased.
                lease.permit.take().unwrap().forget();
            }
        }
        if self.quarantined.load(Ordering::Acquire) {
            if let Some(job) = state.jobs.get_mut(&id) {
                if job.phase == Phase::Completed {
                    job.phase = Phase::Blocked;
                    job.artifact = None;
                }
            }
            for other in state.jobs.values_mut().filter(|j| j.phase.active()) {
                other.cancel.send_replace(true);
                other.phase = if other.phase == Phase::Queued {
                    Phase::Blocked
                } else {
                    Phase::Cancelling
                };
            }
        }
        lease.confirmed = true;
        let _ = self.checkpoint(&mut state);
    }
}

/// Explicit fixture only, never a model or a performance/quality measurement.
pub struct Demo;
pub struct DemoAdmission;
impl Admission for DemoAdmission {
    fn allows_pool(&self, _: usize) -> bool {
        true
    }
}
impl Executor for Demo {
    fn draft(&self, _prompt: String, mut cancel: watch::Receiver<bool>) -> Work {
        Box::pin(async move {
            tokio::select! { _ = tokio::time::sleep(Duration::from_secs(12)) => {}, _ = cancel.changed() => return Err(Failure::Confirmed) }
            Ok("# Borrador SIMULADO, no generado por Qwen y no probado.\n# La integración real todavía requiere admisión del par.\nprint('Vista previa de Seiya')\n".into())
        })
    }
}

/// Operator-injected local OpenAI-compatible draft transport. Never starts or
/// unloads a model. The host Admission must prove ownership/resources separately.
pub struct HttpDraft {
    client: reqwest::Client,
    endpoint: String,
    model: String,
}
impl HttpDraft {
    pub fn new(endpoint: String, model: String) -> Result<Self, Error> {
        let url = reqwest::Url::parse(&endpoint).map_err(|_| Error::Invalid)?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.port().is_none()
            || url.path() != "/v1/chat/completions"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || model.is_empty()
            || model.len() > 256
            || model.chars().any(char::is_control)
        {
            return Err(Error::Invalid);
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|_| Error::Invalid)?;
        Ok(Self {
            client,
            endpoint,
            model,
        })
    }
}
impl Executor for HttpDraft {
    fn draft(&self, prompt: String, _cancel: watch::Receiver<bool>) -> Work {
        let client = self.client.clone();
        let endpoint = self.endpoint.clone();
        let model = self.model.clone();
        Box::pin(async move {
            let mut reply=client.post(endpoint).json(&json!({"model":model,"stream":false,"max_tokens":256,
                "temperature":0,"reasoning_effort":"none","messages":[
                    {"role":"system","content":"Produce a source-code draft as plain text. Do not execute tools or claim files/tests were run. No private reasoning."},
                    {"role":"user","content":prompt}]})).send().await.map_err(|_| Failure::Uncertain)?;
            if !reply.status().is_success() {
                return Err(Failure::Uncertain);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = reply.chunk().await.map_err(|_| Failure::Uncertain)? {
                if chunk.len() > 131072usize.saturating_sub(bytes.len()) {
                    return Err(Failure::Uncertain);
                }
                bytes.extend_from_slice(&chunk);
            }
            decode_draft(&bytes, &model)
        })
    }
}

pub(crate) fn decode_draft(bytes: &[u8], model: &str) -> Result<String, Failure> {
    let data: Value = serde_json::from_slice(bytes).map_err(|_| Failure::Uncertain)?;
    if data["model"] != model
        || data["choices"].as_array().is_none_or(|v| v.len() != 1)
        || data["choices"][0]["message"]["role"] != "assistant"
    {
        return Err(Failure::Uncertain);
    }
    let output = data["usage"]["completion_tokens"]
        .as_u64()
        .ok_or(Failure::Uncertain)?;
    if output > 256
        || data["choices"][0]["finish_reason"] != "stop"
        || data["choices"][0]["message"]
            .get("tool_calls")
            .is_some_and(|v| !v.is_null() && v != &json!([]))
    {
        return Err(Failure::Confirmed);
    }
    let code = data["choices"][0]["message"]["content"]
        .as_str()
        .ok_or(Failure::Confirmed)?;
    if code.len() > 65536
        || code.trim().is_empty()
        || code.contains("<think>")
        || code.contains("</think>")
    {
        return Err(Failure::Confirmed);
    }
    Ok(code.to_owned())
}

#[cfg(test)]
mod draft_tests {
    use super::*;
    #[tokio::test]
    async fn queued_cancel_cannot_delete_record_before_owned_future_finishes() {
        let j = Jobs::new(Arc::new(Demo), Arc::new(Blocked), Duration::from_secs(10)).unwrap();
        let s = j.control(Command::Session {}).unwrap()["session"]
            .as_str()
            .unwrap()
            .to_owned();
        let key = j
            .control(Command::Submit {
                session: s.clone(),
                request_id: Uuid::new_v4().to_string(),
                prompt: "fixture".into(),
            })
            .unwrap()["job_id"]
            .as_str()
            .unwrap()
            .to_owned();
        j.control(Command::Cancel {
            session: s.clone(),
            job_id: key,
        })
        .unwrap();
        assert_eq!(
            j.control(Command::Forget { session: s.clone() }),
            Err(Error::NotReady)
        );
        tokio::task::yield_now().await;
        assert_eq!(
            j.control(Command::Forget { session: s }).unwrap()["forgotten"],
            true
        );
    }
    #[test]
    fn history_examples_are_closed_browser_commands() {
        let v: Value = serde_json::from_str(include_str!(
            "../../../contracts/openai-compat/v0.1.0/job-history.example.json"
        ))
        .unwrap();
        for c in v["commands"].as_array().unwrap() {
            assert!(serde_json::from_value::<Command>(c.clone()).is_ok());
        }
        assert!(serde_json::from_value::<Command>(
            json!({"operation":"open","conversation_id":Uuid::new_v4().to_string(),"allow":true})
        )
        .is_err());
    }
    #[tokio::test]
    async fn failed_draft_checkpoint_blocks_dispatch_and_preserves_previous_snapshot() {
        struct Never;
        impl Executor for Never {
            fn draft(&self, _: String, _: watch::Receiver<bool>) -> Work {
                panic!("disk failure cannot dispatch")
            }
        }
        let root = crate::job_store::tests::directory();
        let j = Jobs::persistent(
            Arc::new(Never),
            Arc::new(DemoAdmission),
            Duration::from_secs(10),
            &root,
        )
        .unwrap();
        let s = j.control(Command::Session {}).unwrap()["session"]
            .as_str()
            .unwrap()
            .to_owned();
        let moved = root.with_extension("moved");
        std::fs::rename(&root, &moved).unwrap();
        assert_eq!(
            j.control(Command::Submit {
                session: s,
                request_id: Uuid::new_v4().to_string(),
                prompt: "fixture".into()
            }),
            Err(Error::Storage)
        );
        assert!(!j.foreground_allowed());
        drop(j);
        assert!(moved.join("snapshot.bin").is_file());
        std::fs::remove_dir_all(moved).unwrap();
    }
    #[tokio::test]
    async fn durable_draft_recovery_and_interrupted_pool_never_replay() {
        struct Never;
        impl Executor for Never {
            fn draft(&self, _: String, _: watch::Receiver<bool>) -> Work {
                panic!("restart must never execute")
            }
        }
        let root = crate::job_store::tests::directory();
        let j = Jobs::persistent(
            Arc::new(Never),
            Arc::new(Blocked),
            Duration::from_secs(10),
            &root,
        )
        .unwrap();
        let conversation = Uuid::new_v4().to_string();
        let s = j
            .control(Command::Open {
                conversation_id: conversation.clone(),
            })
            .unwrap()["session"]
            .as_str()
            .unwrap()
            .to_owned();
        let key = Uuid::new_v4().to_string();
        {
            let mut state = j.registry.lock().unwrap();
            let (cancel, _) = watch::channel(false);
            state.jobs.insert(
                key.clone(),
                Job {
                    owner: s.clone(),
                    request: Uuid::new_v4().to_string(),
                    prompt: "fixture".into(),
                    phase: Phase::Completed,
                    artifact: Some("synthetic draft not executed".into()),
                    cancel,
                    deadline: tokio::time::Instant::now(),
                },
            );
            j.checkpoint(&mut state).unwrap();
        }
        drop(j);
        let j = Jobs::persistent(
            Arc::new(Never),
            Arc::new(Blocked),
            Duration::from_secs(10),
            &root,
        )
        .unwrap();
        assert_eq!(
            j.control(Command::Open {
                conversation_id: conversation
            })
            .unwrap()["session"],
            s
        );
        assert_eq!(
            j.control(Command::Artifact {
                session: s.clone(),
                job_id: key.clone()
            })
            .unwrap()["text"],
            "synthetic draft not executed"
        );
        {
            let mut state = j.registry.lock().unwrap();
            let job = state.jobs.get_mut(&key).unwrap();
            job.phase = Phase::Generating;
            job.artifact = None;
            j.checkpoint(&mut state).unwrap();
        }
        drop(j);
        let j = Jobs::persistent(
            Arc::new(Never),
            Arc::new(Blocked),
            Duration::from_secs(10),
            &root,
        )
        .unwrap();
        assert!(!j.foreground_allowed());
        assert_eq!(
            j.control(Command::Status {
                session: s,
                job_id: key
            })
            .unwrap()["phase"],
            "quarantined"
        );
        drop(j);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn queued_checkpoint_restarts_cancelled_and_can_be_forgotten() {
        let root = crate::job_store::tests::directory();
        let j = Jobs::persistent(
            Arc::new(Demo),
            Arc::new(Blocked),
            Duration::from_secs(10),
            &root,
        )
        .unwrap();
        let s = j.control(Command::Session {}).unwrap()["session"]
            .as_str()
            .unwrap()
            .to_owned();
        let key = Uuid::new_v4().to_string();
        {
            let mut state = j.registry.lock().unwrap();
            let (cancel, _) = watch::channel(false);
            state.jobs.insert(
                key.clone(),
                Job {
                    owner: s.clone(),
                    request: Uuid::new_v4().to_string(),
                    prompt: "fixture".into(),
                    phase: Phase::Queued,
                    artifact: None,
                    cancel,
                    deadline: tokio::time::Instant::now(),
                },
            );
            j.checkpoint(&mut state).unwrap();
        }
        drop(j);
        let j = Jobs::persistent(
            Arc::new(Demo),
            Arc::new(Blocked),
            Duration::from_secs(10),
            &root,
        )
        .unwrap();
        assert_eq!(
            j.control(Command::Status {
                session: s.clone(),
                job_id: key
            })
            .unwrap()["phase"],
            "cancelled"
        );
        assert!(j.foreground_allowed());
        assert_eq!(
            j.control(Command::Forget { session: s }).unwrap()["forgotten"],
            true
        );
        drop(j);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn session_idle_expiry_does_not_expire_an_actively_observed_job() {
        let jobs = Jobs::new(Arc::new(Demo), Arc::new(Blocked), Duration::from_secs(10)).unwrap();
        let owner = jobs.control(Command::Session {}).unwrap()["session"]
            .as_str()
            .unwrap()
            .to_owned();
        jobs.registry
            .lock()
            .unwrap()
            .sessions
            .insert(owner.clone(), Instant::now() - Duration::from_secs(1700));
        assert!(jobs
            .control(Command::List {
                session: owner.clone()
            })
            .is_ok());
        assert!(jobs.registry.lock().unwrap().sessions[&owner].elapsed() < Duration::from_secs(1));
        jobs.registry
            .lock()
            .unwrap()
            .sessions
            .insert(owner.clone(), Instant::now() - Duration::from_secs(1801));
        assert_eq!(
            jobs.control(Command::List { session: owner }),
            Err(Error::Unknown)
        );
    }
    fn reply() -> Value {
        json!({"model":"synthetic","choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"print('draft')"}}],"usage":{"completion_tokens":4}})
    }
    #[test]
    fn response_identity_and_usage_are_required_not_permissions() {
        assert!(decode_draft(&serde_json::to_vec(&reply()).unwrap(), "synthetic").is_ok());
        let mut other = reply();
        other["model"] = json!("different");
        assert!(matches!(
            decode_draft(&serde_json::to_vec(&other).unwrap(), "synthetic"),
            Err(Failure::Uncertain)
        ));
        let mut absent = reply();
        absent["usage"] = Value::Null;
        assert!(matches!(
            decode_draft(&serde_json::to_vec(&absent).unwrap(), "synthetic"),
            Err(Failure::Uncertain)
        ));
    }
    #[test]
    fn truncation_tools_oversize_and_reasoning_do_not_become_artifacts() {
        for content in ["", "<think>private</think>code"] {
            let mut value = reply();
            value["choices"][0]["message"]["content"] = json!(content);
            assert!(decode_draft(&serde_json::to_vec(&value).unwrap(), "synthetic").is_err());
        }
        for kind in 0..4 {
            let mut value = reply();
            match kind {
                0 => value["choices"][0]["finish_reason"] = json!("length"),
                1 => {
                    value["choices"][0]["message"]["tool_calls"] =
                        json!([{"function":{"name":"shell"}}])
                }
                2 => value["choices"][0]["message"]["content"] = json!("x".repeat(65537)),
                _ => value["usage"]["completion_tokens"] = json!(257),
            }
            assert!(decode_draft(&serde_json::to_vec(&value).unwrap(), "synthetic").is_err());
        }
    }
}
