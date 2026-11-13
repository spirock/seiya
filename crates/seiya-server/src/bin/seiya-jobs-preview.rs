//! Explicit synthetic jobs server. Never imports/loads any model.
use seiya_server::{
    app_with_jobs,
    jobs::{Demo, DemoAdmission, Jobs},
};
use std::{sync::Arc, time::Duration};
#[tokio::main]
async fn main() {
    let key = std::env::var("SEIYA_JOBS_PREVIEW_KEY").expect("private preview key required");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let jobs = Jobs::new(
        Arc::new(Demo),
        Arc::new(DemoAdmission),
        Duration::from_secs(60),
    )
    .unwrap();
    let app = app_with_jobs(address, key, jobs).unwrap();
    println!("http://{address}/internal/jobs/v1");
    axum::serve(listener, app).await.unwrap();
}
