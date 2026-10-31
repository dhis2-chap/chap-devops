//! `chap-devops models add|remove`: models the marketplace does not list.
//!
//! `add` resolves a repository URL or an image reference into the entry a
//! marketplace file would have carried, records it in
//! `.chap-devops/models-manual.yaml` and then enables it down the same path as any
//! other model. `remove` is the way back: it disables the model if it is on,
//! and then drops the definition.

use crate::cli::{ModelsAddArgs, ModelsRemoveArgs};
use crate::commands::Ctx;
use crate::compose::{ApplyReport, EnableRequest, Selection, apply};
use crate::error::{ChapError, Result};
use crate::manual::{self, AddRequest, Endpoints, Names, Origin, Resolved};
use crate::output::{self, Report};
use crate::project::{ManualModel, Project};
use crate::registry::Registry;
use serde::Serialize;

/// How `models add`'s own origin reads to every command downstream.
///
/// [`Origin::Default`] is the chapkit fallback rather than a table lookup, but
/// both are "this CLI's own last resort" and both are fixed the same way -
/// pass `--user`, or add the model where its image can be read.
fn user_source(origin: Origin) -> crate::compose::UserSource {
    use crate::compose::UserSource;
    match origin {
        Origin::Flag => UserSource::Flag,
        Origin::Image => UserSource::ImageConfig,
        Origin::Probe => UserSource::DockerProbe,
        Origin::Default => UserSource::Table,
    }
}

/// What `models add` did, and the shape of its `--json`.
#[derive(Debug, Serialize)]
pub(crate) struct AddReport {
    pub(crate) id: String,
    service_id: String,
    display_name: String,
    /// The repository URL or image reference the model was added from.
    source: String,
    image: String,
    /// The pin: a `sha-` tag, or `@sha256:...`.
    tag: String,
    /// Image and pin as docker is handed them.
    image_ref: String,
    commit: Option<String>,
    /// The branch `chap-devops update` follows, `null` for a pinned entry.
    follow: Option<String>,
    data_dir: String,
    user: String,
    runtime_amd64: bool,
    /// What the resolution had to guess at.
    notes: Vec<String>,
    /// What may stop the model from working, printed as warnings.
    warnings: Vec<String>,
    #[serde(flatten)]
    apply: ApplyReport,
}

/// What `models remove` did.
#[derive(Debug, Serialize)]
struct RemoveReport {
    id: String,
    service_id: String,
    /// Whether the model was enabled, and therefore disabled on the way out.
    was_enabled: bool,
    #[serde(flatten)]
    apply: ApplyReport,
    /// Data volumes this run removed.
    purged: Vec<String>,
    /// What the retire in chap-core did, unless `--keep-in-chap`.
    #[serde(skip_serializing_if = "Option::is_none")]
    retired: Option<super::enable::retire::Outcome>,
}

/// Add a model that is not in the marketplace, and enable it.
///
/// Nothing is written until the whole source has resolved: a repository with
/// no published build, an id that is already taken or a service name another
/// model holds all leave the deployment exactly as it was.
pub fn add(ctx: &Ctx, args: &ModelsAddArgs) -> Result<()> {
    let (mut project, _lock) = ctx.project_mut()?;
    match add_in(ctx, &mut project, args)? {
        Added::Marketplace(report) => {
            let changed = super::enable::Changed::new(&report, &report, &project);
            ctx.out.report_ok(&changed, |lines| {
                super::enable::summary(&report, &[], &project, lines)
            })
        }
        Added::Manual(report, resolved) => {
            let notes = report.notes.clone();
            let changed = super::enable::Changed::new(&report, &report.apply, &project);
            ctx.out.report_ok(&changed, |lines| {
                added_block(&resolved, lines);
                super::enable::summary(&report.apply, &notes, &project, lines);
                for warning in &report.warnings {
                    lines.warning(warning.as_str());
                }
            })
        }
    }
}

/// What [`add_in`] did: enabled the marketplace model the source names, or
/// added a definition of its own.
pub(crate) enum Added {
    Marketplace(ApplyReport),
    Manual(Box<AddReport>, Box<Resolved>),
}

