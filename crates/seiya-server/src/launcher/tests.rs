use super::*;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

struct Fixture {
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("seiya-launcher-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(home.join("Library/Application Support"))
            .unwrap();
        fs::create_dir(home.join("models")).unwrap();
        fs::write(home.join("engine"), b"synthetic, never executed").unwrap();
        fs::set_permissions(home.join("engine"), fs::Permissions::from_mode(0o700)).unwrap();
        Self { home }
    }
    fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&json!({"version":1,"target":"macos","data_dir":root(&self.home).join("data"),"coordination_dir":root(&self.home).join("coordination"),"llm_binary":self.home.join("engine"),"model_store":self.home.join("models"),"features":{"images":false,"multiagent":false}})).unwrap()
    }
    fn profile(&self) -> PathBuf {
        let _lock = prepare(&self.home).unwrap();
        save(&self.home, None, &self.bytes()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.home).unwrap();
    }
}
struct FakeUi {
    answers: Mutex<VecDeque<(Step, String)>>,
    opened: Mutex<Vec<String>>,
    fail_open: bool,
}
impl FakeUi {
    fn new(answers: Vec<(Step, String)>) -> Self {
        Self {
            answers: Mutex::new(answers.into()),
            opened: Mutex::new(Vec::new()),
            fail_open: false,
        }
    }
}
impl Ui for FakeUi {
    fn review(&self, summary: String) -> UiFuture<'_> {
        assert!(summary.contains("Motor LLM:"));
        Box::pin(async { Ok(String::new()) })
    }
    fn ask(&self, step: Step) -> UiFuture<'_> {
        if step == Step::Preparing {
            return Box::pin(std::future::pending());
        }
        let answer = self.answers.lock().unwrap().pop_front();
        Box::pin(async move {
            let (expected, answer) = answer.ok_or("cancelled")?;
            assert_eq!(step, expected);
            Ok(answer)
        })
    }
    fn open(&self, url: String) -> UiFuture<'_> {
        self.opened.lock().unwrap().push(url);
        Box::pin(async move {
            if self.fail_open {
                Err("open_failed")
            } else {
                Ok(String::new())
            }
        })
    }
}
struct FakeHost {
    active: Arc<AtomicBool>,
    stops: Arc<AtomicUsize>,
    chat_only: Mutex<Vec<bool>>,
    bad_url: bool,
    cleanup_fail: bool,
}
impl FakeHost {
    fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            stops: Arc::new(AtomicUsize::new(0)),
            chat_only: Mutex::new(Vec::new()),
            bad_url: false,
            cleanup_fail: false,
        }
    }
}
impl Host for FakeHost {
    fn start(&self, _: Vec<u8>, solo: bool) -> HostRun {
        self.chat_only.lock().unwrap().push(solo);
        self.active.store(true, Ordering::SeqCst);
        let (tx, ready) = oneshot::channel();
        let (stop, mut rx) = watch::channel(false);
        let bootstrap = if self.bad_url {
            "https://remote.invalid/session/abc".into()
        } else {
            format!("http://127.0.0.1:8015/session/{}", "a".repeat(64))
        };
        let _ = tx.send(Ready {
            bootstrap,
            home: format!("http://127.0.0.1:8015/trial/{}/", "b".repeat(32)),
        });
        let active = self.active.clone();
        let stops = self.stops.clone();
        let fail = self.cleanup_fail;
        let done = tokio::spawn(async move {
            if !*rx.borrow() {
                let _ = rx.changed().await;
            }
            active.store(false, Ordering::SeqCst);
            stops.fetch_add(1, Ordering::SeqCst);
            if fail {
                Err("synthetic uncertain".into())
            } else {
                Ok(())
            }
        });
        HostRun { ready, stop, done }
    }
}
fn choices(path: &Path) -> Vec<(Step, String)> {
    vec![
        (Step::Menu, "Abrir perfil".into()),
        (Step::Profile, path.to_str().unwrap().into()),
        (Step::Start, "Solo chat".into()),
        (Step::Running, "Detener".into()),
    ]
}

