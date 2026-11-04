//! `chap-devops init DIR --adopt OLD`: a deployment that takes over one that
//! chap-core's compose files and `chap-admin` set up.
//!
//! The old directory is read and never written. The new deployment runs
//! under the old compose project name, so chap-core's volumes (the database
//! in `<project>_chap-db` above all) are the old ones as they are. The
//! `.env` secrets come over, so the database password still matches. Each
//! model in `compose.marketplace.yml` becomes a model of the new deployment,
//! and its data is copied into the volume name chap-devops uses.

use crate::compose::{EnableRequest, Selection};
use crate::error::Result;
use crate::registry::{Channel, Registry, VersionSelector};
use std::path::{Path, PathBuf};

/// The overlay `chap-admin` writes next to chap-core's compose files.
pub(super) const ADMIN_OVERLAY: &str = "compose.marketplace.yml";

/// The host port chap-core's compose files publish the API on.
pub(super) const ADMIN_API_PORT: u16 = 8000;

/// chap-core's overlays for development. A takeover does not take over their
/// services, but names them when their volumes show that they ran.
const DEVELOPMENT_OVERLAYS: &[&str] = &["compose.ewars.yml", "compose.chapkit.yml"];

/// The prefix `chap-admin` gives each model service.
const SERVICE_PREFIX: &str = "marketplace-";

/// The `.env` keys this module carries over or reads.
const CARRIED_KEYS: &[&str] = &[
    "POSTGRES_USER",
    "POSTGRES_PASSWORD",
    "POSTGRES_DB",
    "CHAP_API_TOKEN",
    "SERVICEKIT_REGISTRATION_KEY",
    "CHAP_IMAGE_TAG",
    "COMPOSE_PROJECT_NAME",
];

/// What the old deployment holds, and what the new one makes of it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Adoption {
    pub(super) dir: PathBuf,
    /// The compose project the old containers and volumes run under.
    pub(super) project: String,
    pub(super) env: OldEnv,
    /// `CHAP_IMAGE_TAG` of the old `.env`.
    pub(super) chap_tag: Option<String>,
    pub(super) models: Vec<AdoptedModel>,
    /// Lines for the report: what was decided, and why.
    pub(super) notes: Vec<String>,
    /// Lines that need attention.
    pub(super) warnings: Vec<String>,
    /// The development overlays in the old directory.
    pub(super) development: Vec<DevelopmentOverlay>,
}

/// One development overlay of chap-core: its services and its volumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DevelopmentOverlay {
    pub(super) file: String,
    pub(super) services: Vec<String>,
    pub(super) volumes: Vec<String>,
}

/// The settings of the old `.env` that the new one keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OldEnv {
    pub(super) postgres_user: String,
    pub(super) postgres_password: String,
    pub(super) postgres_db: String,
    pub(super) api_token: Option<String>,
    pub(super) registration_key: Option<String>,
}

/// One model of the old overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AdoptedModel {
    pub(super) id: String,
    /// The compose service `chap-admin` ran it as, `marketplace-<service id>`.
    pub(super) old_service: String,
    pub(super) how: How,
}

/// How the new deployment runs one model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum How {
    /// A marketplace model, at this version or channel.
    Marketplace(VersionSelector),
    /// A model added with `models add`: the image it ran, and the id it
    /// registers with chap-core under.
    Manual { image: String, service_id: String },
}

impl Adoption {
    /// The models that are marketplace models, as `init`'s selection.
    pub(super) fn selection(&self) -> Selection {
        Selection {
            enable: self
                .models
                .iter()
                .filter_map(|model| match &model.how {
                    How::Marketplace(selector) => Some(EnableRequest {
                        selector: selector.clone(),
                        ..EnableRequest::new(model.id.clone())
                    }),
                    How::Manual { .. } => None,
                })
                .collect(),
            ..Selection::default()
        }
    }
}

