//! `chap-devops run`, `ps` and `stop`: start one model and get its URL, without a
//! deployment directory of your own.
//!
//! Inside a deployment the three work on it. Anywhere else they work on a
//! *group*: a deployment chap-devops keeps for this user under its data directory
//! ([`crate::paths::data_dir`]), `run/<group>/`, which the first `chap-devops run`
//! into it creates - no chap-core, no components, model ports on `127.0.0.1`
//! only. `--group` names one, `default` when it is left out, and each group is
//! a compose project of its own. So a tool that only wants "start this model
//! and tell me where it answers" has one code path, with or without
//! `chap-devops init`.

mod chap_core;
mod foreground;
mod group;
mod ps;
mod stop;

use group::{ensure_default, lock_file};
pub use ps::ps;
pub use stop::stop;
pub(crate) use stop::stop_in;

use crate::cli::{ModelRunArgs, ModelsAddArgs, ModelsEnableArgs};
use crate::commands::Ctx;
use crate::commands::docker::wait::{self, Readiness};
use crate::commands::enable::{ModelRef, enabled_id};
use crate::compose::sync::sync;
use crate::docker;
use crate::error::{ChapError, Result};
use crate::output::Report;
use crate::project::Project;
use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The group a `chap-devops run` outside a deployment lands in without `--group`.
pub const DEFAULT_GROUP: &str = "default";

/// Where `chap-devops run` groups publish model ports: this machine only.
const RUN_BIND: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// Where the run family acts: a deployment the caller is inside, or a group.
pub(crate) struct Target {
    pub(crate) dir: PathBuf,
    /// `None` inside a deployment of the caller's own.
    pub(crate) group: Option<String>,
}

/// The directory holding every group.
pub fn groups_dir() -> PathBuf {
    crate::paths::run_groups_dir()
}

/// The groups that exist, by name, in name order.
pub fn groups() -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(groups_dir()) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
            )
        })
        .filter(|(_, dir)| Project::exists(dir))
        .collect();
    out.sort();
    out
}

/// A group's directory, refusing a name that is not one path segment of
/// lowercase letters, digits, `-` and `_`.
fn group_dir(name: &str) -> Result<PathBuf> {
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !ok {
        return Err(ChapError::Usage(format!(
            "`{name}` is not a group name; use lowercase letters, digits, `-` and `_`, \
             such as `--group dengue`"
        ))
        .into());
    }
    Ok(groups_dir().join(name))
}

/// The deployment the command is inside, or the group it names.
pub(crate) fn target(ctx: &Ctx, group: Option<&str>) -> Result<Target> {
    if let Some(dir) = Project::find_root(&ctx.project_dir) {
        if group.is_some() {
            return Err(ChapError::Usage(format!(
                "--group names a `chap-devops run` group, and this runs inside the deployment at {}; \
                 drop --group, or run it from outside a deployment",
                dir.display()
            ))
            .into());
        }
        return Ok(Target { dir, group: None });
    }
    refuse_named_dir(ctx)?;
    let name = group.unwrap_or(DEFAULT_GROUP);
    Ok(Target {
        dir: group_dir(name)?,
        group: Some(name.to_string()),
    })
}

/// Refuse a `-C` that names a directory outside every deployment.
///
/// Without `-C` the run family falls back to the groups. With it, the reader
/// asked for one directory, and the groups are not that directory: the error
/// is the one `chap-devops logs` and `chap-devops status` give for it.
fn refuse_named_dir(ctx: &Ctx) -> Result<()> {
    if ctx.project_dir == Path::new(".") {
        return Ok(());
    }
    let shown = std::path::absolute(&ctx.project_dir).unwrap_or_else(|_| ctx.project_dir.clone());
    Err(ChapError::NotAProject(shown).into())
}

/// What `chap-devops run` did, for `--json`.
#[derive(Debug, Serialize)]
struct RunReport {
    #[serde(flatten)]
    model: ModelRef,
    /// The group it runs in; `null` inside a deployment of the caller's own.
    group: Option<String>,
    /// The deployment the model runs in, for `chap-devops -C <dir> logs`.
    project_dir: PathBuf,
    /// Whether this run enabled the model, or found it enabled already.
    enabled: bool,
    /// What waiting found; `null` under `--no-wait`.
    wait: Option<Readiness>,
    /// Whether the model's container ran before this command, so neither
    /// Ctrl-C nor the end of the foreground stops it.
    #[serde(skip)]
    was_running: bool,
    /// The chap-core elsewhere the model registers with, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    chap_core: Option<String>,
    /// Whether that chap-core listed the model after the start.
    #[serde(skip_serializing_if = "Option::is_none")]
    registered: Option<bool>,
    /// What the sync before the start warned about.
    #[serde(skip)]
    warnings: Vec<String>,
}

