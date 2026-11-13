use crate::config::ConfigError;
use crate::inference::{ApiError, InferenceAdapter, ValidatedInput};
use axum::http::StatusCode;
use seiya_observe::session_guard::{Block, MemoryGuard};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Mutex, Semaphore};

pub const QUEUE_FULL: &str = "request queue full (one active, two waiting); retry later";
pub const QUEUE_TIMEOUT: &str = "timed out waiting for the active request; retry later";

struct Inner {
    adapter: InferenceAdapter,
    slots: Arc<Semaphore>,
    gate: Arc<Mutex<()>>,
    closing: watch::Sender<bool>,
    memory: Option<MemoryGuard>,
}

/// Per-runtime ownership within this process. Share, never recreate per request.
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
    queue_wait: Duration,
}

fn unavailable(message: &'static str) -> ApiError {
    ApiError {
        status: StatusCode::SERVICE_UNAVAILABLE,
        message: message.into(),
        code: None,
    }
}

fn closing_error() -> ApiError {
    unavailable("experimental server is shutting down")
}

fn memory_error(block: Block) -> ApiError {
    let (code, message) = match block {
        Block::AwaitingObservation => (
            "memory_observation_unavailable",
            "memory observation is unavailable; no request dispatched",
        ),
        Block::Stopped(_) => (
            "runtime_memory_blocked",
            "runtime admission stopped by memory observation policy; operator recovery required",
        ),
    };
    ApiError {
        status: StatusCode::SERVICE_UNAVAILABLE,
        message: message.into(),
        code: Some(code),
    }
}

async fn memory_blocked(memory: &Option<MemoryGuard>) -> Block {
    match memory {
        Some(memory) => memory.wait_blocked().await,
        None => std::future::pending().await,
    }
}

impl Scheduler {
    pub(crate) fn model_binding_matches(&self, supplied: &str) -> bool {
        self.inner.adapter.model_binding_matches(supplied)
    }
    pub fn new(adapter: InferenceAdapter, queue_wait: Duration) -> Result<Self, ConfigError> {
        Self::configured(adapter, queue_wait, None)
    }

    /// Supplemental stop-only guard. Caller still owns preflight, load approval,
    /// native observation, runtime and lease. Does not start/stop a backend.
    pub fn new_with_memory_guard(
        adapter: InferenceAdapter,
        queue_wait: Duration,
        memory: MemoryGuard,
    ) -> Result<Self, ConfigError> {
        if adapter.runtime_status().is_none() {
            return Err(ConfigError("memory guard requires opt-in runtime status"));
        }
        Self::configured(adapter, queue_wait, Some(memory))
    }

    fn configured(
        adapter: InferenceAdapter,
        queue_wait: Duration,
        memory: Option<MemoryGuard>,
    ) -> Result<Self, ConfigError> {
        if queue_wait < Duration::from_millis(1) || queue_wait > Duration::from_secs(120) {
            return Err(ConfigError(
                "queue wait must be between 1 and 120000 milliseconds",
            ));
        }
        let (closing, _) = watch::channel(false);
        Ok(Self {
            inner: Arc::new(Inner {
                adapter,
                slots: Arc::new(Semaphore::new(3)),
                gate: Arc::new(Mutex::new(())),
                closing,
                memory,
            }),
            queue_wait,
        })
    }

    pub fn is_closed(&self) -> bool {
        *self.inner.closing.borrow()
    }
    pub fn requires_recovery(&self) -> bool {
        self.inner.adapter.requires_recovery()
    }
    pub fn runtime_status(&self) -> Option<Value> {
        self.inner.adapter.runtime_status().map(|mut value| {
            if let Some(memory) = &self.inner.memory {
                let status = memory.status();
                // Version2 makes memory_admission a structured guard snapshot;
                // unguarded S4-02a stays version1 with its original string.
                value["version"] = serde_json::json!(2);
                if status.state != "monitoring" {
                    value["admission"] = serde_json::json!("blocked");
                }
                value["memory_admission"] = serde_json::json!(status);
            }
            if self.is_closed() {
                value["admission"] = serde_json::json!("closed");
            }
            value
        })
    }
    pub fn admitted_requests(&self) -> usize {
        3 - self.inner.slots.available_permits()
    }
    pub fn is_busy(&self) -> bool {
        self.admitted_requests() != 0 || self.inner.adapter.is_busy()
    }

