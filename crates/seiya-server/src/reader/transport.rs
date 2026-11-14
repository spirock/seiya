//! Explicit public-IP TLS connections. Construction never performs network I/O.
use super::{policy, wire, Error};
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout_at, Duration, Instant};
use tokio_rustls::{rustls, TlsConnector};

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'a>>;

/// Trusted host implementation, not supplied by pages or models. Connect must
/// honor the IP without re-resolving host and validate TLS against host.
pub trait Network: Send + Sync {
    fn resolve(&self, host: String) -> Pending<'_, Vec<IpAddr>>;
    fn connect(&self, host: String, ip: IpAddr) -> Pending<'_, Box<dyn Stream>>;
}

pub struct HttpsNetwork {
    tls: TlsConnector,
}

impl Default for HttpsNetwork {
    fn default() -> Self {
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Self {
            tls: TlsConnector::from(Arc::new(tls_config(roots))),
        }
    }
}

fn tls_config(roots: rustls::RootCertStore) -> rustls::ClientConfig {
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("supported TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

impl Network for HttpsNetwork {
    fn resolve(&self, host: String) -> Pending<'_, Vec<IpAddr>> {
        Box::pin(async move {
            // System DNS can outlive cancellation in its blocking worker.
            // The reader owner must quarantine if this future is interrupted.
            let mut ips = Vec::new();
            for addr in tokio::net::lookup_host((host.as_str(), 443))
                .await
                .map_err(|_| Error::Transport)?
            {
                if !ips.contains(&addr.ip()) {
                    ips.push(addr.ip());
                }
                if ips.len() > 32 {
                    return Err(Error::Policy);
                }
            }
            Ok(ips)
        })
    }
    fn connect(&self, host: String, ip: IpAddr) -> Pending<'_, Box<dyn Stream>> {
        Box::pin(async move {
            if !policy::public_ip(ip) {
                return Err(Error::Policy);
            }
            let name = rustls::pki_types::ServerName::try_from(host).map_err(|_| Error::Policy)?;
            let stream = TcpStream::connect(SocketAddr::new(ip, 443))
                .await
                .map_err(|_| Error::Transport)?;
            let tls = self
                .tls
                .connect(name, stream)
                .await
                .map_err(|_| Error::Transport)?;
            Ok(Box::new(tls) as Box<dyn Stream>)
        })
    }
}

pub struct Page {
    pub body: Vec<u8>,
    pub mime: String,
    pub received_bytes: usize,
    pub decoded_bytes: usize,
    pub header_bytes: usize,
    pub get_count: usize,
}

/// Counters delivered to the parser, including partial failed responses, not
/// ciphertext, kernel traffic, TLS read-ahead, or a measurement of all wire bytes.
#[derive(Default)]
pub struct Counters {
    pub received_bytes: usize,
    pub decoded_bytes: usize,
    pub header_bytes: usize,
    pub get_count: usize,
}

pub struct Report {
    pub result: Result<Page, Error>,
    pub counters: Counters,
}

/// Caller owns the deadline and cancellation lifetime. No pooling, auth,
/// cookies, proxy environment, retries, implicit DNS or redirects.
pub async fn fetch(network: &dyn Network, input: &str, deadline: Instant) -> Result<Page, Error> {
    fetch_report(network, input, deadline).await.result
}

pub async fn fetch_report(network: &dyn Network, input: &str, deadline: Instant) -> Report {
    fetch_report_with_redirect_limit(network, input, deadline, 2).await
}
pub async fn fetch_report_with_redirect_limit(
    network: &dyn Network,
    input: &str,
    deadline: Instant,
    max_redirects: usize,
) -> Report {
    let deadline = deadline.min(Instant::now() + Duration::from_secs(45));
    let mut budget = wire::Budget::default();
    let mut get_count = 0;
    let result = timeout_at(
        deadline,
        fetch_inner(
            network,
            input,
            deadline,
            &mut budget,
            &mut get_count,
            max_redirects.min(2),
        ),
    )
    .await
    .unwrap_or(Err(Error::Timeout));
    Report {
        result,
        counters: Counters {
            received_bytes: budget.received,
            decoded_bytes: budget.decoded,
            header_bytes: budget.headers,
            get_count,
        },
    }
}

