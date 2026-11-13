//! The end of a detached `chap-devops up`: the wait for `--wait`, and what
//! started and where it answers.

use super::report::{configure_next, up_lines};
use super::{edited_config_warnings, note, previous, wait};
use crate::commands::Ctx;
use crate::docker;
use crate::error::Result;
use crate::output::Report;
use crate::project::Project;

/// The end of a detached `up`: wait when asked to, then say what started
/// and, after `--wait`, what answers where.
///
/// Under `--json` the document carries what runs now, what this run started,
/// and every model with where it answers. A wait that runs out fails, naming
/// what never answered; a person sees the lines first.
pub(super) fn finish_up(
    ctx: &Ctx,
    project: &mut Project,
    before: &[docker::Container],
    wait: bool,
    timeout: u64,
) -> Result<()> {
    let after = docker::running_containers(project).unwrap_or_default();
    // Compose does not compare a bind-mounted config, so `up` left a service
    // whose config changed after its start as it was.
    let edited = edited_config_warnings(project, &after);
    let mut readiness = wait.then(|| wait_for(ctx, project, timeout));
    // Once every model has registered, chap-core 2.4 and later still has no
    // configured model to run it with; the step gives it one (see
    // `crate::configs::sync`).
    let configured = match &readiness {
        Some(readiness) if readiness.ready => crate::commands::configs::sync::auto(ctx, project),
        _ => None,
    };
    if let (Some(readiness), Some(configured)) = (readiness.as_mut(), &configured) {
        wait::mark_configured(readiness, &configured.models);
    }
    let restored = configured
        .as_ref()
        .map(|configured| previous::settle(ctx, project, configured))
        .unwrap_or_default();
    let project = &*project;
    let build = |lines: &mut Report| {
        up_lines(
            before,
            &after,
            &project.state.components,
            readiness.is_some(),
            lines,
        );
        if readiness.is_none()
            && let Some(line) = configure_next(before, &after, project)
        {
            lines.info(line);
        }
        for line in &edited {
            lines.warning(line.as_str());
        }
        if let Some(readiness) = &readiness {
            ready_lines(readiness, lines);
        }
        crate::commands::configs::sync::fold(&configured, false, lines);
        for line in &restored {
            lines.warning(line.as_str());
        }
    };
    match &readiness {
        Some(readiness) if !readiness.ready => {
            if !ctx.out.json {
                ctx.out.report(&(), build)?;
            }
            Err(anyhow::anyhow!(
                "not ready after {timeout}s: {}; `chap-devops status` shows each one, and \
                 `chap-devops logs <service>` says why",
                wait::pending(readiness).join(", ")
            ))
        }
        _ => {
            let value = up_value(project, before, readiness.clone());
            ctx.out.report_ok(&value, build)
        }
    }
}

/// `up --json`: what runs now, what this run started, and every model with
/// where it answers.
fn up_value(
    project: &Project,
    before: &[docker::Container],
    readiness: Option<wait::Readiness>,
) -> serde_json::Value {
    let after = docker::running_containers(project).unwrap_or_default();
    let was: std::collections::BTreeSet<String> = docker::running_of(before);
    let running: Vec<String> = docker::running_of(&after).into_iter().collect();
    let started: Vec<&String> = running.iter().filter(|s| !was.contains(*s)).collect();
    let models: Vec<crate::commands::enable::ModelRef> = project
        .state
        .models
        .iter()
        .map(|(id, model)| crate::commands::enable::ModelRef::of(id, model, project))
        .collect();
    serde_json::json!({
        "api_url": project.state.components.has_chap_core_api().then(|| project.api_url()),
        "running": running,
        "started": started,
        "models": models,
        "wait": readiness,
    })
}

/// `up --wait`: wait until chap-core and the models answer, or the deadline.
fn wait_for(ctx: &Ctx, project: &Project, timeout: u64) -> wait::Readiness {
    let has_api = project.state.components.has_chap_core_api();
    let components = wait::health_checked(project);
    if let Some(what) = wait::waited_for(has_api, &components, project.state.models.len()) {
        note(
            ctx,
            &format!("waiting up to {timeout}s for {what} to answer"),
        );
    }
    wait::wait_until_ready(ctx, project, std::time::Duration::from_secs(timeout), None)
}

/// What `--wait` found: one line, then a row per thing it waited for.
fn ready_lines(readiness: &wait::Readiness, lines: &mut Report) {
    lines.info(match readiness.ready {
        true => format!("ready in {}s", readiness.waited_s),
        false => format!("not ready after {}s", readiness.waited_s),
    });
    let api_state = match readiness.api_up {
        true => "up",
        false => "not answering",
    };
    let mut rows: Vec<(&str, &str, &str)> = Vec::new();
    if let Some(url) = &readiness.api_url {
        rows.push(("chap-core", api_state, url));
    }
    rows.extend(
        readiness
            .models
            .iter()
            .map(|m| (m.service_id.as_str(), m.state, m.url.as_str())),
    );
    rows.extend(
        readiness
            .components
            .iter()
            .map(|c| (c.name.as_str(), c.state, c.url.as_str())),
    );
    let name = rows.iter().map(|r| r.0.len()).max().unwrap_or(0);
    let state = rows.iter().map(|r| r.1.len()).max().unwrap_or(0);
    for (service, status, url) in rows {
        lines.info(format!("  {service:name$}  {status:state$}  {url}"));
    }
}
