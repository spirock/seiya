//! Native flock/metadata fixtures in NEW private directories only. No process
//! launch, MLX, model, shared operational lease, memory probe or external network.
#![cfg(target_os = "macos")]
use seiya_observe::{
    campaign::{MlxRecipe, PreparationError, Pressure, PressureSample, Reservation},
    session_guard::MemoryGuard,
    session_supervisor::{
        Coverage, LeaseOrigin, OwnedFuture, OwnedLease, OwnedRuntime, Ownership, Policy,
        ReapEvidence, Stage, Supervisor,
    },
    ProcessIdentity,
};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{symlink, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

const LOCK: &str = "seiya-mlx-baseline.lock";
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let base =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/session-lease-tests");
        fs::create_dir_all(&base).unwrap();
        let directory = base.join(uuid::Uuid::new_v4().to_string());
        let mut builder = fs::DirBuilder::new();
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700).create(&directory).unwrap();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join(LOCK))
            .unwrap();
        file.write_all(b"synthetic-lock-marker").unwrap();
        Self(directory.canonicalize().unwrap())
    }
    fn recipe(&self) -> MlxRecipe {
        MlxRecipe {
            python: PathBuf::from("/synthetic/python"),
            runtime_entrypoint: PathBuf::from("/synthetic/mlx-server"),
            checkpoint: PathBuf::from("/synthetic/checkpoint"),
            coordination_directory: self.0.clone(),
            minimum_free_bytes: 2 << 30,
        }
    }
    fn cleanup(self) {
        // Only this new fixture's file, AFTER release and all reservations drop.
        // Never unlink a live or operational lockfile to release it.
        fs::remove_file(self.0.join(LOCK)).unwrap();
        fs::remove_dir(self.0).unwrap();
    }
}
fn assert_busy(path: &Path) {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    // SAFETY: this fixture's descriptor, fixed nonblocking operation; Drop closes it.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    assert_eq!(result, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EWOULDBLOCK)
    );
}

#[tokio::test]
async fn native_binding_keeps_kernel_exclusion_until_explicit_idempotent_release() {
    let directory = Directory::new();
    let recipe = directory.recipe();
    let stamp = fs::metadata(directory.0.join(LOCK)).unwrap().ino();
    let reservation = Reservation::acquire(&recipe).unwrap();
    let ports = reservation.ports().unwrap();
    let mut lease = reservation.into_session_lease();
    assert_eq!(lease.origin(), LeaseOrigin::NativeCoordinationLock);
    lease.verify_binding().unwrap();
    assert_busy(&directory.0.join(LOCK));
    assert!(matches!(
        Reservation::acquire(&recipe),
        Err(PreparationError::Busy)
    ));
    assert!(std::net::TcpListener::bind(("127.0.0.1", ports.0)).is_err());
    assert!(std::net::TcpListener::bind(("127.0.0.1", ports.1)).is_err());
    lease.release().await.unwrap();
    lease.release().await.unwrap();
    assert!(lease.verify_binding().is_err());
    assert_eq!(fs::metadata(directory.0.join(LOCK)).unwrap().ino(), stamp);
    assert_eq!(
        fs::read(directory.0.join(LOCK)).unwrap(),
        b"synthetic-lock-marker"
    );
    let next = Reservation::acquire(&recipe).unwrap();
    // Idempotence must not unlock a later reservation even if the OS reused FDs.
    lease.release().await.unwrap();
    assert_busy(&directory.0.join(LOCK));
    drop(next);
    drop(lease);
    directory.cleanup();
}

#[tokio::test]
async fn replaced_path_never_unlocks_the_original_inode_and_can_be_restored_explicitly() {
    let directory = Directory::new();
    let recipe = directory.recipe();
    let mut lease = Reservation::acquire(&recipe).unwrap().into_session_lease();
    let original = directory.0.join(LOCK);
    let retained = directory.0.join("retained.lock");
    fs::rename(&original, &retained).unwrap();
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&original)
        .unwrap();
    assert!(lease.verify_binding().is_err());
    assert!(lease.release().await.is_err());
    assert_busy(&retained);
    assert_eq!(fs::read(&retained).unwrap(), b"synthetic-lock-marker");
    // Restore ONLY our synthetic replacement; this is not a lease-release mechanism.
    fs::remove_file(&original).unwrap();
    fs::rename(&retained, &original).unwrap();
    lease.release().await.unwrap();
    drop(lease);
    let next = Reservation::acquire(&recipe).unwrap();
    drop(next);
    directory.cleanup();
}

#[tokio::test]
async fn replacing_directory_binding_fails_even_if_the_original_lock_inode_is_moved_into_it() {
    let directory = Directory::new();
    let recipe = directory.recipe();
    let mut lease = Reservation::acquire(&recipe).unwrap().into_session_lease();
    let retained = directory.0.with_extension("retained");
    fs::rename(&directory.0, &retained).unwrap();
    let mut builder = fs::DirBuilder::new();
    use std::os::unix::fs::DirBuilderExt;
    builder.mode(0o700).create(&directory.0).unwrap();
    fs::rename(retained.join(LOCK), directory.0.join(LOCK)).unwrap();
    assert!(lease.verify_binding().is_err());
    assert!(lease.release().await.is_err());
    assert_busy(&directory.0.join(LOCK));
    fs::rename(directory.0.join(LOCK), retained.join(LOCK)).unwrap();
    fs::remove_dir(&directory.0).unwrap();
    fs::rename(retained, &directory.0).unwrap();
    lease.release().await.unwrap();
    drop(lease);
    directory.cleanup();
}

