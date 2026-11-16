//! Cooperative model ownership. Never unlink a lock or recover quarantine here.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

pub const LEGACY_BASE: &str = "/private/var/folders/tc/6nkb5t5d5116q143rzc2dy400000gn/T/opencode";
const LOCK: &str = "seiya-mlx-baseline.lock";
pub const NATIVE_INFLIGHT: &str = "native-model.inflight";
const MARKERS: [&str; 2] = [
    "image-cleanup-uncertain.json",
    "pair-cleanup-uncertain.json",
];
const TEST_INFLIGHT: &str = "native-model-test-8016.inflight";
const TEST_QUARANTINE: &str = "test-cleanup-uncertain.json";
const REVIEWED_INFLIGHT: &str = "native-model-test-8016-reviewed.inflight";
const REVIEWED_QUARANTINE: &str = "test-reviewed-cleanup-uncertain.json";

// Historical process-group review is implemented only by the macOS probe.
// Other platforms must not treat an unverified group as empty.
fn reviewed_group_is_empty(pid: u32) -> Result<bool, ()> {
    #[cfg(target_os = "macos")]
    {
        crate::probes::exited_group_is_empty(pid).map_err(|_| ())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        Err(())
    }
}

fn marker_present(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("review_marker_observation_failed".into()),
    }
}
fn private_marker(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| "review_marker_open")?;
    let m = file.metadata().map_err(|_| "review_marker_metadata")?;
    let current = fs::symlink_metadata(path).map_err(|_| "review_marker_changed")?;
    if !m.is_file()
        || m.nlink() != 1
        || m.permissions().mode() & 0o077 != 0
        || m.len() > 4096
        || m.dev() != current.dev()
        || m.ino() != current.ino()
    {
        return Err("review_marker_invalid".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "review_marker_read")?;
    if bytes.len() > 4096 {
        return Err("review_marker_limit".into());
    }
    Ok(bytes)
}

#[derive(Clone)]
struct Reviewed {
    intent: [u8; 32],
    quarantine: [u8; 32],
    evidence: PathBuf,
    owner: [u8; 32],
    cleanup: [u8; 32],
    pid: u32,
    port: u16,
    test: Option<ReviewedTest>,
}

#[derive(Clone)]
struct ReviewedTest {
    intent: [u8; 32],
    quarantine: [u8; 32],
    evidence: PathBuf,
    owner: [u8; 32],
    cleanup: [u8; 32],
    pid: u32,
    port: u16,
}

fn review_test(stable: &Path, base: &Path) -> Result<Option<ReviewedTest>, String> {
    let pending = marker_present(&stable.join(TEST_INFLIGHT))?;
    let quarantined = marker_present(&stable.join(TEST_QUARANTINE))?;
    if !pending && !quarantined {
        return Ok(None);
    }
    if !pending || !quarantined {
        return Err("review_test_incomplete".into());
    }
    let intent = private_marker(&stable.join(TEST_INFLIGHT))?;
    let marker = private_marker(&stable.join(TEST_QUARANTINE))?;
    let value: Value = serde_json::from_slice(&intent).map_err(|_| "review_test_intent_json")?;
    let quarantine: Value =
        serde_json::from_slice(&marker).map_err(|_| "review_test_quarantine_json")?;
    let pid = quarantine["pid"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v > 0 && *v <= i32::MAX as u32)
        .ok_or("review_test_pid_invalid")?;
    let evidence = Path::new(
        quarantine["evidence"]
            .as_str()
            .ok_or("review_test_evidence")?,
    );
    let canonical = evidence
        .canonicalize()
        .map_err(|_| "review_test_evidence")?;
    if canonical != evidence || !canonical.starts_with(base) || canonical == base {
        return Err("review_test_evidence_outside_data".into());
    }
    let owner_bytes = private_marker(&canonical.join("owner.json"))?;
    let cleanup_bytes = private_marker(&canonical.join("cleanup.json"))?;
    let owner: Value =
        serde_json::from_slice(&owner_bytes).map_err(|_| "review_test_owner_json")?;
    let cleanup: Value =
        serde_json::from_slice(&cleanup_bytes).map_err(|_| "review_test_cleanup_json")?;
    let port = owner["port"]
        .as_u64()
        .and_then(|v| u16::try_from(v).ok())
        .filter(|v| *v >= 49152)
        .ok_or("review_test_port_invalid")?;
    if value != serde_json::json!({"version":1,"operation":"ollama","cleanup_confirmed":false})
        || quarantine["error"] != "OwnershipChanged"
        || owner["pid"] != pid
        || cleanup["confirmed"] != false
        || cleanup["error"] != "OwnershipChanged"
        || !reviewed_group_is_empty(pid).map_err(|_| "review_test_process_unavailable")?
    {
        return Err("review_test_unexpected_marker".into());
    }
    let _reserved = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .map_err(|_| "review_test_private_port_busy")?;
    Ok(Some(ReviewedTest {
        intent: Sha256::digest(intent).into(),
        quarantine: Sha256::digest(marker).into(),
        evidence: canonical,
        owner: Sha256::digest(owner_bytes).into(),
        cleanup: Sha256::digest(cleanup_bytes).into(),
        pid,
        port,
    }))
}

