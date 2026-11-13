//! Fixed-selection HTTPS download. No model load, proxy, retry, resume or overwrite.
use super::assets::{self, Asset, Selection, CHUNK};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_rustls::{rustls, TlsConnector};

struct Header {
    status: u16,
    fields: BTreeMap<String, String>,
}
async fn header<R: AsyncRead + Unpin>(stream: &mut BufReader<R>) -> Result<Header, String> {
    let mut bytes = Vec::new();
    loop {
        if bytes.len() >= 32768 {
            return Err("download_header_limit".into());
        }
        let mut byte = [0];
        stream
            .read_exact(&mut byte)
            .await
            .map_err(|_| "download_header_eof")?;
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "download_header_encoding")?;
    let mut lines = text.split("\r\n");
    let status = lines.next().ok_or("download_status")?;
    let parts: Vec<_> = status.split(' ').collect();
    if parts.len() < 2 || !matches!(parts[0], "HTTP/1.1" | "HTTP/1.0") || parts[1].len() != 3 {
        return Err("download_status".into());
    }
    let mut fields = BTreeMap::new();
    for line in lines.filter(|s| !s.is_empty()) {
        let (key, value) = line.split_once(':').ok_or("download_header_format")?;
        if key.is_empty()
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || value.bytes().any(|b| b < 32 && b != b'\t')
        {
            return Err("download_header_format".into());
        }
        if fields
            .insert(key.to_ascii_lowercase(), value.trim().into())
            .is_some()
        {
            return Err("download_duplicate_header".into());
        }
    }
    Ok(Header {
        status: parts[1].parse().map_err(|_| "download_status")?,
        fields,
    })
}
fn body_header(h: &Header, size: u64) -> Result<(), String> {
    if h.status != 200
        || h.fields.contains_key("transfer-encoding")
        || h.fields
            .get("content-encoding")
            .is_some_and(|s| s != "identity")
        || h.fields
            .get("content-length")
            .and_then(|s| s.parse::<u64>().ok())
            != Some(size)
    {
        return Err("download_status_encoding_or_size".into());
    }
    Ok(())
}
async fn copy_body<R: AsyncRead + Unpin>(
    stream: &mut R,
    output: &mut File,
    a: &Asset,
    mut headroom: impl FnMut(u64) -> Result<(), String>,
) -> Result<(), String> {
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = vec![0u8; CHUNK];
    loop {
        headroom(count)?;
        let n = stream
            .read(&mut buffer)
            .await
            .map_err(|_| "download_body_read")?;
        if n == 0 {
            break;
        }
        count = count.checked_add(n as u64).ok_or("download_overflow")?;
        if count > a.bytes {
            return Err("download_body_limit".into());
        }
        output
            .write_all(&buffer[..n])
            .map_err(|_| "download_body_write")?;
        hash.update(&buffer[..n]);
    }
    if count != a.bytes || format!("{:x}", hash.finalize()) != a.sha256 {
        return Err("download_body_size_or_hash".into());
    }
    output.sync_all().map_err(|_| "download_sync".into())
}
fn public(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => {
            let [a, b, c, _] = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_broadcast()
                && !v.is_documentation()
                && !v.is_unspecified()
                && !v.is_multicast()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 198 && (b == 18 || b == 19))
                && !(a == 192 && b == 0 && c == 0)
                && !(a == 192 && b == 88 && c == 99)
        }
        std::net::IpAddr::V6(v) => {
            let s = v.segments();
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}
async fn response(
    a: &Asset,
) -> Result<BufReader<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>, String> {
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| "download_tls_versions")?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let mut url = a.url().map_err(str::to_string)?;
    for _ in 0..6 {
        if !assets::allowed_url(&url) {
            return Err("download_url_policy".into());
        }
        let host = url.host_str().ok_or("download_host")?.to_owned();
        let addresses: Vec<_> = tokio::net::lookup_host((host.as_str(), 443))
            .await
            .map_err(|_| "download_dns")?
            .take(33)
            .collect();
        if addresses.is_empty() || addresses.len() > 32 || addresses.iter().any(|a| !public(a.ip()))
        {
            return Err("download_dns_policy".into());
        }
        let tcp = tokio::net::TcpStream::connect(addresses[0])
            .await
            .map_err(|_| "download_connect")?;
        let name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|_| "download_tls_name")?;
        let mut tls = connector
            .connect(name, tcp)
            .await
            .map_err(|_| "download_tls")?;
        let target = match url.query() {
            Some(q) => format!("{}?{q}", url.path()),
            None => url.path().to_owned(),
        };
        tls.write_all(format!("GET {target} HTTP/1.1\r\nHost: {host}\r\nAccept-Encoding: identity\r\nUser-Agent: Seiya-local-evaluation/0.1\r\nConnection: close\r\n\r\n").as_bytes()).await.map_err(|_|"download_request")?;
        let mut stream = BufReader::new(tls);
        let h = header(&mut stream).await?;
        if matches!(h.status, 301 | 302 | 303 | 307 | 308) {
            let location = h.fields.get("location").ok_or("download_redirect")?;
            url = url.join(location).map_err(|_| "download_redirect")?;
            continue;
        }
        body_header(&h, a.bytes)?;
        return Ok(stream);
    }
    Err("download_redirect_limit".into())
}

