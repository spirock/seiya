//! Explicit, opt-in GitHub stable release lookup. Never installs or opens URLs.
use seiya_contracts::update_version::{newer_stable, StableVersion};
use serde::Deserialize;
use std::time::{Duration, Instant};

const ENDPOINT: &str = "https://api.github.com/repos/spirock/seiya/releases/latest";
const PREFIX: &str = "https://github.com/spirock/seiya/releases/tag/v";
const MAX_BODY: usize = 64 * 1024;
const MIN_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Per-instance permission and throttle; no persistence or background task.
#[derive(Default)]
pub struct UpdateChecker {
    enabled: bool,
    last_attempt: Option<Instant>,
}

impl UpdateChecker {
    /// Call only after the user grants or revokes permission for this instance.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn reserve(&mut self, now: Instant) -> Result<(), &'static str> {
        if !self.enabled {
            return Err("update_consent_required");
        }
        if let Some(last) = self.last_attempt {
            if now
                .checked_duration_since(last)
                .is_none_or(|elapsed| elapsed < MIN_INTERVAL)
            {
                return Err("update_check_rate_limited");
            }
        }
        self.last_attempt = Some(now);
        Ok(())
    }

    /// Reserve before network I/O, including failures; no automatic retries.
    pub async fn check_stable(&mut self, current: &str) -> Result<Option<Update>, &'static str> {
        StableVersion::parse(current)?;
        self.reserve(Instant::now())?;
        fetch_stable(current).await
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Update {
    pub version: String,
    pub release_url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
}

fn parse_release(bytes: &[u8], current: &str) -> Result<Option<Update>, &'static str> {
    if bytes.len() > MAX_BODY {
        return Err("update_response_too_large");
    }
    let release: Release = serde_json::from_slice(bytes).map_err(|_| "invalid_update_response")?;
    let version = release
        .tag_name
        .strip_prefix('v')
        .ok_or("invalid_update_response")?;
    if release.draft
        || release.prerelease
        || StableVersion::parse(version).is_err()
        || release.html_url != format!("{PREFIX}{version}")
    {
        return Err("invalid_update_response");
    }
    if !newer_stable(current, version)? {
        return Ok(None);
    }
    Ok(Some(Update {
        version: version.to_owned(),
        release_url: release.html_url,
    }))
}

// No cookies, credentials or application user data; GitHub receives the client IP.
async fn fetch_stable(current: &str) -> Result<Option<Update>, &'static str> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .user_agent("Seiya-update-check")
        .build()
        .map_err(|_| "update_network_unavailable")?;
    let mut response = client
        .get(ENDPOINT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| "update_network_unavailable")?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|v| v > MAX_BODY as u64)
    {
        return Err("update_response_unavailable");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "update_network_unavailable")?
    {
        if chunk.len() > MAX_BODY - body.len() {
            return Err("update_response_too_large");
        }
        body.extend_from_slice(&chunk);
    }
    parse_release(&body, current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release() -> serde_json::Value {
        json!({"tag_name":"v1.2.3","html_url":"https://github.com/spirock/seiya/releases/tag/v1.2.3","draft":false,"prerelease":false,"description":"untrusted"})
    }

    #[test]
    fn consent_and_daily_throttle_apply_even_after_reenable_or_clock_regression() {
        let mut checker = UpdateChecker::default();
        let now = Instant::now();
        assert_eq!(checker.reserve(now), Err("update_consent_required"));
        checker.set_enabled(true);
        assert_eq!(checker.reserve(now), Ok(()));
        assert_eq!(checker.reserve(now), Err("update_check_rate_limited"));
        checker.set_enabled(false);
        assert_eq!(
            checker.reserve(now + MIN_INTERVAL),
            Err("update_consent_required")
        );
        checker.set_enabled(true);
        assert_eq!(checker.reserve(now), Err("update_check_rate_limited"));
        assert_eq!(
            checker.reserve(now + MIN_INTERVAL - Duration::from_secs(1)),
            Err("update_check_rate_limited")
        );
        assert_eq!(checker.reserve(now + MIN_INTERVAL), Ok(()));
        assert_eq!(checker.reserve(now), Err("update_check_rate_limited"));
    }

    #[test]
    fn stable_version_comparison_never_offers_a_downgrade() {
        let b = serde_json::to_vec(&release()).unwrap();
        assert_eq!(
            parse_release(&b, "1.2.2").unwrap().unwrap().version,
            "1.2.3"
        );
        assert_eq!(parse_release(&b, "1.2.3").unwrap(), None);
        assert_eq!(parse_release(&b, "1.3.0").unwrap(), None);
        assert!(parse_release(&b, "1.2.3-rc.1").is_err());
    }

    #[test]
    fn rejects_drafts_prereleases_untrusted_links_and_oversize() {
        for (key, value) in [
            ("draft", json!(true)),
            ("prerelease", json!(true)),
            ("tag_name", json!("v1.2.3-rc.1")),
            (
                "html_url",
                json!("https://github.com.evil.invalid/spirock/seiya/releases/tag/v1.2.3"),
            ),
            (
                "html_url",
                json!("https://github.com/spirock/seiya/releases/tag/v1.2.3?x=1"),
            ),
        ] {
            let mut data = release();
            data[key] = value;
            assert!(parse_release(&serde_json::to_vec(&data).unwrap(), "1.0.0").is_err());
        }
        assert!(parse_release(&vec![0; MAX_BODY + 1], "1.0.0").is_err());
        assert!(parse_release(br#"{"tag_name":"v1.2.3","tag_name":"v1.2.3"}"#, "1.0.0").is_err());
    }
}
