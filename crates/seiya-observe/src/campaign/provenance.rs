use super::PreparationError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const MAX_FILES: usize = 256;
const MAX_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_METADATA_TOTAL_BYTES: u64 = 48 * 1024 * 1024;

pub(super) fn metadata_cap(name: &str) -> usize {
    match name {
        "tokenizer.json" | "tokenizer.model" => 32 * 1024 * 1024,
        _ => MAX_METADATA_BYTES,
    }
}

fn validate_tokenizer_json(bytes: &[u8]) -> Result<(), PreparationError> {
    // IgnoredAny skips string materialization; explicitly reject invalid UTF-8
    // rather than relying on the skipped-string path to validate its encoding.
    let text = std::str::from_utf8(bytes).map_err(|_| PreparationError::InvalidProvenance)?;
    if bytes.iter().copied().find(|b| !b.is_ascii_whitespace()) != Some(b'{') {
        return Err(PreparationError::InvalidProvenance);
    }
    let mut parser = serde_json::Deserializer::from_str(text);
    serde::de::IgnoredAny::deserialize(&mut parser)
        .map_err(|_| PreparationError::InvalidProvenance)?;
    parser
        .end()
        .map_err(|_| PreparationError::InvalidProvenance)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Config,
    Tokenizer,
    Template,
    WeightIndex,
    WeightShard,
    Auxiliary,
}

#[derive(Debug, Clone, Serialize)]
pub struct Asset {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    pub kind: AssetKind,
}

/// Injection seam, not permission to scan a host. A future native provider must
/// bind all operations to one approved directory and verify identity around IO.
pub trait ProvenanceSource {
    /// Flat selected inference assets only; reject symlinks/directories in provider.
    fn files(&mut self) -> Result<Vec<(String, u64)>, PreparationError>;
    fn sha256(&mut self, name: &str) -> Result<String, PreparationError>;
    fn read_metadata(&mut self, name: &str, cap: usize) -> Result<Vec<u8>, PreparationError>;
    /// Read installed metadata without importing the model/runtime or network.
    fn versions(&mut self) -> Result<BTreeMap<String, String>, PreparationError>;
}

#[derive(Debug, Serialize)]
pub struct Provenance {
    pub assets: Vec<Asset>,
    pub versions: BTreeMap<String, String>,
    pub total_asset_bytes: u64,
    /// Do not confuse structurally checked injected evidence with native hashing.
    pub verification: &'static str,
}

pub(super) fn kind(name: &str) -> Option<AssetKind> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || name.starts_with('.')
        || name.contains("..")
    {
        return None;
    }
    Some(match name {
        "config.json" => AssetKind::Config,
        "tokenizer.json"
        | "tokenizer.model"
        | "tokenizer_config.json"
        | "special_tokens_map.json" => AssetKind::Tokenizer,
        "chat_template.jinja" => AssetKind::Template,
        "model.safetensors.index.json" => AssetKind::WeightIndex,
        name if name.ends_with(".safetensors") => AssetKind::WeightShard,
        name if name.ends_with(".json") => AssetKind::Auxiliary,
        _ => return None,
    })
}

