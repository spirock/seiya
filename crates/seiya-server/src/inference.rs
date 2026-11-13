//! Internal JSON completion adapter. Not wired to public HTTP yet (S1-04/05).
use crate::config::ConfigError;
use crate::runtime::{RuntimeConfig, RuntimeTransport, TransportError};
use axum::http::StatusCode;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const READY: u8 = 0;
const ACTIVE: u8 = 1;
const UNCERTAIN: u8 = 2;
const NOT_CONNECTED: u8 = 3;
const LAST_COMPLETED: u8 = 4;
pub const MAX_CHAT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
    pub code: Option<&'static str>,
}

impl ApiError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            code: None,
        }
    }

    fn backend(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: message.into(),
            code: None,
        }
    }

    pub fn body(&self) -> Value {
        let mut result = json!({"error": {"message": self.message}});
        if let Some(code) = self.code {
            result["error"]["code"] = json!(code);
        }
        result
    }

    fn unavailable(code: &'static str, message: &'static str) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
            code: Some(code),
        }
    }
}

fn not_connected() -> ApiError {
    ApiError::unavailable("runtime_unavailable",
        "local runtime connection was not established; verify the runtime before restarting this experimental instance")
}
fn unconfirmed() -> ApiError {
    ApiError::unavailable(
        "runtime_state_unconfirmed",
        "runtime state is unconfirmed; verify backend completion before recovery",
    )
}