/// Enable the model if it is not, start its container, and wait for it;
/// then, in the foreground, follow its log until Ctrl-C, which stops it.
pub fn run(ctx: &Ctx, args: &ModelRunArgs) -> Result<()> {
    // `--attach` under `--json` would print the log between a tool and its
    // one document, so `--json` always returns.
    let attached = args.attach && !ctx.out.json;
    crate::interrupt::install();
    let report = start(ctx, args)?;
    ctx.out
        .report_ok(&report, |lines| say(&report, attached, lines))?;
    match attached {
        true => foreground::follow(ctx, &report, args.rm),
        false => Ok(()),
    }
}

/// A model `chap-devops run` started for another command, `chap-devops chap`.
pub(crate) struct Started {
    /// The id the model is enabled under.
    pub(crate) id: String,
    /// Whether this start enabled it, so the caller can take it out again.
    pub(crate) enabled: bool,
    /// The group directory it runs in.
    pub(crate) dir: PathBuf,
}

/// [`run`] without its report: enable, start and wait for the model, with
/// the progress lines on stderr, so the caller's stdout stays its own.
pub(crate) fn start_quietly(ctx: &Ctx, args: &ModelRunArgs) -> Result<Started> {
    let quiet = Ctx {
        out: crate::output::Out {
            json: true,
            ..ctx.out
        },
        ..ctx.clone()
    };
    let report = start(&quiet, args)?;
    for warning in &report.warnings {
        crate::output::warn(warning);
    }
    Ok(Started {
        id: report.model.id.clone(),
        enabled: report.enabled,
        dir: report.project_dir,
    })
}

/// Everything [`run`] does before it reports.
fn start(ctx: &Ctx, args: &ModelRunArgs) -> Result<RunReport> {
    let Target { dir, group } = target(ctx, args.group.as_deref())?;
    check_known(ctx, &dir, &args.source)?;
    if let Some(group) = &group {
        ensure_default(ctx, &dir, group)?;
    }
    let ctx = &Ctx {
        project_dir: dir.clone(),
        ..ctx.clone()
    };

    let (mut project, lock) = ctx.project_mut()?;
    if let Some(url) = &args.chap_core {
        if group.is_none() {
            return Err(ChapError::Usage(format!(
                "--chap-core sets the chap-core of a `chap-devops run` group, and this runs in the \
                 deployment at {}; set its chap-core with `chap-devops components enable chap-core \
                 --url {url}`",
                dir.display()
            ))
            .into());
        }
        for note in chap_core::point_group_at(&mut project, url, args.models_host.as_deref())? {
            crate::output::notice(&note);
        }
    }
    let registry = super::registry_for(ctx, Some(&project))?;
    // A repository or image added before is that entry again, not a copy;
    // so is the repository or image of a marketplace model enabled here.
    let source = super::manual_models::added_as(&project, &args.source, args.id.as_deref())
        .or_else(|| match args.id {
            None => super::manual_models::enabled_twin(&registry, &project, &args.source),
            Some(_) => None,
        })
        .unwrap_or_else(|| args.source.clone());
    let (id, enabled) = match enabled_id(&project, &source) {
        // Already there: started as it is, whatever port it has.
        Some(id) => (id, false),
        None => (
            enable_source(ctx, &mut project, args, &source, &registry)
                .map_err(|err| for_group(err, group.is_some(), &dir))?,
            true,
        ),
    };
    // Like `chap-devops up`, one sync renders the compose files from `.chap-devops/`
    // before compose reads them.
    let registry = super::registry_for(ctx, Some(&project))?;
    let synced = sync(&mut project, &registry, false)?;
    drop(lock);

    let model = project.state.models[&id].clone();
    let service = model.service_id.clone();
    note(
        ctx,
        &format!("starting {service} ({} in {})", id, dir.display()),
    );
    let args_up = vec![
        "up".to_string(),
        "-d".to_string(),
        "--remove-orphans".to_string(),
        service.clone(),
    ];
    // Two runs of one model would both create its container; the second
    // waits here and finds it running.
    let was_running = docker::running_services(&project).contains(&service);
    let up_lock = lock_file(
        &dir.join(crate::project::CHAP_DEVOPS_DIR)
            .join(format!("up-{service}.lock")),
    )?;
    let piped = docker::run_compose_teed(&project, &args_up)?;
    if crate::interrupt::requested() {
        return Err(foreground::take_back(
            ctx,
            &project,
            &id,
            &service,
            enabled,
            was_running,
        ));
    }
    if piped.code != 0 {
        let said = compose_said(&piped.stderr);
        // The line compose said is in the message below, so it is not a cause
        // of its own as well: that would print it twice.
        let err = anyhow::Error::from(ChapError::DockerFailed(piped.code));
        // A model this run enabled and could not start is taken back out, so
        // `chap-devops ps` does not list it forever and the retry starts afresh; an
        // added definition stays.
        if enabled && let Err(undo) = stop_in(ctx, &dir, &id, false) {
            crate::output::warn(&format!("{id} stays enabled: {undo:#}"));
        }
        let again = match group.as_deref() {
            Some(g) if g != DEFAULT_GROUP => format!("chap-devops run {} --group {g}", args.source),
            _ => format!("chap-devops run {}", args.source),
        };
        let image = crate::compose::image_ref(&model.image, &model.image_tag);
        return Err(err.context(start_failure(&service, &image, said.as_deref(), &again)));
    }
    drop(up_lock);

    let readiness = (!args.no_wait).then(|| {
        wait::wait_until_ready(
            ctx,
            &project,
            Duration::from_secs(args.timeout),
            Some(&service),
        )
    });
    if crate::interrupt::requested() {
        return Err(foreground::take_back(
            ctx,
            &project,
            &id,
            &service,
            enabled,
            was_running,
        ));
    }
    // A chap-core elsewhere is only worth naming when it really lists the
    // model: registration happens inside the model, after it answers.
    let chap_core = project
        .state
        .components
        .chap_core_external
        .as_ref()
        .map(|external| external.url.clone());
    let registered = match (&chap_core, &readiness) {
        (Some(_), Some(wait)) if wait.ready => Some(chap_core::registered(&project, &service)),
        _ => None,
    };
    let report = RunReport {
        model: ModelRef::of(&id, &model, &project),
        group,
        project_dir: dir.clone(),
        enabled,
        wait: readiness,
        was_running,
        chap_core,
        registered,
        warnings: synced.warnings,
    };
    if let Some(wait) = &report.wait
        && !wait.ready
    {
        let state = wait
            .models
            .first()
            .map(|m| m.state)
            .unwrap_or("not running");
        return Err(anyhow::anyhow!(
            "{service} did not answer within {}s ({state}); {}",
            args.timeout,
            logs_hint(&dir, &service)
        ));
    }
    Ok(report)
}

