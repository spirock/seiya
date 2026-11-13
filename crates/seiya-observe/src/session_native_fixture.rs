//! Native Child/lease rehearsal for the repository's audited non-forking helper.
//! Not an MLX launcher, process sandbox, or permission to execute model commands.
use crate::campaign::Pressure;
use crate::session_guard::MemoryGuard;
use crate::session_lease::NativeSessionLease;
use crate::session_supervisor::{Policy, Supervisor};
use crate::ProcessIdentity;
#[cfg(target_os = "macos")]
use crate::{
    campaign::PressureSample,
    probes,
    session_supervisor::{
        Coverage, OwnedFuture, OwnedLease, OwnedRuntime, OwnerError, Ownership, ReapEvidence,
    },
    Role,
};
use serde::Serialize;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub enum Behavior {
    Cooperative,
    IgnoreEof,
    BadHandshake,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Unsupported,
    InvalidExecutable,
    LeaseBinding,
    Spawn,
    Identity,
    Handshake,
}
#[derive(Debug)]
pub struct StartError {
    pub kind: FailureKind,
    pub child_cleanup_confirmed: bool,
    pub lease_release_confirmed: bool,
}
#[derive(Default)]
struct Trace {
    eof: AtomicBool,
    term: AtomicU32,
    reaped: AtomicBool,
    untracked: AtomicBool,
}
#[derive(Clone)]
pub struct Evidence {
    identity: ProcessIdentity,
    trace: Arc<Trace>,
}
#[derive(Serialize)]
pub struct EvidenceSnapshot {
    pub identity: ProcessIdentity,
    pub eof_closed: bool,
    pub term_signals: u32,
    pub reaped: bool,
    pub untracked_descendants: bool,
}
impl Evidence {
    pub fn snapshot(&self) -> EvidenceSnapshot {
        EvidenceSnapshot {
            identity: self.identity.clone(),
            eof_closed: self.trace.eof.load(Ordering::SeqCst),
            term_signals: self.trace.term.load(Ordering::SeqCst),
            reaped: self.trace.reaped.load(Ordering::SeqCst),
            untracked_descendants: self.trace.untracked.load(Ordering::SeqCst),
        }
    }
}
pub struct StartedFixture {
    pub supervisor: Supervisor,
    pub evidence: Evidence,
}

