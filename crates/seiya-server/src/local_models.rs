//! Versioned local model catalogue. No network/process/model work while parsing.
use crate::{
    config::ConfigError,
    runtime::{Backend, RuntimeConfig},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(crate) mod desktop;
mod images;
#[cfg(test)]
pub(crate) use images::fixture_png;
pub(crate) mod ollama;
mod pool;
mod resources;
pub(crate) mod storage;
pub use desktop::check_file;
pub(crate) use pool::Pool;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub runtime: RuntimeConfig,
    pub supports_tools: bool,
    pub max_input_bytes: usize,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImageModel {
    pub id: String,
    pub runtime: RuntimeConfig,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Assignments {
    pub brain: String,
    pub workers: Vec<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub models: Vec<Model>,
    pub assignments: Assignments,
    pub minimum_available_mib: u64,
    #[serde(default)]
    pub web_search: bool,
    #[serde(default)]
    pub image_models: Vec<ImageModel>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub image_chat: bool,
}
impl Catalog {
    pub fn parse(bytes: &[u8]) -> Result<Self, ConfigError> {
        if bytes.len() > 65536 {
            return Err(ConfigError("catalog_too_large"));
        }
        let value: Self =
            serde_json::from_slice(bytes).map_err(|_| ConfigError("catalog_invalid_schema"))?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.version != 1
            || self.models.is_empty()
            || self.models.len() > 32
            || !(256..=1048576).contains(&self.minimum_available_mib)
        {
            return Err(ConfigError("catalog_version_or_limits"));
        }
        let mut ids = HashSet::new();
        let mut bindings = HashSet::new();
        for model in &self.models {
            if !identifier(&model.id)
                || !ids.insert(&model.id)
                || model.runtime.id != model.id
                || model.runtime.backend != Backend::OpenaiLocal
                || !(1024..=131072).contains(&model.max_input_bytes)
            {
                return Err(ConfigError("catalog_model_invalid_or_duplicate"));
            }
            model.runtime.validate()?;
            let endpoint = model.runtime.endpoint()?;
            if matches!(endpoint.port(), Some(8015 | 8016 | 11434 | 49399 | 49400)) {
                return Err(ConfigError("catalog_reserved_endpoint"));
            }
            if !bindings.insert((
                model.runtime.base_url.trim_end_matches('/'),
                &model.runtime.model,
            )) {
                return Err(ConfigError("catalog_duplicate_binding"));
            }
        }
        if self.image_models.len() > 16 {
            return Err(ConfigError("image_catalog_limit"));
        }
        for model in &self.image_models {
            if !identifier(&model.id)
                || !ids.insert(&model.id)
                || model.runtime.id != model.id
                || model.runtime.backend != Backend::OpenaiLocal
            {
                return Err(ConfigError("image_model_invalid_or_duplicate"));
            }
            model.runtime.validate()?;
            if matches!(
                model.runtime.endpoint()?.port(),
                Some(8015 | 8016 | 11434 | 49399 | 49400)
            ) {
                return Err(ConfigError("catalog_reserved_endpoint"));
            }
            if !bindings.insert((
                model.runtime.base_url.trim_end_matches('/'),
                &model.runtime.model,
            )) {
                return Err(ConfigError("catalog_duplicate_binding"));
            }
        }
        if let Some(id) = &self.image {
            self.image_model(id)?;
        }
        if self.image_chat
            && (self.image.is_none()
                || !self.model(&self.assignments.brain)?.supports_tools
                || self.model(&self.assignments.brain)?.max_input_bytes < 8192)
        {
            return Err(ConfigError(
                "image_chat_requires_image_tools_and_8192_byte_budget",
            ));
        }
        self.validate_assignments(&self.assignments)
    }
    pub fn model(&self, id: &str) -> Result<&Model, ConfigError> {
        self.models
            .iter()
            .find(|m| m.id == id)
            .ok_or(ConfigError("assigned_model_not_registered"))
    }
    pub fn image_model(&self, id: &str) -> Result<&ImageModel, ConfigError> {
        self.image_models
            .iter()
            .find(|m| m.id == id)
            .ok_or(ConfigError("assigned_image_not_registered"))
    }
    pub fn validate_assignments(&self, roles: &Assignments) -> Result<(), ConfigError> {
        self.model(&roles.brain)?;
        if self.web_search
            && (!self.model(&roles.brain)?.supports_tools
                || self.model(&roles.brain)?.max_input_bytes < 8192)
        {
            return Err(ConfigError(
                "web_search_requires_tools_and_8192_byte_input_budget",
            ));
        }
        if roles.workers.len() > 16 {
            return Err(ConfigError("worker_pool_too_large"));
        }
        let mut ids = HashSet::new();
        for id in &roles.workers {
            self.model(id)?;
            if id == &roles.brain || !ids.insert(id) {
                return Err(ConfigError("worker_assignment_duplicate"));
            }
        }
        if !roles.workers.is_empty() && !self.model(&roles.brain)?.supports_tools {
            return Err(ConfigError("brain_tools_capability_required_for_workers"));
        }
        Ok(())
    }
}
fn identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

#[cfg(test)]
mod tests;
