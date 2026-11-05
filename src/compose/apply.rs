//! The single state-edit path shared by `init`, `enable`, `disable` and the
//! TUI. It updates `.chap-devops/models.yaml` in memory and then hands over to
//! [`crate::compose::sync()`], which renders the compose files and saves.

use crate::components::{Component, Components};
use crate::compose::overlay_filename;
use crate::compose::overrides::{DEFAULT_DATA_DIR, DEFAULT_USER, known_override};
use crate::compose::ports::allocator_for;
use crate::compose::resolve::{self, ResolveFn, UserSource};
use crate::compose::spec::OverlaySpec;
use crate::compose::sync::sync;
use crate::error::{ChapError, Result};
use crate::manual::Endpoints;
use crate::project::{EnabledModel, Project};
use crate::registry::{Registry, VersionSelector};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// What a caller wants done with one model's host port.
///
/// A model needs none to work, so the whole type is opt-in: it only appears
/// when someone typed `--port`, `expose` or pressed `p` in the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PortRequest {
    /// Publish nothing: the overlay only `expose`s port 8000, chap-core
    /// reaches the model over the compose network, and a human goes through
    /// chap-core's proxy. This is what a fresh `models enable` does.
    #[default]
    None,
    /// Publish on the lowest port in the project's range that no compose file
    /// claims and nothing is listening on.
    Auto,
    /// Publish on exactly this port, or fail saying who has it.
    Fixed(u16),
}

/// One model the caller wants enabled, with any explicit overrides.
#[derive(Debug, Clone)]
pub struct EnableRequest {
    pub id: String,
    pub selector: VersionSelector,
    /// `None` leaves the model's host port exactly as it is - which for a
    /// model being enabled for the first time means none at all. `Some` is an
    /// explicit decision, [`PortRequest::None`] included.
    pub port: Option<PortRequest>,
    /// The host address to publish the port on; `None` keeps the one the
    /// model has, which for a new model is the deployment's default.
    pub bind: Option<std::net::IpAddr>,
    pub data_dir: Option<String>,
    pub user: Option<String>,
    /// Where [`user`] came from, for a caller that resolved it already.
    ///
    /// `chap-devops models add` reads the image itself and hands the answer over;
    /// leaving this `None` records the flag as [`UserSource::Flag`], which is
    /// what it is for everyone who typed `--user`.
    ///
    /// [`user`]: EnableRequest::user
    pub user_from: Option<UserSource>,
    pub allow_starter: bool,
    /// Leave the version the project recorded exactly as it is, [`selector`]
    /// included.
    ///
    /// This is what a request that changes something *around* an enabled model
    /// carries: publishing a host port is no reason to move the pin a running
    /// deployment follows, and re-resolving the channel here would upgrade it
    /// as a side effect. Ignored for a model that is not enabled yet, which
    /// has no version to keep.
    ///
    /// [`selector`]: EnableRequest::selector
    pub keep_version: bool,
}

impl EnableRequest {
    /// A request with no overrides, following the model's stable channel.
    pub fn new(id: impl Into<String>) -> EnableRequest {
        EnableRequest {
            id: id.into(),
            selector: VersionSelector::default(),
            port: None,
            bind: None,
            data_dir: None,
            user: None,
            user_from: None,
            allow_starter: false,
            keep_version: false,
        }
    }
}

/// A batch of enables, disables and a component set applied together.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub enable: Vec<EnableRequest>,
    pub disable: Vec<String>,
    /// The component set the caller wants, or `None` to leave it alone.
    ///
    /// A `Some` that matches what the project already has applies as a no-op:
    /// [`apply_with`] compares the two and reports neither an enable nor a
    /// disable. Whether such a set counts as a change at all is the producer's
    /// question, because only the producer knows the set it started from - the
    /// browser sets this only once a session has moved something, so a
    /// component toggled off and on again still saves nothing.
    pub components: Option<Components>,
}

impl Selection {
    /// Whether the selection would change anything at all.
    ///
    /// A wanted component set counts: nothing here can see the project, so a
    /// caller with nothing to say about components leaves it `None`.
    pub fn is_empty(&self) -> bool {
        self.enable.is_empty() && self.disable.is_empty() && self.components.is_none()
    }
}

