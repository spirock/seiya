//! One exclusive text turn; an ambiguous request leaves durable intent behind.
use super::{
    resources::{Native, Resources},
    storage, Catalog,
};
use crate::{
    prepared_web::HostGate,
    runtime::{HttpTransport, RuntimeFuture, RuntimeTransport, TransportError},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{watch, Semaphore};

type Probe<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, TransportError>> + Send + 'a>>;
pub(super) trait Endpoint: Send + Sync {
    fn advertises(&self, binding: &str) -> Probe<'_>;
    fn complete(&self, body: Value) -> RuntimeFuture<'_>;
    fn image(&self, _: Value) -> RuntimeFuture<'_> {
        Box::pin(async { Err(TransportError::InputRejected) })
    }
}
struct Http(HttpTransport);
impl Endpoint for Http {
    fn advertises(&self, binding: &str) -> Probe<'_> {
        let binding = binding.to_owned();
        Box::pin(async move { self.0.advertises(&binding).await })
    }
    fn complete(&self, body: Value) -> RuntimeFuture<'_> {
        self.0.complete(body)
    }
    fn image(&self, body: Value) -> RuntimeFuture<'_> {
        self.0.image(body)
    }
}
pub(crate) struct Pool {
    pub catalog: Catalog,
    endpoints: BTreeMap<String, Arc<dyn Endpoint>>,
    resources: Arc<dyn Resources>,
    directory: PathBuf,
    slot: Arc<Semaphore>,
    enabled: AtomicBool,
    uncertain: AtomicBool,
    active: AtomicUsize,
    next: AtomicUsize,
    _locks: Vec<File>,
    #[cfg(target_os = "macos")]
    _native: Option<seiya_observe::image::coordination::ModelLease>,
}
impl Drop for Pool {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if self.uncertain.load(Ordering::Acquire) {
            if let Some(lease) = self._native.take() {
                let marker = json!({"version":1,"source":"local-http-catalogue","confirmed":false,"reason":"operator_review_required"});
                if lease
                    .quarantine("pair-cleanup-uncertain.json", &marker)
                    .is_err()
                {
                    // Existing ownership contract: do not release native lease
                    // when durable quarantine of every old/new domain failed.
                    std::mem::forget(lease);
                }
            }
        }
    }
}
impl Pool {
    pub(crate) fn native(catalog: Catalog, root: &Path) -> Result<Arc<Self>, String> {
        let mut endpoints = catalog
            .models
            .iter()
            .map(|m| {
                let http = HttpTransport::new(&m.runtime).map_err(|e| e.to_string())?;
                Ok((m.id.clone(), Arc::new(Http(http)) as Arc<dyn Endpoint>))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        for m in &catalog.image_models {
            endpoints.insert(
                m.id.clone(),
                Arc::new(Http(
                    HttpTransport::new(&m.runtime).map_err(|e| e.to_string())?,
                )),
            );
        }
        #[allow(unused_mut)]
        let mut pool = Self::configured(catalog, root, endpoints, Arc::new(Native))?;
        #[cfg(target_os = "macos")]
        {
            let home = std::path::PathBuf::from(std::env::var_os("HOME").ok_or("home_required")?)
                .canonicalize()
                .map_err(|_| "home_required")?;
            let lease = seiya_observe::image::coordination::Coordination::portable(
                &home,
                &root.join("coordination"),
            )?
            .acquire()?;
            Arc::get_mut(&mut pool).unwrap()._native = Some(lease);
        }
        Ok(pool)
    }
    pub(super) fn configured(
        catalog: Catalog,
        root: &Path,
        endpoints: BTreeMap<String, Arc<dyn Endpoint>>,
        resources: Arc<dyn Resources>,
    ) -> Result<Arc<Self>, String> {
        catalog.validate().map_err(|e| e.to_string())?;
        storage::ensure_directory(root)?;
        let directory = root.join("coordination");
        storage::ensure_directory(&directory)?;
        let mut ports = catalog
            .models
            .iter()
            .map(|m| m.runtime.endpoint().unwrap().port().unwrap())
            .chain(
                catalog
                    .image_models
                    .iter()
                    .map(|m| m.runtime.endpoint().unwrap().port().unwrap()),
            )
            .collect::<Vec<_>>();
        ports.sort_unstable();
        ports.dedup();
        let mut locks = vec![storage::lock(&directory.join("local-http-host.lock"))?];
        // A new catalogue/port must not evade a previous ambiguous operation.
        let entries = std::fs::read_dir(&directory).map_err(|_| "runtime_journal_observation")?;
        for (index, entry) in entries.enumerate() {
            if index >= 4096 {
                return Err("coordination_directory_limit".into());
            }
            let name = entry
                .map_err(|_| "runtime_journal_observation")?
                .file_name();
            let name = name.to_string_lossy();
            if name.starts_with("http-") && name.ends_with(".inflight") {
                return Err("runtime_journal_pending_operator_required".into());
            }
        }
        for port in ports {
            locks.push(storage::lock(&directory.join(format!("http-{port}.lock")))?);
            match std::fs::symlink_metadata(directory.join(format!("http-{port}.inflight"))) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err("runtime_journal_pending_operator_required".into()),
            }
        }
        if catalog
            .models
            .iter()
            .any(|m| !endpoints.contains_key(&m.id))
            || catalog
                .image_models
                .iter()
                .any(|m| !endpoints.contains_key(&m.id))
        {
            return Err("runtime_transport_missing".into());
        }
        Ok(Arc::new(Self {
            catalog,
            endpoints,
            resources,
            directory,
            slot: Arc::new(Semaphore::new(1)),
            enabled: AtomicBool::new(true),
            uncertain: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            next: AtomicUsize::new(0),
            _locks: locks,
            #[cfg(target_os = "macos")]
            _native: None,
        }))
    }
    pub(crate) fn bound(self: &Arc<Self>, id: &str) -> Arc<dyn RuntimeTransport> {
        Arc::new(Bound {
            pool: self.clone(),
            id: id.into(),
        })
    }
    pub(crate) fn stop(&self) {
        self.enabled.store(false, Ordering::Release);
    }
    pub(crate) fn idle(&self) -> bool {
        self.active.load(Ordering::Acquire) == 0
    }
    pub(crate) fn uncertain(&self) -> bool {
        self.uncertain.load(Ordering::Acquire)
    }
    pub(super) fn available(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
            && self.slot.available_permits() == 1
            && self.resources.admits(self.catalog.minimum_available_mib)
    }
    pub(super) async fn image(
        self: &Arc<Self>,
        request: seiya_contracts::image_job::ImageRequest,
        cancel: watch::Receiver<bool>,
        stage: watch::Sender<seiya_contracts::image_job::ImageStage>,
    ) -> Result<Vec<u8>, crate::image_jobs::Failure> {
        use crate::image_jobs::Failure;
        use seiya_contracts::image_job::ImageStage;
        request.validate().map_err(|_| Failure::Blocked)?;
        if request.width != 512 || request.height != 512 || *cancel.borrow() || !self.available() {
            return Err(Failure::Blocked);
        }
        let id = self.catalog.image.as_ref().ok_or(Failure::Blocked)?;
        let model = self.catalog.image_model(id).map_err(|_| Failure::Blocked)?;
        let permit = self
            .slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| Failure::Blocked)?;
        self.active.fetch_add(1, Ordering::AcqRel);
        let mut turn = Turn {
            pool: self.clone(),
            confirmed: true,
            _permit: permit,
        };
        let endpoint = &self.endpoints[id];
        let advertised = tokio::time::timeout(
            Duration::from_secs(model.runtime.timeout_seconds),
            endpoint.advertises(&model.runtime.model),
        )
        .await;
        if !matches!(advertised, Ok(Ok(true)))
            || *cancel.borrow()
            || !self.resources.admits(self.catalog.minimum_available_mib)
            || !self.enabled.load(Ordering::Acquire)
        {
            return Err(Failure::Blocked);
        }
        let journal = storage::Journal::begin(
            &self.directory,
            model.runtime.endpoint().unwrap().port().unwrap(),
        )
        .map_err(|_| Failure::Blocked)?;
        turn.confirmed = false;
        stage.send_replace(ImageStage::Running);
        let work=endpoint.image(json!({"model":model.runtime.model,"prompt":request.prompt,"n":1,"size":"512x512","response_format":"b64_json","seed":request.seed}));
        tokio::pin!(work);
        let deadline = tokio::time::sleep(Duration::from_secs(model.runtime.timeout_seconds));
        tokio::pin!(deadline);
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let response = loop {
            tokio::select! {
                result=&mut work=>break result.map_err(|_|Failure::Uncertain)?,
                _=&mut deadline=>return Err(Failure::Uncertain),
                _=tick.tick()=>{if !self.resources.admits(self.catalog.minimum_available_mib){self.stop();}},
            }
        };
        stage.send_replace(ImageStage::Validating);
        let png = super::images::decode(&response, &model.runtime.model)
            .map_err(|_| Failure::Uncertain)?;
        // Validate pixels before removing durable intent, not just a PNG header.
        seiya_observe::image::artifact::decode(&png, 512, 512).map_err(|_| Failure::Uncertain)?;
        journal.confirm().map_err(|_| Failure::Uncertain)?;
        turn.confirmed = true;
        if *cancel.borrow() {
            Err(Failure::Cancelled)
        } else {
            Ok(png)
        }
    }
    async fn call(self: &Arc<Self>, id: &str, mut request: Value) -> Result<Value, TransportError> {
        if !self.available() {
            return Err(TransportError::NotConnected);
        }
        let permit = self
            .slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| TransportError::NotConnected)?;
        let model = self
            .catalog
            .model(id)
            .map_err(|_| TransportError::InputRejected)?;
        if request["model"] != model.runtime.model
            || request.get("tools").is_some_and(|v| !v.is_null()) && !model.supports_tools
        {
            return Err(TransportError::InputRejected);
        }
        let output = request
            .get("max_tokens")
            .map_or(Some(model.runtime.max_output_tokens), Value::as_u64)
            .filter(|n| *n > 0 && *n <= model.runtime.max_output_tokens)
            .ok_or(TransportError::InputRejected)?;
        request["max_tokens"] = json!(output);
        request["stream"] = json!(false);
        if serde_json::to_vec(&request)
            .map_err(|_| TransportError::InputRejected)?
            .len()
            > model.max_input_bytes
        {
            return Err(TransportError::InputRejected);
        }
        self.active.fetch_add(1, Ordering::AcqRel);
        let mut turn = Turn {
            pool: self.clone(),
            confirmed: true, // Inventory is read-only; no generation dispatched yet.
            _permit: permit,
        };
        let endpoint = &self.endpoints[id];
        let advertised = endpoint
            .advertises(&model.runtime.model)
            .await
            .map_err(|_| TransportError::NotConnected)?;
        if !advertised {
            return Err(TransportError::NotConnected);
        }
        if !self.resources.admits(self.catalog.minimum_available_mib)
            || !self.enabled.load(Ordering::Acquire)
        {
            return Err(TransportError::NotConnected);
        }
        let journal = storage::Journal::begin(
            &self.directory,
            model.runtime.endpoint().unwrap().port().unwrap(),
        )
        .map_err(|_| TransportError::Unavailable)?;
        turn.confirmed = false;
        let work = endpoint.complete(request);
        tokio::pin!(work);
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let deadline = tokio::time::sleep(Duration::from_secs(model.runtime.timeout_seconds));
        tokio::pin!(deadline);
        let response = loop {
            tokio::select! {
                value=&mut work=>break value?,
                _=&mut deadline=>return Err(TransportError::Timeout),
                _=tick.tick()=>{if !self.resources.admits(self.catalog.minimum_available_mib){self.stop();}},
            }
        };
        let choices = response["choices"]
            .as_array()
            .filter(|v| v.len() == 1)
            .ok_or(TransportError::InvalidJson)?;
        let message = &choices[0]["message"];
        if response["model"] != model.runtime.model
            || message["role"] != "assistant"
            || !(message["content"].is_string()
                || message["tool_calls"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty()))
        {
            return Err(TransportError::InvalidJson);
        }
        journal.confirm().map_err(|_| TransportError::Unavailable)?;
        turn.confirmed = true;
        Ok(response)
    }
}
struct Turn {
    pool: Arc<Pool>,
    confirmed: bool,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl Drop for Turn {
    fn drop(&mut self) {
        if !self.confirmed {
            self.pool.uncertain.store(true, Ordering::Release);
            self.pool.stop();
        }
        self.pool.active.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Bound {
    pool: Arc<Pool>,
    id: String,
}
impl RuntimeTransport for Bound {
    fn complete(&self, request: Value) -> RuntimeFuture<'_> {
        Box::pin(self.pool.call(&self.id, request))
    }
}
impl HostGate for Pool {
    fn status(&self) -> Option<&'static str> {
        if self.uncertain() {
            Some("local-runtime-uncertain-operator-required")
        } else if !self.enabled.load(Ordering::Acquire) {
            Some("local-runtime-stopped")
        } else if !self.resources.admits(self.catalog.minimum_available_mib) {
            Some("local-memory-unknown-or-below-policy")
        } else {
            None
        }
    }
    fn admits(&self, foreground: &str, auxiliary: &str) -> bool {
        foreground
            == self
                .catalog
                .model(&self.catalog.assignments.brain)
                .unwrap()
                .runtime
                .model
            && (auxiliary.is_empty()
                || self
                    .catalog
                    .assignments
                    .workers
                    .iter()
                    .any(|id| self.catalog.model(id).unwrap().runtime.model == auxiliary))
            && self.available()
    }
}
pub(crate) struct Workers(pub Arc<Pool>);
impl crate::jobs::Executor for Workers {
    fn draft(&self, prompt: String, cancel: watch::Receiver<bool>) -> crate::jobs::Work {
        let pool = self.0.clone();
        Box::pin(async move {
            if *cancel.borrow() || pool.catalog.assignments.workers.is_empty() {
                return Err(crate::jobs::Failure::Confirmed);
            }
            let index =
                pool.next.fetch_add(1, Ordering::AcqRel) % pool.catalog.assignments.workers.len();
            let id = pool.catalog.assignments.workers[index].clone();
            let model = pool
                .catalog
                .model(&id)
                .map_err(|_| crate::jobs::Failure::Confirmed)?;
            let request = json!({"model":model.runtime.model,"messages":[{"role":"system","content":"Return a short plain-text draft. No execution, file writes, tools, web or private reasoning."},{"role":"user","content":prompt}],"stream":false,"max_tokens":model.runtime.max_output_tokens.min(512)});
            // Never drop the HTTP future on cancellation: await a full response or
            // deadline; a cancelled socket would not prove backend cleanup.
            let response = pool.call(&id, request).await.map_err(|e| match e {
                TransportError::InputRejected | TransportError::NotConnected => {
                    crate::jobs::Failure::Confirmed
                }
                _ => crate::jobs::Failure::Uncertain,
            })?;
            if *cancel.borrow() {
                return Err(crate::jobs::Failure::Confirmed);
            }
            response["choices"][0]["message"]["content"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 65536)
                .map(str::to_owned)
                .ok_or(crate::jobs::Failure::Confirmed)
        })
    }
}

#[cfg(test)]
mod tests;
