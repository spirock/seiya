use super::*;
#[tokio::test(start_paused = true)]
async fn desktop_remains_running_after_thirty_minutes_until_explicit_shutdown() {
    let (tx, mut rx) = watch::channel(false);
    let mut server = tokio::spawn(std::future::pending::<Result<(), std::io::Error>>());
    let waiter = tokio::spawn(async move {
        let ended = wait_for_desktop_shutdown(&mut rx, &mut server).await;
        server.abort();
        ended
    });
    tokio::time::advance(Duration::from_secs(3601)).await;
    assert!(!waiter.is_finished());
    tx.send_replace(true);
    assert!(!waiter.await.unwrap());
}
use axum::{
    body::Body,
    http::{header, Request},
};
use http_body_util::BodyExt;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(not(debug_assertions))]
#[tokio::test]
async fn release_does_not_register_direct_test_entry_or_start_test_chat() {
    let address = "127.0.0.1:8016".parse().unwrap();
    let (tx, _) = watch::channel(false);
    let (app, _, _) = application_at_with_key(
        Setup::new(Arc::new(Unsupported), tx),
        address,
        Some("a".repeat(64)),
    );
    let response = app
        .oneshot(
            Request::get("/app/")
                .header(header::HOST, "localhost:8016")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
    assert_eq!(
        launch_test_chat().await.unwrap_err(),
        "development_test_chat_only"
    );
}

#[tokio::test]
async fn development_test_entry_creates_cookie_without_bootstrap_and_survives_restart() {
    let address = "127.0.0.1:8016".parse().unwrap();
    let key = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let make = || {
        let (tx, _) = watch::channel(false);
        application_at_with_key(
            Setup::new(Arc::new(Unsupported), tx),
            address,
            Some(key.clone()),
        )
    };
    let (app, _, _) = make();
    let request = |router: &Router, path: &str, cookie: &str, host: &str, origin: Option<&str>| {
        let mut r = Request::builder().uri(path).header(header::HOST, host);
        if !cookie.is_empty() {
            r = r.header(header::COOKIE, cookie);
        }
        if let Some(o) = origin {
            r = r.header(header::ORIGIN, o);
        }
        let router = router.clone();
        async move {
            router
                .oneshot(r.body(Body::empty()).unwrap())
                .await
                .unwrap()
        }
    };
    let response = request(&app, "/app/", "", "localhost:8016", None).await;
    assert_eq!(response.status(), 303);
    let first_home = response.headers()[header::LOCATION].to_str().unwrap();
    assert!(first_home.starts_with("/app/") && first_home != "/app/");
    let raw = response.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(
        raw.contains("seiya_test_access=")
            && raw.contains("Path=/app/")
            && raw.contains("HttpOnly")
    );
    assert!(raw.contains("Max-Age=315360000") && !raw.contains("seiya_access="));
    let cookie = raw.split(';').next().unwrap();
    let restarted = make().0;
    for router in [&app, &restarted] {
        let response = request(router, "/app/", "", "localhost:8016", None).await;
        assert_eq!(response.status(), 303);
        assert!(response.headers().contains_key(header::SET_COOKIE));
        let path = response.headers()[header::LOCATION].to_str().unwrap();
        assert!(path.starts_with("/app/"));
        let api = format!("{}api/status", path);
        assert_eq!(
            request(router, &api, "", "localhost:8016", None)
                .await
                .status(),
            401
        );
        assert_eq!(
            request(router, &api, cookie, "localhost:8016", None)
                .await
                .status(),
            200
        );
        let page = request(router, path, cookie, "localhost:8016", None).await;
        assert_eq!(page.status(), 200);
        let html = String::from_utf8(
            page.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(
            html.contains("seiyaInitialSettingsMenu")
                && !html.contains("VISTA PREVIA · Puerto8016")
        );
        assert_eq!(
            request(router, &api, "seiya_access=invalid", "localhost:8016", None)
                .await
                .status(),
            401
        );
        assert_eq!(
            request(router, &api, cookie, "127.0.0.1:8015", None)
                .await
                .status(),
            403
        );
        assert_eq!(
            request(router, "/app/", cookie, "127.0.0.1:8015", None)
                .await
                .status(),
            403
        );
        assert_eq!(
            request(
                router,
                "/app/",
                "",
                "localhost:8016",
                Some("http://127.0.0.1:8016")
            )
            .await
            .status(),
            403
        );
        assert_eq!(
            request(router, "/app/", "", "127.0.0.1:8016", None)
                .await
                .status(),
            303
        );
    }
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn development_enter_preserves_browser_origin_without_consuming_link_on_rejection() {
    let (tx, _) = watch::channel(false);
    let state = Setup::new(Arc::new(Unsupported), tx);
    let (app, _, home) = application_at_with_key(
        state.clone(),
        "127.0.0.1:8016".parse().unwrap(),
        Some("a".repeat(64)),
    );
    let first = format!("http://127.0.0.1:8016/session/{}", "b".repeat(64));
    {
        let mut c = state.control.lock().await;
        c.phase = "running";
        c.bootstrap = Some(first.clone());
        c.chat = Some(format!("http://127.0.0.1:8016/app/{}/", "c".repeat(32)));
    }
    let path = reqwest::Url::parse(&home).unwrap().path().to_owned();
    let entry = app
        .clone()
        .oneshot(
            Request::get("/app/")
                .header(header::HOST, "localhost:8016")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cookie = entry.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let enter = format!("{path}api/enter");
    let send = |host: &str, origin: &str| {
        Request::post(&enter)
            .header(header::HOST, host)
            .header(header::ORIGIN, origin)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(send("localhost:8016", "http://127.0.0.1:8016"))
            .await
            .unwrap()
            .status(),
        403
    );
    assert!(state.control.lock().await.bootstrap.is_some());
    let response = app
        .clone()
        .oneshot(send("localhost:8016", "http://localhost:8016"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        json_body(response).await["url"],
        first.replace("127.0.0.1", "localhost")
    );
    assert!(state.control.lock().await.bootstrap.is_some());
    assert_eq!(
        json_body(
            app.clone()
                .oneshot(send("localhost:8016", "http://localhost:8016"))
                .await
                .unwrap()
        )
        .await["url"],
        first.replace("127.0.0.1", "localhost")
    );
    assert_eq!(
        json_body(
            app.oneshot(send("127.0.0.1:8016", "http://127.0.0.1:8016"))
                .await
                .unwrap()
        )
        .await["url"],
        first
    );
}

#[tokio::test]
async fn normal_desktop_enter_consumes_first_link() {
    let (tx, _) = watch::channel(false);
    let state = Setup::new(Arc::new(Unsupported), tx);
    let _ = application(state.clone());
    let first = format!("http://127.0.0.1:8015/session/{}", "b".repeat(64));
    let home = format!("http://127.0.0.1:8015/app/{}/", "c".repeat(32));
    {
        let mut c = state.control.lock().await;
        c.phase = "running";
        c.bootstrap = Some(first.clone());
        c.chat = Some(home.clone());
    }
    for expected in [&first, &home] {
        let r = Request::post("/api/enter")
            .header(header::HOST, "127.0.0.1:8015")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            json_body(enter(State(state.clone()), r).await).await["url"],
            *expected
        );
    }
}

#[tokio::test]
async fn initial_shell_contains_settings_navigation_not_a_fixed_popup() {
    let Html(html) = page().await;
    assert!(!html.contains("position:fixed"));
    assert!(html.contains("seiyaInitialSettingsMenu"));
    assert!(html.contains("Configuración de modelos"));
    assert!(html.contains("Configuración general"));
    assert!(html.contains("function seiyaSetupSaved()"));
    assert!(html.contains("seiyaInitialPhase === 'running'"));
    assert!(html.contains("seiyaAutoEnterAttempted = true"));
    assert!(html.contains("el('setupEnter').click()"));
    assert!(html.contains("el('prompt').disabled = true"));
    assert!(!html.contains("<iframe"));
}

#[tokio::test]
async fn platforms_without_adapter_have_inert_common_web_and_cannot_start() {
    let (tx, _) = watch::channel(false);
    let state = Setup::new(Arc::new(Unsupported), tx);
    let Json(v) = status(State(state.clone())).await;
    assert_eq!(v["runtime_available"], false);
    assert!(state.adapter.snapshot().unwrap().warning.is_some());
    let r = review(
        State(state.clone()),
        Json(Review {
            draft: Draft::default(),
            baseline: vec![],
        }),
    )
    .await;
    assert_eq!(r.status(), 409);
    assert!(state.control.lock().await.stop.is_none());
}

struct Fake {
    saves: AtomicUsize,
    starts: AtomicUsize,
    stored: bool,
    invalid: bool,
}
impl Adapter for Fake {
    fn available(&self) -> bool {
        true
    }
    fn snapshot(&self) -> Result<Snapshot, String> {
        Ok(Snapshot {
            draft: Draft::default(),
            baseline: vec![],
            warning: None,
        })
    }
    fn saved_host(&self) -> Result<Option<(Vec<u8>, bool)>, String> {
        if self.invalid {
            Err("saved_host_profile_changed".into())
        } else if self.stored {
            Ok(Some((vec![], true)))
        } else {
            Ok(None)
        }
    }
    fn review(&self, draft: Draft, baseline: Vec<u8>) -> Result<Approval, String> {
        Ok(Approval {
            draft,
            baseline,
            profile: vec![],
        })
    }
    fn save(&self, _: &Approval) -> Result<(), String> {
        self.saves.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn pick(&self, _: String) -> Work<'_, String> {
        Box::pin(async { Ok("/synthetic/engine".into()) })
    }
    fn start(
        &self,
        _: Vec<u8>,
        _: bool,
        mut stop: watch::Receiver<bool>,
        _: String,
    ) -> Work<'_, Engine> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let done = tokio::spawn(async move {
                if !*stop.borrow() {
                    let _ = stop.changed().await;
                }
                Ok(())
            });
            Ok(Engine {
                vision_provider: None,
                residency: None,
                router: Router::new()
                    .route("/session/fake", get(|| async { "runtime-auth-boundary" })),
                bootstrap: "http://127.0.0.1:8015/session/fake".into(),
                home: "http://127.0.0.1:8015/runtime/".into(),
                done,
            })
        })
    }
}
async fn call(
    app: &Router,
    path: &str,
    cookie: &str,
    origin: &str,
    body: Option<Value>,
) -> Response {
    let mut r = Request::builder()
        .uri(path)
        .header(header::HOST, "127.0.0.1:8015");
    if !cookie.is_empty() {
        r = r.header(header::COOKIE, cookie);
    }
    if !origin.is_empty() {
        r = r.header(header::ORIGIN, origin);
    }
    let bytes = if let Some(v) = body {
        r = r
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json");
        serde_json::to_vec(&v).unwrap()
    } else {
        vec![]
    };
    app.clone()
        .oneshot(r.body(Body::from(bytes)).unwrap())
        .await
        .unwrap()
}
async fn json_body(r: Response) -> Value {
    serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
#[tokio::test]
async fn portable_setup_without_catalogue_can_review_and_save_paths_but_never_start() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("seiya-path-web-{}", uuid::Uuid::new_v4()));
    crate::local_models::storage::ensure_directory(&root).unwrap();
    let adapter = Arc::new(crate::local_models::desktop::Desktop::new(
        root.clone(),
        root.join("missing.json"),
    ));
    let (tx, _) = watch::channel(false);
    let (app, link, _) = application(Setup::new(adapter, tx));
    let response = call(
        &app,
        reqwest::Url::parse(&link).unwrap().path(),
        "",
        "",
        None,
    )
    .await;
    let cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let prefix = response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_owned();
    let status =
        json_body(call(&app, &format!("{prefix}api/status"), &cookie, "", None).await).await;
    assert_eq!(status["runtime_available"], true); // Existing UI enables review.
    let detected =
        json_body(call(&app, &format!("{prefix}api/detect"), &cookie, "", None).await).await;
    assert!(detected["warning"].as_str().unwrap().contains("Ollama"));
    let mut draft = detected["draft"].clone();
    draft["llm_binary"] = json!("/synthetic/invalid/ollama");
    let review = json_body(
        call(
            &app,
            &format!("{prefix}api/review"),
            &cookie,
            "http://127.0.0.1:8015",
            Some(json!({"draft":draft,"baseline":detected["baseline"]})),
        )
        .await,
    )
    .await;
    assert_eq!(review["executable"], false);
    let saved = call(
        &app,
        &format!("{prefix}api/confirm"),
        &cookie,
        "http://127.0.0.1:8015",
        Some(json!({"approval":review["approval"],"start":false})),
    )
    .await;
    assert_eq!(saved.status(), 200);
    let current =
        json_body(call(&app, &format!("{prefix}api/detect"), &cookie, "", None).await).await;
    assert_eq!(current["draft"], draft);
    assert!(current["warning"].as_str().unwrap().contains("inválida"));
    let review = json_body(
        call(
            &app,
            &format!("{prefix}api/review"),
            &cookie,
            "http://127.0.0.1:8015",
            Some(json!({"draft":draft,"baseline":current["baseline"]})),
        )
        .await,
    )
    .await;
    let rejected = call(
        &app,
        &format!("{prefix}api/confirm"),
        &cookie,
        "http://127.0.0.1:8015",
        Some(json!({"approval":review["approval"],"start":true})),
    )
    .await;
    assert!(!rejected.status().is_success());
    assert!(!root.join("missing.json").exists());
    std::fs::remove_dir_all(root).unwrap();
}
async fn fixture() -> (Router, String, String, Arc<Fake>, Arc<Setup>) {
    let fake = Arc::new(Fake {
        saves: AtomicUsize::new(0),
        starts: AtomicUsize::new(0),
        stored: false,
        invalid: false,
    });
    let (tx, _) = watch::channel(false);
    let state = Setup::new(fake.clone(), tx);
    let (app, link, _) = application(state.clone());
    let path = reqwest::Url::parse(&link).unwrap().path().to_owned();
    let r = call(&app, &path, "", "", None).await;
    assert_eq!(r.status(), 303);
    let cookie = r.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let prefix = r.headers()[header::LOCATION].to_str().unwrap().to_owned();
    assert_eq!(call(&app, &path, "", "", None).await.status(), 403);
    (app, prefix, cookie, fake, state)
}
#[cfg(target_os = "linux")]
#[tokio::test]
async fn calculator_requires_running_visual_brain_even_for_authenticated_requests() {
    let (app, prefix, cookie, _, state) = fixture().await;
    for (phase, vision) in [("running", false), ("stopping", true)] {
        {
            let mut c = state.control.lock().await;
            c.phase = phase;
            c.vision_provider = vision.then(|| VisualProvider {
                role: "visual-worker".into(),
                model: "gemma".into(),
                port: 21011,
            });
        }
        let response = call(
            &app,
            &format!("{prefix}api/assist/sum"),
            &cookie,
            "http://127.0.0.1:8015",
            Some(json!({"a":27,"b":18})),
        )
        .await;
        assert_eq!(
            json_body(response).await["error"],
            "assist_requires_vision_provider"
        );
    }
}

