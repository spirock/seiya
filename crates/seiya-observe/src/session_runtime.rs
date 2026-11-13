//! Opt-in restricted owned runtime. No HTTP control interface or auto promotion.
#![cfg(target_os = "macos")]
use crate::session_containment::{PROFILE, READY};
use crate::session_guard::MemoryGuard;
use crate::session_lease::NativeSessionLease;
use crate::session_supervisor::{
    Coverage, OwnedFuture, OwnedLease, OwnedRuntime, OwnerError, Ownership, Policy, ReapEvidence,
    Supervisor,
};
use crate::{
    campaign::{
        self, LoadBudget, MlxRecipe, ModelLoadApproval, NativeProvenance, PackageMetadata,
        Pressure, PressureSample, Reservation,
    },
    probes, ProcessIdentity, Role,
};
use std::os::{fd::AsRawFd, unix::fs::MetadataExt};
use std::{
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Authorization,
    Budget,
    Preparation,
    Executable,
    Spawn,
    Identity,
    Containment,
    Pressure,
    StartupTimeout,
    Socket,
}
#[derive(Debug)]
pub struct StartError {
    pub cause: Error,
    pub cleanup_confirmed: bool,
    pub lease_released: bool,
}
pub struct StartedRuntime {
    pub supervisor: Supervisor,
    pub port: u16,
    pub identity: ProcessIdentity,
}
struct StopOnFailure {
    guard: MemoryGuard,
    success: bool,
}
impl Drop for StopOnFailure {
    fn drop(&mut self) {
        if !self.success {
            self.guard.stop_supervisor();
        }
    }
}

unsafe extern "C" {
    fn seiya_owned_listener(pid: libc::c_int, port: u16) -> libc::c_int;
}

