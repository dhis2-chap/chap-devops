//! `chap-devops update [--dry-run]` — move the pins forward and pull.
//!
//! Two kinds of pin move here. A model that follows a channel is re-resolved
//! against the marketplace, and the only other thing that moves those is
//! `chap-devops models enable`; exact pins (`--version`) never move. chap-core's own
//! pin moves too when it is a release tag and a newer release exists, which
//! also re-fetches the `compose.ghcr.yml` that release publishes. A moving
//! chap-core tag (`latest`, `master`, `dev`) is only refreshed by the pull,
//! unless `--pin-chap-core` turns it into a release pin.
//!
//! `--chap-tag` is the other direction: not forward, but somewhere else
//! entirely. It is the same run with chap-core's destination decided by the
//! operator rather than by the release feed, and the one move that can cost
//! data - a release older than the one running, or a release after a moving
//! tag, since `dev` and `master` are ahead of every release - is named and
//! confirmed before anything is fetched, written or pulled. `--list-tags`
//! answers where a deployment can go, and writes nothing at all.
//!
//! What it does not do is touch a container. The run reads: the plan, then
//! the pull, then the closing lines: what moved and which running services are
//! now out of date. Applying that is `chap-devops restart`, and starting a
//! deployment that is down is `chap-devops up`; an update that did either would be
//! a deployment nobody asked for.
//!
//! "Out of date" is two questions asked of every running container. Is the
//! image it runs still the image its reference points at, now that the pull
//! has finished? And is the configuration hash compose stamped on it still
//! the hash compose computes for the files as they are? Either answer being
//! no means the service is running something the project no longer describes.

mod chap_core;
mod components;
mod models;
mod registry;
mod report;
mod restart;
mod tags;

use crate::cli::UpdateArgs;
use crate::commands::Ctx;
use crate::compose::sync::refresh_env_pin;
use crate::compose::{sync, tag_env_var};
use crate::docker;
use crate::error::{ChapError, Result};
use crate::manual;
use crate::output;
use crate::project::{ComposeSource, Project};
use crate::registry::Provenance;
use chap_core::{
    ChapCoreUpdate, apply_chap_core, backwards_warning, check_chap_images, check_chap_tag,
    confirm_backwards, exact_pin_warning, lookup_latest, plan_chap_core, pull_failed_after_switch,
};
use components::{ComponentUpdate, dhis2_pull_note, plan_components};
use models::{ModelUpdate, newest_published, plan, resolve_users};
use report::{closing, dry_run, plan_text, registry_hint, updated};
use restart::{pulled_new, restart_needed, service_checks};
use serde::Serialize;
use tags::list_tags;

