#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() -> Result<(), String> {
    let mut a: Vec<_> = std::env::args().skip(1).collect();
    let previous = if a.len() == 16 && a[14] == "--previous-stop" {
        let p = std::path::PathBuf::from(a.pop().unwrap());
        a.pop();
        Some(p)
    } else {
        None
    };
    if let [output, root, approve, permission, listen, address, prefix, bits, cert, certificate, key, private_key, settings_flag, settings_dir] =
        a.as_slice()
    {
        if output == "--output"
            && approve == "--approval"
            && permission == seiya_server::web_diagnostics::APPROVAL
            && listen == "--listen"
            && prefix == "--prefix-length"
            && cert == "--certificate-der"
            && key == "--private-key-der"
            && settings_flag == "--settings-dir"
        {
            let c = seiya_server::image_web::LanConfig {
                address: address.parse().map_err(|_| "LAN_address")?,
                prefix: bits.parse().map_err(|_| "LAN_prefix")?,
                certificate: certificate.into(),
                private_key: private_key.into(),
                history: settings_dir.into(),
            };
            return seiya_server::web_diagnostics::run(
                std::path::Path::new(root),
                c,
                previous.as_deref(),
            )
            .await;
        }
    }
    Err(
        "explicit no-inference LAN diagnostics approval/new output/TLS configuration required"
            .into(),
    )
}
#[cfg(not(target_os = "macos"))]
fn main() -> Result<(), String> {
    Err("native diagnostics host is macOS-only".into())
}
