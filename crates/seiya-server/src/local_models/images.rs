//! Explicit local image API adapter; same ownership authority as text.
use super::Pool;
use crate::image_jobs::{Admission, Executor, Work};
use base64::{engine::general_purpose::STANDARD, Engine};
use seiya_contracts::image_job::{ImageRequest, ImageStage, MAX_PNG_BYTES};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::watch;

pub(super) struct Images(pub Arc<Pool>);
impl Executor for Images {
    fn available(&self) -> bool {
        self.0.catalog.image.is_some() && self.0.available()
    }
    fn generate(
        &self,
        request: ImageRequest,
        cancel: watch::Receiver<bool>,
        stage: watch::Sender<ImageStage>,
    ) -> Work {
        let pool = self.0.clone();
        Box::pin(async move { pool.image(request, cancel, stage).await })
    }
}
impl Admission for Pool {
    fn allows_image(&self) -> bool {
        self.catalog.image.is_some() && self.available()
    }
}
pub(super) fn decode(value: &Value, binding: &str) -> Result<Vec<u8>, ()> {
    if value["model"] != binding {
        return Err(());
    }
    let data = value["data"]
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or(())?;
    let item = data[0].as_object().ok_or(())?;
    if item.contains_key("url") {
        return Err(());
    }
    let text = item
        .get("b64_json")
        .and_then(Value::as_str)
        .filter(|s| s.len() <= MAX_PNG_BYTES.div_ceil(3) * 4)
        .ok_or(())?;
    let bytes = STANDARD.decode(text).map_err(|_| ())?;
    if bytes.is_empty() || bytes.len() > MAX_PNG_BYTES {
        return Err(());
    }
    Ok(bytes)
}

#[cfg(test)]
pub(crate) fn fixture_png() -> Vec<u8> {
    fn crc(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for b in bytes {
            crc ^= u32::from(*b);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(data);
        out.extend(crc(&out[start..]).to_be_bytes());
    }
    let mut raw = vec![];
    for y in 0..512 {
        raw.push(0);
        for x in 0..512 {
            raw.extend([(x % 251) as u8, (y % 251) as u8, 42]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let count = raw.len().div_ceil(65535);
    for (i, part) in raw.chunks(65535).enumerate() {
        z.push(u8::from(i + 1 == count));
        z.extend((part.len() as u16).to_le_bytes());
        z.extend((!(part.len() as u16)).to_le_bytes());
        z.extend(part);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = 512u32.to_be_bytes().to_vec();
    header.extend(512u32.to_be_bytes());
    header.extend([8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn binding_single_png_and_no_url_are_enforced() {
        let png = fixture_png();
        let reply = json!({"model":"binding","data":[{"b64_json":STANDARD.encode(&png)}]});
        assert_eq!(decode(&reply, "binding").unwrap(), png);
        assert!(decode(&reply, "other").is_err());
        for value in [
            json!({"model":"binding","data":[{"url":"http://127.0.0.1/private"}]}),
            json!({"model":"binding","data":[]}),
            json!({"model":"binding","data":[{"b64_json":"not-base64"}]}),
        ] {
            assert!(decode(&value, "binding").is_err());
        }
        #[cfg(target_os = "macos")]
        assert!(
            seiya_observe::image::artifact::decode(&png, 512, 512)
                .unwrap()
                .nonuniform
        );
    }
}
