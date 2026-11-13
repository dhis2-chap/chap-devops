//! What a wrapper says once docker has finished, and when there was nothing
//! for docker to do.

use super::down::{
    DownVolumes, down_lines, down_next, forget_dhis2_connect, remove_leftovers, removed_volumes,
    still_there_line,
};
use crate::cli::DockerCmd;
use crate::commands::Ctx;
use crate::components::{Components, dhis2_connect_hint};
use crate::docker;
use crate::error::Result;
use crate::output::{Out, Report};
use crate::project::Project;

/// What `logs` and `docker ps` say for a project that has no containers at
/// all. Both would otherwise print nothing whatsoever.
pub(super) const NOTHING_RUNNING: &str =
    "nothing is running for this deployment; start Chap with `chap-devops up`";

/// What `restart` says when there is nothing to recreate. Recreating is not
/// starting: a deployment that is down is `chap-devops up`'s to bring up, the same
/// answer `chap-devops status` gives.
const NOT_RUNNING: &str = "Chap is not running; start it with `chap-devops up`";

/// The hint that closes a detached `up` and a `restart`.
const AFTER_UP: &str = "run `chap-devops status` to check that everything answers";

/// Say what the wrapper did, now that docker has finished.
///
/// `volumes` are the ones docker held before a `down --volumes`, and empty
/// for every other wrapper. A detached `up` is not here: it may wait first,
/// and [`super::finish_up`] reports it.
pub(super) fn report_what_changed(
    ctx: &Ctx,
    project: &mut Project,
    cmd: &DockerCmd,
    before: &[docker::Container],
    volumes: &[String],
) -> Result<()> {
    match cmd {
        DockerCmd::Restart(args) => {
            let after = docker::running_containers(project).unwrap_or_default();
            let (recreated, unchanged) = docker::diff_containers(before, &after);
            let value = serde_json::json!({ "recreated": recreated, "unchanged": unchanged });
            ctx.out.report_ok(&value, |lines| {
                restart_lines(before, &after, &args.services, lines)
            })
        }
        DockerCmd::Down(args) => {
            let stopped = docker::service_names(before);
            let refused = match args.volumes {
                true => remove_leftovers(project, volumes),
                false => Vec::new(),
            };
            let removed = args.volumes.then(|| removed_volumes(project, volumes));
            // What `--volumes` did not remove, as docker sees it now.
            let kept: Vec<String> = match &removed {
                Some(names) => volumes
                    .iter()
                    .filter(|volume| !names.contains(volume))
                    .map(|volume| {
                        let why = refused
                            .iter()
                            .find(|(name, _)| name == volume)
                            .map(|(_, why)| why.as_str());
                        still_there_line(volume, why)
                    })
                    .collect(),
                None => Vec::new(),
            };
            // Said after the line that names what went, because it is a
            // consequence of it.
            let forgot = removed
                .as_deref()
                .and_then(|names| forget_dhis2_connect(project, names));
            let mut value = serde_json::json!({ "stopped": stopped });
            if let Some(names) = &removed {
                value["removed_volumes"] = serde_json::json!(names);
            }
            let name = project.compose_project_name();
            ctx.out.report_ok(&value, |lines| {
                let volumes = match &removed {
                    Some(names) => DownVolumes::Removed(names),
                    None => DownVolumes::Kept,
                };
                down_lines(&stopped, volumes, name.as_deref(), lines);
                for line in &kept {
                    lines.warning(line.as_str());
                }
                match forgot {
                    Some(Ok(line)) => lines.info(line),
                    Some(Err(line)) => lines.warning(line),
                    None => lines,
                };
                lines.hint(down_next(removed.is_some(), kept.len()));
            })
        }
        DockerCmd::Pull(_) => {
            let images = docker::image_count(project);
            let value = serde_json::json!({ "images": images });
            ctx.out.report_ok(&value, |lines| {
                lines.info(pull_summary(images));
            })
        }
        _ => Ok(()),
    }
}

/// What a detached `up` changed, from the containers before and after.
///
/// Compose prints one line per service as it goes, in no particular order and
/// in the language of its own steps ("Created", "Running"); this is the one
/// line that says which services are new to this run. A recreated container
/// is a new one, so it counts as started.
///
/// A deployment with a DHIS2 nothing has connected gets one more line under
/// that, because this is the run the reader is about to wait minutes for and
/// the line that named `chap-devops dhis2 connect` scrolled past when the component
/// was added. It comes off `.chap-devops/components.yaml` alone - `up` asks DHIS2
/// nothing, and there would be nothing to ask yet - so it says what chap-devops has
/// recorded rather than what DHIS2 is. See [`dhis2_connect_hint`].
///
/// Not on the run that started nothing at all: there is no deployment up to
/// connect, and the warning already says to go and read the logs.
///
/// `waited` is set after `--wait`, which has already checked what the hint
/// about `chap-devops status` would send the reader to check.
pub fn up_lines(
    before: &[docker::Container],
    after: &[docker::Container],
    components: &Components,
    waited: bool,
    lines: &mut Report,
) {
    let (started, unchanged) = docker::diff_containers(before, after);
    match (started.is_empty(), unchanged.is_empty()) {
        (true, true) => {
            lines.warning(
                "nothing is running after `chap-devops up`; run `chap-devops logs` to see why",
            );
            return;
        }
        (true, false) => {
            lines.info(format!("already running: {}", unchanged.join(", ")));
        }
        (false, _) => {
            lines.info(format!("started {}", started.join(", ")));
            if !unchanged.is_empty() {
                lines.hint(format!("unchanged: {}", unchanged.join(", ")));
            }
        }
    }
    if !waited {
        lines.hint(AFTER_UP);
    }
    if components.dhis2_needs_connecting() {
        lines.info(dhis2_connect_hint(false));
    }
}

