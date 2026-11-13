//! Manual owned-child lifecycle; never accepts a caller-supplied PID.
use super::{native, Metadata, ProbeError};
use crate::MAX_TARGETS;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    os::unix::process::CommandExt,
    process::{Child, Command, ExitStatus},
    time::{Duration, Instant},
};

#[derive(Debug, Serialize)]
pub struct Cleanup {
    pub tree_stopped: bool,
    pub group_empty: bool,
    pub processes_observed: usize,
    pub exit_code: Option<i32>,
}

pub struct OwnedChild {
    child: Child,
    root: Metadata,
    known: BTreeMap<u32, Metadata>,
    status: Option<ExitStatus>,
    closed: bool,
}

fn group_members(pid: u32) -> Result<BTreeSet<u32>, ProbeError> {
    let mut pids = [0i32; MAX_TARGETS + 1];
    // SAFETY: distinguish a genuinely empty group from a denied kernel query.
    unsafe {
        *libc::__error() = 0;
    }
    // SAFETY: fixed writable PID buffer, querying this original child's group.
    let count = unsafe {
        libc::proc_listpgrppids(
            pid as i32,
            pids.as_mut_ptr().cast(),
            std::mem::size_of_val(&pids) as i32,
        )
    };
    if count < 0 || count as usize >= pids.len() {
        return Err(ProbeError::Limit);
    }
    if count == 0
        && std::io::Error::last_os_error()
            .raw_os_error()
            .is_some_and(|e| e != 0 && e != libc::ESRCH)
    {
        return Err(ProbeError::Failed);
    }
    pids[..count as usize]
        .iter()
        .map(|p| {
            u32::try_from(*p)
                .ok()
                .filter(|p| *p > 0)
                .ok_or(ProbeError::InvalidData)
        })
        .collect()
}

fn group(pid: u32) -> Result<Option<u32>, ProbeError> {
    // SAFETY: read-only query, signals are a separate verified operation.
    let result = unsafe { libc::getpgid(pid as i32) };
    if result >= 0 {
        return Ok(Some(result as u32));
    }
    if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(None)
    } else {
        Err(ProbeError::PermissionDenied)
    }
}
/// Read-only recovery observation, never registration or signal authority.
pub fn exited_group_is_empty(pid: u32) -> Result<bool, ProbeError> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(ProbeError::InvalidData);
    }
    Ok(native::metadata(pid)?.is_none() && group_members(pid)?.is_empty())
}

fn verified_member(
    old: &Metadata,
    now: &Metadata,
    actual_group: Option<u32>,
    root: u32,
) -> Result<(), ProbeError> {
    if now.identity != old.identity || actual_group != Some(root) {
        return Err(ProbeError::IdentityChanged);
    }
    Ok(())
}

fn verified_group(
    members: &BTreeSet<u32>,
    known: &BTreeMap<u32, Metadata>,
) -> Result<(), ProbeError> {
    if members.iter().any(|p| !known.contains_key(p)) {
        return Err(ProbeError::OwnershipChanged);
    }
    Ok(())
}

