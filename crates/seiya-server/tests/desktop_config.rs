//! Synthetic local files only. No runtime, model store, network or host startup.
#![cfg(unix)]
use seiya_server::desktop_config::check_file;
use serde_json::{json, Value};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::{symlink, DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

struct Fixture {
    root: PathBuf,
    config: Value,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("seiya-desktop-config-{}", uuid::Uuid::new_v4()));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        for name in ["data", "coordination", "models", "assets"] {
            DirBuilder::new()
                .mode(0o700)
                .create(root.join(name))
                .unwrap();
        }
        for name in ["ollama", "sd-cli"] {
            // Executing this fixture would leave evidence. The checker must not.
            fs::write(
                root.join(name),
                format!("#!/bin/sh\ntouch '{}/EXECUTED'\nexit 99\n", root.display()),
            )
            .unwrap();
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let config = json!({
            "version":1,"target":"macos", "data_dir":root.join("data"),
            "coordination_dir":root.join("coordination"),"model_store":root.join("models"),
            "llm_binary":root.join("ollama"),
            "images":{"binary":root.join("sd-cli"),"assets_dir":root.join("assets")},
            "features":{"images":true,"multiagent":true}
        });
        Self { root, config }
    }

    fn save(&self) -> PathBuf {
        let path = self.root.join("config.json");
        fs::write(&path, serde_json::to_vec(&self.config).unwrap()).unwrap();
        path
    }

    fn rejects(&self, code: &str, field: &str) {
        let e = check_file(&self.save()).unwrap_err();
        assert_eq!((e.code, e.field), (code, field));
        assert!(!serde_json::to_string(&e)
            .unwrap()
            .contains(self.root.to_str().unwrap()));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(snapshot(&path));
        } else {
            files.push((path.clone(), fs::read(path).unwrap()));
        }
    }
    files.sort();
    files
}

#[test]
fn cli_checks_without_writes_execution_or_claiming_a_launcher() {
    let fixture = Fixture::new();
    let path = fixture.save();
    let before = snapshot(&fixture.root);
    let output = Command::new(env!("CARGO_BIN_EXE_seiya-desktop"))
        .arg("--check-config")
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["filesystem_checked"], true);
    for field in [
        "runtime_executed",
        "inference_validated",
        "launcher_available",
    ] {
        assert_eq!(report[field], false);
    }
    assert_eq!(report["target_matches_host"], cfg!(target_os = "macos"));
    assert!(output.stderr.is_empty());
    assert_eq!(snapshot(&fixture.root), before);
    assert!(!fixture.root.join("EXECUTED").exists());
}

#[test]
fn optional_images_and_features_default_to_off() {
    let mut fixture = Fixture::new();
    fixture.config.as_object_mut().unwrap().remove("images");
    fixture.config.as_object_mut().unwrap().remove("features");
    let report = check_file(&fixture.save()).unwrap();
    assert!(!report.requested_features.images);
    assert!(!report.requested_features.multiagent);
    fixture.config["features"] = json!({"images":true,"multiagent":false});
    fixture.rejects("image_adapter_required", "images");
}

#[test]
fn strict_schema_version_target_unknown_duplicate_and_trailing_input() {
    let mut fixture = Fixture::new();
    fixture.config["version"] = json!(2);
    fixture.rejects("unsupported_version", "version");
    fixture.config["version"] = json!(1);
    for (field, value) in [
        ("target", json!("linux")),
        (
            "features",
            json!({"images":false,"multiagent":false,"shell":true}),
        ),
    ] {
        let old = fixture.config[field].clone();
        fixture.config[field] = value;
        fixture.rejects("invalid_schema", "config");
        fixture.config[field] = old;
    }
    fixture.config["token"] = json!("not-a-credential");
    fixture.rejects("invalid_schema", "config");
    fixture.config.as_object_mut().unwrap().remove("token");
    let path = fixture.save();
    let raw = serde_json::to_string(&fixture.config).unwrap();
    for raw in [
        raw.replacen('{', "{\"version\":1,", 1),
        format!("{raw} {{}}"),
        "[]".into(),
    ] {
        fs::write(&path, raw).unwrap();
        assert_eq!(check_file(&path).unwrap_err().code, "invalid_schema");
    }
}

