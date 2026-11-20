//! Operaciones explícitas de gestión sobre la API local de Ollama.
use super::{Assignments, Catalog, Model};
use crate::runtime::{Backend, HttpTransport, RuntimeConfig, TransportError};
use serde::Deserialize;
use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
};

pub(crate) const OWNED_PORT: u16 = 49401;
const OWNED_API: &str = "http://127.0.0.1:49401";
const OWNED_ID: &str = "ollama-managed-brain";
const OWNED_BASE: &str = "http://127.0.0.1:49401/v1";

/// Obtiene el almacén privado de Seiya y comprueba sus permisos sin explorar su contenido.
pub(crate) fn private_store() -> Result<std::path::PathBuf, String> {
    let root = super::storage::default_root()?;
    super::storage::ensure_directory(&root)?;
    let store = root.join("ollama-models");
    super::storage::ensure_directory(&store)?;
    Ok(store)
}

/// Resuelve el ejecutable guardado o la primera ruta habitual, sin ejecutarlo.
fn configured_binary() -> Result<String, &'static str> {
    let root = super::storage::default_root().map_err(|_| "ollama_binary_invalid")?;
    if let Some(bytes) = super::storage::read(&root.join("local-model-roles.json"))
        .map_err(|_| "ollama_binary_invalid")?
    {
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "ollama_binary_invalid")?;
        if let Some(path) = value["ollama_binary"]
            .as_str()
            .filter(|path| !path.is_empty())
        {
            return Ok(path.to_owned());
        }
    }
    let (path, _) = super::ollama::native(None);
    if path.is_empty() {
        Err("ollama_binary_invalid")
    } else {
        Ok(path)
    }
}

/// Inicia solo el servidor de gestión privado; no carga ni genera con modelos.
async fn launch_manager(binary: Option<&str>) -> Result<tokio::process::Child, &'static str> {
    if !supported() {
        return Err("model_manager_unsupported");
    }
    let binary = binary
        .map(str::to_owned)
        .map(Ok)
        .unwrap_or_else(configured_binary)?;
    let path = std::path::Path::new(&binary)
        .canonicalize()
        .map_err(|_| "ollama_binary_invalid")?;
    let metadata = std::fs::metadata(&path).map_err(|_| "ollama_binary_invalid")?;
    if !metadata.is_file() {
        return Err("ollama_binary_invalid");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err("ollama_binary_invalid");
        }
    }
    let store = private_store().map_err(|_| "ollama_private_store_invalid")?;
    if tokio::net::TcpListener::bind(("127.0.0.1", OWNED_PORT))
        .await
        .is_err()
    {
        return Err("ollama_owned_port_busy");
    }
    spawn_owned(path, store).map_err(|_| "ollama_owned_start_failed")
}

fn spawn_owned(
    path: std::path::PathBuf,
    store: std::path::PathBuf,
) -> std::io::Result<tokio::process::Child> {
    let mut command = tokio::process::Command::new(path);
    command
        .arg("serve")
        .env("OLLAMA_HOST", format!("127.0.0.1:{OWNED_PORT}"))
        .env("OLLAMA_MODELS", store)
        .env("OLLAMA_NO_CLOUD", "1")
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    command.spawn()
}

/// Espera la API privada y devuelve sus tags; no solicita carga ni inferencia.
async fn wait_for_tags() -> Result<reqwest::Response, &'static str> {
    let client = client()?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(12);
    loop {
        match client.get(format!("{OWNED_API}/api/tags")).send().await {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(_) => return Err("ollama_api_error"),
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            Err(_) => return Err("ollama_owned_start_timeout"),
        }
    }
}

#[derive(Deserialize)]
struct Tags {
    models: Vec<Tag>,
}

#[derive(Deserialize)]
struct Tag {
    name: String,
    size: u64,
    modified_at: String,
}

