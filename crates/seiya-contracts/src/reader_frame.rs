//! Bounded offline validation of draft reader frames. No process or network access.
use serde::Deserialize;
use std::collections::HashSet;
use std::io::Read;

const VERSION: &str = "0.1-draft.1";
const REQUEST_CAP: usize = 16 * 1024;
const RESPONSE_CAP: usize = 128 * 1024;

/// Synchronous framing primitive for already deadline-controlled streams.
/// A blocked `Read` is not interrupted by this function; callers must own and
/// enforce the deadline and process lifecycle externally. No unbounded buffer.
fn read_frame<R: Read>(reader: R, cap: usize) -> Result<Vec<u8>, &'static str> {
    let mut bytes = Vec::new();
    reader
        .take((cap + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "read_error")?;
    if bytes.len() > cap {
        return Err("invalid_frame");
    }
    Ok(bytes)
}

/// Reads through actual EOF with a bounded buffer; caller enforces deadline.
pub fn read_request<R: Read>(reader: R) -> Result<Request, &'static str> {
    let bytes = read_frame(reader, REQUEST_CAP)?;
    parse_request(&bytes, true)
}

/// Reads through actual EOF with a bounded buffer; caller enforces deadline.
pub fn read_response<R: Read>(request: &Request, reader: R) -> Result<(), &'static str> {
    let bytes = read_frame(reader, RESPONSE_CAP)?;
    validate_response(request, &bytes, true)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    version: String,
    run_id: String,
    remaining_ms: u32,
    sources: Vec<Source>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    version: String,
    run_id: String,
    results: Vec<Page>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Ok,
    PolicyBlocked,
    Timeout,
    LimitExceeded,
    HttpError,
    UnsupportedFormat,
    NoUsefulText,
    TransportError,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    id: String,
    status: Status,
    #[serde(deserialize_with = "required_text")]
    text: Option<String>,
    text_truncated: bool,
    received_bytes: u32,
    decoded_bytes: u32,
    header_bytes: u32,
    get_count: u32,
}

fn required_text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}

fn id_valid(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl Request {
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn remaining_ms(&self) -> u32 {
        self.remaining_ms
    }

    pub fn sources(&self) -> impl Iterator<Item = (&str, &str)> {
        self.sources.iter().map(|s| (s.id.as_str(), s.url.as_str()))
    }

    /// Compare against the host's own results for one run. The supplied mapping
    /// is trusted host state, not data from the reader, model, or page.
    pub fn matches_host_results(&self, run_id: &str, results: &[(&str, &str)]) -> bool {
        if self.run_id != run_id {
            return false;
        }
        self.sources.iter().all(|source| {
            let mut candidates = results.iter().filter(|(id, _)| *id == source.id);
            matches!(candidates.next(), Some((_, url)) if *url == source.url)
                && candidates.next().is_none()
        })
    }
}

/// Called only after bounded incremental read and confirmed EOF. This function
/// does not establish source provenance, deadline, URL safety, or permission.
pub fn parse_request(bytes: &[u8], eof: bool) -> Result<Request, &'static str> {
    if !eof || bytes.len() > REQUEST_CAP {
        return Err("invalid_frame");
    }
    let req: Request = serde_json::from_slice(bytes).map_err(|_| "invalid_json")?;
    if req.version != VERSION
        || !id_valid(&req.run_id)
        || !(1..=45_000).contains(&req.remaining_ms)
        || !(1..=3).contains(&req.sources.len())
    {
        return Err("invalid_request");
    }
    let mut ids = HashSet::new();
    if req.sources.iter().any(|s| {
        !id_valid(&s.id)
            || !ids.insert(&s.id)
            || s.url.is_empty()
            || s.url.len() > 4096
            || s.url.chars().any(char::is_control)
    }) {
        return Err("invalid_source");
    }
    Ok(req)
}

/// Checks shape, counters and correlation only; host must separately verify
/// every requested source against its own current-run search results.
pub fn validate_response(req: &Request, bytes: &[u8], eof: bool) -> Result<(), &'static str> {
    if !eof || bytes.len() > RESPONSE_CAP {
        return Err("invalid_frame");
    }
    let resp: Response = serde_json::from_slice(bytes).map_err(|_| "invalid_json")?;
    if resp.version != VERSION
        || resp.run_id != req.run_id
        || resp.results.len() != req.sources.len()
    {
        return Err("invalid_response");
    }
    let mut total_text = 0usize;
    for (page, source) in resp.results.iter().zip(&req.sources) {
        if page.id != source.id
            || page.received_bytes > 1_048_576
            || page.decoded_bytes > 1_048_576
            || page.header_bytes > 196_608
            || page.get_count > 3
            || page.header_bytes > page.get_count * 65_536
            || (page.get_count == 0 && (page.received_bytes != 0 || page.decoded_bytes != 0))
        {
            return Err("invalid_result");
        }
        match (&page.status, &page.text) {
            (Status::Ok, Some(text)) if !text.trim().is_empty() && page.get_count > 0 => {
                let chars = text.chars().count();
                if chars > 20_000 {
                    return Err("invalid_result");
                }
                total_text += chars;
            }
            (Status::Ok, _) => return Err("invalid_result"),
            (_, None) if !page.text_truncated => (),
            _ => return Err("invalid_result"),
        }
    }
    if total_text > 30_000 {
        return Err("invalid_result");
    }
    Ok(())
}