async fn fetch_inner(
    network: &dyn Network,
    input: &str,
    deadline: Instant,
    budget: &mut wire::Budget,
    get_count: &mut usize,
    max_redirects: usize,
) -> Result<Page, Error> {
    let mut url = policy::url(input)?;
    for count in 1..=max_redirects + 1 {
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        let host = url
            .host_str()
            .ok_or(Error::Policy)?
            .trim_matches(['[', ']'])
            .to_string();
        let addresses = timeout_at(
            deadline.min(Instant::now() + Duration::from_secs(3)),
            network.resolve(host.clone()),
        )
        .await
        .map_err(|_| Error::Timeout)??;
        let ip = policy::addresses(&addresses)?;
        let mut stream = timeout_at(
            deadline.min(Instant::now() + Duration::from_secs(3)),
            network.connect(host, ip),
        )
        .await
        .map_err(|_| Error::Timeout)??;
        let target = match url.query() {
            Some(query) => format!("{}?{query}", url.path()),
            None => url.path().to_string(),
        };
        let request = format!("GET {target} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Seiya/0.1\r\nAccept: text/html, text/plain\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n", url.host_str().ok_or(Error::Policy)?);
        let page_deadline = deadline.min(Instant::now() + Duration::from_secs(10));
        *get_count = count;
        timeout_at(page_deadline, stream.write_all(request.as_bytes()))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Transport)?;
        let reply = timeout_at(page_deadline, wire::response(&mut stream, budget))
            .await
            .map_err(|_| Error::Timeout)??;
        drop(stream);
        if matches!(reply.status, 301 | 302 | 303 | 307 | 308) {
            if count == max_redirects + 1 {
                return Err(Error::Limit);
            }
            let location = reply.location.ok_or(Error::Format)?;
            if location
                .chars()
                .any(|c| c.is_control() || c == '\\' || c == '@')
            {
                return Err(Error::Policy);
            }
            let target = url.join(&location).map_err(|_| Error::Policy)?;
            url = policy::url(target.as_str())?;
            continue;
        }
        if reply.status != 200 {
            return Err(Error::Http);
        }
        let mime = reply.mime.ok_or(Error::Format)?;
        let mut parts = mime.split(';').map(str::trim);
        let kind = parts.next().unwrap_or("").to_ascii_lowercase();
        if !matches!(kind.as_str(), "text/html" | "text/plain") {
            return Err(Error::Format);
        }
        for param in parts {
            if !param.eq_ignore_ascii_case("charset=utf-8")
                && !param.eq_ignore_ascii_case("charset=\"utf-8\"")
            {
                return Err(Error::Format);
            }
        }
        std::str::from_utf8(&reply.body).map_err(|_| Error::Format)?;
        return Ok(Page {
            body: reply.body,
            mime: kind,
            received_bytes: budget.received,
            decoded_bytes: budget.decoded,
            header_bytes: budget.headers,
            get_count: count,
        });
    }
    Err(Error::Limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::task::{Context, Poll};
    use tokio::io::ReadBuf;

    struct MemoryStream(std::io::Cursor<Vec<u8>>);
    impl AsyncRead for MemoryStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let pos = self.0.position() as usize;
            let end = (pos + buf.remaining().min(7)).min(self.0.get_ref().len());
            buf.put_slice(&self.0.get_ref()[pos..end]);
            self.0.set_position(end as u64);
            Poll::Ready(Ok(()))
        }
    }
    impl AsyncWrite for MemoryStream {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            data: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            assert!(data.starts_with(b"GET /"));
            assert!(std::str::from_utf8(data)
                .unwrap()
                .contains("Accept-Encoding: identity"));
            Poll::Ready(Ok(data.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    struct Fake {
        resolutions: Mutex<VecDeque<Vec<IpAddr>>>,
        replies: Mutex<VecDeque<Vec<u8>>>,
        targets: Mutex<Vec<(String, IpAddr)>>,
    }
    impl Fake {
        fn new(resolutions: &[&[&str]], replies: &[&str]) -> Self {
            Self {
                resolutions: Mutex::new(
                    resolutions
                        .iter()
                        .map(|v| v.iter().map(|s| s.parse().unwrap()).collect())
                        .collect(),
                ),
                replies: Mutex::new(replies.iter().map(|s| s.as_bytes().to_vec()).collect()),
                targets: Mutex::new(vec![]),
            }
        }
    }
    impl Network for Fake {
        fn resolve(&self, _: String) -> Pending<'_, Vec<IpAddr>> {
            Box::pin(async { Ok(self.resolutions.lock().unwrap().pop_front().unwrap()) })
        }
        fn connect(&self, host: String, ip: IpAddr) -> Pending<'_, Box<dyn Stream>> {
            Box::pin(async move {
                self.targets.lock().unwrap().push((host, ip));
                Ok(Box::new(MemoryStream(std::io::Cursor::new(
                    self.replies.lock().unwrap().pop_front().unwrap(),
                ))) as Box<dyn Stream>)
            })
        }
    }
    const OK: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 3\r\n\r\nabc";
    #[tokio::test]
    async fn search_policy_zero_redirects_never_contacts_a_second_host() {
        let redirect="HTTP/1.1 302 Found\r\nLocation: https://other.example.org/search\r\nContent-Length: 0\r\n\r\n";
        let fake = Fake::new(&[&["8.8.8.8"], &["8.8.4.4"]], &[redirect]);
        let report = fetch_report_with_redirect_limit(
            &fake,
            "https://html.duckduckgo.com/html/?q=synthetic",
            Instant::now() + Duration::from_secs(1),
            0,
        )
        .await;
        assert!(matches!(report.result, Err(Error::Limit)));
        assert_eq!(report.counters.get_count, 1);
        assert_eq!(fake.targets.lock().unwrap().len(), 1);
        assert_eq!(fake.resolutions.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn tls_memory_handshake_checks_trust_and_hostname() {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, UnixTime};
        fn hex(s: &str) -> Vec<u8> {
            s.trim()
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
                .collect()
        }
        #[derive(Debug)]
        struct FixedTime;
        impl rustls::time_provider::TimeProvider for FixedTime {
            fn current_time(&self) -> Option<UnixTime> {
                Some(UnixTime::since_unix_epoch(Duration::from_secs(
                    1_798_761_600,
                )))
            }
        }
        let cert = CertificateDer::from(hex(include_str!(
            "../../tests/fixtures/reader-test-cert.der.hex"
        )));
        let key_bytes = hex(include_str!("../../tests/fixtures/reader-test-key.der.hex"));
        for (trusted, hostname, expected) in [
            (true, "example.org", true),
            (true, "wrong.example", false),
            (false, "example.org", false),
        ] {
            let mut roots = rustls::RootCertStore::empty();
            if trusted {
                roots.add(cert.clone()).unwrap();
            }
            let mut config = tls_config(roots);
            config.time_provider = Arc::new(FixedTime);
            let client = TlsConnector::from(Arc::new(config));
            let server = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_bytes.clone())),
            )
            .unwrap();
            let server = tokio_rustls::TlsAcceptor::from(Arc::new(server));
            let (left, right) = tokio::io::duplex(8192);
            let handshakes = async {
                tokio::join!(
                    client.connect(
                        rustls::pki_types::ServerName::try_from(hostname).unwrap(),
                        left
                    ),
                    server.accept(right)
                )
            };
            let (client_result, _) = tokio::time::timeout(Duration::from_secs(2), handshakes)
                .await
                .unwrap();
            assert_eq!(client_result.is_ok(), expected, "{trusted} {hostname}");
        }
        // The synthetic trust anchor is confined to this test, never production.
        let _production = HttpsNetwork::default();
    }

    #[tokio::test]
    async fn pinning_preserves_hostname_and_never_reresolves_for_connect() {
        let fake = Fake::new(&[&["8.8.8.8"], &["127.0.0.1"]], &[OK]);
        let page = fetch(
            &fake,
            "https://example.org/a",
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert_eq!(page.body, b"abc");
        assert_eq!(
            *fake.targets.lock().unwrap(),
            vec![("example.org".into(), "8.8.8.8".parse().unwrap())]
        );
        assert_eq!(fake.resolutions.lock().unwrap().len(), 1);
        assert_eq!(page.get_count, 1);
    }

    #[tokio::test]
    async fn redirects_revalidate_and_count_bodies_without_retry() {
        let redirect =
            "HTTP/1.1 302 Found\r\nLocation: https://example.org/b\r\nContent-Length: 1\r\n\r\nx";
        let fake = Fake::new(&[&["8.8.8.8"], &["127.0.0.1"]], &[redirect]);
        assert!(matches!(
            fetch(
                &fake,
                "https://example.org/a",
                Instant::now() + Duration::from_secs(1)
            )
            .await,
            Err(Error::Policy)
        ));
        assert_eq!(fake.targets.lock().unwrap().len(), 1);
        let fake = Fake::new(&[&["8.8.8.8"], &["8.8.4.4"]], &[redirect, OK]);
        let page = fetch(
            &fake,
            "https://example.org/a",
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert_eq!(page.received_bytes, 4);
        assert_eq!(page.get_count, 2);
        assert!(page.header_bytes > OK.find("\r\n\r\n").unwrap());
    }

    #[tokio::test]
    async fn mixed_dns_and_expired_deadline_never_connect() {
        let fake = Fake::new(&[&["8.8.8.8", "10.0.0.1"]], &[]);
        assert!(matches!(
            fetch(
                &fake,
                "https://example.org/a",
                Instant::now() + Duration::from_secs(1)
            )
            .await,
            Err(Error::Policy)
        ));
        assert!(fake.targets.lock().unwrap().is_empty());
        let fake = Fake::new(&[], &[]);
        assert!(matches!(
            fetch(&fake, "https://example.org/a", Instant::now()).await,
            Err(Error::Timeout)
        ));
    }

    #[tokio::test]
    async fn failures_preserve_observed_counters_and_redirect_caps() {
        let truncated = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 3\r\n\r\na";
        let fake = Fake::new(&[&["8.8.8.8"]], &[truncated]);
        let report = fetch_report(
            &fake,
            "https://example.org",
            Instant::now() + Duration::from_secs(1),
        )
        .await;
        assert!(matches!(report.result, Err(Error::Transport)));
        assert_eq!(report.counters.received_bytes, 1);
        assert_eq!(report.counters.get_count, 1);
        assert!(report.counters.header_bytes > 0);
        let redirect = "HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\n\r\n";
        let fake = Fake::new(
            &[&["8.8.8.8"], &["8.8.8.8"], &["8.8.8.8"]],
            &[redirect, redirect, redirect],
        );
        let report = fetch_report(
            &fake,
            "https://example.org",
            Instant::now() + Duration::from_secs(1),
        )
        .await;
        assert!(matches!(report.result, Err(Error::Limit)));
        assert_eq!(report.counters.get_count, 3);
        assert_eq!(fake.targets.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn redirect_downgrade_metadata_mime_and_encoding_never_fallback() {
        for location in [
            "http://example.org/",
            "https://169.254.169.254/",
            "https://@example.org/",
            "https://example.org:444/",
        ] {
            let reply =
                format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n");
            let fake = Fake::new(&[&["8.8.8.8"]], &[&reply]);
            assert!(matches!(
                fetch(
                    &fake,
                    "https://example.org",
                    Instant::now() + Duration::from_secs(1)
                )
                .await,
                Err(Error::Policy)
            ));
            assert_eq!(fake.targets.lock().unwrap().len(), 1);
        }
        for mime in [
            "application/pdf",
            "text/plain; charset=iso-8859-1",
            "text/html; charset=utf-16",
        ] {
            let reply =
                format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: 3\r\n\r\nabc");
            let fake = Fake::new(&[&["8.8.8.8"]], &[&reply]);
            assert!(matches!(
                fetch(
                    &fake,
                    "https://example.org",
                    Instant::now() + Duration::from_secs(1)
                )
                .await,
                Err(Error::Format)
            ));
        }
    }

    #[tokio::test]
    async fn batch_extraction_never_exceeds_unicode_total_or_returns_active_html() {
        let body = format!(
            "<main><p>{}</p><script>NEVER EXECUTE</script></main>",
            "界".repeat(22000)
        );
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let fake = Arc::new(Fake::new(
            &[&["8.8.8.8"], &["8.8.8.8"], &["8.8.8.8"]],
            &[&reply, &reply, &reply],
        ));
        let reader = crate::reader::Reader::new(fake);
        let request = serde_json::json!({"version":"0.1-draft.1","run_id":"extract-run","remaining_ms":12000,
            "sources":[{"id":"a","url":"https://example.org/a"},{"id":"b","url":"https://example.org/b"},{"id":"c","url":"https://example.org/c"}]}).to_string();
        let request =
            seiya_contracts::reader_frame::parse_request(request.as_bytes(), true).unwrap();
        let response = reader
            .read_batch(&request, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            response["results"][0]["text"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            20000
        );
        assert_eq!(
            response["results"][1]["text"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            10000
        );
        assert_eq!(response["results"][1]["text_truncated"], true);
        assert_eq!(response["results"][2]["status"], "limit_exceeded");
        assert_eq!(response["results"][2]["text"], serde_json::Value::Null);
        assert!(!response.to_string().contains("<script"));
    }
}