#[test]
fn bounds_config_input_and_rejects_symlink_or_directory() {
    let fixture = Fixture::new();
    let path = fixture.save();
    fs::write(&path, vec![b' '; 65537]).unwrap();
    assert_eq!(check_file(&path).unwrap_err().code, "too_large");
    let link = fixture.root.join("config-link");
    symlink(&path, &link).unwrap();
    assert_eq!(check_file(&link).unwrap_err().code, "regular_file_required");
    assert_eq!(
        check_file(&fixture.root).unwrap_err().code,
        "regular_file_required"
    );
}

#[test]
fn paths_must_exist_be_absolute_canonical_and_correct_kind() {
    let mut fixture = Fixture::new();
    for (path, code) in [
        (PathBuf::from("relative"), "absolute_path_required"),
        (fixture.root.join("absent"), "existing_path_required"),
        (fixture.root.join("data/../data"), "canonical_path_required"),
        (fixture.root.join("ollama"), "directory_required"),
    ] {
        fixture.config["data_dir"] = json!(path);
        fixture.rejects(code, "data_dir");
    }
    let link = fixture.root.join("linked-data");
    symlink(fixture.root.join("data"), &link).unwrap();
    fixture.config["data_dir"] = json!(link);
    fixture.rejects("canonical_path_required", "data_dir");
}

#[test]
fn rejects_nonprivate_state_and_nonexecutable_or_writable_binary() {
    let fixture = Fixture::new();
    fs::set_permissions(fixture.root.join("data"), fs::Permissions::from_mode(0o755)).unwrap();
    fixture.rejects("private_directory_required", "data_dir");
    fs::set_permissions(fixture.root.join("data"), fs::Permissions::from_mode(0o700)).unwrap();
    for mode in [0o600, 0o777, 0o720] {
        fs::set_permissions(
            fixture.root.join("ollama"),
            fs::Permissions::from_mode(mode),
        )
        .unwrap();
        fixture.rejects("executable_permissions_required", "llm_binary");
    }
}

#[test]
fn operational_paths_cannot_overlap_each_other_or_dependencies() {
    let mut fixture = Fixture::new();
    fixture.config["coordination_dir"] = fixture.config["data_dir"].clone();
    fixture.rejects("overlapping_paths", "coordination_dir");
    fixture.config["coordination_dir"] = json!(fixture.root.join("coordination"));
    for name in ["models", "assets"] {
        fixture.config["data_dir"] = json!(fixture.root.join(name));
        fixture.rejects("overlapping_paths", "data_dir");
    }
    fixture.config["data_dir"] = json!(fixture.root.join("data"));
    fixture.config["coordination_dir"] = json!(fixture.root.join("models"));
    fixture.rejects("overlapping_paths", "coordination_dir");
}

#[test]
fn cli_failures_are_nonzero_and_path_free() {
    let mut fixture = Fixture::new();
    fixture.config["version"] = json!(999);
    let output = Command::new(env!("CARGO_BIN_EXE_seiya-desktop"))
        .args(["--check-config"])
        .arg(fixture.save())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(
        error,
        json!({"code":"unsupported_version","field":"version"})
    );
    let output = Command::new(env!("CARGO_BIN_EXE_seiya-desktop"))
        .arg("--serve")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn serve_requires_approval_and_rejects_invalid_config_before_side_effects() {
    let mut fixture = Fixture::new();
    fixture.config["version"] = json!(999);
    let path = fixture.save();
    let before = snapshot(&fixture.root);
    for (approval, expected) in [
        ("not-approved", 2),
        ("manual-portable-macos-web-no-automatic-inference", 1),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_seiya-desktop"))
            .arg("--serve")
            .arg("--config")
            .arg(&path)
            .arg("--approval")
            .arg(approval)
            .args([
                "--listen",
                "192.168.50.20:8015",
                "--prefix-length",
                "24",
                "--certificate-der",
                "not-read",
                "--private-key-der",
                "not-read",
                "--chat-only",
            ])
            .env("HOME", &fixture.root)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(expected));
        assert!(output.stdout.is_empty());
    }
    assert_eq!(snapshot(&fixture.root), before);
}

#[test]
fn installer_help_and_version_never_start_a_host() {
    for arg in ["--help", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_seiya-desktop"))
            .arg(arg)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.is_empty());
    }
}