#[test]
fn browser_targets_are_exact_local_credentials_free_and_bounded() {
    assert!(valid_url(
        &format!("http://127.0.0.1:8015/session/{}", "a".repeat(64)),
        true
    ));
    for url in [
        "https://remote.invalid/session/a",
        "http://localhost:8015/session/a",
        "http://127.0.0.1:9999/session/a",
        "http://u:p@127.0.0.1:8015/session/a",
        "http://127.0.0.1:8015/session/a?leak=1",
        "http://127.0.0.1:8015/session/a#x",
        "file:///etc/passwd",
    ] {
        assert!(!valid_url(url, true), "{url}");
    }
}
#[test]
fn save_is_private_atomic_and_never_overwrites_a_changed_profile() {
    let f = Fixture::new();
    let _lock = prepare(&f.home).unwrap();
    let bytes = f.bytes();
    let path = save(&f.home, None, &bytes).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(save(&f.home, None, &bytes).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(save(&f.home, Some(bytes.clone()), &bytes).is_ok());
    assert!(profile_lock(&root(&f.home)).is_err());
}
#[test]
fn preparation_rejects_symlinks_without_changing_their_targets() {
    let f = Fixture::new();
    fs::create_dir(root(&f.home)).unwrap();
    fs::set_permissions(root(&f.home), fs::Permissions::from_mode(0o700)).unwrap();
    let outside = f.home.join("models");
    std::os::unix::fs::symlink(&outside, root(&f.home).join("data")).unwrap();
    assert!(prepare(&f.home).is_err());
    assert!(fs::read_dir(outside).unwrap().next().is_none());
}
#[tokio::test]
async fn cancelled_menu_never_creates_files_starts_host_or_opens_browser() {
    let f = Fixture::new();
    let ui = FakeUi::new(vec![(Step::Menu, "Salir".into())]);
    let host = FakeHost::new();
    let (_tx, rx) = watch::channel(false);
    run_with(&ui, &host, &f.home, rx).await.unwrap();
    assert!(!root(&f.home).exists());
    assert!(host.chat_only.lock().unwrap().is_empty());
    assert!(ui.opened.lock().unwrap().is_empty());
}
#[tokio::test]
async fn guided_save_requires_confirmation_and_keeps_features_off() {
    let f = Fixture::new();
    let ui = FakeUi::new(vec![
        (Step::Llm, f.home.join("engine").to_str().unwrap().into()),
        (Step::Models, f.home.join("models").to_str().unwrap().into()),
        (Step::WantImages, "Sin imágenes".into()),
        (Step::Save, "Guardar".into()),
    ]);
    let (_tx, mut rx) = watch::channel(false);
    let path = configure(&ui, &f.home, &mut rx).await.unwrap();
    let c = desktop_config::load_file(&path).unwrap();
    assert!(!c.features.images && !c.features.multiagent);
    assert!(c.images.is_none());
}
#[tokio::test]
async fn ready_precedes_browser_and_stop_awaits_cleanup() {
    let f = Fixture::new();
    let path = f.profile();
    let ui = FakeUi::new(choices(&path));
    let host = FakeHost::new();
    let (_tx, rx) = watch::channel(false);
    run_with(&ui, &host, &f.home, rx).await.unwrap();
    assert!(!host.active.load(Ordering::SeqCst));
    assert_eq!(host.stops.load(Ordering::SeqCst), 1);
    assert_eq!(host.chat_only.lock().unwrap().as_slice(), &[true]);
    assert_eq!(ui.opened.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn browser_failure_stops_host_without_retrying_bootstrap() {
    let f = Fixture::new();
    let path = f.profile();
    let mut ui = FakeUi::new(choices(&path));
    ui.fail_open = true;
    let host = FakeHost::new();
    let (_tx, rx) = watch::channel(false);
    assert!(run_with(&ui, &host, &f.home, rx).await.is_err());
    assert!(!host.active.load(Ordering::SeqCst));
    assert_eq!(host.stops.load(Ordering::SeqCst), 1);
    assert_eq!(ui.opened.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn invalid_ready_url_never_opens_a_browser_and_uncertain_cleanup_is_not_success() {
    let f = Fixture::new();
    let path = f.profile();
    let ui = FakeUi::new(choices(&path));
    let mut host = FakeHost::new();
    host.bad_url = true;
    host.cleanup_fail = true;
    let (_tx, rx) = watch::channel(false);
    assert!(run_with(&ui, &host, &f.home, rx).await.is_err());
    assert!(ui.opened.lock().unwrap().is_empty());
    assert!(!host.active.load(Ordering::SeqCst));
}
#[tokio::test]
async fn reopen_uses_authenticated_home_without_replaying_bootstrap() {
    let f = Fixture::new();
    let path = f.profile();
    let mut answers = choices(&path);
    answers.pop();
    answers.extend([
        (Step::Running, "Abrir chat".into()),
        (Step::Running, "Detener".into()),
    ]);
    let ui = FakeUi::new(answers);
    let host = FakeHost::new();
    let (_tx, rx) = watch::channel(false);
    run_with(&ui, &host, &f.home, rx).await.unwrap();
    let opened = ui.opened.lock().unwrap();
    assert_eq!(opened.len(), 2);
    assert!(opened[0].contains("/session/"));
    assert!(opened[1].contains("/trial/"));
}
#[tokio::test]
async fn shutdown_before_menu_never_starts_or_writes() {
    let f = Fixture::new();
    let ui = FakeUi::new(vec![]);
    let host = FakeHost::new();
    let (_tx, rx) = watch::channel(true);
    assert_eq!(run_with(&ui, &host, &f.home, rx).await, Err("cancelled"));
    assert!(!root(&f.home).exists());
    assert!(host.chat_only.lock().unwrap().is_empty());
}
