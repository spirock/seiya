use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct TestConfig(PathBuf);

impl TestConfig {
    fn new(contents: &[u8]) -> Self {
        let directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/seiya-server-tests");
        std::fs::create_dir_all(&directory).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = directory.join(format!(
            "config-{}-{nonce}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(contents).unwrap();
        Self(path)
    }
}

impl Drop for TestConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_seiya-server"))
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn help_version_and_explicit_config_are_required() {
    for arg in ["--help", "--version"] {
        let output = command().arg(arg).output().unwrap();
        assert!(output.status.success());
        assert!(text(&output).contains("experimental"));
    }
    for args in [
        vec![],
        vec!["serve"],
        vec!["check-config"],
        vec!["serve", "--api-key", "synthetic-secret"],
    ] {
        let output = command().args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(!text(&output).contains("synthetic-secret"));
    }
}

#[test]
fn check_config_does_not_bind_and_never_echoes_secrets() {
    let config =
        TestConfig::new(br#"{"listen":"127.0.0.1:0","api_key_env":"SEIYA_SYNTHETIC_TEST_KEY"}"#);
    let output = command()
        .args(["check-config", "--config"])
        .arg(&config.0)
        .env("SEIYA_SYNTHETIC_TEST_KEY", "synthetic-secret-value")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("No sockets opened; no models loaded"));
    assert!(!text(&output).contains("synthetic-secret-value"));
    let output = command()
        .args(["serve", "--config"])
        .arg(&config.0)
        .env_remove("SEIYA_SYNTHETIC_TEST_KEY")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(text(&output).contains("environment variable is unavailable"));
}

#[test]
fn invalid_file_and_unknown_fields_are_redacted() {
    let config = TestConfig::new(br#"{"listen":"127.0.0.1:0","api_key":"synthetic-secret-value"}"#);
    let output = command()
        .args(["check-config", "--config"])
        .arg(&config.0)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!text(&output).contains("synthetic-secret-value"));
    let output = command()
        .args(["check-config", "--config"])
        .arg(config.0.with_extension("does-not-exist"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(text(&output).contains("cannot open configuration file"));
    assert!(!text(&output).contains("does-not-exist"));
}
