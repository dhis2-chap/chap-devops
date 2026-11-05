//! The pin a model had before `chap-devops update` moved it, settled after
//! the configure step of `chap-devops up`.
//!
//! `chap-admin update` stops the new container and starts the previous image
//! when chap-core refuses the new version. `chap-devops update` only moves the
//! pin, and the new container starts with `chap-devops up`, so the undo is
//! here: when the configure step failed and chap-core retired the new
//! version, the model goes back to its previous pin. A model that chap-core
//! configured forgets its previous pin.

use crate::commands::Ctx;
use crate::compose::{sync, tag_env_var};
use crate::configs::sync::{Folded, Outcome, Undo};
use crate::docker;
use crate::error::Result;
use crate::project::Project;

/// What to do with the previous pin of one model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Settle {
    /// chap-core configured the new version: forget the previous pin.
    Forget(String),
    /// chap-core refused the new version and retired it: go back.
    Restore(String),
}

/// The models whose previous pin the outcomes settle.
pub(super) fn decide(project: &Project, configured: &Folded) -> Vec<Settle> {
    configured
        .models
        .iter()
        .filter(|model| {
            project
                .state
                .models
                .get(&model.id)
                .is_some_and(|enabled| enabled.previous.is_some())
        })
        .filter_map(|model| match &model.outcome {
            Outcome::Created { .. } | Outcome::AlreadyConfigured => {
                Some(Settle::Forget(model.id.clone()))
            }
            Outcome::Failed {
                undo: Some(Undo::Retired { .. }),
                ..
            } => Some(Settle::Restore(model.id.clone())),
            _ => None,
        })
        .collect()
}

/// Settle the previous pins, and return the lines that say what changed.
pub(super) fn settle(ctx: &Ctx, project: &mut Project, configured: &Folded) -> Vec<String> {
    let decided = decide(project, configured);
    if decided.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut restored = Vec::new();
    for settle in &decided {
        match settle {
            Settle::Forget(id) => {
                if let Some(enabled) = project.state.models.get_mut(id) {
                    enabled.previous = None;
                }
            }
            Settle::Restore(id) => {
                let Some(enabled) = project.state.models.get_mut(id) else {
                    continue;
                };
                let Some(previous) = enabled.previous.take() else {
                    continue;
                };
                lines.push(format!(
                    "chap-core refused {id} {}, so chap-devops started {} again; `chap-devops \
                     update` moves it again",
                    enabled.version, previous.version
                ));
                enabled.version = previous.version;
                enabled.image_tag = previous.image_tag;
                enabled.image_digest = previous.image_digest;
                enabled.user = previous.user;
                enabled.user_from = previous.user_from;
                enabled.reads_port = previous.reads_port;
                restored.push((id.clone(), enabled.service_id.clone()));
            }
        }
    }
    if let Err(err) = restart(ctx, project, &restored) {
        lines.push(format!(
            "could not go back to the previous version: {err:#}; run `chap-devops up`"
        ));
    }
    lines
}

/// Write the state, render the files again and start the restored services.
fn restart(ctx: &Ctx, project: &mut Project, restored: &[(String, String)]) -> Result<()> {
    for (id, _) in restored {
        let tag = project.state.models[id].pinned_tag();
        sync::refresh_env_pin(&project.dir, &tag_env_var(id), &tag)?;
    }
    if restored.is_empty() {
        return project.save();
    }
    let registry = crate::commands::registry_for(ctx, Some(project))?;
    sync::sync(project, &registry, false)?;
    let mut args = vec!["up".to_string(), "-d".to_string(), "--no-deps".to_string()];
    args.extend(restored.iter().map(|(_, service)| service.clone()));
    let code = docker::run_compose(project, &args)?;
    if code != 0 {
        return Err(crate::error::ChapError::DockerFailed(code).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
