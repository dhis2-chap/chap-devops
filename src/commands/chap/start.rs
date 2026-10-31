//! The one model a run needs, started without chap-core.
//!
//! `chap eval` talks to a chapkit model over HTTP and needs nothing else of a
//! deployment: no database, no API, no worker. So the model is started on its
//! own, with `--no-deps`, rather than with `chap-devops up`.

use super::plan;
use crate::commands::Ctx;
use crate::docker;
use crate::error::{ChapError, Result};
use crate::output;
use crate::project::Project;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How long to wait between two asks of a model that is starting.
const POLL: Duration = Duration::from_secs(2);

/// A model of a deployment or group: its id and its compose service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Model {
    pub(super) id: String,
    pub(super) service: String,
}

impl Model {
    /// The URL chap reaches it at, on the compose network.
    pub(super) fn url(&self) -> String {
        format!("http://{}:{}", self.service, plan::MODEL_PORT)
    }
}

/// What a run started, so `--stop` stops that and nothing else.
pub(super) enum Started {
    /// A service of a deployment or group, started with `--no-deps`.
    Service {
        project: Box<Project>,
        model: Model,
        files: Vec<PathBuf>,
    },
    /// A marketplace model this run enabled in a `chap-devops run` group.
    Group { dir: PathBuf, model: Model },
}

/// The model of `project` that `id` (an id or a service id) or `host` (the
/// host of a URL) names.
pub(super) fn model_in(project: &Project, id: Option<&str>, host: Option<&str>) -> Option<Model> {
    project
        .state
        .models
        .iter()
        .find(|(key, model)| {
            id.is_some_and(|id| id == key.as_str() || id == model.service_id)
                || host.is_some_and(|host| host.eq_ignore_ascii_case(&model.service_id))
        })
        .map(|(key, model)| Model {
            id: key.clone(),
            service: model.service_id.clone(),
        })
}

/// Start `model` in `project` without chap-core: its init container, then
/// the service, both with `--no-deps`.
///
/// In a deployment whose own chap-core is on, the start also takes the
/// model's environment away, so it does not try to register with a chap-core
/// that does not run. The container then differs from what `chap-devops up`
/// renders, and `chap-devops up` recreates it later, registered.
pub(super) fn start_service(project: &Project, model: &Model) -> Result<Vec<PathBuf>> {
    let components = &project.state.components;
    let local_chap_core = components.chap_core.enabled && components.chap_core_external.is_none();
    let mut files = Vec::new();
    if local_chap_core {
        let name = project
            .compose_project_name()
            .unwrap_or_else(|| "chap-devops".to_string());
        let file = crate::paths::data_dir()
            .join("chap")
            .join(format!("compose.{name}.{}.yml", model.service));
        let body = format!(
            "# Written by `chap-devops chap` for one start of {} without chap-core.\n\
             services:\n  {}:\n    environment: !reset {{}}\n",
            model.service, model.service
        );
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| anyhow::anyhow!("creating {}: {e}", dir.display()))?;
        }
        std::fs::write(&file, body)
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", file.display()))?;
        files.push(file);
    }

    // A first start pulls the model's image, which is 1 to 7 GB. That is
    // minutes, so it is said, and compose's progress is shown, rather than a
    // quiet wait that looks like a hang.
    if let Some(image) = docker::service_images(project).get(&model.service)
        && !docker::image_is_local(image)
    {
        output::notice(&format!(
            "pulling {image} first; a model image is 1 to 7 GB, so this can take some minutes"
        ));
        let pull = ["pull", &model.service].map(str::to_string);
        let piped = docker::run_compose_teed_with(project, &files, &pull)?;
        if piped.code != 0 {
            return Err(
                anyhow::Error::from(ChapError::DockerFailed(piped.code)).context(format!(
                    "{image} could not be pulled; its message is above, and `chap-devops doctor` checks \
                 the connection to the registry"
                )),
            );
        }
    }

    let init = format!("{}-init", model.service);
    let has_init = docker::config_services(project)
        .map(|services| services.contains(&init))
        .unwrap_or(true);
    let mut steps: Vec<Vec<String>> = Vec::new();
    if has_init {
        steps.push(["up", "--no-deps", &init].map(str::to_string).to_vec());
    }
    steps.push(
        ["up", "-d", "--no-deps", &model.service]
            .map(str::to_string)
            .to_vec(),
    );
    // Quiet unless it fails: compose's progress is not what the person
    // asked for, and its reason is what they need when it does fail.
    for step in steps {
        let (code, _, stderr) = docker::compose_output_with(project, &files, &step)?;
        if code != 0 {
            let said = stderr
                .lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty())
                .unwrap_or("compose said nothing")
                .to_string();
            return Err(
                anyhow::Error::from(ChapError::DockerFailed(code)).context(format!(
                    "{} did not start ({said}); `chap-devops -C {} logs {}` shows its log",
                    model.service,
                    project.dir.display(),
                    model.service
                )),
            );
        }
    }
    Ok(files)
}

/// Ask `url` until it answers, or `timeout` runs out.
pub(super) fn wait_for(image: &str, network: Option<&str>, url: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if docker::chapkit_answers(image, network, url) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

/// Stop what the run started. Best-effort: a model that does not stop is a
/// warning, because the run itself is done.
pub(super) fn stop(ctx: &Ctx, started: &Started, lines: &mut output::Report) {
    match started {
        Started::Service {
            project,
            model,
            files,
        } => {
            let args = ["stop", &model.service].map(str::to_string);
            match docker::compose_output_with(project, files, &args) {
                Ok((0, _, _)) => {
                    lines.info(format!("stopped {}, as --stop asked", model.service));
                }
                _ => {
                    lines.warning(format!(
                        "{} did not stop; `chap-devops -C {} down` stops it",
                        model.service,
                        project.dir.display()
                    ));
                }
            }
        }
        Started::Group { dir, model } => {
            match crate::commands::run::stop_in(ctx, dir, &model.id, false) {
                Ok(_) => {
                    lines.info(format!("stopped {}, as --stop asked", model.id));
                }
                Err(err) => {
                    lines.warning(format!(
                        "{} did not stop: {err:#}; `chap-devops stop {}` stops it",
                        model.id, model.id
                    ));
                }
            }
        }
    }
}
