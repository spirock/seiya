//! Explicit filesystem/hash provenance, never imports MLX or starts a server.
#[cfg(target_os = "macos")]
use super::ProvenanceSource;
use super::{MlxRecipe, PreparationError, Provenance};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use {
    std::fs::File,
    std::io::{Read, Seek, SeekFrom},
    std::path::Path,
    std::process::Stdio,
    std::time::Duration,
    tokio::io::AsyncReadExt,
};

#[derive(Debug, Clone)]
pub struct PackageMetadata {
    pub mlx: PathBuf,
    pub mlx_lm: PathBuf,
    pub transformers: PathBuf,
}
#[derive(Debug, Serialize)]
pub struct NativeProvenance {
    pub checkpoint: Provenance,
    pub python_sha256: String,
    pub entrypoint_sha256: String,
    pub package_metadata_sha256: BTreeMap<String, String>,
}

/// Explicit opt-in host observations. No process enumeration or inference.
/// The caller's monotonic epoch must also be used for `assess(now_millis)`.
#[cfg(target_os = "macos")]
pub async fn observe_host(
    epoch: std::time::Instant,
    external_runtime_clearance: bool,
) -> Result<super::HostObservation, PreparationError> {
    let sampled_at_millis = u64::try_from(
        std::time::Instant::now()
            .checked_duration_since(epoch)
            .ok_or(PreparationError::InvalidRecipe)?
            .as_millis(),
    )
    .map_err(|_| PreparationError::Limit)?;
    let mut memory = fixed_command(Path::new("/usr/sbin/sysctl"));
    memory.args(["-n", "hw.memsize"]).stdin(Stdio::null());
    let mut pressure = fixed_command(Path::new("/usr/sbin/sysctl"));
    pressure
        .args(["-n", "kern.memorystatus_vm_pressure_level"])
        .stdin(Stdio::null());
    let mut vm = fixed_command(Path::new("/usr/bin/vm_stat"));
    vm.stdin(Stdio::null());
    let (memory, pressure, vm) = tokio::join!(
        capture(memory, Duration::from_secs(1)),
        capture(pressure, Duration::from_secs(1)),
        capture(vm, Duration::from_secs(1))
    );
    Ok(host_from_outputs(
        sampled_at_millis,
        external_runtime_clearance,
        memory,
        pressure,
        vm,
    ))
}