/// Refuse a bare word that is neither a marketplace id nor a model of the
/// deployment, before a group is made for it: an image always carries a tag,
/// so a word without one is an id that is not there.
fn check_known(ctx: &Ctx, dir: &Path, source: &str) -> Result<()> {
    if source.contains([':', '/', '@']) {
        return Ok(());
    }
    let project = match Project::exists(dir) {
        true => Some(Project::load(dir)?),
        false => None,
    };
    let registry = super::registry_for(ctx, project.as_ref())?;
    let enabled = project
        .as_ref()
        .is_some_and(|p| enabled_id(p, source).is_some());
    if registry.get(source).is_some() || enabled {
        return Ok(());
    }
    Err(anyhow::anyhow!(
        "there is no marketplace model `{source}`; `chap-devops models search {source}` finds one"
    ))
}

/// An error from inside a group, with every `chap-devops ...` it names given the
/// `-C <group dir>` that command needs to reach the group from here.
fn for_group(err: anyhow::Error, grouped: bool, dir: &Path) -> anyhow::Error {
    if !grouped || err.downcast_ref::<ChapError>().is_some() {
        return err;
    }
    let message = with_dir(&err.to_string(), dir);
    match message == err.to_string() {
        true => err,
        false => {
            let causes: Vec<String> = err.chain().skip(1).map(|c| c.to_string()).collect();
            let mut out: Option<anyhow::Error> = None;
            for cause in causes.into_iter().rev() {
                out = Some(match out {
                    Some(inner) => inner.context(cause),
                    None => anyhow::anyhow!(cause),
                });
            }
            match out {
                Some(inner) => inner.context(message),
                None => anyhow::anyhow!(message),
            }
        }
    }
}

/// `text` with every `chap-devops ...` command in it given `-C <dir>`.
fn with_dir(text: &str, dir: &Path) -> String {
    text.replace(
        "`chap-devops ",
        &format!("`chap-devops -C {} ", shell_quote(&dir.to_string_lossy())),
    )
}

