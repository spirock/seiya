//! Explicit S4-08 live probe; never executed by make test. Three fixed public
//! URLs only, no model, browser, cookies, proxy, new listeners or service changes.
use seiya_server::reader::{extract, transport::HttpsNetwork, Error, Reader};
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::Instant;

const SOURCES: [(&str, &str); 3] = [
    ("example", "https://example.org/"),
    ("python", "https://docs.python.org/3/tutorial/index.html"),
    (
        "rust",
        "https://doc.rust-lang.org/book/ch01-01-installation.html",
    ),
];

fn error_name(error: Error) -> &'static str {
    match error {
        Error::Policy => "policy_blocked",
        Error::Timeout => "timeout",
        Error::Limit => "limit_exceeded",
        Error::Transport => "transport_error",
        Error::Format => "unsupported_format",
        Error::Http => "http_error",
        Error::Unavailable => "unavailable",
        Error::Cancelled => "cancelled",
        Error::NoText => "no_useful_text",
    }
}

fn evidence(results: Vec<Value>, reader: &Reader, elapsed: u128) -> Value {
    json!({"version":"reader-live-evidence/0.1.0", "mode":"real_public_https",
        "time_unix_seconds":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        "coordinator_version":env!("CARGO_PKG_VERSION"),"model_loaded":false,"llm_quality":null,
        "safari_used":false,"cookies_used":false,"service_modified":false,
        "body_saved":false,"external_proxy_used":false,"automatic_retries":false,
        "elapsed_ms":elapsed,"deadline_ms":45000,"reader_quarantined":reader.quarantined(),
        "cleanup":"TCP/TLS streams scoped and dropped; system DNS completion not independently proven",
        "counter_scope":"HTTP bytes delivered to parser, not ciphertext, kernel traffic or TLS readahead",
        "results":results})
}

#[tokio::test]
#[ignore = "REAL PUBLIC NETWORK: requires current approval for fixed sources, memory/process review, unique output path and SEIYA_ALLOW_READER_LIVE=1; no models"]
async fn authorized_three_public_sources_without_models() {
    assert_eq!(std::env::var("SEIYA_ALLOW_READER_LIVE").as_deref(), Ok("1"));
    let output = PathBuf::from(
        std::env::var_os("SEIYA_READER_LIVE_OUTPUT").expect("explicit unique evidence path"),
    );
    assert!(output.is_absolute());
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    // Reserve destination before any external effect. Interruption may leave
    // an incomplete marker or invalid JSON, never a fabricated success report.
    // Never overwrite an earlier run or log raw errors/body.
    let mut file = options
        .open(&output)
        .expect("new private evidence output required");
    file.write_all(b"{\"state\":\"incomplete\",\"body_saved\":false}\n")
        .unwrap();
    file.sync_all().unwrap();
    let reader = Reader::new(Arc::new(HttpsNetwork::default()));
    let started = Instant::now();
    let deadline = started + Duration::from_secs(45);
    let mut results = Vec::new();
    let mut chars_remaining = 30000usize;
    for (id, url) in SOURCES {
        if reader.quarantined() || Instant::now() >= deadline {
            results.push(
                json!({"id":id,"url":url,"status":"not_attempted_after_stop",
                "received_bytes":null,"decoded_bytes":null,"header_bytes":null,"get_count":0,
                "extracted_characters":null,"text_truncated":null}),
            );
            continue;
        }
        let page_started = Instant::now();
        let report = reader
            .fetch_report(url, deadline)
            .await
            .expect("single host-owned reader slot");
        let counters = report.counters;
        let (status, mime, chars, truncated) = match report.result {
            Ok(page) => {
                let mime = Some(page.mime.clone());
                match extract::extract(&page.body, &page.mime) {
                    Ok(text) if chars_remaining > 0 => {
                        let count = text.text.chars().count();
                        let supplied = count.min(chars_remaining);
                        chars_remaining -= supplied;
                        (
                            "ok",
                            mime,
                            Some(supplied),
                            Some(text.truncated || count > supplied),
                        )
                    }
                    Ok(_) => ("limit_exceeded", mime, None, None),
                    Err(error) => (error_name(error), mime, None, None),
                }
            }
            Err(error) => (error_name(error), None, None, None),
        };
        results.push(json!({"id":id,"url":url,"status":status,"mime":mime,
            "elapsed_ms":page_started.elapsed().as_millis(),"received_bytes":counters.received_bytes,
            "decoded_bytes":counters.decoded_bytes,"header_bytes":counters.header_bytes,
            "get_count":counters.get_count,"extracted_characters":chars,"text_truncated":truncated}));
    }
    let report = evidence(results, &reader, started.elapsed().as_millis());
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0)).unwrap();
    file.set_len(0).unwrap();
    serde_json::to_writer_pretty(&mut file, &report).unwrap();
    file.write_all(b"\n").unwrap();
    file.sync_all().unwrap();
    // The harness passing means evidence was recorded, NOT all pages succeeded.
    assert_eq!(report["results"].as_array().unwrap().len(), 3);
    println!(
        "fixed-source report written; inspect per-page status (test success is not page success)"
    );
}

#[test]
fn live_harness_manifest_has_exactly_the_operator_reviewable_scope() {
    assert_eq!(SOURCES.len(), 3);
    for (_, url) in SOURCES {
        assert!(seiya_server::reader::policy::url(url).is_ok());
    }
    assert_eq!(error_name(Error::Timeout), "timeout");
    let reader = Reader::new(Arc::new(HttpsNetwork::default())); // Construction only: no DNS/TCP.
    let report = evidence(vec![], &reader, 0);
    assert_eq!(report["body_saved"], false);
    assert!(report["llm_quality"].is_null());
}

#[test]
fn recorded_live_report_retains_unknown_quality_and_matches_fixed_scope() {
    let report: Value = serde_json::from_str(include_str!(
        "../../../contracts/benchmarks/web-reader/v0.1.0/public-read-2026-11-09.json"
    ))
    .unwrap();
    assert_eq!(report["mode"], "real_public_https");
    assert_eq!(report["model_loaded"], false);
    assert_eq!(report["body_saved"], false);
    assert!(report["llm_quality"].is_null());
    let rows = report["results"].as_array().unwrap();
    assert_eq!(rows.len(), SOURCES.len());
    let mut chars = 0;
    for (row, (id, url)) in rows.iter().zip(SOURCES) {
        assert_eq!(row["id"], id);
        assert_eq!(row["url"], url);
        assert_eq!(row["status"], "ok"); // Historical observation, not future site promise.
        assert!(row["received_bytes"].as_u64().unwrap() <= 1048576);
        assert!(row["decoded_bytes"].as_u64().unwrap() <= 1048576);
        assert!(row["header_bytes"].as_u64().unwrap() <= 3 * 65536);
        assert!(row["get_count"].as_u64().unwrap() <= 3);
        chars += row["extracted_characters"].as_u64().unwrap();
    }
    assert!(chars <= 30000);
}
