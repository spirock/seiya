//! Deliberately strict HTTP/1 reader. No implicit decompression or redirects.
use super::Error;
use tokio::io::{AsyncRead, AsyncReadExt};

pub(super) const BODY_CAP: usize = 1024 * 1024;
const HEADER_CAP: usize = 64 * 1024;

#[derive(Default, Debug)]
pub(super) struct Budget {
    pub received: usize,
    pub decoded: usize,
    pub headers: usize,
}

pub(super) struct Reply {
    pub status: u16,
    pub location: Option<String>,
    pub mime: Option<String>,
    pub body: Vec<u8>,
}

async fn line<R: AsyncRead + Unpin>(
    r: &mut R,
    cap: usize,
    received: &mut usize,
) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    while bytes.len() < cap {
        let b = r.read_u8().await.map_err(|_| Error::Transport)?;
        *received += 1;
        if *received > BODY_CAP {
            return Err(Error::Limit);
        }
        bytes.push(b);
        if bytes.ends_with(b"\r\n") {
            return Ok(bytes);
        }
        if b == b'\n' {
            return Err(Error::Format);
        }
    }
    Err(Error::Limit)
}

async fn data<R: AsyncRead + Unpin>(
    r: &mut R,
    n: usize,
    budget: &mut Budget,
    body: &mut Vec<u8>,
) -> Result<(), Error> {
    if n > BODY_CAP - budget.received {
        return Err(Error::Limit);
    }
    let mut remaining = n;
    let mut buf = [0u8; 8192];
    while remaining > 0 {
        let cap = remaining.min(buf.len());
        let read = r
            .read(&mut buf[..cap])
            .await
            .map_err(|_| Error::Transport)?;
        if read == 0 {
            return Err(Error::Transport);
        }
        budget.received += read;
        budget.decoded += read;
        body.extend_from_slice(&buf[..read]);
        remaining -= read;
    }
    Ok(())
}

pub(super) async fn response<R: AsyncRead + Unpin>(
    r: &mut R,
    budget: &mut Budget,
) -> Result<Reply, Error> {
    let mut raw = Vec::new();
    while !raw.ends_with(b"\r\n\r\n") {
        if raw.len() == HEADER_CAP {
            return Err(Error::Limit);
        }
        raw.push(r.read_u8().await.map_err(|_| Error::Transport)?);
        budget.headers += 1;
        if budget.headers > 9 * HEADER_CAP {
            return Err(Error::Limit);
        }
    }
    let text = std::str::from_utf8(&raw).map_err(|_| Error::Format)?;
    if !text.is_ascii() {
        return Err(Error::Format);
    }
    let mut lines = text.split("\r\n");
    let status_line = lines.next().ok_or(Error::Format)?;
    if status_line.bytes().any(|b| b < 32 || b == 127) {
        return Err(Error::Format);
    }
    let mut fields = status_line.splitn(3, ' ');
    if !matches!(fields.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
        return Err(Error::Format);
    }
    let status = fields.next().ok_or(Error::Format)?;
    if status.len() != 3 || !status.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Format);
    }
    let status: u16 = status.parse().map_err(|_| Error::Format)?;
    let mut headers = std::collections::HashMap::new();
    for row in lines {
        if row.is_empty() {
            continue;
        }
        let (key, value) = row.split_once(':').ok_or(Error::Format)?;
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            || value.bytes().any(|b| b < 32 && b != b'\t' || b == 127)
        {
            return Err(Error::Format);
        }
        if headers
            .insert(key.to_ascii_lowercase(), value.trim().to_string())
            .is_some()
        {
            return Err(Error::Format);
        }
    }
    if headers
        .get("content-encoding")
        .is_some_and(|v| !v.eq_ignore_ascii_case("identity"))
    {
        return Err(Error::Format);
    }
    let length = headers.get("content-length");
    let transfer = headers.get("transfer-encoding");
    if length.is_some() && transfer.is_some() {
        return Err(Error::Format);
    }
    let mut body = Vec::new();
    if let Some(value) = length {
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::Format);
        }
        let n: usize = value.parse().map_err(|_| Error::Limit)?;
        data(r, n, budget, &mut body).await?;
    } else if let Some(value) = transfer {
        if !value.eq_ignore_ascii_case("chunked") {
            return Err(Error::Format);
        }
        loop {
            let row = line(r, 128, &mut budget.received).await?;
            if budget.received > BODY_CAP {
                return Err(Error::Limit);
            }
            let digits = &row[..row.len() - 2];
            if digits.is_empty() || !digits.iter().all(u8::is_ascii_hexdigit) {
                return Err(Error::Format);
            }
            let n =
                usize::from_str_radix(std::str::from_utf8(digits).map_err(|_| Error::Format)?, 16)
                    .map_err(|_| Error::Limit)?;
            if n == 0 {
                let _end = line(r, 2, &mut budget.received).await?;
                if budget.received > BODY_CAP {
                    return Err(Error::Limit);
                }
                break;
            }
            data(r, n, budget, &mut body).await?;
            if line(r, 2, &mut budget.received).await? != b"\r\n" {
                return Err(Error::Format);
            }
            if budget.received > BODY_CAP {
                return Err(Error::Limit);
            }
        }
    } else {
        let mut buf = [0u8; 8192];
        loop {
            let cap = (BODY_CAP - budget.received + 1).min(buf.len());
            let n = r
                .read(&mut buf[..cap])
                .await
                .map_err(|_| Error::Transport)?;
            budget.received += n;
            if budget.received > BODY_CAP {
                return Err(Error::Limit);
            }
            budget.decoded += n;
            body.extend_from_slice(&buf[..n]);
            if n == 0 {
                break;
            }
        }
    }
    Ok(Reply {
        status,
        location: headers.remove("location"),
        mime: headers.remove("content-type"),
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn bounded_plain_and_chunked_frames() {
        for raw in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc".as_slice(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n",
        ] {
            let mut budget = Budget::default();
            let reply = response(&mut &raw[..], &mut budget).await.unwrap();
            assert_eq!(reply.body, b"abc");
            assert_eq!(budget.decoded, 3);
            assert!(budget.received >= 3);
        }
    }
    #[tokio::test]
    async fn ambiguity_compression_truncation_and_limits_are_closed() {
        for raw in [
            "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\na",
            "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\na",
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 1048577\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\na",
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3;ext\r\nabc\r\n0\r\n\r\n",
        ] {
            assert!(response(&mut raw.as_bytes(), &mut Budget::default())
                .await
                .is_err());
        }
        let raw = format!("HTTP/1.1 200 OK\r\nX: {}", "a".repeat(65536));
        assert!(matches!(
            response(&mut raw.as_bytes(), &mut Budget::default()).await,
            Err(Error::Limit)
        ));
    }
}