#[derive(Clone, serde::Serialize)]
struct Progress {
    active: bool,
    name: Option<String>,
    status: String,
    completed: Option<u64>,
    total: Option<u64>,
    error: Option<String>,
}

struct Download {
    progress: Progress,
    abort: Option<tokio::task::AbortHandle>,
}

fn download() -> &'static Mutex<Option<Download>> {
    static DOWNLOAD: OnceLock<Mutex<Option<Download>>> = OnceLock::new();
    DOWNLOAD.get_or_init(|| Mutex::new(None))
}

fn installed_models() -> &'static Mutex<HashSet<String>> {
    static MODELS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    MODELS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Comprueba la última lista obtenida explícitamente desde el gestor.
pub(crate) fn installed(id: &str) -> bool {
    installed_models()
        .lock()
        .is_ok_and(|models| models.contains(id))
}

/// Indica si el gestor local de Ollama está disponible en esta plataforma.
pub(crate) fn supported() -> bool {
    cfg!(target_os = "linux")
}

/// Valida identificadores antes de enviarlos a la API local de Ollama.
pub(crate) fn valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':' | b'/'))
        && !id.starts_with('/')
        && !id.ends_with('/')
        && !id.contains("..")
        && id.split('/').all(|part| !part.is_empty())
}

/// Construye un perfil temporal de chat para un modelo instalado de Ollama.
pub(crate) fn managed_catalog(model: &str) -> Result<Vec<u8>, &'static str> {
    if !valid_model_id(model) {
        return Err("invalid_model_id");
    }
    let runtime = RuntimeConfig {
        id: OWNED_ID.into(),
        model: model.into(),
        backend: Backend::OpenaiLocal,
        base_url: OWNED_BASE.into(),
        timeout_seconds: 120,
        max_output_tokens: 256,
    };
    let catalog = Catalog {
        version: 1,
        models: vec![Model {
            id: OWNED_ID.into(),
            runtime,
            supports_tools: false,
            max_input_bytes: 16 * 1024,
        }],
        assignments: Assignments {
            brain: OWNED_ID.into(),
            workers: vec![],
        },
        minimum_available_mib: 2048,
        web_search: false,
        image_models: vec![],
        image: None,
        image_chat: false,
    };
    serde_json::to_vec(&catalog).map_err(|_| "catalog_encode")
}

/// Devuelve el binding si el perfil corresponde a nuestro endpoint dedicado.
pub(crate) fn managed_model(catalog: &Catalog) -> Option<&str> {
    if catalog.models.len() != 1
        || catalog.assignments.brain != OWNED_ID
        || !catalog.assignments.workers.is_empty()
        || catalog.web_search
        || catalog.image.is_some()
        || catalog.image_chat
    {
        return None;
    }
    let model = &catalog.models[0];
    (model.id == OWNED_ID
        && model.runtime.id == OWNED_ID
        && model.runtime.base_url == OWNED_BASE
        && !model.supports_tools)
        .then_some(model.runtime.model.as_str())
}

