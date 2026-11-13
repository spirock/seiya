#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() -> Result<(), String> {
    use seiya_server::pair_trial::native;
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    if let [mode, path, flag, approval] = args.as_slice() {
        if mode == "--recover-quarantine"
            && flag == "--approval"
            && approval == native::RECOVERY_APPROVAL
        {
            println!(
                "{}",
                native::recover_quarantine(std::path::Path::new(path)).await?
            );
            return Ok(());
        }
    }
    let solo = args.first().is_some_and(|a| a == "--solo-gemma");
    if solo {
        args.remove(0);
    }
    let expected = if solo {
        native::SOLO_APPROVAL
    } else {
        native::APPROVAL
    };
    if let [mode, value] = args.as_slice() {
        if mode == "--fixture-listener" || mode == "--fixture-respawn" {
            let port: u16 = value.parse().map_err(|_| "fixture_port")?;
            if port < 49152 {
                return Err("fixture_ephemeral_port_required".into());
            }
            let _listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
                .map_err(|_| "fixture_bind")?;
            if mode == "--fixture-respawn" {
                let start = std::time::Instant::now();
                while start.elapsed() < std::time::Duration::from_secs(10) {
                    std::process::Command::new("/bin/sleep")
                        .arg("2")
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .map_err(|_| "fixture_worker_spawn")?
                        .wait()
                        .map_err(|_| "fixture_worker_wait")?;
                }
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            return Ok(());
        }
        if mode == "--rehearsal" {
            println!("{}", native::rehearsal(std::path::Path::new(value)).await?);
            return Ok(());
        }
    }
    if let [flag, path, approve, value] = args.as_slice() {
        if flag == "--output" && approve == "--approval" && value == expected {
            return if solo {
                native::run_solo(std::path::Path::new(path), Default::default()).await
            } else {
                native::run(std::path::Path::new(path), Default::default()).await
            };
        }
    }
    if let [flag, path, approve, value, limits, config] = args.as_slice() {
        if flag == "--output"
            && approve == "--approval"
            && value == expected
            && limits == "--limits"
        {
            let policy =
                seiya_server::pair_trial::limits::TrialLimits::read(std::path::Path::new(config))?;
            return if solo {
                native::run_solo(std::path::Path::new(path), policy).await
            } else {
                native::run(std::path::Path::new(path), policy).await
            };
        }
    }
    Err("explicit isolated pair-web trial approval and new output directory required".into())
}
#[cfg(not(target_os = "macos"))]
fn main() -> Result<(), String> {
    Err("native pair trial is macOS-only".into())
}
