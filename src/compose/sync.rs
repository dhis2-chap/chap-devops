//! Rendering `.chap-devops/` into the compose files at the project root.
//!
//! `.chap-devops/` is intent; `compose.yml`, `compose.chap-devops.yml`,
//! `compose.<service_id>.yml` and `compose.marketplace.yml` are artifacts.
//! [`sync`] is the one place that
//! turns the former into the latter: `apply` ends in it, `chap-devops sync` calls it
//! directly and `chap-devops up` runs it before `docker compose up`.
//!
//! Rendering is deterministic and every file is compared before it is
//! written, so a second sync reports everything as unchanged. `compose.yml`
//! is deterministic too because the copy of chap-core's `compose.ghcr.yml` it
//! is rendered from is kept in `.chap-devops/`; nothing here touches the network.

mod config;
mod env;

pub use config::{KeyEdit, dhis2_config_path, set_read_only, write_dhis2_config, write_ocs_config};
pub(crate) use env::append_env_pins;
pub use env::{EnvTag, refresh_env_pin, set_env_chap_tag};

use crate::components::{DHIS2_COMPOSE, OCS_COMPOSE, S3_COMPOSE};
use crate::compose::overrides;
use crate::compose::render::{
    render_base, render_chap_devops_overlay, render_dhis2, render_ocs, render_overlay, render_s3,
    render_umbrella,
};
use crate::compose::spec::{
    BaseSpec, ChapDevopsOverlaySpec, Dhis2Spec, OcsSpec, OverlaySpec, S3Spec, UpstreamCompose,
    compose_services,
};
use crate::error::Result;
use crate::project::{
    BASE_COMPOSE, CHAP_DEVOPS_COMPOSE, ComposeSource, MARKETPLACE_COMPOSE, Project,
    compose_files_for,
};
use crate::registry::Registry;
use config::{ensure_dhis2_config, ensure_ocs_config, ensure_plugins_key};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What [`sync`] did, or with `check` what it would have done.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncReport {
    /// Files created or whose content changed.
    pub written: Vec<PathBuf>,
    /// Files that already had the rendered content.
    pub unchanged: Vec<PathBuf>,
    /// Overlays of models that are no longer enabled.
    pub removed: Vec<PathBuf>,
    /// Whether anything differed from what `.chap-devops/` describes.
    pub drift: bool,
    /// Whether this was a dry run.
    pub check: bool,
    pub warnings: Vec<String>,
}

impl SyncReport {
    /// One line: `2 written, 1 unchanged, 1 removed`.
    pub fn summary(&self) -> String {
        let (write, remove) = if self.check {
            ("to write", "to remove")
        } else {
            ("written", "removed")
        };
        format!(
            "{} {write}, {} unchanged, {} {remove}",
            self.written.len(),
            self.unchanged.len(),
            self.removed.len()
        )
    }
}

