//! One module per command, plus the [`Ctx`] every command receives.

pub mod api;
pub mod auth;
pub mod backup;
pub mod bench;
pub mod chap;
pub(crate) mod chap_jobs;
pub mod chapkit;
pub mod cleanup;
pub mod completions;
pub mod components;
pub mod configs;
pub mod dhis2;
pub mod docker;
pub mod docs;
pub mod doctor;
pub mod enable;
pub mod init;
pub mod jobs;
pub mod manual_models;
pub mod models;
pub mod models_new;
pub mod modeltest;
pub mod open;
pub mod registry;
pub mod restore;
pub mod run;
pub mod selfcmd;
pub mod status;
pub mod sync;
pub mod top;
pub mod tui;
pub mod update;

use crate::cli::Cli;
use crate::error::Result;
use crate::output::Out;
use crate::project::Project;
use crate::registry::{DEFAULT_TIMEOUT, Registry, RegistryOptions};
use std::path::PathBuf;

/// The catalogue a command works against: the marketplace, plus the models
/// this deployment defines itself.
///
/// Every command that resolves a model goes through here rather than through
/// [`crate::registry::load`], which is what lets `chap-devops models add` be a
/// change to one file and nothing else. `project` is `None` for a command
/// that ran outside a deployment, where there are no local definitions to
/// add.
pub fn registry_for(ctx: &Ctx, project: Option<&Project>) -> Result<Registry> {
    let mut registry = crate::registry::load(&ctx.registry)?;
    add_manual(&mut registry, project);
    Ok(registry)
}

/// [`registry_for`] with the network refresh `chap-devops update` needs: no
/// fallback to a cached catalogue, because an update from a stale one is not
/// an update.
pub fn refreshed_registry_for(ctx: &Ctx, project: Option<&Project>) -> Result<Registry> {
    let mut registry = crate::registry::update(&ctx.registry)?;
    add_manual(&mut registry, project);
    Ok(registry)
}

/// Fold a project's manual definitions in, showing whatever they collide with.
fn add_manual(registry: &mut Registry, project: Option<&Project>) {
    let Some(project) = project else {
        return;
    };
    for warning in registry.with_manual(&project.state.manual) {
        crate::output::warn(&warning);
    }
}

/// Everything a command needs that came from the global flags.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub out: Out,
    pub project_dir: PathBuf,
    pub registry: RegistryOptions,
    pub cli_version: &'static str,
}

impl Ctx {
    /// Build a context from the parsed global flags.
    pub fn from_cli(cli: &Cli) -> Ctx {
        // The stderr side of the CLI has no `Ctx` to consult, so the flags
        // are recorded once here for `output::warn` and the trace helpers.
        crate::output::set_no_color(cli.no_color);
        let verbosity = crate::output::set_verbosity(cli.verbose, cli.debug);
        Ctx {
            out: Out {
                verbosity,
                ..Out::detect(cli.json, cli.no_color)
            },
            project_dir: cli.project_dir.clone(),
            registry: RegistryOptions {
                url: cli.registry_url.clone(),
                offline: cli.offline,
                cache_dir: cli
                    .cache_dir
                    .clone()
                    .unwrap_or_else(crate::paths::cache_dir),
                timeout: DEFAULT_TIMEOUT,
            },
            cli_version: env!("CARGO_PKG_VERSION"),
        }
    }

    /// Load the project that contains [`Ctx::project_dir`], walking up parent
    /// directories the way git finds `.git`.
    pub fn project(&self) -> crate::error::Result<crate::project::Project> {
        let project = crate::project::Project::find(&self.project_dir)?;
        self.trace_project(&project);
        Ok(project)
    }

    /// Which deployment a command actually landed on, under `-d`: the first
    /// thing to check when it is not the one you meant.
    fn trace_project(&self, project: &crate::project::Project) {
        self.out.debug(&format!(
            "deployment: {} (state in {})",
            project.dir.display(),
            project
                .dir
                .join(crate::project::CHAP_DEVOPS_DIR)
                .join(crate::project::PROJECT_FILE)
                .display()
        ));
    }

    /// [`Ctx::project`] for a command that changes the deployment: the state
    /// is locked before it is read, and stays locked until the returned
    /// [`StateLock`] is dropped.
    pub fn project_mut(
        &self,
    ) -> crate::error::Result<(crate::project::Project, crate::project::StateLock)> {
        let (project, lock) = crate::project::Project::find_locked(&self.project_dir)?;
        self.trace_project(&project);
        Ok((project, lock))
    }
}

#[cfg(test)]
mod tests;
