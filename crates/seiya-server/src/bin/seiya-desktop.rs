//! Offline checks and explicitly approved portable macOS LAN host.
use std::{path::Path, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    if let [flag] = args.as_slice() {
        if flag == "--launch" {
            #[cfg(target_os = "macos")]
            return match seiya_server::launcher::launch().await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("Seiya: {e}");
                    ExitCode::from(1)
                }
            };
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("native launcher is macOS-only; no runtime started");
                return ExitCode::from(1);
            }
        }
        if flag == "--version" {
            println!("seiya-desktop {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        if flag == "--help" {
            println!("Seiya Rust experimental\nOffline macOS profile: seiya-desktop --check-config FILE\nmacOS manual host: --serve --config FILE --approval manual-portable-macos-web-no-automatic-inference --listen PRIVATE_IP:8015 --prefix-length BITS --certificate-der FILE --private-key-der FILE [--chat-only]\nNo bundled models/runtime. No automatic inference. Native --serve is macOS-only. On Windows use seiya-server.exe check-config for API configurations; no Windows native inference host is provided.");
            return ExitCode::SUCCESS;
        }
    }
    if let [flag, path] = args.as_slice() {
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
