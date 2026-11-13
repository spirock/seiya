//! Conservative public destination policy. DNS validation is separate from URL parsing.
use super::Error;
use reqwest::Url;
use std::net::IpAddr;

pub fn url(input: &str) -> Result<Url, Error> {
    if input.len() > 4096 || input.chars().any(|c| c.is_control() || c == '\\') {
        return Err(Error::Policy);
    }
    let mut parsed = Url::parse(input).map_err(|_| Error::Policy)?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port_or_known_default() != Some(443)
        || parsed.host_str().is_none()
    {
        return Err(Error::Policy);
    }
    // Even empty userinfo must not be accepted (the URL parser normalizes it).
    let authority = input
        .strip_prefix("https://")
        .ok_or(Error::Policy)?
        .split(['/', '?', '#'])
        .next()
        .ok_or(Error::Policy)?;
    if authority.contains('@') || authority.ends_with('.') {
        return Err(Error::Policy);
    }
    parsed.set_fragment(None);
    if let Some(host) = parsed.host_str() {
        let literal = host.trim_matches(['[', ']']);
        if let Ok(ip) = literal.parse() {
            if !public_ip(ip) {
                return Err(Error::Policy);
            }
        } else if !host.contains('.') || host.ends_with(".localhost") || host.ends_with(".local") {
            return Err(Error::Policy);
        }
    }
    Ok(parsed)
}

pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, c, _] = v.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 0 || b == 168 || (b == 88 && c == 99)))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            // Excludes mapped, NAT64, local, multicast, documentation, Teredo,
            // benchmarking, ORCHID, 6to4 and special protocol assignments.
            (s[0] & 0xe000 == 0x2000)
                && !(s[0] == 0x2001 && (s[1] < 0x0200 || s[1] == 0x0db8))
                && s[0] != 0x2002
                && s[0] != 0x3ffe
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}

pub fn addresses(ips: &[IpAddr]) -> Result<IpAddr, Error> {
    if ips.is_empty() || ips.len() > 32 || ips.iter().any(|ip| !public_ip(*ip)) {
        return Err(Error::Policy);
    }
    Ok(ips[0]) // No fallback/retry to another address.
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urls_and_alternate_literal_forms_fail_closed() {
        for s in [
            "http://example.org",
            "https://u:p@example.org",
            "https://@example.org",
            "https://example.org:444",
            "https://localhost",
            "https://127.1",
            "https://2130706433",
            "https://0x7f000001",
            "https://[::ffff:8.8.8.8]",
            "https://example.org\\@127.0.0.1",
            "https://example.org\n/x",
            "https://host.local",
            "https://example.org.",
        ] {
            assert_eq!(url(s), Err(Error::Policy), "{s}");
        }
        assert!(url("https://example.org/a?q=x#fragment")
            .unwrap()
            .fragment()
            .is_none());
    }
    #[test]
    fn every_dns_address_must_be_public() {
        for s in [
            "0.0.0.0",
            "10.1.2.3",
            "100.64.0.1",
            "169.254.169.254",
            "172.31.1.1",
            "192.168.1.1",
            "192.0.0.9",
            "198.18.0.1",
            "198.51.100.4",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "2001::1",
            "2002:808:808::1",
            "3fff::1",
            "64:ff9b::808:808",
        ] {
            assert!(!public_ip(s.parse().unwrap()), "{s}");
        }
        let public = "8.8.8.8".parse().unwrap();
        assert!(addresses(&[public]).is_ok());
        assert!(addresses(&[public, "127.0.0.1".parse().unwrap()]).is_err());
        assert!(public_ip("2606:4700:4700::1111".parse().unwrap()));
        assert!(addresses(&[]).is_err());
    }
}
