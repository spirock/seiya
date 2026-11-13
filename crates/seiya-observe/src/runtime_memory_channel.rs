//! Private transport, not an endpoint or an authorization source.
#[cfg(any(unix, test))]
use crate::runtime_memory::{RuntimeMemoryReport, MAX_REPORT_BYTES};
use std::time::Duration;
#[cfg(any(unix, test))]
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) const BOOTSTRAP: &str = concat!(
    include_str!("../../../swarm-app/seiya/runtime_memory.py"),
    "\n",
    include_str!("../../../swarm-app/seiya/runtime_memory_bootstrap.py")
);
#[cfg(any(unix, test))]
pub(crate) const MAX_SAMPLES: u64 = 1024;
pub(crate) const MAX_WAIT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(any(unix, test))]
pub(crate) enum ChannelError {
    Closed,
    Io,
    Frame,
    InvalidReport,
    Sequence,
    Clock,
    TimedOut,
    Limit,
}

/// One request in flight. Removing io before awaiting makes timeout or dropping
/// the future terminal: uncertain bytes can never be reused by a later request.
#[cfg(any(unix, test))]
pub(crate) struct MemoryChannel<S> {
    io: Option<S>,
    pid: u32,
    sequence: u64,
    last_finished: u64,
}

#[cfg(any(unix, test))]
impl<S: AsyncRead + AsyncWrite + Unpin> MemoryChannel<S> {
    pub(crate) fn new(io: S, pid: u32) -> Self {
        Self {
            io: Some(io),
            pid,
            sequence: 0,
            last_finished: 0,
        }
    }

    pub(crate) async fn sample(
        &mut self,
        wait: Duration,
    ) -> Result<RuntimeMemoryReport, ChannelError> {
        let mut io = self.io.take().ok_or(ChannelError::Closed)?;
        if self.sequence >= MAX_SAMPLES || wait.is_zero() || wait > MAX_WAIT {
            return Err(ChannelError::Limit);
        }
        let report = tokio::time::timeout(wait, async {
            io.write_all(b"sample\n")
                .await
                .map_err(|_| ChannelError::Io)?;
            io.flush().await.map_err(|_| ChannelError::Io)?;
            let mut bytes = Vec::with_capacity(1024);
            loop {
                let byte = io.read_u8().await.map_err(|_| ChannelError::Io)?;
                if byte == b'\n' {
                    break;
                }
                if bytes.len() == MAX_REPORT_BYTES {
                    return Err(ChannelError::Frame);
                }
                bytes.push(byte);
            }
            RuntimeMemoryReport::decode(&bytes, self.pid).map_err(|_| ChannelError::InvalidReport)
        })
        .await
        .map_err(|_| ChannelError::TimedOut)??;
        if report.sequence != self.sequence {
            return Err(ChannelError::Sequence);
        }
        if report.read_started_ns < self.last_finished {
            return Err(ChannelError::Clock);
        }
        self.sequence += 1;
        self.last_finished = report.read_finished_ns;
        self.io = Some(io);
        Ok(report)
    }
}

#[cfg(unix)]
pub(crate) fn configure_socket(
    command: &mut std::process::Command,
) -> std::io::Result<tokio::net::UnixStream> {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::process::Stdio;
    let (parent, child) = UnixStream::pair()?;
    command.stdin(Stdio::from(OwnedFd::from(child.try_clone()?)));
    command.stdout(Stdio::from(OwnedFd::from(child)));
    parent.set_nonblocking(true)?;
    tokio::net::UnixStream::from_std(parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tokio::io::duplex;

    fn report() -> Value {
        serde_json::from_str(include_str!(
            "../../../contracts/benchmarks/memory-baseline/v0.1.0/runtime-memory.fixture.json"
        ))
        .unwrap()
    }

    async fn reply(mut peer: tokio::io::DuplexStream, value: Value) {
        let mut request = [0; 7];
        peer.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"sample\n");
        peer.write_all(&serde_json::to_vec(&value).unwrap())
            .await
            .unwrap();
        peer.write_all(b"\n").await.unwrap();
    }

    #[tokio::test]
    async fn sequence_and_clock_are_bound_to_this_channel() {
        let (io, mut peer) = duplex(8192);
        let worker = tokio::spawn(async move {
            for sequence in 0..2 {
                let mut request = [0; 7];
                peer.read_exact(&mut request).await.unwrap();
                let mut value = report();
                value["sequence"] = json!(sequence);
                value["read_started_ns"] = json!(100 + 25 * sequence);
                value["read_finished_ns"] = json!(125 + 25 * sequence);
                peer.write_all(&serde_json::to_vec(&value).unwrap())
                    .await
                    .unwrap();
                peer.write_all(b"\n").await.unwrap();
            }
        });
        let mut channel = MemoryChannel::new(io, 42);
        assert_eq!(channel.sample(MAX_WAIT).await.unwrap().sequence, 0);
        assert_eq!(channel.sample(MAX_WAIT).await.unwrap().sequence, 1);
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn malformed_eof_sequence_and_clock_close_without_retry() {
        for kind in ["sequence", "clock", "pid", "eof", "oversize", "invalid"] {
            let (io, mut peer) = duplex(8192);
            let mut channel = MemoryChannel::new(io, 42);
            if kind == "clock" {
                channel.last_finished = 126;
            }
            let worker = tokio::spawn(async move {
                let mut value = report();
                match kind {
                    "sequence" => value["sequence"] = json!(1),
                    "pid" => value["pid"] = json!(43),
                    "invalid" => value = json!({}),
                    "oversize" => {
                        let mut request = [0; 7];
                        peer.read_exact(&mut request).await.unwrap();
                        peer.write_all(&vec![b'x'; MAX_REPORT_BYTES + 1])
                            .await
                            .unwrap();
                        return;
                    }
                    "eof" => return,
                    _ => {}
                }
                reply(peer, value).await;
            });
            assert!(channel.sample(MAX_WAIT).await.is_err(), "{kind}");
            assert_eq!(channel.sample(MAX_WAIT).await, Err(ChannelError::Closed));
            worker.await.unwrap();
        }
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_and_dropped_future_are_terminal() {
        let (io, _peer) = duplex(8192);
        let mut channel = MemoryChannel::new(io, 42);
        assert_eq!(channel.sample(MAX_WAIT).await, Err(ChannelError::TimedOut));
        assert_eq!(channel.sample(MAX_WAIT).await, Err(ChannelError::Closed));
        let (io, _peer) = duplex(8192);
        let mut channel = MemoryChannel::new(io, 42);
        tokio::select! {
            result = channel.sample(MAX_WAIT) => panic!("unexpected completion: {result:?}"),
            _ = tokio::time::sleep(Duration::from_millis(1)) => {}
        }
        assert_eq!(channel.sample(MAX_WAIT).await, Err(ChannelError::Closed));
    }

    #[tokio::test]
    async fn limits_close_before_any_request() {
        for wait in [Duration::ZERO, MAX_WAIT + Duration::from_millis(1)] {
            let (io, _peer) = duplex(8192);
            let mut channel = MemoryChannel::new(io, 42);
            assert_eq!(channel.sample(wait).await, Err(ChannelError::Limit));
        }
        let (io, _peer) = duplex(8192);
        let mut channel = MemoryChannel::new(io, 42);
        channel.sequence = MAX_SAMPLES;
        assert_eq!(channel.sample(MAX_WAIT).await, Err(ChannelError::Limit));
    }
}
