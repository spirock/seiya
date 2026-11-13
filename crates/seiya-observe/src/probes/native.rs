use super::*;
use std::process::{Child, Stdio};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

pub struct NativeTable<'a> {
    witness: Witness<'a>,
}
enum Witness<'a> {
    Child(&'a mut Child),
    Current(u32),
}
pub struct CommandExecutor {
    deadline: Duration,
}

pub fn from_child(
    child: &mut Child,
    role: Role,
    deadline: Duration,
) -> Result<Adapter<NativeTable<'_>, CommandExecutor>, ProbeError> {
    if !cfg!(target_os = "macos") {
        return Err(ProbeError::Unsupported);
    }
    if !(Duration::from_millis(1)..=Duration::from_millis(2000)).contains(&deadline) {
        return Err(ProbeError::Limit);
    }
    let pid = child.id();
    let mut table = NativeTable {
        witness: Witness::Child(child),
    };
    if !table.root_alive()? {
        return Err(ProbeError::RootExited);
    }
    let root = table.metadata(pid)?.ok_or(ProbeError::RootExited)?;
    if root.parent_pid != std::process::id() {
        return Err(ProbeError::OwnershipChanged);
    }
    if !table.root_alive()? {
        return Err(ProbeError::RootExited);
    }
    Ok(Adapter::new(
        table,
        CommandExecutor { deadline },
        root,
        role,
    ))
}

/// Only this process, never an arbitrary supplied PID; records collector overhead.
pub fn for_current_collector(
    deadline: Duration,
) -> Result<Adapter<NativeTable<'static>, CommandExecutor>, ProbeError> {
    if !cfg!(target_os = "macos") {
        return Err(ProbeError::Unsupported);
    }
    if !(Duration::from_millis(1)..=Duration::from_millis(2000)).contains(&deadline) {
        return Err(ProbeError::Limit);
    }
    let pid = std::process::id();
    let mut table = NativeTable {
        witness: Witness::Current(pid),
    };
    let root = table.metadata(pid)?.ok_or(ProbeError::RootExited)?;
    Ok(Adapter::new(
        table,
        CommandExecutor { deadline },
        root,
        Role::Collector,
    ))
}

impl ProcessTable for NativeTable<'_> {
    fn root_alive(&mut self) -> Result<bool, ProbeError> {
        match &mut self.witness {
            Witness::Child(child) => child
                .try_wait()
                .map(|status| status.is_none())
                .map_err(|_| ProbeError::Failed),
            Witness::Current(pid) => Ok(*pid == std::process::id()),
        }
    }
    fn metadata(&mut self, pid: u32) -> Result<Option<Metadata>, ProbeError> {
        metadata(pid)
    }
    fn children(&mut self, pid: u32) -> Result<Vec<u32>, ProbeError> {
        children(pid)
    }
}

#[cfg(target_os = "macos")]
fn os_error() -> ProbeError {
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::EPERM | libc::EACCES) => ProbeError::PermissionDenied,
        _ => ProbeError::Failed,
    }
}

#[cfg(target_os = "macos")]
pub(super) fn metadata(pid: u32) -> Result<Option<Metadata>, ProbeError> {
    let pid = i32::try_from(pid)
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or(ProbeError::InvalidData)?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: SDK binding, exact-size writable buffer; read only on full success.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if read <= 0 {
        return if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            Ok(None)
        } else {
            Err(os_error())
        };
    }
    if read != size {
        return Err(ProbeError::InvalidData);
    }
    // SAFETY: proc_pidinfo initialized the entire SDK struct above.
    let info = unsafe { info.assume_init() };
    if info.pbi_pid != pid as u32 || info.pbi_start_tvsec == 0 || info.pbi_start_tvusec >= 1_000_000
    {
        return Err(ProbeError::InvalidData);
    }
    let identity = ProcessIdentity::new(
        info.pbi_pid,
        format!("{}-{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
    )
    .map_err(|_| ProbeError::InvalidData)?;
    Ok(Some(Metadata {
        identity,
        parent_pid: info.pbi_ppid,
    }))
}

#[cfg(target_os = "macos")]
pub(super) fn children(pid: u32) -> Result<Vec<u32>, ProbeError> {
    let pid = i32::try_from(pid)
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or(ProbeError::InvalidData)?;
    let mut buffer = [0i32; MAX_TARGETS + 1];
    let size = std::mem::size_of_val(&buffer) as i32;
    // SAFETY: clear this thread's errno to distinguish empty lists from failures.
    unsafe {
        *libc::__error() = 0;
    }
    // SAFETY: fixed writable PID array and byte length, querying one owned parent.
    let read = unsafe { libc::proc_listchildpids(pid, buffer.as_mut_ptr().cast(), size) };
    if read < 0
        || read == 0
            && std::io::Error::last_os_error()
                .raw_os_error()
                .is_some_and(|errno| errno != 0)
    {
        return Err(os_error());
    }
    // proc_listchildpids returns a PID count, unlike proc_listpids' byte count.
    child_ids(&buffer, read)
}

