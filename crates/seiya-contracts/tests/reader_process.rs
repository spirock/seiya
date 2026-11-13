//! Integration with a fixed synthetic child, never an operational launcher.
use seiya_contracts::reader_frame::{parse_request, validate_response};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const REQUEST: &str = include_str!("../../../contracts/reader-ipc/v0.1-draft.1/request.json");

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        // This guard owns only the child created by this test.
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn start(mode: &str, request: &[u8]) -> OwnedChild {
    let mut child = OwnedChild(
        Command::new(env!("CARGO_BIN_EXE_reader-frame-fixture"))
            .arg(mode)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut stdin = child.0.stdin.take().unwrap();
    stdin.write_all(request).unwrap();
    drop(stdin);
    child
}

fn await_exit(child: &mut OwnedChild, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        match child.0.try_wait().unwrap() {
            Some(_) => return true,
            None if Instant::now() >= deadline => return false,
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

#[test]
fn synthetic_child_roundtrip_and_concatenated_response() {
    use std::io::Read;
    let request = parse_request(REQUEST.as_bytes(), true).unwrap();
    for (mode, valid) in [("ok", true), ("extra", false)] {
        let mut child = start(mode, REQUEST.as_bytes());
        assert!(await_exit(&mut child, Duration::from_secs(5)));
        let status = child.0.wait().unwrap();
        assert!(status.success());
        let mut response = Vec::new();
        child
            .0
            .stdout
            .take()
            .unwrap()
            .read_to_end(&mut response)
            .unwrap();
        assert_eq!(validate_response(&request, &response, true).is_ok(), valid);
    }
}

#[test]
fn synthetic_timeout_kills_and_reaps_only_owned_child() {
    let mut child = start("hold", REQUEST.as_bytes());
    assert!(!await_exit(&mut child, Duration::from_millis(100)));
    child.0.kill().unwrap();
    let status = child.0.wait().unwrap();
    assert!(!status.success());
    assert!(child.0.try_wait().unwrap().is_some());
}

#[test]
fn synthetic_child_rejects_invalid_request() {
    let mut child = start("ok", b"{}");
    assert!(await_exit(&mut child, Duration::from_secs(5)));
    assert!(!child.0.wait().unwrap().success());
}

#[test]
fn python_peer_and_rust_share_the_same_frames() {
    use std::io::Read;
    let peer = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/reader-ipc/v0.1-draft.1/python-peer.py");
    let mut child = OwnedChild(
        Command::new("python3")
            .args(["-I", "-S"])
            .arg(peer)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    child
        .0
        .stdin
        .take()
        .unwrap()
        .write_all(REQUEST.as_bytes())
        .unwrap();
    assert!(await_exit(&mut child, Duration::from_secs(5)));
    assert!(child.0.wait().unwrap().success());
    let mut output = Vec::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .take(128 * 1024 + 1)
        .read_to_end(&mut output)
        .unwrap();
    let request = parse_request(REQUEST.as_bytes(), true).unwrap();
    validate_response(&request, &output, true).unwrap();
}
