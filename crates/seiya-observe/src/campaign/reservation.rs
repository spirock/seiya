use super::{MlxRecipe, PreparationError};
#[cfg(target_os = "macos")]
use std::fs::{File, OpenOptions};
#[cfg(target_os = "macos")]
use std::net::Ipv4Addr;
use std::net::TcpListener;
use std::path::PathBuf;

/// Cooperative host lease plus two held loopback sockets. No child launch.
/// The lock file is never unlinked, including on errors or Drop.
pub struct Reservation {
    #[cfg(target_os = "macos")]
    _lease: File,
    directory: PathBuf,
    runtime: Option<TcpListener>,
    coordinator: Option<TcpListener>,
}
impl Reservation {
    /// Move the original reservation to the session supervisor's native lease.
    /// Does not grant load approval, release sockets or unlock the file.
    pub fn into_session_lease(self) -> crate::session_lease::NativeSessionLease {
        crate::session_lease::NativeSessionLease::new(self)
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn session_lock(&self) -> (&File, &std::path::Path) {
        (&self._lease, &self.directory)
    }

    #[cfg(target_os = "macos")]
    pub fn acquire(recipe: &MlxRecipe) -> Result<Self, PreparationError> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        recipe.validate()?;
        // Directory supplied by the operator; the shared location must be the
        // same for every participant. Never choose a new per-run lock directory.
        let directory = recipe
            .coordination_directory
            .canonicalize()
            .map_err(|_| PreparationError::Io)?;
        if !directory.is_dir() {
            return Err(PreparationError::Io);
        }
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory.join("seiya-mlx-baseline.lock"))
            .map_err(|_| PreparationError::Io)?;
        if !lease
            .metadata()
            .map_err(|_| PreparationError::Io)?
            .is_file()
        {
            return Err(PreparationError::Io);
        }
        // SAFETY: valid owned fd, fixed nonblocking advisory lock operation.
        if unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(
                if std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) {
                    PreparationError::Busy
                } else {
                    PreparationError::Io
                },
            );
        }
        let runtime = ephemeral()?;
        let coordinator = ephemeral()?;
        Ok(Self {
            _lease: lease,
            directory,
            runtime: Some(runtime),
            coordinator: Some(coordinator),
        })
    }
    #[cfg(not(target_os = "macos"))]
    pub fn acquire(_: &MlxRecipe) -> Result<Self, PreparationError> {
        Err(PreparationError::Unsupported)
    }

    pub fn ports(&self) -> Result<(u16, u16), PreparationError> {
        Ok((
            self.runtime
                .as_ref()
                .ok_or(PreparationError::Busy)?
                .local_addr()
                .map_err(|_| PreparationError::Io)?
                .port(),
            self.coordinator
                .as_ref()
                .ok_or(PreparationError::Busy)?
                .local_addr()
                .map_err(|_| PreparationError::Io)?
                .port(),
        ))
    }
    /// Refresh held sockets between confirmed runs; never drops the host lease.
    #[cfg(target_os = "macos")]
    pub(crate) fn rearm(&mut self) -> Result<(), PreparationError> {
        if self.runtime.is_none() && self.coordinator.is_none() {
            let runtime = ephemeral()?;
            let coordinator = ephemeral()?;
            self.runtime = Some(runtime);
            self.coordinator = Some(coordinator);
        }
        self.ports().map(|_| ())
    }
    #[cfg(not(target_os = "macos"))]
    pub(crate) fn rearm(&mut self) -> Result<(), PreparationError> {
        Err(PreparationError::Unsupported)
    }
    pub(crate) fn release_runtime(&mut self) -> Result<u16, PreparationError> {
        let listener = self.runtime.take().ok_or(PreparationError::Busy)?;
        listener
            .local_addr()
            .map(|a| a.port())
            .map_err(|_| PreparationError::Io)
    }
    pub(crate) fn release_coordinator(&mut self) -> Result<u16, PreparationError> {
        let listener = self.coordinator.take().ok_or(PreparationError::Busy)?;
        listener
            .local_addr()
            .map(|a| a.port())
            .map_err(|_| PreparationError::Io)
    }
    pub(super) fn matches(&self, recipe: &MlxRecipe) -> bool {
        recipe
            .coordination_directory
            .canonicalize()
            .is_ok_and(|dir| dir == self.directory)
            && self.ports().is_ok_and(|(a, b)| a != b)
    }
}

#[cfg(target_os = "macos")]
fn ephemeral() -> Result<TcpListener, PreparationError> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|_| PreparationError::Io)?;
    if listener
        .local_addr()
        .map_err(|_| PreparationError::Io)?
        .port()
        < 49152
    {
        return Err(PreparationError::Io);
    }
    Ok(listener)
}
