//! Draft contracts only. This crate does not execute tools or provide a sandbox.
use serde::{Deserialize, Serialize};

pub mod image_job;
/// Experimental reader framing; not a network broker or an advertised tool.
pub mod reader_frame;

pub const CONTRACT_VERSION: &str = "1.0-draft.1";
pub const MAX_PAYLOAD_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Execution {
    Auto,
    Single,
    Swarm,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Privacy {
    LocalOnly,
    NetworkAllowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContentPart {
    Text { text: String },
    Image { asset_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRequest {
    pub contract_version: String,
    pub request_id: String,
    pub conversation_id: String,
    pub execution: Execution,
    pub privacy: Privacy,
    pub messages: Vec<Message>,
}

fn valid_id(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 128
}

impl SessionRequest {
    /// Validate the envelope only. Effective runtime capabilities need a host check.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.contract_version != CONTRACT_VERSION {
            return Err("unsupported_version");
        }
        if !valid_id(&self.request_id) || !valid_id(&self.conversation_id) {
            return Err("invalid_id");
        }
        if self.execution == Execution::Remote && self.privacy == Privacy::LocalOnly {
            return Err("remote_requires_network");
        }
        if self.messages.is_empty() || self.messages.len() > 100 {
            return Err("invalid_messages");
        }
        for message in &self.messages {
            if message.content.is_empty() || message.content.len() > 16 {
                return Err("invalid_content");
            }
            for part in &message.content {
                match part {
                    ContentPart::Text { text } if text.chars().count() > 32768 => {
                        return Err("text_too_large");
                    }
                    ContentPart::Image { asset_id } if !valid_id(asset_id) => {
                        return Err("invalid_asset_id");
                    }
                    _ => {}
                }
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| "invalid_request")?
            .len()
            > MAX_PAYLOAD_BYTES
        {
            return Err("payload_too_large");
        }
        Ok(())
    }

    /// Enforce the raw byte limit before JSON parsing, then validate semantics.
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > MAX_PAYLOAD_BYTES {
            return Err("payload_too_large");
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| "invalid_request")?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Macos,
    Linux,
    Windows,
    Ios,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    LocalRead,
    LocalWrite,
    Network,
    MailRead,
    MailSend,
    AppOpen,
    ScreenCapture,
    UiControl,
    ProcessExecute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Planned,
    Available,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDescriptor {
    pub name: String,
    pub version: String,
    pub status: ToolStatus,
    pub platforms: Vec<Platform>,
    pub effects: Vec<Effect>,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCatalogue {
    pub contract_version: String,
    pub notice: String,
    pub tools: Vec<ToolDescriptor>,
}

/// Basic selection gate, NOT authorization of specific arguments/resources.
/// `offered`, `grants`, and `confirmed` must come from trusted host state.
pub fn validate_tool_selection(
    name: &str,
    platform: Platform,
    offered: &[ToolDescriptor],
    grants: &[Effect],
    confirmed: bool,
    privacy: Privacy,
) -> Result<(), &'static str> {
    let matching: Vec<_> = offered.iter().filter(|tool| tool.name == name).collect();
    if matching.len() != 1 {
        return Err("unknown_or_duplicate_tool");
    }
    let tool = matching[0];
    if tool.status != ToolStatus::Available {
        return Err("tool_not_implemented");
    }
    if !tool.platforms.contains(&platform) {
        return Err("unsupported_platform");
    }
    if privacy == Privacy::LocalOnly && tool.effects.contains(&Effect::Network) {
        return Err("network_disabled");
    }
    if tool.effects.iter().any(|effect| !grants.contains(effect)) {
        return Err("permission_denied");
    }
    if tool.requires_confirmation && !confirmed {
        return Err("confirmation_required");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> SessionRequest {
        SessionRequest::parse(include_bytes!(
            "../../../contracts/v1/examples/session-request.json"
        ))
        .unwrap()
    }

    fn catalogue() -> ToolCatalogue {
        serde_json::from_slice(include_bytes!("../../../contracts/v1/tools.json")).unwrap()
    }

    #[test]
    fn request_roundtrip_and_version() {
        let request = example();
        let bytes = serde_json::to_vec(&request).unwrap();
        assert_eq!(SessionRequest::parse(&bytes).unwrap(), request);
        let mut invalid = request;
        invalid.contract_version = "2.0".into();
        assert_eq!(invalid.validate(), Err("unsupported_version"));
    }

    #[test]
    fn reject_extra_fields_and_privileged_roles() {
        let mut request = serde_json::to_value(example()).unwrap();
        request["approved"] = true.into();
        assert!(SessionRequest::parse(&serde_json::to_vec(&request).unwrap()).is_err());
        request.as_object_mut().unwrap().remove("approved");
        request["messages"][0]["role"] = "system".into();
        assert!(SessionRequest::parse(&serde_json::to_vec(&request).unwrap()).is_err());
    }

    #[test]
    fn reject_limits_and_implicit_remote() {
        let mut request = example();
        request.request_id = " ".into();
        assert_eq!(request.validate(), Err("invalid_id"));
        request = example();
        request.execution = Execution::Remote;
        assert_eq!(request.validate(), Err("remote_requires_network"));
        request = example();
        request.messages = vec![];
        assert_eq!(request.validate(), Err("invalid_messages"));
        assert_eq!(
            SessionRequest::parse(&vec![b' '; MAX_PAYLOAD_BYTES + 1]),
            Err("payload_too_large")
        );
    }

    #[test]
    fn catalogue_is_unique_and_not_implemented() {
        let catalogue = catalogue();
        assert_eq!(catalogue.contract_version, CONTRACT_VERSION);
        let mut names = std::collections::HashSet::new();
        for tool in &catalogue.tools {
            assert!(names.insert(&tool.name));
            assert_eq!(tool.status, ToolStatus::Planned);
            assert!(!tool.effects.is_empty());
            assert!(!tool.platforms.is_empty());
        }
        assert_eq!(
            validate_tool_selection(
                "apps_open",
                Platform::Macos,
                &catalogue.tools,
                &[Effect::AppOpen],
                true,
                Privacy::LocalOnly
            ),
            Err("tool_not_implemented")
        );
    }

    #[test]
    fn deny_unoffered_platform_grant_and_confirmation() {
        let mut catalogue = catalogue();
        let tool = catalogue
            .tools
            .iter_mut()
            .find(|t| t.name == "shell_run")
            .unwrap();
        tool.status = ToolStatus::Available;
        let check = |name, platform, grants: &[Effect], confirmed| {
            validate_tool_selection(
                name,
                platform,
                &catalogue.tools,
                grants,
                confirmed,
                Privacy::LocalOnly,
            )
        };
        assert_eq!(
            check("invented", Platform::Macos, &[], false),
            Err("unknown_or_duplicate_tool")
        );
        assert_eq!(
            check("shell_run", Platform::Ios, &[Effect::ProcessExecute], true),
            Err("unsupported_platform")
        );
        assert_eq!(
            check("shell_run", Platform::Macos, &[], true),
            Err("permission_denied")
        );
        assert_eq!(
            check(
                "shell_run",
                Platform::Macos,
                &[Effect::ProcessExecute],
                false
            ),
            Err("confirmation_required")
        );
        assert_eq!(
            check(
                "shell_run",
                Platform::Macos,
                &[Effect::ProcessExecute],
                true
            ),
            Ok(())
        );
    }

    #[test]
    fn local_only_denies_network_even_when_granted() {
        let mut catalogue = catalogue();
        catalogue
            .tools
            .iter_mut()
            .find(|t| t.name == "web_search")
            .unwrap()
            .status = ToolStatus::Available;
        assert_eq!(
            validate_tool_selection(
                "web_search",
                Platform::Ios,
                &catalogue.tools,
                &[Effect::Network],
                false,
                Privacy::LocalOnly
            ),
            Err("network_disabled")
        );
    }

    #[test]
    fn json_schema_documents_and_catalogue_are_consistent() {
        let request: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../contracts/v1/session-request.schema.json"
        ))
        .unwrap();
        let arguments: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../contracts/v1/tool-arguments.schema.json"
        ))
        .unwrap();
        let result: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../contracts/v1/tool-result.schema.json"
        ))
        .unwrap();
        assert_eq!(
            request["properties"]["contract_version"]["const"],
            CONTRACT_VERSION
        );
        assert_eq!(
            result["properties"]["contract_version"]["const"],
            CONTRACT_VERSION
        );
        for tool in catalogue().tools {
            let schema = &arguments["$defs"][&tool.name];
            assert_eq!(schema["type"], "object");
            assert_eq!(schema["additionalProperties"], false);
            assert!(schema["required"].is_array());
        }
        fn check_refs(node: &serde_json::Value, root: &serde_json::Value) {
            match node {
                serde_json::Value::Object(map) => {
                    if let Some(reference) = map.get("$ref").and_then(|v| v.as_str()) {
                        assert!(reference.starts_with('#'));
                        assert!(root.pointer(&reference[1..]).is_some());
                    }
                    for value in map.values() {
                        check_refs(value, root);
                    }
                }
                serde_json::Value::Array(array) => {
                    for value in array {
                        check_refs(value, root);
                    }
                }
                _ => {}
            }
        }
        check_refs(&request, &request);
        check_refs(&arguments, &arguments);
        check_refs(&result, &result);
    }
}