#[derive(Clone)]
pub struct Coordination {
    stable: Option<PathBuf>,
    legacy: PathBuf,
    reviewed: Option<Reviewed>,
}

impl Coordination {
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn isolated_for_test(directory: PathBuf) -> Self {
        Self {
            legacy: directory.join("absent-legacy"),
            stable: Some(directory),
            reviewed: None,
        }
    }
    pub fn legacy() -> Self {
        Self {
            stable: None,
            legacy: Path::new(LEGACY_BASE).join("seiya-host-model-coordination"),
            reviewed: None,
        }
    }

    /// Only an explicitly reviewed debug desktop instance may select the
    /// alternate journal. The old observation remains unchanged and visible.
    pub fn with_development_review(mut self) -> Result<Self, String> {
        if !cfg!(target_os = "macos") || !cfg!(debug_assertions) || self.stable.is_none() {
            return Err("development_review_not_allowed".into());
        }
        let stable = self.stable.as_ref().unwrap();
        let original: Value = serde_json::from_slice(&private_marker(
            &stable.join("pair-cleanup-uncertain.json"),
        )?)
        .map_err(|_| "review_quarantine_json")?;
        let pid = original["pid"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v > 0 && *v <= i32::MAX as u32)
            .ok_or("review_pid_invalid")?;
        let base = stable.parent().ok_or("review_home")?.join("data");
        let evidence = Path::new(original["evidence"].as_str().ok_or("review_evidence")?);
        let canonical = evidence.canonicalize().map_err(|_| "review_evidence")?;
        if canonical != evidence || !canonical.starts_with(&base) || canonical == base {
            return Err("review_evidence_outside_data".into());
        }
        let owner: Value = serde_json::from_slice(&private_marker(&canonical.join("owner.json"))?)
            .map_err(|_| "review_owner_json")?;
        let port = owner["port"]
            .as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .filter(|v| *v >= 49152)
            .ok_or("review_port_invalid")?;
        let owner_hash: [u8; 32] =
            Sha256::digest(private_marker(&canonical.join("owner.json"))?).into();
        let cleanup_hash: [u8; 32] =
            Sha256::digest(private_marker(&canonical.join("cleanup.json"))?).into();
        if !reviewed_group_is_empty(pid).map_err(|_| "review_process_unavailable")? {
            return Err("review_process_or_group_present".into());
        }
        let _reserved = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .map_err(|_| "review_private_port_busy")?;
        let test = review_test(stable, &base)?;
        let mut fingerprints = Vec::new();
        for domain in [self.stable.as_ref().unwrap(), &self.legacy] {
            private_directory(domain)?;
            let intent = private_marker(&domain.join(NATIVE_INFLIGHT))?;
            let marker = private_marker(&domain.join("pair-cleanup-uncertain.json"))?;
            let value: Value = serde_json::from_slice(&intent).map_err(|_| "review_intent_json")?;
            let quarantine: Value =
                serde_json::from_slice(&marker).map_err(|_| "review_quarantine_json")?;
            let evidence = Path::new(quarantine["evidence"].as_str().ok_or("review_evidence")?);
            let canonical = evidence.canonicalize().map_err(|_| "review_evidence")?;
            if canonical != evidence || !canonical.starts_with(&base) || canonical == base {
                return Err("review_evidence_outside_data".into());
            }
            let owner: Value =
                serde_json::from_slice(&private_marker(&canonical.join("owner.json"))?)
                    .map_err(|_| "review_owner_json")?;
            let cleanup: Value =
                serde_json::from_slice(&private_marker(&canonical.join("cleanup.json"))?)
                    .map_err(|_| "review_cleanup_json")?;
            if value
                != serde_json::json!({"version":1,"operation":"ollama","cleanup_confirmed":false})
                || quarantine["pid"] != pid
                || quarantine["error"] != "RootExited"
                || owner["pid"] != pid
                || owner["port"] != port
                || cleanup["confirmed"] != false
                || cleanup["error"] != "RootExited"
                || marker_present(&domain.join("image-cleanup-uncertain.json"))?
                || marker_present(&domain.join(REVIEWED_INFLIGHT))?
                || marker_present(&domain.join(REVIEWED_QUARANTINE))?
            {
                return Err("review_unexpected_marker".into());
            }
            if let Some(review) = &test {
                if Sha256::digest(private_marker(&domain.join(TEST_INFLIGHT))?).as_slice()
                    != review.intent
                    || Sha256::digest(private_marker(&domain.join(TEST_QUARANTINE))?).as_slice()
                        != review.quarantine
                {
                    return Err("review_test_domains_differ".into());
                }
            } else if marker_present(&domain.join(TEST_INFLIGHT))?
                || marker_present(&domain.join(TEST_QUARANTINE))?
            {
                return Err("review_unexpected_marker".into());
            }
            for entry in fs::read_dir(domain).map_err(|_| "review_listing")? {
                let name = entry.map_err(|_| "review_entry")?.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("http-") && name.ends_with(".inflight") {
                    return Err("review_http_pending".into());
                }
            }
            fingerprints.push((Sha256::digest(intent).into(), Sha256::digest(marker).into()));
        }
        if fingerprints[0] != fingerprints[1] {
            return Err("review_domains_differ".into());
        }
        self.reviewed = Some(Reviewed {
            intent: fingerprints[0].0,
            quarantine: fingerprints[0].1,
            evidence: canonical,
            owner: owner_hash,
            cleanup: cleanup_hash,
            pid,
            port,
            test,
        });
        Ok(self)
    }

