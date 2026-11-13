//! Explicit local backend transport. Never connects during construction.
use crate::config::{ConfigError, MAX_CONFIG_BYTES};
use reqwest::{redirect::Policy, Client, Url};
use serde::Deserialize;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

pub const MAX_RUNTIME_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub type RuntimeFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Value, TransportError>> + Send + 'a>>;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub id: String,
    pub model: String,
    pub backend: Backend,
    pub base_url: String,
    pub timeout_seconds: u64,
    pub max_output_tokens: u64,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Mlx,
}

impl RuntimeConfig {
    pub fn parse(bytes: &[u8]) -> Result<Self, ConfigError> {
        if bytes.len() as u64 > MAX_CONFIG_BYTES {
            return Err(ConfigError("runtime configuration exceeds 65536 bytes"));
        }
        let config: Self = serde_json::from_slice(bytes)
            .map_err(|_| ConfigError("runtime configuration is invalid or unsupported"))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.id.is_empty()
            || self.id.len() > 128
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(ConfigError(
                "runtime id must contain 1–128 letters, digits, underscores or hyphens",
            ));
        }
        if self.model.trim().is_empty()
            || self.model.len() > 4096
            || self.model.chars().any(char::is_control)
        {
            return Err(ConfigError("runtime model identifier is invalid"));
        }
        if !(1..=600).contains(&self.timeout_seconds)
            || !(1..=4096).contains(&self.max_output_tokens)
        {
            return Err(ConfigError(
                "runtime timeout or output budget is outside supported limits",
            ));
        }
        self.endpoint()?;
        Ok(())
    }

    pub fn endpoint(&self) -> Result<Url, ConfigError> {
        let invalid = || {
            ConfigError(
                "runtime endpoint must be HTTP at 127.0.0.1 with explicit port and /v1 path only",
            )
        };
        if self.base_url.trim() != self.base_url || self.base_url.chars().any(char::is_control) {
            return Err(invalid());
        }
        let mut url = Url::parse(&self.base_url).map_err(|_| invalid())?;
        // Require the canonical numeric authority too: URL parsing may normalize
        // alternative IPv4 spellings. No DNS, default port, userinfo or redirects.
        let port = url.port().ok_or_else(invalid)?;
        let prefix = format!("http://127.0.0.1:{port}");
        if url.scheme() != "http"
            || port == 0
            || url.host_str() != Some("127.0.0.1")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || ![format!("{prefix}/v1"), format!("{prefix}/v1/")].contains(&self.base_url)
        {
            return Err(invalid());
        }
        url.set_path("/v1/chat/completions");
        Ok(url)
    }
}

/// Redacted categories only. Never attach reqwest errors, URLs or backend bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// Send failed while establishing a connection, before dispatch. Never use
    /// this for response-body failure or ambiguous runtime completion.
    NotConnected,
    Timeout,
    Unavailable,
    HttpFailure,
    InvalidJson,
    ResponseTooLarge,
}

impl TransportError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Timeout => "local model endpoint failed: TimeoutError",
            Self::Unavailable | Self::NotConnected => "local model endpoint failed: URLError",
            Self::HttpFailure => "local model endpoint failed: HTTPError",
            Self::InvalidJson => "local model endpoint failed: ValueError",
            Self::ResponseTooLarge => "model response too large",
        }
    }
}

pub trait RuntimeTransport: Send + Sync {
    fn complete(&self, request: Value) -> RuntimeFuture<'_>;
}

pub struct HttpTransport {
    client: Client,
    endpoint: Url,
}

fn classify(error: reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::Timeout
    } else {
        TransportError::Unavailable
    }
}

fn classify_send(error: reqwest::Error) -> TransportError {
    if !error.is_timeout() && error.is_connect() {
        TransportError::NotConnected
    } else {
        classify(error)
    }
}

impl HttpTransport {
    pub fn new(config: &RuntimeConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        let timeout = Duration::from_secs(config.timeout_seconds);
        let client = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(timeout.min(Duration::from_secs(3)))
            .timeout(timeout)
            .build()
            .map_err(|_| ConfigError("cannot construct local runtime transport"))?;
        Ok(Self {
            client,
            endpoint: config.endpoint()?,
        })
    }
}

impl RuntimeTransport for HttpTransport {
    fn complete(&self, request: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            let mut response = self
                .client
                .post(self.endpoint.clone())
                .json(&request)
                .send()
                .await
                .map_err(classify_send)?;
            if !response.status().is_success() {
                return Err(TransportError::HttpFailure);
            }
            if response
                .content_length()
                .is_some_and(|size| size > MAX_RUNTIME_RESPONSE_BYTES as u64)
            {
                return Err(TransportError::ResponseTooLarge);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(classify)? {
                if chunk.len() > MAX_RUNTIME_RESPONSE_BYTES.saturating_sub(bytes.len()) {
                    return Err(TransportError::ResponseTooLarge);
                }
                bytes.extend_from_slice(&chunk);
            }
            serde_json::from_slice(&bytes).map_err(|_| TransportError::InvalidJson)
        })
    }
}