/// [`add`] on a project the caller has loaded and locked, writing the state
/// and printing nothing.
pub(crate) fn add_in(ctx: &Ctx, project: &mut Project, args: &ModelsAddArgs) -> Result<Added> {
    // A source the marketplace already lists is that model: it is enabled
    // from the marketplace, curated pin and all, unless `--id` asks for a
    // separate entry beside it.
    if args.id.is_none() {
        let source = crate::manual::source::Source::parse(&args.source)?;
        let marketplace = crate::registry::load(&ctx.registry)?;
        if let Some((id, version)) = marketplace_match(&marketplace, &source) {
            output::notice(&format!(
                "{} is the marketplace model {id}; enabling that \
                 (pass `--id <other>` to add it beside it instead)",
                args.source.trim()
            ));
            let report = super::enable::enable_in(
                ctx,
                project,
                &crate::cli::ModelsEnableArgs {
                    ids: vec![id],
                    channel: None,
                    version,
                    port: args.port,
                    bind: args.bind,
                    data_dir: args.data_dir.clone(),
                    user: args.user.clone(),
                    allow_starter: args.allow_starter,
                },
            )?;
            return Ok(Added::Marketplace(report));
        }
    }

    let endpoints = Endpoints::from_env(ctx.registry.offline);
    let id = match args.id.as_deref().map(str::trim) {
        Some(AUTO_ID) => Some(free_id(ctx, project, &args.source)?),
        _ => args.id.clone(),
    };
    let request = AddRequest {
        source: args.source.clone(),
        id,
        service_id: args.service_id.clone(),
        display_name: args.name.clone(),
        data_dir: args.data_dir.clone(),
        user: args.user.clone(),
        runtime_amd64: args.runtime_amd64,
    };
    // What it would be called is settled first, and checked against what the
    // deployment and the catalogue already hold: resolving the source pulls
    // an image for the uid probe, and nobody should wait for that only to be
    // told the id is taken. The marketplace is loaded on its own, so a
    // collision with it can be told from one with an earlier `models add`.
    let names = manual::names(&request)?;
    let mut registry = crate::registry::load(&ctx.registry)?;
    check_free(project, &registry, &names)?;

    let resolved = manual::resolve(&request, &endpoints)?;

    let entry = ManualModel {
        service_id: resolved.service_id.clone(),
        display_name: resolved.display_name.clone(),
        repository: resolved.source.repository(),
        image: resolved.image.clone(),
        tag: resolved.tag.clone(),
        commit: resolved.commit.clone(),
        follow: resolved.follow.clone(),
        data_dir: Some(resolved.data_dir.clone()),
        user: Some(resolved.user.clone()),
        reads_port: resolved.reads_port,
        runtime_amd64: resolved.runtime_amd64,
        added: today(),
    };
    project
        .state
        .manual
        .insert(resolved.id.clone(), entry.clone());
    for warning in registry.with_manual(&project.state.manual) {
        output::warn(&warning);
    }

    let selection = Selection {
        enable: vec![EnableRequest {
            id: resolved.id.clone(),
            selector: resolved.selector(),
            port: args.port.map(|p| p.0),
            bind: args.bind,
            data_dir: Some(resolved.data_dir.clone()),
            user: Some(resolved.user.clone()),
            // `models add` has already asked the image; saying where its
            // answer came from keeps `models info` honest about an entry
            // nobody typed a `--user` for.
            user_from: Some(user_source(resolved.user_from)),
            allow_starter: false,
            keep_version: false,
        }],
        ..Selection::default()
    };
    let applied = apply(project, &registry, &selection, &endpoints)?;

    let mut notes = resolved.notes.clone();
    // A second copy of a marketplace model registers under the id its image
    // carries, which is the marketplace model's service id: that is a name
    // this copy cannot take, so the usual way out below does not work.
    let twin = crate::manual::source::Source::parse(&args.source)
        .ok()
        .and_then(|source| {
            marketplace_twin(&registry, &source, |id| {
                project.state.manual.contains_key(id)
            })
        })
        .filter(|model| model.service_id != resolved.service_id)
        .map(|model| (model.id.clone(), model.service_id.clone()));
    let mut warnings = Vec::new();
    // Only a deployment with a chap-core has anything to register with.
    if let Some((twin_id, twin_service)) =
        twin.filter(|_| project.state.components.has_chap_core_api())
    {
        warnings.push(format!(
            "this is the image of the marketplace model {twin_id}, which registers with \
             chap-core as `{twin_service}`, so `{}` may never register; to run {twin_id}, \
             run `chap-devops models remove {}`, then `chap-devops models enable {twin_id}`",
            resolved.service_id, resolved.id
        ));
    } else if project.state.components.has_chap_core_api() {
        notes.push(format!(
            "the service must register with chap-core as `{}`; \
             if its own MLServiceInfo.id differs, `chap-devops status` shows it as unmanaged - \
             run `chap-devops models remove {}`, then add it again with `--service-id <that id>`",
            resolved.service_id, resolved.id
        ));
    }
    let report = AddReport {
        id: resolved.id.clone(),
        service_id: resolved.service_id.clone(),
        display_name: resolved.display_name.clone(),
        source: resolved.source.describe(),
        image: resolved.image.clone(),
        tag: resolved.tag.clone(),
        image_ref: resolved.image_ref(),
        commit: resolved.commit.clone(),
        follow: resolved.follow.clone(),
        data_dir: resolved.data_dir.clone(),
        user: resolved.user.clone(),
        runtime_amd64: resolved.runtime_amd64,
        notes: notes.clone(),
        warnings,
        apply: applied,
    };
    Ok(Added::Manual(Box::new(report), Box::new(resolved)))
}

