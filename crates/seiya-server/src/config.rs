use crate::runtime::RuntimeConfig;
use serde::Deserialize;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;

pub const MAX_CONFIG_BYTES: u64 = 64 * 1024;

/// Only static, redacted messages may cross the CLI boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigError(pub &'static str);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ConfigError {}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    pub api_key_env: Option<String>,
    pub runtime: Option<RuntimeConfig>,
    pub queue_wait_ms: Option<u64>,
    pub request_read_timeout_ms: Option<u64>,
    #[serde(default)]
    pub runtime_status: bool,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let file = File::open(path).map_err(|_| ConfigError("cannot open configuration file"))?;
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ConfigError("cannot read configuration file"))?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, ConfigError> {
        if bytes.len() as u64 > MAX_CONFIG_BYTES {
            return Err(ConfigError("configuration file exceeds 65536 bytes"));
        }
        let config: Self = serde_json::from_slice(bytes)
            .map_err(|_| ConfigError("configuration JSON is invalid or has unsupported fields"))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.runtime_status && self.runtime.is_none() {
            return Err(ConfigError("runtime_status requires an explicit runtime"));
        }
        if self.listen.ip() != Ipv4Addr::LOCALHOST {
            return Err(ConfigError("listen must use 127.0.0.1"));
        }
        if let Some(runtime) = &self.runtime {
            runtime.validate()?;
            if runtime.backend == crate::runtime::Backend::OpenaiLocal {
                return Err(ConfigError("openai_local requires the owned catalogue host; plain API configuration is not an ownership grant"));
            }
            if self.listen.port() != 0 && runtime.endpoint()?.port() == Some(self.listen.port()) {
                return Err(ConfigError(
                    "runtime endpoint must not point to the frontend listener",
                ));
            }
        }
        if self
            .queue_wait_ms
            .is_some_and(|ms| !(1..=120000).contains(&ms))
            || self
                .request_read_timeout_ms
                .is_some_and(|ms| !(1..=30000).contains(&ms))
        {
            return Err(ConfigError(
                "queue wait or request read timeout is outside supported limits",
            ));
        }
        if let Some(name) = &self.api_key_env {
            let mut chars = name.chars();
            let valid_first = chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
            if name.len() > 128
                || !valid_first
                || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(ConfigError(
                    "api_key_env must be a valid environment variable name",
                ));
            }
        }
        Ok(())
    }

    pub fn resolve_api_key(
        &self,
        lookup: impl FnOnce(&str) -> Option<String>,
    ) -> Result<Option<String>, ConfigError> {
        self.validate()?;
        match &self.api_key_env {
            None => Ok(None),
            Some(name) => {
                let key = lookup(name).ok_or(ConfigError(
                    "configured API key environment variable is unavailable",
                ))?;
                validate_key(&key)?;
                Ok(Some(key))
            }
        }
    }
}

pub(crate) fn validate_key(key: &str) -> Result<(), ConfigError> {
    if key.is_empty() || key.len() > 4096 || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(ConfigError(
            "API key must contain 1–4096 visible ASCII characters without spaces",
        ));
    }
    Ok(())
}