/// Inicia el servidor dedicado y espera su inventario local, nunca una inferencia.
pub(crate) async fn launch_owned(
    binary: &str,
    model: &str,
) -> Result<tokio::process::Child, String> {
    if !supported() || !valid_model_id(model) {
        return Err("model_manager_unsupported".into());
    }
    let mut child = launch_manager(Some(binary)).await.map_err(str::to_owned)?;
    let runtime = RuntimeConfig {
        id: OWNED_ID.into(),
        model: model.into(),
        backend: Backend::OpenaiLocal,
        base_url: OWNED_BASE.into(),
        timeout_seconds: 3,
        max_output_tokens: 1,
    };
    let transport = match HttpTransport::new(&runtime) {
        Ok(transport) => transport,
        Err(_) => {
            let _ = stop_owned(child).await;
            return Err("ollama_owned_start_failed".into());
        }
    };
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(12);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Err("ollama_owned_start_failed".into()),
            Err(_) => {
                let _ = stop_owned(child).await;
                return Err("ollama_owned_start_failed".into());
            }
            Ok(None) => {}
        }
        match transport.advertises(model).await {
            Ok(true) => return Ok(child),
            Ok(false) => {
                let _ = stop_owned(child).await;
                return Err("ollama_model_not_installed".into());
            }
            Err(TransportError::NotConnected | TransportError::Timeout) => {}
            Err(_) => {
                let _ = stop_owned(child).await;
                return Err("ollama_owned_start_failed".into());
            }
        }
        if tokio::time::Instant::now() >= deadline {
            let _ = stop_owned(child).await;
            return Err("ollama_owned_start_timeout".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// Detiene y espera el grupo de procesos Ollama iniciado por Seiya.
pub(crate) async fn stop_owned(mut child: tokio::process::Child) -> Result<(), String> {
    #[cfg(unix)]
    {
        let pid = child.id().ok_or("ollama_owned_stop_failed")? as i32;
        let group = -pid;
        // Every Ollama runner inherits the private process group created at spawn.
        let _ = unsafe { libc::kill(group, libc::SIGTERM) };
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let _ = unsafe { libc::kill(group, libc::SIGKILL) };
        child
            .wait()
            .await
            .map(|_| ())
            .map_err(|_| "ollama_owned_stop_failed".into())
    }
    #[cfg(not(unix))]
    {
        if child
            .try_wait()
            .map_err(|_| "ollama_owned_stop_failed")?
            .is_none()
        {
            child.kill().await.map_err(|_| "ollama_owned_stop_failed")?;
        }
        child
            .wait()
            .await
            .map(|_| ())
            .map_err(|_| "ollama_owned_stop_failed".into())
    }
}

fn client() -> Result<reqwest::Client, &'static str> {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| "ollama_client_unavailable")
}

/// Consulta el almacén privado mediante un servidor temporal sin inferencia.
pub(crate) async fn list() -> Result<serde_json::Value, &'static str> {
    if !supported() {
        return Err("model_manager_unsupported");
    }
    if download()
        .lock()
        .map_err(|_| "model_manager_busy")?
        .as_ref()
        .is_some_and(|job| job.progress.active)
    {
        return Err("model_manager_busy");
    }
    let child = launch_manager(None).await?;
    let result = async {
        let response = wait_for_tags().await?;
        let body = response
            .bytes()
            .await
            .map_err(|_| "ollama_invalid_response")?;
        if body.len() > 1024 * 1024 {
            return Err("ollama_invalid_response");
        }
        serde_json::from_slice::<Tags>(&body).map_err(|_| "ollama_invalid_response")
    }
    .await;
    let stop_result = stop_owned(child)
        .await
        .map_err(|_| "ollama_owned_stop_failed");
    let tags = result?;
    stop_result?;
    if tags.models.len() > 1000 {
        return Err("ollama_invalid_response");
    }
    let models = tags
        .models
        .into_iter()
        .filter(|model| valid_model_id(&model.name))
        .collect::<Vec<_>>();
    if let Ok(mut installed) = installed_models().lock() {
        *installed = models.iter().map(|model| model.name.clone()).collect();
    }
    Ok(serde_json::json!({
        "models": models.into_iter().map(|m| serde_json::json!({
            "name": m.name,
            "size": m.size,
            "modified_at": m.modified_at
        })).collect::<Vec<_>>()
    }))
}

