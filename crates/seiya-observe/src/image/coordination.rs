//! Cooperative model ownership. Never unlink a lock or recover quarantine here.
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

pub const LEGACY_BASE: &str = "/private/var/folders/tc/6nkb5t5d5116q143rzc2dy400000gn/T/opencode";
const LOCK: &str = "seiya-mlx-baseline.lock";
const MARKERS: [&str; 2] = [
    "image-cleanup-uncertain.json",
    "pair-cleanup-uncertain.json",
];

#[derive(Clone)]
pub struct Coordination {
    stable: Option<PathBuf>,
    legacy: PathBuf,
}

impl Coordination {
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn isolated_for_test(directory: PathBuf) -> Self {
        Self {
            legacy: directory.join("absent-legacy"),
            stable: Some(directory),
        }
    }
    pub fn legacy() -> Self {
        Self {
            stable: None,
            legacy: Path::new(LEGACY_BASE).join("seiya-host-model-coordination"),
        }
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
            for marker in MARKERS {
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
}

impl ModelLease {
    /// Caller must retain/leak the lease if this cannot durably mark every domain.
    pub fn quarantine(&self, name: &str, marker: &Value) -> Result<(), String> {
        if !MARKERS.contains(&name) {
            return Err("invalid_quarantine_marker".into());
        }
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
    #[test]
    fn shared_domain_excludes_old_new_and_releases_without_unlink() {
        let f = Fixture::new();
        let old = Coordination {
            stable: None,
            legacy: f.root.join("legacy"),
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
            legacy: f.root.join("legacy")
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
        };
        assert!(coord.acquire().is_ok());
        assert!(!legacy.exists());
        assert!(Coordination {
            legacy,
            stable: None
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