    pub fn close(&self) {
        self.inner.closing.send_replace(true);
        self.inner.slots.close();
    }

    pub async fn complete(&self, body: Value) -> Result<Value, ApiError> {
        if self.is_closed() {
            return Err(closing_error());
        }
        self.complete_validated(ValidatedInput::new(body)?).await
    }

    pub(crate) async fn complete_validated(&self, body: ValidatedInput) -> Result<Value, ApiError> {
        let mut closing = self.inner.closing.subscribe();
        if *closing.borrow() {
            return Err(closing_error());
        }
        if let Some(memory) = &self.inner.memory {
            memory.check().map_err(memory_error)?;
        }
        if self.requires_recovery() {
            if let Some(error) = self.inner.adapter.blocking_error() {
                return Err(error);
            }
            return Err(unavailable(
                "runtime state is unconfirmed; verify backend is idle before recovery",
            ));
        }
        let slot = self.inner.slots.clone().try_acquire_owned().map_err(|_| {
            if self.is_closed() {
                closing_error()
            } else {
                ApiError {
                    status: StatusCode::TOO_MANY_REQUESTS,
                    message: QUEUE_FULL.into(),
                    code: None,
                }
            }
        })?;
        let gate = tokio::select! {
            biased;
            _ = closing.changed() => return Err(closing_error()),
            block = memory_blocked(&self.inner.memory) => return Err(memory_error(block)),
            result = tokio::time::timeout(self.queue_wait, self.inner.gate.clone().lock_owned()) =>
                result.map_err(|_| ApiError { status: StatusCode::TOO_MANY_REQUESTS, message: QUEUE_TIMEOUT.into(), code:None })?,
        };
        if self.is_closed() {
            return Err(closing_error());
        }
        let inner = self.inner.clone();
        // This task owns both queue capacity and the worker lock even when the
        // HTTP consumer disappears. The adapter also owns its generation lease.
        tokio::spawn(async move {
            let (_slot, _gate) = (slot, gate);
            if *inner.closing.borrow() {
                return Err(closing_error());
            }
            if inner.memory.is_none() {
                return inner.adapter.complete_validated(body).await;
            }
            let started = AtomicBool::new(false);
            let generation = async {
                if let Some(memory) = &inner.memory {
                    memory.check().map_err(memory_error)?;
                }
                started.store(true, Ordering::Release);
                inner.adapter.complete_validated(body).await
            };
            tokio::pin!(generation);
            tokio::select! {
                biased;
                block = memory_blocked(&inner.memory) => {
                    // Never poll an unstarted generation after a stop. Once
                    // started, retain worker/slot until adapter result/deadline;
                    // dropping this HTTP future would not stop an external MLX.
                    if started.load(Ordering::Acquire) { let _ = generation.await; }
                    Err(memory_error(block))
                }
                result = &mut generation => {
                    if let Some(memory) = &inner.memory {
                        memory.check().map_err(memory_error)?;
                    }
                    result
                }
            }
        })
        .await
        .map_err(|_| unavailable("scheduled inference task failed; verify runtime state"))?
    }

    /// Stop admission and drain the active job; never claim to cancel MLX.
    pub async fn close_and_drain(&self) -> Result<(), ApiError> {
        self.close();
        let _gate = self.inner.gate.lock().await;
        if self.inner.adapter.is_busy() {
            return Err(unavailable(
                "runtime state is unconfirmed after shutdown; verify backend before reuse",
            ));
        }
        Ok(())
    }
}
