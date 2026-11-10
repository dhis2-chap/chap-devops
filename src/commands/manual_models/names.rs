//! What a `models add` source names, and the checks that its id and its
//! service name are free.

use crate::commands::Ctx;
use crate::error::Result;
use crate::manual::Names;
use crate::project::Project;
use crate::registry::Registry;

/// The `--id` value that picks a free id.
pub(super) const AUTO_ID: &str = "auto";

/// The marketplace model a source names, and the exact version when the
/// source pins one: a repository the marketplace lists, or an image of one of
/// its models at one of the tags its versions publish.
///
/// A local image is never a match: a build from a checkout is what
/// `models add` is for, even of a model the marketplace lists.
pub(super) fn marketplace_match(
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
pub(super) fn marketplace_twin<'a>(
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
pub(super) fn free_id(ctx: &Ctx, project: &Project, source: &str) -> Result<String> {
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
/// Refuse a service id that is live in chap-core and that no model of this
/// deployment has, as `chap-admin` does: the new model would register in its
/// place. When chap-core does not answer, the model is added without the check.
pub(super) fn check_not_live(ctx: &Ctx, project: &Project, service_id: &str) -> Result<()> {
    if ctx.registry.offline
        || !project.state.components.has_chap_core_api()
        || project
            .state
            .models
            .values()
            .any(|model| model.service_id == service_id)
    {
        return Ok(());
    }
    let token = crate::api::token_for(Some(&project.dir));
    let api = crate::api::Api::new(&project.api_url(), token, std::time::Duration::from_secs(3));
    let Ok(services) = crate::configs::sync::registered_services(&api) else {
        ctx.out.verbose(&format!(
            "chap-core at {} did not answer, so chap-devops did not check its registered models",
            api.base()
        ));
        return Ok(());
    };
    match services.iter().find(|service| service.id == service_id) {
        Some(service) => Err(anyhow::anyhow!(
            "chap-core already has a model registered as `{service_id}` at {}, and it is not a \
             model of this deployment; stop that model, or pass `--service-id <other>`",
            service.url
        )),
        None => Ok(()),
    }
}

pub(super) fn check_free(project: &Project, marketplace: &Registry, names: &Names) -> Result<()> {
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
