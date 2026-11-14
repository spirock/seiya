//! macOS-only filesystem/picker/runtime boundary for the common web shell.
use super::*;
use crate::desktop_setup::{Adapter, Approval, Draft, Engine, Snapshot, Work};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) struct WebAdapter {
    home: PathBuf,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Managed {
    version: u32,
    draft: Draft,
    profile: serde_json::Value,
}
#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Baseline {
    manager: Option<Vec<u8>>,
    legacy: Option<Vec<u8>>,
}
fn revision(baseline: &Baseline) -> Vec<u8> {
    Sha256::digest(serde_json::to_vec(baseline).expect("baseline serialization")).to_vec()
}
impl WebAdapter {
    pub(crate) fn new() -> std::result::Result<Self, String> {
        let home = PathBuf::from(std::env::var_os("HOME").ok_or("launcher_home_required")?)
            .canonicalize()
            .map_err(|_| "launcher_home_required")?;
        Ok(Self { home })
    }
    fn manager(&self) -> PathBuf {
        root(&self.home).join("desktop-manager.json")
    }
    fn baseline(&self) -> std::result::Result<Baseline, String> {
        let legacy = baseline(&self.home).map_err(str::to_owned)?;
        let manager = read_private(&self.manager()).map_err(str::to_owned)?;
        Ok(Baseline { manager, legacy })
    }
    fn profile(&self, draft: &Draft, baseline: &Baseline) -> std::result::Result<Vec<u8>, String> {
        // Preserve operational paths when migrating a customized legacy profile.
        // They are not inferred from a new model directory or silently reset.
        let previous = if let Some(bytes) = &baseline.manager {
            let managed: Managed =
                serde_json::from_slice(bytes).map_err(|_| "manager_invalid_schema")?;
            Some(managed.profile)
        } else if let Some(bytes) = &baseline.legacy {
            Some(
                serde_json::from_slice::<serde_json::Value>(bytes)
                    .map_err(|_| "profile_invalid_schema")?,
            )
        } else {
            None
        };
        let (data, coordination) = if let Some(value) = previous {
            (
                PathBuf::from(
                    value["data_dir"]
                        .as_str()
                        .ok_or("profile_data_dir_required")?,
                ),
                PathBuf::from(
                    value["coordination_dir"]
                        .as_str()
                        .ok_or("profile_coordination_dir_required")?,
                ),
            )
        } else {
            (
                root(&self.home).join("data"),
                root(&self.home).join("coordination"),
            )
        };
        let images = if draft.roles.image.is_empty() {
            None
        } else {
            Some(json!({"binary":draft.image_binary,"assets_dir":draft.image_assets}))
        };
        let bytes = serde_json::to_vec(&json!({"version":1,"target":"macos",
            "data_dir":data,"coordination_dir":coordination,
            "llm_binary":draft.llm_binary,"model_store":draft.model_store,"images":images,
            "chat_images":!draft.roles.image.is_empty(),
            "features":{"images":!draft.roles.image.is_empty(),"multiagent":!draft.roles.workers.is_empty()}}))
            .map_err(|_| "profile_encode")?;
        desktop_config::check_bytes(&bytes).map_err(|e| format!("{}:{}", e.field, e.code))?;
        Ok(bytes)
    }
}
impl Adapter for WebAdapter {
    fn effects(&self) -> String {
        "Guardar modifica solo el perfil privado, sin generar ni descargar modelos. Al seleccionar el rol imagen, la revisión activa imágenes desde el chat nativo: una petición explícita puede registrar image_generate. Ollama y el generador se ejecutan secuencialmente como procesos propios; no se adopta11434 ni se relaja el presupuesto3584bytes. Cierre incierto bloquea nuevos modelos; no se mide RAM/VRAM liberada.".into()
    }
    fn available(&self) -> bool {
        true
    }
    fn snapshot(&self) -> std::result::Result<Snapshot, String> {
        let baseline = self.baseline()?;
        let mut draft = Draft::default();
        let warning: Option<String>;
        if let Some(bytes) = &baseline.manager {
            let managed: Managed =
                serde_json::from_slice(bytes).map_err(|_| "manager_invalid_schema")?;
            if managed.version != 1 {
                return Err("manager_unsupported_version".into());
            }
            draft = managed.draft;
            warning =
                Some("Asignaciones recuperadas. No se inicia ningún motor automáticamente.".into());
        } else if let Some(bytes) = &baseline.legacy {
            // Recover editable paths even if a previously selected engine moved.
            let value: serde_json::Value =
                serde_json::from_slice(bytes).map_err(|_| "profile_invalid_schema")?;
            draft.llm_binary = value["llm_binary"].as_str().unwrap_or("").into();
            draft.model_store = value["model_store"].as_str().unwrap_or("").into();
            if let Some(images) = value.get("images").filter(|v| !v.is_null()) {
                draft.image_binary = images["binary"].as_str().unwrap_or("").into();
                draft.image_assets = images["assets_dir"].as_str().unwrap_or("").into();
                draft.roles.image = "qwen-image-2.1-turbo-q4".into();
            }
            if value["features"]["multiagent"] == true {
                draft.roles.workers = vec![crate::prepared_web::AUXILIARY.into()];
            }
            warning = Some("Perfil anterior recuperado para revisión; se conserva como rollback. No se ejecutó ni validó el motor.".into());
        } else {
            if let Ok(path) = self.home.join(".ollama").canonicalize() {
                if path.join("blobs").is_dir() && path.join("manifests-v2").is_dir() {
                    draft.model_store = path.to_string_lossy().into_owned();
                }
            }
            warning = Some("Solo detección de rutas conocidas. Si falta el motor/almacén compatible debes prepararlo por separado; no se descargó nada.".into());
        }
        let (path, detected) = crate::local_models::ollama::native(Some(&draft.llm_binary));
        draft.llm_binary = path;
        let warning = Some(format!(
            "{} {detected} Encontrar el archivo no certifica compatibilidad ni inferencia.",
            warning.unwrap_or_default()
        ));
        Ok(Snapshot {
            draft,
            baseline: revision(&baseline),
            warning,
        })
    }
    fn review(&self, draft: Draft, expected: Vec<u8>) -> std::result::Result<Approval, String> {
        let expected_baseline = self.baseline()?;
        if revision(&expected_baseline) != expected {
            return Err("profile_changed_no_overwrite".into());
        }
        let _lock = prepare(&self.home).map_err(str::to_owned)?;
        let profile = self.profile(&draft, &expected_baseline)?;
        Ok(Approval {
            profile,
            draft,
            baseline: expected,
        })
    }
    fn save(&self, approval: &Approval) -> std::result::Result<(), String> {
        let _lock = prepare(&self.home).map_err(str::to_owned)?;
        let expected = &approval.baseline;
        if revision(&self.baseline()?) != *expected {
            return Err("profile_changed_no_overwrite".into());
        }
        // Recheck filesystem, but never replace the approved bytes with a newly
        // selected JSON or different assignments. No arbitrary config pathname.
        desktop_config::check_bytes(&approval.profile).map_err(|e| e.code.to_owned())?;
        let bytes = serde_json::to_vec_pretty(&Managed {
            version: 1,
            draft: approval.draft.clone(),
            profile: serde_json::from_slice(&approval.profile).map_err(|_| "profile_encode")?,
        })
        .map_err(|_| "manager_encode")?;
        if bytes.len() > 65536 {
            return Err("manager_too_large".into());
        }
        let temp = root(&self.home).join(format!("manager-{}.tmp", uuid::Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_| "manager_temp_create")?;
        let result = (|| {
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| "manager_write")?;
            if revision(&self.baseline()?) != *expected {
                return Err("profile_changed_no_overwrite".into());
            }
            fs::rename(&temp, self.manager()).map_err(|_| "manager_commit")?;
            File::open(root(&self.home))
                .and_then(|f| f.sync_all())
                .map_err(|_| "manager_save_uncertain_do_not_retry")?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
    fn pick(&self, field: String) -> Work<'_, String> {
        Box::pin(async move {
            let step = match field.as_str() {
                "llm_binary" => Step::Llm,
                "model_store" => Step::Models,
                "image_binary" => Step::ImageBinary,
                "image_assets" => Step::ImageAssets,
                _ => return Err("unknown_picker".into()),
            };
            NativeUi.ask(step).await.map_err(str::to_owned)
        })
    }
    fn start(
        &self,
        profile: Vec<u8>,
        solo: bool,
        mut stop: watch::Receiver<bool>,
        setup_home: String,
    ) -> Work<'_, Engine> {
        Box::pin(async move {
            let (tx, mut ready) = oneshot::channel();
            let (mount, mut mounted) = oneshot::channel();
            // Local cancellation also handles startup timeout without leaving a
            // detached host. The adapter never returns while its host is alive.
            let (local_stop, local_rx) = watch::channel(false);
            let forward = tokio::spawn(async move {
                if !*stop.borrow() {
                    let _ = stop.changed().await;
                }
                local_stop.send_replace(true);
            });
            let mut done = tokio::spawn(async move {
                image_web::desktop(
                    &profile,
                    solo,
                    image_web::DesktopControl {
                        ready: tx,
                        stop: local_rx,
                        mount: Some(mount),
                        setup_home: Some(setup_home),
                    },
                )
                .await
            });
            let startup = async {
                let router = (&mut mounted)
                    .await
                    .map_err(|_| "host_router_unavailable".to_owned())?;
                let links = (&mut ready)
                    .await
                    .map_err(|_| "host_not_ready".to_owned())?;
                if !valid_url(&links.bootstrap, true) || !valid_url(&links.home, false) {
                    return Err("invalid_host_browser_url".into());
                }
                Ok((router, links))
            };
            let result = tokio::select! {
                value=tokio::time::timeout(std::time::Duration::from_secs(30), startup)=>value.unwrap_or_else(|_|Err("host_start_timeout".into())),
                value=&mut done=>{
                    forward.abort(); let _ = forward.await;
                    return Err(value.unwrap_or_else(|_| Err("host_task_failed".into())).err().unwrap_or_else(|| "host_ended_before_ready".into()));
                },
            };
            match result {
                Ok((router, links)) => {
                    let done = tokio::spawn(async move {
                        let result = done
                            .await
                            .unwrap_or_else(|_| Err("host_task_failed".into()));
                        forward.abort();
                        let _ = forward.await;
                        result
                    });
                    Ok(Engine {
                        router,
                        bootstrap: links.bootstrap,
                        home: links.home,
                        done,
                    })
                }
                Err(e) => {
                    // Dropping the caller's stop sender signals the forwarder;
                    // explicitly cancel through a separate task-local channel.
                    forward.abort();
                    let _ = forward.await;
                    // Channel sender drops when forward ends; desktop treats
                    // channel closure like cancellation and confirms cleanup.
                    let result = done
                        .await
                        .unwrap_or_else(|_| Err("host_task_failed".into()));
                    result?;
                    Err(e)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manager_example_matches_schema_without_touching_paths() {
        let mut value: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/desktop/manager/v1/config.example.json"
        ))
        .unwrap();
        let managed: Managed = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(managed.version, 1);
        assert!(managed.draft.executable_roles());
        serde_json::from_value::<desktop_config::Config>(managed.profile).unwrap();
        value["unknown"] = json!(true);
        assert!(serde_json::from_value::<Managed>(value).is_err());
    }
    #[test]
    fn manager_preserves_legacy_and_saves_unavailable_roles_without_execution() {
        let fixture = crate::launcher::tests::Fixture::new();
        fixture.profile();
        let adapter = WebAdapter {
            home: fixture.home.clone(),
        };
        let snapshot = adapter.snapshot().unwrap();
        assert_eq!(snapshot.baseline.len(), 32);
        let old = fs::read(profile(&fixture.home)).unwrap();
        let mut draft = snapshot.draft;
        draft.roles.brain = "another-local-brain".into();
        draft.roles.workers = vec!["worker-a".into(), "worker-b".into()];
        let approval = adapter.review(draft.clone(), snapshot.baseline).unwrap();
        assert!(!draft.executable_roles());
        adapter.save(&approval).unwrap();
        assert_eq!(adapter.snapshot().unwrap().draft, draft);
        assert_eq!(fs::read(profile(&fixture.home)).unwrap(), old);
        assert!(adapter.save(&approval).is_err());
        assert_eq!(
            fs::metadata(adapter.manager())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn first_run_needs_no_profile_and_does_not_change_existing_permissions() {
        let fixture = crate::launcher::tests::Fixture::new();
        let adapter = WebAdapter {
            home: fixture.home.clone(),
        };
        assert!(!root(&fixture.home).exists());
        let snapshot = adapter.snapshot().unwrap();
        assert!(!root(&fixture.home).exists());
        let mut draft = snapshot.draft;
        draft.llm_binary = fixture.home.join("engine").to_string_lossy().into_owned();
        draft.model_store = fixture.home.join("models").to_string_lossy().into_owned();
        let approved = adapter.review(draft, snapshot.baseline).unwrap();
        assert!(!adapter.manager().exists());
        adapter.save(&approved).unwrap();
        assert_eq!(adapter.snapshot().unwrap().draft, approved.draft);
        fs::set_permissions(root(&fixture.home), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            adapter.snapshot().err().unwrap(),
            "profile_directory_not_private"
        );
        assert_eq!(
            fs::metadata(root(&fixture.home))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
    #[test]
    fn new_image_review_opts_into_native_chat_without_rewriting_legacy_profile() {
        let f = crate::launcher::tests::Fixture::new();
        let legacy = f.profile();
        let original = fs::read(&legacy).unwrap();
        assert!(!desktop_config::check_bytes(&original).unwrap().chat_images);
        let adapter = WebAdapter {
            home: f.home.clone(),
        };
        let snap = adapter.snapshot().unwrap();
        let mut draft = snap.draft;
        draft.roles.image = "qwen-image-2.1-turbo-q4".into();
        draft.image_binary = f.home.join("engine").to_string_lossy().into_owned();
        draft.image_assets = f.home.join("models").to_string_lossy().into_owned();
        let reviewed = adapter.review(draft, snap.baseline).unwrap();
        assert!(
            desktop_config::check_bytes(&reviewed.profile)
                .unwrap()
                .chat_images
        );
        assert!(adapter.effects().contains("image_generate"));
        adapter.save(&reviewed).unwrap();
        assert_eq!(fs::read(legacy).unwrap(), original);
    }

    #[test]
    fn symlink_manager_and_external_profile_changes_are_not_overwritten() {
        use std::os::unix::fs::symlink;
        let fixture = crate::launcher::tests::Fixture::new();
        fixture.profile();
        let adapter = WebAdapter {
            home: fixture.home.clone(),
        };
        let snapshot = adapter.snapshot().unwrap();
        let approved = adapter.review(snapshot.draft, snapshot.baseline).unwrap();
        fs::write(profile(&fixture.home), b"external edit").unwrap();
        assert_eq!(
            adapter.save(&approved).err().unwrap(),
            "profile_changed_no_overwrite"
        );
        symlink(profile(&fixture.home), adapter.manager()).unwrap();
        assert!(adapter.snapshot().is_err());
        assert_eq!(fs::read(profile(&fixture.home)).unwrap(), b"external edit");
    }

    #[test]
    fn customized_history_directory_survives_migration_and_manager_reload() {
        let fixture = crate::launcher::tests::Fixture::new();
        fixture.profile();
        let custom = fixture.home.join("custom-data");
        fs::DirBuilder::new().mode(0o700).create(&custom).unwrap();
        let mut old: serde_json::Value =
            serde_json::from_slice(&fs::read(profile(&fixture.home)).unwrap()).unwrap();
        old["data_dir"] = json!(custom);
        fs::write(profile(&fixture.home), serde_json::to_vec(&old).unwrap()).unwrap();
        let adapter = WebAdapter {
            home: fixture.home.clone(),
        };
        let snapshot = adapter.snapshot().unwrap();
        let approved = adapter.review(snapshot.draft, snapshot.baseline).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&approved.profile).unwrap()["data_dir"],
            json!(custom)
        );
        adapter.save(&approved).unwrap();
        // Removing a synthetic rollback file must not reset the history path.
        fs::remove_file(profile(&fixture.home)).unwrap();
        let snapshot = adapter.snapshot().unwrap();
        let approved = adapter.review(snapshot.draft, snapshot.baseline).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&approved.profile).unwrap()["data_dir"],
            json!(custom)
        );
    }
}
