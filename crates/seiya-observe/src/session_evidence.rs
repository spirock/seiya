//! Opt-in, synthetic session evidence. No runtime, network, or automatic host IO.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const VERSION: &str = "chat-evidence/0.1.0";
pub const WORKLOAD_VERSION: &str = "chat-workload/0.1.0";
pub const WORKLOAD: &str =
    include_str!("../../../contracts/benchmarks/chat-session/v0.1.0/workload.json");
pub const MAX_FILE_BYTES: u64 = 256 * 1024;
pub const MAX_RECORDS: usize = 32;
pub const MAX_TOTAL_BYTES: u64 = MAX_FILE_BYTES * MAX_RECORDS as u64;
pub const MAX_EVENTS: usize = 128;
const MAX_MILLIS: u64 = 3_600_000;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Limit,
    Io,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Synthetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    RssBytes,
    MlxBytes,
    MetalBytes,
    FootprintBytes,
    QueueMillis,
    LoadMillis,
    SearchMillis,
    ReadMillis,
    InferenceMillis,
    CleanupMillis,
    TtftMillis,
    PromptTokens,
    OutputTokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Synthetic,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingReason {
    NoValidatedProbe,
    BufferedTransport,
    NotObserved,
    ProbeFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub kind: MetricKind,
    pub value: Option<u64>,
    pub origin: Origin,
    pub missing_reason: Option<MissingReason>,
}

impl Metric {
    fn validate(&self) -> Result<(), Error> {
        match (self.value, self.origin, self.missing_reason) {
            (None, Origin::Unavailable, Some(_)) => Ok(()),
            (Some(value), Origin::Synthetic, None) => {
                let max = match self.kind {
                    MetricKind::RssBytes
                    | MetricKind::MlxBytes
                    | MetricKind::MetalBytes
                    | MetricKind::FootprintBytes => 1 << 50,
                    MetricKind::PromptTokens | MetricKind::OutputTokens => 1_000_000,
                    _ => MAX_MILLIS,
                };
                if value <= max {
                    Ok(())
                } else {
                    Err(Error::Limit)
                }
            }
            _ => Err(Error::Invalid),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queue,
    Load,
    Search,
    Read,
    Generate,
    Idle,
    Resume,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pressure {
    Normal,
    Warning,
    Critical,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub at_millis: u64,
    pub phase: Phase,
    pub pressure: Pressure,
    pub metrics: Vec<Metric>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    Partial,
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    Pressure,
    Limit,
    RuntimeUnavailable,
    TimedOut,
    Cancelled,
    UnconfirmedCleanup,
    SourceUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub outcome: Outcome,
    pub cause: Option<Cause>,
    pub elapsed_millis: u64,
    pub cleanup_confirmed: bool,
    pub generation_completed: bool,
    pub task_resolved: Option<bool>,
    pub sources_available: u8,
    pub pages_attempted: u8,
    pub pages_read: u8,
    pub valid_citations: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub schema_version: String,
    pub mode: Mode,
    pub experiment_id: String,
    pub workload_version: String,
    pub profile_sha256: String,
    pub checkpoint_sha256: String,
    pub runtime_recipe_sha256: String,
    pub host_profile_sha256: String,
    pub events: Vec<Event>,
    pub summary: Summary,
}

fn canonical_id(id: &str) -> bool {
    Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id && !uuid.is_nil())
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Evidence {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(Error::Limit);
        }
        let result: Self = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.schema_version != VERSION
            || self.workload_version != WORKLOAD_VERSION
            || !canonical_id(&self.experiment_id)
            || [
                &self.profile_sha256,
                &self.checkpoint_sha256,
                &self.runtime_recipe_sha256,
                &self.host_profile_sha256,
            ]
            .iter()
            .any(|s| !digest(s))
        {
            return Err(Error::Invalid);
        }
        if self.events.len() > MAX_EVENTS || self.summary.elapsed_millis > MAX_MILLIS {
            return Err(Error::Limit);
        }
        let summary = &self.summary;
        if summary.sources_available > 15
            || summary.pages_attempted > summary.sources_available
            || summary.pages_attempted > 3
            || summary.pages_read > summary.pages_attempted
            || summary.valid_citations > summary.sources_available
            || (summary.task_resolved == Some(true) && !summary.generation_completed)
            || (summary.outcome == Outcome::Completed
                && (!summary.cleanup_confirmed || summary.cause.is_some()))
            || (summary.outcome != Outcome::Completed && summary.cause.is_none())
        {
            return Err(Error::Invalid);
        }
        let mut previous = 0;
        for event in &self.events {
            if event.at_millis < previous || event.at_millis > summary.elapsed_millis {
                return Err(Error::Invalid);
            }
            previous = event.at_millis;
            if event.metrics.len() > 13 {
                return Err(Error::Limit);
            }
            let mut kinds = HashSet::new();
            for metric in &event.metrics {
                if !kinds.insert(metric.kind) {
                    return Err(Error::Invalid);
                }
                metric.validate()?;
            }
        }
        Ok(())
    }

    /// Equality of declared recipe keys, not statistical or provenance proof.
    pub fn comparable_with(&self, other: &Self) -> bool {
        self.validate().is_ok()
            && other.validate().is_ok()
            && self.schema_version == other.schema_version
            && self.mode == other.mode
            && self.workload_version == other.workload_version
            && self.profile_sha256 == other.profile_sha256
            && self.checkpoint_sha256 == other.checkpoint_sha256
            && self.runtime_recipe_sha256 == other.runtime_recipe_sha256
            && self.host_profile_sha256 == other.host_profile_sha256
    }
}

#[derive(Debug, Serialize)]
pub struct IndexEntry {
    pub experiment_id: String,
    pub mode: Mode,
    pub workload_version: String,
    pub profile_sha256: String,
    pub checkpoint_sha256: String,
    pub runtime_recipe_sha256: String,
    pub host_profile_sha256: String,
    pub summary: Summary,
}
#[derive(Debug, Serialize)]
pub struct Index {
    pub schema_version: &'static str,
    pub incomplete_files: usize,
    pub entries: Vec<IndexEntry>,
}

/// Single writer for a NEW private directory. Not an arbitrary file importer.
pub struct Store {
    directory: PathBuf,
}

impl Store {
    /// Explicit read-only recovery after restart; no writer/replacement rights.
    /// The operator must supply their own private synthetic evidence directory.
    pub fn read_index(directory: PathBuf) -> Result<Index, Error> {
        if !directory.is_absolute() {
            return Err(Error::Invalid);
        }
        Self { directory }.rebuild_index()
    }

