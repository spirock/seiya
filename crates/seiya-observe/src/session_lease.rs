//! Native coordination lease only. No process owner, load approval or probe.
use crate::campaign::Reservation;
use crate::session_supervisor::{LeaseOrigin, OwnedFuture, OwnedLease, OwnerError};

pub struct NativeSessionLease {
    reservation: Option<Reservation>,
    #[cfg(target_os = "macos")]
    binding: Option<Binding>,
    released: bool,
}
impl NativeSessionLease {
    pub(crate) fn new(reservation: Reservation) -> Self {
        #[cfg(target_os = "macos")]
        let binding = Binding::capture(&reservation).ok();
        Self {
            reservation: Some(reservation),
            #[cfg(target_os = "macos")]
            binding,
            released: false,
        }
    }
    /// Trusted host check before any load. Never a permission or memory estimate.
    pub fn verify_binding(&self) -> Result<(), OwnerError> {
        if self.released {
            return Err(OwnerError::Failed);
        }
        #[cfg(target_os = "macos")]
        {
            let reservation = self.reservation.as_ref().ok_or(OwnerError::Failed)?;
            self.binding
                .as_ref()
                .ok_or(OwnerError::IdentityChanged)?
                .verify(reservation)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = self.reservation.as_ref();
            Err(OwnerError::Unsupported)
        }
    }
    fn release_now(&mut self) -> Result<(), OwnerError> {
        if self.released {
            return Ok(());
        }
        self.verify_binding()?;
        #[cfg(target_os = "macos")]
        {
            use std::os::fd::AsRawFd;
            let (file, _) = self
                .reservation
                .as_ref()
                .ok_or(OwnerError::Failed)?
                .session_lock();
            // SAFETY: original owned descriptor retained by Reservation. Only
            // unlock, no supplied FD/PID and never unlink the persistent lockfile.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) } != 0 {
                return Err(OwnerError::Failed);
            }
            // No await/fallible work after the irreversible kernel operation.
            self.released = true;
            self.reservation.take();
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(OwnerError::Unsupported)
        }
    }
}
impl OwnedLease for NativeSessionLease {
    fn origin(&self) -> LeaseOrigin {
        LeaseOrigin::NativeCoordinationLock
    }
    fn release(&mut self) -> OwnedFuture<'_, ()> {
        Box::pin(async move { self.release_now() })
    }
}

#[cfg(target_os = "macos")]
struct Binding {
    directory: (u64, u64),
    file: (u64, u64),
}
#[cfg(target_os = "macos")]
impl Binding {
    fn capture(reservation: &Reservation) -> Result<Self, OwnerError> {
        use std::os::unix::fs::MetadataExt;
        let (file, directory) = reservation.session_lock();
        let dir = std::fs::symlink_metadata(directory).map_err(|_| OwnerError::Failed)?;
        let meta = file.metadata().map_err(|_| OwnerError::Failed)?;
        let binding = Self {
            directory: (dir.dev(), dir.ino()),
            file: (meta.dev(), meta.ino()),
        };
        binding.verify(reservation)?;
        Ok(binding)
    }
    fn verify(&self, reservation: &Reservation) -> Result<(), OwnerError> {
        use std::os::unix::fs::MetadataExt;
        let (file, directory) = reservation.session_lock();
        let dir = std::fs::symlink_metadata(directory).map_err(|_| OwnerError::IdentityChanged)?;
        let fd = file.metadata().map_err(|_| OwnerError::Failed)?;
        let path = std::fs::symlink_metadata(directory.join("seiya-mlx-baseline.lock"))
            .map_err(|_| OwnerError::IdentityChanged)?;
        // SAFETY: fixed OS query, no arguments or process inspection.
        let uid = unsafe { libc::geteuid() };
        if !dir.is_dir()
            || dir.file_type().is_symlink()
            || (dir.dev(), dir.ino()) != self.directory
            || !fd.is_file()
            || !path.is_file()
            || path.file_type().is_symlink()
            || (fd.dev(), fd.ino()) != self.file
            || (path.dev(), path.ino()) != self.file
        {
            return Err(OwnerError::IdentityChanged);
        }
        if dir.uid() != uid
            || fd.uid() != uid
            || path.uid() != uid
            || dir.mode() & 0o022 != 0
            || fd.mode() & 0o077 != 0
            || path.mode() & 0o077 != 0
            || fd.nlink() != 1
            || path.nlink() != 1
        {
            return Err(OwnerError::Failed);
        }
        Ok(())
    }
}