/// What [`apply`] changed.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApplyReport {
    pub enabled: Vec<(String, EnabledModel)>,
    pub updated: Vec<(String, EnabledModel)>,
    /// Models the selection asked for that were already enabled exactly so.
    pub unchanged: Vec<(String, EnabledModel)>,
    pub disabled: Vec<String>,
    /// Components this run turned on, with the host port each publishes.
    pub components_enabled: Vec<(String, Option<u16>)>,
    /// Components it turned off, by name.
    pub components_disabled: Vec<String>,
    pub written: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

impl ApplyReport {
    /// Whether anything changed on disk or in state.
    pub fn is_empty(&self) -> bool {
        self.enabled.is_empty()
            && self.updated.is_empty()
            && self.disabled.is_empty()
            && self.components_enabled.is_empty()
            && self.components_disabled.is_empty()
            && self.written.is_empty()
            && self.removed.is_empty()
    }

    /// Every model the run touched, newly enabled first.
    pub fn touched(&self) -> impl Iterator<Item = &(String, EnabledModel)> {
        self.enabled.iter().chain(self.updated.iter())
    }
}

/// Check a whole selection against the registry and the project state,
/// changing nothing.
///
/// [`apply`] starts here, so every caller gets these answers before a file is
/// written. `chap-devops init --force` calls it directly as well: it deletes the
/// previous deployment's overlays before handing over to [`apply`], and a
/// selection that was never going to work must not take them with it.
pub fn validate(project: &Project, registry: &Registry, sel: &Selection) -> Result<()> {
    for wanted in &sel.disable {
        if enabled_id(project, wanted).is_none() {
            return Err(ChapError::UnknownModel(wanted.clone()).into());
        }
    }
    for req in &sel.enable {
        let model = registry
            .get(&req.id)
            .ok_or_else(|| ChapError::UnknownModel(req.id.clone()))?;
        if model.is_starter() && !req.allow_starter {
            return Err(ChapError::IsStarter(req.id.clone()).into());
        }
        // Two models on one compose service would be merged into one by
        // compose, so a model whose service name another enabled model holds
        // is refused. A local build standing in for a marketplace model is
        // the case this catches in practice.
        if let Some((other, _)) = project.state.models.iter().find(|(other, e)| {
            **other != model.id && e.service_id == model.service_id && !sel.disable.contains(*other)
        }) {
            return Err(anyhow::anyhow!(
                "{other} already runs as the compose service `{}`; disable it first with \
                 `chap-devops models disable {other}`",
                model.service_id
            ));
        }
        // A request that keeps the recorded version asks the registry for
        // nothing; every other one needs a version that exists and is not
        // yanked.
        if !(req.keep_version && project.state.models.contains_key(&model.id)) {
            model.resolve(&req.selector)?;
        }
        // Two requests in one selection on one service would render to one
        // overlay, the second written over the first.
        if let Some(twin) = sel.enable.iter().find(|other| {
            other.id != req.id
                && registry
                    .get(&other.id)
                    .is_some_and(|o| o.service_id == model.service_id)
        }) {
            return Err(anyhow::anyhow!(
                "{} and {} both run as the compose service `{}`; enable one of them",
                req.id,
                twin.id,
                model.service_id
            ));
        }
        if let Some(dir) = &req.data_dir {
            check_data_dir(dir)?;
        }
        if let Some(user) = &req.user {
            check_user(user)?;
        }
    }
    Ok(())
}

/// A `--data-dir` that goes into the rendered YAML and the init container's
/// `chown` as it is: an absolute path of characters neither YAML nor a shell
/// reads as anything but a path.
pub fn check_data_dir(dir: &str) -> Result<()> {
    let ok = dir.starts_with('/')
        && dir
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'));
    if !ok {
        return Err(ChapError::Usage(format!(
            "`{dir}` is not a data directory chap-devops can render; give an absolute path of \
             letters, digits, `/`, `.`, `_` and `-`, such as `--data-dir /app/data`"
        ))
        .into());
    }
    Ok(())
}

