use seiya_contracts::image_job::{inspect_png, MAX_PNG_BYTES};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub bytes: usize,
    pub sha256: String,
    pub pixel_sha256: String,
    pub nonuniform: bool,
}

/// Native zlib is an OS adapter, not Python or an additional installed runtime.
/// Input/output buffers and consumed byte counts are bounded on both sides.
#[cfg(target_os = "macos")]
fn inflate(input: &[u8], expected: usize) -> Result<Vec<u8>, &'static str> {
    use std::ffi::{c_int, c_ulong};
    #[link(name = "z")]
    extern "C" {
        fn uncompress2(
            dest: *mut u8,
            dest_len: *mut c_ulong,
            source: *const u8,
            source_len: *mut c_ulong,
        ) -> c_int;
    }
    let mut output = vec![0; expected + 1];
    let mut output_len = output.len() as c_ulong;
    let mut input_len = input.len() as c_ulong;
    // SAFETY: exact writable/readable buffers with declared lengths; zlib never
    // owns pointers. Reject errors, oversized output and trailing compressed input.
    let result = unsafe {
        uncompress2(
            output.as_mut_ptr(),
            &mut output_len,
            input.as_ptr(),
            &mut input_len,
        )
    };
    if result != 0 || output_len as usize != expected || input_len as usize != input.len() {
        return Err("png_zlib_size_or_stream");
    }
    output.truncate(expected);
    Ok(output)
}
#[cfg(not(target_os = "macos"))]
fn inflate(_: &[u8], _: usize) -> Result<Vec<u8>, &'static str> {
    Err("native_png_decoder_unsupported")
}

