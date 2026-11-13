use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use seiya_server::app;
use serde_json::Value;
use std::net::SocketAddr;
use tower::ServiceExt;

const TOKEN: &str = "synthetic-fixture-token-not-a-real-secret";
const HOST: &str = "127.0.0.1:8012";

fn router(key: Option<&str>) -> Router {
    app(HOST.parse().unwrap(), key.map(str::to_owned)).unwrap()
}

async fn response(router: Router, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/json; charset=utf-8"
    );
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(headers["x-seiya-experimental"], "discovery-only");
    (status, headers, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn discovery_matches_shared_python_golden_bodies() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../contracts/openai-compat/v0.1.0/scenarios.json"
    ))
    .unwrap();
    let scenario = fixtures["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "discovery_and_health")
        .unwrap();
    for step in scenario["steps"].as_array().unwrap() {
        let request = Request::builder()
            .method(step["method"].as_str().unwrap())
            .uri(step["path"].as_str().unwrap())
            .header(header::HOST, HOST)
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let (status, _, body) = response(router(Some(TOKEN)), request).await;
        assert_eq!(status.as_u16() as u64, step["status"].as_u64().unwrap());
        assert_eq!(body, step["expected"]);
    }
}

#[tokio::test]
async fn discovery_guard_errors_match_shared_http_fixtures() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../contracts/openai-compat/v0.1.0/http-policy.json"
    ))
    .unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let mut request = Request::builder()
            .method(case["method"].as_str().unwrap())
            .uri(case["path"].as_str().unwrap())
            .header(header::HOST, HOST)
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        for (name, value) in case["headers"].as_object().unwrap() {
            if let Some(value) = value.as_str() {
                request.headers_mut().insert(
                    header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                    value.parse().unwrap(),
                );
            } else {
                request.headers_mut().remove(name);
            }
        }
        let (status, _, body) = response(router(Some(TOKEN)), request).await;
        assert_eq!(status.as_u16() as u64, case["status"].as_u64().unwrap());
        assert_eq!(body, serde_json::json!({"error":{"message":case["error"]}}));
    }
}

#[tokio::test]
async fn optional_auth_localhost_and_same_origin_are_supported() {
    let request = Request::builder()
        .uri("/health")
        .header(header::HOST, "localhost:8012")
        .header(header::ORIGIN, "http://localhost:8012")
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = response(router(None), request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["backend_readiness"], "not checked");
    assert_eq!(body["busy"], false);
}

#[tokio::test]
async fn missing_duplicate_and_malformed_guard_headers_fail_closed() {
    let missing_host = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        response(router(None), missing_host).await.0,
        StatusCode::FORBIDDEN
    );
    let duplicate_host = Request::builder()
        .uri("/health")
        .header(header::HOST, HOST)
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        response(router(None), duplicate_host).await.0,
        StatusCode::FORBIDDEN
    );
    let duplicate_origin = Request::builder()
        .uri("/health")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, "http://localhost:8012")
        .header(header::ORIGIN, "http://localhost:8012")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        response(router(None), duplicate_origin).await.0,
        StatusCode::FORBIDDEN
    );
    let duplicate_auth = Request::builder()
        .uri("/health")
        .header(header::HOST, HOST)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        response(router(Some(TOKEN)), duplicate_auth).await.0,
        StatusCode::UNAUTHORIZED
    );
    for origin in ["null", "https://localhost:8012", "http://localhost:8013"] {
        let request = Request::builder()
            .uri("/health")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            response(router(None), request).await.0,
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn unsupported_routes_never_claim_inference_or_native_tools() {
    for (method, path, expected) in [
        ("POST", "/v1/chat/completions", StatusCode::NOT_IMPLEMENTED),
        ("GET", "/", StatusCode::NOT_FOUND),
        ("POST", "/api/chat", StatusCode::NOT_FOUND),
        ("GET", "/native/v1/capabilities", StatusCode::NOT_FOUND),
        ("POST", "/health", StatusCode::METHOD_NOT_ALLOWED),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .body(Body::from("not parsed or forwarded"))
            .unwrap();
        let (status, _, body) = response(router(None), request).await;
        assert_eq!(status, expected);
        assert!(body["error"]["message"].is_string());
        assert!(body.get("choices").is_none());
    }
}

#[test]
fn router_rejects_unbound_addresses_and_invalid_keys() {
    for address in ["0.0.0.0:8012", "[::1]:8012", "127.0.0.1:0"] {
        assert!(app(address.parse::<SocketAddr>().unwrap(), None).is_err());
    }
    assert!(app(HOST.parse().unwrap(), Some("".into())).is_err());
    assert!(app(HOST.parse().unwrap(), Some("bad\nkey".into())).is_err());
}
