//! Native macOS onboarding and lifecycle. Tests inject UI/browser/host adapters.
use crate::{desktop_config, image_web};
use seiya_observe::image::coordination::{private_directory, profile_lock};
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    future::Future,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
};
use tokio::sync::{oneshot, watch};

type Result<T> = std::result::Result<T, &'static str>;
type UiFuture<'a> = Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Menu,
    Profile,
    Llm,
    Models,
    WantImages,
    ImageBinary,
    ImageAssets,
    Save,
    Start,
    Preparing,
    Running,
}
trait Ui: Send + Sync {
    fn ask(&self, step: Step) -> UiFuture<'_>;
    fn review(&self, summary: String) -> UiFuture<'_>;
    fn open(&self, url: String) -> UiFuture<'_>;
}
pub(crate) struct Ready {
    pub bootstrap: String,
    pub home: String,
}
struct HostRun {
    ready: oneshot::Receiver<Ready>,
    stop: watch::Sender<bool>,
    done: tokio::task::JoinHandle<std::result::Result<(), String>>,
}
trait Host: Send + Sync {
    fn start(&self, profile: Vec<u8>, chat_only: bool) -> HostRun;
}
fn selected_bytes(path: &Path) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(0x100 /* macOS O_NOFOLLOW */)
        .open(path)
        .map_err(|_| "profile_open")?;
    let metadata = file.metadata().map_err(|_| "profile_metadata")?;
    if !metadata.is_file() || metadata.len() > 65536 {
        return Err("profile_regular_bounded_required");
    }
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "profile_read")?;
    desktop_config::check_bytes(&bytes).map_err(|e| e.code)?;
    Ok(bytes)
}

