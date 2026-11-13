use seiya_server::jobs::*;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;
use uuid::Uuid;

struct Fast;
impl Executor for Fast {
    fn draft(&self, _: String, _: watch::Receiver<bool>) -> Work {
        Box::pin(async { Ok("print('draft')".into()) })
    }
}
struct Stuck;

struct Panics;
impl Executor for Panics {
    fn draft(&self, _: String, _: watch::Receiver<bool>) -> Work {
        Box::pin(async { panic!("injected synthetic executor failure") })
    }
}

#[tokio::test]
async fn panicked_executor_closes_pool_before_releasing_its_slot() {
    let jobs = Jobs::new(
        Arc::new(Panics),
        Arc::new(DemoAdmission),
        Duration::from_secs(10),
    )
    .unwrap();
    let owner = session(&jobs);
    let a = submit(&jobs, &owner);
    let b = submit(&jobs, &owner);
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    assert_eq!(
        status(&jobs, &owner, a["job_id"].as_str().unwrap())["phase"],
        "quarantined"
    );
    assert_eq!(
        status(&jobs, &owner, b["job_id"].as_str().unwrap())["phase"],
        "blocked"
    );
    assert_eq!(
        jobs.control(Command::List { session: owner }).unwrap()["foreground_allowed"],
        false
    );
}
impl Executor for Stuck {
    fn draft(&self, _: String, _: watch::Receiver<bool>) -> Work {
        Box::pin(std::future::pending())
    }
}

struct Late(Arc<tokio::sync::Notify>);
impl Executor for Late {
    fn draft(&self, _: String, _: watch::Receiver<bool>) -> Work {
        let ready = self.0.clone();
        Box::pin(async move {
            ready.notified().await;
            Ok("late draft".into())
        })
    }
}

#[tokio::test]
async fn cancelled_active_job_never_publishes_late_artifact() {
    let ready = Arc::new(tokio::sync::Notify::new());
    let jobs = Jobs::new(
        Arc::new(Late(ready.clone())),
        Arc::new(DemoAdmission),
        Duration::from_secs(10),
    )
    .unwrap();
    let owner = session(&jobs);
    let first = submit(&jobs, &owner);
    let id = first["job_id"].as_str().unwrap();
    tokio::task::yield_now().await;
    jobs.control(Command::Cancel {
        session: owner.clone(),
        job_id: id.into(),
    })
    .unwrap();
    ready.notify_one();
    tokio::task::yield_now().await;
    assert_eq!(status(&jobs, &owner, id)["phase"], "cancelled");
    assert_eq!(
        jobs.control(Command::Artifact {
            session: owner,
            job_id: id.into()
        }),
        Err(Error::NotReady)
    );
}

#[tokio::test]
async fn jobs_http_is_opt_in_and_guarded_before_command_parsing() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let address = "127.0.0.1:49200".parse().unwrap();
    let jobs = Jobs::new(Arc::new(Fast), Arc::new(Blocked), Duration::from_secs(10)).unwrap();
    let app = seiya_server::app_with_jobs(address, "synthetic-key".into(), jobs).unwrap();
    for (host, auth, payload, expected) in [
        (
            "127.0.0.1:49200",
            "Bearer synthetic-key",
            r#"{"operation":"session"}"#,
            200,
        ),
        (
            "evil.example",
            "Bearer synthetic-key",
            r#"{"operation":"session"}"#,
            403,
        ),
        (
            "127.0.0.1:49200",
            "Bearer wrong",
            r#"{"operation":"session"}"#,
            401,
        ),
        (
            "127.0.0.1:49200",
            "Bearer synthetic-key",
            r#"{"operation":"session","admit":true}"#,
            400,
        ),
    ] {
        let request = Request::post("/internal/jobs/v1")
            .header("Host", host)
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .header("Content-Length", payload.len())
            .body(Body::from(payload))
            .unwrap();
        assert_eq!(
            app.clone()
                .oneshot(request)
                .await
                .unwrap()
                .status()
                .as_u16(),
            expected
        );
    }
    let regular = seiya_server::app(address, None).unwrap();
    let request = Request::post("/internal/jobs/v1")
        .header("Host", "127.0.0.1:49200")
        .body(Body::empty())
        .unwrap();
    assert_eq!(regular.oneshot(request).await.unwrap().status(), 404);
}

#[test]
fn hardware_reservations_can_allow_more_workers_but_are_not_admission() {
    let gib = 1u64 << 30;
    assert_eq!(
        planned_parallelism(16 * gib, 4 * gib, 4 * gib, 8 * gib, 4),
        Ok(1)
    );
    assert_eq!(
        planned_parallelism(32 * gib, 4 * gib, 4 * gib, 8 * gib, 4),
        Ok(3)
    );
    assert_eq!(
        planned_parallelism(32 * gib, 4 * gib, 4 * gib, 8 * gib, 2),
        Ok(2)
    );
    assert_eq!(
        planned_parallelism(8 * gib, 4 * gib, 4 * gib, 8 * gib, 4),
        Ok(0)
    );
    assert_eq!(
        planned_parallelism(32 * gib, 4 * gib, 4 * gib, 0, 4),
        Err(Error::Invalid)
    );
}

#[test]
fn draft_transport_is_local_fixed_and_does_not_start_models() {
    assert!(HttpDraft::new(
        "http://127.0.0.1:11434/v1/chat/completions".into(),
        "qwen3.5:9b-mlx".into()
    )
    .is_ok());
    for url in [
        "http://localhost:11434/v1/chat/completions",
        "https://example.org/v1/chat/completions",
        "http://127.0.0.1:11434/v1/chat/completions?model=x",
    ] {
        assert!(HttpDraft::new(url.into(), "qwen".into()).is_err());
    }
}

