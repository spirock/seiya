//! macOS-only adapter. Constructing/attaching never grants generation permission.
use super::{Executor, Failure, Work};
use seiya_contracts::image_job::{ImageRequest, ImageStage};
use seiya_observe::image::job::{self, Permission, RuntimeFiles};
use std::sync::Arc;
use tokio::sync::watch;
pub struct NativeImage {
    files: RuntimeFiles,
    permission: Arc<dyn Permission>,
    environment: job::Environment,
}
impl NativeImage {
    pub fn new(files: RuntimeFiles, permission: Arc<dyn Permission>) -> Self {
        Self {
            files,
            permission,
            environment: job::Environment::legacy(),
        }
    }
    pub fn with_environment(
        files: RuntimeFiles,
        permission: Arc<dyn Permission>,
        environment: job::Environment,
    ) -> Self {
        Self {
            files,
            permission,
            environment,
        }
    }
}
impl Executor for NativeImage {
    fn available(&self) -> bool {
        self.permission.enabled()
    }
    fn generate(
        &self,
        request: ImageRequest,
        cancel: watch::Receiver<bool>,
        stage: watch::Sender<ImageStage>,
    ) -> Work {
        let files = self.files.clone();
        let permission = self.permission.clone();
        let environment = self.environment.clone();
        Box::pin(async move {
            job::generate_with(files, request, permission, cancel, stage, environment)
                .await
                .map_err(|e| match e {
                    job::Failure::Blocked => Failure::Blocked,
                    job::Failure::Cancelled => Failure::Cancelled,
                    job::Failure::Failed => Failure::Failed,
                    job::Failure::Uncertain => Failure::Uncertain,
                })
        })
    }
}