pub(super) fn digest_valid(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl Provenance {
    pub fn collect(source: &mut impl ProvenanceSource) -> Result<Self, PreparationError> {
        let mut files = source.files()?;
        if files.is_empty() || files.len() > MAX_FILES {
            return Err(PreparationError::Limit);
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        let mut names = BTreeSet::new();
        let mut total = 0u64;
        let mut metadata_total = 0u64;
        // Validate the entire inventory before reading or hashing any asset.
        for (name, size) in &files {
            let kind = kind(name).ok_or(PreparationError::InvalidProvenance)?;
            if !names.insert(name.clone()) || *size == 0 {
                return Err(PreparationError::InvalidProvenance);
            }
            total = total
                .checked_add(*size)
                .filter(|n| *n <= MAX_BYTES)
                .ok_or(PreparationError::Limit)?;
            if kind != AssetKind::WeightShard && *size > metadata_cap(name) as u64 {
                return Err(PreparationError::Limit);
            }
            if kind != AssetKind::WeightShard {
                metadata_total = metadata_total
                    .checked_add(*size)
                    .filter(|n| *n <= MAX_METADATA_TOTAL_BYTES)
                    .ok_or(PreparationError::Limit)?;
            }
        }
        if !names.contains("config.json")
            || !names.contains("tokenizer_config.json")
            || !(names.contains("tokenizer.json") || names.contains("tokenizer.model"))
        {
            return Err(PreparationError::InvalidProvenance);
        }
        let versions = source.versions()?;
        if versions.len() != 4
            || ["python", "mlx", "mlx-lm", "transformers"]
                .iter()
                .any(|key| {
                    versions.get(*key).is_none_or(|v| {
                        v.is_empty() || v.len() > 128 || !v.bytes().all(|b| b.is_ascii_graphic())
                    })
                })
        {
            return Err(PreparationError::InvalidProvenance);
        }
        let mut assets = Vec::new();
        let mut metadata = BTreeMap::new();
        for (name, size) in files {
            let kind = kind(&name).ok_or(PreparationError::InvalidProvenance)?;
            let sha256 = source.sha256(&name)?;
            if !digest_valid(&sha256) {
                return Err(PreparationError::InvalidProvenance);
            }
            if kind != AssetKind::WeightShard {
                let cap = metadata_cap(&name);
                let bytes = source.read_metadata(&name, cap)?;
                if bytes.len() != size as usize || bytes.len() > cap {
                    return Err(PreparationError::InvalidProvenance);
                }
                if kind == AssetKind::Template
                    && std::str::from_utf8(&bytes)
                        .ok()
                        .is_none_or(|text| text.trim().is_empty())
                {
                    return Err(PreparationError::InvalidProvenance);
                }
                if name == "tokenizer.json" {
                    validate_tokenizer_json(&bytes)?;
                } else if name.ends_with(".json") {
                    let value: Value = serde_json::from_slice(&bytes)
                        .map_err(|_| PreparationError::InvalidProvenance)?;
                    if !value.is_object() {
                        return Err(PreparationError::InvalidProvenance);
                    }
                    metadata.insert(name.clone(), value);
                }
            }
            assets.push(Asset {
                name,
                bytes: size,
                sha256,
                kind,
            });
        }
        if metadata["config.json"].get("auto_map").is_some()
            || metadata["tokenizer_config.json"].get("auto_map").is_some()
            || metadata["config.json"]["model_type"]
                .as_str()
                .is_none_or(|s| s.is_empty() || s.len() > 128)
        {
            return Err(PreparationError::InvalidProvenance);
        }
        let tokenizer = &metadata["tokenizer_config.json"];
        let embedded_template = tokenizer.get("chat_template").is_some_and(|v| {
            v.as_str().is_some_and(|s| !s.trim().is_empty())
                || v.as_array().is_some_and(|items| {
                    let mut seen = BTreeSet::new();
                    !items.is_empty()
                        && items.len() <= 16
                        && items.iter().all(|item| {
                            item["name"]
                                .as_str()
                                .is_some_and(|name| !name.is_empty() && seen.insert(name))
                                && item["template"]
                                    .as_str()
                                    .is_some_and(|template| !template.trim().is_empty())
                        })
                })
        });
        if !embedded_template && !names.contains("chat_template.jinja") {
            return Err(PreparationError::InvalidProvenance);
        }
        let shards: BTreeSet<_> = assets
            .iter()
            .filter(|a| a.kind == AssetKind::WeightShard)
            .map(|a| a.name.clone())
            .collect();
        if shards.is_empty() {
            return Err(PreparationError::InvalidProvenance);
        }
        if let Some(index) = metadata.get("model.safetensors.index.json") {
            let map = index["weight_map"]
                .as_object()
                .filter(|map| !map.is_empty() && map.len() <= 100_000)
                .ok_or(PreparationError::InvalidProvenance)?;
            let mut referenced = BTreeSet::new();
            for value in map.values() {
                let name = value.as_str().ok_or(PreparationError::InvalidProvenance)?;
                if !shards.contains(name) {
                    return Err(PreparationError::InvalidProvenance);
                }
                referenced.insert(name.to_owned());
            }
            if referenced != shards {
                return Err(PreparationError::InvalidProvenance);
            }
        } else if shards != BTreeSet::from(["model.safetensors".to_owned()]) {
            return Err(PreparationError::InvalidProvenance);
        }
        Ok(Self {
            assets,
            versions,
            total_asset_bytes: total,
            verification: "injected-source-structural-validation-only",
        })
    }
}