#[cfg(target_os = "macos")]
struct Leaf {
    child: Child,
    identity: Option<ProcessIdentity>,
    pressure: Option<Pressure>,
    trace: Arc<Trace>,
    input_closed: Option<Instant>,
}
#[cfg(target_os = "macos")]
impl Leaf {
    fn exited(&mut self) -> Result<bool, OwnerError> {
        let reaped = self
            .child
            .try_wait()
            .map_err(|_| OwnerError::Failed)?
            .is_some();
        if reaped {
            self.trace.reaped.store(true, Ordering::SeqCst);
        }
        Ok(reaped)
    }
    fn inspect(&mut self) -> Result<(), OwnerError> {
        if self.exited()? {
            return Err(OwnerError::Failed);
        }
        let result = (|| {
            let mut proof =
                probes::from_child(&mut self.child, Role::Runtime, Duration::from_millis(500))
                    .map_err(|_| OwnerError::IdentityChanged)?;
            let identity = proof.root_target().identity;
            if self
                .identity
                .as_ref()
                .is_some_and(|expected| expected != &identity)
            {
                return Err(OwnerError::IdentityChanged);
            }
            let targets = proof
                .inspect_targets()
                .map_err(|_| OwnerError::IdentityChanged)?;
            if targets.len() != 1 {
                self.trace.untracked.store(true, Ordering::SeqCst);
            }
            self.identity = Some(identity);
            Ok(())
        })();
        if result.is_err() {
            self.trace.untracked.store(true, Ordering::SeqCst);
        }
        result
    }
    async fn stop_child(&mut self) -> Result<(), OwnerError> {
        if self.exited()? {
            return Ok(());
        }
        if self.identity.is_some() {
            self.inspect()?;
        }
        if self.input_closed.is_none() {
            self.child.stdin.take();
            self.input_closed = Some(Instant::now());
            self.trace.eof.store(true, Ordering::SeqCst);
        }
        loop {
            if self.exited()? {
                return Ok(());
            }
            if self.input_closed.unwrap().elapsed() >= Duration::from_millis(100)
                && self.trace.term.load(Ordering::SeqCst) == 0
            {
                // No signal without strong identity or with unhandled children.
                if self.identity.is_none() || self.trace.untracked.load(Ordering::SeqCst) {
                    return Err(OwnerError::Unsupported);
                }
                self.inspect()?;
                if self.trace.untracked.load(Ordering::SeqCst) {
                    return Err(OwnerError::Unsupported);
                }
                #[cfg(target_os = "macos")]
                {
                    // SAFETY: original unreaped Child, strong identity checked
                    // immediately above. No PID argument from caller/model.
                    if unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) } != 0 {
                        return Err(OwnerError::Failed);
                    }
                    self.trace.term.fetch_add(1, Ordering::SeqCst);
                }
                #[cfg(not(target_os = "macos"))]
                {
                    return Err(OwnerError::Unsupported);
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    #[cfg(target_os = "macos")]
    async fn ready(&mut self) -> Result<(), FailureKind> {
        use std::io::Read;
        use std::os::fd::AsRawFd;
        let stdout = self.child.stdout.as_ref().ok_or(FailureKind::Handshake)?;
        // SAFETY: our owned stdout pipe FD, fixed fcntl operations.
        let flags = unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                < 0
        {
            return Err(FailureKind::Handshake);
        }
        let mut bytes = Vec::new();
        loop {
            self.inspect().map_err(|_| FailureKind::Identity)?;
            let mut chunk = [0u8; 64];
            match self.child.stdout.as_mut().unwrap().read(&mut chunk) {
                Ok(0) => return Err(FailureKind::Handshake),
                Ok(n) => {
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.len() > 64 {
                        return Err(FailureKind::Handshake);
                    }
                    if bytes.contains(&b'\n') {
                        if bytes != b"SEIYA_OWNED_SESSION_READY\n" {
                            return Err(FailureKind::Handshake);
                        }
                        self.child.stdout.take();
                        return Ok(());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return Err(FailureKind::Handshake),
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
#[cfg(target_os = "macos")]
impl OwnedRuntime for Leaf {
    fn ownership(&self) -> Ownership {
        Ownership::new(
            self.identity
                .as_ref()
                .expect("registered fixture identity")
                .clone(),
            vec![],
        )
        .unwrap()
    }
    fn sample_pressure(&mut self, time: u64) -> OwnedFuture<'_, PressureSample> {
        Box::pin(async move {
            self.inspect()?;
            if self.trace.untracked.load(Ordering::SeqCst) {
                return Err(OwnerError::Unsupported);
            }
            let sample = match self.pressure {
                Some(pressure) => Ok(PressureSample {
                    sampled_at_millis: time,
                    pressure,
                }),
                None => crate::session_pressure::sample(time),
            };
            self.inspect()?;
            if self.trace.untracked.load(Ordering::SeqCst) {
                return Err(OwnerError::Unsupported);
            }
            sample
        })
    }
    fn stop_and_reap(&mut self) -> OwnedFuture<'_, ReapEvidence> {
        Box::pin(async move {
            self.stop_child().await?;
            let root = self
                .identity
                .as_ref()
                .ok_or(OwnerError::IdentityChanged)?
                .clone();
            Ok(ReapEvidence {
                root: root.clone(),
                reaped: vec![root],
                coverage: if self.trace.untracked.load(Ordering::SeqCst) {
                    Coverage::UntrackedDescendants
                } else {
                    Coverage::Complete
                },
            })
        })
    }
}

#[cfg(target_os = "macos")]
struct Preparation {
    leaf: Option<Leaf>,
    lease: Option<NativeSessionLease>,
    guard: MemoryGuard,
    transferred: bool,
    lease_released: bool,
}
#[cfg(target_os = "macos")]
impl Drop for Preparation {
    fn drop(&mut self) {
        if self.transferred {
            return;
        }
        self.guard.stop_supervisor();
        if self.leaf.is_some() && !self.lease_released {
            // Unconfirmed bootstrap: do not drop pipes/lease and accidentally
            // unlock or act on a reused PID. Last-resort retention, ADR0023/25.
            if let Some(leaf) = self.leaf.take() {
                std::mem::forget(leaf);
            }
            if let Some(lease) = self.lease.take() {
                std::mem::forget(lease);
            }
        }
    }
}
#[cfg(target_os = "macos")]
impl Preparation {
    async fn fail(mut self, kind: FailureKind) -> StartError {
        let cleanup = if let Some(leaf) = &mut self.leaf {
            matches!(
                tokio::time::timeout(Duration::from_secs(1), leaf.stop_child()).await,
                Ok(Ok(()))
            ) && !leaf.trace.untracked.load(Ordering::SeqCst)
        } else {
            true
        };
        if cleanup {
            self.lease_released = self.lease.as_mut().unwrap().release().await.is_ok();
        }
        StartError {
            kind,
            child_cleanup_confirmed: cleanup,
            lease_release_confirmed: self.lease_released,
        }
    }
}

/// Operator-selected repository fixture executable ONLY, not a model-provided
/// path. Pressure is explicitly injected; no host-memory probe or load permission.
#[cfg(target_os = "macos")]
pub async fn start(
    executable: &Path,
    lease: NativeSessionLease,
    guard: MemoryGuard,
    policy: Policy,
    pressure: Pressure,
    behavior: Behavior,
) -> Result<StartedFixture, StartError> {
    start_scoped(executable, lease, guard, policy, Some(pressure), behavior).await
}

/// Read-only native pressure, still ONLY the audited leaf helper, never MLX.
#[cfg(target_os = "macos")]
pub async fn start_with_native_pressure(
    executable: &Path,
    lease: NativeSessionLease,
    guard: MemoryGuard,
    policy: Policy,
    behavior: Behavior,
) -> Result<StartedFixture, StartError> {
    start_scoped(executable, lease, guard, policy, None, behavior).await
}

#[cfg(target_os = "macos")]
async fn start_scoped(
    executable: &Path,
    lease: NativeSessionLease,
    guard: MemoryGuard,
    policy: Policy,
    pressure: Option<Pressure>,
    behavior: Behavior,
) -> Result<StartedFixture, StartError> {
    use std::os::unix::fs::MetadataExt;
    let mut prepared = Preparation {
        leaf: None,
        lease: Some(lease),
        guard,
        transferred: false,
        lease_released: false,
    };
    if prepared.lease.as_ref().unwrap().verify_binding().is_err() {
        return Err(prepared.fail(FailureKind::LeaseBinding).await);
    }
    let valid = std::fs::symlink_metadata(executable).is_ok_and(|meta| {
        // SAFETY: fixed OS query only.
        let uid = unsafe { libc::geteuid() };
        executable.is_absolute()
            && executable
                .file_name()
                .is_some_and(|name| name == "seiya-observe-fixture")
            && meta.is_file()
            && !meta.file_type().is_symlink()
            && meta.uid() == uid
            && meta.mode() & 0o022 == 0
            && meta.mode() & 0o100 != 0
            && meta.mode() & 0o6000 == 0
    });
    if !valid {
        return Err(prepared.fail(FailureKind::InvalidExecutable).await);
    }
    let flag = match behavior {
        Behavior::Cooperative => "--owned-session-fixture",
        Behavior::IgnoreEof => "--owned-session-ignore-eof",
        Behavior::BadHandshake => "--owned-session-bad-ready",
    };
    let child = match Command::new(executable)
        .arg(flag)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return Err(prepared.fail(FailureKind::Spawn).await),
    };
    prepared.leaf = Some(Leaf {
        child,
        identity: None,
        pressure,
        trace: Arc::new(Trace::default()),
        input_closed: None,
    });
    let leaf = prepared.leaf.as_mut().unwrap();
    if leaf.inspect().is_err() {
        return Err(prepared.fail(FailureKind::Identity).await);
    }
    match tokio::time::timeout(Duration::from_secs(1), leaf.ready()).await {
        Ok(Ok(())) => {}
        Ok(Err(kind)) => return Err(prepared.fail(kind).await),
        Err(_) => return Err(prepared.fail(FailureKind::Handshake).await),
    }
    if prepared
        .leaf
        .as_ref()
        .unwrap()
        .trace
        .untracked
        .load(Ordering::SeqCst)
    {
        return Err(prepared.fail(FailureKind::Identity).await);
    }
    let leaf = prepared.leaf.take().unwrap();
    let evidence = Evidence {
        identity: leaf.identity.as_ref().unwrap().clone(),
        trace: leaf.trace.clone(),
    };
    let lease = prepared.lease.take().unwrap();
    prepared.transferred = true;
    let supervisor = Supervisor::start_native_fixture(
        leaf,
        lease,
        prepared.guard.clone(),
        policy,
        pressure.is_none(),
    );
    Ok(StartedFixture {
        supervisor,
        evidence,
    })
}

#[cfg(not(target_os = "macos"))]
pub async fn start(
    _: &Path,
    _: NativeSessionLease,
    guard: MemoryGuard,
    _: Policy,
    _: Pressure,
    _: Behavior,
) -> Result<StartedFixture, StartError> {
    guard.stop_supervisor();
    Err(StartError {
        kind: FailureKind::Unsupported,
        child_cleanup_confirmed: true,
        lease_release_confirmed: false,
    })
}
