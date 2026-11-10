//! `chap-devops models expose|unexpose`: give an enabled model a host port,
//! or take it away.

use super::{enabled_id, relative};
use crate::cli::{ModelsExposeArgs, ModelsUnexposeArgs};
use crate::commands::Ctx;
use crate::compose::PortRequest;
use crate::compose::ports::{PortAllocator, allocator_for};
use crate::compose::sync::sync;
use crate::error::{ChapError, Result};
use crate::output::Report;
use crate::ports::OwnPorts;
use crate::project::Project;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Publish a host port for a model that is already enabled.
pub fn expose(ctx: &Ctx, args: &ModelsExposeArgs) -> Result<()> {
    // Omitting --port means "any free one": someone who wanted a specific
    // number would have said so.
    let request = args.port.map(|p| p.0).unwrap_or(PortRequest::Auto);
    set_host_port(ctx, &args.id, request, args.bind)
}

/// Take an enabled model's host port away again.
pub fn unexpose(ctx: &Ctx, args: &ModelsUnexposeArgs) -> Result<()> {
    set_host_port(ctx, &args.id, PortRequest::None, None)
}

/// What `expose` and `unexpose` did, for `--json`.
#[derive(Debug, serde::Serialize)]
pub(super) struct PortChange {
    /// Marketplace id, whichever identifier the caller typed.
    pub(super) id: String,
    pub(super) service_id: String,
    /// The port the model publishes now; `null` for an internal-only service.
    pub(super) host_port: Option<u16>,
    /// What it published before.
    pub(super) previous: Option<u16>,
    /// How to reach it from this machine now.
    pub(super) url: String,
    pub(super) written: Vec<PathBuf>,
    /// Whether nothing changed and the model's container already publishes
    /// the port, so `chap-devops up` has nothing to apply.
    #[serde(skip)]
    pub(super) applied: bool,
    /// Whether the model's container runs, when it does not publish the port
    /// yet.
    #[serde(skip)]
    pub(super) running: bool,
}

/// The host port a `models expose` or `unexpose` gives the model.
///
/// `own` says whether the model's own container publishes a port now; that
/// port is not busy for the model. `--port auto` keeps the port the model has
/// while it is free for the model, so a working port is not changed for a new
/// one.
pub(super) fn choose_port(
    request: PortRequest,
    previous: Option<u16>,
    own: &dyn Fn(u16) -> bool,
    allocator: &dyn Fn(&BTreeSet<u16>) -> Result<PortAllocator>,
    is_busy: &dyn Fn(u16) -> bool,
) -> Result<Option<u16>> {
    // The previous port is out of the claims: it is about to be replaced,
    // and a claim of it again has to succeed.
    let freed: BTreeSet<u16> = previous.into_iter().collect();
    let busy = |port: u16| !(previous == Some(port) && own(port)) && is_busy(port);
    Ok(match request {
        PortRequest::None => None,
        PortRequest::Auto => {
            let mut allocator = allocator(&freed)?;
            match previous.filter(|port| allocator.claim(*port, &busy).is_ok()) {
                Some(port) => Some(port),
                None => Some(allocator.allocate(&busy)?),
            }
        }
        PortRequest::Fixed(port) => {
            allocator(&freed)?.claim(port, &busy)?;
            Some(port)
        }
    })
}

/// Move one enabled model's host port, then re-render the compose files.
///
/// Deliberately not a [`Selection`](crate::compose::Selection): this command is about the port and
/// nothing else, and its report says so in its own words. A selection can ask
/// for the same thing with `keep_version`, which is what the browser's
/// port-only change carries, but it would still have to resolve the rest of
/// the request it is not making.
fn set_host_port(
    ctx: &Ctx,
    wanted: &str,
    request: PortRequest,
    bind: Option<std::net::IpAddr>,
) -> Result<()> {
    let (mut project, _lock) = ctx.project_mut()?;
    let registry = crate::commands::registry_for(ctx, Some(&project))?;
    let id = enabled_id(&project, wanted).ok_or_else(|| ChapError::UnknownModel(wanted.into()))?;
    let previous = project.state.models[&id].host_port;
    let previous_bind = project.state.models[&id].bind;
    let service_id = project.state.models[&id].service_id.clone();

    // What this deployment's own containers publish now. The model's own
    // container on its port is not a reason to move it.
    let own = match request {
        PortRequest::None => OwnPorts::default(),
        _ => OwnPorts::of_project(&project),
    };
    let host_port = choose_port(
        request,
        previous,
        &|port| own.publishes(&service_id, port),
        &|freed| allocator_for(&project, freed),
        &crate::ports::is_busy,
    )?;

    let entry = project
        .state
        .models
        .get_mut(&id)
        .expect("enabled_id only returns keys that are present");
    entry.host_port = host_port;
    if bind.is_some() {
        entry.bind = bind;
    }
    let same = host_port == previous && entry.bind == previous_bind;

    let synced = sync(&mut project, &registry, false)?;
    let applied = match (same && synced.written.is_empty(), host_port) {
        (true, Some(port)) => own.publishes(&service_id, port),
        _ => false,
    };
    // A port that a running container does not publish yet is a step for
    // `chap-devops up`; a stopped container gets it when `chap-devops up` starts it.
    let running = !applied
        && host_port.is_some()
        && crate::docker::running_services(&project).contains(&service_id);
    let change = PortChange {
        applied,
        running,
        id,
        url: match host_port {
            Some(port) => format!("http://localhost:{port}"),
            None => project.proxy_url(&service_id),
        },
        service_id,
        host_port,
        previous,
        written: synced.written,
    };
    ctx.out.report_ok(&change, |lines| {
        port_summary(&change, &project, &synced.warnings, lines)
    })
}

/// The lines of one port change.
pub(super) fn port_summary(
    change: &PortChange,
    project: &Project,
    warnings: &[String],
    lines: &mut Report,
) {
    // Nothing moved and no file was written: there is nothing for `up` to
    // apply.
    if change.host_port == change.previous && change.written.is_empty() {
        match change.host_port {
            Some(port) => lines.info(format!(
                "{} is already exposed on http://localhost:{port}; nothing changed",
                change.service_id
            )),
            None => lines.info(format!(
                "{} was already unexposed; nothing changed, and it is reachable at {}",
                change.service_id, change.url
            )),
        };
        for warning in warnings {
            lines.warning(warning.as_str());
        }
        match (change.applied, change.running) {
            (true, _) => {}
            (false, true) => {
                lines.info(
                    "its running container does not publish this port yet; run `chap-devops up` to \
                     apply it",
                );
            }
            (false, false) => {
                lines.hint("if its container is not running, `chap-devops up` starts it");
            }
        }
        return;
    }
    let same = match change.host_port == change.previous {
        true => " (no change)",
        false => "",
    };
    match change.host_port {
        Some(port) => lines.info(format!(
            "exposed {} on http://localhost:{port}{same}",
            change.service_id
        )),
        None => lines.info(format!(
            "unexposed {}{same}; it stays registered with chap-core and reachable at {}",
            change.service_id, change.url
        )),
    };
    for path in &change.written {
        lines.hint(format!("wrote {}", relative(project, path)));
    }
    for warning in warnings {
        lines.warning(warning.as_str());
    }
    if !change.applied {
        lines.info("run `chap-devops up` to apply");
    }
}
