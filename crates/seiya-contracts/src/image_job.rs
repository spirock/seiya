//! Experimental image boundaries, not a registered image tool or executor.
use serde::{Deserialize, Serialize};

pub const MAX_PNG_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImageRequest {
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    pub seed: u32,
}

/// Observations from the host, never model-authored percentages or completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageStage {
    Preparing,
    Running,
    Validating,
}

impl ImageRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.prompt.trim().is_empty()
            || self.prompt.len() > 2048
            || self.prompt.contains('\0')
            || ![self.width, self.height]
                .iter()
                .all(|n| (256..=1024).contains(n) && n % 32 == 0)
        {
            return Err("invalid_image_request");
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct PngContainer {
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub inflated_bytes: usize,
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

/// Container CRC/header bounds only. The host MUST also decode/validate the
/// bounded zlib pixel stream and establish ownership before claiming an image.
pub fn inspect_png(bytes: &[u8]) -> Result<PngContainer, &'static str> {
    if bytes.len() > MAX_PNG_BYTES || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("invalid_png_signature_or_size");
    }
    let bad = "invalid_png_container";
    let mut offset = 8usize;
    let mut header = None;
    let mut compressed = 0usize;
    let mut idat_ended = false;
    for index in 0..1024 {
        let prefix = bytes.get(offset..offset + 8).ok_or(bad)?;
        let size = u32::from_be_bytes(prefix[..4].try_into().map_err(|_| bad)?) as usize;
        let kind = &prefix[4..8];
        let end = offset
            .checked_add(size)
            .and_then(|n| n.checked_add(12))
            .ok_or(bad)?;
        let chunk = bytes.get(offset..end).ok_or(bad)?;
        let crc = u32::from_be_bytes(chunk[chunk.len() - 4..].try_into().map_err(|_| bad)?);
        if crc32(&chunk[4..chunk.len() - 4]) != crc || !kind.iter().all(u8::is_ascii_alphabetic) {
            return Err(bad);
        }
        if index == 0 && kind != b"IHDR" {
            return Err(bad);
        }
        match kind {
            b"IHDR" if header.is_none() && index == 0 && size == 13 => {
                let data = &chunk[8..21];
                let width = u32::from_be_bytes(data[..4].try_into().map_err(|_| bad)?);
                let height = u32::from_be_bytes(data[4..8].try_into().map_err(|_| bad)?);
                if ![width, height]
                    .iter()
                    .all(|n| (256..=1024).contains(n) && n % 32 == 0)
                    || data[8] != 8
                    || ![2, 6].contains(&data[9])
                    || data[10..] != [0, 0, 0]
                {
                    return Err(bad);
                }
                let channels = if data[9] == 2 { 3 } else { 4 };
                header = Some(PngContainer {
                    width,
                    height,
                    channels,
                    inflated_bytes: height as usize * (1 + width as usize * channels as usize),
                });
            }
            b"IDAT" if header.is_some() && !idat_ended => {
                compressed = compressed.checked_add(size).ok_or(bad)?;
            }
            b"IEND" if size == 0 && compressed > 0 && end == bytes.len() => {
                return header.ok_or(bad)
            }
            b"acTL" | b"fcTL" | b"fdAT" | b"IHDR" | b"IEND" | b"IDAT" => return Err(bad),
            _ if kind[0].is_ascii_uppercase() => return Err(bad),
            _ => {
                if compressed > 0 {
                    idat_ended = true;
                }
            }
        }
        offset = end;
    }
    Err(bad)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut b = (data.len() as u32).to_be_bytes().to_vec();
        b.extend(kind);
        b.extend(data);
        b.extend(crc32(&b[4..]).to_be_bytes());
        b
    }
    fn png() -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut h = 512u32.to_be_bytes().to_vec();
        h.extend(512u32.to_be_bytes());
        h.extend([8, 2, 0, 0, 0]);
        b.extend(chunk(b"IHDR", &h));
        b.extend(chunk(b"IDAT", b"synthetic-not-inflated"));
        b.extend(chunk(b"IEND", b""));
        b
    }
    #[test]
    fn request_rejects_paths_unknown_keys_and_excessive_dimensions_without_truncation() {
        let r = ImageRequest {
            prompt: "a blue cube".into(),
            width: 512,
            height: 512,
            seed: 42,
        };
        assert!(r.validate().is_ok());
        for width in [0, 255, 513, 1056] {
            assert!(ImageRequest {
                prompt: "x".into(),
                width,
                height: 512,
                seed: 0
            }
            .validate()
            .is_err());
        }
        assert!(ImageRequest {
            prompt: "x".repeat(2049),
            width: 512,
            height: 512,
            seed: 0
        }
        .validate()
        .is_err());
        assert!(serde_json::from_value::<ImageRequest>(serde_json::json!({"prompt":"x","width":512,"height":512,"seed":0,"output":"/elsewhere"})).is_err());
    }
    #[test]
    fn container_is_bounded_and_does_not_claim_to_decode_pixels() {
        let b = png();
        let h = inspect_png(&b).unwrap();
        assert_eq!((h.width, h.height, h.channels), (512, 512, 3));
        assert_eq!(h.inflated_bytes, 512 * (1 + 512 * 3));
        // This deliberately non-zlib IDAT only tests the container layer.
        assert!(inspect_png(&b[..b.len() - 1]).is_err());
        assert!(inspect_png(&[b.clone(), b"extra".to_vec()].concat()).is_err());
        let mut corrupt = b.clone();
        corrupt[20] ^= 1;
        assert!(inspect_png(&corrupt).is_err());
        assert!(inspect_png(&vec![0; MAX_PNG_BYTES + 1]).is_err());
        let mut animated = b[..33].to_vec();
        animated.extend(chunk(b"acTL", b"animation"));
        animated.extend(&b[33..]);
        assert!(inspect_png(&animated).is_err());
        let mut split = b[..33].to_vec();
        split.extend(chunk(b"IDAT", b"first"));
        split.extend(chunk(b"tEXt", b"note"));
        split.extend(chunk(b"IDAT", b"second"));
        split.extend(chunk(b"IEND", b""));
        assert!(inspect_png(&split).is_err());
    }
}
