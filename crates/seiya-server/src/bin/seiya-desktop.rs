//! Offline checks and explicitly approved portable macOS LAN host.
use std::{path::Path, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    if let [launch, catalog, path] = args.as_slice() {
        if launch == "--launch" && catalog == "--catalog" {
            return match seiya_server::desktop_setup::launch_catalog(Some(path.into())).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("Seiya: {e}");
                    ExitCode::from(1)
                }
            };
        }
    }
    if let [flag] = args.as_slice() {
        if flag == "--launch" {
            return match seiya_server::desktop_setup::launch().await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("Seiya: {e}");
                    ExitCode::from(1)
                }
            };
        }
        if flag == "--version" {
            println!("seiya-desktop {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        if flag == "--help" {
            println!("Seiya Rust experimental\nWeb-first setup/chat shell: seiya-desktop --launch [--catalog ABSOLUTE_FILE]\nOffline local text catalogue: --check-catalog FILE\nCatalogue backend: externally managed local OpenAI-compatible engines on Windows/Linux/macOS; no bundled engine, model downloads or automatic inference. Existing HTML/CSS/JavaScript retained.\nOffline macOS MLX profile: seiya-desktop --check-config FILE\nmacOS manual host: --serve --config FILE --approval manual-portable-macos-web-no-automatic-inference --listen PRIVATE_IP:8015 --prefix-length BITS --certificate-der FILE --private-key-der FILE [--chat-only]\nNative --serve/MLX and pinned images remain macOS-only. Catalogue engines must be prepared and dedicated by the operator; no implicit remote fallback. On Windows use seiya-server.exe check-config for the separate API configuration.");
            return ExitCode::SUCCESS;
        }
    }
    if let [flag, path] = args.as_slice() {
        if flag == "--check-catalog" {
            return match seiya_server::local_models::check_file(Path::new(path)) {
                Ok(report) => {
                    println!("{}", report);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(1)
                }
            };
        }
        if flag == "--check-config" {
            return match seiya_server::desktop_config::check_file(Path::new(path)) {
                Ok(report) => {
                    println!("{}", serde_json::to_string(&report).unwrap());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("{}", serde_json::to_string(&error).unwrap());
                    ExitCode::from(1)
                }
            };
        }
    }
    let chat_only = args.last().is_some_and(|v| v == "--chat-only");
    if chat_only {
        args.pop();
    }
    if let [serve, config_flag, config, approval, value, listen, address, prefix, bits, cert, certificate, key, private_key] =
        args.as_slice()
    {
        if serve == "--serve"
            && config_flag == "--config"
            && approval == "--approval"
            && value == "manual-portable-macos-web-no-automatic-inference"
            && listen == "--listen"
            && prefix == "--prefix-length"
            && cert == "--certificate-der"
            && key == "--private-key-der"
        {
            #[cfg(target_os = "macos")]
            {
                let result = async {
                    let lan = seiya_server::image_web::LanConfig {
                        history: std::path::PathBuf::new(),
                        address: address
                            .to_str()
                            .ok_or("LAN_address")?
                            .parse()
                            .map_err(|_| "LAN_address")?,
                        prefix: bits
                            .to_str()
                            .ok_or("LAN_prefix")?
                            .parse()
                            .map_err(|_| "LAN_prefix")?,
                        certificate: certificate.into(),
                        private_key: private_key.into(),
                    };
                    seiya_server::image_web::portable(Path::new(config), lan, chat_only).await
                }
                .await;
                return match result {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("{e}");
                        ExitCode::from(1)
                    }
                };
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (config, address, bits, certificate, private_key);
                eprintln!("portable host is macOS-only; no runtime started");
                return ExitCode::from(1);
            }
        }
    }
    eprintln!("usage: seiya-desktop --check-config FILE\nmacOS: --serve --config FILE --approval manual-portable-macos-web-no-automatic-inference --listen PRIVATE_IP:8015 --prefix-length BITS --certificate-der FILE --private-key-der FILE [--chat-only]");
    ExitCode::from(2)
}