#[tokio::test]
async fn dropping_an_unpolled_release_future_does_not_unlock_the_reservation() {
    let directory = Directory::new();
    let recipe = directory.recipe();
    let mut lease = Reservation::acquire(&recipe).unwrap().into_session_lease();
    drop(lease.release());
    assert_busy(&directory.0.join(LOCK));
    assert!(matches!(
        Reservation::acquire(&recipe),
        Err(PreparationError::Busy)
    ));
    lease.release().await.unwrap();
    drop(lease);
    directory.cleanup();
}

#[tokio::test]
async fn symlinks_hardlinks_and_unsafe_permissions_fail_closed_without_touching_targets() {
    let directory = Directory::new();
    let recipe = directory.recipe();
    let mut lease = Reservation::acquire(&recipe).unwrap().into_session_lease();
    let original = directory.0.join(LOCK);
    let retained = directory.0.join("retained.lock");
    let target = directory.0.join("foreign-fixture.txt");
    fs::write(&target, b"unchanged").unwrap();
    fs::rename(&original, &retained).unwrap();
    symlink(&target, &original).unwrap();
    assert!(lease.release().await.is_err());
    assert_busy(&retained);
    assert_eq!(fs::read(&target).unwrap(), b"unchanged");
    fs::remove_file(&original).unwrap();
    fs::rename(&retained, &original).unwrap();
    fs::remove_file(target).unwrap();
    let alias = directory.0.join("alias.lock");
    fs::hard_link(&original, &alias).unwrap();
    assert!(lease.release().await.is_err());
    assert_busy(&original);
    fs::remove_file(alias).unwrap();
    fs::set_permissions(&original, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(lease.release().await.is_err());
    assert_busy(&original);
    fs::set_permissions(&original, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(lease.release().await.is_err());
    assert_busy(&original);
    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700)).unwrap();
    lease.release().await.unwrap();
    drop(lease);
    directory.cleanup();
}

#[tokio::test]
async fn missing_initial_binding_is_not_silently_rebound_to_a_new_lockfile() {
    let directory = Directory::new();
    let recipe = directory.recipe();
    let reservation = Reservation::acquire(&recipe).unwrap();
    let original = directory.0.join(LOCK);
    let retained = directory.0.join("retained.lock");
    fs::rename(&original, &retained).unwrap();
    let mut lease = reservation.into_session_lease();
    fs::rename(&retained, &original).unwrap();
    assert!(lease.verify_binding().is_err());
    assert!(lease.release().await.is_err());
    assert_busy(&original);
    // No runtime was started. Dropping this unused preparation is safe; an actor
    // with unconfirmed processes would instead retain the capability in quarantine.
    drop(lease);
    let next = Reservation::acquire(&recipe).unwrap();
    drop(next);
    directory.cleanup();
}

struct Runtime {
    complete: Arc<AtomicBool>,
    stops: Arc<AtomicUsize>,
}
fn identity() -> ProcessIdentity {
    ProcessIdentity::new(73, "synthetic-runtime-only").unwrap()
}
impl OwnedRuntime for Runtime {
    fn ownership(&self) -> Ownership {
        Ownership::new(identity(), vec![]).unwrap()
    }
    fn sample_pressure(&mut self, time: u64) -> OwnedFuture<'_, PressureSample> {
        Box::pin(async move {
            Ok(PressureSample {
                sampled_at_millis: time,
                pressure: Pressure::Warning,
            })
        })
    }
    fn stop_and_reap(&mut self) -> OwnedFuture<'_, ReapEvidence> {
        Box::pin(async move {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(ReapEvidence {
                root: identity(),
                reaped: vec![identity()],
                coverage: if self.complete.load(Ordering::SeqCst) {
                    Coverage::Complete
                } else {
                    Coverage::UntrackedDescendants
                },
            })
        })
    }
}

#[tokio::test]
async fn actor_retains_real_lock_on_incomplete_synthetic_cleanup_and_releases_after_recovery() {
    let directory = Directory::new();
    let recipe = directory.recipe();
    let lease = Reservation::acquire(&recipe).unwrap().into_session_lease();
    lease.verify_binding().unwrap();
    let complete = Arc::new(AtomicBool::new(false));
    let stops = Arc::new(AtomicUsize::new(0));
    let policy = Policy::new(
        Duration::from_secs(30),
        Duration::from_millis(100),
        Duration::from_millis(100),
    )
    .unwrap();
    let mut supervisor = Supervisor::start_injected(
        Runtime {
            complete: complete.clone(),
            stops: stops.clone(),
        },
        lease,
        MemoryGuard::new(),
        policy,
    );
    let status = supervisor.wait_terminal().await.unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../contracts/benchmarks/chat-session/v0.1.0/native-lease-status.json"
    ))
    .unwrap();
    assert_eq!(
        serde_json::to_value(&status).unwrap(),
        fixture["quarantined"]
    );
    assert_eq!(status.version, 2);
    assert_eq!(status.mode, "injected-supervisor");
    assert_eq!(status.model_inference, Some(false));
    assert_eq!(
        status.lease_origin,
        Some(LeaseOrigin::NativeCoordinationLock)
    );
    assert_eq!(status.stage, Stage::Quarantined);
    assert!(!status.lease_release_confirmed);
    assert_busy(&directory.0.join(LOCK));
    assert!(matches!(
        Reservation::acquire(&recipe),
        Err(PreparationError::Busy)
    ));
    complete.store(true, Ordering::SeqCst);
    let status = supervisor.retry_cleanup().await.unwrap();
    assert_eq!(serde_json::to_value(&status).unwrap(), fixture["stopped"]);
    assert_eq!(status.stage, Stage::Stopped);
    assert!(status.lease_release_confirmed);
    assert_eq!(stops.load(Ordering::SeqCst), 2);
    let next = Reservation::acquire(&recipe).unwrap();
    drop(next);
    drop(supervisor);
    directory.cleanup();
}
