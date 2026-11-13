//! Cross-platform offline contract checks; no server bind or model execution.
use seiya_server::config::Config;
use std::process::Command;

#[test]
fn windows_discovery_example_has_no_runtime_credentials_or_remote_bind() {
    let config = Config::parse(include_bytes!(
        "../../../contracts/desktop/windows/server.discovery.example.json"
    ))
    .unwrap();
    assert!(config.listen.ip().is_loopback());
    assert!(config.runtime.is_none());
    assert!(config.api_key_env.is_none());
    assert!(!config.runtime_status);
}

#[test]
fn packaged_desktop_help_and_version_are_offline_on_every_target() {
    for flag in ["--help", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_seiya-desktop"))
            .arg(flag)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        if flag == "--help" {
            assert!(String::from_utf8(output.stdout)
                .unwrap()
                .contains("macOS-only"));
        }
    }
}
