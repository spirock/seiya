//! Cooperative local storage/locks. Never repairs permissions or old journals.
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(crate) fn default_root() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let base = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("local_appdata_required")?)
        .canonicalize()
        .map_err(|_| "local_appdata_required")?;
    #[cfg(not(target_os = "windows"))]
    let base = PathBuf::from(std::env::var_os("HOME").ok_or("home_required")?)
        .canonicalize()
        .map_err(|_| "home_required")?;
    #[cfg(target_os = "macos")]
    let root = base.join("Library/Application Support/Seiya");
    #[cfg(target_os = "windows")]
    let root = base.join("Seiya");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let root = base.join(".local/state/Seiya");
    Ok(root)
}
pub(crate) fn ensure_directory(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("absolute_storage_required".into());
    }
    if !path.exists() {
        let parent = path.parent().ok_or("storage_parent_required")?;
        if !parent.exists() {
            ensure_directory(parent)?;
        }
        if parent.canonicalize().ok().as_deref() != Some(parent) {
            return Err("canonical_storage_required".into());
        }
        #[allow(unused_mut)]
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(path)
            .map_err(|_| "storage_directory_create")?;
    }
    let m = fs::symlink_metadata(path).map_err(|_| "storage_directory_metadata")?;
    if !m.is_dir()
        || m.file_type().is_symlink()
        || path.canonicalize().ok().as_deref() != Some(path)
    {
        return Err("canonical_storage_required".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if m.permissions().mode() & 0o077 != 0 {
            return Err("private_storage_required".into());
        }
    }
    Ok(())
}
fn options() -> OpenOptions {
    #[allow(unused_mut)]
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000 /* FILE_FLAG_OPEN_REPARSE_POINT */);
    }
    options
}
fn regular(file: &File, path: &Path) -> Result<(), String> {
    let m = file.metadata().map_err(|_| "storage_metadata")?;
    if !m.is_file()
        || fs::symlink_metadata(path)
            .map_err(|_| "storage_metadata")?
            .file_type()
            .is_symlink()
        || path.canonicalize().ok().as_deref() != Some(path)
    {
        return Err("regular_storage_file_required".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if m.permissions().mode() & 0o077 != 0 || m.nlink() != 1 {
            return Err("private_storage_file_required".into());
        }
        let current = fs::symlink_metadata(path).map_err(|_| "storage_metadata")?;
        if current.dev() != m.dev() || current.ino() != m.ino() {
            return Err("storage_file_changed".into());
        }
    }
    Ok(())
}
pub(crate) fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("storage_unreadable".into()),
        Ok(_) => {}
    }
    let mut file = options()
        .read(true)
        .open(path)
        .map_err(|_| "storage_open")?;
    regular(&file, path)?;
    if file.metadata().map_err(|_| "storage_metadata")?.len() > 65536 {
        return Err("storage_too_large".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "storage_read")?;
    if bytes.len() > 65536 {
        return Err("storage_too_large".into());
    }
    Ok(Some(bytes))
}
pub(crate) fn lock(path: &Path) -> Result<File, String> {
    let file = options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|_| "runtime_lease_open")?;
    regular(&file, path)?;
    file.try_lock().map_err(|_| "runtime_lease_busy")?;
    regular(&file, path)?;
    Ok(file)
}
pub(crate) fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|_| "storage_sync_uncertain")?;
    }
    // Windows FlushFileBuffers applies to the file; directory fsync has no
    // equivalent in this std adapter. Do not claim crash-atomic directory durability.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
pub(crate) fn new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "storage_create_new")?;
    regular(&file, path)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "storage_write_uncertain")?;
    sync_directory(path.parent().ok_or("storage_parent_required")?)
}
pub(crate) fn replace(path: &Path, expected: &Option<Vec<u8>>, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > 65536 {
        return Err("storage_too_large".into());
    }
    if &read(path)? != expected {
        return Err("profile_changed_no_overwrite".into());
    }
    let parent = path.parent().ok_or("storage_parent_required")?;
    let temp = parent.join(format!("models-{}.tmp", uuid::Uuid::new_v4()));
    new_file(&temp, bytes)?;
    if &read(path)? != expected {
        let _ = fs::remove_file(&temp);
        return Err("profile_changed_no_overwrite".into());
    }
    // std::fs::rename maps to replacement semantics on both Windows and Unix.
    if fs::rename(&temp, path).is_err() {
        let _ = fs::remove_file(&temp);
        return Err("storage_replace_failed".into());
    }
    sync_directory(parent)
}
pub(crate) fn revision(parts: &[Option<Vec<u8>>]) -> Vec<u8> {
    Sha256::digest(serde_json::to_vec(parts).expect("revision serialization")).to_vec()
}

pub(crate) struct Journal {
    path: PathBuf,
    token: Vec<u8>,
}
impl Journal {
    pub fn begin(root: &Path, port: u16) -> Result<Self, String> {
        Self::named(root, &format!("http-{port}.inflight"))
    }
    pub fn begin_web(root: &Path) -> Result<Self, String> {
        Self::named(root, "web-reader.inflight")
    }
    fn named(root: &Path, name: &str) -> Result<Self, String> {
        let path = root.join(name);
        let token = uuid::Uuid::new_v4().to_string().into_bytes();
        new_file(&path, &token)?;
        Ok(Self { path, token })
    }
    pub fn confirm(self) -> Result<(), String> {
        if read(&self.path)?.as_deref() != Some(&self.token) {
            return Err("runtime_journal_changed".into());
        }
        fs::remove_file(&self.path).map_err(|_| "runtime_journal_close_uncertain")?;
        sync_directory(self.path.parent().ok_or("journal_parent_required")?)
    }
    // Drop deliberately leaves a pending marker. Never infer remote cleanup.
}