/// A `--user` as compose and `chown` take it: a name or a numeric id,
/// optionally with a group after a `:`.
pub fn check_user(user: &str) -> Result<()> {
    let part = |p: &str| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    let ok = match user.split_once(':') {
        Some((name, group)) => part(name) && part(group),
        None => part(user),
    };
    if !ok {
        return Err(ChapError::Usage(format!(
            "`{user}` is not a user chap-devops can render; give a name or an id, with a group after \
             a `:` if needed, such as `--user 1000:1000`"
        ))
        .into());
    }
    Ok(())
}

/// Apply a selection: disable first, then enable, then sync the compose
/// files and `.chap-devops/` from the new state.
///
/// Nothing is written until the whole selection has resolved, so an unknown
/// model or a port clash leaves the directory as it was.
///
/// `endpoints` is where the user of a newly enabled marketplace model is read
/// from: the registry, or the local daemon, or - for an `--offline` run that
/// has neither - the table compiled into this binary. See
/// [`crate::compose::resolve`].
pub fn apply(
    project: &mut Project,
    registry: &Registry,
    sel: &Selection,
    endpoints: &Endpoints,
) -> Result<ApplyReport> {
    apply_with(project, registry, sel, &crate::ports::is_busy, &|req| {
        resolve::from_image(req, endpoints)
    })
}

/// [`apply`] with the host port probe and the user resolution injected, so
/// tests can decide what the machine is listening on without binding anything
/// and what an image declares without pulling one.
pub fn apply_with(
    project: &mut Project,
    registry: &Registry,
    sel: &Selection,
    busy: &dyn Fn(u16) -> bool,
    resolve: ResolveFn,
) -> Result<ApplyReport> {
    let report = plan_with(project, registry, sel, busy, resolve)?;
    write_planned(project, registry, report)
}

/// Render and save what [`plan_with`] decided.
pub fn write_planned(
    project: &mut Project,
    registry: &Registry,
    mut report: ApplyReport,
) -> Result<ApplyReport> {
    // One rendering path: sync writes the overlays, the umbrella and the .env
    // pins, removes the overlays of disabled models, and saves .chap-devops/.
    let synced = sync(project, registry, false)?;
    report.written = synced.written;
    report.removed = synced.removed;
    report.warnings.extend(synced.warnings);
    Ok(report)
}

