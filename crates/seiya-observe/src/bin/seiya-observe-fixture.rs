//! Test fixture process: no model path, runtime selection, shell or real inference.
#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode]
            if matches!(
                mode.as_str(),
                "--owned-session-fixture"
                    | "--owned-session-ignore-eof"
                    | "--owned-session-bad-ready"
            ) =>
        {
            owned_session_fixture(
                mode == "--owned-session-ignore-eof",
                mode == "--owned-session-bad-ready",
            )
        }
        [mode, rest @ ..] if mode == "--recipe-fixture" => {
            match seiya_observe::fixture_driver::serve_recipe_fixture(rest.to_vec()).await {
                Ok(()) => std::process::ExitCode::SUCCESS,
                Err(_) => std::process::ExitCode::FAILURE,
            }
        }
        [flag] if flag == "--version" => {
            println!("seiya-observe-fixture {}", env!("CARGO_PKG_VERSION"));
            std::process::ExitCode::SUCCESS
        }
        [mode, flag, port]
            if matches!(
                mode.as_str(),
                "--fixture-runtime" | "--fixture-runtime-ignore-term"
            ) && flag == "--port" =>
        {
            #[cfg(target_os = "macos")]
            if mode == "--fixture-runtime-ignore-term" {
                // Explicit synthetic cleanup failure; own helper self-expires.
                // SAFETY: fixed disposition only in this explicit fixture child.
                unsafe {
                    libc::signal(libc::SIGTERM, libc::SIG_IGN);
                }
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    std::process::exit(0);
                });
            }
            let Some(port) = port
                .parse::<u16>()
                .ok()
                .filter(|port| *port > 0 && ![8010, 8012, 8081].contains(port))
            else {
                return std::process::ExitCode::FAILURE;
            };
            match seiya_observe::fixture_driver::serve_fixture(port).await {
                Ok(()) => std::process::ExitCode::SUCCESS,
                Err(_) => std::process::ExitCode::FAILURE,
            }
        }
        _ => {
            eprintln!("Fixture only: --version or --fixture-runtime --port EPHEMERAL_PORT");
            std::process::ExitCode::FAILURE
        }
    }
}

fn owned_session_fixture(ignore_eof: bool, bad_ready: bool) -> std::process::ExitCode {
    use std::io::{Read, Write};
    // Audited leaf fixture: no child processes, network or model selection. A
    // bounded self-expiry is not a signal to another process or a native watchdog.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(10));
        std::process::exit(0);
    });
    println!(
        "{}",
        if bad_ready {
            "INVALID_FIXTURE_READY"
        } else {
            "SEIYA_OWNED_SESSION_READY"
        }
    );
    if std::io::stdout().flush().is_err() {
        return std::process::ExitCode::FAILURE;
    }
    let mut byte = [0u8; 1];
    while std::io::stdin().read(&mut byte).unwrap_or(0) > 0 {}
    if ignore_eof {
        loop {
            std::thread::park();
        }
    }
    std::process::ExitCode::SUCCESS
}