fn validate(body: &Value) -> Result<(), ApiError> {
    let object = body
        .as_object()
        .ok_or_else(|| ApiError::invalid("request must be an object"))?;
    if object.get("model").is_some_and(|model| model != "pegassus") {
        return Err(ApiError::invalid("the exposed model ID is pegassus"));
    }
    let messages = object
        .get("messages")
        .and_then(Value::as_array)
        .filter(|m| !m.is_empty() && m.len() <= 100)
        .ok_or_else(|| ApiError::invalid("messages must contain 1–100 messages"))?;
    for message in messages {
        if !message.is_object()
            || !matches!(
                message["role"].as_str(),
                Some("system" | "user" | "assistant" | "tool")
            )
        {
            return Err(ApiError::invalid("unsupported message role"));
        }
        if !message["content"].is_null() && !message["content"].is_string() {
            return Err(ApiError::invalid("text-only messages are supported"));
        }
        if message["role"] == "tool" && !message["tool_call_id"].is_string() {
            return Err(ApiError::invalid("tool messages require tool_call_id"));
        }
    }
    requested_output_tokens(body)?;
    if object.get("temperature").is_some_and(|n| {
        !n.as_f64()
            .is_some_and(|n| n.is_finite() && (0.0..=2.0).contains(&n))
    }) {
        return Err(ApiError::invalid("temperature must be 0–2"));
    }
    if object.get("stream").is_some_and(|v| !v.is_boolean()) {
        return Err(ApiError::invalid("stream must be boolean"));
    }
    if let Some(mode) = object.get("swarm_mode") {
        if !matches!(mode.as_str(), Some("fast" | "consensus")) {
            return Err(ApiError::invalid("swarm_mode must be fast or consensus"));
        }
        if mode == "consensus" {
            return Err(ApiError::invalid(
                "consensus is not supported in this experimental build",
            ));
        }
    }
    for key in [
        "tool_choice",
        "response_format",
        "logprobs",
        "parallel_tool_calls",
    ] {
        if object.contains_key(key) {
            return Err(ApiError::invalid(format!(
                "{key} is not supported in this version"
            )));
        }
    }
    if let Some(tools) = object.get("tools") {
        let tools = tools.as_array().filter(|t| t.len() <= 64).ok_or_else(|| {
            ApiError::invalid("tools must be a list of at most 64 function tools")
        })?;
        let mut names = HashSet::new();
        for tool in tools {
            if !tool.is_object() || tool["type"] != "function" {
                return Err(ApiError::invalid("only function tools are supported"));
            }
            let name = tool["function"]["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| ApiError::invalid("tool requires function.name"))?;
            if !names.insert(name)
                || tool["function"]
                    .get("parameters")
                    .is_some_and(|v| !v.is_object())
            {
                return Err(ApiError::invalid(
                    "tool names must be unique; parameters must be an object",
                ));
            }
        }
    }
    validate_options(body)?;
    Ok(())
}

fn validate_options(body: &Value) -> Result<(), ApiError> {
    if body.get("store").is_some_and(|v| v != &Value::Bool(false)) {
        return Err(ApiError::invalid(
            "store must be false; server-side completion storage is unsupported",
        ));
    }
    if let Some(options) = body.get("stream_options") {
        if body.get("stream") != Some(&Value::Bool(true)) {
            return Err(ApiError::invalid("stream_options requires stream:true"));
        }
        let options = options
            .as_object()
            .ok_or_else(|| ApiError::invalid("stream_options must be an object"))?;
        if options.keys().any(|key| key != "include_usage") {
            return Err(ApiError::invalid(
                "stream_options supports only include_usage",
            ));
        }
        if options
            .get("include_usage")
            .is_some_and(|v| !v.is_boolean())
        {
            return Err(ApiError::invalid(
                "stream_options.include_usage must be boolean",
            ));
        }
    }
    const SUPPORTED: &[&str] = &[
        "model",
        "messages",
        "tools",
        "temperature",
        "max_tokens",
        "max_completion_tokens",
        "swarm_mode",
        "stream",
        "stream_options",
        "store",
    ];
    if body
        .as_object()
        .unwrap()
        .keys()
        .any(|key| !SUPPORTED.contains(&key.as_str()))
    {
        return Err(ApiError::invalid("request contains unsupported fields"));
    }
    Ok(())
}

pub(crate) fn validate_input(body: &Value) -> Result<(), ApiError> {
    let bytes = serde_json::to_vec(body).map_err(|_| ApiError::invalid("invalid JSON request"))?;
    if bytes.len() > MAX_CHAT_BYTES {
        return Err(ApiError::invalid(
            "body must be 1–262144 bytes; chunked requests unsupported",
        ));
    }
    validate(body)
}

/// Owned proof of validation. No mutable access or unchecked constructor: every
/// public entry point must establish the same boundary before queue admission.
pub(crate) struct ValidatedInput(Value);

impl ValidatedInput {
    pub(crate) fn new(body: Value) -> Result<Self, ApiError> {
        validate_input(&body)?;
        Ok(Self(body))
    }
}

/// Validate both aliases before applying the operator's cap; never silently
/// discard a smaller client budget or resolve contradictory values by priority.
fn requested_output_tokens(body: &Value) -> Result<Option<u64>, ApiError> {
    let read = |key: &str| -> Result<Option<u64>, ApiError> {
        body.get(key)
            .map(|value| {
                value
                    .as_u64()
                    .filter(|n| (1..=4096).contains(n))
                    .ok_or_else(|| {
                        ApiError::invalid(format!("{key} must be an integer between 1 and 4096"))
                    })
            })
            .transpose()
    };
    let legacy = read("max_tokens")?;
    let completion = read("max_completion_tokens")?;
    if let (Some(a), Some(b)) = (legacy, completion) {
        if a != b {
            return Err(ApiError::invalid(
                "max_tokens and max_completion_tokens must match when both are provided",
            ));
        }
    }
    Ok(completion.or(legacy))
}

fn check_tools(message: &Value, tools: &[Value]) -> Result<bool, ApiError> {
    let calls = match message.get("tool_calls") {
        None | Some(Value::Null) => return Ok(false),
        Some(Value::Array(calls)) => calls,
        _ => return Err(ApiError::backend("invalid tool_calls array")),
    };
    let allowed: HashSet<_> = tools
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect();
    let mut ids = HashSet::new();
    for call in calls {
        let mut valid = || {
            let id = call.get("id")?.as_str()?;
            let function = call.get("function")?;
            let arguments = function.get("arguments")?.as_str()?;
            if id.is_empty()
                || id.len() > 128
                || id.chars().any(char::is_control)
                || !ids.insert(id)
                || call["type"] != "function"
                || !allowed.contains(function["name"].as_str()?)
            {
                return None;
            }
            serde_json::from_str::<Value>(arguments)
                .ok()
                .filter(Value::is_object)
        };
        if valid().is_none() {
            return Err(ApiError::backend(
                "invalid structured tool call; nothing executed",
            ));
        }
    }
    Ok(!calls.is_empty())
}

fn decode_response(
    config: &RuntimeConfig,
    reply: Value,
    tools: &[Value],
    mode: &str,
    started: Instant,
) -> Result<Value, ApiError> {
    let malformed = || ApiError::backend(format!("{}: malformed model response", config.id));
    let choice = reply
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .ok_or_else(malformed)?;
    let message = choice
        .get("message")
        .filter(|m| m.is_object())
        .ok_or_else(malformed)?;
    if choice["finish_reason"] == "length" {
        return Err(ApiError::backend(format!(
            "{}: output truncated; increase output budget",
            config.id
        )));
    }
    if !matches!(choice.get("finish_reason"), None | Some(Value::Null))
        && !matches!(
            choice["finish_reason"].as_str(),
            Some("stop" | "tool_calls")
        )
    {
        return Err(ApiError::backend(format!(
            "{}: unsupported finish reason",
            config.id
        )));
    }
    let tokens = match reply.get("usage") {
        None | Some(Value::Null) => None,
        Some(Value::Object(tokens)) => Some(tokens),
        _ => return Err(ApiError::backend("invalid token usage object")),
    };
    let has_calls = check_tools(message, tools)?;
    if message["role"] != "assistant"
        || (!message["content"].is_null() && !message["content"].is_string())
        || (!has_calls && !message["content"].is_string())
        || (choice["finish_reason"] == "tool_calls" && !has_calls)
    {
        return Err(malformed());
    }
    let mut usage = serde_json::Map::new();
    for key in ["prompt_tokens", "completion_tokens", "total_tokens"] {
        let value = tokens.and_then(|t| t.get(key));
        let count = match value {
            None => 0,
            Some(value) => value
                .as_u64()
                .ok_or_else(|| ApiError::backend("invalid token usage object"))?,
        };
        usage.insert(key.to_owned(), json!(count));
    }
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ApiError::backend("system clock is unavailable"))?
        .as_secs();
    Ok(json!({
        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()),
        "object": "chat.completion", "created": created, "model": "pegassus",
        "choices": [{"index":0,"message":message,"finish_reason":if has_calls {"tool_calls"} else {"stop"}}],
        "usage": usage,
        "swarm": {"mode":mode,"selected_model":config.id,
            "elapsed_seconds":(started.elapsed().as_secs_f64()*1000.0).round()/1000.0}
    }))
}