/// Render every enabled model's overlay and the umbrella from the project
/// state, remove overlays this CLI wrote for models that are no longer
/// enabled, and append missing `.env` pin comments.
///
/// With `check` nothing is written or removed and the state is left alone;
/// the report says what a real run would do. Otherwise
/// `state.rendered_files` is updated and `.chap-devops/` is saved.
pub fn sync(project: &mut Project, registry: &Registry, check: bool) -> Result<SyncReport> {
    let mut report = SyncReport {
        check,
        ..SyncReport::default()
    };
    let dir = project.dir.clone();
    if !check {
        std::fs::create_dir_all(&dir)
            .map_err(|e| anyhow::anyhow!("creating {}: {e}", dir.display()))?;
        // A restored or hand-made `.env` can be readable by everyone; every
        // sync, and so every `chap-devops up`, closes it to its owner.
        crate::dotenv::protect(&dir.join(crate::project::ENV_FILE));
    }

    // Desired artifacts: the base stack, the overlays in marketplace-id order,
    // then the umbrella that includes them.
    let mut desired: Vec<(String, String)> = Vec::new();
    // The umbrella includes the overlays and only the overlays; the base file
    // is one of the `-f` list, not something to include.
    let mut overlays: Vec<String> = Vec::new();
    let components = project.state.components.clone();
    // The compose project name is settled before anything is rendered: it goes
    // into every chap-devops-owned file. `chap-devops init` records it.
    let project_name = project.compose_project_name();
    // A `chap-devops run` group, which every container's labels name.
    let group = project.state.group.clone();
    // chap-core is a component like the others: with it off, neither the base
    // stack nor the chap-devops-owned override belongs to this deployment, and both
    // are removed below.
    if components.chap_core.enabled {
        let (base, base_warnings) = base_compose(project);
        report.warnings.extend(base_warnings);
        // The services the chap-core labels go on are the ones the base file
        // defines, at whatever tag or checkout it comes from; a base file that
        // could not be rendered is left on disk, so that is where they are.
        let mut chap_devops = ChapDevopsOverlaySpec {
            project_name: project_name.clone(),
            checkout: match &project.state.chap_compose_source {
                ComposeSource::Checkout { path } => Some(path.clone()),
                _ => None,
            },
            group: group.clone(),
            ..ChapDevopsOverlaySpec::new(project.state.api_port)
        };
        if let Some(services) = base
            .clone()
            .or_else(|| std::fs::read_to_string(dir.join(BASE_COMPOSE)).ok())
            .and_then(|text| compose_services(&text))
        {
            chap_devops.services = services;
        }
        if let Some(base) = base {
            desired.push((BASE_COMPOSE.to_string(), base));
        }
        // Always rendered, base file or not: it is the only place the API's
        // host port is decided, and it has to be a `-f` entry of its own
        // because a file in `include:` cannot override a service compose.yml
        // defines.
        desired.push((
            CHAP_DEVOPS_COMPOSE.to_string(),
            render_chap_devops_overlay(&chap_devops),
        ));
    }
    // The components sit between the base stack and the model overlays, in
    // the same order as the `-f` list.
    if components.ocs.enabled {
        desired.push((
            OCS_COMPOSE.to_string(),
            render_ocs(&OcsSpec {
                group: group.clone(),
                ..OcsSpec::from_components(&components, project.ocs_plugins_path().is_dir())
            }),
        ));
    }
    if components.s3.enabled {
        desired.push((
            S3_COMPOSE.to_string(),
            render_s3(&S3Spec {
                group: group.clone(),
                ..S3Spec::from_components(&components)
            }),
        ));
    }
    if components.dhis2.enabled {
        let mut spec = Dhis2Spec {
            group: group.clone(),
            ..Dhis2Spec::from_components(&components)
        };
        if let Some(crate::compose::spec::Dhis2SeedSource::File(path)) = &spec.seed {
            spec.seed_bytes = std::fs::metadata(dir.join(path)).ok().map(|m| m.len());
        }
        let rendered = render_dhis2(&spec);
        // The seed was left at `default` and the pinned minor line publishes no
        // dump chap-devops knows the path of, so this deployment starts empty. Said
        // on the sync that writes the DHIS2 compose file - the first, and any
        // that changes it - because nothing else on the screen would show it;
        // every `chap-devops up` after that would only repeat it.
        let writes_dhis2 = std::fs::read_to_string(dir.join(DHIS2_COMPOSE))
            .map_or(true, |existing| existing != rendered);
        if components.dhis2_seed_is_unknown() && writes_dhis2 {
            report
                .warnings
                .push(crate::components::dhis2_unknown_seed(&spec.image_tag));
        }
        // A dump that is a file is a bind mount, and compose refuses to start a
        // service whose bind source does not exist - so a path that is not there
        // is a `chap-devops up` that fails, reported now instead.
        if let Some(crate::compose::spec::Dhis2SeedSource::File(path)) = &spec.seed
            && !dir.join(path).exists()
        {
            report.warnings.push(format!(
                "the dhis2 seed names {path}, which is not in {}; copy the dump there before \
                 `chap-devops up`, or run `chap-devops components enable dhis2 --seed none`",
                dir.display()
            ));
        }
        desired.push((DHIS2_COMPOSE.to_string(), rendered));
    }
    // Every overlay carries the registration key line when the deployment has
    // a key, because chap-core then requires it from each service that
    // registers. The value stays in `.env`, which compose reads from the
    // project directory, so nothing has to be threaded into the overlays.
    let registration_key = project.state.auth.registration_key;
    let standalone = !project
        .state
        .components
        .is_enabled(crate::components::Component::ChapCore);
    let external = project
        .state
        .components
        .chap_core_external
        .as_ref()
        .map(|external| crate::compose::spec::ExternalRegistration {
            register_url: external
                .url_from_containers()
                .trim_end_matches('/')
                .to_string(),
            models_host: external.models_host.clone(),
        });
    for (id, model) in &project.state.models {
        let mut spec = match registry.get(id) {
            Some(m) => OverlaySpec::from_enabled(id, model, m),
            None => {
                report.warnings.push(format!(
                    "{id} is enabled but not in the registry; its overlay header \
                     names the image in place of the repository"
                ));
                OverlaySpec::from_enabled_without_registry(id, model)
            }
        };
        spec.registration_key = registration_key;
        spec.standalone = standalone;
        spec.external_chap_core = external.clone();
        spec.bind = model.bind.or(project.state.model_bind);
        spec.group = group.clone();
        // A chap-core elsewhere calls the model back on its host port. On
        // Linux, a container reaches the host through the docker bridge, and
        // a port published on loopback does not answer there. Docker Desktop
        // routes `host.docker.internal` to the host's loopback, so a chap-core
        // container on macOS reaches it (measured), and the note is Linux's.
        if let (Some(_), Some(bind), Some(port)) = (&external, spec.bind, spec.host_port)
            && bind.is_loopback()
            && cfg!(target_os = "linux")
        {
            report.warnings.push(format!(
                "{id} is published on {bind}:{port} only, which the chap-core it registers \
                 with cannot call back; run `chap-devops models expose {id} --bind 0.0.0.0`"
            ));
        }
        // The overlay's init container chowns the data volume from busybox,
        // which resolves no account name of its own, so the user has to be
        // expressible as numbers. An unknown one still renders, with the
        // chapkit ids, but the operator should know the guess was taken. A
        // model that runs as root resolves to `0:0`, so it never lands here.
        if overrides::numeric_pair(&spec.user).is_none() {
            report.warnings.push(format!(
                "{id} runs as `{}`, which has no known uid:gid; the volume init \
                 container will chown {} to {} instead - pass `--user <uid>:<gid>` \
                 if that is wrong",
                spec.user,
                spec.data_dir,
                overrides::FALLBACK_UID_GID
            ));
        }
        // A hand-edited `models-manual.yaml` never passed `models add`'s check.
        crate::manual::source::check_not_reserved(&model.service_id)
            .map_err(|e| anyhow::anyhow!("{id}: {e:#}"))?;
        overlays.push(model.compose_file.clone());
        desired.push((model.compose_file.clone(), render_overlay(&spec)));
    }
    desired.push((
        MARKETPLACE_COMPOSE.to_string(),
        render_umbrella(&overlays, project_name.as_deref()),
    ));

    // Two outputs of one name would be one file, the second written over the
    // first; refused before anything is written.
    let mut seen = BTreeSet::new();
    for (name, _) in &desired {
        if !seen.insert(name.as_str()) {
            return Err(anyhow::anyhow!(
                "two parts of this deployment would both be written to `{name}`; give one of \
                 the models another `service_id:` in `.chap-devops/models-manual.yaml`, then `chap-devops sync`"
            ));
        }
    }

    for (name, content) in &desired {
        let path = dir.join(name);
        let same = std::fs::read_to_string(&path).ok().as_deref() == Some(content.as_str());
        crate::output::verbose(&format!(
            "compared {} ({} rendered bytes): {}",
            path.display(),
            content.len(),
            if same { "unchanged" } else { "differs" }
        ));
        if same {
            report.unchanged.push(path);
            continue;
        }
        if !check {
            std::fs::write(&path, content)
                .map_err(|e| anyhow::anyhow!("writing {}: {e}", path.display()))?;
        }
        report.written.push(path);
    }

    // Only overlays a previous sync wrote are candidates for removal, and
    // only when their service is gone: a hand-written compose.*.yml is never
    // ours to delete. The base stack is the one exception, and a narrow one:
    // it goes when chap-core itself was turned off, never because it failed to
    // render (a base stack we cannot reproduce is not one to delete either).
    let wanted: BTreeSet<&str> = desired.iter().map(|(f, _)| f.as_str()).collect();
    let doomed: BTreeSet<&str> = if components.chap_core.enabled {
        BTreeSet::new()
    } else {
        BTreeSet::from([BASE_COMPOSE, CHAP_DEVOPS_COMPOSE])
    };
    for name in &project.state.rendered_files {
        if wanted.contains(name.as_str()) {
            continue;
        }
        if !is_overlay_name(name) && !doomed.contains(name.as_str()) {
            continue;
        }
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        if !check {
            std::fs::remove_file(&path)
                .map_err(|e| anyhow::anyhow!("removing {}: {e}", path.display()))?;
        }
        report.removed.push(path);
    }

    if let Some(env) = append_env_pins(project, check)? {
        report.written.push(env);
    }
    // The OCS instance config is scaffolded, not rendered: it is the
    // operator's file the moment it exists, so sync only ever creates a
    // missing one.
    if let Some(path) = ensure_ocs_config(project, check)? {
        report.written.push(path);
    }
    // And DHIS2's, on exactly the same terms: DHIS2 will not start without
    // `dhis.conf`, so a missing one is scaffolded, and one that is there is the
    // operator's.
    if let Some(path) = ensure_dhis2_config(project, check)? {
        report.written.push(path);
    }
    // And once it is there, the one key that has to follow the project rather
    // than the operator: a plugin directory that is mounted and not configured
    // is a mount OCS never looks in.
    if let Some(path) = ensure_plugins_key(project, check)?
        && !report.written.contains(&path)
    {
        report.written.push(path);
    }

    report.drift = !report.written.is_empty() || !report.removed.is_empty();
    if check {
        return Ok(report);
    }

    // The `-f` list follows the components: an enabled component puts its
    // files in the list, and a disabled one takes them out.
    project.state.compose_files = compose_files_for(&components);
    project.state.rendered_files = desired.into_iter().map(|(f, _)| f).collect();
    project.save()?;
    Ok(report)
}

