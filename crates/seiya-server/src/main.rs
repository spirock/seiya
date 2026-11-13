use seiya_server::config::Config;
use seiya_server::BoundServer;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "Seiya experimental server (discovery by default; opt-in chat with buffered SSE)

Usage:
  seiya-server serve --config PATH
  seiya-server check-config --config PATH
  seiya-server --help
  seiya-server --version

Configuration is explicit; no production files are changed.
Only 127.0.0.1 is supported. Example port: 8012; Python stays on 8010.
API keys are read from a configured environment variable, never CLI arguments.
";

enum Action {
    Help,
    Version,
    Check(PathBuf),
    Serve(PathBuf),
}

fn arguments(args: &[OsString]) -> Result<Action, &'static str> {
    match args {
        [flag] if flag == "--help" || flag == "-h" => Ok(Action::Help),
        [flag] if flag == "--version" || flag == "-V" => Ok(Action::Version),
        [command, flag]
            if (command == "serve" || command == "check-config") && flag == "--help" =>
        {
            Ok(Action::Help)
        }
        [command, flag, path] if flag == "--config" && !path.is_empty() => {
            if command == "serve" {
                Ok(Action::Serve(path.into()))
            } else if command == "check-config" {
                Ok(Action::Check(path.into()))
            } else {
                Err("invalid arguments; use --help")
            }
        }
        _ => Err("invalid arguments; use --help"),
    }
}

#[cfg(unix)]
async fn shutdown_signal() {
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(mut signal) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = signal.recv() => {},
            }
        }
        Err(_) => {
            eprintln!("Seiya experimental: signal handler unavailable; stopping");
        }
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match arguments(&args).map_err(str::to_owned)? {
        Action::Help => print!("{HELP}"),
        Action::Version => println!(
            "seiya-server {} (experimental; discovery or opt-in JSON chat)",
            env!("CARGO_PKG_VERSION")
        ),
        Action::Check(path) => {
            let config = Config::load(&path).map_err(|error| error.to_string())?;
            let _ = config
                .resolve_api_key(|name| std::env::var(name).ok())
                .map_err(|error| error.to_string())?;
            println!("Configuration valid. No sockets opened; no models loaded.");
        }
        Action::Serve(path) => {
            let config = Config::load(&path).map_err(|error| error.to_string())?;
            let server = BoundServer::bind(&config)
                .await
                .map_err(|error| error.to_string())?;
            println!(
                "Seiya experimental · {} · http://{}",
                if server.chat_enabled() {
                    "chat; buffered SSE (not live tokens)"
                } else {
                    "discovery only"
                },
                server.local_addr()
            );
            println!("Runtime not probed. No UI or native tool execution. Ctrl+C to stop.");
            server.serve(shutdown_signal()).await.map_err(|_| {
                "experimental HTTP server failed or runtime state is unconfirmed".to_owned()
            })?;
        }
    }
    Ok(())
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("Seiya experimental: {message}");
            ExitCode::FAILURE
        }
    }
}
