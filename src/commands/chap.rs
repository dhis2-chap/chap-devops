//! `chap-devops chap`: chap-core's own command line, run in a container.
//!
//! The current directory is mounted at the same path inside the container,
//! so the file arguments mean what they mean on the host and every file chap
//! writes is on the host after the run. The caches and the model runs live
//! under chap-devops's data directory, mounted the same way. See `docs/chap-cli.md`.

pub(crate) mod files;
pub(crate) mod plan;
mod start;

use crate::cli::{ChapArgs, ChapImage, ModelRunArgs, PortArg};
use crate::commands::Ctx;
use crate::commands::run::target;
use crate::docker;
use crate::error::{ChapError, Result};
use crate::output;
use crate::project::Project;
use files::Snapshot;
use plan::{ModelKind, RunSpec};
use start::{Model, Started};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The exit code `docker run` gives when it could not start the container
/// at all: an image that does not pull, a mount docker refuses.
const DOCKER_RUN_FAILED: i32 = 125;

/// How many written files the closing line names.
const WRITTEN_LIMIT: usize = 8;

/// Run the chap CLI in a container, and say what it wrote.
pub fn run(ctx: &Ctx, args: &ChapArgs) -> Result<()> {
    if ctx.out.json {
        return Err(ChapError::Usage(
            "`chap-devops chap` prints chap's own output, so --json does not apply; drop it"
                .to_string(),
        )
        .into());
    }
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("reading the current directory: {e}"))?;
    let help = plan::is_help(&args.args);
    if let Some(why) = plan::needs_a_browser(&args.args) {
        return Err(ChapError::Usage(why).into());
    }
    let mut place = Place::find(ctx, args.group.as_deref())?;
    let mut chap_args = args.args.clone();
    let mut kind = plan::model_kind(plan::model_name(&chap_args));

    if let ModelKind::Chapkit(url) = &kind
        && let Some(port) = plan::loopback_port(url)
    {
        return Err(ChapError::Usage(loopback_refusal(url, port, &place)).into());
    }

    if let ModelKind::Local(path) = &kind
        && !cwd.join(path).exists()
        && plan::id_candidate(&kind, false).is_none()
    {
        return Err(ChapError::Usage(format!(
            "`{}` is not a directory here; give a model directory, a GitHub URL or a model id \
             such as `auto_arima_chapkit`",
            path.display()
        ))
        .into());
    }

    // A model id, or the URL of a model chap-devops knows: start the model when it
    // does not run, and give chap its URL on the network.
    let (model, started) = resolve_model(ctx, args, &cwd, &kind, &mut place)?;
    if let Some(model) = &model {
        let url = model.url();
        set_model_name(&mut chap_args, &url);
        kind = ModelKind::Chapkit(url);
    }

    // The tag first: both images have the same chap at a tag, and which of
    // them is already on this machine decides the image.
    let needs_worker = args.image == Some(ChapImage::Worker)
        || (args.image.is_none() && matches!(kind, ModelKind::GitHub { .. } | ModelKind::Local(_)));
    let tag = resolve_tag(
        ctx,
        args.tag.as_deref(),
        place.deployment.as_ref(),
        needs_worker,
    )?;
    if let Some(why) = plan::too_old(&chap_args, &kind, &tag) {
        return Err(ChapError::Usage(why).into());
    }
    let image = args.image.unwrap_or_else(|| {
        let core = format!("{}:{tag}", plan::CORE_REPOSITORY);
        let worker = format!("{}:{tag}", plan::WORKER_REPOSITORY);
        let here = docker::image_ids(&[core.clone(), worker.clone()]);
        plan::image_for(&kind, here.contains_key(&core), here.contains_key(&worker))
    });
    let repository = plan::repository(image);
    let reference = format!("{repository}:{tag}");
    pull_moving_tag(ctx, args.tag.as_deref(), &reference);

    if args.image == Some(ChapImage::Core)
        && matches!(kind, ModelKind::GitHub { .. } | ModelKind::Local(_))
    {
        output::warn(
            "the core image has no R, so an R model fails in it with `Rscript: not found`; \
             without --image, chap-devops runs this model in the worker image",
        );
    }
    if !args.docker && !plan::as_chapkit(&chap_args) && runs_in_docker(ctx, &kind, &cwd) {
        return Err(ChapError::Usage(format!(
            "`{}` runs in docker (`docker_env` in its MLproject), and the container has no \
             docker socket; run it again with `chap-devops chap --docker ...`",
            plan::model_name(&chap_args).unwrap_or_default()
        ))
        .into());
    }

    let data = crate::paths::data_dir().join("chap");
    for dir in ["runs", "cache"] {
        std::fs::create_dir_all(data.join(dir))
            .map_err(|e| anyhow::anyhow!("creating {}: {e}", data.join(dir).display()))?;
    }
    // chap does not make the directory of an output file, and fails with a
    // traceback when it is not there; chap-devops makes it.
    let outputs: Vec<PathBuf> = plan::output_files(&chap_args)
        .iter()
        .map(|file| plan::normalize(&cwd.join(file)))
        .collect();
    for file in &outputs {
        if let Some(dir) = file.parent()
            && !dir.exists()
        {
            std::fs::create_dir_all(dir)
                .map_err(|e| anyhow::anyhow!("creating {}: {e}", dir.display()))?;
        }
    }
    let outside = plan::outside_paths(&chap_args, &cwd);
    let mounts =
        plan::mount_dirs(&outside, &[cwd.clone(), data.clone()], Path::exists).map_err(|path| {
            ChapError::Usage(format!(
                "`{}` is outside the current directory, and no directory above it exists to \
                 mount; create its directory first, or use a path in {}",
                path.display(),
                cwd.display()
            ))
        })?;

    announce(&Announce {
        args: &chap_args,
        image,
        reference: &reference,
        cwd: &cwd,
        mounts: &mounts,
        place: &place,
        socket: args.docker,
        list_models: model.is_none() && matches!(kind, ModelKind::Chapkit(_)),
    });

    // A chapkit service has to answer before chap is started, or chap ends
    // in a traceback. One that this run started is given the time to.
    if let ModelKind::Chapkit(url) = &kind {
        let network = place.network.as_deref();
        let wait = match &started {
            Some(_) => Duration::from_secs(args.timeout),
            None => Duration::ZERO,
        };
        if !start::wait_for(&reference, network, url, wait) {
            let message = match &model {
                Some(model) => format!(
                    "{} did not answer on {url} within {}s; `chap-devops -C {} logs {}` says why",
                    model.service,
                    args.timeout,
                    place
                        .project
                        .as_ref()
                        .map(|p| p.dir.display().to_string())
                        .unwrap_or_default(),
                    model.service
                ),
                None => plan::unreachable_message(url),
            };
            return Err(ChapError::Usage(message).into());
        }
        if !help {
            let name = model
                .as_ref()
                .map(|m| format!("{} at ", m.id))
                .unwrap_or_default();
            output::notice(&format!("model: {name}{url} answers"));
        }
    }

    let socket_group = args.docker.then(|| {
        let gid = docker::socket_gid(crate::backup::BUSYBOX_IMAGE);
        if gid.is_none() {
            output::warn(
                "could not read the docker socket's group; the run may be refused the socket",
            );
        }
        gid
    });
    let spec = RunSpec {
        image: reference.clone(),
        cwd: cwd.clone(),
        mounts: mounts.clone(),
        data,
        user: host_user(),
        network: place.network.clone(),
        docker: socket_group,
        tty: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        program: "chap",
        args: chap_args,
    };

    let roots: Vec<PathBuf> = std::iter::once(cwd.clone()).chain(mounts).collect();
    let before = Snapshot::take(&roots, &outputs);
    let code = docker::run_plain(&plan::docker_args(&spec))?;
    let written = before.written(&Snapshot::take(&roots, &outputs));

    let mut result = Ok(());
    ctx.out.report_stderr(|lines| {
        result = finish(code, help, &written, &cwd, &reference, lines);
        match (&started, &model, args.stop) {
            (Some(started), _, true) => start::stop(ctx, started, lines),
            (Some(_), Some(model), false) => {
                lines.info(keeps_running(model, &place));
            }
            (None, Some(model), true) => {
                lines.info(format!(
                    "{} ran before this run, so --stop left it running",
                    model.id
                ));
            }
            _ => {}
        }
    })?;
    result
}

