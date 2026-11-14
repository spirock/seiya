//! Opt-in browser search/reading. No /v1 tools and no I/O at construction.
mod flow;
pub mod search;
use crate::{
    inference::ApiError,
    local_models::storage,
    prepared_web::HostGate,
    reader::{transport::HttpsNetwork, Reader},
    scheduler::Scheduler,
};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{
    sync::{watch, Semaphore},
    time::Instant,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub run_id: String,
    pub token: String,
}
struct Run {
    token: String,
    phase: &'static str,
    consumed: bool,
    active: bool,
    touched: Instant,
    cancel: watch::Sender<bool>,
}
pub(crate) struct Context {
    pub scheduler: Scheduler,
    pub gate: Arc<dyn HostGate>,
    pub foreground: String,
    pub auxiliary: String,
    pub job_tools: Value,
}
pub struct WebChat {
    search: Arc<dyn search::Search>,
    reader: Arc<Reader>,
    records: Mutex<HashMap<String, Run>>,
    slot: Arc<Semaphore>,
    stopped: AtomicBool,
    uncertain: AtomicBool,
    directory: Option<PathBuf>,
    max_input: usize,
    output_tokens: u64,
}
pub(crate) fn failure(code: &'static str, status: StatusCode, message: &'static str) -> ApiError {
    ApiError {
        status,
        message: message.into(),
        code: Some(code),
    }
}
fn unavailable() -> ApiError {
    failure(
        "web_reader_unavailable",
        StatusCode::SERVICE_UNAVAILABLE,
        "La búsqueda/lectura no está disponible. No se ha reenviado ni cambiado de proveedor.",
    )
}
fn cancelled() -> ApiError {
    failure(
        "web_run_cancelled",
        StatusCode::CONFLICT,
        "Tarea cancelada. No se confirma una generación nueva ni se reenvía el turno.",
    )
}
impl WebChat {
    pub fn with_io(
        search: Arc<dyn search::Search>,
        reader: Arc<Reader>,
        max_input: usize,
        output_tokens: u64,
    ) -> Result<Arc<Self>, ApiError> {
        if !(8192..=131072).contains(&max_input) || !(1..=256).contains(&output_tokens) {
            return Err(unavailable());
        }
        Ok(Arc::new(Self {
            search,
            reader,
            records: Mutex::new(HashMap::new()),
            slot: Arc::new(Semaphore::new(1)),
            stopped: AtomicBool::new(false),
            uncertain: AtomicBool::new(false),
            directory: None,
            max_input,
            output_tokens,
        }))
    }
    pub(crate) fn native(
        root: &Path,
        max_input: usize,
        output_tokens: u64,
    ) -> Result<Arc<Self>, String> {
        let directory = root.join("coordination");
        storage::ensure_directory(&directory)?;
        match std::fs::symlink_metadata(directory.join("web-reader.inflight")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("web_reader_cleanup_unconfirmed_operator_required".into()),
        }
        let search_reader = Arc::new(Reader::new(Arc::new(HttpsNetwork::default())));
        let reader = Arc::new(Reader::new(Arc::new(HttpsNetwork::default())));
        let mut state = Self::with_io(
            Arc::new(search::PublicSearch::new(search_reader)),
            reader,
            max_input,
            output_tokens,
        )
        .map_err(|_| "web_configuration_invalid")?;
        Arc::get_mut(&mut state).unwrap().directory = Some(directory);
        Ok(state)
    }
    pub(crate) fn create(&self) -> Result<Reference, ApiError> {
        if self.stopped.load(Ordering::Acquire) || self.quarantined() {
            return Err(unavailable());
        }
        let mut rows = self.records.lock().map_err(|_| unavailable())?;
        let now = Instant::now();
        rows.retain(|_, r| r.active || now.duration_since(r.touched) < Duration::from_secs(600));
        if rows.len() >= 128 {
            return Err(failure(
                "web_run_capacity",
                StatusCode::TOO_MANY_REQUESTS,
                "Límite de tareas web alcanzado; no se ha iniciado otra.",
            ));
        }
        let run_id = uuid::Uuid::new_v4().simple().to_string();
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let (cancel, _) = watch::channel(false);
        rows.insert(
            run_id.clone(),
            Run {
                token: token.clone(),
                phase: "ready",
                consumed: false,
                active: false,
                touched: now,
                cancel,
            },
        );
        Ok(Reference { run_id, token })
    }
    fn find<'a>(
        rows: &'a mut HashMap<String, Run>,
        reference: &Reference,
    ) -> Result<&'a mut Run, ApiError> {
        if reference.run_id.len() != 32
            || reference.token.len() != 64
            || !reference
                .run_id
                .bytes()
                .chain(reference.token.bytes())
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(unavailable());
        }
        let row = rows.get_mut(&reference.run_id).ok_or_else(unavailable)?;
        if !bool::from(row.token.as_bytes().ct_eq(reference.token.as_bytes())) {
            return Err(unavailable());
        }
        row.touched = Instant::now();
        Ok(row)
    }
    pub(crate) fn status(&self, reference: &Reference) -> Result<Value, ApiError> {
        let mut rows = self.records.lock().map_err(|_| unavailable())?;
        let row = Self::find(&mut rows, reference)?;
        Ok(
            json!({"run_id":reference.run_id,"phase":row.phase,"consumed":row.consumed,"cleanup_confirmed":!row.active && !self.quarantined()}),
        )
    }
    pub(crate) fn cancel(&self, reference: &Reference) -> Result<Value, ApiError> {
        let mut rows = self.records.lock().map_err(|_| unavailable())?;
        let row = Self::find(&mut rows, reference)?;
        let reading = row.phase == "reading";
        row.cancel.send_replace(true);
        row.phase = if row.active {
            "cancelling"
        } else {
            "cancelled"
        };
        if !row.active {
            row.consumed = true;
        }
        if reading {
            self.reader
                .cancel(&reference.run_id)
                .map_err(|_| unavailable())?;
        }
        Ok(json!({"run_id":reference.run_id,"phase":row.phase,"cleanup_confirmed":false}))
    }
    fn phase(&self, id: &str, phase: &'static str) {
        if let Ok(mut rows) = self.records.lock() {
            if let Some(row) = rows.get_mut(id) {
                if !*row.cancel.borrow() {
                    row.phase = phase;
                }
                row.touched = Instant::now();
            }
        }
    }
    fn quarantined(&self) -> bool {
        self.uncertain.load(Ordering::Acquire)
            || self.reader.quarantined()
            || self.search.quarantined()
    }
    pub(crate) async fn shutdown(&self) -> Result<(), String> {
        self.stopped.store(true, Ordering::Release);
        if let Ok(rows) = self.records.lock() {
            for (id, row) in rows.iter() {
                if row.active {
                    row.cancel.send_replace(true);
                    if row.phase == "reading" {
                        let _ = self.reader.cancel(id);
                    }
                }
            }
        }
        let _permit = self
            .slot
            .acquire()
            .await
            .map_err(|_| "web_reader_cleanup_unconfirmed")?;
        if self.quarantined() {
            Err("web_reader_cleanup_unconfirmed".into())
        } else {
            Ok(())
        }
    }
    pub(crate) async fn complete(
        self: &Arc<Self>,
        reference: Reference,
        messages: Vec<Value>,
        context: Context,
    ) -> Result<Value, ApiError> {
        if self.stopped.load(Ordering::Acquire) || self.quarantined() {
            return Err(unavailable());
        }
        let permit = self.slot.clone().try_acquire_owned().map_err(|_| {
            failure(
                "web_turn_busy",
                StatusCode::TOO_MANY_REQUESTS,
                "Hay otro turno web activo; no se ha ejecutado otra búsqueda.",
            )
        })?;
        let cancel = {
            let mut rows = self.records.lock().map_err(|_| unavailable())?;
            let row = Self::find(&mut rows, &reference)?;
            if row.consumed || *row.cancel.borrow() {
                return Err(failure(
                    "web_run_consumed",
                    StatusCode::CONFLICT,
                    "La tarea ya fue usada o cancelada. No se reenvía.",
                ));
            }
            row.consumed = true;
            row.active = true;
            row.phase = "ready";
            row.cancel.subscribe()
        };
        let owner = self.clone();
        // Own the whole run independently of a lost browser connection. Explicit
        // cancellation/deadlines are the authority, not TCP disappearance.
        tokio::spawn(async move{
            let mut guard=RunGuard{owner:owner.clone(),id:reference.run_id.clone(),_permit:permit,finished:false};
            let mut journal=None;
            let scheduler=context.scheduler.clone();
            let result=flow::execute(&owner,&reference.run_id,messages,context,cancel,&mut journal).await;
            if scheduler.is_busy(){owner.uncertain.store(true,Ordering::Release);}
            let uncertain=owner.quarantined();
            let result=if uncertain {owner.uncertain.store(true,Ordering::Release);Err(failure("web_reader_cleanup_unconfirmed",StatusCode::SERVICE_UNAVAILABLE,"La tarea web/modelo quedó en estado incierto. No reenvíes; requiere revisión operativa."))}
                else if let Some(marker)=journal {match marker.confirm(){Ok(())=>result,Err(_)=>{owner.uncertain.store(true,Ordering::Release);Err(unavailable())}}}else{result};
            if let Ok(mut rows)=owner.records.lock(){if let Some(row)=rows.get_mut(&reference.run_id){row.active=false;row.phase=if result.is_ok(){"finished"}else if *row.cancel.borrow()&&!owner.quarantined(){"cancelled"}else{"failed"};row.touched=Instant::now();}}
            guard.finished=true;
            result
        }).await.map_err(|_|unavailable())?
    }
}
struct RunGuard {
    owner: Arc<WebChat>,
    id: String,
    _permit: tokio::sync::OwnedSemaphorePermit,
    finished: bool,
}
impl Drop for RunGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.owner.uncertain.store(true, Ordering::Release);
            self.owner.stopped.store(true, Ordering::Release);
            if let Ok(mut rows) = self.owner.records.lock() {
                if let Some(row) = rows.get_mut(&self.id) {
                    row.active = false;
                    row.phase = "failed";
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
