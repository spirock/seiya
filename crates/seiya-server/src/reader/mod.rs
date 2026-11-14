//! Experimental reader components; opt-in library router only, no CLI activation.
pub mod extract;
pub mod policy;
pub mod selection;
pub mod transport;
mod wire;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Policy,
    Timeout,
    Limit,
    Transport,
    Format,
    Http,
    Unavailable,
    Cancelled,
    NoText,
}

/// One host-owned slot. Unknown cleanup after timeout or consumer cancellation
/// is terminal for this instance, even if a later host sample looks healthy.
pub struct Reader {
    network: std::sync::Arc<dyn transport::Network>,
    state: std::sync::atomic::AtomicU8,
    cancellations: std::sync::Mutex<std::collections::HashSet<String>>,
    changed: tokio::sync::Notify,
}

struct Active<'a> {
    state: &'a std::sync::atomic::AtomicU8,
    armed: bool,
}

impl Active<'_> {
    fn release(mut self) {
        self.armed = false;
        self.state.store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Drop for Active<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.state.store(2, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

impl Reader {
    /// Explicit library integration only. Never announces a tool or opens a socket.
    pub fn new(network: std::sync::Arc<dyn transport::Network>) -> Self {
        Self {
            network,
            state: std::sync::atomic::AtomicU8::new(0),
            cancellations: std::sync::Mutex::new(std::collections::HashSet::new()),
            changed: tokio::sync::Notify::new(),
        }
    }

    pub fn quarantined(&self) -> bool {
        self.state.load(std::sync::atomic::Ordering::SeqCst) == 2
    }

    /// Idempotent, host-authorized cancellation. Also records requests arriving
    /// before the read is admitted, so an upload/cancel race cannot start DNS.
    /// Bounded tombstones never expire silently or permit a run ID to restart.
    pub fn cancel(&self, run_id: &str) -> Result<(), Error> {
        if run_id.is_empty()
            || run_id.len() > 128
            || !run_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(Error::Policy);
        }
        let mut ids = self.cancellations.lock().map_err(|_| Error::Unavailable)?;
        if !ids.contains(run_id) && ids.len() >= 128 {
            return Err(Error::Unavailable);
        }
        ids.insert(run_id.to_string());
        drop(ids);
        self.changed.notify_one();
        Ok(())
    }

    fn is_cancelled(&self, run_id: &str) -> bool {
        self.cancellations
            .lock()
            .map(|ids| ids.contains(run_id))
            .unwrap_or(true)
    }

    async fn cancelled(&self, run_id: &str) {
        while !self.is_cancelled(run_id) {
            self.changed.notified().await;
        }
    }

    fn acquire(&self) -> Result<Active<'_>, Error> {
        use std::sync::atomic::Ordering::SeqCst;
        self.state
            .compare_exchange(0, 1, SeqCst, SeqCst)
            .map_err(|_| Error::Unavailable)?;
        Ok(Active {
            state: &self.state,
            armed: true,
        })
    }

    pub async fn fetch(
        &self,
        input: &str,
        deadline: tokio::time::Instant,
    ) -> Result<transport::Page, Error> {
        self.fetch_report(input, deadline).await?.result
    }

    /// Single-page observation preserving partial counters on failure. The
    /// same ownership/quarantine rules apply as for fetch, with no retries.
    pub async fn fetch_report(
        &self,
        input: &str,
        deadline: tokio::time::Instant,
    ) -> Result<transport::Report, Error> {
        self.fetch_report_with_redirect_limit(input, deadline, 2)
            .await
    }
    pub async fn fetch_report_with_redirect_limit(
        &self,
        input: &str,
        deadline: tokio::time::Instant,
        max_redirects: usize,
    ) -> Result<transport::Report, Error> {
        let active = self.acquire()?;
        let report = transport::fetch_report_with_redirect_limit(
            self.network.as_ref(),
            input,
            deadline,
            max_redirects,
        )
        .await;
        if !matches!(report.result, Err(Error::Timeout)) {
            active.release();
        }
        Ok(report)
    }

    /// Fetch and extract text under one run budget. No raw HTML escapes, and
    /// successful transport alone is not considered useful extracted content.
    pub async fn read_batch(
        &self,
        request: &seiya_contracts::reader_frame::Request,
        deadline: tokio::time::Instant,
    ) -> Result<serde_json::Value, Error> {
        let deadline = deadline.min(
            tokio::time::Instant::now()
                + std::time::Duration::from_millis(request.remaining_ms().into()),
        );
        let active = self.acquire()?;
        let mut results = Vec::new();
        let mut text_remaining = 30000usize;
        for (id, url) in request.sources() {
            if self.is_cancelled(request.run_id()) {
                active.release();
                return Err(Error::Cancelled);
            }
            let report = tokio::select! {
                biased;
                _ = self.cancelled(request.run_id()) => return Err(Error::Cancelled),
                report = transport::fetch_report(self.network.as_ref(), url, deadline) => report,
            };
            let mut text: Option<String> = None;
            let mut text_truncated = false;
            let status = match report.result {
                Ok(page) => match extract::extract(&page.body, &page.mime) {
                    Ok(value) if text_remaining > 0 => {
                        let count = value.text.chars().count();
                        text_truncated = value.truncated || count > text_remaining;
                        let selected: String = value.text.chars().take(text_remaining).collect();
                        text_remaining -= selected.chars().count();
                        text = Some(selected);
                        "ok"
                    }
                    Ok(_) | Err(Error::Limit) => "limit_exceeded",
                    Err(Error::NoText) => "no_useful_text",
                    Err(_) => "unsupported_format",
                },
                Err(Error::Policy) => "policy_blocked",
                Err(Error::Transport) => "transport_error",
                Err(Error::Http) => "http_error",
                Err(Error::Format) => "unsupported_format",
                Err(Error::Timeout) => return Err(Error::Timeout), // Active drop quarantines.
                Err(error) => {
                    active.release();
                    return Err(error);
                }
            };
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            if self.is_cancelled(request.run_id()) {
                active.release();
                return Err(Error::Cancelled);
            }
            results.push(serde_json::json!({"id":id,"status":status,"text":text,
                "text_truncated":text_truncated,"received_bytes":report.counters.received_bytes,
                "decoded_bytes":report.counters.decoded_bytes,"header_bytes":report.counters.header_bytes,
                "get_count":report.counters.get_count}));
        }
        let response = serde_json::json!({"version":"0.1-draft.1","run_id":request.run_id(),"results":results});
        let encoded = serde_json::to_vec(&response).map_err(|_| Error::Format)?;
        let validation = seiya_contracts::reader_frame::validate_response(request, &encoded, true);
        active.release();
        validation.map_err(|_| Error::Limit)?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::net::IpAddr;
    use std::pin::Pin;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use tokio::time::{Duration, Instant};
    type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'a>>;
    struct Hanging(Arc<AtomicBool>);
    impl transport::Network for Hanging {
        fn resolve(&self, _: String) -> Pending<'_, Vec<IpAddr>> {
            Box::pin(async {
                struct Dropped(Arc<AtomicBool>);
                impl Drop for Dropped {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::SeqCst);
                    }
                }
                let _guard = Dropped(self.0.clone());
                std::future::pending().await
            })
        }
        fn connect(&self, _: String, _: IpAddr) -> Pending<'_, Box<dyn transport::Stream>> {
            panic!("must not connect")
        }
    }
    #[tokio::test]
    async fn timeout_drops_pending_io_and_quarantines_without_rearm() {
        let dropped = Arc::new(AtomicBool::new(false));
        let reader = Reader::new(Arc::new(Hanging(dropped.clone())));
        assert!(matches!(
            reader
                .fetch(
                    "https://example.org",
                    Instant::now() + Duration::from_millis(20)
                )
                .await,
            Err(Error::Timeout)
        ));
        assert!(dropped.load(Ordering::SeqCst));
        assert!(reader.quarantined());
        assert!(matches!(
            reader
                .fetch(
                    "https://example.org",
                    Instant::now() + Duration::from_secs(1)
                )
                .await,
            Err(Error::Unavailable)
        ));
    }
    #[tokio::test]
    async fn consumer_cancellation_keeps_slot_terminal_and_busy_never_dispatches() {
        let reader = Reader::new(Arc::new(Hanging(Arc::new(AtomicBool::new(false)))));
        let mut first = Box::pin(reader.fetch(
            "https://example.org",
            Instant::now() + Duration::from_secs(30),
        ));
        tokio::select! {
            _ = &mut first => panic!("unexpected completion"),
            _ = tokio::time::sleep(Duration::from_millis(10)) => (),
        }
        assert!(matches!(
            reader
                .fetch(
                    "https://example.org",
                    Instant::now() + Duration::from_secs(1)
                )
                .await,
            Err(Error::Unavailable)
        ));
        drop(first);
        assert!(reader.quarantined());
    }
    #[tokio::test]
    async fn pre_network_policy_error_releases_slot_without_permission_claim() {
        let reader = Reader::new(Arc::new(Hanging(Arc::new(AtomicBool::new(false)))));
        for _ in 0..2 {
            assert!(matches!(
                reader
                    .fetch("http://localhost", Instant::now() + Duration::from_secs(1))
                    .await,
                Err(Error::Policy)
            ));
            assert!(!reader.quarantined());
        }
    }
}
