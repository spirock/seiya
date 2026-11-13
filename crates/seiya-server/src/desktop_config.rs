//! Offline filesystem checks only. This module never starts a runtime or host.
use crate::diagnostics::Features;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

const MAX_CONFIG_BYTES: u64 = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    version: u32,
    target: Target,
    pub(crate) data_dir: PathBuf,
    pub(crate) coordination_dir: PathBuf,
    pub(crate) model_store: PathBuf,
    pub(crate) llm_binary: PathBuf,
    pub(crate) images: Option<Images>,
    #[serde(default)]
    pub(crate) features: Features,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Target {
    Macos,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Images {
    pub(crate) binary: PathBuf,
    pub(crate) assets_dir: PathBuf,
}

/// Stable path-free diagnostics: do not copy private config or paths into logs.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct CheckError {
    pub code: &'static str,
    pub field: &'static str,
}

fn error(code: &'static str, field: &'static str) -> CheckError {
    CheckError { code, field }
}

#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub version: u32,
    pub target: &'static str,
    pub target_matches_host: bool,
    pub filesystem_checked: bool,
    pub requested_features: Features,
    pub runtime_executed: bool,
    pub inference_validated: bool,
    pub launcher_available: bool,
}

/// Reads a bounded regular JSON file. No writes, commands or network access.
pub fn check_file(path: &Path) -> Result<CheckReport, CheckError> {
    let config = load_file(path)?;
    let Target::Macos = config.target;
    Ok(CheckReport {
        version: 1,
        target: "macos",
        target_matches_host: cfg!(target_os = "macos"),
        filesystem_checked: true,
        requested_features: config.features,
        runtime_executed: false,
        inference_validated: false,
        launcher_available: false,
    })
}
pub(crate) fn load_file(path: &Path) -> Result<Config, CheckError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| error("unreadable", "config"))?;
    if !metadata.is_file() {
        return Err(error("regular_file_required", "config"));
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(error("too_large", "config"));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| error("unreadable", "config"))?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error("unreadable", "config"))?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(error("too_large", "config"));
    }
    check_bytes(&bytes)
}

pub(crate) fn check_bytes(bytes: &[u8]) -> Result<Config, CheckError> {
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(error("too_large", "config"));
    }
    let config: Config =
        serde_json::from_slice(bytes).map_err(|_| error("invalid_schema", "config"))?;
    if config.version != 1 {
        return Err(error("unsupported_version", "version"));
    }
    if config.features.images && config.images.is_none() {
        return Err(error("image_adapter_required", "images"));
    }
    directory(&config.data_dir, "data_dir", true)?;
    directory(&config.coordination_dir, "coordination_dir", true)?;
    directory(&config.model_store, "model_store", false)?;
    executable(&config.llm_binary, "llm_binary")?;
    let mut dependencies = vec![&config.model_store, &config.llm_binary];
    if let Some(images) = &config.images {
        executable(&images.binary, "images.binary")?;
        directory(&images.assets_dir, "images.assets_dir", false)?;
        dependencies.extend([&images.binary, &images.assets_dir]);
    }
    if overlap(&config.data_dir, &config.coordination_dir) {
        return Err(error("overlapping_paths", "coordination_dir"));
    }
    for (field, operational) in [
        ("data_dir", &config.data_dir),
        ("coordination_dir", &config.coordination_dir),
    ] {
        if dependencies.iter().any(|path| overlap(operational, path)) {
            return Err(error("overlapping_paths", field));
        }
    }
    Ok(config)
}

fn overlap(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

fn metadata(path: &Path, field: &'static str) -> Result<fs::Metadata, CheckError> {
    if !path.is_absolute() {
        return Err(error("absolute_path_required", field));
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| error("existing_path_required", field))?;
    if canonical != path {
        return Err(error("canonical_path_required", field));
    }
    fs::symlink_metadata(path).map_err(|_| error("unreadable", field))
}

#[cfg(unix)]
fn mode(metadata: &fs::Metadata, _: &'static str) -> Result<u32, CheckError> {
    use std::os::unix::fs::PermissionsExt;
    Ok(metadata.permissions().mode())
}

#[cfg(not(unix))]
fn mode(_: &fs::Metadata, field: &'static str) -> Result<u32, CheckError> {
    Err(error("unix_permissions_required", field))
}

fn directory(path: &Path, field: &'static str, private: bool) -> Result<(), CheckError> {
    let metadata = metadata(path, field)?;
    if !metadata.is_dir() {
        return Err(error("directory_required", field));
    }
    if private && mode(&metadata, field)? & 0o077 != 0 {
        return Err(error("private_directory_required", field));
    }
    Ok(())
}

pub(crate) fn executable(path: &Path, field: &'static str) -> Result<(), CheckError> {
    let metadata = metadata(path, field)?;
    if !metadata.is_file() {
        return Err(error("regular_file_required", field));
    }
    let mode = mode(&metadata, field)?;
    if mode & 0o111 == 0 || mode & 0o022 != 0 {
        return Err(error("executable_permissions_required", field));
    }
    Ok(())
}
