use seiya_contracts::reader_frame::{
    parse_request, read_request, read_response, validate_response,
};
use std::io::{self, Read};

const REQUEST: &str = include_str!("../../../contracts/reader-ipc/v0.1-draft.1/request.json");
const RESPONSE: &str = include_str!("../../../contracts/reader-ipc/v0.1-draft.1/response.json");

#[test]
fn shared_adversarial_corpus_is_rejected() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../contracts/reader-ipc/v0.1-draft.1/invalid-frames.json"
    ))
    .unwrap();
    let request = parse_request(REQUEST.as_bytes(), true).unwrap();
    for case in cases.as_array().unwrap() {
        let frame = case["frame"].as_str().unwrap().as_bytes();
        match case["kind"].as_str().unwrap() {
            "request" => assert!(parse_request(frame, true).is_err()),
            "response" => assert!(validate_response(&request, frame, true).is_err()),
            _ => panic!("unknown fixture kind"),
        }
    }
}

#[test]
fn fixtures_are_correlated_only_after_eof() {
    let request = parse_request(REQUEST.as_bytes(), true).unwrap();
    assert!(validate_response(&request, RESPONSE.as_bytes(), true).is_ok());
    assert!(parse_request(REQUEST.as_bytes(), false).is_err());
    assert!(validate_response(&request, RESPONSE.as_bytes(), false).is_err());
    let binding = [("source-1", "https://example.org/article")];
    assert!(request.matches_host_results("run-1", &binding));
    assert!(!request.matches_host_results("other", &binding));
    assert!(!request.matches_host_results("run-1", &[("source-1", "https://example.org/other")]));
    assert!(!request.matches_host_results("run-1", &[binding[0], binding[0]]));
}

#[test]
fn malformed_duplicates_versions_and_sizes_fail_closed() {
    let request = parse_request(REQUEST.as_bytes(), true).unwrap();
    for input in [
        REQUEST.replace("0.1-draft.1", "0.2"),
        REQUEST.replace("\"run_id\":", "\"run_id\":\"other\",\"run_id\":"),
        format!("{REQUEST}{{}}"),
        REQUEST.replace("12000", "45001"),
        REQUEST.replace("source-1", "source 1"),
        " ".repeat(16 * 1024 + 1),
    ] {
        assert!(parse_request(input.as_bytes(), true).is_err());
    }
    assert!(parse_request(&[0xff], true).is_err());
    for input in [
        RESPONSE.replace("run-1", "other"),
        RESPONSE.replace("source-1", "other"),
        RESPONSE.replace("\"ok\"", "\"timeout\""),
        RESPONSE.replace("\"text\":\"Contenido sintético.\",", ""),
        RESPONSE.replace("\"get_count\":1", "\"get_count\":4"),
        RESPONSE.replace("\"header_bytes\":64", "\"header_bytes\":65537"),
        RESPONSE.replace("\"received_bytes\":128", "\"received_bytes\":1048577"),
        format!("{RESPONSE}{{}}"),
        " ".repeat(128 * 1024 + 1),
    ] {
        assert!(
            validate_response(&request, input.as_bytes(), true).is_err(),
            "{input}"
        );
    }
}

#[test]
fn error_results_cannot_smuggle_text_or_claim_truncation() {
    let request = parse_request(REQUEST.as_bytes(), true).unwrap();
    let error = RESPONSE
        .replace("\"ok\"", "\"timeout\"")
        .replace("\"Contenido sintético.\"", "null");
    assert!(validate_response(&request, error.as_bytes(), true).is_ok());
    assert!(validate_response(&request, error.replace("false", "true").as_bytes(), true).is_err());
    let no_get_with_bytes = error.replace("\"get_count\":1", "\"get_count\":0");
    assert!(validate_response(&request, no_get_with_bytes.as_bytes(), true).is_err());
}

#[test]
fn bounded_reader_consumes_eof_and_propagates_io_error() {
    let request = read_request(REQUEST.as_bytes()).unwrap();
    assert!(read_response(&request, RESPONSE.as_bytes()).is_ok());
    assert!(read_request(" ".repeat(16 * 1024 + 1).as_bytes()).is_err());
    struct Interrupted;
    impl Read for Interrupted {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("synthetic"))
        }
    }
    assert!(read_request(Interrupted).is_err());
}

#[test]
fn fragmented_frames_consume_at_most_cap_plus_one() {
    struct Chunked<'a> {
        data: &'a [u8],
        consumed: usize,
    }
    impl Read for Chunked<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let n = self.data.len().min(output.len()).min(3);
            output[..n].copy_from_slice(&self.data[..n]);
            self.data = &self.data[n..];
            self.consumed += n;
            Ok(n)
        }
    }
    let mut reader = Chunked {
        data: REQUEST.as_bytes(),
        consumed: 0,
    };
    assert!(read_request(&mut reader).is_ok());
    assert_eq!(reader.consumed, REQUEST.len());
    let overflow = vec![b' '; 32 * 1024];
    let mut reader = Chunked {
        data: &overflow,
        consumed: 0,
    };
    assert!(read_request(&mut reader).is_err());
    assert_eq!(reader.consumed, 16 * 1024 + 1);
    let truncated = &REQUEST.as_bytes()[..REQUEST.len() / 2];
    assert!(read_request(Chunked {
        data: truncated,
        consumed: 0
    })
    .is_err());
}

#[test]
fn fixtures_and_schema_documents_are_bundled_with_matching_version() {
    for schema in [
        include_str!("../../../contracts/reader-ipc/v0.1-draft.1/request.schema.json"),
        include_str!("../../../contracts/reader-ipc/v0.1-draft.1/response.schema.json"),
    ] {
        let value: serde_json::Value = serde_json::from_str(schema).unwrap();
        assert_eq!(value["properties"]["version"]["const"], "0.1-draft.1");
        assert_eq!(value["additionalProperties"], false);
    }
}

#[cfg(unix)]
#[test]
fn synthetic_local_stream_frames_require_write_shutdown() {
    use std::io::Write;
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let (mut writer, reader) = UnixStream::pair().unwrap();
    // The framing primitive requires a deadline-controlled stream. A stalled
    // synthetic socket must fail this test, not hang the entire regression.
    writer
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    reader
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let worker = std::thread::spawn(move || read_request(reader).is_ok());
    for chunk in REQUEST.as_bytes().chunks(7) {
        writer.write_all(chunk).unwrap();
    }
    writer.shutdown(Shutdown::Write).unwrap();
    assert!(worker.join().unwrap());

    let request = parse_request(REQUEST.as_bytes(), true).unwrap();
    let (mut writer, reader) = UnixStream::pair().unwrap();
    writer
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    reader
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let worker = std::thread::spawn(move || read_response(&request, reader).is_ok());
    writer.write_all(RESPONSE.as_bytes()).unwrap();
    writer.write_all(b"{}").unwrap();
    writer.shutdown(Shutdown::Write).unwrap();
    assert!(!worker.join().unwrap());
}