/// Read the old deployment in `dir`.
pub(super) fn read(dir: &Path, registry: &Registry, registry_url: &str) -> Result<Adoption> {
    if !dir.is_dir() {
        return Err(anyhow::anyhow!(
            "{} is not a directory; give --adopt the directory of the chap-admin deployment",
            dir.display()
        ));
    }
    if crate::project::Project::exists(dir) {
        return Err(anyhow::anyhow!(
            "{} is a chap-devops deployment already; move it with `chap-devops backup create` \
             and `chap-devops backup restore` instead",
            dir.display()
        ));
    }
    if !["compose.yml", "compose.ghcr.yml"]
        .iter()
        .any(|file| dir.join(file).is_file())
    {
        return Err(anyhow::anyhow!(
            "{} has no compose.yml or compose.ghcr.yml, so it is not a chap-core deployment; \
             give --adopt the directory that `chap-admin` ran in",
            dir.display()
        ));
    }
    let env = std::fs::read_to_string(dir.join(".env")).unwrap_or_default();
    let overlay = match std::fs::read_to_string(dir.join(ADMIN_OVERLAY)) {
        Ok(body) => Some(body),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "reading {}: {e}",
                dir.join(ADMIN_OVERLAY).display()
            ));
        }
    };
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut adoption = plan(dir, &name, &env, overlay.as_deref(), registry, registry_url)?;
    adoption.development = development_overlays(dir);
    Ok(adoption)
}

/// The development overlays in `dir`, and the files they include. A file
/// that cannot be read or parsed is left out: it only adds a warning.
fn development_overlays(dir: &Path) -> Vec<DevelopmentOverlay> {
    let mut files: Vec<String> = DEVELOPMENT_OVERLAYS.iter().map(|f| f.to_string()).collect();
    let mut found = Vec::new();
    let mut next = 0;
    while next < files.len() {
        let file = files[next].clone();
        next += 1;
        let Ok(body) = std::fs::read_to_string(dir.join(&file)) else {
            continue;
        };
        let Ok(doc) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&body) else {
            continue;
        };
        for included in doc
            .get("include")
            .and_then(|v| v.as_sequence())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
        {
            if !files.iter().any(|f| f == included) {
                files.push(included.to_string());
            }
        }
        let keys = |key: &str| -> Vec<String> {
            doc.get(key)
                .and_then(|v| v.as_mapping())
                .into_iter()
                .flatten()
                .filter_map(|(k, _)| k.as_str().map(str::to_string))
                .collect()
        };
        let overlay = DevelopmentOverlay {
            services: keys("services"),
            volumes: keys("volumes"),
            file,
        };
        if !overlay.services.is_empty() {
            found.push(overlay);
        }
    }
    found
}

/// [`read`] on what was read, so the decisions can be tested without files.
pub(super) fn plan(
    dir: &Path,
    dir_name: &str,
    env: &str,
    overlay: Option<&str>,
    registry: &Registry,
    registry_url: &str,
) -> Result<Adoption> {
    let mut notes = Vec::new();
    let mut warnings = Vec::new();

    let project = crate::dotenv::non_empty(env, "COMPOSE_PROJECT_NAME")
        .unwrap_or_else(|| compose_default_name(dir_name));
    if project.is_empty() {
        return Err(anyhow::anyhow!(
            "the name of {} gives no compose project name; set COMPOSE_PROJECT_NAME in its \
             .env to the name its containers ran under, then run this again",
            dir.display()
        ));
    }

    // compose.ghcr.yml falls back to these when POSTGRES_* is unset.
    let postgres = |key: &str, fallback: &str| {
        crate::dotenv::non_empty(env, key).unwrap_or_else(|| fallback.to_string())
    };
    let old_env = OldEnv {
        postgres_user: postgres("POSTGRES_USER", "chap"),
        postgres_password: postgres("POSTGRES_PASSWORD", "chap"),
        postgres_db: postgres("POSTGRES_DB", "chap_core"),
        api_token: crate::dotenv::non_empty(env, "CHAP_API_TOKEN"),
        registration_key: crate::dotenv::non_empty(env, "SERVICEKIT_REGISTRATION_KEY"),
    };
    if old_env.api_token.is_none() && old_env.registration_key.is_some() {
        notes.push(
            "the old .env sets SERVICEKIT_REGISTRATION_KEY without CHAP_API_TOKEN; chap-devops \
             writes the key only with a token, so registration is open as the API is"
                .to_string(),
        );
    }
    let left_out: Vec<String> = active_keys(env)
        .into_iter()
        .filter(|key| !CARRIED_KEYS.contains(&key.as_str()))
        .collect();
    if !left_out.is_empty() {
        warnings.push(format!(
            "the .env of {} sets {}, which chap-devops does not carry over; copy those lines \
             into the new .env if the deployment needs them",
            dir.display(),
            left_out.join(", ")
        ));
    }

    let chap_tag = crate::dotenv::non_empty(env, "CHAP_IMAGE_TAG");
    let models = match overlay {
        Some(body) => adopt_models(body, registry, registry_url, &mut notes)?,
        None => {
            notes.push(format!(
                "{} has no {ADMIN_OVERLAY}, so no models come over",
                dir.display()
            ));
            Vec::new()
        }
    };
    Ok(Adoption {
        dir: dir.to_path_buf(),
        project,
        env: old_env,
        chap_tag,
        models,
        notes,
        warnings,
        development: Vec::new(),
    })
}