/// The `compose.yml` this project's state describes, plus any warning about
/// the source it is rendered from.
///
/// `None` means the recorded source cannot be replayed - the cached copy of
/// chap-core's `compose.ghcr.yml` is gone - in which case the file on disk is
/// left exactly as it is: a base stack we cannot reproduce is not one to
/// overwrite with a guess.
fn base_compose(project: &Project) -> (Option<String>, Vec<String>) {
    let embedded = || render_base(&BaseSpec { upstream: None });
    if let ComposeSource::Checkout { path } = &project.state.chap_compose_source {
        let file = Path::new(path).join(crate::project::CHECKOUT_COMPOSE);
        return match std::fs::read_to_string(&file) {
            Ok(body) => (
                Some(render_base(&BaseSpec {
                    upstream: Some(UpstreamCompose {
                        tag: format!("the checkout at {path}"),
                        body,
                    }),
                })),
                Vec::new(),
            ),
            Err(_) => (
                None,
                vec![format!(
                    "{} is missing, so {BASE_COMPOSE} is left as it is; check the chap-core \
                     checkout is still at {path}, or re-run `chap-devops init --force --source PATH`",
                    file.display()
                )],
            ),
        };
    }
    let ComposeSource::Fetched { tag, sha256, .. } = &project.state.chap_compose_source else {
        return (Some(embedded()), Vec::new());
    };

    let Some(path) = project.cached_compose_path() else {
        return (Some(embedded()), Vec::new());
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Ok(body) = std::fs::read_to_string(&path) else {
        return (
            None,
            vec![format!(
                ".chap-devops/{name} is missing, so {BASE_COMPOSE} is left as it is; \
                 re-run `chap-devops init --force --chap-tag {tag}` to fetch it again"
            )],
        );
    };

    let mut warnings = Vec::new();
    if crate::chapcore::sha256_hex(body.as_bytes()) != *sha256 {
        warnings.push(format!(
            ".chap-devops/{name} no longer matches the checksum recorded in .chap-devops/project.yaml; \
             {BASE_COMPOSE} follows the file as it is now"
        ));
    }
    let text = render_base(&BaseSpec {
        upstream: Some(UpstreamCompose {
            tag: tag.clone(),
            body,
        }),
    });
    (Some(text), warnings)
}

/// `compose.<something>.yml`, the shape of a model overlay.
fn is_overlay_name(name: &str) -> bool {
    name.starts_with("compose.")
        && name.ends_with(".yml")
        && name != MARKETPLACE_COMPOSE
        && name != BASE_COMPOSE
        && name != CHAP_DEVOPS_COMPOSE
}

#[cfg(test)]
mod tests;
