//! Operator limits/observations only. No loader, grant, reset, process list or shell.
use crate::job_store::Store;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub version: u32,
    pub chat_warning_ms: u64,
    pub chat_swap_growth_mib: u64,
    pub chat_deadline_seconds: u64,
    pub image_swap_growth_mib: u64,
    pub image_deadline_seconds: u64,
    pub session_seconds: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            version: 1,
            chat_warning_ms: 10000,
            chat_swap_growth_mib: 2048,
            chat_deadline_seconds: 120,
            image_swap_growth_mib: 512,
            image_deadline_seconds: 600,
            session_seconds: 1800,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1
            || self.chat_warning_ms > 10000
            || !(64..=2048).contains(&self.chat_swap_growth_mib)
            || !(10..=120).contains(&self.chat_deadline_seconds)
            || !(64..=512).contains(&self.image_swap_growth_mib)
            || !(30..=600).contains(&self.image_deadline_seconds)
            || !(60..=1800).contains(&self.session_seconds)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Stop {
    pub source: StopSource,
    pub reason: StopReason,
    pub pressure: Option<u64>,
    pub swap_bytes: Option<u64>,
    pub baseline_swap_bytes: Option<u64>,
    pub elapsed_ms: Option<u64>,
    pub warning_used_ms: Option<u64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopSource {
    Host,
    PreviousControlledRun,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Critical,
    MemoryPolicy,
    Deadline,
    DiskOrLog,
    Ownership,
    SessionEnded,
}
impl Stop {
    pub(crate) fn from_sample(source: StopSource, reason: &str, sample: &Value) -> Self {
        let pressure = sample["memory"]["pressure"].as_u64();
        let reason = if pressure == Some(4) {
            StopReason::Critical
        } else {
            match reason {
                "turn_deadline" => StopReason::Deadline,
                "disk_reserve" | "runtime_log_limit_or_unavailable" => StopReason::DiskOrLog,
                "runtime_ownership" => StopReason::Ownership,
                "session_revoked_or_expired" => StopReason::SessionEnded,
                _ => StopReason::MemoryPolicy,
            }
        };
        Self {
            source,
            reason,
            pressure,
            swap_bytes: sample["memory"]["swap_bytes"].as_u64(),
            baseline_swap_bytes: sample["baseline_swap_bytes"].as_u64(),
            elapsed_ms: sample["elapsed_ms"].as_u64(),
            warning_used_ms: sample["warning_used_ms"].as_u64(),
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    revision: u64,
    limits: Limits,
    last_stop: Option<Stop>,
    #[serde(default)]
    features: Features,
    #[serde(default)]
    features_initialized: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Features {
    pub images: bool,
    pub multiagent: bool,
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Status {},
    Configure { revision: u64, limits: Limits },
    Features { revision: u64, features: Features },
}
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Conflict,
    Storage,
}
pub struct Diagnostics {
    state: Mutex<Saved>,
    store: Option<Store>,
    failed: AtomicBool,
    sample: Arc<dyn Fn() -> Value + Send + Sync>,
}
impl Diagnostics {
    pub fn memory(sample: Arc<dyn Fn() -> Value + Send + Sync>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(Saved {
                version: 1,
                revision: 1,
                limits: Limits::default(),
                last_stop: None,
                features: Features::default(),
                features_initialized: false,
            }),
            store: None,
            failed: AtomicBool::new(false),
            sample,
        })
    }
    pub fn persistent(
        root: &Path,
        sample: Arc<dyn Fn() -> Value + Send + Sync>,
    ) -> Result<Arc<Self>, String> {
        let store = Store::open(root)?;
        let mut saved = Saved {
            version: 1,
            revision: 1,
            limits: Limits::default(),
            last_stop: None,
            features: Features::default(),
            features_initialized: false,
        };
        if let Some((manifest, blobs)) = store.load()? {
            if !blobs.is_empty() {
                return Err("diagnostics_unexpected_blobs".into());
            }
            saved = serde_json::from_value(manifest).map_err(|_| "diagnostics_snapshot_schema")?;
            if saved.version != 1 || saved.revision == 0 || saved.revision > 9007199254740991 {
                return Err("diagnostics_snapshot_version".into());
            }
            saved
                .limits
                .validate()
                .map_err(|_| "diagnostics_snapshot_limits")?;
        }
        store.save(
            serde_json::to_value(&saved).map_err(|_| "diagnostics_encode")?,
            vec![],
        )?;
        Ok(Arc::new(Self {
            state: Mutex::new(saved),
            store: Some(store),
            failed: AtomicBool::new(false),
            sample,
        }))
    }
    pub fn limits(&self) -> Limits {
        self.state.lock().unwrap().limits.clone()
    }
    pub fn features(&self) -> Features {
        self.state.lock().unwrap().features
    }
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn initialize_evaluation(&self) -> Result<(), Error> {
        self.initialize_features(Features {
            images: true,
            multiagent: true,
        })
    }
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn initialize_features(&self, features: Features) -> Result<(), Error> {
        let mut state = self.state.lock().unwrap();
        if !state.features_initialized {
            let mut next = state.clone();
            next.features = features;
            next.features_initialized = true;
            next.revision = next
                .revision
                .checked_add(1)
                .filter(|v| *v <= 9007199254740991)
                .ok_or(Error::Conflict)?;
            self.commit(&next)?;
            *state = next;
        }
        Ok(())
    }
    pub fn storage_failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
    fn commit(&self, next: &Saved) -> Result<(), Error> {
        if self.storage_failed() {
            return Err(Error::Storage);
        }
        if let Some(s) = &self.store {
            if s.save(
                serde_json::to_value(next).map_err(|_| Error::Storage)?,
                vec![],
            )
            .is_err()
            {
                self.failed.store(true, Ordering::Release);
                return Err(Error::Storage);
            }
        }
        Ok(())
    }
    pub(crate) fn record_stop(&self, stop: Stop) -> Result<(), Error> {
        let mut state = self.state.lock().unwrap();
        let mut next = state.clone();
        next.last_stop = Some(stop);
        self.commit(&next)?;
        *state = next;
        Ok(())
    }
    pub fn import_stop(&self, path: &Path) -> Result<(), String> {
        use std::{fs, io::Read};
        let m = fs::symlink_metadata(path).map_err(|_| "stop_metadata")?;
        if !m.is_file() || m.file_type().is_symlink() || m.len() > 65536 {
            return Err("stop_regular_bounded_required".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(path)
            .map_err(|_| "stop_open")?
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| "stop_read")?;
        if bytes.len() > 65536 {
            return Err("stop_size".into());
        }
        let v: Value = serde_json::from_slice(&bytes).map_err(|_| "stop_json")?;
        let sample = &v["last_sample"];
        if !sample.is_object()
            || !matches!(sample["memory"]["pressure"].as_u64(), Some(1 | 2 | 4))
            || sample["elapsed_ms"].as_u64().is_none()
            || sample["memory"]["swap_bytes"].as_u64().is_none()
        {
            return Err("stop_sample_schema".into());
        }
        self.record_stop(Stop::from_sample(
            StopSource::PreviousControlledRun,
            v["reason"].as_str().ok_or("stop_reason")?,
            sample,
        ))
        .map_err(|_| "stop_checkpoint".into())
    }
    pub fn control(&self, command: Command) -> Result<Value, Error> {
        if let Command::Features { revision, features } = command {
            let mut state = self.state.lock().unwrap();
            if state.revision != revision {
                return Err(Error::Conflict);
            }
            let mut next = state.clone();
            next.features = features;
            next.features_initialized = true;
            next.revision = next
                .revision
                .checked_add(1)
                .filter(|v| *v <= 9007199254740991)
                .ok_or(Error::Conflict)?;
            self.commit(&next)?;
            *state = next;
            drop(state);
            return Ok(self.status());
        }
        if let Command::Configure { revision, limits } = command {
            limits.validate()?;
            let mut state = self.state.lock().unwrap();
            if state.revision != revision {
                return Err(Error::Conflict);
            }
            let mut next = state.clone();
            next.limits = limits;
            next.revision = next
                .revision
                .checked_add(1)
                .filter(|v| *v <= 9007199254740991)
                .ok_or(Error::Conflict)?;
            self.commit(&next)?;
            *state = next;
        }
        Ok(self.status())
    }
    pub fn status(&self) -> Value {
        let state = self.state.lock().unwrap().clone();
        let sample = (self.sample)();
        let pressure = match sample["pressure"].as_u64() {
            Some(1) => "normal",
            Some(2) => "warning",
            Some(4) => "critical",
            _ => "unknown",
        };
        json!({"version":1,"revision":state.revision,"limits":state.limits,"features":state.features,"last_stop":state.last_stop,"volatile":self.store.is_none(),"storage_failed":self.storage_failed(),"observation":{"scope":"whole-host","pressure":pressure,"swap_bytes":sample["swap_bytes"].as_u64(),"observed_at_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).map(|v|v.as_millis() as u64).ok(),"runtime_residency":"not_observed","capacity_guaranteed":false},"safety":{"critical_stop":true,"unknown_stop":true,"image_warning_allowed":false,"settings_grant_inference":false,"applies":"new-turns; session-duration-next-startup"}})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_initial_features_never_override_a_saved_choice() {
        let d = Diagnostics::memory(sampler());
        let configured = Features {
            images: false,
            multiagent: true,
        };
        d.initialize_features(configured).unwrap();
        assert_eq!(d.features(), configured);
        let revision = d.status()["revision"].as_u64().unwrap();
        d.control(Command::Features {
            revision,
            features: Features::default(),
        })
        .unwrap();
        d.initialize_features(configured).unwrap();
        assert_eq!(d.features(), Features::default());
    }
    #[test]
    fn evaluation_initializes_once_and_features_never_reset_stop_or_limits() {
        let d = Diagnostics::memory(sampler());
        assert_eq!(d.features(), Features::default());
        d.initialize_evaluation().unwrap();
        assert_eq!(
            d.features(),
            Features {
                images: true,
                multiagent: true
            }
        );
        let revision = d.status()["revision"].as_u64().unwrap();
        d.control(Command::Features {
            revision,
            features: Features::default(),
        })
        .unwrap();
        d.initialize_evaluation().unwrap();
        assert_eq!(d.features(), Features::default());
        assert_eq!(d.limits(), Limits::default());
        assert_eq!(d.status()["safety"]["critical_stop"], true);
    }
    #[test]
    fn examples_and_previous_stop_are_bounded_data_not_permissions() {
        let v: Value = serde_json::from_str(include_str!(
            "../../../contracts/diagnostics/v0.1.0/commands.json"
        ))
        .unwrap();
        for c in v["examples"].as_array().unwrap() {
            assert!(serde_json::from_value::<Command>(c.clone()).is_ok());
        }
        let root = crate::job_store::tests::directory();
        let file = root.join("stop.json");
        std::fs::write(&file,serde_json::to_vec(&json!({"reason":"memory_pressure_swap_or_stale_observation","last_sample":{"memory":{"pressure":4,"swap_bytes":1899954176u64},"elapsed_ms":3006,"baseline_swap_bytes":1604911104u64}})).unwrap()).unwrap();
        let d = Diagnostics::memory(sampler());
        d.import_stop(&file).unwrap();
        assert_eq!(d.status()["last_stop"]["reason"], "critical");
        assert_eq!(d.status()["last_stop"]["source"], "previous_controlled_run");
        std::fs::write(&file, b"{\"grant\":true}").unwrap();
        assert!(d.import_stop(&file).is_err());
        assert_eq!(d.status()["last_stop"]["elapsed_ms"], 3006);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn http_auth_origin_schema_bounds_and_revision_precede_effects() {
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let d = Diagnostics::memory(sampler());
        let state = crate::prepared_web::PreparedWeb::diagnostics_only()
            .unwrap()
            .with_diagnostics(d.clone())
            .unwrap();
        let app = crate::prepared_web::app(
            "127.0.0.1:49440".parse().unwrap(),
            "diagnostics-fixture-key".into(),
            state,
        )
        .unwrap();
        let request = |body: Value, key: &str, origin: &str| {
            let b = serde_json::to_vec(&body).unwrap();
            Request::post("/api/diagnostics/v1")
                .header("host", "127.0.0.1:49440")
                .header("origin", origin)
                .header("authorization", key)
                .header("content-type", "application/json")
                .header("content-length", b.len())
                .body(Body::from(b))
                .unwrap()
        };
        for (key, origin, expected) in [
            ("Bearer wrong", "http://127.0.0.1:49440", 401),
            (
                "Bearer diagnostics-fixture-key",
                "https://evil.example",
                403,
            ),
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(request(json!({"operation":"status"}), key, origin))
                    .await
                    .unwrap()
                    .status()
                    .as_u16(),
                expected
            );
        }
        let good = |b| {
            request(
                b,
                "Bearer diagnostics-fixture-key",
                "http://127.0.0.1:49440",
            )
        };
        assert_eq!(
            app.clone()
                .oneshot(good(
                    json!({"operation":"status","padding":"x".repeat(5000)})
                ))
                .await
                .unwrap()
                .status(),
            400
        );
        assert_eq!(
            app.clone()
                .oneshot(good(json!({"operation":"status","allow":true})))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        let p = Limits {
            chat_warning_ms: 500,
            ..Default::default()
        };
        let r = app
            .clone()
            .oneshot(good(
                json!({"operation":"configure","revision":1,"limits":p}),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        let v: Value =
            serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["runtime_available"], false);
        assert_eq!(v["no_inference_profile"], true);
        assert_eq!(v["limits"]["chat_warning_ms"], 500);
        assert_eq!(
            app.clone()
                .oneshot(good(
                    json!({"operation":"configure","revision":1,"limits":Limits::default()})
                ))
                .await
                .unwrap()
                .status(),
            409
        );
        let mut limits = serde_json::to_value(Limits::default()).unwrap();
        limits["critical_stop"] = json!(false);
        assert_eq!(
            app.oneshot(good(
                json!({"operation":"configure","revision":2,"limits":limits})
            ))
            .await
            .unwrap()
            .status(),
            400
        );
        assert_eq!(d.limits().chat_warning_ms, 500);
    }
    fn sampler() -> Arc<dyn Fn() -> Value + Send + Sync> {
        Arc::new(|| json!({"pressure":4,"swap_bytes":1024}))
    }
    #[test]
    fn closed_limits_cas_and_critical_cannot_be_disabled() {
        let d = Diagnostics::memory(sampler());
        let p = Limits {
            chat_warning_ms: 10001,
            ..Default::default()
        };
        assert_eq!(
            d.control(Command::Configure {
                revision: 1,
                limits: p
            })
            .unwrap_err(),
            Error::Invalid
        );
        assert_eq!(
            d.control(Command::Configure {
                revision: 0,
                limits: Limits::default()
            })
            .unwrap_err(),
            Error::Conflict
        );
        let p = Limits {
            chat_warning_ms: 500,
            ..Default::default()
        };
        assert_eq!(
            d.control(Command::Configure {
                revision: 1,
                limits: p
            })
            .unwrap()["revision"],
            2
        );
        assert_eq!(d.status()["observation"]["pressure"], "critical");
        assert_eq!(d.status()["safety"]["settings_grant_inference"], false);
        assert!(
            serde_json::from_value::<Command>(json!({"operation":"status","allow":true})).is_err()
        );
    }
    #[test]
    fn persisted_limits_stop_and_failed_commit_do_not_reset_state() {
        let root = crate::job_store::tests::directory();
        let d = Diagnostics::persistent(&root, sampler()).unwrap();
        let p = Limits {
            chat_swap_growth_mib: 512,
            ..Default::default()
        };
        d.control(Command::Configure {
            revision: 1,
            limits: p.clone(),
        })
        .unwrap();
        d.record_stop(Stop::from_sample(
            StopSource::Host,
            "memory",
            &json!({"memory":{"pressure":4,"swap_bytes":12},"elapsed_ms":3}),
        ))
        .unwrap();
        drop(d);
        let d = Diagnostics::persistent(&root, sampler()).unwrap();
        assert_eq!(d.limits(), p);
        assert_eq!(d.status()["last_stop"]["reason"], "critical");
        let moved = root.with_extension("moved");
        std::fs::rename(&root, &moved).unwrap();
        assert_eq!(
            d.control(Command::Configure {
                revision: 2,
                limits: Limits::default()
            })
            .unwrap_err(),
            Error::Storage
        );
        assert_eq!(d.limits(), p);
        assert!(d.storage_failed());
        drop(d);
        std::fs::remove_dir_all(moved).unwrap();
    }
}