#[tokio::test]
async fn configured_pool_runs_two_auxiliaries_and_queues_the_third() {
    let jobs = Jobs::with_parallelism(
        Arc::new(Stuck),
        Arc::new(DemoAdmission),
        Duration::from_secs(10),
        2,
    )
    .unwrap();
    let owner = session(&jobs);
    let a = submit(&jobs, &owner);
    let b = submit(&jobs, &owner);
    let c = submit(&jobs, &owner);
    tokio::task::yield_now().await;
    assert_eq!(
        status(&jobs, &owner, a["job_id"].as_str().unwrap())["phase"],
        "generating"
    );
    assert_eq!(
        status(&jobs, &owner, b["job_id"].as_str().unwrap())["phase"],
        "generating"
    );
    assert_eq!(
        status(&jobs, &owner, c["job_id"].as_str().unwrap())["phase"],
        "queued"
    );
}
fn session(jobs: &Arc<Jobs>) -> String {
    jobs.control(Command::Session {}).unwrap()["session"]
        .as_str()
        .unwrap()
        .into()
}
fn submit(jobs: &Arc<Jobs>, owner: &str) -> Value {
    jobs.control(Command::Submit {
        session: owner.into(),
        request_id: Uuid::new_v4().to_string(),
        prompt: "write a Python draft".into(),
    })
    .unwrap()
}
fn status(jobs: &Arc<Jobs>, owner: &str, id: &str) -> Value {
    jobs.control(Command::Status {
        session: owner.into(),
        job_id: id.into(),
    })
    .unwrap()
}

#[tokio::test]
async fn artifact_is_bounded_untrusted_and_owner_bound() {
    let jobs = Jobs::new(
        Arc::new(Fast),
        Arc::new(DemoAdmission),
        Duration::from_secs(10),
    )
    .unwrap();
    let owner = session(&jobs);
    let other = session(&jobs);
    let job = submit(&jobs, &owner);
    let id = job["job_id"].as_str().unwrap();
    assert_eq!(
        jobs.control(Command::Status {
            session: other,
            job_id: id.into()
        }),
        Err(Error::Unknown)
    );
    tokio::task::yield_now().await;
    assert_eq!(status(&jobs, &owner, id)["phase"], "completed");
    let artifact = jobs
        .control(Command::Artifact {
            session: owner,
            job_id: id.into(),
        })
        .unwrap();
    assert_eq!(artifact["untrusted"], true);
    assert_eq!(artifact["tests_executed"], false);
    assert_eq!(artifact["workspace_written"], false);
}

#[tokio::test]
async fn idempotency_and_limits_never_duplicate_or_overwrite() {
    let jobs = Jobs::new(
        Arc::new(Stuck),
        Arc::new(DemoAdmission),
        Duration::from_secs(10),
    )
    .unwrap();
    let owner = session(&jobs);
    let request = Uuid::new_v4().to_string();
    let frame = || Command::Submit {
        session: owner.clone(),
        request_id: request.clone(),
        prompt: "draft".into(),
    };
    let first = jobs.control(frame()).unwrap();
    assert_eq!(first, jobs.control(frame()).unwrap());
    assert_eq!(
        jobs.control(Command::Submit {
            session: owner.clone(),
            request_id: request,
            prompt: "changed".into()
        }),
        Err(Error::Invalid)
    );
    for _ in 0..4 {
        submit(&jobs, &owner);
    }
    assert_eq!(
        jobs.control(Command::Submit {
            session: owner,
            request_id: Uuid::new_v4().to_string(),
            prompt: "sixth".into()
        }),
        Err(Error::Capacity)
    );
    assert!(
        serde_json::from_value::<Command>(json!({"operation":"session","admit":true})).is_err()
    );
}

#[tokio::test]
async fn denied_gate_never_runs_executor_and_queue_cancel_is_terminal() {
    let jobs = Jobs::new(Arc::new(Fast), Arc::new(Blocked), Duration::from_secs(10)).unwrap();
    let owner = session(&jobs);
    let first = submit(&jobs, &owner);
    let id = first["job_id"].as_str().unwrap();
    tokio::task::yield_now().await;
    assert_eq!(status(&jobs, &owner, id)["phase"], "blocked");
    let next = submit(&jobs, &owner);
    let id = next["job_id"].as_str().unwrap();
    jobs.control(Command::Cancel {
        session: owner.clone(),
        job_id: id.into(),
    })
    .unwrap();
    tokio::task::yield_now().await;
    assert_eq!(status(&jobs, &owner, id)["phase"], "cancelled");
}

#[tokio::test(start_paused = true)]
async fn active_cancel_does_not_fake_completion_and_timeout_quarantines() {
    let jobs = Jobs::new(
        Arc::new(Stuck),
        Arc::new(DemoAdmission),
        Duration::from_secs(10),
    )
    .unwrap();
    let owner = session(&jobs);
    let first = submit(&jobs, &owner);
    let id = first["job_id"].as_str().unwrap();
    tokio::task::yield_now().await;
    assert_eq!(
        jobs.control(Command::Cancel {
            session: owner.clone(),
            job_id: id.into()
        })
        .unwrap()["phase"],
        "cancelling"
    );
    tokio::time::advance(Duration::from_secs(11)).await;
    tokio::task::yield_now().await;
    assert_eq!(status(&jobs, &owner, id)["phase"], "quarantined");
    assert_eq!(
        jobs.control(Command::List { session: owner }).unwrap()["foreground_allowed"],
        false
    );
}