/// Remove a manually added model: disable it if it is on, then drop the
/// definition.
///
/// A marketplace model is refused rather than half-removed: there is no local
/// definition to delete, and what the caller wants is `models disable`.
pub fn remove(ctx: &Ctx, args: &ModelsRemoveArgs) -> Result<()> {
    let (mut project, _lock) = ctx.project_mut()?;
    let registry = super::registry_for(ctx, Some(&project))?;
    let Some(id) = manual_id(&project, &args.id) else {
        return Err(match crate::registry::load(&ctx.registry)?.get(&args.id) {
            Some(model) => anyhow::anyhow!(
                "{} is a marketplace model, so there is no local definition to remove; \
                 run `chap-devops models disable {}`{}",
                model.id,
                model.id,
                if args.purge { " --purge" } else { "" }
            ),
            None => ChapError::UnknownModel(args.id.clone()).into(),
        });
    };

    let entry = project.state.manual[&id].clone();
    let was_enabled = project.state.models.contains_key(&id);
    let pending = match args.keep_in_chap {
        true => None,
        false => super::enable::retire::prepare(
            &project,
            &id,
            &entry.service_id,
            false,
            "pass --keep-in-chap",
        )?,
    };
    let (mut applied, mut notes, mut purged) = (ApplyReport::default(), Vec::new(), Vec::new());
    if was_enabled {
        let (report, disable_notes) =
            super::enable::disable_enabled(&mut project, &registry, &id, args.purge, true)?;
        applied = report.apply;
        purged = report.purged;
        notes = disable_notes;
    } else if args.purge {
        match project.prefixed_volume(&crate::compose::volume_name(&id)) {
            Some(name) => {
                let (removed, line) = super::docker::purge_volume(&name);
                purged.extend(removed);
                notes.push(line);
            }
            None => notes.push(super::docker::UNNAMEABLE_VOLUME.to_string()),
        }
    }

    // The definition goes last: until it does, everything above still sees
    // the model the way the deployment recorded it.
    project.state.manual.remove(&id);
    project.save()?;
    let retired = super::enable::retire::finish(pending);

    let report = RemoveReport {
        id: id.clone(),
        service_id: entry.service_id.clone(),
        was_enabled,
        apply: applied,
        purged,
        retired,
    };
    ctx.out.report_ok(&report, |lines| {
        lines.info(format!("removed {id}")).hint(format!(
            "{id} is gone from `{}`",
            crate::project::MANUAL_MODELS_FILE
        ));
        if was_enabled {
            super::enable::summary(&report.apply, &notes, &project, lines);
        } else {
            for note in &notes {
                lines.hint(note.as_str());
            }
            lines.hint("it was not enabled, so the compose files did not change");
        }
        if let Some(retired) = &report.retired {
            super::enable::retire::line(&id, retired, lines);
        }
    })
}

