//! Cliente del conector opcional Open Interface (Linux, protocolo calculator.v1).
use super::*;
use tokio::io::AsyncWriteExt;
static SLOT: Semaphore = Semaphore::const_new(1);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Sum {
    a: u32,
    b: u32,
}

pub(super) async fn open(State(s): State<Arc<Setup>>, Json(sum): Json<Sum>) -> Response {
    let provider = {
        let c = s.control.lock().await;
        let Some(p) = c.vision_provider.as_ref().filter(|_| c.phase == "running") else {
            return failure("assist_requires_vision_provider");
        };
        p.clone()
    };
    if sum.a > 999999 || sum.b > 999999 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(_permit) = SLOT.try_acquire() else {
        return failure("calculator_already_open");
    };
    let Some(home) = std::env::var_os("HOME") else {
        return failure("connector_not_installed");
    };
    let binary = std::path::PathBuf::from(home)
        .join(".local/share/seiya-open-interface/seiya-open-interface");
    let output = tokio::process::Command::new(binary)
        .args([
            "calculator.v1",
            &sum.a.to_string(),
            &sum.b.to_string(),
            &provider.port.to_string(),
            &provider.model,
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = match tokio::time::timeout(Duration::from_secs(200), output).await {
        Ok(Ok(v)) if v.status.success() => v,
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return failure("connector_not_installed")
        }
        _ => return failure("visual_result_not_confirmed"),
    };
    let reply = serde_json::from_slice::<Value>(&out.stdout).unwrap_or(Value::Null);
    let expected = (sum.a + sum.b).to_string();
    if reply["operation"] != "calculator.v1" || reply["observed"] != expected {
        return failure("visual_result_not_confirmed");
    }
    Json(json!({"observed":expected,"provider":provider.role})).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Goal {
    goal: String,
}

/// Entrega un objetivo al cliente MCP de la sesión; ninguna acción se interpreta como shell.
pub(super) async fn goal(State(s): State<Arc<Setup>>, Json(input): Json<Goal>) -> Response {
    let provider = {
        let c = s.control.lock().await;
        let Some(p) = c.vision_provider.as_ref().filter(|_| c.phase == "running") else {
            return failure("assist_requires_vision_provider");
        };
        p.clone()
    };
    if input.goal.trim().is_empty() || input.goal.chars().count() > 1500 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(_permit) = SLOT.try_acquire() else {
        return failure("assist_busy");
    };
    let Some(home) = std::env::var_os("HOME") else {
        return failure("connector_not_installed");
    };
    let root = std::path::PathBuf::from(home).join(".local/share/seiya-open-interface");
    let Ok(mut child) = tokio::process::Command::new(root.join("venv/bin/python"))
        .arg("-I")
        .arg(root.join("desktop_goal.py"))
        .arg(provider.port.to_string())
        .arg(&provider.model)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    else {
        return failure("connector_not_installed");
    };
    let mut stdin = child.stdin.take().unwrap();
    if stdin
        .write_all(json!({"goal":input.goal}).to_string().as_bytes())
        .await
        .is_err()
    {
        return failure("assist_failed");
    }
    drop(stdin);
    let Ok(Ok(out)) =
        tokio::time::timeout(Duration::from_secs(600), child.wait_with_output()).await
    else {
        return failure("assist_timeout");
    };
    if !out.status.success() {
        return failure("assist_not_confirmed");
    }
    let Ok(result) = serde_json::from_slice::<Value>(&out.stdout) else {
        return failure("assist_failed");
    };
    Json(json!({"provider":provider.role,"result":result})).into_response()
}
