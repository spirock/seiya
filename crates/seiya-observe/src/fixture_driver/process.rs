use crate::runner::RunError;
use crate::{probes, Role, Target};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::Instant;
use tokio::io::AsyncReadExt;

#[derive(Clone)]
pub struct Paths {
    pub python: PathBuf,
    pub python_source: PathBuf,
    pub rust_server: PathBuf,
    pub fixture_runtime: PathBuf,
}

#[derive(Clone, Serialize)]
pub(super) struct Asset {
    kind: String,
    sha256: String,
    version: Option<String>,
}

pub(super) fn clean(mut command: Command, home: &Path) -> Command {
    command
        .env_clear()
        .env("HOME", home)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONSAFEPATH", "1")
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}
pub(super) fn canonical(path: &Path, directory: bool) -> Result<PathBuf, RunError> {
    let path = path.canonicalize().map_err(|_| RunError::Driver)?;
    if !path.is_absolute()
        || path
            .to_str()
            .is_none_or(|s| s.chars().any(char::is_control))
        || directory != path.is_dir()
        || !directory && !path.is_file()
    {
        return Err(RunError::Driver);
    }
    Ok(path)
}
impl Paths {
    pub(super) async fn inspect(self) -> Result<(Self, Vec<Asset>), RunError> {
        self.inspect_mode(true).await
    }
    pub(super) async fn inspect_mode(self, fixture: bool) -> Result<(Self, Vec<Asset>), RunError> {
        let paths = Self {
            python: canonical(&self.python, false)?,
            python_source: canonical(&self.python_source, true)?,
            rust_server: canonical(&self.rust_server, false)?,
            fixture_runtime: if fixture {
                canonical(&self.fixture_runtime, false)?
            } else {
                self.fixture_runtime
            },
        };
        let mut assets = Vec::new();
        let collector = std::env::current_exe().map_err(|_| RunError::Driver)?;
        assets.push(Asset {
            kind: "collector-executable".into(),
            sha256: hash(&collector).await?,
            version: Some(env!("CARGO_PKG_VERSION").into()),
        });
        for (kind, path, prefix) in [
            ("python", &paths.python, "Python "),
            ("rust-server", &paths.rust_server, "seiya-server "),
            (
                "synthetic-runtime",
                &paths.fixture_runtime,
                "seiya-observe-fixture ",
            ),
        ] {
            if kind == "synthetic-runtime" && !fixture {
                continue;
            }
            let version = output(path, &["--version".into()]).await?;
            let version = std::str::from_utf8(&version)
                .map_err(|_| RunError::Driver)?
                .trim();
            if version.len() > 256 || !version.starts_with(prefix) || version.contains('\n') {
                return Err(RunError::Driver);
            }
            assets.push(Asset {
                kind: kind.into(),
                sha256: hash(path).await?,
                version: Some(version.into()),
            });
        }
        let package = paths.python_source.join("seiya");
        if !package.join("__main__.py").is_file() {
            return Err(RunError::Driver);
        }
        let mut files = Vec::new();
        for entry in std::fs::read_dir(package).map_err(|_| RunError::Driver)? {
            let entry = entry.map_err(|_| RunError::Driver)?;
            if entry.path().extension().is_some_and(|e| e == "py") {
                if !entry.file_type().map_err(|_| RunError::Driver)?.is_file() {
                    return Err(RunError::Driver);
                }
                files.push(entry.path());
            }
        }
        if files.is_empty() || files.len() > 32 {
            return Err(RunError::Limit);
        }
        files.sort();
        for path in files {
            let path = canonical(&path, false)?;
            if !path.starts_with(&paths.python_source)
                || std::fs::metadata(&path)
                    .map_err(|_| RunError::Driver)?
                    .len()
                    > 1024 * 1024
            {
                return Err(RunError::Limit);
            }
            assets.push(Asset {
                kind: format!(
                    "python-source/{}",
                    path.file_name()
                        .and_then(|s| s.to_str())
                        .ok_or(RunError::Driver)?
                ),
                sha256: hash(&path).await?,
                version: None,
            });
        }
        Ok((paths, assets))
    }
}