/// The shape of `update --json`, and what the human output is built from.
#[derive(Debug, Serialize)]
pub struct UpdateReport {
    pub registry: RegistryInfo,
    pub models: Vec<ModelUpdate>,
    /// `None` for a deployment chap-core is not a component of: there is no
    /// pin to move and no release to look up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chap_core: Option<ChapCoreUpdate>,
    /// One row per enabled component other than chap-core. They follow moving
    /// tags, so there is no pin to move - only a pull to report.
    pub components: Vec<ComponentUpdate>,
    pub pulled: bool,
    /// Services the pull brought a different image for. A moving tag that
    /// still points at the image this machine already had is not one of them.
    pub pulled_new: Vec<String>,
    /// Running services whose image or configuration no longer matches what
    /// this project describes: what `chap-devops restart` would recreate.
    pub restart_needed: Vec<String>,
    /// Whether any of this project's containers was up, `null` when docker
    /// could not be asked.
    pub stack_running: Option<bool>,
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
pub struct RegistryInfo {
    pub url: String,
    pub provenance: Provenance,
}

impl UpdateReport {
    pub fn changed(&self) -> impl Iterator<Item = &ModelUpdate> {
        self.models.iter().filter(|m| m.changed)
    }
}

/// Refresh the registry, re-resolve every channel-following model, sync and
/// pull, then say what needs restarting.
pub fn run(ctx: &Ctx, args: &UpdateArgs) -> Result<()> {
    // Without a deployment there is no pin to move, but the registry is this
    // machine's and can still be refreshed.
    if crate::project::Project::find_root(&ctx.project_dir).is_none() {
        return registry::refresh(ctx, args);
    }
    // The warnings of the run go under its closing lines. A run that fails
    // part way prints the ones it has, so the error does not hide them.
    let mut warnings = Vec::new();
    let result = update(ctx, args, &mut warnings);
    if result.is_err() {
        for warning in &warnings {
            output::warn(warning);
        }
    }
    result
}

/// The run in a deployment, with the warnings it collects on the way.
fn update(ctx: &Ctx, args: &UpdateArgs, warnings: &mut Vec<String>) -> Result<()> {
    let (mut project, _lock) = ctx.project_mut()?;
    // A listing writes nothing and asks the marketplace nothing, so it is
    // answered before the refresh that the rest of the command needs.
    if args.list_tags {
        return list_tags(ctx, &project);
    }
    // A chap-core built from a checkout has no pin either: the checkout is
    // the version, and `chap-devops up` builds it as it is.
    if let ComposeSource::Checkout { path } = &project.state.chap_compose_source
        && (args.chap_tag.is_some() || args.pin_chap_core)
    {
        return Err(anyhow::anyhow!(
            "this deployment builds chap-core from the checkout at {path}; check out the \
             version you want there and run `chap-devops up`"
        ));
    }
    let own_chap_core = project
        .state
        .components
        .is_enabled(crate::components::Component::ChapCore);
    let has_chap_core = own_chap_core
        && !matches!(
            project.state.chap_compose_source,
            ComposeSource::Checkout { .. }
        );
    if !own_chap_core && (args.chap_tag.is_some() || args.pin_chap_core) {
        return Err(anyhow::anyhow!(
            "{} moves chap-core's pin, and this deployment has no chap-core; \
             `chap-devops components enable chap-core` adds it",
            if args.chap_tag.is_some() {
                "--chap-tag"
            } else {
                "--pin-chap-core"
            }
        ));
    }
    // A run in a deployment refreshes the registry, which needs the network;
    // the listing above is the one part that works without it.
    if ctx.registry.offline {
        return Err(ChapError::Usage(
            "`chap-devops update` refreshes the marketplace registry, which needs the network; drop \
             --offline, or run `chap-devops update --list-tags` to see the chap-core tags offline"
                .to_string(),
        )
        .into());
    }
    // The tag `--chap-tag` names is checked before anything else happens: a
    // typo should cost one lookup, not a marketplace refresh and a pull.
    let requested = match &args.chap_tag {
        Some(tag) => Some(check_chap_tag(
            tag.trim(),
            ctx.registry.offline,
            ctx.registry.timeout,
            warnings,
        )?),
        None => None,
    };
    if let Some(tag) = &requested
        && !ctx.registry.offline
    {
        let ghcr = manual::Endpoints::from_env(ctx.registry.offline).ghcr_url;
        check_chap_images(tag, &ghcr, ctx.registry.timeout, warnings)?;
    }
    let requested = requested.as_deref();
    // No fallback on purpose: an update from a stale catalogue is not an update.
    let registry = super::refreshed_registry_for(ctx, Some(&project))?;

    // The manual entries are resolved against their own repositories, which
    // is the one lookup the registry refresh above cannot make.
    let endpoints = manual::Endpoints::from_env(ctx.registry.offline);
    let manual_warnings = std::cell::RefCell::new(Vec::new());
    let models = plan(&project, &registry, &|id, entry| {
        newest_published(id, entry, &endpoints, &mut manual_warnings.borrow_mut())
    })?;
    warnings.extend(manual_warnings.into_inner());
    // A new tag is a new image, and an image can change what it runs as
    // between two builds - which is the difference between a model that works
    // and one whose own binaries it may not execute. Only the rows that move
    // are asked, and only about the user: moving the data directory here
    // would point the service at an empty path next to a volume holding its
    // database.
    let mut models = models;
    resolve_users(&project, &mut models, &endpoints, warnings);
    // Without chap-core there is no pin for the newest release to move.
    let latest = has_chap_core
        .then(|| {
            lookup_latest(
                &project.state.chap_image_tag,
                args.pin_chap_core,
                requested,
                args.dry_run,
                ctx.registry.timeout,
                warnings,
            )
        })
        .flatten();
    // `--pin-chap-core` asked for a pin, and a moving tag without the newest
    // release has nothing to be pinned to. Ending on "already up to date"
    // would claim a pin this run did not make.
    if args.pin_chap_core
        && has_chap_core
        && latest.is_none()
        && crate::chapcore::is_moving_tag(&project.state.chap_image_tag)
    {
        return Err(anyhow::anyhow!(
            "--pin-chap-core pinned nothing, because the newest chap-core release could not be \
             looked up; try again later, or run `chap-devops update --chap-tag <TAG>` with a release \
             from `chap-devops update --list-tags`"
        ));
    }
    // Nothing here starts or stops a container, so this answer holds for the
    // whole run and the dry run answers the same question.
    let running = docker::running_containers(&project);
    let mut report = UpdateReport {
        registry: RegistryInfo {
            url: registry.url.clone(),
            provenance: registry.provenance.clone(),
        },
        models,
        chap_core: has_chap_core
            .then(|| plan_chap_core(&project, latest, args.pin_chap_core, requested)),
        components: plan_components(&project),
        pulled: false,
        pulled_new: Vec::new(),
        restart_needed: Vec::new(),
        stack_running: running.as_ref().map(|c| !c.is_empty()),
        dry_run: args.dry_run,
    };

    // The plan first, before any of docker's own noise: it is what the pull
    // is about to act on, and it is the half a person reads.
    say(ctx, &plan_text(&report, &ctx.out));
    // Going back to an older chap-core is the one move this command makes
    // that can cost data. A real run names it before anything is fetched,
    // written or pulled; a dry run names it with its result, where it is half
    // of what there is to see.
    if let Some(core) = report.chap_core.as_ref().filter(|c| c.backwards) {
        let warning = backwards_warning(&core.old_tag, &core.new_tag);
        match args.dry_run {
            true => warnings.push(warning),
            false => output::warn(&warning),
        }
    }
    // DHIS2 needs no move to be at risk: the pull alone can bring an image
    // that migrates `dhis2_db` on the next `chap-devops up`, and nothing in the plan
    // above would show it. In the dry run too, where it is the only thing this
    // run has to say about that.
    warnings.extend(dhis2_pull_note(&project, &report.components));
    // A tag nothing will move again is named once the pin moves to it.
    let exact_pin = report
        .chap_core
        .as_ref()
        .filter(|c| c.changed && c.requested.is_some())
        .filter(|c| crate::chapcore::tag_kind(&c.new_tag) == crate::chapcore::TagKind::Other)
        .map(|c| c.new_tag.clone());
    if args.dry_run {
        warnings.extend(exact_pin.map(|tag| exact_pin_warning(&tag, true)));
        let warnings = std::mem::take(warnings);
        return ctx.out.report(&report, |lines| {
            dry_run(lines, updated(&report).as_deref());
            registry_hint(&report, lines);
            for warning in &warnings {
                lines.warning(warning.as_str());
            }
        });
    }
    // And confirmed, once it is about to actually happen.
    if let Some(core) = report.chap_core.as_ref().filter(|c| c.backwards)
        && !args.yes
    {
        confirm_backwards(ctx, core)?;
    }

    if let Some(core) = report.chap_core.as_mut().filter(|c| c.changed) {
        apply_chap_core(&mut project, core, ctx.registry.timeout, warnings)?;
    }
    warnings.extend(exact_pin.map(|tag| exact_pin_warning(&tag, false)));
    for change in report.models.iter().filter(|m| m.changed) {
        // A manual entry's definition carries the pin as well: the recorded
        // model says what runs, and `models-manual.yaml` says what the next
        // `models enable` would bring back.
        if let Some(manual) = project.state.manual.get_mut(&change.id) {
            manual.tag = change.new_tag.clone();
            if let Some(commit) = &change.new_commit {
                manual.commit = Some(commit.clone());
            }
        }
        let manual = project.state.manual.contains_key(&change.id);
        let entry = project
            .state
            .models
            .get_mut(&change.id)
            .expect("plan only lists enabled models");
        record_previous(entry, &change.new_pin(), manual);
        entry.version = change.new_version.clone();
        entry.image_tag = change.new_tag.clone();
        entry.image_digest = change.new_digest.clone();
        entry.user = change.new_user.clone();
        entry.user_from = change.user_from;
        entry.reads_port = change.reads_port;
        entry.multi_arch = change.multi_arch;
        refresh_env_pin(&project.dir, &tag_env_var(&change.id), &change.new_pin())?;
    }
    let synced = sync(&mut project, &registry, false)?;
    warnings.extend(synced.warnings.iter().cloned());

    // After the sync, so these are the images the files pin now, and before
    // the pull, so the difference the pull makes can be seen at all.
    let images = docker::service_images(&project);
    let refs: Vec<String> = images.values().cloned().collect();
    let before = docker::image_ids(&refs);

    // A pull that fails once chap-core's pin has moved leaves the deployment
    // describing images docker could not fetch - upstream publishing a tag
    // for one of its images and not the other is enough to get there - so the
    // failure says where the pin is now and how to put it back.
    if let Err(err) = run_compose(&project, &["pull".to_string()]) {
        if let Some(core) = report.chap_core.as_ref().filter(|c| c.changed) {
            return Err(err.context(pull_failed_after_switch(&core.old_tag, &core.new_tag)));
        }
        return Err(err);
    }
    report.pulled = true;

    let after = docker::image_ids(&refs);
    report.pulled_new = pulled_new(&images, &before, &after);
    let running = running.unwrap_or_default();
    let builds =
        docker::container_builds(&running.iter().map(|c| c.id.clone()).collect::<Vec<_>>());
    let checks = service_checks(
        &running,
        &builds,
        &images,
        &after,
        &docker::config_hashes(&project),
    );
    report.restart_needed = restart_needed(&checks);

    let warnings = std::mem::take(warnings);
    ctx.out.report(&report, |lines| {
        closing(
            lines,
            updated(&report).as_deref(),
            &report.restart_needed,
            report.stack_running,
        );
        registry_hint(&report, lines);
        for warning in &warnings {
            lines.warning(warning.as_str());
        }
    })
}

/// Keep the pin that worked last, for `chap-devops up` to go back to when
/// chap-core refuses the new version. An earlier update that was never
/// configured keeps the pin from before it. A model from `models add` also
/// has its pin in `models-manual.yaml`, so it gets no previous pin.
fn record_previous(entry: &mut crate::project::EnabledModel, new_pin: &str, manual: bool) {
    if manual || entry.previous.is_some() || entry.pinned_tag() == new_pin {
        return;
    }
    entry.previous = Some(crate::project::PreviousPin {
        version: entry.version.clone(),
        image_tag: entry.image_tag.clone(),
        image_digest: entry.image_digest.clone(),
        user: entry.user.clone(),
        user_from: entry.user_from,
        reads_port: entry.reads_port,
        multi_arch: entry.multi_arch,
    });
}

/// Print one of the command's own sections, unless a parser is reading.
///
/// Under `--json` stdout carries exactly one document, printed at the end;
/// everything the human rendering would have said is left out rather than
/// mixed into it.
fn say(ctx: &Ctx, text: &str) {
    if !ctx.out.json && !text.is_empty() {
        print!("{text}");
    }
}

fn run_compose(project: &Project, args: &[String]) -> Result<()> {
    let code = docker::run_compose(project, args)?;
    if code != 0 {
        return Err(ChapError::DockerFailed(code).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