/// The id a manually added model is recorded under, for whichever identifier
/// was typed: the id itself, or its compose service name.
fn manual_id(project: &Project, wanted: &str) -> Option<String> {
    if project.state.manual.contains_key(wanted) {
        return Some(wanted.to_string());
    }
    project
        .state
        .manual
        .iter()
        .find(|(_, entry)| entry.service_id == wanted)
        .map(|(id, _)| id.clone())
}

/// The `--id` value that picks a free id.
const AUTO_ID: &str = "auto";

/// The marketplace model a source names, and the exact version when the
/// source pins one: a repository the marketplace lists, or an image of one of
/// its models at one of the tags its versions publish.
///
/// A local image is never a match: a build from a checkout is what
/// `models add` is for, even of a model the marketplace lists.
fn marketplace_match(
    marketplace: &Registry,
    source: &crate::manual::source::Source,
) -> Option<(String, Option<String>)> {
    use crate::manual::source::Source;
    match source {
        Source::Repo(repo) => marketplace
            .models
            .iter()
            .find(|m| same_repo(&m.source.repository, &repo.url()))
            .map(|m| (m.id.clone(), None)),
        Source::Image(image) => marketplace.models.iter().find_map(|m| {
            if !m.source.image.eq_ignore_ascii_case(&image.image) {
                return None;
            }
            m.versions
                .iter()
                .find(|v| v.image_tag == image.tag)
                .map(|v| (m.id.clone(), Some(v.version.clone())))
        }),
        Source::Local(_) => None,
    }
}

/// The id of the enabled marketplace model that `source` names: its
/// repository, or its image at the tag of the enabled version. `None` when
/// the source names no marketplace model, or one that is not enabled here,
/// or another version of it.
pub(crate) fn enabled_twin(
    marketplace: &Registry,
    project: &Project,
    source: &str,
) -> Option<String> {
    let source = crate::manual::source::Source::parse(source).ok()?;
    let (id, version) = marketplace_match(marketplace, &source)?;
    let enabled = project.state.models.get(&id)?;
    version.is_none_or(|v| v == enabled.version).then_some(id)
}

/// The marketplace model whose repository or image `source` is, at any tag.
/// `manual` tells the entries `models add` made, which are not marketplace
/// models.
fn marketplace_twin<'a>(
    registry: &'a Registry,
    source: &crate::manual::source::Source,
    manual: impl Fn(&str) -> bool,
) -> Option<&'a crate::registry::Model> {
    use crate::manual::source::Source;
    let mut others = registry.models.iter().filter(|m| !manual(&m.id));
    match source {
        Source::Repo(repo) => others.find(|m| same_repo(&m.source.repository, &repo.url())),
        Source::Image(image) => others.find(|m| m.source.image.eq_ignore_ascii_case(&image.image)),
        Source::Local(_) => None,
    }
}

/// Whether two repository URLs name the same repository.
fn same_repo(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.trim()
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .to_lowercase()
    };
    norm(a) == norm(b)
}

/// The id an earlier add recorded `source` under: the same repository, or the
/// same image at the same tag. `None` when nothing was added from it, or when
/// `id` asks for an entry other than the one found.
pub(crate) fn added_as(project: &Project, source: &str, id: Option<&str>) -> Option<String> {
    use crate::manual::source::Source;
    let source = Source::parse(source).ok()?;
    let (found, _) = project.state.manual.iter().find(|(_, m)| match &source {
        Source::Repo(repo) => m
            .repository
            .as_deref()
            .is_some_and(|r| same_repo(r, &repo.url())),
        Source::Image(image) | Source::Local(image) => {
            m.repository.is_none()
                && m.image.eq_ignore_ascii_case(&image.image)
                && m.tag == image.tag
        }
    })?;
    match id.map(str::trim) {
        None | Some(AUTO_ID) => Some(found.clone()),
        Some(other) => (other == found).then(|| found.clone()),
    }
}