#[cfg(any(target_os = "macos", test))]
fn child_ids(buffer: &[i32], count: i32) -> Result<Vec<u32>, ProbeError> {
    let count = usize::try_from(count).map_err(|_| ProbeError::InvalidData)?;
    if count >= buffer.len() {
        return Err(ProbeError::Limit);
    }
    buffer[..count]
        .iter()
        .map(|pid| {
            u32::try_from(*pid)
                .ok()
                .filter(|pid| *pid > 0)
                .ok_or(ProbeError::InvalidData)
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub(super) fn metadata(_: u32) -> Result<Option<Metadata>, ProbeError> {
    Err(ProbeError::Unsupported)
}
#[cfg(not(target_os = "macos"))]
pub(super) fn children(_: u32) -> Result<Vec<u32>, ProbeError> {
    Err(ProbeError::Unsupported)
}

impl Executor for CommandExecutor {
    fn run(&mut self, request: Request) -> ProbeFuture<'_> {
        Box::pin(async move {
            if !cfg!(target_os = "macos") {
                return Err(ProbeError::Unsupported);
            }
            let mut command = command(request)?;
            let mut child = command.spawn().map_err(|error| match error.kind() {
                std::io::ErrorKind::PermissionDenied => ProbeError::PermissionDenied,
                _ => ProbeError::Failed,
            })?;
            let deadline = self.deadline;
            let work = async move {
                let stdout = child.stdout.take().ok_or(ProbeError::Failed)?;
                let bytes = read_output(stdout).await?;
                if !child
                    .wait()
                    .await
                    .map_err(|_| ProbeError::Failed)?
                    .success()
                {
                    return Err(ProbeError::Failed);
                }
                Ok(bytes)
            };
            timed(deadline, work).await
        })
    }
}

fn command(request: Request) -> Result<Command, ProbeError> {
    let mut command = match request {
        Request::Rss(pid) if pid > 0 && pid <= i32::MAX as u32 => {
            let mut command = Command::new("/bin/ps");
            command.args(["-p", &pid.to_string(), "-o", "pid=,rss="]);
            command
        }
        Request::Rss(_) => return Err(ProbeError::InvalidData),
        Request::VmStat => Command::new("/usr/bin/vm_stat"),
        Request::Swap => {
            let mut command = Command::new("/usr/sbin/sysctl");
            command.arg("vm.swapusage");
            command
        }
    };
    command
        .env_clear()
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    Ok(command)
}

async fn read_output(stdout: impl AsyncRead + Unpin) -> Result<Vec<u8>, ProbeError> {
    let mut bytes = Vec::new();
    stdout
        .take((macos::MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| ProbeError::Failed)?;
    if bytes.len() > macos::MAX_BYTES {
        return Err(ProbeError::Limit);
    }
    Ok(bytes)
}

async fn timed(
    work_deadline: Duration,
    work: impl Future<Output = Result<Vec<u8>, ProbeError>>,
) -> Result<Vec<u8>, ProbeError> {
    tokio::time::timeout(work_deadline, work)
        .await
        .map_err(|_| ProbeError::TimedOut)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[test]
    fn only_fixed_programs_flags_and_clean_locale_are_constructed() {
        for (request, program, args) in [
            (
                Request::Rss(42),
                "/bin/ps",
                vec!["-p", "42", "-o", "pid=,rss="],
            ),
            (Request::VmStat, "/usr/bin/vm_stat", vec![]),
            (Request::Swap, "/usr/sbin/sysctl", vec!["vm.swapusage"]),
        ] {
            let command = command(request).unwrap();
            let native = command.as_std();
            assert_eq!(native.get_program(), program);
            assert_eq!(native.get_args().collect::<Vec<_>>(), args);
            let env: Vec<_> = native.get_envs().collect();
            assert_eq!(env.len(), 2);
            assert!(env
                .iter()
                .all(|(key, value)| (*key == "LANG" || *key == "LC_ALL")
                    && *value == Some(std::ffi::OsStr::new("C"))));
        }
        assert!(command(Request::Rss(0)).is_err());
        assert!(command(Request::Rss(u32::MAX)).is_err());
    }

    #[test]
    fn native_child_list_result_is_pid_count_not_byte_count() {
        assert_eq!(child_ids(&[11, 12, 0], 2), Ok(vec![11, 12]));
        assert_eq!(child_ids(&[11, 12, 0], 0), Ok(vec![]));
        assert_eq!(child_ids(&[11, 12, 0], 3), Err(ProbeError::Limit));
        assert_eq!(child_ids(&[11, 12, 0], -1), Err(ProbeError::InvalidData));
        assert_eq!(child_ids(&[0, 0], 1), Err(ProbeError::InvalidData));
    }

    #[tokio::test]
    async fn stdout_at_cap_is_allowed_and_excess_stops_after_one_extra_byte() {
        let exact = vec![b'x'; macos::MAX_BYTES];
        assert_eq!(
            read_output(exact.as_slice()).await.unwrap().len(),
            macos::MAX_BYTES
        );
        let mut huge = Cursor::new(vec![b'x'; macos::MAX_BYTES * 4]);
        assert_eq!(read_output(&mut huge).await, Err(ProbeError::Limit));
        assert_eq!(huge.position(), (macos::MAX_BYTES + 1) as u64);
    }

    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_drops_pending_work_instead_of_detaching_it() {
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = Dropped(dropped.clone());
        let work = async move {
            let _guard = guard;
            std::future::pending().await
        };
        assert_eq!(
            timed(Duration::from_millis(10), work).await,
            Err(ProbeError::TimedOut)
        );
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_also_bounds_a_pipe_that_never_reaches_eof() {
        let (reader, _writer) = tokio::io::duplex(64);
        assert_eq!(
            timed(Duration::from_millis(10), read_output(reader)).await,
            Err(ProbeError::TimedOut)
        );
    }
}
