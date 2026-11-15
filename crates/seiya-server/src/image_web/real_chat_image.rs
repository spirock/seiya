//! Manual single-attempt acceptance harness. Never part of normal test execution.
use super::*;
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn http(
    app: &Router,
    path: &str,
    cookie: &str,
    frame: Option<Value>,
) -> Result<axum::response::Response, String> {
    let mut request = Request::builder()
        .method(if frame.is_some() { "POST" } else { "GET" })
        .uri(path)
        .header("host", "127.0.0.1:8015")
        .header("cookie", cookie);
    let body = if let Some(frame) = frame {
        let bytes = frame.to_string();
        request = request
            .header("content-type", "application/json")
            .header("content-length", bytes.len());
        Body::from(bytes)
    } else {
        Body::empty()
    };
    app.clone()
        .oneshot(request.body(body).map_err(|_| "request_schema")?)
        .await
        .map_err(|_| "router_error".into())
}
async fn json_http(app: &Router, path: &str, cookie: &str, frame: Value) -> Result<Value, String> {
    let response = http(app, path, cookie, Some(frame)).await?;
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|_| "response_body")?
        .to_bytes();
    let data: Value = serde_json::from_slice(&bytes).map_err(|_| "response_json")?;
    if !status.is_success() {
        return Err(format!(
            "http_{}_{}",
            status.as_u16(),
            data["error"]["code"].as_str().unwrap_or("unclassified")
        ));
    }
    Ok(data)
}
async fn exercise(
    app: Router,
    ready: crate::launcher::Ready,
    root: &Path,
) -> Result<Value, String> {
    let bootstrap = reqwest::Url::parse(&ready.bootstrap).map_err(|_| "bootstrap_url")?;
    let response = http(&app, bootstrap.path(), "", None).await?;
    if response.status() != 303 {
        return Err("bootstrap_rejected".into());
    }
    let cookie = response
        .headers()
        .get("set-cookie")
        .ok_or("bootstrap_cookie")?
        .to_str()
        .map_err(|_| "bootstrap_cookie")?
        .split(';')
        .next()
        .ok_or("bootstrap_cookie")?
        .to_owned();
    let home = reqwest::Url::parse(&ready.home).map_err(|_| "home_url")?;
    let prefix = home.path().trim_end_matches('/');
    let endpoint = format!("{prefix}/api/images/v1");
    let opened = json_http(
        &app,
        &endpoint,
        &cookie,
        json!({"operation":"open","conversation_id":uuid::Uuid::new_v4().to_string()}),
    )
    .await?;
    let session = opened["session"].as_str().ok_or("image_session")?;
    let id = uuid::Uuid::new_v4().to_string();
    // Exactly one chat dispatch. No forced tool call, model retry or manual submit.
    let registered = json_http(
        &app,
        &format!("{prefix}/api/chat"),
        &cookie,
        json!({"model":"pegassus","messages":[{"role":"user","content":"Genera una imagen de un cubo azul sobre fondo blanco con image_generate."}],"image_session":session,"image_request_id":id}),
    )
    .await?;
    let job = registered["image_job"]["job_id"]
        .as_str()
        .ok_or("brain_did_not_register_image")?;
    if registered["image_job"]["request_id"] != id {
        return Err("image_reference_mismatch".into());
    }
    let deadline = Instant::now() + Duration::from_secs(630);
    let mut phases = Vec::new();
    loop {
        let state = json_http(
            &app,
            &endpoint,
            &cookie,
            json!({"operation":"status","session":session,"job_id":job}),
        )
        .await?;
        let phase = state["phase"].as_str().ok_or("image_phase")?;
        if phases.last().is_none_or(|last| last != phase) {
            phases.push(phase.to_owned());
        }
        if phase == "completed" {
            break;
        }
        if ["blocked", "failed", "cancelled", "quarantined"].contains(&phase) {
            return Err(format!("image_{phase}"));
        }
        if Instant::now() >= deadline {
            return Err("image_wait_deadline_no_retry".into());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let response = http(
        &app,
        &endpoint,
        &cookie,
        Some(json!({"operation":"artifact","session":session,"job_id":job})),
    )
    .await?;
    if !response.status().is_success()
        || response
            .headers()
            .get("content-type")
            .is_none_or(|v| v != "image/png")
    {
        return Err("image_artifact_unavailable".into());
    }
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|_| "png_body")?
        .to_bytes();
    let decoded =
        seiya_observe::image::artifact::decode(&bytes, 512, 512).map_err(|_| "png_validation")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join("result.png"))
        .map_err(|_| "png_evidence_create")?;
    file.write_all(&bytes).map_err(|_| "png_evidence_write")?;
    file.sync_all().map_err(|_| "png_evidence_sync")?;
    Ok(
        json!({"mode":"real-single-chat-image","chat_dispatches":1,"manual_image_submissions":0,"observed_phases":phases,"png":decoded,"browser_gui_validated":false,"physical_memory_release_measured":false}),
    )
}

#[tokio::test]
#[ignore = "requires explicit operator consent, installed weights and exclusive native preflight"]
async fn real_chat_image_once() {
    assert_eq!(
        std::env::var("SEIYA_REAL_IMAGE_APPROVAL").ok().as_deref(),
        Some("one-real-chat-image-512-no-downloads")
    );
    let path = PathBuf::from(
        std::env::var_os("SEIYA_REAL_IMAGE_CONFIG").expect("explicit private profile required"),
    );
    let profile = fs::read(&path).expect("profile readable");
    let config = crate::desktop_config::check_bytes(&profile).expect("valid isolated profile");
    let root = config.data_dir.clone();
    assert_eq!(root.parent(), Some(Path::new(native::BASE)));
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("seiya-chat-image-real-"));
    assert!(
        fs::read_dir(&root).unwrap().next().is_none(),
        "fresh empty data root required"
    );
    assert!(config.chat_images && config.features.images && !config.features.multiagent);
    let (stop, rx) = watch::channel(false);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (mount_tx, mount_rx) = tokio::sync::oneshot::channel();
    let host = tokio::spawn(async move {
        desktop(
            &profile,
            false,
            DesktopControl {
                ready: ready_tx,
                stop: rx,
                mount: Some(mount_tx),
                setup_home: Some("http://127.0.0.1:8015/".into()),
            },
        )
        .await
    });
    let result = async {
        let (ready, app) = tokio::time::timeout(Duration::from_secs(180), async {
            tokio::join!(ready_rx, mount_rx)
        })
        .await
        .map_err(|_| "host_preflight_deadline")?;
        exercise(
            app.map_err(|_| "host_mount_unavailable")?,
            ready.map_err(|_| "host_not_ready")?,
            &root,
        )
        .await
    }
    .await;
    stop.send_replace(true);
    let host_result = host
        .await
        .map_err(|_| "host_join_failure".to_owned())
        .and_then(|r| r);
    let evidence = json!({"result":result.as_ref().ok(),"error":result.as_ref().err(),"host_clean_shutdown":host_result.is_ok(),"host_error":host_result.as_ref().err(),"retry_attempted":false});
    save(&root, "acceptance-evidence.json", &evidence).expect("save private acceptance evidence");
    assert!(
        result.is_ok() && host_result.is_ok(),
        "single attempt blocked/failed; inspect private acceptance evidence (no retry)"
    );
}
