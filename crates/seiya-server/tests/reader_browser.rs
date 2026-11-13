//! Python browser HTTP -> authenticated Rust reader/selector -> synthetic streams.
//! No DNS, external network, model, browser profile or operational service.
use seiya_server::{
    app_with_reader,
    reader::{
        transport::{Network, Stream},
        Error, Reader,
    },
};
use std::{
    future::Future,
    net::IpAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'a>>;
struct Peer {
    cases: serde_json::Value,
}
struct Memory {
    input: Vec<u8>,
    output: std::io::Cursor<Vec<u8>>,
    cases: serde_json::Value,
}
impl AsyncWrite for Memory {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.input.extend_from_slice(data);
        if self.input.ends_with(b"\r\n\r\n") {
            let request = std::str::from_utf8(&self.input).unwrap();
            let path = request.split_whitespace().nth(1).unwrap();
            let parts: Vec<_> = path.trim_matches('/').split('/').collect();
            let case = self.cases["synthesis_cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == parts[0])
                .unwrap();
            let source = case["sources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["id"] == parts[1]);
            let reply = if let Some(source) = source {
                let text = source["text"].as_str().unwrap();
                let body = if parts[0] == "summary-03" {
                    "<nav>Only a homepage</nav>".to_string()
                } else {
                    format!("<article><p>{}</p></article>", text.repeat(2))
                };
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{body}",body.len())
            } else {
                "HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\n\r\n".to_string()
            };
            self.output = std::io::Cursor::new(reply.into_bytes());
        }
        Poll::Ready(Ok(data.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
impl AsyncRead for Memory {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let p = self.output.position() as usize;
        let end = (p + out.remaining().min(31)).min(self.output.get_ref().len());
        out.put_slice(&self.output.get_ref()[p..end]);
        self.output.set_position(end as u64);
        Poll::Ready(Ok(()))
    }
}
impl Network for Peer {
    fn resolve(&self, host: String) -> Pending<'_, Vec<IpAddr>> {
        Box::pin(async move {
            assert_eq!(host, "example.org");
            Ok(vec!["8.8.8.8".parse().unwrap()])
        })
    }
    fn connect(&self, host: String, ip: IpAddr) -> Pending<'_, Box<dyn Stream>> {
        Box::pin(async move {
            assert_eq!(host, "example.org");
            assert_eq!(ip, "8.8.8.8".parse::<IpAddr>().unwrap());
            Ok(Box::new(Memory {
                input: vec![],
                output: std::io::Cursor::new(vec![]),
                cases: self.cases.clone(),
            }) as Box<dyn Stream>)
        })
    }
}

#[tokio::test]
async fn ten_browser_cases_cross_python_and_rust_without_real_model_quality_claim() {
    let cases = serde_json::from_str(include_str!(
        "../../../contracts/benchmarks/chat-session/v0.1.0/workload.json"
    ))
    .unwrap();
    let reader = Arc::new(Reader::new(Arc::new(Peer { cases })));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = app_with_reader(addr, "synthetic-reader-key".into(), reader).unwrap();
    let (stop, closed) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = closed.await;
            })
            .await
            .unwrap()
    });
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local-tools-tests/reader_browser_fixture.py");
    let mut command = tokio::process::Command::new("python3");
    command
        .args(["-I", "-S"])
        .arg(script)
        .arg(addr.port().to_string())
        .env("SEIYA_READER_FIXTURE_KEY", "synthetic-reader-key")
        .kill_on_drop(true);
    let result = tokio::time::timeout(std::time::Duration::from_secs(30), command.output()).await;
    let _ = stop.send(());
    server.await.unwrap();
    let output = result.unwrap().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["cases"].as_array().unwrap().len(), 10);
    assert_eq!(report["synthetic_only"], true);
    for case in report["cases"].as_array().unwrap() {
        assert!(case["real_model_quality"].is_null());
    }
}
