//! Narrow process-creation restriction, not a data/network/XPC security sandbox.
pub const PROFILE: &str = "(version 1)(allow default)(deny process-fork)";
pub const READY: &[u8] = b"SEIYA_PROCESS_CREATION_DENIED_V1\n";

/// Only used by the single-threaded bootstrap/explicit fixture, before runtime.
/// If unexpectedly allowed, immediately exit/reap test child and reject launch.
#[cfg(target_os = "macos")]
pub fn fork_and_spawn_denied() -> bool {
    let fork_denied;
    // SAFETY: fork probe's child calls only async-signal-safe _exit, never Rust
    // allocators/destructors or inherited IO. Parent waits for that exact child.
    unsafe {
        let pid = libc::fork();
        fork_denied = pid < 0 && *libc::__error() == libc::EPERM;
        if pid == 0 {
            libc::_exit(0);
        }
        if pid > 0 {
            while libc::waitpid(pid, std::ptr::null_mut(), 0) < 0 {
                if *libc::__error() != libc::EINTR {
                    return false;
                }
            }
        }
    }
    let mut pid = 0;
    let argv = [c"true".as_ptr().cast_mut(), std::ptr::null_mut()];
    let env = [std::ptr::null_mut()];
    // SAFETY: fixed program and NUL-terminated argv/env arrays, writable pid;
    // no custom file actions/attributes or model/user-controlled shell text.
    let result = unsafe {
        libc::posix_spawn(
            &mut pid,
            c"/usr/bin/true".as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            argv.as_ptr(),
            env.as_ptr(),
        )
    };
    if result == 0 {
        // SAFETY: exact original probe child, never a supplied PID.
        unsafe {
            while libc::waitpid(pid, std::ptr::null_mut(), 0) < 0 {
                if *libc::__error() != libc::EINTR {
                    return false;
                }
            }
        }
    }
    fork_denied && result == libc::EPERM
}
