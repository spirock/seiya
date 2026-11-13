use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

pub const MANIFEST: &str = include_str!(
    "../../../../contracts/benchmarks/image-candidates/v0.1.0/qwen-image21-turbo-q4.json"
);
pub const CHUNK: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Asset {
    pub repository: String,
    pub revision: String,
    pub file: String,
    pub local_name: String,
    pub bytes: u64,
    pub sha256: String,
    pub license: String,
}

#[derive(Debug, Deserialize)]
pub struct Selection {
    pub version: u32,
    pub total_asset_bytes: u64,
    pub download_reserve_bytes: u64,
    pub assets: Vec<Asset>,
}

fn hex(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Selection {
    pub fn selected() -> Result<Self, &'static str> {
        let s: Self = serde_json::from_str(MANIFEST).map_err(|_| "invalid_selection")?;
        s.validate()?;
        Ok(s)
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        let names = [
            "diffusion-q4.gguf",
            "text-encoder-q4.gguf",
            "vae-bf16.safetensors",
        ];
        if self.version != 1
            || self.assets.len() != 3
            || self.total_asset_bytes != 10_023_773_264
            || self.download_reserve_bytes != 5 * 1024u64.pow(3)
        {
            return Err("invalid_selection_limits");
        }
        let mut total = 0u64;
        for (a, name) in self.assets.iter().zip(names) {
            if a.local_name != name
                || !a.repository.starts_with("unsloth/")
                || !a
                    .repository
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b))
                || a.repository.split('/').count() != 2
                || !hex(&a.revision, 40)
                || !hex(&a.sha256, 64)
                || a.file.starts_with('/')
                || a.file
                    .split('/')
                    .any(|p| p.is_empty() || p == ".." || p == ".")
                || !a
                    .file
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b))
                || a.bytes == 0
                || a.bytes > 6 * 1024u64.pow(3)
            {
                return Err("invalid_selected_asset");
            }
            total = total.checked_add(a.bytes).ok_or("asset_total_overflow")?;
        }
        if total != self.total_asset_bytes {
            return Err("asset_total_mismatch");
        }
        Ok(())
    }
}

pub fn hash_reader(mut reader: impl Read, cap: u64) -> Result<(u64, String), String> {
    let mut hasher = Sha256::new();
    let mut count = 0u64;
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let n = reader.read(&mut buffer).map_err(|_| "asset_read_error")?;
        if n == 0 {
            break;
        }
        count = count.checked_add(n as u64).ok_or("asset_size_overflow")?;
        if count > cap {
            return Err("asset_size_limit".into());
        }
        hasher.update(&buffer[..n]);
    }
    Ok((count, format!("{:x}", hasher.finalize())))
}

pub fn verify(path: &Path, a: &Asset) -> Result<(), String> {
    use std::fs;
    let m = fs::symlink_metadata(path).map_err(|_| "asset_metadata")?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() != a.bytes {
        return Err("asset_size_or_type".into());
    }
    let (size, hash) = hash_reader(File::open(path).map_err(|_| "asset_open")?, a.bytes)?;
    if size != a.bytes || hash != a.sha256 {
        return Err("asset_hash_mismatch".into());
    }
    Ok(())
}

pub fn allowed_url(url: &reqwest::Url) -> bool {
    let h = url.host_str().unwrap_or("");
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url.fragment().is_none()
        && (h == "huggingface.co" || h.ends_with(".huggingface.co") || h.ends_with(".hf.co"))
}

impl Asset {
    pub fn url(&self) -> Result<reqwest::Url, &'static str> {
        reqwest::Url::parse(&format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repository, self.revision, self.file
        ))
        .map_err(|_| "invalid_asset_url")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_is_pinned_and_not_an_available_tool() {
        let mut s = Selection::selected().unwrap();
        assert_eq!(s.assets.len(), 3);
        assert!(s.assets[0]
            .url()
            .unwrap()
            .as_str()
            .contains(&s.assets[0].revision));
        s.assets[0].local_name = "../escape".into();
        assert!(s.validate().is_err());
        let m: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(m["available_in_seiya"], false);
    }
    #[test]
    fn hash_is_bounded_and_checks_actual_content() {
        assert_eq!(
            hash_reader(&b"abc"[..], 3).unwrap(),
            (
                3,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into()
            )
        );
        assert!(hash_reader(&b"abcd"[..], 3).is_err());
    }
    #[test]
    fn redirect_allowlist_does_not_allow_credentials_local_or_plain_http() {
        for u in [
            "https://huggingface.co/a",
            "https://cas-bridge.xethub.hf.co/a",
        ] {
            assert!(allowed_url(&reqwest::Url::parse(u).unwrap()));
        }
        for u in [
            "http://huggingface.co/a",
            "https://huggingface.co.evil.example/a",
            "https://127.0.0.1/a",
            "https://u:p@huggingface.co/a",
        ] {
            assert!(!allowed_url(&reqwest::Url::parse(u).unwrap()));
        }
    }
}
