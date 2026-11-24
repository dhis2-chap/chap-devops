//! An unstable Modeling App: whether chap-frontend's `main` has a build that
//! is newer than the one this deployment's DHIS2 has.
//!
//! `update` does not install it. That needs DHIS2 up and its credentials,
//! which `chap-devops dhis2 apps` has and `update` does not. `update` asks
//! GitHub, compares with the build `.chap-devops/components.yaml` records,
//! and names the command.

use crate::components::ModelingApp;
use crate::output::Report;
use crate::project::Project;
use serde::Serialize;
use std::time::Duration;

/// What `update` found for an unstable Modeling App.
#[derive(Debug, Clone, Serialize)]
pub struct ModelingAppUpdate {
    /// The commit of the build GitHub lists now.
    pub newest: String,
    /// The commit of the build chap-devops installed last, `None` when it
    /// installed none.
    pub installed: Option<String>,
    /// Whether the two are different builds.
    pub newer: bool,
}

/// The check, for a deployment with a DHIS2 whose setting is `unstable`.
/// A GitHub that does not answer is a warning: the rest of the update does
/// not need it.
pub fn plan_modeling_app(
    project: &Project,
    timeout: Duration,
    warnings: &mut Vec<String>,
) -> Option<ModelingAppUpdate> {
    let components = &project.state.components;
    let has_dhis2 = components.dhis2.enabled || components.dhis2_external.is_some();
    if !has_dhis2 || *components.modeling_app() != ModelingApp::Unstable {
        return None;
    }
    let newest = match crate::dhis2::fetch_unstable(timeout) {
        Ok(newest) => newest.build,
        Err(err) => {
            warnings.push(format!(
                "could not ask GitHub for the newest unstable Modeling App ({err:#}); \
                 `chap-devops dhis2 apps` installs it when GitHub answers"
            ));
            return None;
        }
    };
    let installed = components.modeling_app_build();
    Some(ModelingAppUpdate {
        newer: installed.is_none_or(|build| build.digest != newest.digest),
        newest: newest.short_commit().to_string(),
        installed: installed.map(|build| build.short_commit().to_string()),
    })
}

/// The line the update ends on for it.
pub fn modeling_app_lines(found: Option<&ModelingAppUpdate>, lines: &mut Report) {
    let Some(found) = found else {
        return;
    };
    match (&found.installed, found.newer) {
        (Some(installed), true) => lines.info(format!(
            "the unstable Modeling App has a newer build, {} (installed: {installed}); run \
             `chap-devops dhis2 apps` to install it",
            found.newest
        )),
        (None, _) => lines.info(format!(
            "the unstable Modeling App {} is not installed; run `chap-devops dhis2 apps` to \
             install it",
            found.newest
        )),
        (Some(installed), false) => lines.hint(format!(
            "the unstable Modeling App {installed} is the newest build"
        )),
    };
}
