//! Static in-page settings views. No new endpoint, frame permission or loader.
pub(crate) fn valid_home(home: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(home) else {
        return false;
    };
    url.as_str() == home
        && url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && matches!(url.port(), Some(8015 | 8016))
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url
            .path()
            .strip_prefix("/app/")
            .and_then(|p| p.strip_suffix('/'))
            .is_some_and(|id| id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit()))
}

pub(crate) fn desktop_port(home: &str) -> Result<u16, &'static str> {
    if !valid_home(home) {
        return Err("invalid_desktop_setup_home");
    }
    reqwest::Url::parse(home)
        .ok()
        .and_then(|url| url.port())
        .ok_or("invalid_desktop_setup_home")
}

pub(crate) fn render(home: Option<&str>) -> String {
    let home = home.filter(|home| valid_home(home));
    let fragment = home.map_or("", |_| include_str!("desktop_setup.html"));
    // Reuse the audited setup handlers, but not the standalone shell's composer
    // lock/status overrides. Setup requests keep their original authenticated base.
    let setup = crate::desktop_setup::navigation_script()
        .replace(
            "new URL('api/' + path, location.href)",
            "new URL('api/' + path, seiyaModelsBase.href)",
        )
        .replace(
            "const options = {credentials:'same-origin', cache:'no-store'};",
            "const options = {credentials:'same-origin', cache:'no-store', redirect:'error'};",
        )
        .replace(
            "  el('prompt').disabled = true; el('send').disabled = true;",
            "",
        )
        .replace(
            "  el('form').addEventListener('submit', e => e.preventDefault());",
            "",
        )
        .replace(
            "  el('runtimeStatus').textContent = 'Configura Seiya antes de conversar.';",
            "",
        )
        .replace(
            "      el('runtimeStatus').textContent = 'Seiya: ' + v.phase;",
            "",
        );
    format!(
        "<div id=\"seiyaModelsSource\" hidden>{fragment}</div><script>\nconst seiyaModelsHome={};\n{}\nlet seiyaModelsInitialized=false;\nfunction seiyaInitializeModels(){{if(seiyaModelsInitialized)return;if(!seiyaModelsBase)throw Error('Configuración de modelos no disponible.');seiyaModelsInitialized=true;\n{setup}\n{}\nseiyaMountUpdate(seiyaModelsBase.href);\n}}\n{}\n</script>",
        serde_json::to_string(&home).unwrap(),
        include_str!("update_ui.js"),
        include_str!("desktop_setup_feedback.js"),
        include_str!("settings_menu_ui.js"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_settings_never_accept_bootstrap_external_or_noncanonical_targets() {
        assert!(valid_home(&format!(
            "http://127.0.0.1:8015/app/{}/",
            "a".repeat(32)
        )));
        assert_eq!(
            desktop_port(&format!("http://127.0.0.1:8016/app/{}/", "a".repeat(32))),
            Ok(8016)
        );
        assert_eq!(
            desktop_port(&format!("http://127.0.0.1:8015/app/{}/", "a".repeat(32))),
            Ok(8015)
        );
        for home in [
            "http://127.0.0.1:8017/app/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
            "http://127.0.0.1:8016/app/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/?wrong=1",
            "http://127.0.0.1:8015/session/token",
            "http://127.0.0.1:8015/app/not-an-id/",
            "http://localhost:8015/app/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
            "https://external.invalid/app/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
            "http://127.0.0.1:8015/app/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/?key=secret",
            "http://127.0.0.1:8015/app/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/#fragment",
            "http://user@127.0.0.1:8015/app/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
            "</script><script>untrusted()</script>",
        ] {
            assert!(!valid_home(home));
        }
    }
    #[test]
    fn embedded_setup_is_static_lazy_and_cannot_disable_or_navigate_chat_on_load() {
        let html = render(Some(&format!(
            "http://127.0.0.1:8015/app/{}/",
            "b".repeat(32)
        )));
        assert!(html.contains("function seiyaInitializeModels()"));
        assert!(html.contains("new URL('api/' + path, seiyaModelsBase.href)"));
        assert!(html.contains("redirect:'error'"));
        assert!(!html.contains("el('prompt').disabled = true"));
        assert!(!html.contains("el('runtimeStatus').textContent"));
        assert!(!html.contains("el('setupEnter').click()"));
        assert!(!html.contains("<iframe"));
        let rejected = render(Some("</script><script>untrusted()</script>"));
        assert!(rejected.contains("const seiyaModelsHome=null"));
        assert!(!rejected.contains("untrusted()"));
        assert!(!rejected.contains("id=\"setup\""));
        let test_port = render(Some(&format!(
            "http://127.0.0.1:8016/app/{}/",
            "c".repeat(32)
        )));
        assert!(test_port.contains("const seiyaModelsHome=\"http://127.0.0.1:8016/app/"));
        assert!(test_port.contains("id=\"setup\""));
    }
}
