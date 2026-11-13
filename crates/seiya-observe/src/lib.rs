//! Observation core with explicit opt-in probes. No automatic host work or policy.
use serde::Serialize;
use std::collections::HashSet;

pub mod campaign;
pub mod campaign_summary;
pub mod fixture_driver;
pub mod image;
pub mod macos;
pub mod probes;
pub mod runner;
pub mod runtime_memory;
mod runtime_memory_channel;
pub mod session_containment;
pub mod session_evidence;
pub mod session_guard;
pub mod session_lease;
pub mod session_native_fixture;
pub mod session_pressure;
pub mod session_runtime;
pub mod session_supervisor;

pub const MAX_TARGETS: usize = 32;
pub const MAX_SNAPSHOTS: u32 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidIdentity,
    InvalidLimits,
    DuplicatePid,
    ClockRegressed,
    IdentityChanged,
    Failed,
    SnapshotLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProcessIdentity {
    pid: u32,
    start_marker: String,
}

impl ProcessIdentity {
    pub fn new(pid: u32, start_marker: impl Into<String>) -> Result<Self, Error> {
        let start_marker = start_marker.into();
        if pid == 0
            || start_marker.is_empty()
            || start_marker.len() > 128
            || !start_marker.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(Error::InvalidIdentity);
        }
        Ok(Self { pid, start_marker })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn start_marker(&self) -> &str {
        &self.start_marker
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Runtime,
    Coordinator,
    Collector,
    OpencodeCli,
    OpencodePrivateServer,
    Descendant,
}

#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub identity: ProcessIdentity,
    pub role: Role,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    ListenerReady,
    ColdText,
    ToolRoundtrip,
    WarmRepeat,
    Idle,
    Resume,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Exited,
    ProbeFailed,
    PermissionDenied,
    InvalidData,
    NoValidatedProbe,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unavailable,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Measurement {
    value: Option<u64>,
    unit: &'static str,
    scope: &'static str,
    method: Option<&'static str>,
    availability: Availability,
    reason: Option<Reason>,
}

impl Measurement {
    fn rss(result: Result<u64, Reason>) -> Self {
        let (value, availability, reason) = match result {
            Ok(bytes) => (Some(bytes), Availability::Available, None),
            Err(reason) => (None, Availability::Unavailable, Some(reason)),
        };
        Self {
            value,
            unit: "bytes",
            scope: "process",
            method: Some("rss-kib-times-1024"),
            availability,
            reason,
        }
    }

    fn unsupported() -> Self {
        Self {
            value: None,
            unit: "bytes",
            scope: "process",
            method: None,
            availability: Availability::Unsupported,
            reason: Some(Reason::NoValidatedProbe),
        }
    }

    pub fn value(&self) -> Option<u64> {
        self.value
    }
    pub fn availability(&self) -> Availability {
        self.availability
    }
    pub fn reason(&self) -> Option<Reason> {
        self.reason
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ProbeFailure {
    Failed,
    PermissionDenied,
    InvalidData,
    TimedOut,
}

/// The real provider/launcher must prove ownership independently of this core.
pub enum Observation {
    Present {
        identity: ProcessIdentity,
        rss_kib: u64,
    },
    Exited,
    Unavailable(ProbeFailure),
}

pub trait Provider {
    fn observe(&mut self, expected: &ProcessIdentity) -> Observation;
}

#[derive(Debug, Serialize)]
pub struct ProcessSample {
    pub identity: ProcessIdentity,
    pub role: Role,
    pub rss: Measurement,
    pub footprint: Measurement,
    pub mlx_active_memory: Measurement,
    pub metal_allocated_memory: Measurement,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub diagnostic_version: u8,
    pub elapsed_millis: u64,
    pub phase: Phase,
    pub processes: Vec<ProcessSample>,
}

struct Tracked {
    target: Target,
    exited: bool,
}

/// Bounded targets and snapshot count, with no retained sample history.
/// Timestamps are supplied by the caller's monotonic clock, not wall time.
pub struct Collector {
    targets: Vec<Tracked>,
    max_snapshots: u32,
    snapshots: u32,
    last_millis: Option<u64>,
    failed: bool,
}

impl Collector {
    pub fn new(targets: Vec<Target>, max_snapshots: u32) -> Result<Self, Error> {
        if targets.is_empty()
            || targets.len() > MAX_TARGETS
            || !(1..=MAX_SNAPSHOTS).contains(&max_snapshots)
        {
            return Err(Error::InvalidLimits);
        }
        let mut seen = HashSet::new();
        for target in &targets {
            if !seen.insert(target.identity.pid()) {
                return Err(Error::DuplicatePid);
            }
        }
        Ok(Self {
            targets: targets
                .into_iter()
                .map(|target| Tracked {
                    target,
                    exited: false,
                })
                .collect(),
            max_snapshots,
            snapshots: 0,
            last_millis: None,
            failed: false,
        })
    }

    pub fn collect(
        &mut self,
        elapsed_millis: u64,
        phase: Phase,
        provider: &mut impl Provider,
    ) -> Result<Snapshot, Error> {
        if self.failed {
            return Err(Error::Failed);
        }
        if self.snapshots >= self.max_snapshots {
            return Err(Error::SnapshotLimit);
        }
        if self.last_millis.is_some_and(|last| elapsed_millis < last) {
            self.failed = true;
            return Err(Error::ClockRegressed);
        }
        let mut processes = Vec::with_capacity(self.targets.len());
        for tracked in &mut self.targets {
            let rss = if tracked.exited {
                Err(Reason::Exited)
            } else {
                match provider.observe(&tracked.target.identity) {
                    Observation::Present { identity, rss_kib } => {
                        if identity != tracked.target.identity {
                            self.failed = true;
                            return Err(Error::IdentityChanged);
                        }
                        rss_kib.checked_mul(1024).ok_or(Reason::InvalidData)
                    }
                    Observation::Exited => {
                        tracked.exited = true;
                        Err(Reason::Exited)
                    }
                    Observation::Unavailable(failure) => Err(match failure {
                        ProbeFailure::Failed => Reason::ProbeFailed,
                        ProbeFailure::PermissionDenied => Reason::PermissionDenied,
                        ProbeFailure::InvalidData => Reason::InvalidData,
                        ProbeFailure::TimedOut => Reason::TimedOut,
                    }),
                }
            };
            processes.push(ProcessSample {
                identity: tracked.target.identity.clone(),
                role: tracked.target.role,
                rss: Measurement::rss(rss),
                footprint: Measurement::unsupported(),
                mlx_active_memory: Measurement::unsupported(),
                metal_allocated_memory: Measurement::unsupported(),
            });
        }
        self.snapshots += 1;
        self.last_millis = Some(elapsed_millis);
        Ok(Snapshot {
            diagnostic_version: 1,
            elapsed_millis,
            phase,
            processes,
        })
    }
}
