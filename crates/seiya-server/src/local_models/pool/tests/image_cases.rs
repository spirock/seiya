use super::*;
use crate::image_jobs::Failure;
use base64::{engine::general_purpose::STANDARD, Engine};
use seiya_contracts::image_job::{ImageRequest, ImageStage};
struct ImageEndpoint {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    hold: bool,
    bad: bool,
}
impl Endpoint for ImageEndpoint {
    fn advertises(&self, binding: &str) -> Probe<'_> {
        let valid = binding == "image-binding";
        Box::pin(async move { Ok(valid) })
    }
    fn complete(&self, _: Value) -> RuntimeFuture<'_> {
        panic!("image is not a chat completion")
    }
    fn image(&self, body: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            assert_eq!(body["model"], "image-binding");
            assert_eq!(body["size"], "512x512");
            assert_eq!(body["n"], 1);
            assert!(body.get("messages").is_none());
            self.entered.notify_one();
            if self.hold {
                self.release.notified().await;
            }
            Ok(
                json!({"model":if self.bad{"wrong"}else{"image-binding"},"data":[{"b64_json":STANDARD.encode(crate::local_models::images::fixture_png())}]}),
            )
        })
    }
}
fn configured(
    f: &Fixture,
    hold: bool,
    bad: bool,
) -> (
    Arc<Pool>,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Notify>,
) {
    let mut c = catalog();
    let mut runtime = c.models[0].runtime.clone();
    runtime.id = "image".into();
    runtime.model = "image-binding".into();
    runtime.base_url = "http://127.0.0.1:21004/v1".into();
    c.image_models.push(crate::local_models::ImageModel {
        id: "image".into(),
        runtime,
    });
    c.image = Some("image".into());
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let mut map = endpoints(
        &c,
        &Arc::new(Mutex::new(vec![])),
        false,
        false,
        Arc::new(tokio::sync::Notify::new()),
    );
    map.insert(
        "image".into(),
        Arc::new(ImageEndpoint {
            entered: entered.clone(),
            release: release.clone(),
            hold,
            bad,
        }),
    );
    (
        Pool::configured(c, &f.0, map, Arc::new(Memory(AtomicBool::new(true)))).unwrap(),
        entered,
        release,
    )
}
fn request() -> ImageRequest {
    ImageRequest {
        prompt: "Synthetic test image".into(),
        width: 512,
        height: 512,
        seed: 42,
    }
}
#[cfg(target_os = "macos")]
#[tokio::test]
async fn http_image_adapter_uses_exact_local_inventory_and_generation_route() {
    use axum::{
        routing::{get, post},
        Json, Router,
    };
    let f = Fixture::new();
    let mut c = catalog();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let listener = loop {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        if !matches!(
            l.local_addr().unwrap().port(),
            8015 | 8016 | 11434 | 49399 | 49400
        ) {
            break l;
        }
    };
    let mut runtime = c.models[0].runtime.clone();
    runtime.id = "image".into();
    runtime.model = "image-binding".into();
    runtime.base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    c.image_models.push(crate::local_models::ImageModel {
        id: "image".into(),
        runtime: runtime.clone(),
    });
    c.image = Some("image".into());
    let router=Router::new().route("/v1/models",get(||async {Json(json!({"data":[{"id":"image-binding"}]}))})).route("/v1/images/generations",post(move|Json(body):Json<Value>|{let calls=observed.clone();async move {
        calls.fetch_add(1,Ordering::SeqCst);assert_eq!(body["model"],"image-binding");assert_eq!(body["response_format"],"b64_json");assert_eq!(body["seed"],42);
        Json(json!({"model":"image-binding","data":[{"b64_json":STANDARD.encode(crate::local_models::images::fixture_png())}]}))
    }}));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut map = endpoints(
        &c,
        &Arc::new(Mutex::new(vec![])),
        false,
        false,
        Arc::new(tokio::sync::Notify::new()),
    );
    map.insert(
        "image".into(),
        Arc::new(Http(HttpTransport::new(&runtime).unwrap())),
    );
    let p = Pool::configured(c, &f.0, map, Arc::new(Memory(AtomicBool::new(true)))).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let (_tx, cancel) = watch::channel(false);
    let (stage, _) = watch::channel(ImageStage::Preparing);
    let png = p.image(request(), cancel, stage).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(seiya_observe::image::artifact::decode(&png, 512, 512).is_ok());
    assert!(!p.uncertain());
    server.abort();
    let _ = server.await;
}
#[cfg(target_os = "macos")]
#[tokio::test]
async fn image_and_text_share_slot_and_cancel_waits_for_confirmed_response() {
    let f = Fixture::new();
    let (p, entered, release) = configured(&f, true, false);
    let (cancel, rx) = watch::channel(false);
    let (stage, _) = watch::channel(ImageStage::Preparing);
    let owner = p.clone();
    let task = tokio::spawn(async move { owner.image(request(), rx, stage).await });
    entered.notified().await;
    assert!(p
        .bound("brain")
        .complete(json!({"model":"runtime-brain","messages":[]}))
        .await
        .is_err());
    cancel.send_replace(true);
    assert!(!task.is_finished());
    assert!(!p.idle());
    assert!(f.0.join("coordination/http-21004.inflight").exists());
    release.notify_one();
    assert_eq!(task.await.unwrap(), Err(Failure::Cancelled));
    assert!(p.idle());
    assert!(!p.uncertain());
    assert!(!f.0.join("coordination/http-21004.inflight").exists());
    assert!(p
        .bound("brain")
        .complete(json!({"model":"runtime-brain","messages":[]}))
        .await
        .is_ok());
}
#[tokio::test(start_paused = true)]
async fn image_timeout_keeps_journal_and_blocks_text_and_new_catalogue() {
    let f = Fixture::new();
    let (p, _, _) = configured(&f, true, false);
    let (_, cancel) = watch::channel(false);
    let (stage, _) = watch::channel(ImageStage::Preparing);
    assert_eq!(
        p.image(request(), cancel, stage).await,
        Err(Failure::Uncertain)
    );
    assert!(p.uncertain());
    assert!(f.0.join("coordination/http-21004.inflight").exists());
    assert!(p
        .bound("brain")
        .complete(json!({"model":"runtime-brain","messages":[]}))
        .await
        .is_err());
    drop(p);
    let c = catalog();
    let map = endpoints(
        &c,
        &Arc::new(Mutex::new(vec![])),
        false,
        false,
        Arc::new(tokio::sync::Notify::new()),
    );
    assert!(Pool::configured(c, &f.0, map, Arc::new(Memory(AtomicBool::new(true)))).is_err());
}
#[tokio::test]
async fn bad_binding_is_quarantined_and_pre_cancel_does_not_dispatch() {
    let f = Fixture::new();
    let (p, _, _) = configured(&f, false, true);
    let (cancel, rx) = watch::channel(true);
    let (stage, _) = watch::channel(ImageStage::Preparing);
    assert_eq!(
        p.image(request(), rx, stage.clone()).await,
        Err(Failure::Blocked)
    );
    assert!(!p.uncertain());
    cancel.send_replace(false);
    assert_eq!(
        p.image(request(), cancel.subscribe(), stage).await,
        Err(Failure::Uncertain)
    );
    assert!(p.uncertain());
}
#[tokio::test]
async fn low_or_unknown_resources_never_start_image_or_create_intent() {
    let f = Fixture::new();
    let (mut p, _, _) = configured(&f, false, false);
    Arc::get_mut(&mut p).unwrap().resources = Arc::new(Memory(AtomicBool::new(false)));
    let (_tx, cancel) = watch::channel(false);
    let (stage, _) = watch::channel(ImageStage::Preparing);
    assert_eq!(
        p.image(request(), cancel, stage).await,
        Err(Failure::Blocked)
    );
    assert!(!p.uncertain());
    assert!(!f.0.join("coordination/http-21004.inflight").exists());
}
