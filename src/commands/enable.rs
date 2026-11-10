//! `chap-devops models enable|disable` — the two single-model write commands.
//!
//! Both go through [`crate::compose::apply()`], the same path `init` and the
//! TUI use.

use crate::cli::{ModelsDisableArgs, ModelsEnableArgs};
use crate::commands::Ctx;
use crate::compose::{ApplyReport, EnableRequest, PortRequest, Selection, apply};
use crate::error::{ChapError, Result};
use crate::output::Report;
use crate::project::Project;
use crate::registry::{Channel, Registry, VersionSelector};

mod port;

pub use port::{expose, unexpose};

/// Enable one or more models: resolve each version, write the overlays, sync
/// the compose files and update .chap-devops/models.yaml.
///
/// No host port unless `--port` asks for one: chap-core reaches the service
/// over the compose network, which is the URL the service registers.
pub fn enable(ctx: &Ctx, args: &ModelsEnableArgs) -> Result<()> {
    let (mut project, _lock) = ctx.project_mut()?;
    let report = enable_in(ctx, &mut project, args)?;
    let changed = Changed::new(&report, &report, &project);
    ctx.out
        .report_ok(&changed, |lines| summary(&report, &[], &project, lines))
}

/// [`enable`] on a project the caller has loaded and locked, writing the
/// state and printing nothing.
pub(crate) fn enable_in(
    ctx: &Ctx,
    project: &mut Project,
    args: &ModelsEnableArgs,
) -> Result<ApplyReport> {
    let registry = super::registry_for(ctx, Some(project))?;

    if let Some(why) = one_model_only(args) {
        return Err(ChapError::Usage(why).into());
    }

    // --version and --channel conflict in the parser, so at most one is set.
    let selector = match (&args.version, args.channel) {
        (Some(version), _) => VersionSelector::Exact(version.clone()),
        (None, Some(channel)) => VersionSelector::Channel(channel),
        (None, None) => VersionSelector::Channel(Channel::Stable),
    };
    let selection = Selection {
        enable: args
            .ids
            .iter()
            .map(|id| EnableRequest {
                id: id.clone(),
                selector: selector.clone(),
                port: args.port.map(|p| p.0),
                bind: args.bind,
                data_dir: args.data_dir.clone(),
                user: args.user.clone(),
                user_from: None,
                allow_starter: args.allow_starter,
                // `models enable` is where a version is decided, so it
                // always resolves: that is what `--version` and `--channel`
                // are for, and a bare run follows the stable channel.
                keep_version: false,
            })
            .collect(),
        ..Selection::default()
    };

    let endpoints = crate::manual::Endpoints::from_env(ctx.registry.offline);
    apply(project, &registry, &selection, &endpoints)
}

/// Why the options cannot go with several ids, if they cannot: a version, a
/// fixed port, a data directory and a user belong to one model.
pub(crate) fn one_model_only(args: &ModelsEnableArgs) -> Option<String> {
    if args.ids.len() < 2 {
        return None;
    }
    let fixed_port = matches!(args.port, Some(crate::cli::PortArg(PortRequest::Fixed(_))));
    let option = match () {
        _ if args.version.is_some() => "--version",
        _ if fixed_port => "--port PORT",
        _ if args.data_dir.is_some() => "--data-dir",
        _ if args.user.is_some() => "--user",
        _ => return None,
    };
    Some(format!(
        "{option} is for one model, and this names {}; enable that model alone with \
         `chap-devops models enable ID {option}`",
        args.ids.len()
    ))
}

/// One model a command left enabled, the way a `--json` caller wants it: the
/// two identifiers, the port and where it answers.
#[derive(Debug, serde::Serialize)]
pub(crate) struct ModelRef {
    pub(crate) id: String,
    pub(crate) service_id: String,
    pub(crate) port: Option<u16>,
    pub(crate) bind: Option<std::net::IpAddr>,
    /// The model's own host port, else chap-core's proxy to it, else `null`
    /// for a model nothing outside the compose network reaches.
    pub(crate) url: Option<String>,
}

