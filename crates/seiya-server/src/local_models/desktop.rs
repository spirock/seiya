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
    // Production starts always take the stable macOS coordination lease;
    // only tests with a temporary root set this to false.
    stable_lease: bool,
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
    #[serde(default)]
    managed_ollama: bool,
}
impl Desktop {
    pub(crate) fn new(root: PathBuf, catalog_path: PathBuf) -> Self {
        Self {
            root,
            catalog_path,
            stable_lease: true,
        }
    }
    /// Test-only constructor: a temporary root can never satisfy the stable
    /// coordination directory contract, so the native lease is skipped.
    #[cfg(test)]
    pub(crate) fn new_without_stable_lease(root: PathBuf, catalog_path: PathBuf) -> Self {
        Self {
            root,
            catalog_path,
            stable_lease: false,
        }
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
    fn managed_candidate(draft: &Draft) -> bool {
        super::manager::supported()
            && super::manager::valid_model_id(&draft.roles.brain)
            && super::manager::installed(&draft.roles.brain) == Some(true)
            && draft.roles.workers.is_empty()
            && draft.roles.image.is_empty()
            && draft.image_binary.is_empty()
            && draft.image_assets.is_empty()
    }
    fn saved_managed(&self) -> Result<bool, String> {
        let Some(bytes) = storage::read(&self.roles_path())? else {
            return Ok(false);
        };
        let saved: Saved =
            serde_json::from_slice(&bytes).map_err(|_| "local_roles_invalid_schema")?;
        Ok(saved.version == 1 && saved.managed_ollama)
    }
}
impl Adapter for Desktop {
    fn inventory_supported(&self) -> bool {
        true
    }
    fn inventory(&self, id: String) -> Work<'_, &'static str> {
        Box::pin(async move {
            use crate::runtime::{HttpTransport, TransportError};
            let (catalog, _) = self.catalog()?;
            let runtime = catalog
                .models
                .iter()
                .find(|m| m.id == id)
                .map(|m| &m.runtime)
                .or_else(|| {
                    catalog
                        .image_models
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| &m.runtime)
                })
                .ok_or("unknown_catalog_id")?;
            let transport = HttpTransport::new(runtime).map_err(|e| e.to_string())?;
            Ok(match transport.advertises(&runtime.model).await {
                Ok(true) => "advertised",
                Ok(false) => "absent",
                Err(TransportError::NotConnected) => "unreachable",
                Err(TransportError::Timeout) => "timeout",
                Err(_) => "inventory_error",
            })
        })
    }
    fn allows_unassigned_configuration(&self) -> bool {
        true
    }
    fn available(&self) -> bool {
        // The unchanged UI uses this as adapter/configuration availability.
        // Keep review/save enabled without a catalogue; supports gates start.
        true
    }
    fn supports(&self, draft: &Draft) -> bool {
        match self.catalog() {
            Ok((mut catalog, _)) => Self::assignments(draft)
                .and_then(|r| {
                    catalog.assignments = r;
                    catalog.image =
                        (!draft.roles.image.is_empty()).then(|| draft.roles.image.clone());
                    catalog.validate().map_err(|e| e.to_string())
                })
                .is_ok(),
            Err(error) if error == "local_catalog_required" => {
                super::manager::supported()
                    && (Self::managed_candidate(draft)
                        || !draft.roles.brain.is_empty() && self.saved_managed().unwrap_or(false))
            }
            Err(_) => false,
        }
    }
    fn filesystem_checked(&self) -> bool {
        false
    }
    fn effects(&self) -> String {
        let mut text="Backend HTTP local externo. Guardar cambia asignaciones y rutas privadas; no modifica el catálogo, inicia motores, descarga pesos ni valida inferencia. La ruta Ollama solo comprueba archivos; el almacén es un metadato no explorado. Los endpoints deben estar dedicados a esta instancia; locks cooperativos no controlan clientes ajenos. Memoria/VRAM y modelos residentes siguen bajo el motor. Si eliges un modelo Ollama sin catálogo, Confirmar e iniciar crea una instancia privada en 127.0.0.1:49401, comprueba solo su inventario y la detiene al cerrar el chat; nunca adopta ni detiene el servicio compartido en 11434. No se ejecuta inferencia hasta enviar un mensaje. ".to_owned();
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
        let managed = saved
            .as_ref()
            .is_some_and(|s| s.managed_ollama && super::manager::supported());
        let backend = if managed {
            "Modelo Ollama seleccionado; Seiya usará su instancia privada al iniciar explícitamente."
        } else if saved.as_ref().is_some_and(|s| s.managed_ollama) {
            "Este perfil de instancia privada Ollama todavía no se ejecuta en esta plataforma."
        } else if catalog.is_some() {
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
            Err(e)
                if e == "local_catalog_required"
                    && Self::managed_candidate(&draft)
                    && assignments.workers.is_empty() =>
            {
                super::manager::managed_catalog(&draft.roles.brain).map_err(str::to_owned)?
            }
            Err(e)
                if e == "local_catalog_required"
                    && !draft.roles.brain.is_empty()
                    && !super::manager::valid_model_id(&draft.roles.brain) =>
            {
                return Err("invalid_model_id".into());
            }
            Err(e)
                if e == "local_catalog_required"
                    && !draft.roles.brain.is_empty()
                    && (!draft.roles.workers.is_empty() || !draft.roles.image.is_empty()) =>
            {
                return Err("ollama_managed_roles_unsupported".into());
            }
            Err(e) if e == "local_catalog_required" && !draft.roles.brain.is_empty() => {
                if super::manager::supported()
                    && super::manager::installed(&draft.roles.brain).is_none()
                {
                    return Err("ollama_inventory_not_checked".into());
                }
                return Err("ollama_model_not_installed".into());
            }
            Err(e) => return Err(e),
        };
        storage::ensure_directory(&self.root)?;
        let _lock = storage::config_lock(&self.root)?;
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
        let _lock = storage::config_lock(&self.root)?;
        if self.baseline()? != approval.baseline {
            return Err("profile_changed_no_overwrite".into());
        }
        let managed_ollama = !approval.profile.is_empty()
            && Catalog::parse(&approval.profile)
                .ok()
                .and_then(|catalog| super::manager::managed_model(&catalog).map(str::to_owned))
                .is_some();
        let assignments = if managed_ollama {
            Some(Assignments {
                brain: approval.draft.roles.brain.clone(),
                workers: vec![],
            })
        } else if approval.profile.is_empty() {
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
            managed_ollama,
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
            let managed_model = super::manager::managed_model(&catalog).map(str::to_owned);
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
            let candidates = std::iter::once(&brain_id)
                .chain(&catalog.assignments.workers)
                .map(|id| {
                    catalog
                        .model(id)
                        .map(|m| (id.clone(), m.runtime.clone()))
                        .map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
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
            let pool = Pool::native(catalog, &self.root, self.stable_lease)?;
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
                let history = self.root.join("image-history");
                storage::ensure_directory(&history)?;
                let images = crate::image_jobs::ImageJobs::persistent(
                    Arc::new(super::images::Images(pool.clone())),
                    pool.clone(),
                    &history,
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
            let port = crate::settings_menu::desktop_port(&setup_home)?;
            state.desktop_setup_home(setup_home);
            let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
            let (router, bootstrap) = crate::pair_trial::access::app(address, state.clone())
                .map_err(|e| e.to_string())?;
            let home = state.local_browser_home(address);
            let mut owned_ollama = if let Some(model) = managed_model.as_deref() {
                let saved = storage::read(&self.roles_path())?.ok_or("local_roles_required")?;
                let saved: Saved =
                    serde_json::from_slice(&saved).map_err(|_| "local_roles_invalid_schema")?;
                if !saved.managed_ollama {
                    return Err("local_roles_invalid_schema".into());
                }
                tokio::select! {
                    result = super::manager::launch_owned(&saved.ollama_binary, model) => {
                        Some(result?)
                    },
                    _ = stop.changed() => return Err("startup_cancelled".into()),
                }
            } else {
                None
            };
            let (probe, mut requests) =
                tokio::sync::mpsc::channel::<tokio::sync::oneshot::Sender<Option<bool>>>(1);
            let residency = managed_model.as_ref().map(|_| probe.clone());
            let mut vision_provider = None;
            for (id, runtime) in candidates {
                if *stop.borrow() {
                    break;
                }
                if super::manager::advertises_vision(&runtime).await {
                    vision_provider = Some(crate::desktop_setup::VisualProvider {
                        role: id,
                        port: runtime
                            .endpoint()
                            .map_err(|e| e.to_string())?
                            .port()
                            .ok_or("missing_local_port")?,
                        model: runtime.model,
                    });
                    break;
                }
            }
            drop(probe);
            let done = tokio::spawn(async move {
                if let (Some(child), Some(model)) = (&mut owned_ollama, managed_model) {
                    while !*stop.borrow() {
                        tokio::select! {
                            biased;
                            _ = stop.changed() => break,
                            req = requests.recv() => {
                                let Some(reply) = req else { break };
                                let loaded = if matches!(child.try_wait(), Ok(None)) {
                                    super::manager::model_loaded(&model).await
                                } else { None };
                                let _ = reply.send(loaded);
                            }
                        }
                    }
                } else if !*stop.borrow() {
                    let _ = stop.changed().await;
                }
                pool.stop();
                let drained = state.shutdown_local_models().await;
                while !pool.idle() {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                if let Some(child) = owned_ollama {
                    super::manager::stop_owned(child).await?;
                }
                drained?;
                if pool.uncertain() {
                    Err("host_cleanup_unconfirmed".into())
                } else {
                    Ok(())
                }
            });
            Ok(Engine {
                vision_provider,
                residency,
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