/// The models of a `chap-admin` overlay.
fn adopt_models(
    body: &str,
    registry: &Registry,
    registry_url: &str,
    notes: &mut Vec<String>,
) -> Result<Vec<AdoptedModel>> {
    let overlay: serde_yaml_ng::Value = serde_yaml_ng::from_str(body)
        .map_err(|e| anyhow::anyhow!("{ADMIN_OVERLAY} of the old deployment: {e}"))?;
    let ours = registry_base(registry_url);
    let mut models = Vec::new();
    let services = overlay
        .get("services")
        .and_then(|services| services.as_mapping())
        .cloned()
        .unwrap_or_default();
    for (name, service) in services {
        let Some(old_service) = name.as_str().map(str::to_string) else {
            continue;
        };
        let text = |key: &str| {
            service
                .get(key)
                .and_then(|value| value.as_str())
                .map(str::to_string)
        };
        let Some(image) = text("image") else {
            notes.push(format!(
                "{old_service} in {ADMIN_OVERLAY} names no image, so it does not come over"
            ));
            continue;
        };
        let service_id = old_service
            .strip_prefix(SERVICE_PREFIX)
            .unwrap_or(&old_service)
            .to_string();
        let id = service_id.replace('-', "_");
        let custom = service
            .get("x-chap-custom")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let version = text("x-chap-version");
        let from = text("x-chap-registry");
        let listed = registry.get(&id).filter(|model| !model.manual);
        let how = match (custom, listed, version.as_deref()) {
            (false, Some(model), Some(version))
                if from.as_deref().map(registry_base) == Some(ours.clone()) =>
            {
                match model.version(version) {
                    Some(found) if found.status != crate::registry::VersionStatus::Yanked => {
                        if model.channels.stable == version {
                            How::Marketplace(VersionSelector::Channel(Channel::Stable))
                        } else {
                            notes.push(format!(
                                "{id} stays at {version}, the version it ran; run `chap-devops \
                                 models enable {id} --channel stable` to follow the marketplace"
                            ));
                            How::Marketplace(VersionSelector::Exact(version.to_string()))
                        }
                    }
                    _ => {
                        notes.push(format!(
                            "the marketplace no longer has {id} {version}, so it comes over as \
                             a model added at the image it ran"
                        ));
                        manual(&image, &service_id)
                    }
                }
            }
            (false, None, _) => {
                notes.push(format!(
                    "the marketplace no longer lists {id}, so it comes over as a model added \
                     at the image it ran"
                ));
                manual(&image, &service_id)
            }
            _ => {
                // A custom image registers under its own id, which chap-admin
                // recorded as the template name.
                let registers_as = text("x-chap-template").unwrap_or(service_id);
                manual(&image, &registers_as)
            }
        };
        models.push(AdoptedModel {
            id,
            old_service,
            how,
        });
    }
    Ok(models)
}

fn manual(image: &str, service_id: &str) -> How {
    How::Manual {
        image: image.to_string(),
        service_id: service_id.to_string(),
    }
}

/// The project name compose gives a directory: lowercase, with everything
/// outside `[a-z0-9_-]` removed, and starting with a letter or a digit.
pub(super) fn compose_default_name(dir_name: &str) -> String {
    let kept: String = dir_name
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_' || *c == '-')
        .collect();
    kept.trim_start_matches(['_', '-']).to_string()
}

/// The keys the active lines of a `.env` body set to something.
fn active_keys(body: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for line in body.lines().map(str::trim) {
        if line.starts_with('#') {
            continue;
        }
        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_start_matches("export ").trim().to_string();
        if !key.is_empty() && crate::dotenv::non_empty(body, &key).is_some() && !keys.contains(&key)
        {
            keys.push(key);
        }
    }
    keys
}

/// What docker says about the old deployment, asked before anything is
/// written. The lookups are injected so the rules can be tested.
pub(super) struct Docker<'a> {
    /// Whether the project has containers, running or stopped; `None` when
    /// docker could not be asked.
    pub(super) has_containers: &'a dyn Fn(&str) -> Option<bool>,
    pub(super) volume_exists: &'a dyn Fn(&str) -> bool,
}

impl Docker<'_> {
    /// The real lookups.
    pub(super) fn real() -> Docker<'static> {
        Docker {
            has_containers: &crate::docker::project_has_containers,
            volume_exists: &crate::docker::volume_exists,
        }
    }
}

