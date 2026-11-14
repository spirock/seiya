//! One structured image proposal, bound to a browser-owned broker capability.
use crate::{
    image_jobs::{Command, ImageJobs, Reply},
    inference::ApiError,
};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) fn tool() -> Value {
    json!({"type":"function","function":{"name":"image_generate","description":"Generate one image only when the user explicitly requests a new image. Not for describing images, web searches or mere discussion. Submit a descriptive prompt; host fixes model, resolution and seed. This registers a job, not a completed PNG.","parameters":{"type":"object","properties":{"prompt":{"type":"string","minLength":1,"maxLength":2048}},"required":["prompt"],"additionalProperties":false}}})
}
fn invalid() -> ApiError {
    ApiError {
        status: StatusCode::BAD_REQUEST,
        message: "Invalid image action/reference; no image submitted".into(),
        code: Some("invalid_image_action"),
    }
}
fn broker() -> ApiError {
    ApiError {
        status: StatusCode::CONFLICT,
        message: "Image session or job unavailable; check image panel before resending".into(),
        code: Some("image_job_unavailable"),
    }
}
pub(crate) fn existing(
    images: &Arc<ImageJobs>,
    session: &str,
    id: &str,
) -> Result<Option<Value>, ApiError> {
    if !uuid::Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id) || session.len() != 64 {
        return Err(invalid());
    }
    let Reply::Json(list) = images
        .control(Command::List {
            session: session.into(),
        })
        .map_err(|_| broker())?
    else {
        return Err(broker());
    };
    Ok(list["jobs"]
        .as_array()
        .ok_or_else(broker)?
        .iter()
        .find(|j| j["request_id"] == id)
        .map(reply))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    prompt: String,
}
pub(crate) fn submit(
    images: &Arc<ImageJobs>,
    session: &str,
    id: &str,
    call: &Value,
) -> Result<Value, ApiError> {
    if call["type"] != "function" || call["function"]["name"] != "image_generate" {
        return Err(invalid());
    }
    let args: Arguments = serde_json::from_str(
        call["function"]["arguments"]
            .as_str()
            .filter(|v| v.len() <= 16384)
            .ok_or_else(invalid)?,
    )
    .map_err(|_| invalid())?;
    let request = seiya_contracts::image_job::ImageRequest {
        prompt: args.prompt,
        width: 512,
        height: 512,
        seed: 42,
    };
    request.validate().map_err(|_| invalid())?;
    let Reply::Json(job) = images
        .control(Command::Submit {
            session: session.into(),
            request_id: id.into(),
            request,
        })
        .map_err(|_| broker())?
    else {
        return Err(broker());
    };
    Ok(reply(&job))
}
fn reply(job: &Value) -> Value {
    json!({"model":"pegassus","choices":[{"index":0,"message":{"role":"assistant","content":"Solicitud de imagen registrada. Consulta el panel de imágenes para ver progreso, cancelación y el PNG validado; este mensaje no confirma que haya terminado."},"finish_reason":"stop"}],"usage":null,"image_job":job,"swarm":{"status_rendered_by_host":true,"elapsed_seconds":0}})
}
#[cfg(all(test, target_os = "macos"))]
mod tests;
