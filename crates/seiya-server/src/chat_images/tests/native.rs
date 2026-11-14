use super::*;
use crate::{
    managed_residency::{Driver, Phase, Role},
    runtime::TransportError,
};

struct NativeText {
    driver: Arc<Driver>,
    events: Arc<Mutex<Vec<&'static str>>>,
}
impl RuntimeTransport for NativeText {
    fn complete(&self, input: Value) -> RuntimeFuture<'_> {
        Box::pin(async move {
            crate::pair_trial::request(input.clone(), prepared_web::FOREGROUND)
                .map_err(|_| TransportError::InputRejected)?;
            let turn = self
                .driver
                .acquire(Role::Brain)
                .await
                .map_err(|_| TransportError::Unavailable)?;
            turn.advance(Phase::Loading).unwrap();
            self.events.lock().unwrap().push("brain_loading");
            turn.advance(Phase::Running).unwrap();
            self.events.lock().unwrap().push("brain_running");
            turn.advance(Phase::Unloading).unwrap();
            turn.confirm_cleanup();
            self.events.lock().unwrap().push("brain_closed");
            Ok(
                json!({"model":prepared_web::FOREGROUND,"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"native-image","type":"function","function":{"name":"image_generate","arguments":json!({"prompt":"A synthetic blue horse"}).to_string()}}]},"finish_reason":"tool_calls"}]}),
            )
        })
    }
}
struct NativeImage {
    driver: Arc<Driver>,
    events: Arc<Mutex<Vec<&'static str>>>,
}
impl Executor for NativeImage {
    fn available(&self) -> bool {
        true
    }
    fn generate(
        &self,
        _: ImageRequest,
        _: watch::Receiver<bool>,
        _: watch::Sender<ImageStage>,
    ) -> image_jobs::Work {
        let driver = self.driver.clone();
        let events = self.events.clone();
        Box::pin(async move {
            let turn = driver
                .acquire(Role::Image)
                .await
                .map_err(|_| image_jobs::Failure::Blocked)?;
            turn.advance(Phase::Loading).unwrap();
            events.lock().unwrap().push("image_loading");
            turn.advance(Phase::Running).unwrap();
            turn.advance(Phase::Unloading).unwrap();
            turn.confirm_cleanup();
            events.lock().unwrap().push("image_closed");
            Ok(crate::local_models::fixture_png())
        })
    }
}
#[tokio::test]
async fn native_chat_closes_text_before_image_and_keeps_3584_byte_admission() {
    for multiagent in [false, true] {
        check_native_chat(multiagent).await;
    }
}
async fn check_native_chat(multiagent: bool) {
    let driver = Driver::new();
    let events = Arc::new(Mutex::new(vec![]));
    let images = ImageJobs::new(
        Arc::new(NativeImage {
            driver: driver.clone(),
            events: events.clone(),
        }),
        Arc::new(Allowed),
    );
    let config = RuntimeConfig {
        id: "native-fixture".into(),
        model: prepared_web::FOREGROUND.into(),
        backend: Backend::Mlx,
        base_url: "http://127.0.0.1:49399/v1".into(),
        timeout_seconds: 120,
        max_output_tokens: 256,
    };
    let transport = Arc::new(NativeText {
        driver: driver.clone(),
        events: events.clone(),
    });
    let state = if multiagent {
        PreparedWeb::with_host(config, transport, Arc::new(NoJobs), Arc::new(Allowed))
    } else {
        PreparedWeb::solo(config, transport, Arc::new(Allowed))
    }
    .unwrap()
    .with_images(images)
    .unwrap()
    .with_chat_images()
    .unwrap();
    state.mark_real_trial();
    let app = prepared_web::app(
        "127.0.0.1:8015".parse().unwrap(),
        "synthetic-image-chat-key".into(),
        state,
    )
    .unwrap();
    let session = data(
        http(
            &app,
            "/api/images/v1",
            json!({"operation":"open","conversation_id":uuid::Uuid::new_v4().to_string()}),
        )
        .await,
    )
    .await["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut first = request(&session, &uuid::Uuid::new_v4().to_string());
    let job_session = if multiagent {
        data(http(&app, "/api/jobs", json!({"operation":"session"})).await).await["session"]
            .as_str()
            .unwrap()
            .to_owned()
    } else {
        String::new()
    };
    if multiagent {
        first["job_session"] = json!(job_session);
        first["job_request_id"] = json!(uuid::Uuid::new_v4().to_string());
    }
    let response = http(&app, "/api/chat", first).await;
    assert_eq!(response.status(), 200);
    let response = data(response).await;
    let id = &response["image_job"]["job_id"];
    let mut completed = false;
    for _ in 0..100 {
        let status = data(
            http(
                &app,
                "/api/images/v1",
                json!({"operation":"status","session":session,"job_id":id}),
            )
            .await,
        )
        .await;
        if status["phase"] == "completed" {
            completed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(completed);
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "brain_loading",
            "brain_running",
            "brain_closed",
            "image_loading",
            "image_closed"
        ]
    );
    assert!(driver.snapshot().owned_cleanup_confirmed);
    let mut too_large = request(&session, &uuid::Uuid::new_v4().to_string());
    if multiagent {
        too_large["job_session"] = json!(job_session);
        too_large["job_request_id"] = json!(uuid::Uuid::new_v4().to_string());
    }
    too_large["messages"][0]["content"] = json!("x".repeat(4000));
    let response = http(&app, "/api/chat", too_large).await;
    assert_eq!(response.status(), 400);
    assert!(data(response)
        .await
        .to_string()
        .contains("context_exhausted"));
    assert_eq!(events.lock().unwrap().len(), 5);
}