pub(super) async fn output(program: &Path, args: &[String]) -> Result<Vec<u8>, RunError> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .env_clear()
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| RunError::Driver)?;
    tokio::time::timeout(Duration::from_secs(10), async move {
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .ok_or(RunError::Driver)?
            .take(8193)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| RunError::Driver)?;
        if bytes.len() > 8192 {
            return Err(RunError::Limit);
        }
        if !child.wait().await.map_err(|_| RunError::Driver)?.success() {
            return Err(RunError::Driver);
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| RunError::TimedOut)?
}

pub(super) async fn hash(path: &Path) -> Result<String, RunError> {
    let bytes = output(
        Path::new("/usr/bin/shasum"),
        &[
            "-a".into(),
            "256".into(),
            "--".into(),
            path.to_str().ok_or(RunError::Driver)?.into(),
        ],
    )
    .await?;
    parse_hash(&bytes)
}
pub(super) fn parse_hash(bytes: &[u8]) -> Result<String, RunError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RunError::Driver)?;
    let digest = text.split_whitespace().next().ok_or(RunError::Driver)?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || text.lines().count() != 1
    {
        return Err(RunError::Driver);
    }
    Ok(digest.into())
}

pub(super) struct Owned {
    pub child: Child,
    pub target: Target,
    #[cfg(unix)]
    memory: Option<crate::runtime_memory_channel::MemoryChannel<tokio::net::UnixStream>>,
}
impl Owned {
    pub fn spawn(command: &mut Command, role: Role) -> Result<Self, RunError> {
        Self::spawn_inner(command, role, false)
    }
    pub fn spawn_instrumented(command: &mut Command) -> Result<Self, RunError> {
        Self::spawn_inner(command, Role::Runtime, true)
    }
    fn spawn_inner(
        command: &mut Command,
        role: Role,
        instrumented: bool,
    ) -> Result<Self, RunError> {
        #[cfg(unix)]
        let stream = if instrumented {
            Some(
                crate::runtime_memory_channel::configure_socket(command)
                    .map_err(|_| RunError::Driver)?,
            )
        } else {
            None
        };
        #[cfg(not(unix))]
        if instrumented {
            return Err(RunError::Driver);
        }
        let mut child = command.spawn().map_err(|_| RunError::Driver)?;
        // Do not retain duplicate child-side socket descriptors in the parent.
        if instrumented {
            command.stdin(Stdio::null()).stdout(Stdio::null());
        }
        let proof = probes::from_child(&mut child, role, Duration::from_millis(500))
            .map(|probe| probe.root_target());
        match proof {
            Ok(target) => {
                #[cfg(unix)]
                let memory = stream
                    .map(|io| crate::runtime_memory_channel::MemoryChannel::new(io, child.id()));
                Ok(Self {
                    child,
                    target,
                    #[cfg(unix)]
                    memory,
                })
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(RunError::Driver)
            }
        }
    }
    pub async fn memory_sample(
        &mut self,
        wait: Duration,
    ) -> Result<crate::runtime_memory::RuntimeMemoryReport, RunError> {
        #[cfg(unix)]
        {
            if let Err(error) = self.verify() {
                self.memory = None;
                return Err(error);
            }
            let report = self
                .memory
                .as_mut()
                .ok_or(RunError::InvalidEvidence)?
                .sample(wait)
                .await
                .map_err(|_| RunError::InvalidEvidence)?;
            if let Err(error) = self.verify() {
                self.memory = None;
                return Err(error);
            }
            Ok(report)
        }
        #[cfg(not(unix))]
        {
            let _ = wait;
            Err(RunError::Driver)
        }
    }
    pub fn verify(&mut self) -> Result<(), RunError> {
        let proof = probes::from_child(
            &mut self.child,
            self.target.role,
            Duration::from_millis(500),
        )
        .map_err(|_| RunError::IdentityChanged)?;
        if proof.root_target().identity != self.target.identity {
            return Err(RunError::IdentityChanged);
        }
        Ok(())
    }
    pub async fn port_owned(&mut self, port: u16) -> Result<bool, RunError> {
        self.verify()?;
        let result = output(
            Path::new("/usr/sbin/lsof"),
            &[
                "-nP".into(),
                "-a".into(),
                "-p".into(),
                self.child.id().to_string(),
                format!("-iTCP:{port}"),
                "-sTCP:LISTEN".into(),
                "-t".into(),
            ],
        )
        .await;
        self.verify()?;
        match result {
            Ok(bytes) => {
                let text = std::str::from_utf8(&bytes).map_err(|_| RunError::Driver)?;
                let pids: Vec<_> = text.split_whitespace().collect();
                Ok(pids.len() == 1 && pids[0] == self.child.id().to_string())
            }
            Err(RunError::Driver) => Ok(false),
            Err(error) => Err(error),
        }
    }
    pub async fn stop(&mut self, interrupt: bool) -> Result<bool, RunError> {
        #[cfg(unix)]
        {
            self.memory = None;
        }
        if self
            .child
            .try_wait()
            .map_err(|_| RunError::Driver)?
            .is_some()
        {
            return Ok(false);
        }
        #[cfg(target_os = "macos")]
        {
            // Parent's original Child is not reaped until wait, so this PID cannot
            // be reused between try_wait and signaling. Only our own direct child.
            let signal = if interrupt {
                libc::SIGINT
            } else {
                libc::SIGTERM
            };
            // SAFETY: original child PID, valid constant signal, no foreign PID input.
            if unsafe { libc::kill(self.child.id() as i32, signal) } != 0 {
                return Err(RunError::Driver);
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while self
                .child
                .try_wait()
                .map_err(|_| RunError::Driver)?
                .is_none()
            {
                if Instant::now() >= deadline {
                    self.child.kill().map_err(|_| RunError::Driver)?;
                    self.child.wait().map_err(|_| RunError::Driver)?;
                    return Ok(true);
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Ok(false)
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Native signaling/identity is macOS-only. Do not compile the
            // macOS wait/escalation path after a terminal unsupported result.
            let _ = interrupt;
            Err(RunError::Driver)
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

pub(super) fn reserve_port() -> Result<(std::net::TcpListener, u16), RunError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|_| RunError::Driver)?;
    let port = listener.local_addr().map_err(|_| RunError::Driver)?.port();
    if [8010, 8012, 8081].contains(&port) {
        return Err(RunError::Driver);
    }
    Ok((listener, port))
}

#[cfg(all(test, target_os = "macos"))]
mod memory_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "explicit native identity/socketpair test with owned synthetic Python only, no model or network"]
    async fn owned_memory_wrapper_roundtrip_and_cleanup() {
        let python =
            std::env::var_os("SEIYA_TELEMETRY_TEST_PYTHON").expect("explicit Python required");
        let home = PathBuf::from(
            std::env::var_os("SEIYA_TELEMETRY_TEST_HOME")
                .expect("explicit synthetic home required"),
        );
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_memory_peer.py");
        let mut command = clean(Command::new(python), &home);
        command.args(["-I", "-S", "-c", crate::runtime_memory_channel::BOOTSTRAP]);
        command.arg(fixture).current_dir(&home);
        let mut owned = Owned::spawn_instrumented(&mut command).unwrap();
        let identity = owned.target.identity.clone();
        let mut available = false;
        for sequence in 0..10 {
            let report = owned
                .memory_sample(crate::runtime_memory_channel::MAX_WAIT)
                .await
                .unwrap();
            assert_eq!(report.pid, identity.pid());
            assert_eq!(report.sequence, sequence);
            if report.mlx_active_bytes.value == Some(1024) {
                assert_eq!(report.mlx_cache_bytes.value, Some(0));
                assert_eq!(report.mlx_peak_bytes.value, Some(2048));
                assert!(report.metal_allocated_bytes.value.is_none());
                available = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(available, "synthetic core was never observed");
        assert!(
            !owned.stop(false).await.unwrap(),
            "no force expected for owned helper"
        );
        assert!(owned.child.try_wait().unwrap().is_some());
        assert!(owned
            .memory_sample(crate::runtime_memory_channel::MAX_WAIT)
            .await
            .is_err());
    }
}