/// What closes an `up` without `--wait` that started a model next to
/// chap-core.
///
/// chap-core 2.4 and later makes no configured model from a registration, so
/// a model it cannot run until `chap-devops models configs sync` makes one.
/// `--wait` does that step itself, after the models registered; without it,
/// `up` returns before they did, and this line is the only one that says the
/// step is still to do. A run that started no model says nothing: the
/// configured models of a model already running are made already.
pub const CONFIGURE_NEXT: &str = "when the models are registered, run `chap-devops models \
     configs sync` to give chap-core configured models; `chap-devops up --wait` does this itself";

/// [`CONFIGURE_NEXT`], when this `up` started or recreated a model service of
/// a deployment with chap-core.
pub fn configure_next(
    before: &[docker::Container],
    after: &[docker::Container],
    project: &Project,
) -> Option<&'static str> {
    if !project.state.components.has_chap_core_api() {
        return None;
    }
    let (started, _) = docker::diff_containers(before, after);
    let model_started = project
        .state
        .models
        .values()
        .any(|m| started.contains(&m.service_id));
    model_started.then_some(CONFIGURE_NEXT)
}

/// What `restart` recreated, from the containers before and after.
///
/// Compose recreates only the containers that no longer match the files, and
/// says nothing at all about the ones it skipped; this is the line that says
/// which half each service fell in. A run that recreated nothing is the
/// answer "nothing had moved on under you", not a failure.
///
/// `named` are the services the command was given, which is what the way to
/// force it spells out; none named is the whole project.
pub fn restart_lines(
    before: &[docker::Container],
    after: &[docker::Container],
    named: &[String],
    lines: &mut Report,
) {
    let (recreated, unchanged) = docker::diff_containers(before, after);
    if recreated.is_empty() {
        lines.info("nothing needed a restart: every container matches its files");
        lines.hint(match named {
            [] => "`chap-devops restart --all` recreates every one anyway".to_string(),
            [one] => format!("`chap-devops restart --all {one}` recreates it anyway"),
            many => format!(
                "`chap-devops restart --all {}` recreates them anyway",
                many.join(" ")
            ),
        });
        return;
    }
    lines.info(format!("recreated {}", recreated.join(", ")));
    if !unchanged.is_empty() {
        lines.hint(format!("unchanged: {}", unchanged.join(", ")));
    }
    lines.hint(AFTER_UP);
}

/// The "there is nothing here" answer `logs` and `ps` give a project whose
/// containers have never been created.
pub(super) fn nothing_running(out: &Out) -> String {
    out.backticks(NOTHING_RUNNING)
}

/// The "there is nothing to recreate" answer `restart` gives a deployment
/// that is not running.
pub(super) fn not_running(out: &Out) -> String {
    out.backticks(NOT_RUNNING)
}

/// What `docker pull` fetched. Compose's own output is progress, not a result.
pub fn pull_summary(images: Option<usize>) -> String {
    match images {
        Some(1) => "pulled 1 image".to_string(),
        Some(count) => format!("pulled {count} images"),
        // `config --images` is the only thing that could have failed here, and
        // the pull itself succeeded, so the count is all that is missing.
        None => "pulled the images this deployment pins".to_string(),
    }
}

/// The named services that have no container at all, running or not, in
/// the order they were named.
pub(super) fn absent_services(named: &[String], containers: &[docker::Container]) -> Vec<String> {
    named
        .iter()
        .filter(|name| !containers.iter().any(|c| &c.service == *name))
        .cloned()
        .collect()
}

/// What `logs SERVICE` says about a service that has never started, and so
/// has no log.
pub(super) fn no_container_message(absent: &[String]) -> String {
    let named: Vec<String> = absent.iter().map(|s| format!("`{s}`")).collect();
    format!(
        "this deployment has no container for {}, so there is no log; start it with `chap-devops up`",
        named.join(", ")
    )
}

/// What `logs SERVICE` says about a name the project does not have.
pub fn unknown_service_message(unknown: &[String], services: &[String]) -> String {
    let named: Vec<String> = unknown.iter().map(|s| format!("`{s}`")).collect();
    format!(
        "no service {} in this deployment; the services are {}",
        named.join(", "),
        services.join(", ")
    )
}

#[cfg(test)]
mod tests;
