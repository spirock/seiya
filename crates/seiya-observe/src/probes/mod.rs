//! Explicit opt-in adapter. Native registration requires an original Child handle.
mod native;
#[cfg(target_os = "macos")]
mod owned;
pub use native::{for_current_collector, from_child, CommandExecutor, NativeTable};
#[cfg(target_os = "macos")]
pub use owned::{exited_group_is_empty, OwnedChild};

use crate::{
    macos, Observation, ProbeFailure, ProcessIdentity, Provider, Role, Target, MAX_TARGETS,
};
use serde::Serialize;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeError {
    RootExited,
    IdentityChanged,
    OwnershipChanged,
    Limit,
    TimedOut,
    Failed,
    PermissionDenied,
    InvalidData,
    Unsupported,
    Closed,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Metadata {
    pub identity: ProcessIdentity,
    pub parent_pid: u32,
}

/// Internal injection seam, not a permission to inspect arbitrary processes.
pub trait ProcessTable {
    fn root_alive(&mut self) -> Result<bool, ProbeError>;
    fn metadata(&mut self, pid: u32) -> Result<Option<Metadata>, ProbeError>;
    fn children(&mut self, pid: u32) -> Result<Vec<u32>, ProbeError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Rss(u32),
    VmStat,
    Swap,
}
pub type ProbeFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>, ProbeError>> + Send + 'a>>;
pub trait Executor {
    fn run(&mut self, request: Request) -> ProbeFuture<'_>;
}

struct Node {
    target: Target,
    parent: Option<usize>,
    parent_pid: u32,
    exited: bool,
}

pub struct Readings {
    items: Vec<(Target, Option<Observation>)>,
}
impl Readings {
    pub fn targets(&self) -> Vec<Target> {
        self.items
            .iter()
            .map(|(target, _)| target.clone())
            .collect()
    }
}
impl Provider for Readings {
    fn observe(&mut self, expected: &ProcessIdentity) -> Observation {
        self.items
            .iter_mut()
            .find(|(target, _)| &target.identity == expected)
            .and_then(|(_, observation)| observation.take())
            .unwrap_or(Observation::Unavailable(ProbeFailure::InvalidData))
    }
}

#[derive(Serialize)]
pub struct SystemReadings {
    pub scope: &'static str,
    pub vm_stat: Result<macos::VmStat, ProbeError>,
    pub swap: Result<macos::SwapUsage, ProbeError>,
}

/// Constructors are private: native callers enter via from_child, never a free PID.
pub struct Adapter<T, E> {
    table: T,
    executor: E,
    nodes: Vec<Node>,
    failed: bool,
}

impl<T: ProcessTable, E: Executor> Adapter<T, E> {
    /// Bounded identity/ancestry inspection only; no RSS/VM/executor commands.
    pub fn inspect_targets(&mut self) -> Result<Vec<Target>, ProbeError> {
        let result = (|| {
            self.root()?;
            self.discover()?;
            self.root()?;
            Ok(self.nodes.iter().map(|node| node.target.clone()).collect())
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// Identity/role established by the registration witness, not a client PID.
    pub fn root_target(&self) -> Target {
        self.nodes[0].target.clone()
    }
    fn new(table: T, executor: E, root: Metadata, role: Role) -> Self {
        Self {
            table,
            executor,
            nodes: vec![Node {
                target: Target {
                    identity: root.identity,
                    role,
                },
                parent: None,
                parent_pid: root.parent_pid,
                exited: false,
            }],
            failed: false,
        }
    }

    fn root(&mut self) -> Result<(), ProbeError> {
        if self.failed {
            return Err(ProbeError::Closed);
        }
        if !self.table.root_alive()? {
            return Err(ProbeError::RootExited);
        }
        if self.verify(0)?.is_none() {
            return Err(ProbeError::RootExited);
        }
        Ok(())
    }

    /// Verify the entire recorded ancestry without a global process-table scan.
    fn verify(&mut self, index: usize) -> Result<Option<Metadata>, ProbeError> {
        let mut cursor = Some(index);
        let mut result = None;
        while let Some(i) = cursor {
            let node = &self.nodes[i];
            let Some(meta) = self.table.metadata(node.target.identity.pid())? else {
                return if i == index {
                    Ok(None)
                } else {
                    Err(ProbeError::OwnershipChanged)
                };
            };
            if meta.identity != node.target.identity {
                return Err(ProbeError::IdentityChanged);
            }
            if meta.parent_pid != node.parent_pid {
                return Err(ProbeError::OwnershipChanged);
            }
            if i == index {
                result = Some(meta);
            }
            cursor = node.parent;
        }
        Ok(result)
    }

    fn discover(&mut self) -> Result<(), ProbeError> {
        let mut index = 0;
        while index < self.nodes.len() {
            if self.nodes[index].exited {
                index += 1;
                continue;
            }
            if self.verify(index)?.is_none() {
                self.nodes[index].exited = true;
                index += 1;
                continue;
            }
            let pid = self.nodes[index].target.identity.pid();
            let children = self.table.children(pid)?;
            if children.len() > MAX_TARGETS {
                return Err(ProbeError::Limit);
            }
            let mut seen = HashSet::new();
            for child in children {
                if child == 0 || !seen.insert(child) {
                    return Err(ProbeError::InvalidData);
                }
                let Some(meta) = self.table.metadata(child)? else {
                    continue;
                };
                if meta.parent_pid != pid {
                    return Err(ProbeError::OwnershipChanged);
                }
                if self.verify(index)?.is_none() {
                    return Err(ProbeError::OwnershipChanged);
                }
                if self.table.metadata(child)?.as_ref() != Some(&meta) {
                    return Err(ProbeError::IdentityChanged);
                }
                if let Some(known) = self
                    .nodes
                    .iter()
                    .find(|node| node.target.identity.pid() == child)
                {
                    if known.target.identity != meta.identity
                        || known.parent != Some(index)
                        || known.exited
                    {
                        return Err(ProbeError::IdentityChanged);
                    }
                } else {
                    if self.nodes.len() >= MAX_TARGETS {
                        return Err(ProbeError::Limit);
                    }
                    self.nodes.push(Node {
                        target: Target {
                            identity: meta.identity,
                            role: Role::Descendant,
                        },
                        parent: Some(index),
                        parent_pid: pid,
                        exited: false,
                    });
                }
            }
            if self.verify(index)?.is_none() {
                return Err(ProbeError::OwnershipChanged);
            }
            index += 1;
        }
        Ok(())
    }

    pub async fn sample(&mut self) -> Result<Readings, ProbeError> {
        let result = self.sample_inner().await;
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    async fn sample_inner(&mut self) -> Result<Readings, ProbeError> {
        self.root()?;
        self.discover()?;
        let mut items = Vec::with_capacity(self.nodes.len());
        for index in 0..self.nodes.len() {
            let target = self.nodes[index].target.clone();
            let observation = if self.nodes[index].exited || self.verify(index)?.is_none() {
                self.nodes[index].exited = true;
                Observation::Exited
            } else {
                let raw = self.executor.run(Request::Rss(target.identity.pid())).await;
                // A PID that exits/reparents/recycles while ps runs is not accepted.
                if self.verify(index)?.is_none() {
                    self.nodes[index].exited = true;
                    Observation::Exited
                } else {
                    match raw {
                        Ok(bytes) => match std::str::from_utf8(&bytes)
                            .ok()
                            .and_then(|text| macos::parse_rss(target.identity.pid(), text).ok())
                        {
                            Some(rss_kib) => Observation::Present {
                                identity: target.identity.clone(),
                                rss_kib,
                            },
                            None => Observation::Unavailable(ProbeFailure::InvalidData),
                        },
                        Err(ProbeError::TimedOut) => {
                            Observation::Unavailable(ProbeFailure::TimedOut)
                        }
                        Err(ProbeError::PermissionDenied) => {
                            Observation::Unavailable(ProbeFailure::PermissionDenied)
                        }
                        Err(ProbeError::Limit | ProbeError::InvalidData) => {
                            Observation::Unavailable(ProbeFailure::InvalidData)
                        }
                        Err(_) => Observation::Unavailable(ProbeFailure::Failed),
                    }
                }
            };
            items.push((target, Some(observation)));
        }
        self.root()?;
        Ok(Readings { items })
    }

    pub async fn sample_system(&mut self) -> Result<SystemReadings, ProbeError> {
        if let Err(error) = self.root() {
            self.failed = true;
            return Err(error);
        }
        let vm = self.executor.run(Request::VmStat).await.and_then(|bytes| {
            let text = std::str::from_utf8(&bytes).map_err(|_| ProbeError::InvalidData)?;
            macos::parse_vm_stat(text).map_err(|_| ProbeError::InvalidData)
        });
        let swap = self.executor.run(Request::Swap).await.and_then(|bytes| {
            let text = std::str::from_utf8(&bytes).map_err(|_| ProbeError::InvalidData)?;
            macos::parse_swap(text).map_err(|_| ProbeError::InvalidData)
        });
        if let Err(error) = self.root() {
            self.failed = true;
            return Err(error);
        }
        Ok(SystemReadings {
            scope: "whole-host",
            vm_stat: vm,
            swap,
        })
    }
}

#[cfg(test)]
mod tests;