    /// The config cannot elect a different authority for each profile.
    pub fn portable(home: &Path, configured: &Path) -> Result<Self, String> {
        let expected = home.join("Library/Application Support/Seiya/coordination");
        private_directory(&expected)?;
        if configured != expected {
            return Err("stable_coordination_directory_required".into());
        }
        Ok(Self {
            stable: Some(expected),
            ..Self::legacy()
        })
    }

    pub fn acquire(&self) -> Result<ModelLease, String> {
        let mut domains = Vec::new();
        if let Some(stable) = &self.stable {
            domains.push((stable, true));
        }
        match fs::symlink_metadata(&self.legacy) {
            Ok(_) => domains.push((&self.legacy, false)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && self.stable.is_some() => {}
            Err(_) => return Err("legacy_coordination_unavailable".into()),
        }
        let mut lease = ModelLease {
            files: Vec::new(),
            domains: Vec::new(),
            journal: match &self.reviewed {
                Some(review) if review.test.is_some() => REVIEWED_INFLIGHT,
                Some(_) => TEST_INFLIGHT,
                None => NATIVE_INFLIGHT,
            },
        };
        for (directory, create) in domains {
            private_directory(directory)?;
            let path = directory.join(LOCK);
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(create)
                .truncate(false)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&path)
                .map_err(|_| "shared_lease_open")?;
            let metadata = file.metadata().map_err(|_| "shared_lease_metadata")?;
            if !metadata.is_file()
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err("invalid_lease_file".into());
            }
            // SAFETY: owned valid fd; cooperative nonblocking flock, never unlinked.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err("shared_lease_busy".into());
            }
            let current = fs::symlink_metadata(&path).map_err(|_| "shared_lease_changed")?;
            if current.dev() != metadata.dev() || current.ino() != metadata.ino() {
                return Err("shared_lease_changed".into());
            }
            lease.files.push(file);
            lease.domains.push(directory.clone());
        }
        for domain in &lease.domains {
            if let Some(review) = &self.reviewed {
                if Sha256::digest(private_marker(&review.evidence.join("owner.json"))?).as_slice()
                    != review.owner
                    || Sha256::digest(private_marker(&review.evidence.join("cleanup.json"))?)
                        .as_slice()
                        != review.cleanup
                {
                    return Err("review_evidence_changed".into());
                }
                if !reviewed_group_is_empty(review.pid).map_err(|_| "review_process_unavailable")?
                    || std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, review.port))
                        .is_err()
                {
                    return Err("review_original_process_or_port_present".into());
                }
                if Sha256::digest(private_marker(&domain.join(NATIVE_INFLIGHT))?).as_slice()
                    != review.intent
                    || Sha256::digest(private_marker(&domain.join("pair-cleanup-uncertain.json"))?)
                        .as_slice()
                        != review.quarantine
                {
                    return Err("review_historical_records_changed".into());
                }
                if let Some(test) = &review.test {
                    if Sha256::digest(private_marker(&test.evidence.join("owner.json"))?).as_slice()
                        != test.owner
                        || Sha256::digest(private_marker(&test.evidence.join("cleanup.json"))?)
                            .as_slice()
                            != test.cleanup
                        || !reviewed_group_is_empty(test.pid)
                            .map_err(|_| "review_test_process_unavailable")?
                        || std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, test.port))
                            .is_err()
                        || Sha256::digest(private_marker(&domain.join(TEST_INFLIGHT))?).as_slice()
                            != test.intent
                        || Sha256::digest(private_marker(&domain.join(TEST_QUARANTINE))?).as_slice()
                            != test.quarantine
                    {
                        return Err("review_test_records_changed".into());
                    }
                }
            }
            // Current native hosts also honor pending external HTTP operations.
            // Never inspect their contents as authority or recover them here.
            let entries = fs::read_dir(domain).map_err(|_| "quarantine_observation_failed")?;
            for (index, entry) in entries.enumerate() {
                if index >= 4096 {
                    return Err("coordination_directory_limit".into());
                }
                let name = entry
                    .map_err(|_| "quarantine_observation_failed")?
                    .file_name();
                let name = name.to_string_lossy();
                if name == NATIVE_INFLIGHT && self.reviewed.is_none() {
                    return Err("native_model_pending_operator_review_required".into());
                }
                if (name == TEST_INFLIGHT || name == TEST_QUARANTINE)
                    && self.reviewed.as_ref().is_none_or(|r| r.test.is_none())
                {
                    return Err("test_model_pending_operator_review_required".into());
                }
                if name == REVIEWED_INFLIGHT || name == REVIEWED_QUARANTINE {
                    return Err("test_model_pending_operator_review_required".into());
                }
                if name.starts_with("http-") && name.ends_with(".inflight") {
                    return Err("external_http_pending_operator_review_required".into());
                }
            }
            for marker in MARKERS {
                if self.reviewed.is_some() && marker == "pair-cleanup-uncertain.json" {
                    continue;
                }
                match fs::symlink_metadata(domain.join(marker)) {
                    Ok(_) => {
                        return Err("model_cleanup_quarantined_operator_review_required".into())
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err("quarantine_observation_failed".into()),
                }
            }
        }
        Ok(lease)
    }
}

