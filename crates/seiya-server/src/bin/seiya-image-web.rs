#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() -> Result<(), String> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    if let [mode, root, flag, approval] = args.as_slice() {
        if mode == "--gemma-smoke"
            && flag == "--approval"
            && approval == seiya_server::image_web::GEMMA_SMOKE_APPROVAL
        {
            println!(
                "{}",
                seiya_server::image_web::gemma_smoke(std::path::Path::new(root)).await?
            );
            return Ok(());
        }
    }
    let evaluation = args.first().is_some_and(|v| v == "--evaluation-web");
    if evaluation {
        args.remove(0);
    }
    let gemma_only = args.first().is_some_and(|v| v == "--gemma-web");
    if gemma_only {
        args.remove(0);
    }
    if let [flag, root, approval, value, listen, address, prefix, bits, cert, certificate, key, private_key, history_flag, history] =
        args.as_slice()
    {
        if flag == "--output"
            && approval == "--approval"
            && value
                == if evaluation {
                    seiya_server::image_web::EVALUATION_APPROVAL
                } else if gemma_only {
                    seiya_server::image_web::GEMMA_WEB_APPROVAL
                } else {
                    seiya_server::image_web::APPROVAL
                }
            && listen == "--listen"
            && prefix == "--prefix-length"
            && cert == "--certificate-der"
            && key == "--private-key-der"
            && history_flag == "--history-dir"
        {
            let config = seiya_server::image_web::LanConfig {
                history: history.into(),
                address: address.parse().map_err(|_| "LAN_address")?,
                prefix: bits.parse().map_err(|_| "LAN_prefix")?,
                certificate: certificate.into(),
                private_key: private_key.into(),
            };
            return if evaluation {
                seiya_server::image_web::evaluation_web(std::path::Path::new(root), config).await
            } else if gemma_only {
                seiya_server::image_web::gemma_web(std::path::Path::new(root), config).await
            } else {
                seiya_server::image_web::run(std::path::Path::new(root), config).await
            };
        }
    }
    Err("explicit temporary sequential chat/image approval and new private root required".into())
}
#[cfg(not(target_os = "macos"))]
fn main() -> Result<(), String> {
    Err("native image web host is macOS-only".into())
}