/// Descarga explícitamente al almacén privado de Seiya.
pub(crate) async fn pull(id: &str) -> Result<serde_json::Value, &'static str> {
    if !supported() {
        return Err("model_manager_unsupported");
    }
    if !valid_model_id(id) {
        return Err("invalid_model_id");
    }
    let mut state = download().lock().map_err(|_| "model_manager_busy")?;
    if state.as_ref().is_some_and(|job| job.progress.active) {
        return Err("model_manager_busy");
    }
    *state = Some(Download {
        progress: Progress {
            active: true,
            name: Some(id.to_owned()),
            status: "Iniciando el gestor privado de Ollama…".into(),
            completed: None,
            total: None,
            error: None,
        },
        abort: None,
    });
    let id = id.to_owned();
    let task = tokio::spawn(async move {
        match pull_private(&id).await {
            Ok(()) => {
                if let Ok(mut installed) = installed_models().lock() {
                    installed.insert(id);
                }
                finish(true, "Descarga privada terminada.", None);
            }
            Err(error) => finish(
                false,
                "Falló la descarga en el almacén privado.",
                Some(error),
            ),
        }
    });
    if let Some(job) = state.as_mut() {
        job.abort = Some(task.abort_handle());
    }
    Ok(serde_json::json!({"started":true}))
}

async fn pull_private(id: &str) -> Result<(), &'static str> {
    let child = launch_manager(None).await?;
    let result = async {
        let _ = wait_for_tags().await?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(std::time::Duration::from_secs(3))
            .timeout(std::time::Duration::from_secs(60 * 60))
            .build()
            .map_err(|_| "ollama_client_unavailable")?;
        let mut response = client
            .post(format!("{OWNED_API}/api/pull"))
            .json(&serde_json::json!({"name": id, "stream": true}))
            .send()
            .await
            .map_err(|_| "ollama_unreachable")?;
        if !response.status().is_success() {
            return Err("ollama_pull_rejected");
        }
        let mut buffer = Vec::new();
        let mut success = false;
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    buffer.extend_from_slice(&chunk);
                    if buffer.len() > 64 * 1024 && !buffer.contains(&b'\n') {
                        return Err("ollama_invalid_response");
                    }
                    while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
                        let line: Vec<u8> = buffer.drain(..=end).collect();
                        let event: serde_json::Value =
                            serde_json::from_slice(&line).map_err(|_| "ollama_invalid_response")?;
                        if event.get("error").is_some() {
                            return Err("ollama_pull_failed");
                        }
                        let status = event
                            .get("status")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("Descargando…")
                            .to_owned();
                        let completed = event.get("completed").and_then(serde_json::Value::as_u64);
                        let total = event.get("total").and_then(serde_json::Value::as_u64);
                        update(status.clone(), completed, total);
                        success |= status == "success";
                    }
                }
                Ok(None) => break,
                Err(_) => return Err("ollama_stream_error"),
            }
        }
        if success {
            Ok(())
        } else {
            Err("ollama_pull_failed")
        }
    }
    .await;
    let stopped = stop_owned(child).await;
    result?;
    stopped.map_err(|_| "ollama_owned_stop_failed")
}

/// Devuelve el progreso actual sin iniciar otra petición al motor.
pub(crate) fn progress() -> Result<serde_json::Value, &'static str> {
    let state = download().lock().map_err(|_| "model_manager_busy")?;
    Ok(serde_json::json!(state
        .as_ref()
        .map(|job| &job.progress)
        .cloned()
        .unwrap_or(Progress {
            active: false,
            name: None,
            status: "Sin descargas recientes.".into(),
            completed: None,
            total: None,
            error: None,
        })))
}

/// Cancela la solicitud activa cerrando la conexión de descarga a Ollama.
pub(crate) fn cancel() -> Result<(), &'static str> {
    let mut state = download().lock().map_err(|_| "model_manager_busy")?;
    let Some(job) = state.as_mut().filter(|job| job.progress.active) else {
        return Err("no_active_download");
    };
    if let Some(abort) = job.abort.take() {
        abort.abort();
    }
    job.progress.active = false;
    job.progress.status = "Cancelación solicitada.".into();
    Ok(())
}