pub struct ModelLease {
    files: Vec<File>,
    domains: Vec<PathBuf>,
    journal: &'static str,
}

impl ModelLease {
    /// Persist before spawning an owned model process. The caller must retain
    /// this model lease until native operation cleanup/intent confirmation.
    pub fn begin_native(&self, operation: &str) -> Result<NativeOperation, String> {
        if !matches!(operation, "ollama" | "image")
            || self.journal == REVIEWED_INFLIGHT && operation != "ollama"
        {
            return Err("native_operation_invalid".into());
        }
        let bytes = serde_json::to_vec(
            &serde_json::json!({"version":1,"operation":operation,"cleanup_confirmed":false}),
        )
        .map_err(|_| "native_intent_encode")?;
        let mut records = vec![];
        for directory in &self.domains {
            private_directory(directory)?;
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(directory.join(self.journal))
                .map_err(|_| "native_intent_create")?;
            // If any persistence step fails, leave every observed intent in
            // place. No process may be spawned after this error.
            (&file)
                .write_all(&bytes)
                .and_then(|_| file.sync_all())
                .and_then(|_| File::open(directory)?.sync_all())
                .map_err(|_| "native_intent_persist")?;
            records.push((directory.clone(), file));
        }
        if records.is_empty() {
            return Err("native_intent_authority_required".into());
        }
        Ok(NativeOperation {
            records,
            journal: self.journal,
        })
    }
    /// Caller must retain/leak the lease if this cannot durably mark every domain.
    pub fn quarantine(&self, name: &str, marker: &Value) -> Result<(), String> {
        if !MARKERS.contains(&name) {
            return Err("invalid_quarantine_marker".into());
        }
        let name = match self.journal {
            TEST_INFLIGHT => TEST_QUARANTINE,
            REVIEWED_INFLIGHT => REVIEWED_QUARANTINE,
            _ => name,
        };
        let bytes = serde_json::to_vec_pretty(marker).map_err(|_| "quarantine_encode")?;
        let mut failed = false;
        for directory in &self.domains {
            let result = (|| -> std::io::Result<()> {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(directory.join(name))?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                File::open(directory)?.sync_all()
            })();
            // Even an existing marker is an error here: retain lock on uncertain persistence.
            failed |= result.is_err();
        }
        if failed {
            Err("quarantine_persistence_failed".into())
        } else {
            Ok(())
        }
    }
}

/// Drop intentionally retains markers: loss of the future/process never proves
/// that a model was unloaded. Only owned native cleanup may call confirm().
pub struct NativeOperation {
    records: Vec<(PathBuf, File)>,
    journal: &'static str,
}
impl NativeOperation {
    pub fn confirm(self) -> Result<(), String> {
        fn unchanged(directory: &Path, file: &File, journal: &str) -> Result<(), String> {
            private_directory(directory)?;
            let expected = file.metadata().map_err(|_| "native_intent_metadata")?;
            let current = fs::symlink_metadata(directory.join(journal))
                .map_err(|_| "native_intent_missing")?;
            if !current.is_file()
                || current.file_type().is_symlink()
                || current.nlink() != 1
                || expected.dev() != current.dev()
                || expected.ino() != current.ino()
            {
                return Err("native_intent_changed".into());
            }
            Ok(())
        }
        for (directory, file) in &self.records {
            unchanged(directory, file, self.journal)?;
        }
        for (directory, file) in &self.records {
            unchanged(directory, file, self.journal)?;
            fs::remove_file(directory.join(self.journal))
                .and_then(|_| File::open(directory)?.sync_all())
                .map_err(|_| "native_intent_confirm")?;
        }
        Ok(())
    }
}

