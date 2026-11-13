//! Pure parsers for bounded, locale-C macOS command output. No host execution.
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

pub const MAX_BYTES: usize = 16 * 1024;
pub const MAX_LINES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    Limit,
    InvalidData,
    PidMismatch,
}

fn bounded(text: &str) -> Result<(), ParseError> {
    if text.len() > MAX_BYTES || text.lines().count() > MAX_LINES {
        return Err(ParseError::Limit);
    }
    if !text.is_ascii()
        || text
            .bytes()
            .any(|b| b.is_ascii_control() && b != b'\n' && b != b'\t')
    {
        return Err(ParseError::InvalidData);
    }
    Ok(())
}

fn number(text: &str) -> Result<u64, ParseError> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::InvalidData);
    }
    text.parse().map_err(|_| ParseError::InvalidData)
}

/// RSS only: the caller must independently verify a strong start identity.
pub fn parse_rss(expected_pid: u32, text: &str) -> Result<u64, ParseError> {
    bounded(text)?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let line = lines.next().ok_or(ParseError::InvalidData)?;
    if lines.next().is_some() || expected_pid == 0 {
        return Err(ParseError::InvalidData);
    }
    let parts: Vec<_> = line.split_whitespace().collect();
    if parts.len() != 2 {
        return Err(ParseError::InvalidData);
    }
    if number(parts[0])? != u64::from(expected_pid) {
        return Err(ParseError::PidMismatch);
    }
    let kib = number(parts[1])?;
    kib.checked_mul(1024).ok_or(ParseError::InvalidData)?;
    Ok(kib)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PageCounter {
    Free,
    Active,
    Inactive,
    Speculative,
    Wired,
    Compressor,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct VmStat {
    pub page_size_bytes: u64,
    pub pages: BTreeMap<PageCounter, u64>,
}

pub fn parse_vm_stat(text: &str) -> Result<VmStat, ParseError> {
    bounded(text)?;
    let mut lines = text.lines();
    let size = lines
        .next()
        .and_then(|line| line.strip_prefix("Mach Virtual Memory Statistics: (page size of "))
        .and_then(|rest| rest.strip_suffix(" bytes)"))
        .ok_or(ParseError::InvalidData)?;
    let page_size_bytes = number(size)?;
    if !(1024..=65536).contains(&page_size_bytes) || !page_size_bytes.is_power_of_two() {
        return Err(ParseError::InvalidData);
    }
    let mut pages = BTreeMap::new();
    let mut seen = HashSet::new();
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let (name, value) = line.split_once(':').ok_or(ParseError::InvalidData)?;
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphabetic() || b == b' ' || b == b'"' || b == b'-')
            || !seen.insert(name)
        {
            return Err(ParseError::InvalidData);
        }
        let value = value.trim();
        let count = number(value.strip_suffix('.').unwrap_or(value))?;
        let counter = match name {
            "Pages free" => Some(PageCounter::Free),
            "Pages active" => Some(PageCounter::Active),
            "Pages inactive" => Some(PageCounter::Inactive),
            "Pages speculative" => Some(PageCounter::Speculative),
            "Pages wired down" => Some(PageCounter::Wired),
            "Pages occupied by compressor" => Some(PageCounter::Compressor),
            _ => None,
        };
        if let Some(counter) = counter {
            count
                .checked_mul(page_size_bytes)
                .ok_or(ParseError::InvalidData)?;
            pages.insert(counter, count);
        }
    }
    if pages.is_empty() {
        return Err(ParseError::InvalidData);
    }
    Ok(VmStat {
        page_size_bytes,
        pages,
    })
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct SwapUsage {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

fn bytes(text: &str) -> Result<(u64, u64), ParseError> {
    let (numeric, suffix) =
        text.split_at(text.len().checked_sub(1).ok_or(ParseError::InvalidData)?);
    let factor: u128 = match suffix {
        "B" => 1,
        "K" => 1024,
        "M" => 1024 * 1024,
        "G" => 1024 * 1024 * 1024,
        "T" => 1024_u128.pow(4),
        _ => return Err(ParseError::InvalidData),
    };
    let (whole, fraction) = numeric.split_once('.').unwrap_or((numeric, ""));
    if fraction.len() > 2 || (numeric.contains('.') && fraction.is_empty()) {
        return Err(ParseError::InvalidData);
    }
    let whole = u128::from(number(whole)?);
    let fraction = if fraction.is_empty() {
        0
    } else {
        u128::from(number(fraction)?) * if fraction.len() == 1 { 10 } else { 1 }
    };
    let count = (whole * 100 + fraction)
        .checked_mul(factor)
        .ok_or(ParseError::InvalidData)?
        / 100;
    let count = u64::try_from(count).map_err(|_| ParseError::InvalidData)?;
    Ok((
        count,
        u64::try_from(factor.div_ceil(100)).map_err(|_| ParseError::InvalidData)?,
    ))
}

pub fn parse_swap(text: &str) -> Result<SwapUsage, ParseError> {
    bounded(text)?;
    if text.trim().lines().count() != 1 {
        return Err(ParseError::InvalidData);
    }
    let parts: Vec<_> = text
        .strip_prefix("vm.swapusage:")
        .ok_or(ParseError::InvalidData)?
        .split_whitespace()
        .collect();
    if !(parts.len() == 9 || parts.len() == 10 && parts[9] == "(encrypted)")
        || parts[0] != "total"
        || parts[1] != "="
        || parts[3] != "used"
        || parts[4] != "="
        || parts[6] != "free"
        || parts[7] != "="
    {
        return Err(ParseError::InvalidData);
    }
    let (total_bytes, a) = bytes(parts[2])?;
    let (used_bytes, b) = bytes(parts[5])?;
    let (free_bytes, c) = bytes(parts[8])?;
    let sum = used_bytes
        .checked_add(free_bytes)
        .ok_or(ParseError::InvalidData)?;
    if used_bytes > total_bytes || free_bytes > total_bytes || sum.abs_diff(total_bytes) > a + b + c
    {
        return Err(ParseError::InvalidData);
    }
    Ok(SwapUsage {
        total_bytes,
        used_bytes,
        free_bytes,
    })
}
