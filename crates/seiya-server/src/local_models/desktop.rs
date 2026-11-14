//! Catalogue adapter for the unchanged desktop web protocol on all desktop OSes.
use super::{pool::Workers, storage, Assignments, Catalog, Pool};
use crate::{
    desktop_setup::{Adapter, Approval, Draft, Engine, Snapshot, Work},
    prepared_web::PreparedWeb,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::watch;

pub(crate) struct Desktop {
    root: PathBuf,
    catalog_path: PathBuf,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    assignments: Option<Assignments>,
    #[serde(default)]
    ollama_binary: String,
    #[serde(default)]
    model_store: String,
    #[serde(default)]
    image: Option<String>,
}
impl Desktop {
    pub(crate) fn new(root: PathBuf, catalog_path: PathBuf) -> Self {
        Self { root, catalog_path }
    }
    fn roles_path(&self) -> PathBuf {
        self.root.join("local-model-roles.json")
    }
    fn catalog(&self) -> Result<(Catalog, Vec<u8>), String> {
        let bytes = storage::read(&self.catalog_path)?.ok_or("local_catalog_required")?;
        let config = Catalog::parse(&bytes).map_err(|e| e.to_string())?;
        Ok((config, bytes))
    }
    fn baseline(&self) -> Result<Vec<u8>, String> {
        Ok(storage::revision(&[
            storage::read(&self.catalog_path)?,
            storage::read(&self.roles_path())?,
        ]))
    }
    fn assignments(draft: &Draft) -> Result<Assignments, String> {
        if !draft.image_binary.is_empty() || !draft.image_assets.is_empty() {
            return Err("El catálogo usa endpoints de imagen preparados, no ejecutables/assets; deja esas rutas vacías.".into());
        }
        Ok(Assignments {
            brain: draft.roles.brain.clone(),
            workers: draft.roles.workers.clone(),
        })
    }
}
impl Adapter for Desktop {
    fn allows_unassigned_configuration(&self) -> bool {
        true
    }
    fn available(&self) -> bool {
        // The unchanged UI uses this as adapter/configuration availability.
        // Keep review/save enabled without a catalogue; supports gates start.
        true
    }
    fn supports(&self, draft: &Draft) -> bool {
        Self::assignments(draft)
            .and_then(|r| {
                self.catalog().and_then(|(mut c, _)| {
                    c.assignments = r;
                    c.image = (!draft.roles.image.is_empty()).then(|| draft.roles.image.clone());
                    c.validate().map_err(|e| e.to_string())
                })
            })
            .is_ok()
    }
    fn filesystem_checked(&self) -> bool {
        false
    }
    fn effects(&self) -> String {
        let mut text="Backend HTTP local externo. Guardar cambia asignaciones y rutas privadas; no modifica el catálogo, inicia motores, descarga pesos ni valida inferencia. La ruta Ollama solo comprueba archivos, no conecta ni certifica su runtime; el almacén es un metadato no explorado. Los endpoints deben estar dedicados a esta instancia; locks cooperativos no controlan clientes ajenos. Memoria/VRAM y modelos residentes siguen bajo el motor. ".to_owned();
        if let Ok((catalog, _)) = self.catalog() {
            if catalog.web_search {
                text.push_str("Búsqueda web ACTIVADA: las consultas propuestas se envían a html.duckduckgo.com y pueden leerse fuentes HTTPS públicas. Sin cookies/login, archivos personales, proxy o fallback; crear/iniciar host no consulta Internet. ");
            }
            for m in catalog.models {
                text.push_str(&format!(
                    "{} → {} (binding {}). ",
                    m.id, m.runtime.base_url, m.runtime.model
                ));
            }
            for m in catalog.image_models {
                text.push_str(&format!(
                    "Imagen {} → {} (binding {}). ",
                    m.id, m.runtime.base_url, m.runtime.model
                ));
            }
            if catalog.image_chat {
                text.push_str("Imagen desde chat ACTIVADA: una propuesta image_generate registra un trabajo con prompt, 512×512, semilla42. Sin ejecución de prosa ni URLs del modelo. ");
            }
        }
        text
    }
    fn snapshot(&self) -> Result<Snapshot, String> {
        let catalog = match self.catalog() {
            Ok((v, _)) => Some(v),
            Err(e) if e == "local_catalog_required" => None,
            Err(e) => return Err(e),
        };
        let saved = if let Some(bytes) = storage::read(&self.roles_path())? {
            let saved: Saved =
                serde_json::from_slice(&bytes).map_err(|_| "local_roles_invalid_schema")?;
            if saved.version != 1 {
                return Err("local_roles_unsupported_version".into());
            }
            Some(saved)
        } else {
            None
        };
        let assignments = saved
            .as_ref()
            .and_then(|s| s.assignments.clone())
            .or_else(|| catalog.as_ref().map(|c| c.assignments.clone()));
        let (llm_binary, status) =
            super::ollama::native(saved.as_ref().map(|s| s.ollama_binary.as_str()));
        let draft = Draft {
            llm_binary,
            model_store: saved
                .as_ref()
                .map(|s| s.model_store.clone())
                .unwrap_or_default(),
            roles: crate::desktop_setup::Roles {
                brain: assignments
                    .as_ref()
                    .map(|a| a.brain.clone())
                    .unwrap_or_default(),
                workers: assignments.map(|a| a.workers).unwrap_or_default(),
                image: saved
                    .as_ref()
                    .and_then(|s| s.image.clone())
                    .or_else(|| catalog.as_ref().and_then(|c| c.image.clone()))
                    .unwrap_or_default(),
            },
            ..Draft::default()
        };
        let backend = if catalog.is_some() {
            "Catálogo HTTP local de texto disponible; revisa IDs y endpoints antes de iniciar."
        } else {
            "Falta local-models.json: puedes guardar la ruta, pero el chat no puede iniciarse sin catálogo válido."
        };
        Ok(Snapshot{draft,baseline:self.baseline()?,warning:Some(format!("{status} {backend} Encontrar el archivo no valida runtime/modelos ni inicia Ollama. El almacén no se explora."))})
    }
    fn review(&self, draft: Draft, baseline: Vec<u8>) -> Result<Approval, String> {
        if self.baseline()? != baseline {
            return Err("profile_changed_no_overwrite".into());
        }
        let assignments = Self::assignments(&draft)?;
        let profile = match self.catalog() {
            Ok((mut catalog, _)) => {
                catalog.assignments = assignments;
                catalog.image = (!draft.roles.image.is_empty()).then(|| draft.roles.image.clone());
                catalog.validate().map_err(|e| e.to_string())?;
                serde_json::to_vec(&catalog).map_err(|_| "catalog_encode")?
            }
            Err(e)
                if e == "local_catalog_required"
                    && assignments.brain.is_empty()
                    && assignments.workers.is_empty() =>
            {
                if !draft.roles.image.is_empty() {
                    return Err("image_catalog_required".into());
                }
                vec![]
            }
            Err(e) => return Err(e),
        };
        storage::ensure_directory(&self.root)?;
        let _lock = storage::lock(&self.root.join("local-model-config.lock"))?;
        if self.baseline()? != baseline {
            return Err("profile_changed_no_overwrite".into());
        }
        Ok(Approval {
            profile,
            draft,
            baseline,
        })
    }
    fn save(&self, approval: &Approval) -> Result<(), String> {
        storage::ensure_directory(&self.root)?;
        let _lock = storage::lock(&self.root.join("local-model-config.lock"))?;
        if self.baseline()? != approval.baseline {
            return Err("profile_changed_no_overwrite".into());
        }
        let assignments = if approval.profile.is_empty() {
            None
        } else {
            Some(
                Catalog::parse(&approval.profile)
                    .map_err(|e| e.to_string())?
                    .assignments,
            )
        };
        let bytes = serde_json::to_vec(&Saved {
            version: 1,
            assignments,
            ollama_binary: approval.draft.llm_binary.clone(),
            model_store: approval.draft.model_store.clone(),
            image: Some(approval.draft.roles.image.clone()),
        })
        .map_err(|_| "local_roles_encode")?;
        let old = storage::read(&self.roles_path())?;
        if self.baseline()? != approval.baseline {
            return Err("profile_changed_no_overwrite".into());
        }
        storage::replace(&self.roles_path(), &old, &bytes)
    }
    fn pick(&self, _: String) -> Work<'_, String> {
        Box::pin(async {
            Err("Este adaptador conecta endpoints HTTP ya preparados; no selecciona ni ejecuta binarios. Deja las rutas vacías.".into())
        })
    }
    fn start(
        &self,
        profile: Vec<u8>,
        _: bool,
        mut stop: watch::Receiver<bool>,
        setup_home: String,
    ) -> Work<'_, Engine> {
        Box::pin(async move {
            if *stop.borrow() {
                return Err("startup_cancelled".into());
            }
            let catalog = Catalog::parse(&profile).map_err(|e| e.to_string())?;
            let brain = catalog
                .model(&catalog.assignments.brain)
                .map_err(|e| e.to_string())?
                .runtime
                .clone();
            let auxiliary = catalog
                .assignments
                .workers
                .first()
                .map(|id| catalog.model(id).unwrap().runtime.model.clone());
            let brain_id = catalog.assignments.brain.clone();
            let image_chat = catalog.image_chat;
            let has_image = catalog.image.is_some();
            let web = if catalog.web_search {
                Some(crate::web_chat::WebChat::native(
                    &self.root,
                    catalog
                        .model(&brain_id)
                        .map_err(|e| e.to_string())?
                        .max_input_bytes,
                    brain.max_output_tokens.min(256),
                )?)
            } else {
                None
            };
            let pool = Pool::native(catalog, &self.root)?;
            let state = PreparedWeb::local_models(
                brain,
                pool.bound(&brain_id),
                Arc::new(Workers(pool.clone())),
                pool.clone(),
                auxiliary,
            )
            .map_err(|e| e.to_string())?;
            let state = if let Some(web) = web {
                state.with_web(web).map_err(|e| e.to_string())?
            } else {
                state
            };
            let state = if has_image {
                let images = crate::image_jobs::ImageJobs::persistent(
                    Arc::new(super::images::Images(pool.clone())),
                    pool.clone(),
                    &self.root.join("image-history"),
                )?;
                let state = state.with_images(images).map_err(|e| e.to_string())?;
                if image_chat {
                    state.with_chat_images().map_err(|e| e.to_string())?
                } else {
                    state
                }
            } else {
                state
            };
            state.desktop_setup_home(setup_home);
            let address = "127.0.0.1:8015".parse().unwrap();
            let (router, bootstrap) = crate::pair_trial::access::app(address, state.clone())
                .map_err(|e| e.to_string())?;
            let home = state.local_browser_home(address);
            let done = tokio::spawn(async move {
                if !*stop.borrow() {
                    let _ = stop.changed().await;
                }
                pool.stop();
                let drained = state.shutdown_local_models().await;
                while !pool.idle() {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                drained?;
                if pool.uncertain() {
                    Err("host_cleanup_unconfirmed".into())
                } else {
                    Ok(())
                }
            });
            Ok(Engine {
                router,
                bootstrap,
                home,
                done,
            })
        })
    }
}
pub fn check_file(path: &Path) -> Result<serde_json::Value, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "local_catalog_required")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("regular_catalog_file_required".into());
    }
    let path = path
        .canonicalize()
        .map_err(|_| "catalog_canonical_path_required")?;
    let bytes = storage::read(&path)?.ok_or("local_catalog_required")?;
    let c = Catalog::parse(&bytes).map_err(|e| e.to_string())?;
    Ok(
        serde_json::json!({"version":1,"model_count":c.models.len(),"assignments":c.assignments,"network_used":false,"inference_validated":false}),
    )
}

#[cfg(test)]
mod tests;
