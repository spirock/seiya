//! Offline decoder only. A PID match is not authentication or Child ownership.
//! Allocator readings stay separate from native RSS and whole-host observations.
use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_REPORT_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingReason {
    MissingApi,
    ProbeFailed,
    InvalidValue,
    NoValidatedProbe,
}

// deserialize_with requires the JSON key even when its value is null.
fn nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Counter {
    #[serde(deserialize_with = "nullable")]
    pub value: Option<u64>,
    #[serde(deserialize_with = "nullable")]
    pub reason: Option<MissingReason>,
}

impl Counter {
    fn valid(&self) -> bool {
        self.value.is_some() != self.reason.is_some()
    }

    fn unsupported(&self) -> bool {
        self.value.is_none() && self.reason == Some(MissingReason::NoValidatedProbe)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PeakScope {
    RuntimeGlobalExternalResetsUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeMemoryReport {
    pub version: String,
    pub pid: u32,
    pub sequence: u64,
    pub read_started_ns: u64,
    pub read_finished_ns: u64,
    pub peak_scope: PeakScope,
    pub mlx_active_bytes: Counter,
    pub mlx_cache_bytes: Counter,
    pub mlx_peak_bytes: Counter,
    pub metal_allocated_bytes: Counter,
    pub footprint_bytes: Counter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Size,
    Json,
    InvalidReport,
    IdentityMismatch,
}

impl RuntimeMemoryReport {
    /// Expected PID comes from an owned Child, never from the report itself.
    /// Caller still must bind the channel to that Child/start identity and run,
    /// and validate cross-sample sequence/time against its own bounded state.
    pub fn decode(bytes: &[u8], expected_pid: u32) -> Result<Self, DecodeError> {
        if bytes.is_empty() || bytes.len() > MAX_REPORT_BYTES {
            return Err(DecodeError::Size);
        }
        let report: Self = serde_json::from_slice(bytes).map_err(|_| DecodeError::Json)?;
        if expected_pid == 0 || report.pid != expected_pid {
            return Err(DecodeError::IdentityMismatch);
        }
        if report.version != "seiya-runtime-memory/1"
            || report.read_finished_ns < report.read_started_ns
            || !report.mlx_active_bytes.valid()
            || !report.mlx_cache_bytes.valid()
            || !report.mlx_peak_bytes.valid()
            || !report.metal_allocated_bytes.unsupported()
            || !report.footprint_bytes.unsupported()
        {
            return Err(DecodeError::InvalidReport);
        }
        Ok(report)
    }
}
