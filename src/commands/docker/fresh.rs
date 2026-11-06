//! The first `up` of a deployment on a moving chap-core tag pulls the
//! chap-core images first.
//!
//! Compose uses the image of a tag that is already on the machine. For
//! `master` or `dev`, that image can be days old, and the API and the worker
//! can come from different days: the API then sends the worker arguments it
//! does not know (`unexpected keyword argument 'queue_baselines'`), and every
//! job fails. Later `up`s keep what runs; `chap-devops update` or `up --pull`
//! move them.

use crate::commands::Ctx;
use crate::components::Component;
use crate::docker::Container;
use crate::project::{ComposeSource, Project};

/// The compose services that run the chap-core images.
pub(super) const CHAP_CORE_SERVICES: [&str; 2] = ["chap", "worker"];

/// The services to pull before this `up`, or none.
///
/// Only for a chap-core this deployment runs from published images, on a
/// moving tag, with no container of it yet, and not under `--offline`.
pub(super) fn pulls(project: &Project, before: &[Container], offline: bool) -> Vec<String> {
    let own = project.state.components.is_enabled(Component::ChapCore)
        && !matches!(
            project.state.chap_compose_source,
            ComposeSource::Checkout { .. }
        );
    let started_before = before
        .iter()
        .any(|c| CHAP_CORE_SERVICES.contains(&c.service.as_str()));
    if offline
        || !own
        || started_before
        || !crate::chapcore::is_moving_tag(&project.state.chap_image_tag)
    {
        return Vec::new();
    }
    CHAP_CORE_SERVICES.iter().map(|s| s.to_string()).collect()
}

/// The compose arguments that pull `services`.
pub(super) fn pull_args(services: &[String]) -> Vec<String> {
    let mut args = vec!["pull".to_string(), "--quiet".to_string()];
    args.extend(services.iter().cloned());
    args
}

/// Pull the chap-core images before the first start on a moving tag; see
/// the module docs. A failed pull is a warning: the images on this machine still run.
pub(super) fn pull_first(ctx: &Ctx, project: &Project, before: &[Container]) {
    let services = pulls(project, before, ctx.registry.offline);
    if services.is_empty() {
        return;
    }
    let tag = &project.state.chap_image_tag;
    match crate::docker::run_compose(project, &pull_args(&services)) {
        Ok(0) => super::note(
            ctx,
            &ctx.out.backticks(&format!(
                "pulled {} first: `{tag}` is a moving tag, and this is the first start",
                services.join(", ")
            )),
        ),
        _ => crate::output::warn(&format!(
            "could not pull {} at `{tag}`, so the images on this machine start, which can be older; \
             run `chap-devops up --pull` when the registry answers",
            services.join(", ")
        )),
    }
}
