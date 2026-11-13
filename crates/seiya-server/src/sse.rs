use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

/// Encode only a fully validated completion. This is not token streaming.
pub(crate) fn buffered(completion: Value, include_usage: bool) -> Response {
    let choice = &completion["choices"][0];
    let mut delta = choice["message"].clone();
    if let Some(calls) = delta.get_mut("tool_calls").and_then(Value::as_array_mut) {
        for (index, call) in calls.iter_mut().enumerate() {
            call["index"] = index.into();
        }
    }
    let base = json!({
        "id": completion["id"], "created": completion["created"],
        "model": completion["model"], "object": "chat.completion.chunk",
    });
    let mut first = base.clone();
    first["choices"] = json!([{"index":0,"delta":delta,"finish_reason":null}]);
    let mut last = base;
    last["choices"] = json!([{"index":0,"delta":{},"finish_reason":choice["finish_reason"]}]);
    if include_usage {
        last["usage"] = completion["usage"].clone();
    }
    last["swarm"] = completion["swarm"].clone();
    // Value's serializer escapes embedded newlines; model text cannot inject
    // SSE frames. Return a finite body, not a producer task or unbounded queue.
    let body = format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n");
    (
        [
            (header::CONTENT_TYPE, "text/event-stream; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (header::HeaderName::from_static("x-accel-buffering"), "no"),
        ],
        body,
    )
        .into_response()
}