/// The run's own result: the closing line, or the error that names chap's
/// exit status.
fn finish(
    code: i32,
    help: bool,
    written: &[PathBuf],
    cwd: &Path,
    reference: &str,
    lines: &mut output::Report,
) -> Result<()> {
    match code {
        0 => {
            if !help {
                report(written, cwd, lines);
            }
            Ok(())
        }
        DOCKER_RUN_FAILED => Err(ChapError::Exited {
            command: "docker run",
            code,
            next: format!(
                "docker could not start {reference}, and its message is above; \
                 `chap-devops chap --tag TAG` picks another chap-core tag"
            ),
        }
        .into()),
        _ => {
            if !written.is_empty() {
                report(written, cwd, lines);
            }
            Err(ChapError::Exited {
                command: "chap",
                code,
                next: "its own message is above".to_string(),
            }
            .into())
        }
    }
}

/// The model the run needs, when `--model-name` is a model id or the URL of
/// a model chap-devops knows, and what the run started for it.
///
/// - A model of the deployment or group: started on its own when it does
///   not run, with no chap-core.
/// - A marketplace id outside a deployment: started in a `chap-devops run` group.
/// - A marketplace id in a deployment: refused, because enabling a model
///   changes the deployment, and that is `chap-devops models enable`.
fn resolve_model(
    ctx: &Ctx,
    args: &ChapArgs,
    cwd: &Path,
    kind: &ModelKind,
    place: &mut Place,
) -> Result<(Option<Model>, Option<Started>)> {
    let exists = matches!(kind, ModelKind::Local(path) if cwd.join(path).exists());
    let id = plan::id_candidate(kind, exists);
    let host = match kind {
        ModelKind::Chapkit(url) => plan::url_host(url),
        _ => None,
    };
    if id.is_none() && host.is_none() {
        return Ok((None, None));
    }
    let known = place
        .project
        .as_ref()
        .and_then(|project| start::model_in(project, id.as_deref(), host.as_deref()));

    if let Some(model) = known {
        let project = place.project.clone().expect("the model was found in it");
        if docker::running_services(&project).contains(&model.service) {
            return Ok((Some(model), None));
        }
        output::notice(&format!(
            "starting {} for this run, without chap-core",
            model.service
        ));
        let files = start::start_service(&project, &model)?;
        *place = Place::find(ctx, args.group.as_deref())?;
        let started = Started::Service {
            project: Box::new(project),
            model: model.clone(),
            files,
        };
        return Ok((Some(model), Some(started)));
    }

    let Some(id) = id else {
        // A URL of something chap-devops does not run: the probe before the run
        // says whether it answers.
        return Ok((None, None));
    };
    let registry = super::registry_for(ctx, place.deployment.as_ref())?;
    if registry.get(&id).is_none() {
        return Err(ChapError::Usage(format!(
            "`{id}` is not a directory here and not a marketplace model id; give a model \
             directory, a GitHub URL, or an id from `chap-devops models search {id}`"
        ))
        .into());
    }
    if let Some(project) = &place.deployment {
        return Err(ChapError::Usage(format!(
            "`{id}` is not a model of the deployment at {}; `chap-devops models enable {id}` adds \
             it, and `chap-devops models list` lists the ids",
            project.dir.display()
        ))
        .into());
    }
    let run = ModelRunArgs {
        source: id.clone(),
        port: "auto".parse::<PortArg>().map_err(|e| anyhow::anyhow!(e))?,
        bind: None,
        id: None,
        group: args.group.clone(),
        allow_starter: false,
        no_wait: false,
        attach: false,
        rm: false,
        chap_core: None,
        models_host: None,
        timeout: args.timeout,
    };
    let group = crate::commands::run::start_quietly(ctx, &run)?;
    *place = Place::find(ctx, args.group.as_deref())?;
    let project = place
        .project
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("the group at {} did not load", group.dir.display()))?;
    let model = start::model_in(project, Some(&group.id), None)
        .ok_or_else(|| anyhow::anyhow!("{} is not in the group it was started in", group.id))?;
    let started = group.enabled.then(|| Started::Group {
        dir: group.dir.clone(),
        model: model.clone(),
    });
    Ok((Some(model), started))
}