/// Stop-only load watchdog input. No memory allocation test or generated command.
#[cfg(target_os = "macos")]
pub async fn observe_pressure(
    epoch: std::time::Instant,
) -> Result<super::PressureSample, PreparationError> {
    let sampled_at_millis = u64::try_from(
        std::time::Instant::now()
            .checked_duration_since(epoch)
            .ok_or(PreparationError::InvalidRecipe)?
            .as_millis(),
    )
    .map_err(|_| PreparationError::Limit)?;
    let mut command = fixed_command(Path::new("/usr/sbin/sysctl"));
    command
        .args(["-n", "kern.memorystatus_vm_pressure_level"])
        .stdin(Stdio::null());
    let bytes = capture(command, Duration::from_secs(1)).await?;
    let pressure = match bytes.as_slice() {
        b"1\n" => super::Pressure::Normal,
        b"2\n" => super::Pressure::Warning,
        b"4\n" => super::Pressure::Critical,
        _ => super::Pressure::Unknown,
    };
    Ok(super::PressureSample {
        sampled_at_millis,
        pressure,
    })
}
#[cfg(not(target_os = "macos"))]
pub async fn observe_pressure(
    _: std::time::Instant,
) -> Result<super::PressureSample, PreparationError> {
    Err(PreparationError::Unsupported)
}
#[cfg(not(target_os = "macos"))]
pub async fn observe_host(
    _: std::time::Instant,
    _: bool,
) -> Result<super::HostObservation, PreparationError> {
    Err(PreparationError::Unsupported)
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn host_from_outputs(
    sampled_at_millis: u64,
    external_runtime_clearance: bool,
    memory: Result<Vec<u8>, PreparationError>,
    pressure: Result<Vec<u8>, PreparationError>,
    vm: Result<Vec<u8>, PreparationError>,
) -> super::HostObservation {
    fn number(bytes: Result<Vec<u8>, PreparationError>) -> Option<u64> {
        let bytes = bytes.ok()?;
        let value = std::str::from_utf8(&bytes).ok()?.strip_suffix('\n')?;
        if value.is_empty() || value.len() > 20 || !value.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        value.parse().ok()
    }
    let physical_bytes = number(memory).filter(|n| *n > 0);
    let pressure = match number(pressure) {
        Some(1) => super::Pressure::Normal,
        Some(2) => super::Pressure::Warning,
        Some(4) => super::Pressure::Critical,
        _ => super::Pressure::Unknown,
    };
    let free_bytes = vm.ok().and_then(|bytes| {
        let vm = crate::macos::parse_vm_stat(std::str::from_utf8(&bytes).ok()?).ok()?;
        vm.pages
            .get(&crate::macos::PageCounter::Free)?
            .checked_mul(vm.page_size_bytes)
    });
    super::HostObservation {
        sampled_at_millis,
        physical_bytes,
        free_bytes,
        pressure,
        external_runtime_clearance,
    }
}

#[cfg(target_os = "macos")]
struct Cached {
    files: BTreeMap<String, (u64, String, Vec<u8>)>,
    versions: BTreeMap<String, String>,
}
#[cfg(target_os = "macos")]
impl ProvenanceSource for Cached {
    fn files(&mut self) -> Result<Vec<(String, u64)>, PreparationError> {
        Ok(self
            .files
            .iter()
            .map(|(name, (size, _, _))| (name.clone(), *size))
            .collect())
    }
    fn sha256(&mut self, name: &str) -> Result<String, PreparationError> {
        Ok(self
            .files
            .get(name)
            .ok_or(PreparationError::InvalidProvenance)?
            .1
            .clone())
    }
    fn read_metadata(&mut self, name: &str, cap: usize) -> Result<Vec<u8>, PreparationError> {
        let bytes = &mut self
            .files
            .get_mut(name)
            .ok_or(PreparationError::InvalidProvenance)?
            .2;
        if bytes.len() > cap {
            return Err(PreparationError::Limit);
        }
        Ok(std::mem::take(bytes))
    }
    fn versions(&mut self) -> Result<BTreeMap<String, String>, PreparationError> {
        Ok(self.versions.clone())
    }
}

#[cfg(target_os = "macos")]
impl NativeProvenance {
    /// Opt-in potentially expensive weight hashing. Call before measurement and
    /// only for operator-approved paths. There is no implicit call from a runner.
    pub async fn inspect(
        recipe: &MlxRecipe,
        packages: &PackageMetadata,
    ) -> Result<Self, PreparationError> {
        tokio::time::timeout(
            Duration::from_secs(600),
            Self::inspect_inner(recipe, packages),
        )
        .await
        .map_err(|_| PreparationError::TimedOut)?
    }
    async fn inspect_inner(
        recipe: &MlxRecipe,
        packages: &PackageMetadata,
    ) -> Result<Self, PreparationError> {
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::OpenOptionsExt;
        recipe.validate()?;
        let root = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&recipe.checkpoint)
            .map_err(|_| PreparationError::Io)?;
        let root_id = (
            root.metadata().map_err(|_| PreparationError::Io)?.dev(),
            root.metadata().map_err(|_| PreparationError::Io)?.ino(),
        );
        let mut inventory = Vec::new();
        // macOS /dev/fd/N does not support readdir; duplicate the held directory
        // into fdopendir instead. All asset opens use the original fd + nofollow.
        for name in names(&root)? {
            if name.ends_with(".py") {
                return Err(PreparationError::InvalidProvenance);
            }
            if super::provenance::kind(&name).is_none() {
                continue;
            }
            if inventory.len() >= 256 {
                return Err(PreparationError::Limit);
            }
            let c_name = std::ffi::CString::new(name.as_bytes())
                .map_err(|_| PreparationError::InvalidProvenance)?;
            // SAFETY: live directory fd, NUL-terminated flat validated basename.
            let fd = unsafe {
                libc::openat(
                    root.as_raw_fd(),
                    c_name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(PreparationError::Io);
            }
            // SAFETY: newly opened fd exclusively transferred to File.
            let file = unsafe { File::from_raw_fd(fd) };
            if !file.metadata().map_err(|_| PreparationError::Io)?.is_file() {
                return Err(PreparationError::InvalidProvenance);
            }
            inventory.push((name, file));
        }
        if inventory.len() > 256 {
            return Err(PreparationError::Limit);
        }
        let mut total = 0u64;
        let mut metadata_total = 0u64;
        // All size limits before any weight hashing.
        for (name, file) in &inventory {
            let size = file.metadata().map_err(|_| PreparationError::Io)?.len();
            total = total
                .checked_add(size)
                .filter(|v| *v <= 64 * 1024 * 1024 * 1024)
                .ok_or(PreparationError::Limit)?;
            if size == 0
                || !name.ends_with(".safetensors")
                    && size > super::provenance::metadata_cap(name) as u64
            {
                return Err(PreparationError::Limit);
            }
            if !name.ends_with(".safetensors") {
                metadata_total = metadata_total
                    .checked_add(size)
                    .filter(|v| *v <= super::provenance::MAX_METADATA_TOTAL_BYTES)
                    .ok_or(PreparationError::Limit)?;
            }
        }
        let mut cached = Cached {
            files: BTreeMap::new(),
            versions: BTreeMap::new(),
        };
        for (name, mut file) in inventory {
            let before = stamp(&file)?;
            let sha256 = hash(&file).await?;
            file.seek(SeekFrom::Start(0))
                .map_err(|_| PreparationError::Io)?;
            let mut bytes = Vec::new();
            if !name.ends_with(".safetensors") {
                let cap = super::provenance::metadata_cap(&name);
                Read::by_ref(&mut file)
                    .take(cap as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| PreparationError::Io)?;
                if bytes.len() > cap {
                    return Err(PreparationError::Limit);
                }
            }
            if stamp(&file)? != before {
                return Err(PreparationError::InvalidProvenance);
            }
            cached.files.insert(name, (before.bytes, sha256, bytes));
        }
        let python = File::open(&recipe.python).map_err(|_| PreparationError::Io)?;
        let entrypoint =
            File::open(&recipe.runtime_entrypoint).map_err(|_| PreparationError::Io)?;
        let python_sha256 = hash(&python).await?;
        let entrypoint_sha256 = hash(&entrypoint).await?;
        let mut command = fixed_command(&recipe.python);
        command.arg("--version").stdin(Stdio::null());
        let bytes = capture(command, Duration::from_secs(5)).await?;
        let version = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|s| s.trim().strip_prefix("Python "))
            .ok_or(PreparationError::InvalidProvenance)?;
        cached.versions.insert("python".into(), version.into());
        let mut package_metadata_sha256 = BTreeMap::new();
        for (key, path) in [
            ("mlx", &packages.mlx),
            ("mlx-lm", &packages.mlx_lm),
            ("transformers", &packages.transformers),
        ] {
            if !super::absolute(path) {
                return Err(PreparationError::InvalidRecipe);
            }
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)
                .map_err(|_| PreparationError::Io)?;
            let before = stamp(&file)?;
            if before.bytes > 1024 * 1024 {
                return Err(PreparationError::Limit);
            }
            let digest = hash(&file).await?;
            file.seek(SeekFrom::Start(0))
                .map_err(|_| PreparationError::Io)?;
            let mut bytes = Vec::new();
            Read::by_ref(&mut file)
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| PreparationError::Io)?;
            if bytes.len() > 1024 * 1024 || stamp(&file)? != before {
                return Err(PreparationError::InvalidProvenance);
            }
            cached
                .versions
                .insert(key.into(), package_version(&bytes, key)?);
            package_metadata_sha256.insert(key.into(), digest);
        }
        let current =
            std::fs::symlink_metadata(&recipe.checkpoint).map_err(|_| PreparationError::Io)?;
        if current.file_type().is_symlink() || (current.dev(), current.ino()) != root_id {
            return Err(PreparationError::InvalidProvenance);
        }
        let mut checkpoint = Provenance::collect(&mut cached)?;
        checkpoint.verification = "native-held-files-sha256-before-measurement";
        Ok(Self {
            checkpoint,
            python_sha256,
            entrypoint_sha256,
            package_metadata_sha256,
        })
    }
}