#[tokio::test]
async fn saved_host_starts_once_without_saving_or_dispatching_a_model() {
    for (stored, invalid, expected) in [
        (false, false, "unconfigured"),
        (true, true, "blocked"),
        (true, false, "running"),
    ] {
        let fake = Arc::new(Fake {
            saves: AtomicUsize::new(0),
            starts: AtomicUsize::new(0),
            stored,
            invalid,
        });
        let (tx, _) = watch::channel(false);
        let state = Setup::new(fake.clone(), tx);
        let (app, _, _) = application(state.clone());
        restore_saved_host(&state).await;
        if stored && !invalid {
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if state.control.lock().await.phase == "running" {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
        let status = status(State(state.clone())).await.0;
        assert_eq!(status["phase"], expected);
        assert_eq!(status["restored_host"], stored && !invalid);
        assert_eq!(fake.saves.load(Ordering::SeqCst), 0);
        assert_eq!(
            fake.starts.load(Ordering::SeqCst),
            usize::from(stored && !invalid)
        );
        if invalid {
            assert_eq!(status["error"], "saved_host_profile_changed");
        }
        if stored && !invalid {
            let _ = stop(State(state.clone())).await;
            tokio::time::timeout(Duration::from_secs(2), async {
                while state.control.lock().await.stop.is_some() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
        drop(app);
    }
}
#[tokio::test]
async fn inert_shell_preserves_styles_and_requires_auth_before_side_effects() {
    let (app, prefix, cookie, fake, _) = fixture().await;
    assert_eq!(call(&app, &prefix, "", "", None).await.status(), 401);
    for (path, body) in [
        ("api/models", None),
        ("api/models/pull", Some(json!({"name":"gemma4:12b"}))),
        ("api/models/delete", Some(json!({"name":"gemma4:12b"}))),
        ("api/models/cancel", Some(json!({}))),
    ] {
        assert_eq!(
            call(&app, &format!("{prefix}{path}"), "", "", body)
                .await
                .status(),
            401,
            "{path} must require the setup session"
        );
    }
    assert_eq!(
        call(&app, &prefix, &cookie, "https://foreign.invalid", None)
            .await
            .status(),
        403
    );
    let r = call(&app, &prefix, &cookie, "", None).await;
    assert_eq!(r.status(), 200);
    let html =
        String::from_utf8(r.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    let css = |s: &str| {
        s.split("<style>")
            .nth(1)
            .unwrap()
            .split("</style>")
            .next()
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        css(&html),
        css(include_str!("../../../../swarm-app/seiya/ui.html"))
    );
    assert!(html.contains("Gestor de modelos por roles"));
    assert!(html.contains("setupReviewFeedback"));
    assert!(html.contains(r"app\/[a-f0-9]{32}"));
    assert!(html.contains("Desactivar imágenes en este borrador"));
    assert!(!html.contains("async function send"));
    assert_eq!(fake.starts.load(Ordering::SeqCst), 0);
    assert_eq!(fake.saves.load(Ordering::SeqCst), 0);
    let body = json!({"approval":"forged","start":true});
    assert_eq!(
        call(
            &app,
            &format!("{prefix}api/confirm"),
            "",
            "",
            Some(body.clone())
        )
        .await
        .status(),
        401
    );
    assert_eq!(
        call(
            &app,
            &format!("{prefix}api/confirm"),
            &cookie,
            "https://foreign.invalid",
            Some(body)
        )
        .await
        .status(),
        403
    );
    assert_eq!(fake.saves.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn update_check_requires_auth_origin_and_explicit_consent() {
    let (app, prefix, cookie, _, _) = fixture().await;
    let path = format!("{prefix}api/update-check");
    assert_eq!(
        call(&app, &path, "", "", Some(json!({"consent":true})))
            .await
            .status(),
        401
    );
    assert_eq!(
        call(
            &app,
            &path,
            &cookie,
            "https://foreign.invalid",
            Some(json!({"consent":true}))
        )
        .await
        .status(),
        403
    );
    assert_eq!(
        call(&app, &path, &cookie, "", Some(json!({"consent":false})))
            .await
            .status(),
        403
    );
    assert_eq!(
        call(
            &app,
            &path,
            &cookie,
            "",
            Some(json!({"consent":false,"url":"https://evil.invalid"}))
        )
        .await
        .status(),
        422
    );
}

#[tokio::test]
async fn shell_and_mounted_runtime_keep_independent_bootstrap_and_cookie_boundaries() {
    let (app, prefix, cookie, _, state) = fixture().await;
    let (runtime, link) = crate::pair_trial::access::app(
        "127.0.0.1:8015".parse().unwrap(),
        crate::prepared_web::PreparedWeb::diagnostics_only().unwrap(),
    )
    .unwrap();
    *state.mounted.write().unwrap() = runtime;
    let path = reqwest::Url::parse(&link).unwrap().path().to_owned();
    let response = call(&app, &path, "", "", None).await;
    assert_eq!(response.status(), 303);
    let runtime_cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let runtime_home = response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        call(&app, &format!("{runtime_home}health"), &cookie, "", None)
            .await
            .status(),
        401
    );
    // The existing runtime deliberately serves inert static HTML without auth;
    // health/chat/control APIs, not a random pathname, enforce the secret.
    assert_eq!(
        call(&app, &runtime_home, &runtime_cookie, "", None)
            .await
            .status(),
        200
    );
    assert_eq!(
        call(&app, &prefix, &runtime_cookie, "", None)
            .await
            .status(),
        401
    );
    assert_eq!(call(&app, &prefix, &cookie, "", None).await.status(), 200);
    assert_eq!(call(&app, &path, "", "", None).await.status(), 403);
}
#[tokio::test]
async fn review_confirm_consumes_immutable_assignment_and_stops_runtime() {
    let (app, prefix, cookie, fake, state) = fixture().await;
    let review = call(
        &app,
        &format!("{prefix}api/review"),
        &cookie,
        "",
        Some(json!({"draft":Draft::default(),"baseline":[]})),
    )
    .await;
    let approval = json_body(review).await;
    assert_eq!(fake.starts.load(Ordering::SeqCst), 0);
    let body = json!({"approval":approval["approval"],"start":true});
    assert_eq!(
        call(
            &app,
            &format!("{prefix}api/confirm"),
            &cookie,
            "",
            Some(body.clone())
        )
        .await
        .status(),
        202
    );
    for _ in 0..30 {
        if state.control.lock().await.phase == "running" {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(fake.starts.load(Ordering::SeqCst), 1);
    assert_eq!(fake.saves.load(Ordering::SeqCst), 1);
    assert_eq!(
        call(&app, "/session/fake", "", "", None).await.status(),
        200
    );
    assert_eq!(
        call(
            &app,
            &format!("{prefix}api/confirm"),
            &cookie,
            "",
            Some(body)
        )
        .await
        .status(),
        409
    );
    assert_eq!(
        call(
            &app,
            &format!("{prefix}api/stop"),
            &cookie,
            "",
            Some(json!({}))
        )
        .await
        .status(),
        200
    );
    for _ in 0..30 {
        if state.control.lock().await.stop.is_none() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(state.control.lock().await.stop.is_none());
}
#[tokio::test]
async fn unsupported_roles_save_but_never_start_and_unknown_fields_are_rejected() {
    let (app, prefix, cookie, fake, _) = fixture().await;
    let mut draft = Draft::default();
    draft.roles.brain = "different-brain".into();
    draft.roles.workers = vec!["worker-a".into(), "worker-b".into()];
    for start in [true, false] {
        let approval = json_body(
            call(
                &app,
                &format!("{prefix}api/review"),
                &cookie,
                "",
                Some(json!({"draft":draft,"baseline":[]})),
            )
            .await,
        )
        .await;
        assert_eq!(approval["executable"], false);
        let r = call(
            &app,
            &format!("{prefix}api/confirm"),
            &cookie,
            "",
            Some(json!({"approval":approval["approval"],"start":start})),
        )
        .await;
        assert_eq!(r.status(), if start { 409 } else { 200 });
    }
    assert_eq!(fake.starts.load(Ordering::SeqCst), 0);
    assert_eq!(fake.saves.load(Ordering::SeqCst), 1);
    let r = call(
        &app,
        &format!("{prefix}api/pick"),
        &cookie,
        "",
        Some(json!({"field":"shell","command":"no"})),
    )
    .await;
    assert_eq!(r.status(), 422);
}

#[tokio::test]
async fn approval_replacement_bounds_host_and_shutdown_are_fail_closed() {
    let (app, prefix, cookie, fake, state) = fixture().await;
    let mut r = Request::get(&prefix)
        .header(header::HOST, "foreign.invalid")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(r).await.unwrap().status(), 403);
    r = Request::get(&prefix)
        .header(header::HOST, "127.0.0.1:8015")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    r.headers_mut()
        .append(header::HOST, "127.0.0.1:8015".parse().unwrap());
    assert_eq!(app.clone().oneshot(r).await.unwrap().status(), 403);
    let review_path = format!("{prefix}api/review");
    let approved = json_body(
        call(
            &app,
            &review_path,
            &cookie,
            "",
            Some(json!({"draft":Draft::default(),"baseline":[]})),
        )
        .await,
    )
    .await;
    // A second review invalidates the previous grant even if the new input is bad.
    let mut bad = Draft::default();
    bad.roles.workers = vec!["same".into(), "same".into()];
    assert_eq!(
        call(
            &app,
            &review_path,
            &cookie,
            "",
            Some(json!({"draft":bad,"baseline":[]}))
        )
        .await
        .status(),
        409
    );
    assert_eq!(
        call(
            &app,
            &format!("{prefix}api/confirm"),
            &cookie,
            "",
            Some(json!({"approval":approved["approval"],"start":false}))
        )
        .await
        .status(),
        409
    );
    let r = Request::post(&review_path)
        .header(header::HOST, "127.0.0.1:8015")
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(vec![b' '; 129 * 1024]))
        .unwrap();
    assert_eq!(app.clone().oneshot(r).await.unwrap().status(), 413);
    state.shutdown.send_replace(true);
    assert_eq!(
        call(&app, &format!("{prefix}api/detect"), &cookie, "", None)
            .await
            .status(),
        410
    );
    assert_eq!(fake.saves.load(Ordering::SeqCst), 0);
    assert_eq!(fake.starts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn shutdown_revokes_a_confirm_already_waiting_for_the_control_lock() {
    let (_, _, _, fake, state) = fixture().await;
    let response = review(
        State(state.clone()),
        Json(Review {
            draft: Draft::default(),
            baseline: vec![],
        }),
    )
    .await;
    let approval = json_body(response).await["approval"]
        .as_str()
        .unwrap()
        .to_owned();
    let guard = state.control.lock().await;
    let task_state = state.clone();
    let confirm = tokio::spawn(async move {
        confirm(
            State(task_state),
            Json(Confirm {
                approval,
                start: true,
            }),
        )
        .await
    });
    tokio::task::yield_now().await;
    state.shutdown.send_replace(true);
    drop(guard);
    assert_eq!(confirm.await.unwrap().status(), 409);
    assert_eq!(fake.saves.load(Ordering::SeqCst), 0);
    assert_eq!(fake.starts.load(Ordering::SeqCst), 0);
}

struct Cancellable {
    starts: AtomicUsize,
    uncertain: bool,
}
impl Adapter for Cancellable {
    fn available(&self) -> bool {
        true
    }
    fn snapshot(&self) -> Result<Snapshot, String> {
        Ok(Snapshot {
            draft: Draft::default(),
            baseline: vec![],
            warning: None,
        })
    }
    fn review(&self, draft: Draft, baseline: Vec<u8>) -> Result<Approval, String> {
        Ok(Approval {
            draft,
            baseline,
            profile: vec![],
        })
    }
    fn save(&self, _: &Approval) -> Result<(), String> {
        Ok(())
    }
    fn pick(&self, _: String) -> Work<'_, String> {
        Box::pin(std::future::pending())
    }
    fn start(
        &self,
        _: Vec<u8>,
        _: bool,
        mut stop: watch::Receiver<bool>,
        _: String,
    ) -> Work<'_, Engine> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        let uncertain = self.uncertain;
        Box::pin(async move {
            if !*stop.borrow() {
                let _ = stop.changed().await;
            }
            Err(if uncertain {
                "host_cleanup_unconfirmed"
            } else {
                "startup_cancelled"
            }
            .into())
        })
    }
}
#[tokio::test]
async fn stop_cancels_startup_without_waiting_for_operation_lock_and_uncertainty_blocks_restart() {
    for uncertain in [false, true] {
        let adapter = Arc::new(Cancellable {
            starts: AtomicUsize::new(0),
            uncertain,
        });
        let (tx, _) = watch::channel(false);
        let state = Setup::new(adapter.clone(), tx);
        state
            .home
            .set("http://127.0.0.1:8015/trial/setup/".into())
            .unwrap();
        let Json(_) = stop(State(state.clone())).await;
        let reviewed = json_body(
            review(
                State(state.clone()),
                Json(Review {
                    draft: Draft::default(),
                    baseline: vec![],
                }),
            )
            .await,
        )
        .await;
        assert_eq!(
            confirm(
                State(state.clone()),
                Json(Confirm {
                    approval: reviewed["approval"].as_str().unwrap().into(),
                    start: true
                })
            )
            .await
            .status(),
            202
        );
        for _ in 0..100 {
            if adapter.starts.load(Ordering::SeqCst) > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        let _ = stop(State(state.clone())).await;
        for _ in 0..100 {
            if state.control.lock().await.phase == "blocked" {
                break;
            }
            tokio::task::yield_now().await;
        }
        let c = state.control.lock().await;
        assert_eq!(c.phase, "blocked");
        assert_eq!(c.stop.is_some(), uncertain);
        drop(c);
        if uncertain {
            let _ = stop(State(state.clone())).await;
            assert_eq!(state.control.lock().await.phase, "blocked");
            assert_eq!(
                review(
                    State(state.clone()),
                    Json(Review {
                        draft: Draft::default(),
                        baseline: vec![]
                    })
                )
                .await
                .status(),
                409
            );
        }
    }
}