struct Lease {
    state: Arc<AtomicU8>,
    confirmed: bool,
    runtime_status: bool,
}
impl Lease {
    fn confirm_finished(&mut self) {
        self.confirmed = true;
        self.state.store(
            if self.runtime_status {
                LAST_COMPLETED
            } else {
                READY
            },
            Ordering::Release,
        );
    }
    fn confirm_not_dispatched(&mut self) {
        self.confirmed = true;
        self.state.store(NOT_CONNECTED, Ordering::Release);
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if !self.confirmed {
            self.state.store(UNCERTAIN, Ordering::Release);
        }
    }
}

/// Share one instance across requests. Not a cross-process GPU scheduler.
#[derive(Clone)]
pub struct InferenceAdapter {
    config: RuntimeConfig,
    transport: Arc<dyn RuntimeTransport>,
    state: Arc<AtomicU8>,
    runtime_status: bool,
}

impl InferenceAdapter {
    pub(crate) fn model_binding_matches(&self, supplied: &str) -> bool {
        if !self.runtime_status || supplied.len() != self.config.model.len() * 2 {
            return false;
        }
        let expected: String = self
            .config
            .model
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        supplied == expected
    }
    pub fn new(
        config: RuntimeConfig,
        transport: Arc<dyn RuntimeTransport>,
    ) -> Result<Self, ConfigError> {
        Self::configured(config, transport, false)
    }

    pub fn new_with_runtime_status(
        config: RuntimeConfig,
        transport: Arc<dyn RuntimeTransport>,
    ) -> Result<Self, ConfigError> {
        Self::configured(config, transport, true)
    }

    fn configured(
        config: RuntimeConfig,
        transport: Arc<dyn RuntimeTransport>,
        runtime_status: bool,
    ) -> Result<Self, ConfigError> {
        config.validate()?;
        Ok(Self {
            config,
            transport,
            state: Arc::new(AtomicU8::new(READY)),
            runtime_status,
        })
    }

    pub fn is_busy(&self) -> bool {
        matches!(self.state.load(Ordering::Acquire), ACTIVE | UNCERTAIN)
    }
    pub fn requires_recovery(&self) -> bool {
        matches!(
            self.state.load(Ordering::Acquire),
            UNCERTAIN | NOT_CONNECTED
        )
    }

    pub(crate) fn blocking_error(&self) -> Option<ApiError> {
        if !self.runtime_status {
            return None;
        }
        match self.state.load(Ordering::Acquire) {
            NOT_CONNECTED => Some(not_connected()),
            UNCERTAIN => Some(unconfirmed()),
            _ => None,
        }
    }

