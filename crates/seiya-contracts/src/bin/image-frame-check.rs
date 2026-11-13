//! Manual offline boundary checker; no paths, model load or image tool registration.
use seiya_contracts::image_job::{inspect_png, ImageRequest, MAX_PNG_BYTES};
use serde_json::json;
use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args().nth(1).ok_or("mode_required")?;
    let cap = match mode.as_str() {
        "request" => 4096,
        "png" => MAX_PNG_BYTES,
        _ => return Err("unknown_mode".into()),
    };
    let mut input = Vec::new();
    std::io::stdin()
        .take((cap + 1) as u64)
        .read_to_end(&mut input)?;
    if input.len() > cap {
        return Err("input_limit".into());
    }
    let result = if mode == "request" {
        let request: ImageRequest = serde_json::from_slice(&input)?;
        request.validate()?;
        json!({"ok":true,"request":request})
    } else {
        json!({"ok":true,"level":"container-integrity-only","png":inspect_png(&input)?})
    };
    serde_json::to_writer(std::io::stdout(), &result)?;
    Ok(())
}
