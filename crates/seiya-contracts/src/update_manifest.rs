//! Bounded, offline stable-release metadata validation.
use crate::update_version::{newer_stable, StableVersion};
use serde::Deserialize;

const MAX_BYTES: usize = 4096;
const RELEASE_PREFIX: &str = "https://github.com/spirock/seiya/releases/tag/v";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StableManifest {
    schema: u8,
    version: String,
    revision: String,
    release_url: String,
}

impl StableManifest {
    /// Parse metadata as untrusted data. This does not verify a GitHub release.
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > MAX_BYTES {
            return Err("update_manifest_too_large");
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| "invalid_update_manifest")?;
        if value.schema != 1
            || StableVersion::parse(&value.version).is_err()
            || value.revision.len() != 40
            || !value
                .revision
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || value.release_url != format!("{RELEASE_PREFIX}{}", value.version)
        {
            return Err("invalid_update_manifest");
        }
        Ok(value)
    }

    pub fn is_newer_than(&self, current: &str) -> Result<bool, &'static str> {
        newer_stable(current, &self.version)
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn release_url(&self) -> &str {
        &self.release_url
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> serde_json::Value {
        json!({
            "schema": 1,
            "version": "1.2.3",
            "revision": "0123456789abcdef0123456789abcdef01234567",
            "release_url": "https://github.com/spirock/seiya/releases/tag/v1.2.3"
        })
    }

    #[test]
    fn valid_manifest_only_suggests_later_stable_versions() {
        let data = serde_json::to_vec(&fixture()).unwrap();
        let manifest = StableManifest::parse(&data).unwrap();
        let example = include_bytes!("../../../contracts/updates/v1/stable.example.json");
        assert_eq!(StableManifest::parse(example).unwrap().version(), "1.2.3");
        assert_eq!(manifest.version(), "1.2.3");
        assert_eq!(manifest.is_newer_than("1.2.2"), Ok(true));
        assert_eq!(manifest.is_newer_than("1.2.3"), Ok(false));
        assert_eq!(manifest.is_newer_than("1.2.4"), Ok(false));
        assert!(manifest.is_newer_than("1.2.3-rc.1").is_err());
    }

    #[test]
    fn rejects_untrusted_urls_schema_and_invalid_versions() {
        for (key, value) in [
            ("schema", json!(2)),
            ("version", json!("1.2.3-rc.1")),
            (
                "revision",
                json!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            ),
            ("release_url", json!("https://evil.invalid/")),
            (
                "release_url",
                json!("https://github.com.evil.invalid/spirock/seiya/releases/tag/v1.2.3"),
            ),
            (
                "release_url",
                json!("https://github.com/spirock/seiya/releases/tag/v1.2.3?x=1"),
            ),
            ("unexpected", json!(true)),
        ] {
            let mut data = fixture();
            data[key] = value;
            assert!(StableManifest::parse(&serde_json::to_vec(&data).unwrap()).is_err());
        }
        assert!(StableManifest::parse(&vec![b' '; MAX_BYTES + 1]).is_err());
        assert!(StableManifest::parse(br#"{"schema":1,"schema":1}"#).is_err());
    }
}