#[cfg(not(target_os = "macos"))]
impl NativeProvenance {
    pub async fn inspect(_: &MlxRecipe, _: &PackageMetadata) -> Result<Self, PreparationError> {
        Err(PreparationError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
fn names(root: &File) -> Result<Vec<String>, PreparationError> {
    use std::os::fd::AsRawFd;
    struct Directory(*mut libc::DIR);
    impl Drop for Directory {
        fn drop(&mut self) {
            // SAFETY: owned fdopendir stream, closed exactly once.
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    // SAFETY: live fd, duplicate with close-on-exec before any child starts.
    let fd = unsafe { libc::fcntl(root.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
    if fd < 0 {
        return Err(PreparationError::Io);
    }
    // SAFETY: transfer newly owned directory fd to libc on success.
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        // SAFETY: fdopendir failed; fd still belongs to this function.
        unsafe {
            libc::close(fd);
        }
        return Err(PreparationError::Io);
    }
    let stream = Directory(stream);
    let mut names = Vec::new();
    loop {
        // SAFETY: thread-local errno pointer, owned stream used synchronously.
        unsafe {
            *libc::__error() = 0;
        }
        // SAFETY: stream remains live; entry is copied before the next call.
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            // SAFETY: thread-local errno immediately after readdir.
            if unsafe { *libc::__error() } != 0 {
                return Err(PreparationError::Io);
            }
            break;
        }
        // SAFETY: readdir guarantees a NUL-terminated name until the next call.
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }
            .to_str()
            .map_err(|_| PreparationError::InvalidProvenance)?;
        if matches!(name, "." | "..") {
            continue;
        }
        if names.len() >= 512 {
            return Err(PreparationError::Limit);
        }
        names.push(name.to_owned());
    }
    Ok(names)
}

#[cfg(target_os = "macos")]
#[derive(PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    bytes: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

#[cfg(target_os = "macos")]
fn stamp(file: &File) -> Result<Stamp, PreparationError> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata().map_err(|_| PreparationError::Io)?;
    if !meta.is_file() {
        return Err(PreparationError::InvalidProvenance);
    }
    Ok(Stamp {
        device: meta.dev(),
        inode: meta.ino(),
        bytes: meta.len(),
        modified: (meta.mtime(), meta.mtime_nsec()),
        changed: (meta.ctime(), meta.ctime_nsec()),
    })
}

#[cfg(target_os = "macos")]
fn fixed_command(program: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    command
        .env_clear()
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONSAFEPATH", "1")
        .arg0(program)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}
#[cfg(target_os = "macos")]
async fn capture(
    mut command: tokio::process::Command,
    timeout: Duration,
) -> Result<Vec<u8>, PreparationError> {
    let mut child = command.spawn().map_err(|_| PreparationError::Io)?;
    let result = tokio::time::timeout(timeout, async {
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .ok_or(PreparationError::Io)?
            .take(8193)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| PreparationError::Io)?;
        if bytes.len() > 8192 {
            return Err(PreparationError::Limit);
        }
        if !child
            .wait()
            .await
            .map_err(|_| PreparationError::Io)?
            .success()
        {
            return Err(PreparationError::Io);
        }
        Ok(bytes)
    })
    .await;
    match result {
        Ok(Ok(bytes)) => Ok(bytes),
        result => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            result.unwrap_or(Err(PreparationError::TimedOut))
        }
    }
}
#[cfg(target_os = "macos")]
async fn hash(file: &File) -> Result<String, PreparationError> {
    let before = stamp(file)?;
    let mut command = fixed_command(Path::new("/usr/bin/shasum"));
    command.args(["-a", "256"]).stdin(Stdio::from(
        file.try_clone().map_err(|_| PreparationError::Io)?,
    ));
    let bytes = capture(command, Duration::from_secs(120)).await?;
    if stamp(file)? != before {
        return Err(PreparationError::InvalidProvenance);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| PreparationError::InvalidProvenance)?;
    let digest = text
        .strip_suffix("  -\n")
        .filter(|s| super::provenance::digest_valid(s))
        .ok_or(PreparationError::InvalidProvenance)?;
    Ok(digest.into())
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn package_version(bytes: &[u8], expected: &str) -> Result<String, PreparationError> {
    if bytes.len() > 1024 * 1024 {
        return Err(PreparationError::Limit);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| PreparationError::InvalidProvenance)?;
    let mut name = None;
    let mut version = None;
    for line in text.lines().take_while(|line| !line.is_empty()) {
        if line.len() > 8192 {
            return Err(PreparationError::Limit);
        }
        if let Some(value) = line.strip_prefix("Name: ") {
            if name
                .replace(value.replace('_', "-").to_ascii_lowercase())
                .is_some()
            {
                return Err(PreparationError::InvalidProvenance);
            }
        }
        if let Some(value) = line.strip_prefix("Version: ") {
            if version.replace(value.to_owned()).is_some() {
                return Err(PreparationError::InvalidProvenance);
            }
        }
    }
    if name.as_deref() != Some(expected) {
        return Err(PreparationError::InvalidProvenance);
    }
    version
        .filter(|v| !v.is_empty() && v.len() <= 128 && v.bytes().all(|b| b.is_ascii_graphic()))
        .ok_or(PreparationError::InvalidProvenance)
}