pub fn decode(raw: &[u8], width: u32, height: u32) -> Result<DecodedImage, &'static str> {
    if raw.len() > MAX_PNG_BYTES {
        return Err("png_limit");
    }
    let header = inspect_png(raw)?;
    if (header.width, header.height) != (width, height) {
        return Err("png_dimensions_mismatch");
    }
    let mut offset = 8;
    let mut compressed = Vec::new();
    while offset < raw.len() {
        let n = u32::from_be_bytes(
            raw[offset..offset + 4]
                .try_into()
                .map_err(|_| "png_bounds")?,
        ) as usize;
        if &raw[offset + 4..offset + 8] == b"IDAT" {
            compressed.extend_from_slice(&raw[offset + 8..offset + 8 + n]);
        }
        offset += 12 + n;
    }
    let pixels = inflate(&compressed, header.inflated_bytes)?;
    let channels = header.channels as usize;
    let stride = width as usize * channels;
    let mut prev = vec![0u8; stride];
    let mut first = None;
    let mut nonuniform = false;
    let mut visible = channels == 3;
    let mut pixel_hash = Sha256::new();
    for line in pixels.chunks_exact(stride + 1) {
        let mode = line[0];
        if mode > 4 {
            return Err("png_row_filter");
        }
        let mut row = line[1..].to_vec();
        for x in 0..stride {
            let a = if x >= channels { row[x - channels] } else { 0 };
            let b = prev[x];
            let c = if x >= channels { prev[x - channels] } else { 0 };
            let predictor = match mode {
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                4 => {
                    let p = i32::from(a) + i32::from(b) - i32::from(c);
                    let pa = (p - i32::from(a)).abs();
                    let pb = (p - i32::from(b)).abs();
                    let pc = (p - i32::from(c)).abs();
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                _ => 0,
            };
            row[x] = row[x].wrapping_add(predictor);
        }
        for pixel in row.chunks_exact(channels) {
            let rgb = [pixel[0], pixel[1], pixel[2]];
            let origin = *first.get_or_insert(rgb);
            nonuniform |= rgb != origin;
            if channels == 4 {
                visible |= pixel[3] > 0;
            }
        }
        pixel_hash.update(&row);
        prev = row;
    }
    if !nonuniform || !visible {
        return Err("synthetic_png_blank_or_uniform");
    }
    Ok(DecodedImage {
        width,
        height,
        channels: header.channels,
        bytes: raw.len(),
        sha256: format!("{:x}", Sha256::digest(raw)),
        pixel_sha256: format!("{:x}", pixel_hash.finalize()),
        nonuniform,
    })
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::ffi::{c_int, c_ulong};
    #[link(name = "z")]
    extern "C" {
        fn compress2(
            dest: *mut u8,
            dest_len: *mut c_ulong,
            source: *const u8,
            source_len: c_ulong,
            level: c_int,
        ) -> c_int;
    }
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut b = (data.len() as u32).to_be_bytes().to_vec();
        b.extend(kind);
        b.extend(data);
        let mut crc = !0u32;
        for v in &b[4..] {
            crc ^= u32::from(*v);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
            }
        }
        b.extend((!crc).to_be_bytes());
        b
    }
    fn png(mode: u8, uniform: bool, bomb: bool) -> (Vec<u8>, String) {
        let mut row = vec![255u8; 256 * 3];
        row[..3].copy_from_slice(&[0, 0, 255]);
        if uniform {
            row.fill(0);
        }
        let expected = format!("{:x}", Sha256::digest(row.repeat(256)));
        let mut previous = vec![0u8; row.len()];
        let mut raw = Vec::new();
        for _ in 0..256 {
            raw.push(mode);
            for x in 0..row.len() {
                let a = if x >= 3 { row[x - 3] } else { 0 };
                let b = previous[x];
                let c = if x >= 3 { previous[x - 3] } else { 0 };
                let p = i32::from(a) + i32::from(b) - i32::from(c);
                let pa = (p - i32::from(a)).abs();
                let pb = (p - i32::from(b)).abs();
                let pc = (p - i32::from(c)).abs();
                let pred = match mode {
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    4 => {
                        if pa <= pb && pa <= pc {
                            a
                        } else if pb <= pc {
                            b
                        } else {
                            c
                        }
                    }
                    _ => 0,
                };
                raw.push(row[x].wrapping_sub(pred));
            }
            previous.clone_from(&row);
        }
        if bomb {
            raw.extend([0u8; 10000]);
        }
        let mut compressed = vec![0u8; raw.len() + raw.len() / 100 + 128];
        let mut len = compressed.len() as c_ulong;
        // SAFETY: synthetic buffers sized above zlib's worst-case test bound.
        assert_eq!(
            unsafe {
                compress2(
                    compressed.as_mut_ptr(),
                    &mut len,
                    raw.as_ptr(),
                    raw.len() as c_ulong,
                    6,
                )
            },
            0
        );
        compressed.truncate(len as usize);
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut h = 256u32.to_be_bytes().to_vec();
        h.extend(256u32.to_be_bytes());
        h.extend([8, 2, 0, 0, 0]);
        b.extend(chunk(b"IHDR", &h));
        b.extend(chunk(b"IDAT", &compressed));
        b.extend(chunk(b"IEND", &[]));
        (b, expected)
    }
    #[test]
    fn all_five_filters_decode_to_expected_pixels_without_python() {
        for mode in 0..5 {
            let (b, hash) = png(mode, false, false);
            let a = decode(&b, 256, 256).unwrap();
            assert!(a.nonuniform);
            assert_eq!(a.pixel_sha256, hash);
        }
    }
    #[test]
    fn zlib_bomb_uniform_corrupt_dimensions_and_filter_are_not_artifacts() {
        for b in [
            png(0, true, false).0,
            png(0, false, true).0,
            png(5, false, false).0,
        ] {
            assert!(decode(&b, 256, 256).is_err());
        }
        let (mut b, _) = png(0, false, false);
        assert!(decode(&b, 512, 256).is_err());
        assert!(decode(&b[..b.len() - 2], 256, 256).is_err());
        let mut trailing = b.clone();
        trailing.extend(b"extra");
        assert!(decode(&trailing, 256, 256).is_err());
        assert!(decode(b"<svg/>", 256, 256).is_err());
        b[20] ^= 1;
        assert!(decode(&b, 256, 256).is_err());
    }
}