    pub fn create(directory: PathBuf) -> Result<Self, Error> {
        if !directory.is_absolute() {
            return Err(Error::Invalid);
        }
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        builder.create(&directory).map_err(|_| Error::Conflict)?;
        Ok(Self { directory })
    }

    fn inventory(&self) -> Result<(Vec<PathBuf>, usize, u64), Error> {
        let root = fs::symlink_metadata(&self.directory).map_err(|_| Error::Io)?;
        if !root.is_dir() || root.file_type().is_symlink() {
            return Err(Error::Invalid);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if root.permissions().mode() & 0o077 != 0 {
                return Err(Error::Invalid);
            }
        }
        let mut records = Vec::new();
        let mut pending = 0;
        let mut total = 0u64;
        let mut count = 0;
        for entry in fs::read_dir(&self.directory).map_err(|_| Error::Io)? {
            count += 1;
            if count > MAX_RECORDS * 2 {
                return Err(Error::Limit);
            }
            let entry = entry.map_err(|_| Error::Io)?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|_| Error::Io)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(Error::Invalid);
            }
            if metadata.len() > MAX_FILE_BYTES {
                return Err(Error::Limit);
            }
            total = total
                .checked_add(metadata.len())
                .filter(|v| *v <= MAX_TOTAL_BYTES)
                .ok_or(Error::Limit)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Error::Invalid)?;
            if name.strip_prefix(".pending-").is_some_and(canonical_id) {
                pending += 1;
            } else if name.strip_suffix(".json").is_some_and(canonical_id) {
                records.push(entry.path());
            } else {
                return Err(Error::Invalid);
            }
        }
        if records.len() > MAX_RECORDS {
            return Err(Error::Limit);
        }
        records.sort();
        Ok((records, pending, total))
    }

    pub fn publish(&mut self, evidence: &Evidence) -> Result<(), Error> {
        self.publish_before_commit(evidence, |_| Ok(()))
    }

    fn publish_before_commit(
        &mut self,
        evidence: &Evidence,
        before_commit: impl FnOnce(&Path) -> Result<(), Error>,
    ) -> Result<(), Error> {
        evidence.validate()?;
        let bytes = serde_json::to_vec(evidence).map_err(|_| Error::Invalid)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(Error::Limit);
        }
        let (records, pending, total) = self.inventory()?;
        if records.len() >= MAX_RECORDS
            || pending >= MAX_RECORDS
            || total + bytes.len() as u64 > MAX_TOTAL_BYTES
        {
            return Err(Error::Limit);
        }
        let destination = self
            .directory
            .join(format!("{}.json", evidence.experiment_id));
        if destination.try_exists().map_err(|_| Error::Io)? {
            return Err(Error::Conflict);
        }
        let temporary = self.directory.join(format!(".pending-{}", Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|_| Error::Io)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| Error::Io)?;
        before_commit(&temporary)?;
        // Atomic publication without replacing an existing path. Failed IO leaves
        // the private pending file as evidence, never a partially visible record.
        fs::hard_link(&temporary, &destination).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::Conflict
            } else {
                Error::Io
            }
        })?;
        fs::remove_file(&temporary).map_err(|_| Error::Io)?;
        File::open(&self.directory)
            .and_then(|f| f.sync_all())
            .map_err(|_| Error::Io)?;
        Ok(())
    }

    pub fn rebuild_index(&self) -> Result<Index, Error> {
        let (records, incomplete_files, _) = self.inventory()?;
        let mut entries = Vec::new();
        for path in records {
            let evidence = read_evidence(&path)?;
            if path.file_stem().and_then(|v| v.to_str()) != Some(&evidence.experiment_id) {
                return Err(Error::Invalid);
            }
            entries.push(IndexEntry {
                experiment_id: evidence.experiment_id,
                mode: evidence.mode,
                workload_version: evidence.workload_version,
                profile_sha256: evidence.profile_sha256,
                checkpoint_sha256: evidence.checkpoint_sha256,
                runtime_recipe_sha256: evidence.runtime_recipe_sha256,
                host_profile_sha256: evidence.host_profile_sha256,
                summary: evidence.summary,
            });
        }
        Ok(Index {
            schema_version: VERSION,
            incomplete_files,
            entries,
        })
    }
}

fn read_evidence(path: &Path) -> Result<Evidence, Error> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options.open(path).map_err(|_| Error::Io)?;
    if !file.metadata().map_err(|_| Error::Io)?.is_file() {
        return Err(Error::Invalid);
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Io)?;
    Evidence::parse(&bytes)
}

#[cfg(test)]
mod tests;
