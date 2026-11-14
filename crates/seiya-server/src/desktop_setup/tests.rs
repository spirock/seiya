use super::*;
use axum::{
    body::Body,
    http::{header, Request},
};
use http_body_util::BodyExt;
use std::sync::atomic::{AtomicUsize, Ordering};

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
#[tokio::test]
async fn inert_shell_preserves_styles_and_requires_auth_before_side_effects() {
    let (app, prefix, cookie, fake, _) = fixture().await;
    assert_eq!(call(&app, &prefix, "", "", None).await.status(), 401);
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
