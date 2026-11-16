//! Fixed-port UI preview. All profile state is ephemeral, no filesystem/runtime.
use super::*;

#[derive(Default)]
struct Memory(std::sync::Mutex<(u64, Draft)>);
impl Adapter for Memory {
    fn available(&self) -> bool {
        true // review/save UI is available, not inference (supports always false).
    }
    fn supports(&self, _: &Draft) -> bool {
        false
    }
    fn filesystem_checked(&self) -> bool {
        false
    }
    fn effects(&self) -> String {
        "VISTA PREVIA: guardar cambia solo memoria temporal de esta prueba; no escribe perfiles, valida rutas ni inicia modelos.".into()
    }
    fn snapshot(&self) -> Result<Snapshot, String> {
        let state = self.0.lock().unwrap();
        Ok(Snapshot {
            draft: state.1.clone(),
            baseline: state.0.to_le_bytes().to_vec(),
            warning: Some(self.effects()),
        })
    }
    fn review(&self, draft: Draft, baseline: Vec<u8>) -> Result<Approval, String> {
        if baseline != self.0.lock().unwrap().0.to_le_bytes() {
            return Err("profile_changed_no_overwrite".into());
        }
        Ok(Approval {
            profile: vec![],
            draft,
            baseline,
        })
    }
    fn save(&self, approval: &Approval) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        if approval.baseline != state.0.to_le_bytes() {
            return Err("profile_changed_no_overwrite".into());
        }
        state.0 += 1;
        state.1 = approval.draft.clone();
        Ok(())
    }
    fn pick(&self, _: String) -> Work<'_, String> {
        Box::pin(async { Err("preview_no_filesystem_or_runtime".into()) })
    }
    fn start(&self, _: Vec<u8>, _: bool, _: watch::Receiver<bool>, _: String) -> Work<'_, Engine> {
        Box::pin(async { Err("preview_no_filesystem_or_runtime".into()) })
    }
}

pub(super) async fn page() -> Html<String> {
    let Html(html) = super::page().await;
    Html(html.replace(
        "</header>",
        "</header><p role=\"status\" style=\"padding:12px;background:#172744;color:#a5f3fc\">VISTA PREVIA · Puerto8016 · Sin inferencia. Configuración solo en memoria; no modifica la app instalada.</p>",
    ).replace(" · adaptador macOS disponible", " · vista previa sin inferencia"))
}

pub(super) async fn launch() -> Result<(), String> {
    let address = "127.0.0.1:8016".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|_| "preview_port_8016_busy_no_adoption")?;
    let (tx, mut rx) = watch::channel(false);
    let state = Setup::new(Arc::new(Memory::default()), tx);
    let (app, link, _) = application_at(state, address);
    println!("Seiya VISTA PREVIA sin modelos: {link}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! { _=rx.changed()=>{}, _=termination()=>{} }
        })
        .await
        .map_err(|_| "preview_server_failed".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{header, Request},
    };
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn preview_is_authenticated_on_8016_and_never_starts_or_writes_profiles() {
        let memory = Arc::new(Memory::default());
        let snapshot = memory.snapshot().unwrap();
        let approval = memory.review(snapshot.draft, snapshot.baseline).unwrap();
        assert!(!memory.supports(&approval.draft));
        assert!(!memory.filesystem_checked());
        memory.save(&approval).unwrap();
        assert!(memory.save(&approval).is_err());
        assert!(memory.pick("llm_binary".into()).await.is_err());
        let (tx, _) = watch::channel(false);
        let state = Setup::new(memory, tx);
        let (app, link, _) = application_at(state.clone(), "127.0.0.1:8016".parse().unwrap());
        assert!(link.starts_with("http://127.0.0.1:8016/session/"));
        let path = reqwest::Url::parse(&link).unwrap().path().to_owned();
        let response = app
            .clone()
            .oneshot(
                Request::get(&path)
                    .header(header::HOST, "127.0.0.1:8015")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = app
            .clone()
            .oneshot(
                Request::get(&path)
                    .header(header::HOST, "127.0.0.1:8016")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let home = response.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .to_owned();
        let response = app
            .clone()
            .oneshot(
                Request::get(&home)
                    .header(header::HOST, "127.0.0.1:8016")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = app
            .oneshot(
                Request::get(&home)
                    .header(header::HOST, "127.0.0.1:8016")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");
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
        assert!(html.contains("VISTA PREVIA · Puerto8016"));
        let response = review(
            State(state.clone()),
            Json(Review {
                draft: Draft::default(),
                baseline: 1_u64.to_le_bytes().to_vec(),
            }),
        )
        .await;
        let summary: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(summary["executable"], false);
        let response = confirm(
            State(state.clone()),
            Json(Confirm {
                approval: summary["approval"].as_str().unwrap().into(),
                start: true,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(state.control.lock().await.stop.is_none());
    }
}
