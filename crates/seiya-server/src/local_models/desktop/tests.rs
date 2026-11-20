use super::*;
#[tokio::test]
async fn inventory_uses_registered_binding_and_only_reads_models() {
    use axum::{routing::get, Json, Router};
    let fixture = Fixture::new();
    let adapter = fixture.adapter();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut catalog, _) = adapter.catalog().unwrap();
    catalog.models[0].runtime.base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let runtime = catalog.models[0].runtime.clone();
    let binding = runtime.model.clone();
    catalog.image_models.push(crate::local_models::ImageModel {
        id: "art".into(),
        runtime: crate::runtime::RuntimeConfig {
            id: "art".into(),
            model: "absent-image-binding".into(),
            ..runtime
        },
    });
    std::fs::write(&adapter.catalog_path, serde_json::to_vec(&catalog).unwrap()).unwrap();
    let app = Router::new().route(
        "/v1/models",
        get(move || async move { Json(serde_json::json!({"data":[{"id":binding}]})) }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    assert_eq!(
        adapter.inventory("brain-small".into()).await.unwrap(),
        "advertised"
    );
    assert_eq!(adapter.inventory("art".into()).await.unwrap(), "absent");
    assert_eq!(
        adapter.inventory("unregistered".into()).await.unwrap_err(),
        "unknown_catalog_id"
    );
    server.abort();
    let _ = server.await;
    // Reserva un puerto sin listen: otro test no puede reutilizarlo durante la comprobación.
    let offline = tokio::net::TcpSocket::new_v4().unwrap();
    offline.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    catalog.models[0].runtime.base_url = format!("http://{}/v1", offline.local_addr().unwrap());
    std::fs::write(&adapter.catalog_path, serde_json::to_vec(&catalog).unwrap()).unwrap();
    assert_eq!(
        adapter.inventory("brain-small".into()).await.unwrap(),
        "unreachable"
    );
}
#[test]
fn image_role_is_interchangeable_and_cas_preserves_models_and_profiles() {
    let f = Fixture::new();
    let adapter = f.adapter();
    let (mut c, _) = adapter.catalog().unwrap();
    for id in ["image-a", "image-b"] {
        let mut runtime = c.models[0].runtime.clone();
        runtime.id = id.into();
        runtime.model = format!("binding-{id}");
        runtime.base_url = format!(
            "http://127.0.0.1:{}/v1",
            if id == "image-a" { 21004 } else { 21005 }
        );
        c.image_models.push(crate::local_models::ImageModel {
            id: id.into(),
            runtime,
        });
    }
    let original = serde_json::to_vec(&c).unwrap();
    std::fs::write(&adapter.catalog_path, &original).unwrap();
    for id in ["image-a", "image-b", ""] {
        let snap = adapter.snapshot().unwrap();
        let mut draft = snap.draft;
        draft.roles.image = id.into();
        assert!(adapter.supports(&draft));
        let approved = adapter.review(draft.clone(), snap.baseline).unwrap();
        adapter.save(&approved).unwrap();
        assert_eq!(adapter.snapshot().unwrap().draft, draft);
        assert_eq!(
            storage::read(&adapter.catalog_path).unwrap().unwrap(),
            original
        );
        let reviewed = Catalog::parse(&approved.profile).unwrap();
        assert_eq!(reviewed.image.as_deref(), (!id.is_empty()).then_some(id));
    }
    assert!(!f.0.join("desktop.json").exists());
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("seiya-local-desktop-{}", uuid::Uuid::new_v4()));
        storage::ensure_directory(&p).unwrap();
        Self(p)
    }
    fn adapter(&self) -> Desktop {
        let path = self.0.join("local-models.json");
        storage::new_file(
            &path,
            include_bytes!("../../../../../contracts/local-models/v1/catalog.example.json"),
        )
        .unwrap();
        Desktop::new(self.0.clone(), path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn assignments_swap_and_persist_without_changing_catalogue_or_native_profiles() {
    let f = Fixture::new();
    let adapter = f.adapter();
    let original = storage::read(&adapter.catalog_path).unwrap();
    let snap = adapter.snapshot().unwrap();
    assert_eq!(snap.baseline.len(), 32);
    assert!(snap.warning.as_ref().unwrap().contains("Ollama"));
    assert!(adapter.available());
    assert!(adapter.supports(&snap.draft));
    let mut draft = snap.draft;
    draft.roles.brain = "code-large".into();
    let approved = adapter.review(draft.clone(), snap.baseline).unwrap();
    assert!(storage::read(&adapter.roles_path()).unwrap().is_none());
    adapter.save(&approved).unwrap();
    assert_eq!(adapter.snapshot().unwrap().draft, draft);
    assert_eq!(storage::read(&adapter.catalog_path).unwrap(), original);
    assert!(!f.0.join("desktop.json").exists());
    assert!(!f.0.join("desktop-manager.json").exists());
    assert_eq!(
        adapter.save(&approved).err().unwrap(),
        "profile_changed_no_overwrite"
    );
}
#[test]
fn missing_catalogue_is_inert_and_paths_images_unknown_models_and_stale_approvals_rejected() {
    let f = Fixture::new();
    let missing = Desktop::new(f.0.clone(), f.0.join("absent.json"));
    assert!(missing.available());
    assert!(!missing.supports(&missing.snapshot().unwrap().draft));
    assert!(missing.snapshot().unwrap().warning.is_some());
    let adapter = f.adapter();
    let snap = adapter.snapshot().unwrap();
    for case in 0..3 {
        let mut draft = snap.draft.clone();
        match case {
            0 => draft.roles.brain = "unknown".into(),
            1 => draft.image_binary = "/not-executed".into(),
            _ => draft.roles.image = "image-not-supported".into(),
        }
        assert!(!adapter.supports(&draft));
        assert!(adapter.review(draft, snap.baseline.clone()).is_err());
    }
    let approved = adapter.review(snap.draft, snap.baseline).unwrap();
    let mut bytes = storage::read(&adapter.catalog_path).unwrap().unwrap();
    bytes.push(b' ');
    std::fs::write(&adapter.catalog_path, bytes).unwrap();
    assert_eq!(
        adapter.save(&approved).err().unwrap(),
        "profile_changed_no_overwrite"
    );
}

#[test]
fn ollama_model_can_be_saved_without_catalogue_but_only_as_owned_runtime_profile() {
    let f = Fixture::new();
    let adapter = Desktop::new(f.0.clone(), f.0.join("local-models.json"));
    super::super::manager::remember_installed_for_test("gemma4:12b");
    let snap = adapter.snapshot().unwrap();
    let mut draft = snap.draft;
    draft.roles.brain = "gemma4:12b".into();
    assert!(adapter.supports(&draft));
    let approved = adapter.review(draft.clone(), snap.baseline).unwrap();
    let catalog = Catalog::parse(&approved.profile).unwrap();
    assert_eq!(
        super::super::manager::managed_model(&catalog),
        Some("gemma4:12b")
    );
    assert_eq!(
        catalog.models[0].runtime.base_url,
        "http://127.0.0.1:49401/v1"
    );
    adapter.save(&approved).unwrap();
    let saved = adapter.snapshot().unwrap();
    assert_eq!(saved.draft.roles.brain, "gemma4:12b");
    assert!(saved.warning.unwrap().contains("instancia privada"));
    assert!(!adapter.catalog_path.exists());
    assert!(adapter.supports(&saved.draft));
}
#[test]
fn ollama_override_is_saved_without_catalogue_but_does_not_enable_start() {
    let f = Fixture::new();
    let adapter = Desktop::new(f.0.clone(), f.0.join("absent.json"));
    let snap = adapter.snapshot().unwrap();
    let mut draft = snap.draft;
    draft.llm_binary = "/synthetic/missing/ollama".into();
    draft.model_store = "/synthetic/not-explored".into();
    let approved = adapter.review(draft.clone(), snap.baseline).unwrap();
    assert!(approved.profile.is_empty());
    assert!(!adapter.supports(&draft));
    adapter.save(&approved).unwrap();
    let loaded = adapter.snapshot().unwrap();
    assert_eq!(loaded.draft, draft);
    assert!(loaded.warning.unwrap().contains("inválida"));
    assert!(adapter.available());
    assert!(!adapter.supports(&loaded.draft));
    assert!(!adapter.catalog_path.exists());
}
#[test]
fn old_saved_assignments_remain_readable_and_bad_override_does_not_block_http_backend() {
    let f = Fixture::new();
    let adapter = f.adapter();
    let snap = adapter.snapshot().unwrap();
    storage::new_file(&adapter.roles_path(),&serde_json::to_vec(&serde_json::json!({"version":1,"assignments":{"brain":snap.draft.roles.brain,"workers":snap.draft.roles.workers}})).unwrap()).unwrap();
    let snap = adapter.snapshot().unwrap();
    assert!(adapter.supports(&snap.draft));
    let mut draft = snap.draft;
    draft.llm_binary = "/missing/override".into();
    let approved = adapter.review(draft.clone(), snap.baseline).unwrap();
    adapter.save(&approved).unwrap();
    assert_eq!(adapter.snapshot().unwrap().draft, draft);
    assert!(adapter.supports(&draft));
}