/// Enable a marketplace model by id, or add whatever else the source names.
fn enable_source(
    ctx: &Ctx,
    project: &mut Project,
    args: &ModelRunArgs,
    source: &str,
    registry: &crate::registry::Registry,
) -> Result<String> {
    if let Some(model) = registry.get(source) {
        let id = model.id.clone();
        super::enable::enable_in(
            ctx,
            project,
            &ModelsEnableArgs {
                ids: vec![id.clone()],
                channel: None,
                version: None,
                port: Some(args.port),
                bind: args.bind,
                data_dir: None,
                user: None,
                allow_starter: args.allow_starter,
            },
        )?;
        return Ok(id);
    }
    let added = super::manual_models::add_in(
        ctx,
        project,
        &ModelsAddArgs {
            source: source.to_string(),
            id: args.id.clone(),
            service_id: None,
            name: None,
            port: Some(args.port),
            bind: args.bind,
            data_dir: None,
            user: None,
            runtime_amd64: false,
            allow_starter: args.allow_starter,
        },
    )?;
    Ok(match added {
        super::manual_models::Added::Marketplace(report) => report
            .enabled
            .iter()
            .chain(&report.updated)
            .chain(&report.unchanged)
            .next()
            .map(|(id, _)| id.clone())
            .expect("enabling a marketplace model reports it"),
        super::manual_models::Added::Manual(report, _) => report.id.clone(),
    })
}

/// The closing lines of a run. Under `-a` the log follows, and Ctrl-C is
/// the way to stop the model, so no command for that is named.
fn say(report: &RunReport, attached: bool, lines: &mut Report) {
    let url = report.model.url.as_deref().unwrap_or("internal");
    let place = match report.group.as_deref() {
        Some(group) if group != DEFAULT_GROUP => format!(" in group {group}"),
        _ => String::new(),
    };
    match &report.wait {
        Some(wait) => lines.info(format!(
            "running {}{place} on {url} (answered in {}s)",
            report.model.id, wait.waited_s
        )),
        None => lines
            .info(format!("started {}{place} on {url}", report.model.id))
            .hint("`chap-devops ps` shows when it answers"),
    };
    for warning in &report.warnings {
        lines.warning(warning.as_str());
    }
    match (&report.chap_core, report.registered) {
        (Some(url), Some(true)) => {
            lines.info(format!("registered with {url}"));
        }
        (Some(url), Some(false)) => {
            lines.warning(format!(
                "not registered with {url}; {}",
                logs_hint(&report.project_dir, &report.model.service_id)
            ));
        }
        _ => {}
    }
    if attached {
        return;
    }
    lines.hint(format!(
        "`chap-devops stop {}{}` stops it",
        report.model.id,
        match report.group.as_deref() {
            Some(g) if g != DEFAULT_GROUP => format!(" --group {g}"),
            _ => String::new(),
        }
    ));
    lines.hint(logs_hint(&report.project_dir, &report.model.service_id));
}

/// The `logs` command for a service, with `-C` when the deployment is not
/// one the working directory is inside.
fn logs_hint(dir: &Path, service: &str) -> String {
    let here = std::env::current_dir()
        .ok()
        .and_then(|cwd| Project::find_root(&cwd));
    match here.as_deref() == Some(dir) {
        true => format!("`chap-devops logs {service}` shows its log"),
        false => format!(
            "`chap-devops -C {} logs {service}` shows its log",
            shell_quote(&dir.to_string_lossy())
        ),
    }
}

/// `text`, quoted for a POSIX shell when it needs to be.
fn shell_quote(text: &str) -> String {
    match text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-~".contains(c))
    {
        true => text.to_string(),
        false => format!("'{}'", text.replace('\'', r"'\''")),
    }
}

/// Why compose stopped: the last line of its stderr that says `error`, or
/// the last line with anything on it.
fn compose_said(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .map(str::trim)
        .rfind(|line| line.to_ascii_lowercase().contains("error"))
        .map(str::to_string)
        .or_else(|| last_line(stderr))
}

/// The error for a model container that did not start.
///
/// A registry says `denied` both for an image that does not exist and for a
/// private one, and the bare word gives no way out, so that case is said in
/// full. `permission denied` is the Docker socket, not the registry.
fn start_failure(service: &str, image: &str, said: Option<&str>, again: &str) -> String {
    let lower = said.unwrap_or_default().to_ascii_lowercase();
    if lower.contains("denied") && !lower.contains("permission denied") {
        let login = match crate::compose::is_local_image(image) {
            true => "docker login".to_string(),
            false => format!(
                "docker login {}",
                image.split('/').next().unwrap_or_default()
            ),
        };
        return format!(
            "{service} did not start: the registry answered `denied` for {image}, so the image \
             does not exist or is private; check the reference, or run `{login}`, then \
             `{again}` tries again"
        );
    }
    format!(
        "{service} did not start ({}); fix that, then `{again}` tries again",
        said.unwrap_or("docker compose failed")
    )
}

/// The last line of `text` with anything on it.
fn last_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_string)
}

/// A progress line: stdout for a person, stderr under `--json`.
fn note(ctx: &Ctx, line: &str) {
    if ctx.out.json {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
}

#[cfg(test)]
mod tests;