fn root(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Seiya")
}
fn profile(home: &Path) -> PathBuf {
    root(home).join("desktop.json")
}
fn baseline(home: &Path) -> Result<Option<Vec<u8>>> {
    let base = root(home);
    match fs::symlink_metadata(&base) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("profile_directory_unreadable"),
        Ok(_) => private_directory(&base).map_err(|_| "profile_directory_not_private")?,
    }
    read_private(&profile(home))
}
fn read_private(path: &Path) -> Result<Option<Vec<u8>>> {
    let m = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("profile_unreadable"),
    };
    if !m.is_file() || m.permissions().mode() & 0o077 != 0 || m.len() > 65536 {
        return Err("profile_regular_private_bounded_required");
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(0x100 /* macOS O_NOFOLLOW */)
        .open(path)
        .map_err(|_| "profile_open")?;
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "profile_read")?;
    if bytes.len() > 65536 {
        return Err("profile_too_large");
    }
    Ok(Some(bytes))
}
fn prepare(home: &Path) -> Result<File> {
    let parent = home.join("Library/Application Support");
    if !home.is_absolute()
        || home.canonicalize().ok().as_deref() != Some(home)
        || parent.canonicalize().ok().as_deref() != Some(parent.as_path())
    {
        return Err("canonical_home_application_support_required");
    }
    let base = root(home);
    for path in [&base, &base.join("data"), &base.join("coordination")] {
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("private_directory_create"),
        }
        private_directory(path).map_err(|_| "private_directory_required")?;
    }
    profile_lock(&base).map_err(|_| "launcher_profile_busy")
}
fn save(home: &Path, expected: Option<Vec<u8>>, bytes: &[u8]) -> Result<PathBuf> {
    desktop_config::check_bytes(bytes).map_err(|e| e.code)?;
    let path = profile(home);
    if read_private(&path)? != expected {
        return Err("profile_changed_no_overwrite");
    }
    let temp = root(home).join(format!("desktop-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|_| "profile_temp_create")?;
    if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
        let _ = fs::remove_file(&temp);
        return Err("profile_temp_write");
    }
    if read_private(&path)? != expected {
        let _ = fs::remove_file(&temp);
        return Err("profile_changed_no_overwrite");
    }
    if fs::rename(&temp, &path).is_err() {
        let _ = fs::remove_file(&temp);
        return Err("profile_commit_failed");
    }
    File::open(root(home))
        .and_then(|f| f.sync_all())
        .map_err(|_| "profile_save_uncertain_do_not_retry")?;
    Ok(path)
}
async fn ask(ui: &dyn Ui, step: Step, shutdown: &mut watch::Receiver<bool>) -> Result<String> {
    if *shutdown.borrow() {
        return Err("cancelled");
    }
    tokio::select! {r=ui.ask(step)=>r,_=shutdown.changed()=>Err("cancelled")}
}
async fn configure(
    ui: &dyn Ui,
    home: &Path,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<PathBuf> {
    let expected = baseline(home)?;
    let binary = ask(ui, Step::Llm, shutdown).await?;
    let models = ask(ui, Step::Models, shutdown).await?;
    let image_choice = ask(ui, Step::WantImages, shutdown).await?;
    let images = match image_choice.as_str() {
        "Añadir imágenes" => Some(
            json!({"binary":ask(ui,Step::ImageBinary,shutdown).await?,"assets_dir":ask(ui,Step::ImageAssets,shutdown).await?}),
        ),
        "Sin imágenes" => None,
        _ => return Err("cancelled"),
    };
    if ask(ui, Step::Save, shutdown).await? != "Guardar" {
        return Err("cancelled");
    }
    let _lock = prepare(home)?;
    let bytes=serde_json::to_vec_pretty(&json!({"version":1,"target":"macos","data_dir":root(home).join("data"),"coordination_dir":root(home).join("coordination"),"llm_binary":binary,"model_store":models,"images":images,"features":{"images":false,"multiagent":false}})).map_err(|_|"profile_encode")?;
    save(home, expected, &bytes)
}

fn valid_url(url: &str, bootstrap: bool) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else {
        return false;
    };
    let prefix = if bootstrap { "/session/" } else { "/trial/" };
    u.scheme() == "http"
        && u.host_str() == Some("127.0.0.1")
        && u.port() == Some(8015)
        && u.username().is_empty()
        && u.password().is_none()
        && u.query().is_none()
        && u.fragment().is_none()
        && u.path().strip_prefix(prefix).is_some_and(|p| {
            let p = if bootstrap {
                p
            } else {
                p.strip_suffix('/').unwrap_or("")
            };
            p.len() == if bootstrap { 64 } else { 32 } && p.bytes().all(|b| b.is_ascii_hexdigit())
        })
}
async fn stop(run: HostRun) -> Result<()> {
    run.stop.send_replace(true);
    run.done
        .await
        .map_err(|_| "host_task_failed")?
        .map_err(|_| "host_cleanup_unconfirmed")
}
async fn run_with(
    ui: &dyn Ui,
    host: &dyn Host,
    home: &Path,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let selected = match ask(ui, Step::Menu, &mut shutdown).await?.as_str() {
        "Configurar" => configure(ui, home, &mut shutdown).await?,
        "Abrir perfil" => PathBuf::from(ask(ui, Step::Profile, &mut shutdown).await?),
        _ => return Ok(()),
    };
    let approved = selected_bytes(&selected)?;
    let config = desktop_config::check_bytes(&approved).map_err(|e| e.code)?;
    let summary=format!("Revisa las rutas antes de autorizar el host. Un JSON no concede permisos por sí solo.\nMotor LLM: {:?}\nAlmacén: {:?}\nDatos/historial: {:?}\nImágenes adjuntas: {}\nModelos del ensayo fijados; no descargas ni cambio de proveedor.",config.llm_binary,config.model_store,config.data_dir,config.images.is_some());
    tokio::select! {r=ui.review(summary)=>{r?;},_=shutdown.changed()=>return Err("cancelled")};
    let mode = ask(ui, Step::Start, &mut shutdown).await?;
    let chat_only = match mode.as_str() {
        "Solo chat" => true,
        "Con opciones" => false,
        _ => return Ok(()),
    };
    let mut run = host.start(approved, chat_only);
    enum Startup {
        Ready(std::result::Result<Ready, oneshot::error::RecvError>),
        Ended,
        Cancel,
    }
    let start = tokio::select! {
        r=&mut run.ready=>Startup::Ready(r),
        _=&mut run.done=>Startup::Ended,
        _=ask(ui,Step::Preparing,&mut shutdown)=>Startup::Cancel,
    };
    let ready = match start {
        Startup::Ready(Ok(r)) => r,
        Startup::Ready(Err(_)) => {
            stop(run).await?;
            return Err("host_did_not_become_ready");
        }
        Startup::Ended => return Err("host_start_failed_no_browser_opened"),
        Startup::Cancel => return stop(run).await,
    };
    if !valid_url(&ready.bootstrap, true) || !valid_url(&ready.home, false) {
        stop(run).await?;
        return Err("invalid_host_browser_url");
    }
    let opened =
        tokio::select! {r=ui.open(ready.bootstrap)=>r,_=shutdown.changed()=>Err("cancelled")};
    if opened.is_err() {
        stop(run).await?;
        return Err("browser_open_failed_host_stopped");
    }
    loop {
        tokio::select! {
            result=&mut run.done=>return result.map_err(|_|"host_task_failed")?.map_err(|_|"host_cleanup_unconfirmed"),
            choice=ask(ui,Step::Running,&mut shutdown)=>{
                if choice.as_deref()==Ok("Abrir chat"){
                    let opened=tokio::select!{r=ui.open(ready.home.clone())=>r,_=shutdown.changed()=>Err("cancelled")};
                    if opened.is_err(){stop(run).await?;return Err("browser_open_failed_host_stopped");}
                }else{return stop(run).await;}
            }
        }
    }
}

struct NativeUi;
async fn script(source: &str) -> Result<String> {
    let output = tokio::process::Command::new("/usr/bin/osascript")
        .args(["-e", source])
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|_| "native_dialog_unavailable")?;
    if !output.status.success() {
        return Err("cancelled");
    }
    if output.stdout.len() > 8192 {
        return Err("dialog_result_too_large");
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim_end_matches(['\r', '\n']).to_owned())
        .map_err(|_| "dialog_result_encoding")
}
impl Ui for NativeUi {
    fn review(&self, summary: String) -> UiFuture<'_> {
        Box::pin(async move {
            let output=tokio::process::Command::new("/usr/bin/osascript").args(["-e","on run argv\nreturn button returned of (display dialog (item 1 of argv) with title \"Seiya Rust · Revisar perfil\" buttons {\"Cancelar\", \"Continuar\"} default button \"Continuar\" cancel button \"Cancelar\")\nend run",&summary]).kill_on_drop(true).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().await.map_err(|_|"native_dialog_unavailable")?;
            if output.success() {
                Ok(String::new())
            } else {
                Err("cancelled")
            }
        })
    }
    fn ask(&self, step: Step) -> UiFuture<'_> {
        Box::pin(async move {
            script(match step{
        Step::Menu=>"return button returned of (display dialog \"Seiya Rust experimental. Elige un perfil o configura motores ya instalados. No se descargan modelos ni se genera al abrir.\" with title \"Seiya Rust\" buttons {\"Salir\", \"Abrir perfil\", \"Configurar\"} default button \"Abrir perfil\" cancel button \"Salir\")",
        Step::Profile=>"return POSIX path of (choose file with prompt \"Selecciona desktop.json de Seiya (perfil macOS)\")",
        Step::Llm=>"return POSIX path of (choose file with prompt \"Elige el ejecutable Ollama compatible, no la app completa. Usa Ir a carpeta si está dentro de una app.\")",
        Step::Models=>"return POSIX path of (choose folder with prompt \"Elige almacén existente con blobs y manifests-v2 (Gemma e2b-mlx fijado).\")",
        Step::WantImages=>"return button returned of (display dialog \"¿Adjuntar un motor de imágenes ya preparado? No se activará ni generará al guardar.\" with title \"Seiya Rust\" buttons {\"Sin imágenes\", \"Añadir imágenes\"} default button \"Sin imágenes\")",
        Step::ImageBinary=>"return POSIX path of (choose file with prompt \"Elige sd-cli de la build verificada (no cualquier versión)\")",
        Step::ImageAssets=>"return POSIX path of (choose folder with prompt \"Elige los assets de imagen ya verificados del ensayo\")",
        Step::Save=>"return button returned of (display dialog \"Guardar crea carpetas privadas de Seiya (data/coordination) y guarda desktop.json en Library/Application Support/Seiya. Si ya existe, se sustituye solo ese perfil; no se borran modelos/historial. Opciones inicialmente apagadas.\" with title \"Seiya Rust\" buttons {\"Cancelar\", \"Guardar\"} default button \"Guardar\" cancel button \"Cancelar\")",
        Step::Start=>"return button returned of (display dialog \"Perfil comprobado (filesystem, no inferencia). Iniciar abre web solo en este Mac, sin prompts automáticos. Solo chat no adjunta Qwen/imágenes; Con opciones respeta preferencias guardadas y guardas de memoria. Cambia el origen frente a LAN: no migra chats del navegador.\" with title \"Seiya Rust\" buttons {\"Cancelar\", \"Con opciones\", \"Solo chat\"} default button \"Solo chat\" cancel button \"Cancelar\")",
        Step::Preparing=>"return button returned of (display dialog \"Comprobando entorno y preparando servidor local. No se está generando. Puedes cancelar; Seiya esperará el cierre propio.\" with title \"Seiya Rust\" buttons {\"Cancelar\"} default button \"Cancelar\")",
        Step::Running=>"return button returned of (display dialog \"Seiya está en el navegador. Detener revoca nuevos trabajos y espera cancelación/cleanup. La sesión sigue siendo experimental y caduca. Reabrir requiere la cookie del navegador original.\" with title \"Seiya Rust\" buttons {\"Detener\", \"Abrir chat\"} default button \"Detener\" cancel button \"Detener\")",
    }).await
        })
    }
    fn open(&self, url: String) -> UiFuture<'_> {
        Box::pin(async move {
            let mut command = tokio::process::Command::new("/usr/bin/open");
            command
                .arg(url)
                .kill_on_drop(true)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let status = tokio::time::timeout(std::time::Duration::from_secs(10), command.status())
                .await
                .map_err(|_| "browser_open_timeout")?
                .map_err(|_| "browser_open_failed")?;
            if status.success() {
                Ok(String::new())
            } else {
                Err("browser_open_failed")
            }
        })
    }
}
struct NativeHost;
impl Host for NativeHost {
    fn start(&self, profile: Vec<u8>, chat_only: bool) -> HostRun {
        let (tx, ready) = oneshot::channel();
        let (stop, rx) = watch::channel(false);
        let done = tokio::spawn(async move {
            image_web::desktop(
                &profile,
                chat_only,
                image_web::DesktopControl {
                    ready: tx,
                    stop: rx,
                    mount: None,
                    setup_home: None,
                },
            )
            .await
        });
        HostRun { ready, stop, done }
    }
}
pub async fn launch() -> std::result::Result<(), String> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("launcher_home_required")?)
        .canonicalize()
        .map_err(|_| "launcher_home_required")?;
    let (shutdown, rx) = watch::channel(false);
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| "launcher_signal")?;
    let signal = tokio::spawn(async move {
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}};
        shutdown.send_replace(true);
    });
    let result = run_with(&NativeUi, &NativeHost, &home, rx).await;
    signal.abort();
    let _ = signal.await;
    match result {
        Ok(()) | Err("cancelled") => Ok(()),
        Err(e) => {
            // Error data is passed as argv, never interpolated into AppleScript.
            let _=tokio::process::Command::new("/usr/bin/osascript").args(["-e","on run argv\n display alert \"Seiya Rust\" message (item 1 of argv) as warning\nend run",e]).kill_on_drop(true).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().await;
            Err(e.into())
        }
    }
}

#[cfg(test)]
mod tests;

mod web_adapter;
pub(crate) use web_adapter::WebAdapter;