/// Everything [`apply`] decides, made on `project` in memory and nothing
/// written: the checks, the ports and the users. What fails, fails here,
/// before [`write_planned`] touches a file - so a caller with something to do
/// in between (`chap-devops ui` stopping the components it switches off) does it
/// only for a selection that will apply.
pub(crate) fn plan_with(
    project: &mut Project,
    registry: &Registry,
    sel: &Selection,
    busy: &dyn Fn(u16) -> bool,
    resolve: ResolveFn,
) -> Result<ApplyReport> {
    validate(project, registry, sel)?;
    let mut report = ApplyReport::default();

    // Disable first: a model can be removed and another put on its port in
    // the same call, and re-enabling one must see its own slot as free. The
    // overlay itself is removed by sync, which knows the file was ours.
    let mut freed: BTreeSet<u16> = BTreeSet::new();
    for wanted in &sel.disable {
        let id =
            enabled_id(project, wanted).ok_or_else(|| ChapError::UnknownModel(wanted.clone()))?;
        let entry = project
            .state
            .models
            .remove(&id)
            .expect("enabled_id only returns keys that are present");
        freed.extend(entry.host_port);
        report.disabled.push(id);
    }

    // A model being re-enabled keeps its own port: its overlay is about to be
    // rewritten, so the port it publishes today is not a conflict.
    for req in &sel.enable {
        if let Some(id) = enabled_id(project, &req.id)
            && let Some(existing) = project.state.models.get(&id)
        {
            freed.extend(existing.host_port);
        }
    }
    let mut allocator = allocator_for(project, &freed)?;
    // Without chap-core nothing in the deployment reaches a model over the
    // compose network, so a model left without a host port could not be
    // reached at all: every one gets a published port unless the caller
    // explicitly asked for none.
    let standalone = !sel
        .components
        .as_ref()
        .unwrap_or(&project.state.components)
        .is_enabled(Component::ChapCore);

    for req in &sel.enable {
        let model = registry
            .get(&req.id)
            .ok_or_else(|| ChapError::UnknownModel(req.id.clone()))?;
        // A starter nobody allowed was refused by validate() above; what is
        // left is to say out loud that this one was allowed.
        if model.is_starter() {
            report.warnings.push(format!(
                "{} is a starter, not a deployable model; it is scaffolding to copy",
                model.id
            ));
        }
        let existing = project.state.models.get(&model.id).cloned();
        // What a `keep_version` request stands on: the entry as the project
        // recorded it, pin and all. A model that is not enabled yet has
        // nothing to keep, so it resolves like any other.
        let pinned = existing.clone().filter(|_| req.keep_version);

        let host_port = match req.port {
            // No decision made: keep whatever the model has, which for a new
            // one is no host port at all. A port it already had is kept even
            // if a later --port-base narrowed the range around it.
            None => {
                let kept = existing.as_ref().and_then(|e| e.host_port);
                if let Some(port) = kept {
                    allocator.reserve(port);
                }
                match kept {
                    None if standalone => Some(allocator.allocate(busy)?),
                    kept => kept,
                }
            }
            Some(PortRequest::None) => None,
            Some(PortRequest::Auto) => Some(allocator.allocate(busy)?),
            Some(PortRequest::Fixed(port)) => {
                allocator.claim(port, busy)?;
                Some(port)
            }
        };

        let bind = req.bind.or_else(|| existing.as_ref().and_then(|e| e.bind));

        let known = known_override(&model.id);
        // A manually added model carries its own answers: `models add` read
        // them off the image when the entry was written, and the built-in
        // table cannot have an entry for an image this CLI has never seen.
        let manual = project.state.manual.get(&model.id).cloned();
        // A marketplace model's user comes from the image itself, re-read on
        // every enable, because an image can change what it runs as between
        // two tags. A port-only change (`keep_version`) asks nothing: it is
        // not about the image, and re-resolving here would make publishing a
        // port a network operation.
        let resolved = match (&manual, &pinned) {
            (None, None) => {
                let version = model.resolve(&req.selector)?;
                Some(resolve(&resolve::Request {
                    id: &model.id,
                    image: &model.source.image,
                    image_tag: &version.image_tag,
                    image_digest: version.image_digest.as_deref(),
                    data_dir_flag: req.data_dir.as_deref(),
                    user_flag: req.user.as_deref(),
                }))
            }
            _ => None,
        };
        let (data_dir, user, user_from, reads_port) = match &resolved {
            Some(resolution) => {
                report.warnings.extend(resolution.notes.iter().cloned());
                (
                    resolution.data_dir.clone(),
                    resolution.user.clone(),
                    resolution.user_from,
                    resolution.reads_port,
                )
            }
            None => {
                let data_dir = req
                    .data_dir
                    .clone()
                    .or_else(|| existing.as_ref().map(|e| e.data_dir.clone()))
                    .or_else(|| manual.as_ref().and_then(|m| m.data_dir.clone()))
                    .or_else(|| known.map(|k| k.data_dir.to_string()))
                    .unwrap_or_else(|| DEFAULT_DATA_DIR.to_string());
                let user = req
                    .user
                    .clone()
                    .or_else(|| existing.as_ref().map(|e| e.user.clone()))
                    .or_else(|| manual.as_ref().and_then(|m| m.user.clone()))
                    .or_else(|| known.map(|k| k.user.to_string()))
                    .unwrap_or_else(|| DEFAULT_USER.to_string());
                let user_from = req
                    .user_from
                    .or_else(|| req.user.as_ref().map(|_| UserSource::Flag))
                    .or_else(|| existing.as_ref().map(|e| e.user_from))
                    .unwrap_or_default();
                let reads_port = existing
                    .as_ref()
                    .map(|e| e.reads_port)
                    .or_else(|| manual.as_ref().map(|m| m.reads_port))
                    .unwrap_or(false);
                (data_dir, user, user_from, reads_port)
            }
        };

        let entry = match pinned {
            // A port change, and nothing about the image: the recorded pin,
            // the channel it follows and the platform all stay as they are.
            Some(previous) => EnabledModel {
                host_port,
                bind,
                data_dir,
                user,
                user_from,
                ..previous
            },
            None => {
                let version = model.resolve(&req.selector)?;
                let spec = OverlaySpec::from_model(
                    model,
                    version,
                    host_port,
                    Some(&data_dir),
                    Some(&user),
                );
                EnabledModel {
                    previous: None,
                    reads_port,
                    service_id: model.service_id.clone(),
                    image: model.source.image.clone(),
                    image_tag: version.image_tag.clone(),
                    image_digest: version.image_digest.clone(),
                    version: version.version.clone(),
                    channel: match &req.selector {
                        VersionSelector::Channel(c) => Some(*c),
                        VersionSelector::Exact(_) => None,
                    },
                    host_port,
                    bind,
                    data_dir,
                    user,
                    user_from,
                    platform: spec.platform.clone(),
                    compose_file: overlay_filename(&model.service_id),
                }
            }
        };
        if existing.as_ref() == Some(&entry) {
            report.unchanged.push((model.id.clone(), entry.clone()));
        } else if existing.is_some() {
            report.updated.push((model.id.clone(), entry.clone()));
        } else {
            report.enabled.push((model.id.clone(), entry.clone()));
        }
        project.state.models.insert(model.id.clone(), entry);
    }

    // A chap-core elsewhere is reached from a model that listens on its host
    // port, which an image that hard-codes 8000 cannot do: said now, since the
    // symptom later is only a model that never registers.
    let external = sel
        .components
        .as_ref()
        .unwrap_or(&project.state.components)
        .chap_core_external
        .is_some();
    if external {
        for id in sel.enable.iter().map(|req| req.id.as_str()) {
            if crate::compose::overrides::FIXED_PORT_MODELS.contains(&id) {
                report.warnings.push(format!(
                    "{id} starts on port 8000 whatever PORT says, so it cannot register with \
                     the chap-core elsewhere this deployment uses; run it with chap-devops's own \
                     chap-core (`chap-devops components enable chap-core`), or pick another model"
                ));
            }
        }
    }

    // The same for the models this selection does not mention, when it is the
    // one that takes chap-core away: they keep running, and need a way in.
    if standalone {
        let asked: BTreeSet<&str> = sel.enable.iter().map(|r| r.id.as_str()).collect();
        let unreachable: Vec<String> = project
            .state
            .models
            .iter()
            .filter(|(id, e)| e.host_port.is_none() && !asked.contains(id.as_str()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in unreachable {
            let port = allocator.allocate(busy)?;
            let entry = project
                .state
                .models
                .get_mut(&id)
                .expect("collected from the same map");
            entry.host_port = Some(port);
            report.updated.push((id, entry.clone()));
        }
    }

    // The wanted component set goes in before the sync, so the component
    // compose files are rendered by the same run that renders the overlays:
    // one rendering path stays one rendering path.
    if let Some(wanted) = &sel.components {
        let before = project.state.components.clone();
        for component in Component::ALL {
            match (before.is_enabled(*component), wanted.is_enabled(*component)) {
                (false, true) => report
                    .components_enabled
                    .push((component.name().to_string(), wanted.port_of(*component))),
                (true, false) => report
                    .components_disabled
                    .push(component.name().to_string()),
                _ => {}
            }
        }
        project.state.components = wanted.clone();
        // A component publishes its host port straight from
        // `.chap-devops/components.yaml`, so every one of them gets the same "is
        // anything listening on it" question a model's port does. It is a
        // warning and never a refusal, which is what `init` does with a busy
        // component port too; a port one of this deployment's own running
        // services already holds is not a conflict, and
        // `component_port_warning` is where that is known.
        for component in Component::ALL {
            if let Some(port) = wanted.port_of(*component)
                && let Some(line) =
                    crate::ports::component_port_warning(project, *component, port, busy)
            {
                report.warnings.push(line);
            }
        }
    }

    Ok(report)
}

/// The state key for a marketplace id or a compose service id, when that
/// model is currently enabled.
fn enabled_id(project: &Project, wanted: &str) -> Option<String> {
    if project.state.models.contains_key(wanted) {
        return Some(wanted.to_string());
    }
    project
        .state
        .models
        .iter()
        .find(|(_, m)| m.service_id == wanted)
        .map(|(id, _)| id.clone())
}

#[cfg(test)]
mod tests;