fn update(status: String, completed: Option<u64>, total: Option<u64>) {
    if let Ok(mut state) = download().lock() {
        if let Some(job) = state.as_mut().filter(|job| job.progress.active) {
            job.progress.status = status;
            job.progress.completed = completed;
            job.progress.total = total;
        }
    }
}

fn finish(success: bool, status: &str, error: Option<&str>) {
    if let Ok(mut state) = download().lock() {
        if let Some(job) = state.as_mut().filter(|job| job.progress.active) {
            job.progress.active = false;
            job.progress.status = status.into();
            job.progress.error = error.map(str::to_owned);
            if success {
                job.progress.completed = job.progress.total;
            }
        }
    }
}

/// Elimina del almacén privado un modelo cuyo ID confirmó el usuario.
pub(crate) async fn delete(id: &str) -> Result<(), &'static str> {
    if !supported() {
        return Err("model_manager_unsupported");
    }
    if !valid_model_id(id) {
        return Err("invalid_model_id");
    }
    if download()
        .lock()
        .map_err(|_| "model_manager_busy")?
        .as_ref()
        .is_some_and(|job| job.progress.active)
    {
        return Err("model_manager_busy");
    }
    let child = launch_manager(None).await?;
    let result = async {
        let _ = wait_for_tags().await?;
        let response = client()?
            .delete(format!("{OWNED_API}/api/delete"))
            .json(&serde_json::json!({"name": id}))
            .send()
            .await
            .map_err(|_| "ollama_unreachable")?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err("ollama_delete_rejected")
        }
    }
    .await;
    let stopped = stop_owned(child).await;
    result?;
    stopped.map_err(|_| "ollama_owned_stop_failed")?;
    {
        if let Ok(mut installed) = installed_models().lock() {
            installed.remove(id);
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn remember_installed_for_test(id: &str) {
    installed_models().lock().unwrap().insert(id.to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids_are_constrained_to_safe_ollama_names() {
        for id in ["gemma3:4b", "team/model:latest", "qwen3.5:9b"] {
            assert!(valid_model_id(id), "{id}");
        }
        for id in [
            "",
            "../etc/passwd",
            "/root",
            "a//b",
            "model name",
            "model\n",
        ] {
            assert!(!valid_model_id(id), "{id:?}");
        }
    }

    #[test]
    fn progress_is_inert_and_cancel_requires_an_active_download() {
        let progress = progress().unwrap();
        assert_eq!(progress["active"], false);
        assert_eq!(progress["name"], serde_json::Value::Null);
        assert_eq!(cancel().unwrap_err(), "no_active_download");
    }

    #[test]
    fn managed_profile_uses_only_the_dedicated_loopback_and_no_tools() {
        let bytes = managed_catalog("gemma4:12b").unwrap();
        let catalog = Catalog::parse(&bytes).unwrap();
        assert_eq!(managed_model(&catalog), Some("gemma4:12b"));
        assert_eq!(catalog.models[0].runtime.base_url, OWNED_BASE);
        assert!(!catalog.models[0].supports_tools);
        assert!(catalog.assignments.workers.is_empty());
        assert!(!catalog.web_search && !catalog.image_chat);
        assert!(managed_catalog("../bad-model").is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stopping_owned_ollama_also_stops_its_runner_process_group() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!("seiya-owned-group-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let script = root.join("fake-ollama");
        let runner_pid = root.join("runner.pid");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\necho $! > '{}'\nwait\n",
                runner_pid.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&script, permissions).unwrap();

        let child = spawn_owned(script, root.clone()).unwrap();
        let pid = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if let Ok(value) = std::fs::read_to_string(&runner_pid) {
                    if let Ok(pid) = value.trim().parse::<i32>() {
                        break pid;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runner pid should be written");
        stop_owned(child).await.unwrap();
        for _ in 0..50 {
            if unsafe { libc::kill(pid, 0) } != 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_ne!(
            unsafe { libc::kill(pid, 0) },
            0,
            "runner process survived stop"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