/// The source's own id, or that id with the lowest `_2`, `_3`, ... suffix
/// that neither the marketplace nor this deployment uses yet.
fn free_id(ctx: &Ctx, project: &Project, source: &str) -> Result<String> {
    let base = crate::manual::source::Source::parse(source)?.default_id();
    let marketplace = crate::registry::load(&ctx.registry)?;
    let taken = |id: &str| {
        marketplace.models.iter().any(|m| m.id == id) || project.state.manual.contains_key(id)
    };
    let id = std::iter::once(base.clone())
        .chain((2..).map(|n| format!("{base}_{n}")))
        .find(|id| !taken(id))
        .expect("an unbounded sequence has a free id");
    Ok(id)
}

/// Whether the id and the service name this add wants are free.
///
/// The marketplace is checked first because it is the one this deployment
/// does not control: an id it already lists is a model to enable, not one to
/// add, and a service name it holds would make two overlays fight over one
/// compose service.
fn check_free(project: &Project, marketplace: &Registry, names: &Names) -> Result<()> {
    let id = &names.id;
    if let Some(model) = marketplace.models.iter().find(|m| m.id == *id) {
        return Err(anyhow::anyhow!(
            "the marketplace already lists {id} ({}); run `chap-devops models enable {id}`, \
             or pass `--id auto` (or `--id <other>`) to add this one beside it",
            model.display_name
        ));
    }
    if let Some(existing) = project.state.manual.get(id) {
        return Err(anyhow::anyhow!(
            "{id} was already added, from {}; run `chap-devops models remove {id}` first, \
             or pass `--id <other>`",
            existing
                .repository
                .clone()
                .unwrap_or_else(|| existing.image_ref())
        ));
    }
    let service_id = &names.service_id;
    // A local build of a marketplace model is the one case where sharing its
    // service name is the point: it is that model, built from a checkout. It
    // stands in for the marketplace entry, so the two cannot both be enabled.
    let local = matches!(names.source, crate::manual::source::Source::Local(_));
    if let Some(model) = marketplace
        .models
        .iter()
        .find(|m| m.service_id == *service_id)
    {
        if local && !project.state.models.contains_key(&model.id) {
            return Ok(());
        }
        if local {
            return Err(anyhow::anyhow!(
                "the marketplace model {id} is enabled as the compose service `{service_id}`; \
                 disable it with `chap-devops models disable {id}` to run this local build in its place",
                id = model.id
            ));
        }
        return Err(anyhow::anyhow!(
            "the marketplace model {id} already uses the compose service `{service_id}`; \
             if this image is that model, `chap-devops models enable {id}` runs it, and \
             otherwise pass `--service-id <other>`",
            id = model.id
        ));
    }
    if let Some((other, _)) = project
        .state
        .manual
        .iter()
        .find(|(_, entry)| entry.service_id == *service_id)
    {
        return Err(anyhow::anyhow!(
            "{other} already uses the compose service `{service_id}`; \
             pass `--service-id <other>`"
        ));
    }
    Ok(())
}

/// The block that says what was resolved, above the lines `enable` prints.
fn added_block(resolved: &Resolved, lines: &mut Report) {
    // The service id in brackets only when it says something the id does not.
    let service = match resolved.service_id == resolved.id {
        true => String::new(),
        false => format!(" ({})", resolved.service_id),
    };
    lines.info(format!("added {}{service}", resolved.id));
    let pin = match &resolved.commit {
        Some(commit) => format!("{} (commit {})", resolved.tag, short(commit)),
        None => resolved.tag.clone(),
    };
    let follows = match &resolved.follow {
        Some(branch) => format!("{branch} (`chap-devops update` moves the pin)"),
        None => "nothing (pinned)".to_string(),
    };
    lines
        .hint(format!("source: {}", resolved.source.describe()))
        .hint(format!("image: {}", resolved.image_ref()))
        .hint(format!("pin: {pin}"))
        .hint(format!("follows: {follows}"))
        .hint(format!(
            "data dir: {} ({})",
            resolved.data_dir,
            resolved.data_dir_from.label()
        ))
        .hint(format!(
            "user: {} ({})",
            resolved.user,
            resolved.user_from.label()
        ));
}

/// The first seven characters of a commit, as the image tag uses.
fn short(commit: &str) -> String {
    commit.chars().take(7).collect()
}

/// Today in UTC, as `YYYY-MM-DD`.
fn today() -> String {
    crate::backup::timestamp(crate::backup::now())
        .chars()
        .take(10)
        .collect()
}

#[cfg(test)]
mod tests;
