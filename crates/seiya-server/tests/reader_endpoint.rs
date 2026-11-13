//! Router requests and synthetic DNS only. No listener, DNS or TLS network I/O.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use seiya_server::{
    app, app_with_reader,
    reader::{
        transport::{Network, Stream},
        Error, Reader,
    },
};
use std::{
    future::Future,
    net::IpAddr,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:8012";
const PATH: &str = "/internal/reader/v0.1/read";
const KEY: &str = "synthetic-reader-token-not-a-secret";
const FRAME: &str = include_str!("../../../contracts/reader-ipc/v0.1-draft.1/request.json");
type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'a>>;

struct Fake {
    calls: AtomicUsize,
    hang: bool,
}
impl Network for Fake {
    fn resolve(&self, _: String) -> Pending<'_, Vec<IpAddr>> {
        Box::pin(async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.hang {
                std::future::pending::<()>().await;
            }
            Ok(vec!["127.0.0.1".parse().unwrap()])
        })
    }
    fn connect(&self, _: String, _: IpAddr) -> Pending<'_, Box<dyn Stream>> {
        panic!("private DNS must not connect")
    }
}

fn request(body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(PATH)
        .header("host", HOST)
        .header("authorization", format!("Bearer {KEY}"))
        .header("content-type", "application/json")
        .header("content-length", body.len())
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn setup(hang: bool) -> (Router, Arc<Reader>, Arc<Fake>) {
    let fake = Arc::new(Fake {
        calls: AtomicUsize::new(0),
        hang,
    });
    let reader = Arc::new(Reader::new(fake.clone()));
    (
        app_with_reader(HOST.parse().unwrap(), KEY.into(), reader.clone()).unwrap(),
        reader,
        fake,
    )
}

#[tokio::test]
async fn default_router_has_no_reader_and_key_is_mandatory() {
    assert_eq!(
        app(HOST.parse().unwrap(), None)
            .unwrap()
            .oneshot(request(FRAME))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let (_, reader, _) = setup(false);
    assert!(app_with_reader(HOST.parse().unwrap(), String::new(), reader).is_err());
}

#[tokio::test]
async fn guards_and_frame_rejections_precede_any_network() {
    let (router, _, fake) = setup(false);
    for (name, value, status) in [
        ("host", "evil.invalid", 403),
        ("authorization", "Bearer wrong", 401),
        ("origin", "https://evil.invalid", 403),
        ("content-type", "text/plain", 415),
        ("content-encoding", "gzip", 415),
        ("content-length", "16385", 413),
        ("transfer-encoding", "chunked", 400),
    ] {
        let mut req = request(FRAME);
        req.headers_mut().insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
        assert_eq!(
            router.clone().oneshot(req).await.unwrap().status().as_u16(),
            status
        );
    }
    let mut req = request(FRAME);
    req.headers_mut()
        .append("content-length", FRAME.len().into());
    assert_eq!(
        router.clone().oneshot(req).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        router.oneshot(request("{}")).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn blocked_page_has_correlated_null_text_and_no_url_leak() {
    let (router, _, fake) = setup(false);
    let reply = router.oneshot(request(FRAME)).await.unwrap();
    assert_eq!(reply.status(), StatusCode::OK);
    let bytes = reply.into_body().collect().await.unwrap().to_bytes();
    let parsed = seiya_contracts::reader_frame::parse_request(FRAME.as_bytes(), true).unwrap();
    seiya_contracts::reader_frame::validate_response(&parsed, &bytes, true).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["results"][0]["status"], "policy_blocked");
    assert_eq!(body["results"][0]["text"], serde_json::Value::Null);
    assert!(!std::str::from_utf8(&bytes).unwrap().contains("example.org"));
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn global_deadline_keeps_reader_quarantined_without_second_dns() {
    let (router, reader, fake) = setup(true);
    let frame = FRAME.replace("12000", "20");
    assert_eq!(
        router
            .clone()
            .oneshot(request(&frame))
            .await
            .unwrap()
            .status(),
        StatusCode::GATEWAY_TIMEOUT
    );
    assert!(reader.quarantined());
    assert_eq!(
        router.oneshot(request(FRAME)).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn dropping_handler_future_quarantines_but_is_not_a_tcp_disconnect_test() {
    let (router, reader, fake) = setup(true);
    let mut pending = Box::pin(router.clone().oneshot(request(FRAME)));
    tokio::select! {
        _ = &mut pending => panic!("unexpected completion"),
        _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => (),
    }
    assert_eq!(
        router
            .clone()
            .oneshot(request(FRAME))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(pending);
    assert!(reader.quarantined());
    assert_eq!(
        router.oneshot(request(FRAME)).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn three_unique_sources_share_one_batch_and_four_are_rejected() {
    let (router, _, fake) = setup(false);
    let mut frame: serde_json::Value = serde_json::from_str(FRAME).unwrap();
    frame["sources"] = serde_json::json!([
        {"id":"one", "url":"https://example.org/a"},
        {"id":"two", "url":"https://example.org/b"},
        {"id":"three", "url":"https://example.org/c"}]);
    let input = serde_json::to_string(&frame).unwrap();
    let response = router.clone().oneshot(request(&input)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let parsed = seiya_contracts::reader_frame::parse_request(input.as_bytes(), true).unwrap();
    seiya_contracts::reader_frame::validate_response(&parsed, &bytes, true).unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
    frame["sources"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"id":"four","url":"https://example.org/d"}));
    assert_eq!(
        router
            .oneshot(request(&frame.to_string()))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
}

fn cancel_request(id: &str) -> Request<Body> {
    let body = serde_json::json!({"version":"0.1-draft.1","run_id":id}).to_string();
    let mut req = request(&body);
    *req.uri_mut() = "/internal/reader/v0.1/cancel".parse().unwrap();
    req
}

#[tokio::test]
async fn explicit_cancel_is_authenticated_and_covers_upload_race() {
    let (router, reader, fake) = setup(false);
    let mut req = cancel_request("run-1");
    req.headers_mut().remove("authorization");
    assert_eq!(
        router.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    for _ in 0..2 {
        assert_eq!(
            router
                .clone()
                .oneshot(cancel_request("run-1"))
                .await
                .unwrap()
                .status(),
            StatusCode::ACCEPTED
        );
    }
    assert_eq!(
        router.oneshot(request(FRAME)).await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert!(!reader.quarantined()); // No I/O was ever started.
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn in_flight_cancel_quarantines_and_wrong_run_does_not_cancel_it() {
    let (router, reader, fake) = setup(true);
    let mut pending = Box::pin(router.clone().oneshot(request(FRAME)));
    tokio::select! {
        _ = &mut pending => panic!("unexpected completion"),
        _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => (),
    }
    assert_eq!(
        router
            .clone()
            .oneshot(cancel_request("other-run"))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    assert!(!reader.quarantined());
    assert_eq!(
        router
            .clone()
            .oneshot(cancel_request("run-1"))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(pending.await.unwrap().status(), StatusCode::CONFLICT);
    assert!(reader.quarantined());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn cancellation_tombstones_are_bounded_and_do_not_expire_silently() {
    let (_, reader, _) = setup(false);
    for i in 0..128 {
        reader.cancel(&format!("run-{i}")).unwrap();
    }
    reader.cancel("run-0").unwrap();
    assert_eq!(reader.cancel("overflow"), Err(Error::Unavailable));
    assert_eq!(reader.cancel("invalid id"), Err(Error::Policy));
}

#[tokio::test]
async fn selection_auth_and_original_frames_are_checked_without_network() {
    let (router, _, fake) = setup(false);
    let body = include_str!("../../../contracts/reader-ipc/v0.1-draft.1/select.json");
    let mut req = request(body);
    *req.uri_mut() = "/internal/reader/v0.1/select".parse().unwrap();
    let mut unauthenticated = request(body);
    *unauthenticated.uri_mut() = req.uri().clone();
    unauthenticated.headers_mut().remove("authorization");
    assert_eq!(
        router
            .clone()
            .oneshot(unauthenticated)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let reply = router.clone().oneshot(req).await.unwrap();
    assert_eq!(reply.status(), StatusCode::OK);
    let bytes = reply.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["sources"][0]["id"], "source-1");
    let mut invalid: serde_json::Value = serde_json::from_str(body).unwrap();
    invalid["ids"] = serde_json::json!(["foreign"]);
    let mut req = request(&invalid.to_string());
    *req.uri_mut() = "/internal/reader/v0.1/select".parse().unwrap();
    assert_eq!(
        router.oneshot(req).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}
