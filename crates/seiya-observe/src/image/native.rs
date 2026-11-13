//! macOS-only operational adapters. No automatic load on construction/import.
use serde_json::{json, Value};
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::Path,
};

pub const BASE: &str = super::coordination::LEGACY_BASE;
pub fn lease() -> Result<File, String> {
    for name in [
        "image-cleanup-uncertain.json",
        "pair-cleanup-uncertain.json",
    ] {
        if Path::new(BASE)
            .join("seiya-host-model-coordination")
            .join(name)
            .symlink_metadata()
            .is_ok()
        {
            return Err("model_cleanup_quarantined_operator_review_required".into());
        }
    }
    quarantine_review_lease()
}
/// Exclusive review only, not model admission. Dedicated operator recovery must
/// verify/archive quarantine; all model launchers still call lease(), not this.
pub fn quarantine_review_lease() -> Result<File, String> {
    let path = Path::new(BASE).join("seiya-host-model-coordination/seiya-mlx-baseline.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| "shared_lease_open")?;
    if !file
        .metadata()
        .map_err(|_| "shared_lease_metadata")?
        .is_file()
    {
        return Err("invalid_lease_file".into());
    }
    // SAFETY: valid owned fd, nonblocking cooperative lock, never unlinked.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("shared_lease_busy".into());
    }
    Ok(file)
}
pub fn disk_free(path: &Path) -> Result<u64, String> {
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| "disk_path")?;
    let mut info = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: SDK exact-size writable struct and a NUL-terminated local path.
    if unsafe { libc::statfs(path.as_ptr(), info.as_mut_ptr()) } != 0 {
        return Err("disk_observation".into());
    }
    // SAFETY: successful syscall initialized the entire SDK struct.
    let info = unsafe { info.assume_init() };
    info.f_bavail
        .checked_mul(u64::from(info.f_bsize))
        .ok_or("disk_overflow".into())
}
fn sysctl<T>(name: &std::ffi::CStr) -> Option<T> {
    let mut value = std::mem::MaybeUninit::<T>::zeroed();
    let mut size = std::mem::size_of::<T>();
    // SAFETY: private helper used only for SDK integer/xsw_usage POD types,
    // exact writable length, read-only sysctl (no new value pointer).
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            value.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || size != std::mem::size_of::<T>()
    {
        return None;
    }
    // SAFETY: exact-size successful kernel result for the POD callers below.
    Some(unsafe { value.assume_init() })
}
pub fn memory() -> Value {
    let pressure: Option<u32> = sysctl(c"kern.memorystatus_vm_pressure_level");
    let swap: Option<libc::xsw_usage> = sysctl(c"vm.swapusage");
    json!({"scope":"whole-host","pressure":pressure,"swap_bytes":swap.map(|s|s.xsu_used),"memory_stop_enabled":false})
}
pub fn child_profile(assets: &Path) -> Result<String, String> {
    let s = assets.to_str().ok_or("profile_path")?;
    Ok(format!(
        "(version 1)(allow default)(deny file-write* (subpath {}))(deny network*)",
        serde_json::to_string(s).map_err(|_| "profile_path")?
    ))
}
pub fn require_private_new(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path.exists()
        || path.parent().and_then(|p| p.canonicalize().ok()) != Path::new(BASE).canonicalize().ok()
    {
        return Err("new_private_directory_required".into());
    }
    Ok(())
}
