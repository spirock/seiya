//! Web-first desktop shell. No inference implementation is constructed at launch.
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::{watch, Mutex, Semaphore};
use tower::ServiceExt;

pub(crate) type Work<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Roles {
    pub brain: String,
    pub workers: Vec<String>,
    pub image: String,
}
impl Default for Roles {
    fn default() -> Self {
        Self {
            brain: crate::prepared_web::FOREGROUND.into(),
            workers: vec![],
            image: String::new(),
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Draft {
    pub llm_binary: String,
    pub model_store: String,
    pub image_binary: String,
    pub image_assets: String,
    pub roles: Roles,
}
impl Draft {
    fn bounded(&self) -> bool {
        [
            &self.llm_binary,
            &self.model_store,
            &self.image_binary,
            &self.image_assets,
        ]
        .into_iter()
        .all(|s| s.len() <= 4096 && !s.contains('\0'))
            && self.roles.workers.len() <= 16
            && std::iter::once(&self.roles.brain)
                .chain(&self.roles.workers)
                .chain(std::iter::once(&self.roles.image))
                .all(|s| s.len() <= 200 && !s.contains(['\0', '\n', '\r']))
            && self.roles.workers.iter().all(|s| !s.trim().is_empty())
            && self
                .roles
                .workers
                .iter()
                .enumerate()
                .all(|(i, s)| !self.roles.workers[..i].contains(s))
    }
    /// Capability intersection, not a claim that an arbitrary model ID works.
    pub(crate) fn executable_roles(&self) -> bool {
        self.roles.brain == crate::prepared_web::FOREGROUND
            && (self.roles.workers.is_empty()
                || self.roles.workers == [crate::prepared_web::AUXILIARY])
            && (self.roles.image.is_empty() || self.roles.image == "qwen-image-2.1-turbo-q4")
    }
}
pub(crate) struct Snapshot {
    pub draft: Draft,
    pub baseline: Vec<u8>,
    pub warning: Option<String>,
}
pub(crate) struct Approval {
    pub profile: Vec<u8>,
    pub draft: Draft,
    pub baseline: Vec<u8>,
}
pub(crate) struct Engine {
    pub router: Router,
    pub bootstrap: String,
    pub home: String,
    pub done: tokio::task::JoinHandle<Result<(), String>>,
}
pub(crate) trait Adapter: Send + Sync {
    fn available(&self) -> bool;
    fn allows_unassigned_configuration(&self) -> bool {
        false
    }
    fn supports(&self, draft: &Draft) -> bool {
        draft.executable_roles()
    }
    fn filesystem_checked(&self) -> bool {
        true
    }
    fn effects(&self) -> String {
        "Guardar solo modifica el perfil privado de Seiya. Iniciar prepara el host; no genera ni descarga modelos. Los motores siguen sujetos a pins, memoria y exclusión.".into()
    }
    fn snapshot(&self) -> Result<Snapshot, String>;
    fn review(&self, draft: Draft, baseline: Vec<u8>) -> Result<Approval, String>;
    fn save(&self, approval: &Approval) -> Result<(), String>;
    fn pick(&self, field: String) -> Work<'_, String>;
    fn start(
        &self,
        profile: Vec<u8>,
        solo: bool,
        stop: watch::Receiver<bool>,
        setup_home: String,
    ) -> Work<'_, Engine>;
}
struct Reviewed {
    token: String,
    approval: Approval,
}
struct Control {
    phase: &'static str,
    error: Option<String>,
    reviewed: Option<Reviewed>,
    stop: Option<watch::Sender<bool>>,
    chat: Option<String>,
    bootstrap: Option<String>,
}
struct Setup {
    home: std::sync::OnceLock<String>,
    adapter: Arc<dyn Adapter>,
    control: Mutex<Control>,
    operation: Arc<Semaphore>,
    bodies: Semaphore,
    mounted: Arc<std::sync::RwLock<Router>>,
    shutdown: watch::Sender<bool>,
}
impl Setup {
    fn new(adapter: Arc<dyn Adapter>, shutdown: watch::Sender<bool>) -> Arc<Self> {
        Arc::new(Self {
            home: std::sync::OnceLock::new(),
            adapter,
            control: Mutex::new(Control {
                phase: "unconfigured",
                error: None,
                reviewed: None,
                stop: None,
                chat: None,
                bootstrap: None,
            }),
            operation: Arc::new(Semaphore::new(1)),
            bodies: Semaphore::new(4),
            mounted: Arc::new(std::sync::RwLock::new(Router::new())),
            shutdown,
        })
    }
}
fn failure(code: impl Into<String>) -> Response {
    (StatusCode::CONFLICT, Json(json!({"error":code.into()}))).into_response()
}
async fn page() -> Html<String> {
    // Keep the chat's original styles/layout, but none of its inference JS runs
    // before configuration. No draft/history is read or cleared by this page.
    let source = include_str!("../../../swarm-app/seiya/ui.html");
    let shell = source.split("<script>").next().unwrap();
    Html(format!(
        "{shell}{}<script>{}\n{}</script></html>",
        include_str!("desktop_setup.html"),
        include_str!("desktop_setup.js"),
        include_str!("desktop_setup_feedback.js")
    ))
}
async fn status(State(s): State<Arc<Setup>>) -> Json<Value> {
    let c = s.control.lock().await;
    Json(
        json!({"platform":crate::platform::current().os.name(),"architecture":crate::platform::current().architecture,"runtime_available":s.adapter.available(),
        "phase":c.phase,"error":c.error,"chat":c.chat,
        "automatic_inference_requested":false,"inference_tested":false}),
    )
}
async fn detect(State(s): State<Arc<Setup>>) -> Response {
    match s.adapter.snapshot() {
        Ok(v) => {
            Json(json!({"draft":v.draft,"baseline":v.baseline,"warning":v.warning})).into_response()
        }
        Err(e) => failure(e),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pick {
    field: String,
}
async fn pick(State(s): State<Arc<Setup>>, Json(v): Json<Pick>) -> Response {
    let Ok(_permit) = s.operation.try_acquire() else {
        return failure("operation_busy");
    };
    if !matches!(
        v.field.as_str(),
        "llm_binary" | "model_store" | "image_binary" | "image_assets"
    ) {
        return failure("unknown_picker");
    }
    let mut shutdown = s.shutdown.subscribe();
    if *shutdown.borrow() {
        return failure("session_closed");
    }
    let result = tokio::select! {
        value=s.adapter.pick(v.field)=>value,
        _=shutdown.changed()=>Err("cancelled".into()),
    };
    match result {
        Ok(path) => Json(json!({"path":path})).into_response(),
        Err(e) => failure(e),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    draft: Draft,
    baseline: Vec<u8>,
}
async fn review(State(s): State<Arc<Setup>>, Json(v): Json<Review>) -> Response {
    let Ok(_permit) = s.operation.try_acquire() else {
        return failure("operation_busy");
    };
    let mut c = s.control.lock().await;
    if *s.shutdown.borrow() {
        return failure("session_closed");
    }
    if c.stop.is_some() {
        return failure("stop_runtime_before_editing");
    }
    c.reviewed = None;
    if !v.draft.bounded()
        || v.baseline.len() > 65536
        || (v.draft.roles.brain.trim().is_empty() && !s.adapter.allows_unassigned_configuration())
    {
        return failure("configuration_limits");
    }
    match s.adapter.review(v.draft, v.baseline) {
        Ok(approval) => {
            let token = uuid::Uuid::new_v4().to_string();
            let executable = s.adapter.available() && s.adapter.supports(&approval.draft);
            let summary = json!({"approval":token,"draft":approval.draft,"executable":executable,
                "filesystem_checked":s.adapter.filesystem_checked(),"inference_tested":false,
                "effects":s.adapter.effects()});
            c.reviewed = Some(Reviewed { token, approval });
            Json(summary).into_response()
        }
        Err(e) => failure(e),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Confirm {
    approval: String,
    start: bool,
}
async fn confirm(State(s): State<Arc<Setup>>, Json(v): Json<Confirm>) -> Response {
    let Ok(permit) = s.operation.clone().try_acquire_owned() else {
        return failure("operation_busy");
    };
    let mut c = s.control.lock().await;
    if *s.shutdown.borrow() {
        return failure("session_closed");
    }
    if c.stop.is_some() {
        return failure("runtime_busy");
    }
    let Some(reviewed) = c.reviewed.take() else {
        return failure("review_required");
    };
    if reviewed.token != v.approval {
        return failure("approval_invalid_or_consumed");
    }
    if reviewed.approval.baseline.len() > 65536 {
        return failure("configuration_limits");
    }
    if v.start && (!s.adapter.available() || !s.adapter.supports(&reviewed.approval.draft)) {
        return failure("selected_roles_not_supported_by_runtime");
    }
    if let Err(e) = s.adapter.save(&reviewed.approval) {
        return failure(e);
    }
    c.error = None;
    c.phase = "saved";
    if !v.start {
        return Json(json!({"saved":true,"started":false})).into_response();
    }
    let (stop, rx) = watch::channel(false);
    c.stop = Some(stop);
    c.phase = "starting";
    c.chat = None;
    c.bootstrap = None;
    let profile = reviewed.approval.profile;
    let solo = reviewed.approval.draft.roles.workers.is_empty()
        && reviewed.approval.draft.roles.image.is_empty();
    drop(c);
    tokio::spawn(async move {
        // The permit serializes startup, but Stop can always signal cancellation.
        match s
            .adapter
            .start(profile, solo, rx, s.home.get().unwrap().clone())
            .await
        {
            Ok(engine) => {
                *s.mounted.write().unwrap() = engine.router;
                let mut c = s.control.lock().await;
                if c.stop.as_ref().is_some_and(|v| *v.borrow()) {
                    c.phase = "stopping";
                } else {
                    c.phase = "running";
                    c.chat = Some(engine.home);
                    c.bootstrap = Some(engine.bootstrap);
                }
                drop(c);
                drop(permit);
                let result = engine
                    .done
                    .await
                    .unwrap_or_else(|_| Err("host_task_failed".into()));
                *s.mounted.write().unwrap() = Router::new();
                let mut c = s.control.lock().await;
                c.chat = None;
                c.bootstrap = None;
                c.phase = if result.is_ok() { "saved" } else { "blocked" };
                c.error = result.err();
                // Uncertain cleanup deliberately prevents another startup.
                if c.error.is_none() {
                    c.stop = None;
                }
            }
            Err(e) => {
                let mut c = s.control.lock().await;
                c.phase = "blocked";
                let uncertain = e.contains("cleanup_unconfirmed") || e == "host_task_failed";
                c.error = Some(e);
                // Adapter returns only after awaiting shutdown on startup error.
                if !uncertain {
                    c.stop = None;
                }
            }
        }
    });
    (
        StatusCode::ACCEPTED,
        Json(json!({"saved":true,"started":false,"phase":"starting"})),
    )
        .into_response()
}
async fn enter(State(s): State<Arc<Setup>>) -> Response {
    let mut c = s.control.lock().await;
    if c.phase != "running" {
        return failure("runtime_not_ready");
    }
    // Return the one-use link once, then only the cookie-authenticated home.
    let url = c.bootstrap.take().or_else(|| c.chat.clone());
    Json(json!({"url":url})).into_response()
}
async fn stop(State(s): State<Arc<Setup>>) -> Json<Value> {
    let mut c = s.control.lock().await;
    c.reviewed = None;
    if let Some(tx) = &c.stop {
        tx.send_replace(true);
        if c.phase != "blocked" {
            c.phase = "stopping";
        }
    }
    Json(json!({"phase":c.phase}))
}
async fn quit(State(s): State<Arc<Setup>>) -> StatusCode {
    let _ = stop(State(s.clone())).await;
    s.shutdown.send_replace(true);
    StatusCode::ACCEPTED
}
fn application(s: Arc<Setup>) -> (Router, String, String) {
    let inner = Router::new()
        .route("/", get(page))
        .route("/api/status", get(status))
        .route("/api/detect", get(detect))
        .route("/api/pick", post(pick))
        .route("/api/review", post(review))
        .route("/api/confirm", post(confirm))
        .route("/api/enter", post(enter))
        .route("/api/stop", post(stop))
        .route("/api/quit", post(quit))
        .layer(DefaultBodyLimit::max(128 * 1024))
        .layer(axum::middleware::from_fn_with_state(s.clone(), admission))
        .with_state(s.clone());
    let (app, bootstrap, home) =
        crate::pair_trial::access::desktop_shell("127.0.0.1:8015".parse().unwrap(), inner);
    s.home.set(home.clone()).unwrap();
    let prefix = reqwest::Url::parse(&home).unwrap().path().to_owned();
    let bootstrap_path = reqwest::Url::parse(&bootstrap).unwrap().path().to_owned();
    let dispatch = Router::new().fallback(move |r: Request| {
        let selected = if r.uri().path().starts_with(&prefix) || r.uri().path() == bootstrap_path {
            app.clone()
        } else {
            s.mounted.read().unwrap().clone()
        };
        async move {
            selected
                .oneshot(r)
                .await
                .unwrap_or_else(|never| match never {})
        }
    });
    (dispatch, bootstrap, home)
}
async fn admission(
    State(s): State<Arc<Setup>>,
    r: Request,
    next: axum::middleware::Next,
) -> Response {
    if *s.shutdown.borrow() {
        return StatusCode::GONE.into_response();
    }
    let Ok(permit) = s.bodies.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let (parts, body) = r.into_parts();
    let body = match tokio::time::timeout(
        Duration::from_secs(5),
        axum::body::to_bytes(body, 128 * 1024),
    )
    .await
    {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    drop(permit);
    if *s.shutdown.borrow() {
        return StatusCode::GONE.into_response();
    }
    next.run(Request::from_parts(parts, axum::body::Body::from(body)))
        .await
}
pub(crate) async fn run(
    adapter: Arc<dyn Adapter>,
    open: impl FnOnce(String) -> Work<'static, String>,
) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8015")
        .await
        .map_err(|_| "desktop_port_busy_no_adoption")?;
    let (shutdown, mut rx) = watch::channel(false);
    let signal_shutdown = shutdown.clone();
    let signal = tokio::spawn(async move {
        termination().await;
        signal_shutdown.send_replace(true);
    });
    let state = Setup::new(adapter, shutdown.clone());
    let (app, bootstrap, _) = application(state.clone());
    let mut server = tokio::spawn(async move { axum::serve(listener, app).await });
    let opened =
        tokio::select! { value=open(bootstrap)=>value, _=rx.changed()=>Err("cancelled".into()) };
    let mut server_ended = false;
    if opened.is_ok() && !*rx.borrow() {
        tokio::select! {
            _=rx.changed()=>{},
            _=tokio::time::sleep(Duration::from_secs(1800))=>{},
            _=&mut server=>{server_ended=true; shutdown.send_replace(true);},
        }
    }
    shutdown.send_replace(true);
    signal.abort();
    let _ = signal.await;
    let _ = stop(State(state.clone())).await;
    // Stop remains live until all runtime work confirms cleanup; no force-kill.
    loop {
        let c = state.control.lock().await;
        if c.stop.is_none() || c.phase == "blocked" {
            break;
        }
        drop(c);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    server.abort();
    if !server_ended {
        let _ = server.await;
    }
    let c = state.control.lock().await;
    if c.phase == "blocked" && c.stop.is_some() {
        return Err("host_cleanup_unconfirmed".into());
    }
    if server_ended {
        return Err("desktop_web_ended_unexpectedly".into());
    }
    match opened {
        Ok(_) => Ok(()),
        Err(e) if e == "cancelled" => Ok(()),
        Err(e) => Err(e),
    }
}
async fn termination() {
    #[cfg(unix)]
    {
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _=term.recv()=>{}, _=tokio::signal::ctrl_c()=>{} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
struct Unsupported;
#[cfg(test)]
impl Adapter for Unsupported {
    fn available(&self) -> bool {
        false
    }
    fn snapshot(&self) -> Result<Snapshot, String> {
        Ok(Snapshot { draft: Draft::default(), baseline: vec![], warning: Some("Esta plataforma todavía no tiene adaptador de inferencia local. El chat está desactivado; no se usa un proveedor remoto.".into()) })
    }
    fn review(&self, _: Draft, _: Vec<u8>) -> Result<Approval, String> {
        Err("platform_adapter_not_implemented".into())
    }
    fn save(&self, _: &Approval) -> Result<(), String> {
        Err("platform_adapter_not_implemented".into())
    }
    fn pick(&self, _: String) -> Work<'_, String> {
        Box::pin(async { Err("platform_picker_not_implemented".into()) })
    }
    fn start(&self, _: Vec<u8>, _: bool, _: watch::Receiver<bool>, _: String) -> Work<'_, Engine> {
        Box::pin(async { Err("platform_adapter_not_implemented".into()) })
    }
}
pub async fn launch() -> Result<(), String> {
    launch_catalog(None).await
}
pub async fn launch_catalog(explicit: Option<std::path::PathBuf>) -> Result<(), String> {
    let root = crate::local_models::storage::default_root()?;
    let mut path = explicit
        .clone()
        .unwrap_or_else(|| root.join("local-models.json"));
    if !path.is_absolute() {
        return Err("absolute_catalog_path_required".into());
    }
    if explicit.is_some()
        && std::fs::symlink_metadata(&path)
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
    {
        path = path
            .canonicalize()
            .map_err(|_| "catalog_canonical_path_required")?;
    }
    let portable = explicit.is_some() || std::fs::symlink_metadata(&path).is_ok();
    #[cfg(target_os = "macos")]
    let adapter: Arc<dyn Adapter> = if portable {
        Arc::new(crate::local_models::desktop::Desktop::new(root, path))
    } else {
        Arc::new(crate::launcher::WebAdapter::new()?)
    };
    #[cfg(not(target_os = "macos"))]
    let adapter: Arc<dyn Adapter> = {
        let _ = portable;
        Arc::new(crate::local_models::desktop::Desktop::new(root, path))
    };
    run(adapter, |url| {
        Box::pin(async move {
            #[cfg(target_os = "macos")]
            let mut command = tokio::process::Command::new("/usr/bin/open");
            #[cfg(target_os = "windows")]
            let mut command = {
                let root = std::env::var_os("SystemRoot").ok_or("windows_system_root_required")?;
                let mut c = tokio::process::Command::new(
                    std::path::Path::new(&root).join("System32/rundll32.exe"),
                );
                c.arg("url.dll,FileProtocolHandler");
                c
            };
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let mut command = tokio::process::Command::new("xdg-open");
            let status = tokio::time::timeout(
                Duration::from_secs(10),
                command
                    .arg(&url)
                    .kill_on_drop(true)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status(),
            )
            .await
            .map_err(|_| "browser_open_timeout")?
            .map_err(|_| "browser_open_failed")?;
            if status.success() {
                Ok(String::new())
            } else {
                Err("browser_open_failed".into())
            }
        })
    })
    .await
}

#[cfg(test)]
mod tests;