    /// Cached observations only. No endpoint probe, load, or memory admission.
    pub fn runtime_status(&self) -> Option<Value> {
        if !self.runtime_status {
            return None;
        }
        let state = self.state.load(Ordering::Acquire);
        let observation = match state {
            NOT_CONNECTED => "connection_not_established",
            UNCERTAIN => "unconfirmed",
            LAST_COMPLETED => "last_request_completed",
            ACTIVE => "request_in_flight",
            _ => "not_checked",
        };
        Some(json!({"version":1,"endpoint_observation":observation,
            "model_residency":"unknown","memory_admission":"not_integrated",
            "admission":if matches!(state, NOT_CONNECTED | UNCERTAIN) {"blocked"} else {"may_attempt"},
            "active_requests":match state { UNCERTAIN => Value::Null, ACTIVE => json!(1), _ => json!(0) }}))
    }

    pub async fn complete(&self, body: Value) -> Result<Value, ApiError> {
        self.complete_validated(ValidatedInput::new(body)?).await
    }

    pub(crate) async fn complete_validated(
        &self,
        input: ValidatedInput,
    ) -> Result<Value, ApiError> {
        let mut body = input.0;
        let output_tokens = requested_output_tokens(&body)?
            .unwrap_or(self.config.max_output_tokens)
            .min(self.config.max_output_tokens);
        // Equivalent conditional atomic update using the MSRV-compatible CAS
        // API. Newer Rust deprecates fetch_update, while its renamed API is not
        // available on every supported toolchain. Preserve memory ordering and
        // never transition an active or quarantined runtime back to admission.
        let mut observed = self.state.load(Ordering::Acquire);
        let previous = loop {
            if !matches!(observed, READY | LAST_COMPLETED) {
                break Err(observed);
            }
            match self.state.compare_exchange_weak(
                observed,
                ACTIVE,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(previous) => break Ok(previous),
                Err(current) => observed = current,
            }
        };
        if let Err(state) = previous {
            return Err(if let Some(error) = self.blocking_error() {
                error
            } else if state == UNCERTAIN {
                ApiError { status: StatusCode::SERVICE_UNAVAILABLE,
                    message: "runtime state is unconfirmed after a transport error; verify it is idle before recovery".into(), code:None }
            } else {
                ApiError {
                    status: StatusCode::TOO_MANY_REQUESTS,
                    message: "one request is already running; retry when it completes".into(),
                    code: None,
                }
            });
        }
        let lease = Lease {
            state: self.state.clone(),
            confirmed: false,
            runtime_status: self.runtime_status,
        };
        let config = self.config.clone();
        let transport = self.transport.clone();
        // The task owns the lease: dropping the consumer does not imply that a
        // local runtime stopped generating. A failed transport stays quarantined.
        let task = tokio::spawn(async move {
            let mut lease = lease;
            let started = Instant::now();
            let tools = body
                .get("tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let messages = body.as_object_mut().unwrap().remove("messages").unwrap();
            let tool_history = messages.as_array().unwrap().iter().any(|m| {
                m["role"] == "tool"
                    || m.get("tool_calls")
                        .is_some_and(|v| !v.is_null() && v != &json!([]))
            });
            let mode = if !tools.is_empty() || tool_history {
                "tools"
            } else {
                "fast"
            };
            let mut request = json!({
                "model":config.model,"stream":false,
                "max_tokens":output_tokens,
                "temperature":body.get("temperature").cloned().unwrap_or(json!(0)),
                "chat_template_kwargs":{"enable_thinking":false}
            });
            request["messages"] = messages;
            if !tools.is_empty() {
                request["tools"] = json!(tools);
            }
            let reply = tokio::time::timeout(
                Duration::from_secs(config.timeout_seconds),
                transport.complete(request),
            )
            .await
            .unwrap_or(Err(TransportError::Timeout));
            let reply = match reply {
                Ok(reply) => reply,
                Err(TransportError::NotConnected) if lease.runtime_status => {
                    lease.confirm_not_dispatched();
                    return Err(not_connected());
                }
                Err(error) => {
                    let mut failure = ApiError::backend(error.message());
                    if lease.runtime_status {
                        failure.code = Some("runtime_state_unconfirmed");
                    }
                    return Err(failure);
                }
            };
            let result = decode_response(&config, reply, &tools, mode, started);
            lease.confirm_finished();
            result
        });
        task.await.map_err(|_| {
            ApiError::backend("local inference task failed; runtime state is unconfirmed")
        })?
    }
}