struct Runtime {
    child: Child,
    identity: Option<ProcessIdentity>,
    port: u16,
    contained: bool,
    uncertain_tree: bool,
    term_sent: bool,
    pressure: Option<Pressure>,
    authorization_deadline: Option<Instant>,
}
impl Runtime {
    fn exited(&mut self) -> Result<bool, OwnerError> {
        Ok(self
            .child
            .try_wait()
            .map_err(|_| OwnerError::Failed)?
            .is_some())
    }
    fn verify(&mut self) -> Result<(), OwnerError> {
        if self.exited()? {
            return Err(OwnerError::Failed);
        }
        let result = (|| {
            let mut proof =
                probes::from_child(&mut self.child, Role::Runtime, Duration::from_millis(500))
                    .map_err(|_| OwnerError::IdentityChanged)?;
            let root = proof.root_target().identity;
            if self.identity.as_ref().is_some_and(|id| id != &root) {
                return Err(OwnerError::IdentityChanged);
            }
            let targets = proof.inspect_targets().map_err(|_| OwnerError::Failed)?;
            if targets.len() != 1 {
                return Err(OwnerError::Unsupported);
            }
            self.identity = Some(root);
            Ok(())
        })();
        if result.is_err() {
            self.uncertain_tree = true;
        }
        result
    }
    fn listening(&mut self) -> Result<bool, OwnerError> {
        self.verify()?;
        // SAFETY: exact original, unreaped Child verified before/after; fixed port
        // from owned reservation. C shim bounds buffers and uses SDK ABI structs.
        let result = unsafe { seiya_owned_listener(self.child.id() as i32, self.port) };
        self.verify()?;
        match result {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(OwnerError::Failed),
        }
    }
    fn pressure(&mut self, now: u64) -> Result<PressureSample, OwnerError> {
        if self
            .authorization_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(OwnerError::Unsupported);
        }
        self.verify()?;
        let sample = match self.pressure {
            Some(pressure) => Ok(PressureSample {
                sampled_at_millis: now,
                pressure,
            }),
            None => crate::session_pressure::sample(now),
        };
        self.verify()?;
        sample
    }
    async fn stop(&mut self) -> Result<(), OwnerError> {
        if self.exited()? {
            return Ok(());
        }
        self.verify()?;
        if !self.term_sent {
            // SAFETY: original Child cannot have PID reused until reaped; strong
            // identity checked immediately above. Never signal group/descendant.
            if unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) } != 0 {
                return Err(OwnerError::Failed);
            }
            self.term_sent = true;
        }
        while !self.exited()? {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    }
    async fn bootstrap(&mut self, guard: &MemoryGuard, deadline: Instant) -> Result<(), Error> {
        let pipe = self.child.stdout.as_ref().ok_or(Error::Containment)?;
        // SAFETY: owned pipe fd and fixed nonblocking fcntl operations.
        let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            return Err(Error::Containment);
        }
        let mut bytes = Vec::new();
        loop {
            if Instant::now() >= deadline {
                return Err(Error::StartupTimeout);
            }
            let mut buffer = [0u8; 64];
            match self.child.stdout.as_mut().unwrap().read(&mut buffer) {
                Ok(0) => return Err(Error::Containment),
                Ok(n) => {
                    bytes.extend_from_slice(&buffer[..n]);
                    if bytes.len() > READY.len() {
                        return Err(Error::Containment);
                    }
                    if bytes.contains(&b'\n') {
                        if bytes != READY {
                            return Err(Error::Containment);
                        }
                        self.contained = true;
                        self.child.stdout.take();
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return Err(Error::Containment),
            }
            self.observe(guard)?;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        loop {
            if Instant::now() >= deadline {
                return Err(Error::StartupTimeout);
            }
            self.observe(guard)?;
            if self.listening().map_err(|_| Error::Socket)? {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    fn observe(&mut self, guard: &MemoryGuard) -> Result<(), Error> {
        let now = guard.now_millis().ok_or(Error::Pressure)?;
        let sample = self.pressure(now).map_err(|_| Error::Identity)?;
        guard.observe(Ok(sample)).map_err(|_| Error::Pressure)
    }
}
impl OwnedRuntime for Runtime {
    fn ownership(&self) -> Ownership {
        Ownership::new(self.identity.as_ref().unwrap().clone(), vec![]).unwrap()
    }
    fn sample_pressure(&mut self, now: u64) -> OwnedFuture<'_, PressureSample> {
        Box::pin(async move {
            let sample = self.pressure(now)?;
            if !self.contained || !self.listening()? {
                return Err(OwnerError::Failed);
            }
            Ok(sample)
        })
    }
    fn stop_and_reap(&mut self) -> OwnedFuture<'_, ReapEvidence> {
        Box::pin(async move {
            self.stop().await?;
            let root = self
                .identity
                .as_ref()
                .ok_or(OwnerError::IdentityChanged)?
                .clone();
            Ok(ReapEvidence {
                root: root.clone(),
                reaped: vec![root],
                coverage: if self.contained && !self.uncertain_tree {
                    Coverage::Complete
                } else {
                    Coverage::UntrackedDescendants
                },
            })
        })
    }
}

struct Preparation {
    runtime: Option<Runtime>,
    lease: Option<NativeSessionLease>,
    guard: MemoryGuard,
    transferred: bool,
}
impl Drop for Preparation {
    fn drop(&mut self) {
        if self.transferred {
            return;
        }
        self.guard.stop_supervisor();
        if self.runtime.is_some() {
            // Cancellation/panic/unconfirmed cleanup: never unlock in Drop.
            if let Some(runtime) = self.runtime.take() {
                std::mem::forget(runtime);
            }
            if let Some(lease) = self.lease.take() {
                std::mem::forget(lease);
            }
        }
    }
}
impl Preparation {
    async fn fail(mut self, cause: Error) -> StartError {
        let cleanup = if let Some(runtime) = &mut self.runtime {
            matches!(
                tokio::time::timeout(Duration::from_secs(2), runtime.stop()).await,
                Ok(Ok(()))
            ) && runtime.contained
                && !runtime.uncertain_tree
        } else {
            true
        };
        let released = cleanup && self.lease.as_mut().unwrap().release().await.is_ok();
        if released {
            self.runtime.take();
        }
        StartError {
            cause,
            cleanup_confirmed: cleanup,
            lease_released: released,
        }
    }
}

fn executable(path: &Path, name: Option<&str>, system: bool) -> bool {
    // Cooperative path checks, not authentication against a hostile same-UID writer.
    path.is_absolute()
        && path
            .to_str()
            .is_some_and(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
        && name.is_none_or(|name| path.file_name().is_some_and(|n| n == name))
        && std::fs::symlink_metadata(path).is_ok_and(|m| {
            m.is_file()&&!m.file_type().is_symlink()
            && m.mode()&0o022==0 && m.mode()&0o100!=0 && m.mode()&0o6000==0
            // SAFETY: fixed UID query only.
            && m.uid()==if system {0} else {unsafe {libc::geteuid()}}
        })
}

struct Launch<'a> {
    bootstrap: &'a Path,
    program: &'a Path,
    args: Vec<String>,
    model: bool,
    pressure: Option<Pressure>,
    startup: Duration,
    authorization_deadline: Option<Instant>,
    admission_deadline: Option<Instant>,
}

fn validate_launch_deadlines(launch: &Launch<'_>, now: Instant) -> Result<(), Error> {
    if launch.authorization_deadline.is_some_and(|end| now >= end) {
        return Err(Error::Authorization);
    }
    if launch.admission_deadline.is_some_and(|end| now > end) {
        return Err(Error::Preparation);
    }
    Ok(())
}
async fn launch(
    mut reservation: Reservation,
    guard: MemoryGuard,
    policy: Policy,
    launch: Launch<'_>,
) -> Result<StartedRuntime, StartError> {
    let port = match reservation.release_runtime() {
        Ok(port) => port,
        Err(_) => {
            return Err(StartError {
                cause: Error::Preparation,
                cleanup_confirmed: true,
                lease_released: false,
            })
        }
    };
    let mut prepared = Preparation {
        runtime: None,
        lease: Some(reservation.into_session_lease()),
        guard,
        transferred: false,
    };
    if prepared.lease.as_ref().unwrap().verify_binding().is_err() {
        return Err(prepared.fail(Error::Preparation).await);
    }
    if !executable(Path::new("/usr/bin/sandbox-exec"), None, true)
        || !executable(launch.bootstrap, Some("seiya-session-bootstrap"), false)
        || !(if launch.model {
            launch
                .program
                .canonicalize()
                .is_ok_and(|path| executable(&path, None, false) || executable(&path, None, true))
        } else {
            executable(launch.program, Some("seiya-observe-fixture"), false)
        })
        || !(Duration::from_millis(100)..=Duration::from_secs(120)).contains(&launch.startup)
    {
        return Err(prepared.fail(Error::Executable).await);
    }
    let deadline = launch
        .authorization_deadline
        .map_or(Instant::now() + launch.startup, |expires| {
            expires.min(Instant::now() + launch.startup)
        });
    if deadline <= Instant::now() {
        return Err(prepared.fail(Error::Authorization).await);
    }
    let now = prepared.guard.now_millis().ok_or(Error::Pressure);
    let pressure = now.and_then(|now| match launch.pressure {
        Some(pressure) => Ok(PressureSample {
            sampled_at_millis: now,
            pressure,
        }),
        None => crate::session_pressure::sample(now).map_err(|_| Error::Pressure),
    });
    if pressure.is_err()
        || prepared
            .guard
            .observe(pressure.map_err(|_| campaign::MonitorStop::Unavailable))
            .is_err()
    {
        return Err(prepared.fail(Error::Pressure).await);
    }
    if prepared.guard.check().is_err() {
        return Err(prepared.fail(Error::Pressure).await);
    }
    if prepared.lease.as_ref().unwrap().verify_binding().is_err() {
        return Err(prepared.fail(Error::Preparation).await);
    }
    if let Err(cause) = validate_launch_deadlines(&launch, Instant::now()) {
        return Err(prepared.fail(cause).await);
    }
    let child = match Command::new("/usr/bin/sandbox-exec")
        .arg("-p")
        .arg(PROFILE)
        .arg(launch.bootstrap)
        .arg("--exec")
        .arg(launch.program)
        .args(&launch.args)
        .env_clear()
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONSAFEPATH", "1")
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return Err(prepared.fail(Error::Spawn).await),
    };
    prepared.runtime = Some(Runtime {
        child,
        identity: None,
        port,
        contained: false,
        uncertain_tree: false,
        term_sent: false,
        pressure: launch.pressure,
        authorization_deadline: launch.authorization_deadline,
    });
    if let Err(cause) = prepared
        .runtime
        .as_mut()
        .unwrap()
        .bootstrap(&prepared.guard, deadline)
        .await
    {
        return Err(prepared.fail(cause).await);
    }
    let runtime = prepared.runtime.take().unwrap();
    let identity = runtime.identity.as_ref().unwrap().clone();
    let lease = prepared.lease.take().unwrap();
    prepared.transferred = true;
    let supervisor = Supervisor::start_restricted(
        runtime,
        lease,
        prepared.guard.clone(),
        policy,
        launch.model,
        launch.pressure.is_none(),
    );
    Ok(StartedRuntime {
        supervisor,
        port,
        identity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_pressure_cannot_extend_preflight_or_authorization_deadlines() {
        let now = Instant::now();
        let mut launch = Launch {
            bootstrap: Path::new("/synthetic/seiya-session-bootstrap"),
            program: Path::new("/synthetic/python"),
            args: vec![],
            model: true,
            pressure: Some(Pressure::Normal),
            startup: Duration::from_secs(120),
            authorization_deadline: Some(now + Duration::from_secs(30)),
            admission_deadline: Some(now + Duration::from_secs(5)),
        };
        assert_eq!(validate_launch_deadlines(&launch, now), Ok(()));
        assert_eq!(
            validate_launch_deadlines(&launch, now + Duration::from_secs(5)),
            Ok(())
        );
        assert_eq!(
            validate_launch_deadlines(&launch, now + Duration::from_millis(5001)),
            Err(Error::Preparation)
        );
        // Neither a longer startup deadline nor a new normal pressure sample
        // refreshes the original preflight's free-page observation.
        launch.startup = Duration::from_secs(120);
        launch.pressure = Some(Pressure::Normal);
        assert_eq!(
            validate_launch_deadlines(&launch, now + Duration::from_secs(6)),
            Err(Error::Preparation)
        );
        launch.admission_deadline = Some(now + Duration::from_secs(60));
        assert_eq!(
            validate_launch_deadlines(&launch, now + Duration::from_secs(30)),
            Err(Error::Authorization)
        );
    }
}

/// Rehearsal ONLY: exact repository fixture program/arguments, never model paths.
pub async fn start_rehearsal(
    reservation: Reservation,
    bootstrap: &Path,
    fixture: &Path,
    guard: MemoryGuard,
    policy: Policy,
    pressure: Option<Pressure>,
) -> Result<StartedRuntime, StartError> {
    start_rehearsal_with_behavior(
        reservation,
        bootstrap,
        fixture,
        guard,
        policy,
        RehearsalOptions {
            pressure,
            behavior: RehearsalBehavior::Http,
        },
    )
    .await
}
#[derive(Clone, Copy)]
pub enum RehearsalBehavior {
    Http,
    IgnoreTerm,
    NoListener,
}
pub struct RehearsalOptions {
    pub pressure: Option<Pressure>,
    pub behavior: RehearsalBehavior,
}
pub async fn start_rehearsal_with_behavior(
    reservation: Reservation,
    bootstrap: &Path,
    fixture: &Path,
    guard: MemoryGuard,
    policy: Policy,
    options: RehearsalOptions,
) -> Result<StartedRuntime, StartError> {
    let mut on_failure = StopOnFailure {
        guard: guard.clone(),
        success: false,
    };
    let result =
        start_rehearsal_inner(reservation, bootstrap, fixture, guard, policy, options).await;
    on_failure.success = result.is_ok();
    result
}
async fn start_rehearsal_inner(
    reservation: Reservation,
    bootstrap: &Path,
    fixture: &Path,
    guard: MemoryGuard,
    policy: Policy,
    options: RehearsalOptions,
) -> Result<StartedRuntime, StartError> {
    if !executable(fixture, Some("seiya-observe-fixture"), false) {
        return Err(StartError {
            cause: Error::Executable,
            cleanup_confirmed: true,
            lease_released: true,
        });
    }
    let port = reservation
        .ports()
        .map_err(|_| StartError {
            cause: Error::Preparation,
            cleanup_confirmed: true,
            lease_released: false,
        })?
        .0;
    let args = match options.behavior {
        RehearsalBehavior::Http => vec![
            "--fixture-runtime".into(),
            "--port".into(),
            port.to_string(),
        ],
        RehearsalBehavior::IgnoreTerm => vec![
            "--fixture-runtime-ignore-term".into(),
            "--port".into(),
            port.to_string(),
        ],
        RehearsalBehavior::NoListener => vec!["--owned-session-ignore-eof".into()],
    };
    launch(
        reservation,
        guard,
        policy,
        Launch {
            bootstrap,
            program: fixture,
            args,
            model: false,
            pressure: options.pressure,
            startup: Duration::from_secs(5),
            authorization_deadline: None,
            admission_deadline: None,
        },
    )
    .await
}

/// Real model-capable route: NOT called by normal tests, CLI or HTTP. Approval
/// must reflect actual user authorization, budget review and external clearance.
/// Budget manifest is specifically the native weight-index SHA256 in this adapter.
pub struct MlxLaunch<'a> {
    pub recipe: &'a MlxRecipe,
    pub packages: &'a PackageMetadata,
    pub approval: &'a ModelLoadApproval,
    pub budget: &'a LoadBudget,
    pub bootstrap: &'a Path,
}
pub async fn start_mlx(
    request: MlxLaunch<'_>,
    guard: MemoryGuard,
    policy: Policy,
) -> Result<StartedRuntime, StartError> {
    let mut on_failure = StopOnFailure {
        guard: guard.clone(),
        success: false,
    };
    let result = start_mlx_inner(request, guard, policy).await;
    on_failure.success = result.is_ok();
    result
}
async fn start_mlx_inner(
    request: MlxLaunch<'_>,
    guard: MemoryGuard,
    policy: Policy,
) -> Result<StartedRuntime, StartError> {
    let reject = |cause| StartError {
        cause,
        cleanup_confirmed: true,
        lease_released: true,
    };
    if !request.approval.valid(request.recipe) {
        return Err(reject(Error::Authorization));
    }
    if request.recipe.minimum_free_bytes < 2 << 30 {
        return Err(reject(Error::Preparation));
    }
    let budget = request
        .budget
        .evaluate()
        .map_err(|_| reject(Error::Budget))?;
    if !budget.missing.is_empty() || request.budget.output_tokens != 256 {
        return Err(reject(Error::Budget));
    }
    let reservation =
        Reservation::acquire(request.recipe).map_err(|_| reject(Error::Preparation))?;
    let provenance = NativeProvenance::inspect(request.recipe, request.packages)
        .await
        .map_err(|_| reject(Error::Preparation))?;
    if !provenance.checkpoint.assets.iter().any(|a| {
        a.name == "model.safetensors.index.json" && a.sha256 == request.budget.manifest_sha256
    }) {
        return Err(reject(Error::Budget));
    }
    let epoch = Instant::now();
    let mut observations = vec![];
    for index in 0..3 {
        if index > 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        if !request.approval.valid(request.recipe) {
            return Err(reject(Error::Authorization));
        }
        observations.push(
            campaign::observe_host(epoch, true)
                .await
                .map_err(|_| reject(Error::Preparation))?,
        );
    }
    let now = epoch
        .elapsed()
        .as_millis()
        .try_into()
        .map_err(|_| reject(Error::Preparation))?;
    let host = observations.last().unwrap().clone();
    let review = campaign::review_pressure_window(&observations, now);
    let readiness = campaign::assess(request.recipe, host.clone(), now, Some(&reservation))
        .map_err(|_| reject(Error::Preparation))?;
    if !readiness.blockers.is_empty()
        || !review.normal_pressure_observed
        || host.free_bytes.is_none_or(|free| {
            budget
                .planned_runtime_and_host_margin_bytes
                .is_none_or(|planned| planned > free)
        })
    {
        return Err(reject(Error::Preparation));
    }
    if !request.approval.valid(request.recipe) {
        return Err(reject(Error::Authorization));
    }
    let port = reservation
        .ports()
        .map_err(|_| reject(Error::Preparation))?
        .0;
    let args = request
        .recipe
        .runtime_arguments(port)
        .map_err(|_| reject(Error::Preparation))?;
    let authorization_deadline = request
        .approval
        .deadline_for(request.recipe)
        .ok_or_else(|| reject(Error::Authorization))?;
    let admission_deadline = epoch
        .checked_add(Duration::from_millis(host.sampled_at_millis))
        .and_then(|instant| instant.checked_add(Duration::from_secs(5)))
        .ok_or_else(|| reject(Error::Preparation))?;
    let guard_now = guard
        .now_millis()
        .ok_or_else(|| reject(Error::Authorization))?;
    let remaining: u64 = authorization_deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .try_into()
        .map_err(|_| reject(Error::Authorization))?;
    guard
        .stop_at_millis(
            guard_now
                .checked_add(remaining)
                .ok_or_else(|| reject(Error::Authorization))?,
        )
        .map_err(|_| reject(Error::Authorization))?;
    // Release only the reserved runtime socket in launch, never the coordination
    // lock. Coordinator socket remains held until confirmed stop/release.
    launch(
        reservation,
        guard,
        policy,
        Launch {
            bootstrap: request.bootstrap,
            program: &request.recipe.python,
            args,
            model: true,
            pressure: None,
            startup: Duration::from_secs(120),
            authorization_deadline: Some(authorization_deadline),
            admission_deadline: Some(admission_deadline),
        },
    )
    .await
}