/// Explicitly called by the operator CLI only; tests never contact the network.
#[cfg(target_os = "macos")]
pub async fn download(root: &Path) -> Result<(), String> {
    use super::native;
    let s = Selection::selected().map_err(str::to_string)?;
    native::require_private_new(root)?;
    if native::disk_free(root.parent().ok_or("download_parent")?)?
        < s.total_asset_bytes + s.download_reserve_bytes
    {
        return Err("download_disk_reserve".into());
    }
    std::fs::create_dir(root).map_err(|_| "download_directory")?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| "download_mode")?;
    let mut remaining = s.total_asset_bytes;
    for a in &s.assets {
        let part = root.join(format!("{}.part", a.local_name));
        let final_path = root.join(&a.local_name);
        if final_path.symlink_metadata().is_ok() {
            return Err("download_existing_destination".into());
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)
            .map_err(|_| "download_partial_exists_or_open")?;
        let transfer = async {
            let mut stream = tokio::time::timeout(Duration::from_secs(45), response(a))
                .await
                .map_err(|_| "download_connect_deadline")??;
            copy_body(&mut stream, &mut output, a, |count| {
                if native::disk_free(root)? < remaining - count + s.download_reserve_bytes {
                    return Err("download_disk_reserve".into());
                }
                Ok(())
            })
            .await
        };
        tokio::time::timeout(Duration::from_secs(3600), transfer)
            .await
            .map_err(|_| "download_deadline")??;
        assets::verify(&part, a)?;
        // Atomic no-clobber publication; preserve the verified partial as evidence.
        std::fs::hard_link(&part, &final_path).map_err(|_| "download_publish_no_clobber")?;
        remaining -= a.bytes;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn strict_response_rejects_ambiguity_compression_and_size() {
        for raw in [
            "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Length: 3\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Encoding: gzip\r\n\r\n",
        ] {
            let h = header(&mut BufReader::new(raw.as_bytes())).await;
            assert!(h.and_then(|h| body_header(&h, 3)).is_err());
        }
        let h = header(&mut BufReader::new(
            &b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n"[..],
        ))
        .await
        .unwrap();
        assert!(body_header(&h, 3).is_ok());
    }
    #[test]
    fn rejects_nonpublic_addresses() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "169.254.169.254",
            "::1",
            "::ffff:127.0.0.1",
            "192.88.99.1",
            "2002:7f00:1::1",
            "3fff::1",
            "2001:db8::1",
        ] {
            assert!(!public(ip.parse().unwrap()));
        }
        assert!(public("1.1.1.1".parse().unwrap()));
    }
    #[tokio::test]
    async fn bounded_transfer_hash_readback_and_no_overwrite() {
        let root =
            std::env::temp_dir().join(format!("seiya-download-fixture-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let mut a = Selection::selected().unwrap().assets.remove(0);
        a.bytes = 3;
        a.sha256 = format!("{:x}", Sha256::digest(b"abc"));
        for (i, raw) in [b"abc".as_slice(), b"abcd", b"ab", b"bad"]
            .into_iter()
            .enumerate()
        {
            let path = root.join(i.to_string());
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            let result = copy_body(&mut &raw[..], &mut f, &a, |_| Ok(())).await;
            assert_eq!(result.is_ok(), i == 0);
            if i == 0 {
                assets::verify(&path, &a).unwrap();
            }
            assert!(OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .is_err());
        }
        let mut f = File::create(root.join("reserve")).unwrap();
        assert!(
            copy_body(&mut &b"abc"[..], &mut f, &a, |_| Err("disk_reserve".into()))
                .await
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