/// A change report with [`ModelRef`]s for the models it enabled or updated.
#[derive(Debug, serde::Serialize)]
pub(crate) struct Changed<'a, T: serde::Serialize> {
    pub(crate) models: Vec<ModelRef>,
    #[serde(flatten)]
    pub(crate) report: &'a T,
}

impl<'a, T: serde::Serialize> Changed<'a, T> {
    pub(crate) fn new(report: &'a T, apply: &ApplyReport, project: &Project) -> Changed<'a, T> {
        let models = apply
            .enabled
            .iter()
            .chain(&apply.updated)
            .chain(&apply.unchanged)
            .map(|(id, model)| ModelRef::of(id, model, project))
            .collect();
        Changed { models, report }
    }
}

impl ModelRef {
    pub(crate) fn of(
        id: &str,
        model: &crate::project::EnabledModel,
        project: &Project,
    ) -> ModelRef {
        ModelRef {
            id: id.to_string(),
            service_id: model.service_id.clone(),
            port: model.host_port,
            bind: model.bind.or(project.state.model_bind),
            url: match model.host_port {
                Some(port) => Some(format!("http://localhost:{port}")),
                None if project.state.components.has_chap_core_api() => {
                    Some(project.proxy_url(&model.service_id))
                }
                None => None,
            },
        }
    }
}

/// Disable one model: stop its container, remove its overlay, regenerate the
/// umbrella file and update .chap-devops/models.yaml.
///
/// The model's data volume is kept, which is what makes disabling a model a
/// reversible thing to do. It is named either way, because once the overlay
/// is gone nothing else knows it: `chap-devops down --volumes` removes the volumes
/// the compose files still declare, so a volume nobody names again
/// lingers for as long as the machine does. `--purge` is how the data goes
/// with the model.
pub fn disable(ctx: &Ctx, args: &ModelsDisableArgs) -> Result<()> {
    let (mut project, _lock) = ctx.project_mut()?;
    let registry = super::registry_for(ctx, Some(&project))?;

    // Accept either identifier, but only for a model this project enabled:
    // disabling something that was never on is a typo, not a no-op. With
    // `--purge` it is neither, because the volume outlives the model it
    // belonged to and removing it is what the flag is for.
    let Some(id) = enabled_id(&project, &args.id) else {
        if args.retire {
            return Err(anyhow::anyhow!(
                "{} is not enabled here, so there is no model to retire; run the command \
                 without --retire",
                args.id
            ));
        }
        if args.purge {
            return purge_only(ctx, &project, &registry, &args.id);
        }
        return Err(ChapError::UnknownModel(args.id.clone()).into());
    };
    let service_id = project.state.models[&id].service_id.clone();
    let pending = match args.retire {
        true => retire::prepare(&project, &id, &service_id, true, "run it without --retire")?,
        false => None,
    };
    let (mut report, notes) = disable_enabled(&mut project, &registry, &id, args.purge, false)?;
    report.retired = retire::finish(pending);
    ctx.out.report_ok(&report, |lines| {
        summary(&report.apply, &notes, &project, lines);
        if let Some(retired) = &report.retired {
            retire::line(&id, retired, lines);
        }
    })
}

/// Disable a model this project has enabled: stop its container, remove its
/// overlay and take it out of `.chap-devops/models.yaml`, then deal with its data
/// volume.
///
/// The half of [`disable`] that `chap-devops models remove` needs too: removing a
/// manually added model has to disable it first, and doing that by any other
/// path would leave the container running and the volume unnamed. Returns
/// what was done plus the notes that belong in the closing lines. `removing`
/// is set by `chap-devops models remove`, after which the id no longer names
/// anything a later command could purge.
pub(crate) fn disable_enabled(
    project: &mut Project,
    registry: &Registry,
    id: &str,
    purge: bool,
    removing: bool,
) -> Result<(DisableReport, Vec<String>)> {
    let id = id.to_string();
    let service_id = project.state.models[&id].service_id.clone();
    let selection = Selection {
        disable: vec![id.clone()],
        ..Selection::default()
    };
    // Nothing is written before this passes, so the container is only touched
    // for a disable that is going through.
    crate::compose::apply::validate(project, registry, &selection)?;

    // The container goes now, while compose still has the overlay that
    // defines it: a service whose definition has just been deleted cannot be
    // stopped by name, and one left running keeps its host port published
    // long after the model was disabled.
    let stopped = super::docker::stop_and_remove(project, &|service| {
        // The model's own container, and the exited one-shot companion that
        // handed its volume over before it started.
        service.strip_suffix("-init").unwrap_or(service) == service_id
    });

    // And the volume after them: docker refuses to remove one a container
    // still has mounted, so the order here is the difference between a purge
    // that works and one that reports "volume is in use".
    let mut notes: Vec<String> = stopped.iter().cloned().collect();
    let volume = project.prefixed_volume(&crate::compose::volume_name(&id));
    let mut purged = Vec::new();
    let mut kept_volumes = Vec::new();
    match (&volume, purge) {
        (Some(name), true) => {
            let (removed, line) = super::docker::purge_volume(name);
            purged.extend(removed);
            notes.push(line);
        }
        // A model that was never started has no volume to have kept.
        (Some(name), false) if crate::docker::volume_exists(name) => {
            // After `models remove` the id is gone, so a `models disable
            // --purge` would only answer "unknown model".
            let again = (!removing).then(|| format!("chap-devops models disable {id}"));
            notes.push(super::docker::kept_volume_line(name, again.as_deref()));
            kept_volumes.push(name.clone());
        }
        (None, true) => notes.push(super::docker::UNNAMEABLE_VOLUME.to_string()),
        (_, false) => {}
    }

    let report = DisableReport {
        // A disable enables nothing, so nothing here is ever looked up; the
        // offline endpoints say so rather than leaving it to chance.
        apply: apply(project, registry, &selection, &offline_endpoints())?,
        stopped,
        purged,
        kept_volumes,
        retired: None,
    };
    Ok((report, notes))
}

/// `--purge` for a model this project does not have enabled.
///
/// The volume is all that is left of such a model, so removing it is the whole
/// command. Refusing instead would put the one thing `--purge` exists for out
/// of reach: the line a plain `disable` closes with names this very command,
/// and by then the model is already gone from `.chap-devops/models.yaml`.
///
/// A name the marketplace does not list and that no volume answers to either
/// is still the typo it would be without the flag.
fn purge_only(ctx: &Ctx, project: &Project, registry: &Registry, wanted: &str) -> Result<()> {
    let listed = registry.get(wanted);
    let id = purge_id(listed.map(|model| model.id.as_str()), wanted);
    let volume = project.prefixed_volume(&crate::compose::volume_name(&id));
    if listed.is_none() && !volume.as_deref().is_some_and(crate::docker::volume_exists) {
        return Err(ChapError::UnknownModel(wanted.to_string()).into());
    }

    let mut notes = vec![format!(
        "{id} is not enabled here, so only its data volume was looked for"
    )];
    let mut purged = Vec::new();
    match volume {
        Some(name) => {
            let (removed, line) = super::docker::purge_volume(&name);
            purged.extend(removed);
            notes.push(line);
        }
        None => notes.push(super::docker::UNNAMEABLE_VOLUME.to_string()),
    }
    let report = DisableReport {
        apply: ApplyReport::default(),
        stopped: None,
        purged,
        kept_volumes: Vec::new(),
        retired: None,
    };
    ctx.out.report_ok(&report, |lines| {
        for note in &notes {
            lines.info(note.as_str());
        }
    })
}

/// The marketplace id a `--purge` on a model that is not enabled is about.
///
/// `listed` is the id the marketplace holds for whichever identifier was
/// typed, which is what every other command accepts. For an entry the
/// marketplace no longer lists there is nothing to ask: a service id is the
/// marketplace id with its underscores written as hyphens, so folding them
/// back is the one guess worth making, and the volume of that name either
/// exists or the run says the model is unknown.
fn purge_id(listed: Option<&str>, wanted: &str) -> String {
    match listed {
        Some(id) => id.to_string(),
        None => wanted.replace('-', "_"),
    }
}

/// What `models disable` did: the state edit, what became of the container
/// that was running the model, and what became of its data volume.
#[derive(Debug, serde::Serialize)]
pub(crate) struct DisableReport {
    #[serde(flatten)]
    pub(crate) apply: ApplyReport,
    /// What was done about the container, when there was one to do anything
    /// about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stopped: Option<String>,
    /// Data volumes this run removed: the model's own, when `--purge` asked
    /// for it and it was there to remove.
    pub(crate) purged: Vec<String>,
    /// Data volumes it left in place, which is what a disable without
    /// `--purge` does.
    pub(crate) kept_volumes: Vec<String>,
    /// What `--retire` did in chap-core.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) retired: Option<retire::Outcome>,
}

/// A path in the deployment, relative to its directory when it is inside it.
fn relative(project: &Project, path: &std::path::Path) -> String {
    path.strip_prefix(&project.dir)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Endpoints for a call that enables nothing and therefore looks nothing up.
fn offline_endpoints() -> crate::manual::Endpoints {
    crate::manual::Endpoints {
        offline: true,
        docker_probe: false,
        ..crate::manual::Endpoints::default()
    }
}

/// The state key for a marketplace id or a compose service id.
pub(crate) fn enabled_id(project: &Project, wanted: &str) -> Option<String> {
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

/// The lines of one enable or disable.
///
/// `notes` is what `disable` did beyond the state edit: the container that
/// was running the model, and the data volume it kept or removed. They are
/// info, because they say what happened to the data.
pub(crate) fn summary(
    report: &ApplyReport,
    notes: &[String],
    project: &Project,
    lines: &mut Report,
) {
    // A manually added model's version is its image tag, so `v` in front of
    // it would read as a version number it does not have.
    let version = |model: &crate::project::EnabledModel| match model.version == model.image_tag {
        true => model.image_tag.clone(),
        false => format!("v{}", model.version),
    };
    let place = |model: &crate::project::EnabledModel| match model.host_port {
        Some(port) => format!("on http://localhost:{port}"),
        None => format!("at {}", project.proxy_url(&model.service_id)),
    };
    for (id, model) in report.touched() {
        let verb = match report.enabled.iter().any(|(e, _)| e == id) {
            true => "enabled",
            false => "updated",
        };
        lines.info(format!("{verb} {id} {} {}", version(model), place(model)));
        lines.hint(format!("{id} is in `{}`", model.compose_file));
    }
    for (id, model) in &report.unchanged {
        lines.info(format!(
            "{id} {} is already enabled {}; nothing changed",
            version(model),
            place(model)
        ));
    }
    for id in &report.disabled {
        lines.info(format!("disabled {id}"));
    }
    for path in &report.removed {
        lines.hint(format!("removed {}", relative(project, path)));
    }
    for warning in &report.warnings {
        lines.warning(warning.as_str());
    }
    for note in notes {
        lines.info(note.as_str());
    }
    // Taking a model away has already stopped its container, and a change
    // that wrote no compose file leaves the containers as they are: there is
    // nothing left for `up` to apply, only a deployment to look at.
    let rendered = !report.written.is_empty() || !report.removed.is_empty();
    match report.touched().next().is_none() || !rendered {
        true => lines.hint("`chap-devops status` shows what runs now"),
        false => lines.info("run `chap-devops up` to apply"),
    };
}

pub(crate) mod retire;

#[cfg(test)]
mod tests;
