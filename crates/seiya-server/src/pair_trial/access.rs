//! Browser authentication; no key in HTML or HTTP logs.
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
    local_app: bool,
    development_localhost: bool,
    reusable_test_link: bool,
    persistent_test: bool,
    scheme: &'static str,
    host: String,
    key: String,
    bootstrap: Vec<(String, AtomicBool)>,
    prefix: String,
}
fn same(a: &str, b: &str) -> bool {
    bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}
/// Persist only the test cookie key, never the browser bootstrap URL.
pub(crate) fn test_key() -> Result<String, String> {
    use crate::local_models::storage;
    let root = storage::default_root()?;
    test_key_at(&root)
}
fn test_key_at(root: &std::path::Path) -> Result<String, String> {
    use crate::local_models::storage;
    storage::ensure_directory(root)?;
    let path = root.join("test-chat-8016.key");
    let bytes = match storage::read(&path)? {
        Some(bytes) => bytes,
        None => {
            let candidate = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            match storage::new_file(&path, candidate.as_bytes()) {
                Ok(()) => candidate.into_bytes(),
                Err(_) => storage::read(&path)?.ok_or("test_key_create_failed")?,
            }
        }
    };
    if bytes.len() != 64
        || !bytes
            .iter()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err("test_key_invalid".into());
    }
    String::from_utf8(bytes).map_err(|_| "test_key_invalid".into())
}
/// An inert desktop shell needs the same origin/cookie boundary as inference,
/// but must not construct a model adapter just to display onboarding.
pub(crate) fn desktop_shell(address: SocketAddr, inner: Router) -> (Router, String, String) {
    desktop_shell_with_key(address, inner, None)
}
pub(crate) fn desktop_shell_with_key(
    address: SocketAddr,
    inner: Router,
    test_key: Option<String>,
) -> (Router, String, String) {
    let persistent_test = cfg!(debug_assertions)
        && test_key.is_some()
        && address == SocketAddr::from(([127, 0, 0, 1], 8016));
    let token = || {
        format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        )
    };
    let a = Arc::new(Access {
        local_app: true,
        development_localhost: false,
        persistent_test,
        reusable_test_link: address.ip() == std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
            && address.port() == 8016,
        scheme: "http",
        host: address.to_string(),
        key: if persistent_test {
            test_key.unwrap()
        } else {
            token()
        },
        bootstrap: vec![(token(), AtomicBool::new(false))],
        prefix: format!("/app/{}", uuid::Uuid::new_v4().simple()),
    });
    let link = format!("http://{address}/session/{}", a.bootstrap[0].0);
    let home = format!("http://{address}{}/", a.prefix);
    let inner = inner
        .layer(middleware::from_fn_with_state(
            a.clone(),
            shell_authorization,
        ))
        .layer(middleware::from_fn_with_state(a.clone(), cookie));
    let mut app = Router::new().nest(&format!("{}/", a.prefix), inner).merge(
        Router::new()
            .route("/session/{token}", get(bootstrap))
            .with_state(a.clone()),
    );
    if persistent_test {
        app = app.route(
            "/app/",
            get(move |r: Request| {
                let a = a.clone();
                async move {
                    if !origin(&a, &r) {
                        return StatusCode::FORBIDDEN.into_response();
                    }
                    (
                        StatusCode::SEE_OTHER,
                        [
                            (header::LOCATION, format!("{}/", a.prefix)),
                            (header::CACHE_CONTROL, "no-store".to_owned()),
                            (header::REFERRER_POLICY, "no-referrer".to_owned()),
                            (header::SET_COOKIE, access_cookie(&a)),
                        ],
                    )
                        .into_response()
                }
            }),
        );
    }
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
    let host = crate::single_header(r.headers(), header::HOST);
    (host == Some(a.host.as_str())
        || (a.persistent_test || a.development_localhost) && host == Some("localhost:8016"))
        && (!r.headers().contains_key(header::ORIGIN)
            || crate::single_header(r.headers(), header::ORIGIN)
                == host.map(|h| format!("{}://{h}", a.scheme)).as_deref())
}
fn access_cookie(a: &Access) -> String {
    format!(
        "{}={}; HttpOnly; SameSite=Strict; Path={}{}{}",
        if a.persistent_test {
            "seiya_test_access"
        } else {
            "seiya_access"
        },
        a.key,
        if a.persistent_test {
            "/app/".to_owned()
        } else {
            format!("{}/", a.prefix)
        },
        if a.persistent_test {
            "; Max-Age=315360000"
        } else if a.local_app {
            ""
        } else {
            "; Max-Age=1800"
        },
        if a.scheme == "https" { "; Secure" } else { "" },
    )
}
async fn cookie(State(a): State<Arc<Access>>, mut r: Request, next: Next) -> Response {
    if !origin(&a, &r) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !r.headers().contains_key(header::AUTHORIZATION) && valid_cookie(&a, &r) {
        r.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", a.key)).unwrap(),
        );
    }
    next.run(r).await
}
fn valid_cookie(a: &Access, r: &Request) -> bool {
    let name = if a.persistent_test {
        "seiya_test_access="
    } else {
        "seiya_access="
    };
    let Some(raw) = crate::single_header(r.headers(), header::COOKIE) else {
        return false;
    };
    let values: Vec<_> = raw
        .split(';')
        .map(str::trim)
        .filter_map(|s| s.strip_prefix(name))
        .collect();
    values.len() == 1 && same(values[0], &a.key)
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
    if !a.reusable_test_link && used.swap(true, Ordering::AcqRel) {
        return StatusCode::FORBIDDEN.into_response();
    }
    (
        StatusCode::SEE_OTHER,
        [
            (
                header::LOCATION,
                if a.persistent_test {
                    "/app/".to_owned()
                } else {
                    format!("{}/", a.prefix)
                },
            ),
            (header::CACHE_CONTROL, "no-store".into()),
            (header::REFERRER_POLICY, "no-referrer".into()),
            (header::SET_COOKIE, access_cookie(&a)),
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
        local_app: scheme == "http" && address.ip().is_loopback(),
        development_localhost: cfg!(debug_assertions)
            && scheme == "http"
            && address == SocketAddr::from(([127, 0, 0, 1], 8016)),
        persistent_test: false,
        reusable_test_link: scheme == "http"
            && address.ip() == std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
            && address.port() == 8016,
        scheme,
        host: address.to_string(),
        key: token(),
        bootstrap: (0..count)
            .map(|_| (token(), AtomicBool::new(false)))
            .collect(),
        prefix: format!(
            "/{}/{}",
            if scheme == "http" && address.ip().is_loopback() {
                "app"
            } else {
                "trial"
            },
            uuid::Uuid::new_v4().simple()
        ),
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
    #[test]
    fn test_port_key_is_private_and_persists_without_repairing_bad_files() {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("seiya-test-key-{}", uuid::Uuid::new_v4()));
        let first = test_key_at(&root).unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(test_key_at(&root).unwrap(), first);
        let path = root.join("test-chat-8016.key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(test_key_at(&root).is_err());
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&root).unwrap();
    }
    use crate::{
        jobs::Demo,
        prepared_web::FOREGROUND,
        runtime::{Backend, RuntimeConfig, RuntimeFuture, RuntimeTransport},
    };
    use axum::body::Body;
    use tower::ServiceExt;

    #[tokio::test]
    async fn mounted_chat_localhost_alias_is_exactly_debug_test_port() {
        for port in [8015, 8016] {
            let address: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
            let (router, link) = app(address, PreparedWeb::diagnostics_only().unwrap()).unwrap();
            let path = reqwest::Url::parse(&link).unwrap().path().to_owned();
            let req = |host: &str, origin: Option<&str>, path: &str, cookie: Option<&str>| {
                let mut r = axum::http::Request::get(path).header(header::HOST, host);
                if let Some(v) = origin {
                    r = r.header(header::ORIGIN, v);
                }
                if let Some(v) = cookie {
                    r = r.header(header::COOKIE, v);
                }
                r.body(Body::empty()).unwrap()
            };
            let allow = cfg!(debug_assertions) && port == 8016;
            let host = format!("localhost:{port}");
            let origin = format!("http://localhost:{port}");
            let first = router
                .clone()
                .oneshot(req(&host, Some(&origin), &path, None))
                .await
                .unwrap();
            assert_eq!(first.status(), if allow { 303 } else { 403 });
            if allow {
                let cookie = first.headers()[header::SET_COOKIE]
                    .to_str()
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap();
                let home = first.headers()[header::LOCATION].to_str().unwrap();
                assert_eq!(
                    router
                        .clone()
                        .oneshot(req(&host, Some(&origin), home, Some(cookie)))
                        .await
                        .unwrap()
                        .status(),
                    200
                );
                assert_eq!(
                    router
                        .clone()
                        .oneshot(req(
                            &host,
                            Some("http://127.0.0.1:8016"),
                            home,
                            Some(cookie)
                        ))
                        .await
                        .unwrap()
                        .status(),
                    403
                );
                assert_eq!(
                    router
                        .clone()
                        .oneshot(req("otherhost:8016", None, home, Some(cookie)))
                        .await
                        .unwrap()
                        .status(),
                    403
                );
            }
            assert_eq!(
                router
                    .clone()
                    .oneshot(req("localhost:8017", None, &path, None))
                    .await
                    .unwrap()
                    .status(),
                403
            );
            assert_eq!(
                router
                    .oneshot(req(&format!("127.0.0.1:{port}"), None, &path, None))
                    .await
                    .unwrap()
                    .status(),
                303
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn test_port_link_reopens_without_timer_but_restarts_revoke_it() {
        let address: SocketAddr = "127.0.0.1:8016".parse().unwrap();
        for chat in [false, true] {
            let (router, link, home) = if chat {
                let (router, url) = app(address, PreparedWeb::diagnostics_only().unwrap()).unwrap();
                (router, url, None)
            } else {
                let (router, url, _) =
                    desktop_shell(address, Router::new().route("/", get(|| async { "setup" })));
                (router, url, Some("setup"))
            };
            let path = reqwest::Url::parse(&link).unwrap().path().to_owned();
            let request = |path: &str, cookie: Option<&str>, host: &str, origin: Option<&str>| {
                let mut req = axum::http::Request::get(path).header(header::HOST, host);
                if let Some(value) = cookie {
                    req = req.header(header::COOKIE, value);
                }
                if let Some(value) = origin {
                    req = req.header(header::ORIGIN, value);
                }
                req.body(Body::empty()).unwrap()
            };
            for _ in 0..2 {
                let response = router
                    .clone()
                    .oneshot(request(&path, None, "127.0.0.1:8016", None))
                    .await
                    .unwrap();
                assert_eq!(response.status(), 303);
                let raw = response.headers()[header::SET_COOKIE].to_str().unwrap();
                assert!(raw.contains("HttpOnly; SameSite=Strict"));
                assert!(!raw.contains("Max-Age") && !raw.contains("Expires"));
                let cookie = raw.split(';').next().unwrap();
                let location = response.headers()[header::LOCATION].to_str().unwrap();
                if home.is_some() {
                    assert!(location.starts_with("/app/"));
                }
                let response = router
                    .clone()
                    .oneshot(request(location, Some(cookie), "127.0.0.1:8016", None))
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
            }
            tokio::time::advance(std::time::Duration::from_secs(48 * 3600)).await;
            assert_eq!(
                router
                    .clone()
                    .oneshot(request(&path, None, "127.0.0.1:8016", None))
                    .await
                    .unwrap()
                    .status(),
                303
            );
            assert_eq!(
                router
                    .clone()
                    .oneshot(request(&path, None, "127.0.0.1:8015", None))
                    .await
                    .unwrap()
                    .status(),
                403
            );
            assert_eq!(
                router
                    .clone()
                    .oneshot(request(
                        &path,
                        None,
                        "127.0.0.1:8016",
                        Some("http://127.0.0.1:8015")
                    ))
                    .await
                    .unwrap()
                    .status(),
                403
            );
            assert_eq!(
                router
                    .clone()
                    .oneshot(request("/session/invalid", None, "127.0.0.1:8016", None))
                    .await
                    .unwrap()
                    .status(),
                403
            );
            let restarted = if chat {
                app(address, PreparedWeb::diagnostics_only().unwrap())
                    .unwrap()
                    .0
            } else {
                desktop_shell(address, Router::new().route("/", get(|| async { "setup" }))).0
            };
            assert_eq!(
                restarted
                    .oneshot(request(&path, None, "127.0.0.1:8016", None))
                    .await
                    .unwrap()
                    .status(),
                403
            );
        }
    }
    #[tokio::test(start_paused = true)]
    async fn local_app_cookie_has_no_timer_and_restart_requires_fresh_bootstrap() {
        let address = "127.0.0.1:8015".parse().unwrap();
        let inner = Router::new().route("/", get(|| async { "setup" }));
        let (app, link, _) = desktop_shell(address, inner.clone());
        let req = |path: &str, cookie: &str| {
            axum::http::Request::get(path)
                .header(header::HOST, "127.0.0.1:8015")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap()
        };
        let response = app
            .clone()
            .oneshot(req(reqwest::Url::parse(&link).unwrap().path(), ""))
            .await
            .unwrap();
        assert_eq!(response.status(), 303);
        let raw = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(!raw.contains("Max-Age"));
        assert!(!raw.contains("Expires"));
        assert!(raw.contains("HttpOnly; SameSite=Strict"));
        assert!(!raw.contains("trial"));
        let cookie = raw.split(';').next().unwrap().to_owned();
        let path = response.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(path.starts_with("/app/"));
        tokio::time::advance(std::time::Duration::from_secs(24 * 3600)).await;
        assert_eq!(
            app.clone()
                .oneshot(req(&path, &cookie))
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            app.oneshot(req(reqwest::Url::parse(&link).unwrap().path(), ""))
                .await
                .unwrap()
                .status(),
            403
        );
        let (restarted, fresh, _) = desktop_shell(address, inner);
        assert_eq!(
            restarted
                .clone()
                .oneshot(req(&path, &cookie))
                .await
                .unwrap()
                .status(),
            404
        );
        let fresh = restarted
            .clone()
            .oneshot(req(reqwest::Url::parse(&fresh).unwrap().path(), ""))
            .await
            .unwrap();
        let fresh_path = fresh.headers()[header::LOCATION].to_str().unwrap();
        assert_eq!(
            restarted
                .oneshot(req(fresh_path, &cookie))
                .await
                .unwrap()
                .status(),
            401
        );
    }
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
        assert!(!response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age"));
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
        assert!(r.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=1800; Secure"));
        assert!(r.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .starts_with("/trial/"));
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
