//! Where a `chap-devops chap` run belongs: the deployment or group around
//! it, and the network that reaches its models.

use super::plan;
use super::start::Model;
use crate::commands::Ctx;
use crate::commands::run::target;
use crate::docker;
use crate::error::{ChapError, Result};
use crate::project::Project;

/// The closing note for a model this run started and left running.
pub(super) fn keeps_running(model: &Model, place: &Place) -> String {
    match &place.deployment {
        Some(project) => format!(
            "{} keeps running for the next run; `chap-devops -C {} down` stops it, and `--stop` \
             stops it after a run",
            model.service,
            project.dir.display()
        ),
        None => format!(
            "{} keeps running for the next run; `chap-devops stop {}` stops it, and `--stop` stops \
             it after a run",
            model.id, model.id
        ),
    }
}

/// The deployment or group the run belongs to, when there is one, and the
/// network that reaches its models.
pub(super) struct Place {
    /// The deployment the command runs in; `None` for a group or nothing.
    pub(super) deployment: Option<Project>,
    /// The project whose network the container joins.
    pub(super) project: Option<Project>,
    /// `<compose project>_default`, when that network exists.
    pub(super) network: Option<String>,
}

impl Place {
    pub(super) fn find(ctx: &Ctx, group: Option<&str>) -> Result<Place> {
        let target = target(ctx, group)?;
        let loaded = if target.dir.join(crate::project::CHAP_DEVOPS_DIR).is_dir() {
            Some(Project::load(&target.dir)?)
        } else if let Some(name) = group {
            return Err(ChapError::Usage(format!(
                "there is no `chap-devops run` group called `{name}`; `chap-devops ps` lists the groups, \
                 and `chap-devops run MODEL --group {name}` starts one"
            ))
            .into());
        } else {
            None
        };
        let network = loaded
            .as_ref()
            .and_then(Project::compose_project_name)
            .filter(|name| docker::default_network_exists(name))
            .map(|name| format!("{name}_default"));
        let deployment = loaded.clone().filter(|_| target.group.is_none());
        Ok(Place {
            deployment,
            project: loaded,
            network,
        })
    }

    /// `id at http://<service>:8000` for every model the network reaches.
    pub(super) fn model_urls(&self) -> Vec<String> {
        let Some(project) = &self.project else {
            return Vec::new();
        };
        project
            .state
            .models
            .iter()
            .map(|(id, model)| format!("{id} at http://{}:{}", model.service_id, plan::MODEL_PORT))
            .collect()
    }
}

/// Why a model URL on `localhost` cannot work, and the URL that does: the
/// service name of the model published on that port, when chap-devops knows it.
pub(super) fn loopback_refusal(url: &str, port: u16, place: &Place) -> String {
    let service = place.project.as_ref().and_then(|project| {
        project
            .state
            .models
            .values()
            .find(|model| model.host_port == Some(port))
            .map(|model| model.service_id.clone())
    });
    match (service, &place.network) {
        (Some(service), Some(_)) => format!(
            "`{url}` is this machine, and in the container `localhost` is the container \
             itself; use `--model-name http://{service}:{}`, the same model on the network",
            plan::MODEL_PORT
        ),
        _ => format!(
            "`{url}` is this machine, and in the container `localhost` is the container \
             itself; use `--model-name http://{}:{port}`, or give a model id such as \
             `auto_arima_chapkit` and chap-devops starts it",
            crate::components::HOST_GATEWAY
        ),
    }
}
