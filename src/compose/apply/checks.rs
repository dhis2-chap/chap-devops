//! The checks of a selection that change nothing: the models, their service
//! names, and the `--data-dir` and `--user` values.

use super::{Selection, enabled_id};
use crate::error::{ChapError, Result};
use crate::project::Project;
use crate::registry::Registry;

/// Check a whole selection against the registry and the project state,
/// changing nothing.
///
/// [`apply`](super::apply) starts here, so every caller gets these answers before a file is
/// written. `chap-devops init --force` calls it directly as well: it deletes the
/// previous deployment's overlays before handing over to [`apply`](super::apply), and a
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
