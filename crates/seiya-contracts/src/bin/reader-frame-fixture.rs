//! Synthetic protocol peer for tests only; never opens network or model files.
use seiya_contracts::reader_frame::{read_request, validate_response};
use std::io::{self, Write};

const RESPONSE: &str = include_str!("../../../../contracts/reader-ipc/v0.1-draft.1/response.json");

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let request = match read_request(io::stdin().lock()) {
        Ok(value) => value,
        Err(_) => std::process::exit(2),
    };
    if !request.matches_host_results("run-1", &[("source-1", "https://example.org/article")])
        || validate_response(&request, RESPONSE.as_bytes(), true).is_err()
    {
        std::process::exit(2);
    }
    match mode.as_str() {
        "ok" => io::stdout().write_all(RESPONSE.as_bytes()).unwrap(),
        "extra" => io::stdout()
            .write_all(format!("{RESPONSE}{{}}").as_bytes())
            .unwrap(),
        "hold" => std::thread::sleep(std::time::Duration::from_secs(30)),
        _ => std::process::exit(2),
    }
}