/// Put `url` in place of the `--model-name` value, in either spelling.
fn set_model_name(args: &mut [String], url: &str) {
    let mut next_is_value = false;
    for arg in args.iter_mut() {
        if next_is_value {
            *arg = url.to_string();
            return;
        }
        if arg == "--model-name" {
            next_is_value = true;
        } else if arg.starts_with("--model-name=") {
            *arg = format!("--model-name={url}");
            return;
        }
    }
}

/// The closing note for a model this run started and left running.
fn keeps_running(model: &Model, place: &Place) -> String {
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
struct Place {
    /// The deployment the command runs in; `None` for a group or nothing.
    deployment: Option<Project>,
    /// The project whose network the container joins.
    project: Option<Project>,
    /// `<compose project>_default`, when that network exists.
    network: Option<String>,
}

impl Place {
    fn find(ctx: &Ctx, group: Option<&str>) -> Result<Place> {
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
    fn model_urls(&self) -> Vec<String> {
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
fn loopback_refusal(url: &str, port: u16, place: &Place) -> String {
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

/// Pull `reference` before the run when [`plan::pulls_first`] says so.
///
/// A failed pull is a warning, not an error: the image this machine has may
/// still run, and if there is none, `docker run` says so itself.
pub(crate) fn pull_moving_tag(ctx: &Ctx, asked: Option<&str>, reference: &str) {
    if !plan::pulls_first(asked, ctx.registry.offline) {
        return;
    }
    output::notice(&format!(
        "pulling {reference}: the tag moves, and the image on this machine can be old"
    ));
    if !docker::pull_image(reference) {
        output::warn(&format!(
            "could not pull {reference}; the run uses the image on this machine, which can be              old; run `docker pull --platform {} {reference}` to see why",
            crate::compose::AMD64_PLATFORM
        ));
    }
}

/// The tag to run: `--tag`, else the deployment's own, else the newest
/// release, else the newest one this docker has.
/// The tag of the run. Without network, the newest local tag: of the worker
/// image when the run needs it, and else of either image, since both carry
/// the same chap at a tag.
pub(crate) fn resolve_tag(
    ctx: &Ctx,
    asked: Option<&str>,
    deployment: Option<&Project>,
    needs_worker: bool,
) -> Result<String> {
    if let Some(tag) = asked {
        return Ok(tag.to_string());
    }
    if let Some(project) = deployment {
        return Ok(project.state.chap_image_tag.clone());
    }
    let local = || {
        let mut tags = docker::local_tags(plan::WORKER_REPOSITORY);
        if !needs_worker {
            tags.extend(docker::local_tags(plan::CORE_REPOSITORY));
        }
        plan::newest_tag(&tags)
    };
    let core = plan::CORE_REPOSITORY;
    if ctx.registry.offline {
        return local().ok_or_else(|| {
            anyhow::anyhow!(
                "--offline needs a chap-core or chap-worker image on this machine, and there is \
                 none; run once without --offline, or `docker pull --platform {} {core}:TAG`",
                crate::compose::AMD64_PLATFORM
            )
        });
    }
    match crate::chapcore::latest_release(ctx.registry.timeout) {
        Ok(tag) => Ok(tag),
        Err(err) => {
            let tag = local().ok_or_else(|| {
                err.context(
                    "the newest chap-core release could not be read, and this machine has no \
                     chap-core or chap-worker image; pass `--tag TAG`",
                )
            })?;
            output::warn(&format!(
                "the newest chap-core release could not be read; using {tag}, the newest \
                 image on this machine"
            ));
            Ok(tag)
        }
    }
}

/// Whether the model's `MLproject` asks for `docker_env`. `false` when it
/// cannot be read: chap-core says so itself if it does, so this check only
/// moves the answer before the run.
fn runs_in_docker(ctx: &Ctx, kind: &ModelKind, cwd: &Path) -> bool {
    let text = match kind {
        ModelKind::Local(dir) => std::fs::read_to_string(cwd.join(dir).join("MLproject")).ok(),
        ModelKind::GitHub {
            owner,
            repo,
            git_ref,
        } if !ctx.registry.offline => {
            let url = format!(
                "{}/{owner}/{repo}/{}/MLproject",
                crate::chapcore::raw_base(),
                git_ref.as_deref().unwrap_or("HEAD")
            );
            crate::chapcore::get_raw(&url, ctx.registry.timeout).ok()
        }
        _ => None,
    };
    text.is_some_and(|text| plan::uses_docker_env(&text))
}

/// The user and group to run as, so the files chap writes belong to the
/// person who ran it.
#[cfg(unix)]
pub(crate) fn host_user() -> Option<(u32, u32)> {
    let id = |flag: &str| -> Option<u32> {
        let out = std::process::Command::new("id").arg(flag).output().ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    };
    Some((id("-u")?, id("-g")?))
}

#[cfg(not(unix))]
pub(crate) fn host_user() -> Option<(u32, u32)> {
    None
}

/// The lines before the run: the image, the directories chap sees, and the
/// models it can reach. A run that only asks for help gets the first line.
struct Announce<'a> {
    args: &'a [String],
    image: ChapImage,
    reference: &'a str,
    cwd: &'a Path,
    mounts: &'a [PathBuf],
    place: &'a Place,
    socket: bool,
    /// Whether to list the models on the network: for a URL chap-devops did not
    /// resolve to one of them.
    list_models: bool,
}

fn announce(what: &Announce) {
    let Announce {
        args,
        image,
        reference,
        cwd,
        mounts,
        place,
        socket,
        list_models,
    } = *what;
    let command = args.first().map(String::as_str).unwrap_or("--help");
    let pull = match docker::image_is_local(reference) {
        true => String::new(),
        false => format!(
            "; the first run pulls it, about {}",
            plan::approximate_size(image)
        ),
    };
    output::notice(&format!("running `chap {command}` in {reference}{pull}"));
    if plan::is_help(args) {
        return;
    }
    let mut seen: Vec<String> = vec![cwd.display().to_string()];
    seen.extend(mounts.iter().map(|dir| dir.display().to_string()));
    output::notice(&format!(
        "files: chap reads and writes in {}",
        seen.join(", ")
    ));
    if place.deployment.as_ref().is_some_and(|p| p.dir == cwd) {
        output::notice(
            "files: this is the deployment directory; a subdirectory (`mkdir eval && cd eval`) \
             keeps chap's files apart from the compose files",
        );
    }
    let urls = place.model_urls();
    // A model chap-devops resolved gets its own line once it answers; the list is
    // for a URL that may be one of them.
    if list_models && place.network.is_some() && !urls.is_empty() {
        output::notice(&format!("models: {}", urls.join(", ")));
    }
    if socket {
        output::warn("--docker gives the container control of docker on this machine");
    }
}

/// The closing lines: what the run wrote, and what to do with it.
fn report(written: &[PathBuf], cwd: &Path, lines: &mut output::Report) {
    if written.is_empty() {
        lines.info(format!(
            "chap finished; it wrote no file in {}",
            cwd.display()
        ));
        return;
    }
    lines.info(format!(
        "chap finished; it wrote {}",
        files::describe(written, cwd, WRITTEN_LIMIT)
    ));
    let evaluation = written
        .iter()
        .find(|path| path.extension().is_some_and(|ext| ext == "nc"));
    if let Some(nc) = evaluation {
        let name = nc.strip_prefix(cwd).unwrap_or(nc).to_string_lossy();
        let html = Path::new(name.as_ref()).with_extension("html");
        lines.hint(format!(
            "`chap-devops chap plot-backtest {name} --output-file {}` plots it",
            html.display()
        ));
    }
}

#[cfg(test)]
mod tests;