pub fn private_directory(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .canonicalize()
            .map_err(|_| "private_directory_missing")?
            != path
    {
        return Err("private_canonical_directory_required".into());
    }
    let m = fs::symlink_metadata(path).map_err(|_| "private_directory_metadata")?;
    if !m.is_dir() || m.permissions().mode() & 0o077 != 0 {
        return Err("private_directory_required".into());
    }
    Ok(())
}

pub fn require_new_in(path: &Path, parent: &Path) -> Result<(), String> {
    private_directory(parent)?;
    if path.parent() != Some(parent)
        || fs::symlink_metadata(path).is_ok()
        || path.file_name().is_none()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("new_private_directory_required".into());
    }
    Ok(())
}

/// One host per data profile; separate from the short-lived inference lease.
pub fn profile_lock(directory: &Path) -> Result<File, String> {
    private_directory(directory)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory.join("desktop-host.lock"))
        .map_err(|_| "profile_lock_open")?;
    let m = file.metadata().map_err(|_| "profile_lock_metadata")?;
    if !m.is_file() || m.nlink() != 1 || m.permissions().mode() & 0o077 != 0 {
        return Err("invalid_profile_lock".into());
    }
    // SAFETY: valid owned fd, nonblocking cooperative lock; never unlinked.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("profile_already_in_use".into());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, DirBuilderExt};

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn unsupported_platform_cannot_approve_historical_process_group() {
        assert!(reviewed_group_is_empty(1).is_err());
        let f = Fixture::new();
        assert_eq!(
            f.coord.with_development_review().err().as_deref(),
            Some("development_review_not_allowed")
        );
    }

    struct Fixture {
        root: PathBuf,
        coord: Coordination,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(format!("seiya-coordination-{}", uuid::Uuid::new_v4()));
            fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
            for name in ["stable", "legacy"] {
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(root.join(name))
                    .unwrap();
            }
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(root.join("legacy").join(LOCK))
                .unwrap();
            Self {
                coord: Coordination {
                    stable: Some(root.join("stable")),
                    legacy: root.join("legacy"),
                    reviewed: None,
                },
                root,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    #[cfg(all(target_os = "macos", debug_assertions))]
    #[test]
    fn reviewed_dev_uses_new_journal_without_changing_historical_records() {
        let f = Fixture::new();
        let pid = i32::MAX as u32 - 11;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(port >= 49152);
        let evidence = f.root.join("data/run-synthetic/chat-synthetic");
        fs::create_dir_all(&evidence).unwrap();
        for (name, value) in [
            ("owner.json", serde_json::json!({"pid":pid,"port":port})),
            (
                "cleanup.json",
                serde_json::json!({"confirmed":false,"error":"RootExited"}),
            ),
        ] {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(evidence.join(name))
                .unwrap();
            file.write_all(value.to_string().as_bytes()).unwrap();
        }
        let intent =
            serde_json::json!({"version":1,"operation":"ollama","cleanup_confirmed":false})
                .to_string();
        let quarantine =
            serde_json::json!({"pid":pid,"error":"RootExited","evidence":evidence}).to_string();
        for domain in ["stable", "legacy"] {
            for (name, data) in [
                (NATIVE_INFLIGHT, &intent),
                ("pair-cleanup-uncertain.json", &quarantine),
            ] {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(f.root.join(domain).join(name))
                    .unwrap();
                file.write_all(data.as_bytes()).unwrap();
            }
        }
        assert!(f.coord.acquire().is_err());
        let occupied = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
        assert!(f.coord.clone().with_development_review().is_err());
        drop(occupied);
        let legacy_marker = f.root.join("legacy/pair-cleanup-uncertain.json");
        fs::write(&legacy_marker, b"different record").unwrap();
        assert!(f.coord.clone().with_development_review().is_err());
        fs::write(&legacy_marker, quarantine.as_bytes()).unwrap(); // fixture only
        let reviewed = f.coord.clone().with_development_review().unwrap();
        assert!(Coordination {
            stable: None,
            legacy: f.root.join("legacy"),
            reviewed: None
        }
        .acquire()
        .is_err());
        let lease = reviewed.acquire().unwrap();
        lease.begin_native("ollama").unwrap().confirm().unwrap();
        assert!(!f.root.join("stable").join(TEST_INFLIGHT).exists());
        assert!(f.root.join("stable").join(NATIVE_INFLIGHT).exists());
        assert!(f.coord.acquire().is_err());
        let intent_new = lease.begin_native("ollama").unwrap();
        assert!(f.root.join("stable").join(TEST_INFLIGHT).exists());
        drop(intent_new);
        drop(lease);
        assert!(reviewed.acquire().is_err()); // unconfirmed NEW operation, never ignored
        for domain in ["stable", "legacy"] {
            assert_eq!(
                fs::read_to_string(f.root.join(domain).join(NATIVE_INFLIGHT)).unwrap(),
                intent
            );
            assert_eq!(
                fs::read_to_string(f.root.join(domain).join("pair-cleanup-uncertain.json"))
                    .unwrap(),
                quarantine
            );
            fs::remove_file(f.root.join(domain).join(TEST_INFLIGHT)).unwrap(); // fixture only
        }
        let lease = reviewed.acquire().unwrap();
        lease
            .quarantine(
                "pair-cleanup-uncertain.json",
                &serde_json::json!({"synthetic":true}),
            )
            .unwrap();
        drop(lease);
        assert!(reviewed.acquire().is_err());
        assert!(f.coord.acquire().is_err());
        for domain in ["stable", "legacy"] {
            fs::remove_file(f.root.join(domain).join(TEST_QUARANTINE)).unwrap();
            // fixture only
        }
        let path = f.root.join("stable").join(NATIVE_INFLIGHT);
        fs::write(&path, b"changed historical record").unwrap();
        assert!(reviewed.acquire().is_err());
        fs::write(&path, intent.as_bytes()).unwrap();
        fs::write(evidence.join("cleanup.json"), b"changed evidence").unwrap();
        assert!(reviewed.acquire().is_err());
    }
    #[cfg(not(debug_assertions))]
    #[test]
    fn release_rejects_development_review() {
        let f = Fixture::new();
        assert!(f.coord.clone().with_development_review().is_err());
    }
    #[cfg(all(target_os = "macos", debug_assertions))]
    #[test]
    fn second_review_preserves_both_histories_and_rejects_new_uncertainty() {
        let f = Fixture::new();
        let old_pid = i32::MAX as u32 - 31;
        let new_pid = i32::MAX as u32 - 32;
        let old_reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let new_reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = old_reservation.local_addr().unwrap().port();
        let new_port = new_reservation.local_addr().unwrap().port();
        assert!(port >= 49152);
        assert!(new_port >= 49152);
        drop(old_reservation);
        drop(new_reservation);
        let old = f.root.join("data/run-old/chat-old");
        let new = f.root.join("data/run-new/chat-new");
        for (evidence, pid, error) in [
            (&old, old_pid, "RootExited"),
            (&new, new_pid, "OwnershipChanged"),
        ] {
            fs::create_dir_all(evidence).unwrap();
            for (name, value) in [
                (
                    "owner.json",
                    serde_json::json!({"pid":pid,"port":if pid == old_pid { port } else { new_port }}),
                ),
                (
                    "cleanup.json",
                    serde_json::json!({"confirmed":false,"error":error}),
                ),
            ] {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(evidence.join(name))
                    .unwrap();
                file.write_all(value.to_string().as_bytes()).unwrap();
            }
        }
        let intent =
            serde_json::json!({"version":1,"operation":"ollama","cleanup_confirmed":false})
                .to_string();
        let old_marker =
            serde_json::json!({"pid":old_pid,"error":"RootExited","evidence":old}).to_string();
        let new_marker =
            serde_json::json!({"pid":new_pid,"error":"OwnershipChanged","evidence":new})
                .to_string();
        for domain in ["stable", "legacy"] {
            for (name, bytes) in [
                (NATIVE_INFLIGHT, &intent),
                ("pair-cleanup-uncertain.json", &old_marker),
                (TEST_INFLIGHT, &intent),
                (TEST_QUARANTINE, &new_marker),
            ] {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(f.root.join(domain).join(name))
                    .unwrap();
                file.write_all(bytes.as_bytes()).unwrap();
            }
        }
        let marker = f.root.join("legacy").join(TEST_QUARANTINE);
        fs::write(&marker, b"foreign").unwrap();
        assert!(f.coord.clone().with_development_review().is_err());
        fs::write(&marker, new_marker.as_bytes()).unwrap();
        let stable_marker = f.root.join("stable").join(TEST_QUARANTINE);
        fs::write(
            &stable_marker,
            new_marker.replace("OwnershipChanged", "RootExited"),
        )
        .unwrap();
        assert!(f.coord.clone().with_development_review().is_err());
        fs::write(&stable_marker, new_marker.as_bytes()).unwrap();
        let occupied =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, new_port)).unwrap();
        assert!(f.coord.clone().with_development_review().is_err());
        drop(occupied);
        let reviewed = f.coord.clone().with_development_review().unwrap();
        assert!(f.coord.acquire().is_err());
        let occupied =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, new_port)).unwrap();
        assert!(reviewed.acquire().is_err());
        drop(occupied);
        let lease = reviewed.acquire().unwrap();
        assert!(lease.begin_native("image").is_err());
        lease.begin_native("ollama").unwrap().confirm().unwrap();
        for domain in ["stable", "legacy"] {
            let path = f.root.join(domain);
            assert!(!path.join(REVIEWED_INFLIGHT).exists());
            assert_eq!(
                fs::read_to_string(path.join(TEST_INFLIGHT)).unwrap(),
                intent
            );
            assert_eq!(
                fs::read_to_string(path.join(TEST_QUARANTINE)).unwrap(),
                new_marker
            );
            assert_eq!(
                fs::read_to_string(path.join(NATIVE_INFLIGHT)).unwrap(),
                intent
            );
            assert_eq!(
                fs::read_to_string(path.join("pair-cleanup-uncertain.json")).unwrap(),
                old_marker
            );
        }
        lease
            .quarantine(
                "pair-cleanup-uncertain.json",
                &serde_json::json!({"synthetic":true}),
            )
            .unwrap();
        drop(lease);
        assert!(reviewed.acquire().is_err());
        for domain in ["stable", "legacy"] {
            fs::remove_file(f.root.join(domain).join(REVIEWED_QUARANTINE)).unwrap();
            // fixture only
        }
        let path = new.join("cleanup.json");
        fs::write(&path, b"changed").unwrap();
        assert!(reviewed.acquire().is_err());
        assert!(f.coord.clone().with_development_review().is_err());
    }
    #[cfg(all(target_os = "macos", debug_assertions))]
    #[test]
    #[ignore = "explicit operator consent required: reads real coordination metadata; no model start"]
    fn real_review_only_observes_historical_markers_without_model_start() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap())
            .canonicalize()
            .unwrap();
        let stable = home.join("Library/Application Support/Seiya/coordination");
        let coord = Coordination::portable(&home, &stable).unwrap();
        assert!(coord.acquire().is_err());
        let reviewed = coord.with_development_review().unwrap();
        let lease = reviewed.acquire().unwrap();
        drop(lease);
    }
    #[test]
    fn shared_domain_excludes_old_new_and_releases_without_unlink() {
        let f = Fixture::new();
        let old = Coordination {
            stable: None,
            legacy: f.root.join("legacy"),
            reviewed: None,
        };
        let legacy = old.acquire().unwrap();
        assert!(f.coord.acquire().is_err());
        drop(legacy);
        let portable = f.coord.acquire().unwrap();
        assert!(old.acquire().is_err());
        assert!(f.coord.acquire().is_err());
        drop(portable);
        assert!(f.root.join("stable").join(LOCK).is_file());
        assert!(old.acquire().is_ok());
    }
    #[test]
    fn native_intent_survives_drop_and_blocks_both_domains_without_replay() {
        let f = Fixture::new();
        let lease = f.coord.acquire().unwrap();
        let intent = lease.begin_native("ollama").unwrap();
        for domain in ["stable", "legacy"] {
            let v: Value = serde_json::from_slice(
                &fs::read(f.root.join(domain).join(NATIVE_INFLIGHT)).unwrap(),
            )
            .unwrap();
            assert_eq!(v["cleanup_confirmed"], false);
        }
        drop(intent);
        drop(lease);
        assert!(
            matches!(f.coord.acquire(),Err(e) if e=="native_model_pending_operator_review_required")
        );
        assert!(Coordination {
            stable: None,
            legacy: f.root.join("legacy"),
            reviewed: None
        }
        .acquire()
        .is_err());
    }
    #[test]
    fn confirmed_cleanup_removes_only_owned_intent_and_allows_next_model() {
        let f = Fixture::new();
        let lease = f.coord.acquire().unwrap();
        assert!(lease.begin_native("unknown").is_err());
        lease.begin_native("image").unwrap().confirm().unwrap();
        drop(lease);
        for domain in ["stable", "legacy"] {
            assert!(!f.root.join(domain).join(NATIVE_INFLIGHT).exists());
        }
        assert!(f.coord.acquire().is_ok());
    }
    #[test]
    fn changed_or_partially_persisted_intent_never_unlinks_foreign_files() {
        let f = Fixture::new();
        let lease = f.coord.acquire().unwrap();
        let intent = lease.begin_native("ollama").unwrap();
        let path = f.root.join("stable").join(NATIVE_INFLIGHT);
        fs::rename(&path, f.root.join("original-intent")).unwrap();
        fs::write(&path, b"replacement").unwrap();
        assert!(intent.confirm().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"replacement");
        drop(lease);
        assert!(f.coord.acquire().is_err());
        let f = Fixture::new();
        let lease = f.coord.acquire().unwrap();
        let existing = f.root.join("legacy").join(NATIVE_INFLIGHT);
        fs::write(&existing, b"preexisting").unwrap();
        assert!(lease.begin_native("image").is_err());
        assert_eq!(fs::read(existing).unwrap(), b"preexisting");
        assert!(f.root.join("stable").join(NATIVE_INFLIGHT).exists());
    }
    #[test]
    fn either_domain_quarantine_blocks_and_is_not_archived() {
        for domain in ["legacy", "stable"] {
            let f = Fixture::new();
            let path = f.root.join(domain).join(MARKERS[0]);
            fs::write(&path, b"untrusted marker").unwrap();
            assert!(f.coord.acquire().is_err());
            assert_eq!(fs::read(path).unwrap(), b"untrusted marker");
        }
    }
    #[test]
    fn external_http_pending_blocks_native_and_is_never_deleted() {
        let f = Fixture::new();
        let path = f.root.join("stable/http-8080.inflight");
        fs::write(&path, b"synthetic pending request").unwrap();
        assert!(
            matches!(f.coord.acquire(),Err(e) if e=="external_http_pending_operator_review_required")
        );
        assert_eq!(fs::read(path).unwrap(), b"synthetic pending request");
    }
    #[test]
    fn uncertain_cleanup_fans_out_and_blocks_old_launcher() {
        let f = Fixture::new();
        let lease = f.coord.acquire().unwrap();
        let marker = serde_json::json!({"confirmed":false,"synthetic":true});
        lease.quarantine(MARKERS[1], &marker).unwrap();
        drop(lease);
        for domain in ["legacy", "stable"] {
            let v: Value =
                serde_json::from_slice(&fs::read(f.root.join(domain).join(MARKERS[1])).unwrap())
                    .unwrap();
            assert_eq!(v, marker);
        }
        assert!(f.coord.acquire().is_err());
        assert!(Coordination {
            stable: None,
            legacy: f.root.join("legacy"),
            reviewed: None
        }
        .acquire()
        .is_err());
    }
    #[test]
    fn persistence_failure_still_marks_other_domain_and_held_locks_exclude() {
        let f = Fixture::new();
        let lease = f.coord.acquire().unwrap();
        fs::write(f.root.join("legacy").join(MARKERS[0]), b"existing").unwrap();
        assert!(lease
            .quarantine(MARKERS[0], &serde_json::json!({"synthetic":true}))
            .is_err());
        assert!(f.root.join("stable").join(MARKERS[0]).is_file());
        assert!(f.coord.acquire().is_err());
        drop(lease);
    }
    #[test]
    fn missing_legacy_is_allowed_only_for_portable_and_never_recreated() {
        let f = Fixture::new();
        let legacy = f.root.join("not-provisioned");
        let coord = Coordination {
            legacy: legacy.clone(),
            stable: f.coord.stable.clone(),
            reviewed: None,
        };
        assert!(coord.acquire().is_ok());
        assert!(!legacy.exists());
        assert!(Coordination {
            legacy,
            stable: None,
            reviewed: None
        }
        .acquire()
        .is_err());
    }
    #[test]
    fn existing_legacy_without_lock_or_symlink_lock_fails_closed() {
        let f = Fixture::new();
        let path = f.root.join("legacy").join(LOCK);
        fs::remove_file(&path).unwrap();
        assert!(f.coord.acquire().is_err());
        assert!(!path.exists());
        symlink(f.root.join("stable").join(LOCK), &path).unwrap();
        assert!(f.coord.acquire().is_err());
    }
    #[test]
    fn authority_is_fixed_under_home_not_selected_by_config() {
        let f = Fixture::new();
        let expected = f
            .root
            .join("Library/Application Support/Seiya/coordination");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&expected)
            .unwrap();
        assert!(Coordination::portable(&f.root, &expected).is_ok());
        assert!(Coordination::portable(&f.root, &f.root.join("stable")).is_err());
    }
    #[test]
    fn profile_lock_is_distinct_and_new_roots_cannot_escape() {
        let f = Fixture::new();
        let a = profile_lock(&f.root).unwrap();
        assert!(profile_lock(&f.root).is_err());
        assert!(f.coord.acquire().is_ok());
        drop(a);
        assert!(profile_lock(&f.root).is_ok());
        assert!(require_new_in(&f.root.join("new-run"), &f.root).is_ok());
        for p in [
            f.root.join("stable"),
            f.root.join("../escape"),
            f.root.join("stable/run"),
        ] {
            assert!(require_new_in(&p, &f.root).is_err());
        }
    }
}
