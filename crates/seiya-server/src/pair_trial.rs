//! Explicit native Ollama adapter for the isolated trial, not SessionRequest.
use crate::{
    jobs::{self, Executor, Failure, Work},
    prepared_web::{AUXILIARY, FOREGROUND},
    runtime::{RuntimeFuture, RuntimeTransport, TransportError},
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod access;
pub mod limits;
#[cfg(target_os = "macos")]
pub mod native;

pub struct NativeChat {
    client: reqwest::Client,
    endpoint: String,
    model: &'static str,
}
impl NativeChat {
    pub fn new(port: u16, model: &'static str) -> Result<Self, TransportError> {
        if port == 0 || port == 11434 || !matches!(model, FOREGROUND | AUXILIARY) {
            return Err(TransportError::NotConnected);
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|_| TransportError::NotConnected)?;
        Ok(Self {
            client,
            endpoint: format!("http://127.0.0.1:{port}/api/chat"),
            model,
        })
    }
}
pub(crate) fn request(input: Value, model: &str) -> Result<Value, TransportError> {
    if input["model"] != model || !input["messages"].is_array() {
        return Err(TransportError::NotConnected);
    }
    let mut body = json!({"model":model,"messages":input["messages"],"stream":false,"think":false,"keep_alive":"30m","options":{"num_ctx":4096,"num_predict":256,"temperature":0}});
    if let Some(tools) = input.get("tools") {
        body["tools"] = tools.clone();
    }
    // Conservative byte bound incl. schemas/system, never truncate history.
    if body.to_string().len() > 3584 {
        return Err(TransportError::NotConnected);
    }
    Ok(body)
}
pub fn decode_native(raw: &[u8], model: &str) -> Result<Value, TransportError> {
    let r: Value = serde_json::from_slice(raw).map_err(|_| TransportError::InvalidJson)?;
    if r["model"] != model
        || r["done"] != true
        || r["done_reason"] != "stop"
        || r.get("error").is_some()
        || r["message"]["role"] != "assistant"
        || r["eval_count"].as_u64().is_none_or(|n| n > 256)
        || r["prompt_eval_count"].as_u64().is_none_or(|n| n > 4096)
        || r["message"]
            .get("thinking")
            .is_some_and(|t| !t.is_null() && t != "")
    {
        return Err(TransportError::InvalidJson);
    }
    let mut message = r["message"].clone();
    if let Some(calls) = message.get_mut("tool_calls") {
        if !calls.is_null() {
            let calls = calls
                .as_array_mut()
                .filter(|v| v.len() <= 4)
                .ok_or(TransportError::InvalidJson)?;
            for (i, call) in calls.iter_mut().enumerate() {
                let args = &call["function"]["arguments"];
                if !args.is_object() || !call["function"]["name"].is_string() {
                    return Err(TransportError::InvalidJson);
                }
                *call = json!({"id":format!("trial_call_{i}"),"type":"function","function":{"name":call["function"]["name"],"arguments":args.to_string()}});
            }
        }
    }
    let tool = message["tool_calls"]
        .as_array()
        .is_some_and(|v| !v.is_empty());
    if !message["content"].is_string() && !tool {
        return Err(TransportError::InvalidJson);
    }
    if message["content"]
        .as_str()
        .is_some_and(|s| s.contains("<think>") || s.contains("</think>"))
    {
        return Err(TransportError::InvalidJson);
    }
    Ok(
        json!({"model":model,"choices":[{"index":0,"message":message,"finish_reason":if tool{"tool_calls"}else{"stop"}}],"usage":{"prompt_tokens":r["prompt_eval_count"],"completion_tokens":r["eval_count"],"total_tokens":r["prompt_eval_count"].as_u64().unwrap()+r["eval_count"].as_u64().unwrap()}}),
    )
}
impl RuntimeTransport for NativeChat {
    fn complete(&self, input: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            let body = request(input, self.model)?;
            let mut r = self
                .client
                .post(&self.endpoint)
                .json(&body)
                .send()
                .await
                .map_err(|e| {
                    if e.is_connect() {
                        TransportError::NotConnected
                    } else if e.is_timeout() {
                        TransportError::Timeout
                    } else {
                        TransportError::Unavailable
                    }
                })?;
            if r.status() != reqwest::StatusCode::OK {
                return Err(TransportError::HttpFailure);
            }
            let mut raw = Vec::new();
            while let Some(chunk) = r.chunk().await.map_err(|_| TransportError::Unavailable)? {
                if raw.len() + chunk.len() > 131072 {
                    return Err(TransportError::ResponseTooLarge);
                }
                raw.extend(chunk);
            }
            decode_native(&raw, self.model)
        })
    }
}
pub struct NativeDraft(pub Arc<NativeChat>);
impl Executor for NativeDraft {
    fn draft(&self, prompt: String, cancel: watch::Receiver<bool>) -> Work {
        let chat = self.0.clone();
        Box::pin(async move {
            if *cancel.borrow() {
                return Err(Failure::Confirmed);
            }
            // Do not drop HTTP and claim cancellation. Await natural completion;
            // native host watchdog can end the owned runtime on a real stop.
            let reply=chat.complete(json!({"model":AUXILIARY,"messages":[{"role":"system","content":"Return a short source-code draft as plain text. No tools, private reasoning, execution or tests."},{"role":"user","content":prompt}]})).await.map_err(|e|if e==TransportError::NotConnected{Failure::Confirmed}else{Failure::Uncertain})?;
            jobs::decode_draft(
                &serde_json::to_vec(&reply).map_err(|_| Failure::Uncertain)?,
                AUXILIARY,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reply() -> Value {
        json!({"model":FOREGROUND,"done":true,"done_reason":"stop","message":{"role":"assistant","content":"SEIYA_OK"},"eval_count":4,"prompt_eval_count":24})
    }
    #[test]
    fn native_and_openai_are_distinct_and_thinking_context_are_fixed() {
        let r = request(
            json!({"model":FOREGROUND,"messages":[{"role":"user","content":"hi"}]}),
            FOREGROUND,
        )
        .unwrap();
        assert_eq!(r["think"], false);
        assert_eq!(r["options"]["num_ctx"], 4096);
        assert_eq!(r["options"]["num_predict"], 256);
        assert!(decode_native(&serde_json::to_vec(&reply()).unwrap(), FOREGROUND).is_ok());
        assert!(decode_native(br#"{"choices":[]}"#, FOREGROUND).is_err());
        assert!(request(
            json!({"model":FOREGROUND,"messages":[{"role":"user","content":"x".repeat(4000)}]}),
            FOREGROUND
        )
        .is_err());
    }
    #[test]
    fn reject_identity_truncation_reasoning_and_unbounded_usage() {
        for field in ["model", "done_reason", "eval_count", "prompt_eval_count"] {
            let mut r = reply();
            r[field] = match field {
                "eval_count" => json!(257),
                "prompt_eval_count" => json!(4097),
                _ => json!("wrong"),
            };
            assert!(decode_native(&serde_json::to_vec(&r).unwrap(), FOREGROUND).is_err());
        }
        let mut r = reply();
        r["message"]["thinking"] = json!("hidden");
        assert!(decode_native(&serde_json::to_vec(&r).unwrap(), FOREGROUND).is_err());
    }
    #[test]
    fn native_tools_are_adapted_but_never_executed_here() {
        let mut r = reply();
        r["message"]["tool_calls"] = json!([{"function":{"name":"code_job_list","arguments":{}}}]);
        let out = decode_native(&serde_json::to_vec(&r).unwrap(), FOREGROUND).unwrap();
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(
            out["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            "{}"
        );
        assert!(NativeChat::new(11434, FOREGROUND).is_err());
    }
    #[tokio::test]
    async fn native_http_fixture_uses_closed_local_protocol_without_real_models() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let capture = seen.clone();
        let router = axum::Router::new().route(
            "/api/chat",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let capture = capture.clone();
                async move {
                    capture.lock().unwrap().push(body);
                    axum::Json(reply())
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stop, done) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = done.await;
                })
                .await
                .unwrap()
        });
        let chat = NativeChat::new(port, FOREGROUND).unwrap();
        let v = chat
            .complete(
                json!({"model":FOREGROUND,"messages":[{"role":"user","content":"synthetic"}]}),
            )
            .await
            .unwrap();
        assert_eq!(v["choices"][0]["message"]["content"], "SEIYA_OK");
        let r = seen.lock().unwrap()[0].clone();
        assert_eq!(r["think"], false);
        assert_eq!(r["stream"], false);
        assert_eq!(r["options"]["num_ctx"], 4096);
        assert_eq!(r["keep_alive"], "30m");
        stop.send(()).unwrap();
        server.await.unwrap();
    }
}
