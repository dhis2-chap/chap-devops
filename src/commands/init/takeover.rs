//! `init --adopt`: check the flags and the old deployment, carry its
//! secrets, and report what came over.

use super::{adopt, env, resolve_dir};
use crate::cli::InitArgs;
use crate::commands::Ctx;
use crate::error::{ChapError, Result};
use crate::project::Project;
use crate::registry::Registry;
use std::path::Path;

/// Read the deployment `--adopt` names and check it with docker, refusing
/// the flags a takeover decides itself.
pub(super) fn start_adoption(
    ctx: &Ctx,
    args: &InitArgs,
    dir: &Path,
    old: &Path,
    registry: &Registry,
) -> Result<adopt::Adoption> {
    let refused = [
        (args.models.trim() != "none", "--models"),
        (args.interactive, "--interactive"),
        (args.source.is_some(), "--source"),
        (args.chap_core_url.is_some(), "--chap-core-url"),
        (args.api_token.is_some(), "--api-token"),
        (args.fresh_env, "--fresh-env"),
        (args.no_env, "--no-env"),
    ];
    if let Some((_, flag)) = refused.iter().find(|(given, _)| *given) {
        return Err(ChapError::Usage(format!(
            "--adopt takes the models and the secrets from the old deployment, so {flag} has \
             nothing to do; run it without {flag}"
        ))
        .into());
    }
    if Project::exists(dir) {
        return Err(anyhow::anyhow!(
            "{} is a deployment already, and --adopt makes a new one; give --adopt a new \
             directory",
            dir.display()
        ));
    }
    let old = resolve_dir(old)?;
    let mut adoption = adopt::read(&old, registry, &ctx.registry.url)?;
    adopt::check(&mut adoption, &adopt::Docker::real())?;
    Ok(adoption)
}

/// The secrets of a takeover: the old token, and the old registration key
/// or a new one, because chap-devops writes a token only with a key.
pub(super) fn adopted_secrets(old: Option<&adopt::OldEnv>) -> Result<Option<env::Secrets>> {
    let Some(token) = old.and_then(|old| old.api_token.clone()) else {
        return Ok(None);
    };
    Ok(Some(env::Secrets {
        api_token: token,
        registration_key: match old.and_then(|old| old.registration_key.clone()) {
            Some(key) => key,
            None => crate::auth::random_secret()?,
        },
    }))
}

/// Add the models that come over as `models add` entries, and copy the data
/// of every model. Each failure is a line; the deployment is written either
/// way.
pub(super) fn finish_adoption(
    ctx: &Ctx,
    project: &mut Project,
    adoption: &adopt::Adoption,
) -> Vec<String> {
    let mut lines = Vec::new();
    for args in adopt::manual_args(adoption) {
        let id = args.id.clone().unwrap_or_default();
        if let Err(err) = crate::commands::manual_models::add_in(ctx, project, &args) {
            lines.push(format!(
                "could not add {id}: {err:#}; run `chap-devops models add {} --id {id} \
                 --service-id {}` in the new deployment",
                args.source,
                args.service_id.as_deref().unwrap_or_default()
            ));
        }
    }
    adopt::copy_model_data(adoption, project, &mut lines);
    lines
}

/// The closing lines of a takeover.
pub(super) fn adoption_lines(
    adoption: &adopt::Adoption,
    adopted: &[String],
    lines: &mut crate::output::Report,
) {
    lines.info(format!(
        "took over the deployment in {} (compose project {})",
        adoption.dir.display(),
        adoption.project
    ));
    for warning in &adoption.warnings {
        lines.warning(warning.as_str());
    }
    for line in adopted {
        match line.starts_with("could not") {
            true => lines.warning(line.as_str()),
            false => lines.info(line.as_str()),
        };
    }
    for note in &adoption.notes {
        lines.hint(note.as_str());
    }
}
