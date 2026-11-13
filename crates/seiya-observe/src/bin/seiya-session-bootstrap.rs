//! Internal trusted-operator bootstrap. No HTTP, shell, model selection or tools.
#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !seiya_observe::session_containment::fork_and_spawn_denied() {
        return std::process::ExitCode::FAILURE;
    }
    if args.as_slice() == [std::ffi::OsStr::new("--probe-only")] {
        println!("SEIYA_PROCESS_CREATION_DENIED_V1");
        return std::process::ExitCode::SUCCESS;
    }
    if args.len() < 2
        || args[0] != "--exec"
        || args.len() > 34
        || !std::path::Path::new(&args[1]).is_absolute()
        || args.iter().any(|arg| arg.len() > 4096)
    {
        return std::process::ExitCode::FAILURE;
    }
    println!("SEIYA_PROCESS_CREATION_DENIED_V1");
    if std::io::stdout().flush().is_err() {
        return std::process::ExitCode::FAILURE;
    }
    // Replaces this original Child; does not create another PID. Kernel policy
    // is inherited across exec; runtime gets no bootstrap pipe/logging channel.
    let _error = std::process::Command::new(&args[1])
        .args(&args[2..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .exec();
    std::process::ExitCode::FAILURE
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::FAILURE
}
