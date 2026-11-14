//! Temporary browser authentication; no key in HTML/storage or HTTP logs.
use crate::{
    config::ConfigError,
    prepared_web::{self, PreparedWeb},
};
use axum::{
    extract::{Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use subtle::ConstantTimeEq;
struct Access {
    scheme: &'static str,
    host: String,
    key: String,
    bootstrap: Vec<(String, AtomicBool)>,
    prefix: String,
}
fn same(a: &str, b: &str) -> bool {
    bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}
/// An inert desktop shell needs the same origin/cookie boundary as inference,
/// but must not construct a model adapter just to display onboarding.
pub(crate) fn desktop_shell(address: SocketAddr, inner: Router) -> (Router, String, String) {
    let token = || {
        format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        )
    };
    let a = Arc::new(Access {
        scheme: "http",
        host: address.to_string(),
        key: token(),
        bootstrap: vec![(token(), AtomicBool::new(false))],
        prefix: format!("/trial/{}", uuid::Uuid::new_v4().simple()),
    });
    let link = format!("http://{address}/session/{}", a.bootstrap[0].0);
    let home = format!("http://{address}{}/", a.prefix);
    let inner = inner
        .layer(middleware::from_fn_with_state(
            a.clone(),
            shell_authorization,
        ))
        .layer(middleware::from_fn_with_state(a.clone(), cookie));
    let app = Router::new().nest(&format!("{}/", a.prefix), inner).merge(
        Router::new()
            .route("/session/{token}", get(bootstrap))
            .with_state(a),
    );
    (app, link, home)
}
async fn shell_authorization(State(a): State<Arc<Access>>, r: Request, next: Next) -> Response {
    let expected = format!("Bearer {}", a.key);
    if crate::single_header(r.headers(), header::AUTHORIZATION).is_none_or(|v| !same(v, &expected))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut response = next.run(r).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
        .headers_mut()
        .insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    response
}
fn origin(a: &Access, r: &Request) -> bool {
    crate::single_header(r.headers(), header::HOST) == Some(a.host.as_str())
        && (!r.headers().contains_key(header::ORIGIN)
            || crate::single_header(r.headers(), header::ORIGIN)
                == Some(format!("{}://{}", a.scheme, a.host).as_str()))
}
async fn cookie(State(a): State<Arc<Access>>, mut r: Request, next: Next) -> Response {
    if !origin(&a, &r) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !r.headers().contains_key(header::AUTHORIZATION) {
        if let Some(raw) = crate::single_header(r.headers(), header::COOKIE) {
            let values: Vec<_> = raw
                .split(';')
                .map(str::trim)
                .filter_map(|s| s.strip_prefix("seiya_trial="))
                .collect();
            if values.len() == 1 && same(values[0], &a.key) {
                r.headers_mut().insert(
                    header::AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {}", a.key)).unwrap(),
                );
            }
        }
    }
    next.run(r).await
}
async fn bootstrap(
    State(a): State<Arc<Access>>,
    Path(token): Path<String>,
    r: Request,
) -> Response {
    if !origin(&a, &r) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some((_, used)) = a.bootstrap.iter().find(|(t, _)| same(&token, t)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if used.swap(true, Ordering::AcqRel) {
        return StatusCode::FORBIDDEN.into_response();
    }
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, format!("{}/", a.prefix)),
            (header::CACHE_CONTROL, "no-store".into()),
            (header::REFERRER_POLICY, "no-referrer".into()),
            (
                header::SET_COOKIE,
                format!(
                    "seiya_trial={}; HttpOnly; SameSite=Strict; Path={}/; Max-Age=1800{}",
                    a.key,
                    a.prefix,
                    if a.scheme == "https" { "; Secure" } else { "" }
                ),
            ),
        ],
    )
        .into_response()
}
pub(crate) fn app(
    address: SocketAddr,
    state: Arc<PreparedWeb>,
) -> Result<(Router, String), ConfigError> {
    configured(address, state, "http")
}
#[cfg(any(test, target_os = "macos"))]
pub(crate) fn lan_tls_app(
    address: SocketAddr,
    state: Arc<PreparedWeb>,
) -> Result<(Router, String), ConfigError> {
    configured(address, state, "https")
}
fn configured(
    address: SocketAddr,
    state: Arc<PreparedWeb>,
    scheme: &'static str,
) -> Result<(Router, String), ConfigError> {
    let (app, mut links) = configured_links(address, state, scheme, 1)?;
    Ok((app, links.remove(0)))
}
#[cfg(target_os = "macos")]
pub(crate) fn lan_tls_probe_app(
    address: SocketAddr,
    state: Arc<PreparedWeb>,
) -> Result<(Router, String, String), ConfigError> {
    let (app, mut links) = configured_links(address, state, "https", 2)?;
    let user = links.remove(0);
    Ok((app, user, links.remove(0)))
}
fn configured_links(
    address: SocketAddr,
    state: Arc<PreparedWeb>,
    scheme: &'static str,
    count: usize,
) -> Result<(Router, Vec<String>), ConfigError> {
    let token = || {
        format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        )
    };
    let a = Arc::new(Access {
        scheme,
        host: address.to_string(),
        key: token(),
        bootstrap: (0..count)
            .map(|_| (token(), AtomicBool::new(false)))
            .collect(),
        prefix: format!("/trial/{}", uuid::Uuid::new_v4().simple()),
    });
    state.browser_prefix(a.prefix.clone());
    let links = a
        .bootstrap
        .iter()
        .map(|(t, _)| format!("{scheme}://{address}/session/{t}"))
        .collect();
    let inner = if scheme == "https" {
        prepared_web::lan_tls_app(address, a.key.clone(), state)?
    } else {
        prepared_web::app(address, a.key.clone(), state)?
    }
    .layer(middleware::from_fn_with_state(a.clone(), cookie));
    // Axum0.8 distinguishes /prefix from /prefix/. Bootstrap and cookie scope
    // deliberately use the slash form, so the nested root must match it too.
    let app = Router::new().nest(&format!("{}/", a.prefix), inner).merge(
        Router::new()
            .route("/session/{token}", get(bootstrap))
            .with_state(a),
    );
    Ok((app, links))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        jobs::Demo,
        prepared_web::FOREGROUND,
        runtime::{Backend, RuntimeConfig, RuntimeFuture, RuntimeTransport},
    };
    use axum::body::Body;
    use tower::ServiceExt;
    #[tokio::test]
    async fn desktop_loopback_cookie_root_preserves_css_and_rejects_foreign_origin() {
        use http_body_util::BodyExt;
        let address = "127.0.0.1:8015".parse().unwrap();
        let (app, link) = app(address, PreparedWeb::diagnostics_only().unwrap()).unwrap();
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get(reqwest::Url::parse(&link).unwrap().path())
                    .header("host", "127.0.0.1:8015")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 303);
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let path = response.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .to_owned();
        let request = |origin: &str| {
            axum::http::Request::get(&path)
                .header("host", "127.0.0.1:8015")
                .header("cookie", &cookie)
                .header("origin", origin)
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(
            app.clone()
                .oneshot(request("https://outside.invalid"))
                .await
                .unwrap()
                .status(),
            403
        );
        let response = app.oneshot(request("http://127.0.0.1:8015")).await.unwrap();
        assert_eq!(response.status(), 200);
        let html = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
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
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn probe_bootstrap_does_not_consume_user_and_diagnostics_cannot_infer() {
        use http_body_util::BodyExt;
        let (app, user, probe) = lan_tls_probe_app(
            "192.168.1.149:8016".parse().unwrap(),
            PreparedWeb::diagnostics_only().unwrap(),
        )
        .unwrap();
        let request = |url: &str| {
            axum::http::Request::get(reqwest::Url::parse(url).unwrap().path())
                .header("host", "192.168.1.149:8016")
                .body(Body::empty())
                .unwrap()
        };
        let r = app.clone().oneshot(request(&probe)).await.unwrap();
        assert_eq!(r.status(), 303);
        let cookie = r.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let prefix = r.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .trim_end_matches('/')
            .to_owned();
        assert_eq!(
            app.clone()
                .oneshot(request(&format!("https://192.168.1.149:8016{prefix}/")))
                .await
                .unwrap()
                .status(),
            200
        );
        let h = app
            .clone()
            .oneshot(
                axum::http::Request::get(format!("{prefix}/health"))
                    .header("host", "192.168.1.149:8016")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&h.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["no_inference"], true);
        assert_eq!(v["images"]["available"], false);
        let body = r#"{"model":"pegassus","messages":[{"role":"user","content":"not executed"}]}"#;
        let r = app
            .clone()
            .oneshot(
                axum::http::Request::post(format!("{prefix}/api/chat"))
                    .header("host", "192.168.1.149:8016")
                    .header("cookie", cookie)
                    .header("content-type", "application/json")
                    .header("content-length", body.len())
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), 503);
        assert_eq!(
            app.clone().oneshot(request(&probe)).await.unwrap().status(),
            403
        );
        assert_eq!(app.oneshot(request(&user)).await.unwrap().status(), 303);
    }
    struct NoModel;
    impl RuntimeTransport for NoModel {
        fn complete(&self, _: serde_json::Value) -> RuntimeFuture<'_> {
            panic!("no dispatch authorized")
        }
    }
    #[tokio::test]
    async fn bootstrap_one_use_cookie_auth_and_origin_fail_closed() {
        let c = RuntimeConfig {
            id: "fixture".into(),
            model: FOREGROUND.into(),
            backend: Backend::Mlx,
            base_url: "http://127.0.0.1:49380/v1".into(),
            timeout_seconds: 10,
            max_output_tokens: 256,
        };
        let state = PreparedWeb::blocked(c, Arc::new(NoModel), Arc::new(Demo)).unwrap();
        let (app, link) = app("127.0.0.1:49381".parse().unwrap(), state).unwrap();
        let path = reqwest::Url::parse(&link).unwrap().path().to_string();
        let request = |path: &str| {
            axum::http::Request::get(path)
                .header("host", "127.0.0.1:49381")
                .body(Body::empty())
                .unwrap()
        };
        let r = app.clone().oneshot(request(&path)).await.unwrap();
        assert_eq!(r.status(), 303);
        let prefix = r.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .trim_end_matches('/')
            .to_owned();
        let health = format!("{prefix}/health");
        let denied = app.clone().oneshot(request(&health)).await.unwrap();
        assert_eq!(denied.status(), 401);
        let value = r.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_string();
        assert!(value.contains("HttpOnly; SameSite=Strict"));
        assert_eq!(
            app.clone().oneshot(request(&path)).await.unwrap().status(),
            403
        );
        let cookie = value.split(';').next().unwrap();
        let mut r = request(&health);
        r.headers_mut()
            .insert(header::COOKIE, cookie.parse().unwrap());
        assert_eq!(app.clone().oneshot(r).await.unwrap().status(), 200);
        let mut r = request(&health);
        r.headers_mut()
            .insert(header::COOKIE, cookie.parse().unwrap());
        r.headers_mut()
            .insert(header::ORIGIN, "http://evil.example".parse().unwrap());
        assert_eq!(app.oneshot(r).await.unwrap().status(), 403);
    }
    #[tokio::test]
    async fn lan_https_cookie_secure_exact_authority_and_scheme() {
        let c = RuntimeConfig {
            id: "fixture".into(),
            model: FOREGROUND.into(),
            backend: Backend::Mlx,
            base_url: "http://127.0.0.1:49380/v1".into(),
            timeout_seconds: 10,
            max_output_tokens: 256,
        };
        let state = PreparedWeb::blocked(c, Arc::new(NoModel), Arc::new(Demo)).unwrap();
        let (app, link) = lan_tls_app("192.168.1.149:8015".parse().unwrap(), state).unwrap();
        assert!(link.starts_with("https://192.168.1.149:8015/"));
        let path = reqwest::Url::parse(&link).unwrap().path().to_owned();
        let request = |path: &str, origin: &str| {
            axum::http::Request::get(path)
                .header("host", "192.168.1.149:8015")
                .header("origin", origin)
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(
            app.clone()
                .oneshot(request(&path, "http://192.168.1.149:8015"))
                .await
                .unwrap()
                .status(),
            403
        );
        let r = app
            .clone()
            .oneshot(request(&path, "https://192.168.1.149:8015"))
            .await
            .unwrap();
        assert_eq!(r.status(), 303);
        let cookie = r.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(cookie.contains("; Secure"));
        let health = format!("{}health", r.headers()[header::LOCATION].to_str().unwrap());
        let mut req = request(&health, "https://192.168.1.149:8015");
        req.headers_mut().insert(
            header::COOKIE,
            cookie.split(';').next().unwrap().parse().unwrap(),
        );
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), 200);
        let mut wrong = request(&health, "https://192.168.1.149:8015");
        wrong
            .headers_mut()
            .insert(header::HOST, "192.168.1.150:8015".parse().unwrap());
        assert_eq!(app.oneshot(wrong).await.unwrap().status(), 403);
    }
}