/// Refuse a takeover that would run two deployments on one database, or
/// one that would start with an empty database because the name is wrong.
pub(super) fn check(adoption: &mut Adoption, docker: &Docker) -> Result<()> {
    let project = adoption.project.clone();
    match (docker.has_containers)(&project) {
        Some(true) => {
            return Err(anyhow::anyhow!(
                "the deployment in {} still has containers in compose project {project}; stop \
                 it with `docker compose -p {project} down`, which keeps its volumes, then run \
                 this again",
                adoption.dir.display()
            ));
        }
        Some(false) => {}
        None => {
            adoption.warnings.push(format!(
                "docker could not be asked, so chap-devops did not check that the deployment in \
                 {} is stopped, and copied no model data; stop it with `docker compose -p \
                 {project} down` before `chap-devops up`",
                adoption.dir.display()
            ));
            return Ok(());
        }
    }
    let database = format!("{project}_chap-db");
    if !(docker.volume_exists)(&database) {
        return Err(anyhow::anyhow!(
            "found no volume {database}, so this would start chap-core with an empty \
             database; if the deployment in {} ran under another compose project name, set \
             COMPOSE_PROJECT_NAME in its .env to that name, then run this again",
            adoption.dir.display()
        ));
    }
    adoption
        .notes
        .push(format!("chap-core keeps its database in {database}"));
    for overlay in &adoption.development {
        let Some(volume) = overlay
            .volumes
            .iter()
            .map(|volume| format!("{project}_{volume}"))
            .find(|volume| (docker.volume_exists)(volume))
        else {
            continue;
        };
        adoption.warnings.push(format!(
            "the deployment in {} also ran {} ({volume} is there), and chap-devops does not take over its services ({}); if you need the model, add it with `chap-devops models enable`",
            adoption.dir.display(),
            overlay.file,
            overlay.services.join(", ")
        ));
    }
    Ok(())
}

/// Copy the data volume of each adopted model to the name chap-devops gives
/// it. The old volume stays; a new volume that exists already is left alone,
/// and a model that did not come over gets no volume.
pub(super) fn copy_model_data(
    adoption: &Adoption,
    project: &crate::project::Project,
    lines: &mut Vec<String>,
) {
    for model in &adoption.models {
        if !project.state.models.contains_key(&model.id) {
            continue;
        }
        let from = format!("{}_{}-data", adoption.project, model.old_service);
        let Some(to) = project.prefixed_volume(&crate::compose::volume_name(&model.id)) else {
            continue;
        };
        if !crate::docker::volume_exists(&from) {
            continue;
        }
        if crate::docker::volume_exists(&to) {
            lines.push(format!(
                "{to} exists already, so the data of {} in {from} was not copied",
                model.id
            ));
            continue;
        }
        match crate::docker::copy_volume(&from, &to, &adoption.project) {
            Ok(()) => lines.push(format!(
                "copied the data of {} from {from} to {to}",
                model.id
            )),
            Err(err) => lines.push(format!(
                "could not copy the data of {} from {from} to {to}: {err}; the model starts \
                 with no data, and {from} still has it",
                model.id
            )),
        }
    }
}

/// The models that come over as `models add` entries, as the arguments
/// `models add` takes.
pub(super) fn manual_args(adoption: &Adoption) -> Vec<crate::cli::ModelsAddArgs> {
    adoption
        .models
        .iter()
        .filter_map(|model| match &model.how {
            How::Manual { image, service_id } => Some(crate::cli::ModelsAddArgs {
                source: image.clone(),
                id: Some(model.id.clone()),
                service_id: Some(service_id.clone()),
                name: None,
                port: None,
                bind: None,
                data_dir: None,
                user: None,
                runtime_amd64: false,
                allow_starter: false,
            }),
            How::Marketplace(_) => None,
        })
        .collect()
}

/// The base URL of a registry, for a comparison with `x-chap-registry`. The
/// marketplace repository was `dhis2-chap/model-marketplace` before it became
/// `dhis2-chap/chap-marketplace`. A deployment that `chap-admin` made before
/// chap-core #828 has the old name, so both names give the same base.
fn registry_base(url: &str) -> String {
    url.trim_end_matches('/')
        .trim_end_matches("/registry.yaml")
        .trim_end_matches('/')
        .replace(
            "/dhis2-chap/model-marketplace/",
            "/dhis2-chap/chap-marketplace/",
        )
}

#[cfg(test)]
mod tests;