impl OwnedChild {
    /// Existing bounded SDK shim; requires the original live child before/after.
    pub fn listening(&mut self, port: u16) -> Result<bool, ProbeError> {
        unsafe extern "C" {
            fn seiya_owned_listener(pid: libc::c_int, port: u16) -> libc::c_int;
        }
        self.refresh()?;
        if self.status.is_some() {
            return Err(ProbeError::RootExited);
        }
        // SAFETY: original child and identity freshly verified, fixed local port;
        // shim bounds SDK buffers. Revalidate identity after the query as well.
        let result = unsafe { seiya_owned_listener(self.pid() as i32, port) };
        self.refresh()?;
        match result {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ProbeError::Failed),
        }
    }
    pub fn spawn(command: &mut Command) -> Result<Self, ProbeError> {
        let mut child = command
            .process_group(0)
            .spawn()
            .map_err(|_| ProbeError::Failed)?;
        let result = (|| {
            let root = native::metadata(child.id())?.ok_or(ProbeError::RootExited)?;
            if root.parent_pid != std::process::id() || group(child.id())? != Some(child.id()) {
                return Err(ProbeError::OwnershipChanged);
            }
            Ok(root)
        })();
        match result {
            Ok(root) => Ok(Self {
                known: BTreeMap::from([(child.id(), root.clone())]),
                child,
                root,
                status: None,
                closed: false,
            }),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(e)
            }
        }
    }
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    pub fn status(&self) -> Option<ExitStatus> {
        self.status
    }
    pub fn refresh(&mut self) -> Result<Vec<u32>, ProbeError> {
        if self.status.is_none() {
            self.status = self.child.try_wait().map_err(|_| ProbeError::Failed)?;
        }
        if self.status.is_none() {
            let root = native::metadata(self.pid())?.ok_or(ProbeError::RootExited)?;
            if root != self.root || group(self.pid())? != Some(self.pid()) {
                return Err(ProbeError::IdentityChanged);
            }
            let mut todo = vec![self.pid()];
            let mut visited = BTreeSet::new();
            while let Some(parent) = todo.pop() {
                if !visited.insert(parent) {
                    continue;
                }
                for pid in native::children(parent)? {
                    let Some(meta) = native::metadata(pid)? else {
                        continue;
                    };
                    if meta.parent_pid != parent || group(pid)? != Some(self.pid()) {
                        return Err(ProbeError::OwnershipChanged);
                    }
                    if let Some(old) = self.known.get(&pid) {
                        if old != &meta {
                            return Err(ProbeError::IdentityChanged);
                        }
                    } else {
                        if self.known.len() >= MAX_TARGETS {
                            return Err(ProbeError::Limit);
                        }
                        if native::metadata(parent)?.as_ref() != self.known.get(&parent) {
                            return Err(ProbeError::OwnershipChanged);
                        }
                        self.known.insert(pid, meta);
                    }
                    todo.push(pid);
                }
            }
        }
        let members = group_members(self.pid())?;
        verified_group(&members, &self.known)?;
        let mut live = Vec::new();
        for (pid, old) in &self.known {
            if let Some(now) = native::metadata(*pid)? {
                verified_member(old, &now, group(*pid)?, self.pid())?;
                live.push(*pid);
            }
        }
        Ok(live)
    }
    pub fn rss(&self) -> Result<BTreeMap<u32, u64>, ProbeError> {
        let mut result = BTreeMap::new();
        for (pid, old) in &self.known {
            let Some(before) = native::metadata(*pid)? else {
                continue;
            };
            if before.identity != old.identity {
                return Err(ProbeError::IdentityChanged);
            }
            let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
            let size = std::mem::size_of::<libc::proc_taskinfo>() as i32;
            // SAFETY: exact-size SDK buffer; identity checked on both sides.
            let n = unsafe {
                libc::proc_pidinfo(
                    *pid as i32,
                    libc::PROC_PIDTASKINFO,
                    0,
                    info.as_mut_ptr().cast(),
                    size,
                )
            };
            let Some(after) = native::metadata(*pid)? else {
                continue;
            };
            if before != after || n != size {
                return Err(ProbeError::InvalidData);
            }
            // SAFETY: full initialized struct confirmed by exact return size.
            result.insert(*pid, unsafe { info.assume_init() }.pti_resident_size);
        }
        Ok(result)
    }
    fn signal_known(&self, pid: u32, signal: i32) -> Result<(), ProbeError> {
        let old = self.known.get(&pid).ok_or(ProbeError::OwnershipChanged)?;
        let Some(now) = native::metadata(pid)? else {
            return Ok(());
        };
        verified_member(old, &now, group(pid)?, self.pid())?;
        // SAFETY: original child or witnessed descendant, start identity and group
        // freshly verified. macOS has no pidfd; remaining syscall race is explicit.
        if unsafe { libc::kill(pid as i32, signal) } != 0
            && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
        {
            return Err(ProbeError::Failed);
        }
        Ok(())
    }
    pub fn close(&mut self) -> Result<Cleanup, ProbeError> {
        if self.closed {
            return Ok(Cleanup {
                tree_stopped: true,
                group_empty: true,
                processes_observed: self.known.len(),
                exit_code: self.status.and_then(|s| s.code()),
            });
        }
        // Quiesce verified parents before killing workers: a live parent must
        // not respawn a replacement between the tree query and termination.
        self.signal_known(self.pid(), libc::SIGSTOP)?;
        let mut frozen = false;
        for _ in 0..=MAX_TARGETS {
            let before = self.known.len();
            let mut pids: Vec<_> = self.known.keys().copied().collect();
            pids.sort_by_key(|p| *p != self.pid());
            for pid in pids {
                self.signal_known(pid, libc::SIGSTOP)?;
            }
            match self.refresh() {
                Ok(_) if before == self.known.len() => {
                    frozen = true;
                    break;
                }
                Ok(_) | Err(ProbeError::OwnershipChanged) => {}
                Err(e) => return Err(e),
            }
        }
        if !frozen {
            return Err(ProbeError::OwnershipChanged);
        }
        let mut pids = self.refresh()?;
        pids.sort_by_key(|p| *p == self.pid());
        // All recorded parents are paused; no group-wide signal, no discovery
        // by process name, no fresh authority from membership alone.
        for pid in pids {
            self.signal_known(pid, libc::SIGKILL)?;
        }
        let until = Instant::now() + Duration::from_secs(6);
        loop {
            self.refresh()?;
            if self.status.is_some() && group_members(self.pid())?.is_empty() {
                self.closed = true;
                return Ok(Cleanup {
                    tree_stopped: true,
                    group_empty: true,
                    processes_observed: self.known.len(),
                    exit_code: self.status.and_then(|s| s.code()),
                });
            }
            if Instant::now() >= until {
                return Err(ProbeError::TimedOut);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        for pid in self.known.keys().rev() {
            let _ = self.signal_known(*pid, libc::SIGKILL);
        }
        let _ = self.child.try_wait(); // uncertain cleanup never becomes confirmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn meta(pid: u32, start: &str) -> Metadata {
        Metadata {
            identity: crate::ProcessIdentity::new(pid, start).unwrap(),
            parent_pid: 1,
        }
    }
    #[test]
    fn reused_pid_changed_group_or_unknown_member_prevents_signalling() {
        let old = meta(20, "start1");
        assert!(verified_member(&old, &old, Some(20), 20).is_ok());
        assert!(verified_member(&old, &meta(20, "start2"), Some(20), 20).is_err());
        assert!(verified_member(&old, &meta(21, "start1"), Some(20), 20).is_err());
        assert!(verified_member(&old, &old, Some(99), 20).is_err());
        assert!(verified_member(&old, &old, None, 20).is_err());
        let known = BTreeMap::from([(20, old)]);
        assert!(verified_group(&BTreeSet::from([20]), &known).is_ok());
        assert!(verified_group(&BTreeSet::from([20, 21]), &known).is_err());
    }
}
