//! `chap-devops chapkit mlproject run DIR`: serve an MLproject model with
//! `chapkit mlproject run`, in the foreground, in an image that has chapkit.
//!
//! The chap-core image has chapkit but no R, so it cannot serve most
//! MLproject models. This runs a chapkit `-cli` image (Python, Python with
//! the PyData stack, R, R with tidyverse, or R with INLA, each with chapkit),
//! or an image of your own,
//! with the MLproject directory mounted at `/work`. In a deployment that runs
//! chap-core, the service joins the deployment network and registers with
//! chap-core while it runs. Ctrl-C stops it. See `docs/chapkit-cli.md`.

use crate::cli::ChapkitArgs;
use crate::commands::Ctx;
use crate::commands::run::target;
use crate::docker;
use crate::error::{ChapError, Result};
use crate::interrupt::{self, Throwaway};
use crate::output;
use crate::ports;
use crate::project::{CHAP_DEVOPS_DIR, DEFAULT_PORT_RANGE, Project};
use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

mod image;

pub(super) use image::list;
use image::{amd64_only, default_flavour, ensure_image, image_for};

/// The file names chapkit looks for in an MLproject directory.
const MLPROJECT_FILES: &[&str] = &["MLproject", "MLproject.yaml", "MLproject.yml"];

/// The port the service listens on in the container.
const SERVICE_PORT: u16 = 8000;

/// The options of `chapkit mlproject run` that chap-devops sets itself.
const OWNED_OPTIONS: &[&str] = &[
    "--host",
    "--port",
    "--register-url",
    "--advertise-host",
    "--advertise-port",
];

/// The options of `chapkit mlproject run` that take a value.
const VALUE_OPTIONS: &[&str] = &[
    "--param",
    "--keepalive-interval",
    "--config-format",
    "--database",
    "--log-level",
];

/// How long to wait between two checks of `/health`.
const POLL: Duration = Duration::from_secs(1);

/// How long the service may take to answer before chap-devops says so.
const SLOW_START: Duration = Duration::from_secs(120);

/// The exit code `docker run` gives when it could not start the container.
const DOCKER_RUN_FAILED: i32 = 125;

/// The exit code of a container whose command is not in the image.
const COMMAND_NOT_FOUND: i32 = 127;

/// The exit code of a container that SIGTERM stopped, as `docker stop` does.
const STOPPED_BY_SIGTERM: i32 = 143;

/// The arguments after `mlproject run`, as chap-devops reads them.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Request {
    /// The MLproject directory or file; `None` for the current directory.
    pub path: Option<String>,
    /// The other options, passed to chapkit unchanged.
    pub pass: Vec<String>,
    /// Whether `--help` was given.
    pub help: bool,
}

/// Read the arguments after `mlproject run`. The error is the message.
pub(super) fn parse(rest: &[String]) -> std::result::Result<Request, String> {
    let mut request = Request::default();
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        if word == "--help" || word == "-h" {
            request.help = true;
        } else if word.starts_with('-') {
            let name = word.split('=').next().unwrap_or(word);
            if OWNED_OPTIONS.contains(&name) {
                return Err(format!(
                    "chap-devops sets `{name}` of `chapkit mlproject run` itself; drop it, and use the \
                     URL that chap-devops prints"
                ));
            }
            request.pass.push(word.clone());
            if !word.contains('=') && VALUE_OPTIONS.contains(&name) {
                let value = words
                    .next()
                    .ok_or_else(|| format!("`{name}` needs a value, such as `{name} VALUE`"))?;
                request.pass.push(value.clone());
            }
        } else if request.path.is_none() {
            request.path = Some(word.clone());
        } else {
            return Err(format!(
                "`chapkit mlproject run` serves one MLproject, and `{word}` is a second one; \
                 give one directory"
            ));
        }
    }
    Ok(request)
}

/// The directory to mount and the MLproject argument for chapkit in it: `.`
/// for the usual `MLproject` file, or the file name for another spelling.
pub(super) fn locate(
    cwd: &Path,
    path: Option<&str>,
) -> std::result::Result<(PathBuf, String), String> {
    let given = cwd.join(path.unwrap_or("."));
    let (dir, file) = if given.is_file() {
        let file = given
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        (
            given.parent().map(Path::to_path_buf).unwrap_or_default(),
            file,
        )
    } else if given.is_dir() {
        let file = MLPROJECT_FILES
            .iter()
            .find(|name| given.join(name).is_file())
            .ok_or_else(|| {
                format!(
                    "{} has no `MLproject` file; give the directory of an MLproject model",
                    given.display()
                )
            })?;
        (given, file.to_string())
    } else {
        return Err(format!(
            "{} does not exist; give the directory of an MLproject model",
            given.display()
        ));
    };
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    let argument = match file.as_str() {
        "MLproject" => ".".to_string(),
        _ => file,
    };
    Ok((dir, argument))
}

/// How the service registers with chap-core in a deployment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Registration {
    /// The deployment network, where chap-core is the service `chap`.
    pub network: String,
    /// Whether `.env` sets a registration key; its value goes in through the
    /// environment of `docker run`, never on its command line.
    pub key: bool,
}

/// Everything `docker run` needs to serve the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Serve {
    pub name: String,
    pub image: String,
    pub dir: PathBuf,
    pub argument: String,
    pub port: u16,
    pub registration: Option<Registration>,
    pub tty: bool,
    pub pass: Vec<String>,
    /// The git commit of the model directory: chapkit reports it, and
    /// chap-core stores a model template only with a revision.
    pub revision: Option<String>,
}

/// The arguments of the `docker run` that serves the model.
pub(super) fn docker_args(serve: &Serve) -> Vec<String> {
    let mut out: Vec<String> = ["run", "--rm", "-i"].map(str::to_string).to_vec();
    if serve.tty {
        out.push("-t".to_string());
    }
    out.extend(["--init", "--name", &serve.name].map(str::to_string));
    if amd64_only(&serve.image) {
        out.extend(["--platform", crate::compose::AMD64_PLATFORM].map(str::to_string));
    }
    out.push("-p".to_string());
    out.push(format!("{}:{SERVICE_PORT}", serve.port));
    out.push("-v".to_string());
    out.push(format!("{}:/work", serve.dir.display()));
    out.extend(["-w", "/work"].map(str::to_string));
    if let Some(registration) = &serve.registration {
        out.extend(["--network", &registration.network].map(str::to_string));
        out.push("-e".to_string());
        out.push(format!(
            "SERVICEKIT_ORCHESTRATOR_URL=http://{}:8000/v2/services/$register",
            crate::compose::API_SERVICE
        ));
        out.push("-e".to_string());
        out.push(format!("SERVICEKIT_HOST={}", serve.name));
        out.push("-e".to_string());
        out.push(format!("SERVICEKIT_PORT={SERVICE_PORT}"));
        if registration.key {
            out.extend(["-e", crate::auth::REGISTRATION_KEY_ENV_VAR].map(str::to_string));
        }
    }
    if let Some(revision) = &serve.revision {
        out.push("-e".to_string());
        out.push(format!("GIT_REVISION={revision}"));
    }
    out.push("--label".to_string());
    out.push(format!("{}=cli", docker::KIND_LABEL));
    out.push(serve.image.clone());
    out.extend(["chapkit", "mlproject", "run", &serve.argument].map(str::to_string));
    out.extend(["--host", "0.0.0.0", "--port"].map(str::to_string));
    out.push(SERVICE_PORT.to_string());
    out.extend(serve.pass.iter().cloned());
    out
}

/// Serve the MLproject in the foreground until it stops or Ctrl-C.
pub(super) fn run(ctx: &Ctx, args: &ChapkitArgs, rest: &[String]) -> Result<()> {
    if args.tag.is_some() {
        return Err(ChapError::Usage(
            "--tag picks the chap-core image, and `mlproject run` uses --image; drop --tag"
                .to_string(),
        )
        .into());
    }
    let request = parse(rest).map_err(ChapError::Usage)?;
    if request.help {
        return help(args.image.as_deref());
    }
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("reading the current directory: {e}"))?;
    let (dir, argument) = locate(&cwd, request.path.as_deref()).map_err(ChapError::Usage)?;
    let mlproject_file = match argument.as_str() {
        "." => dir.join("MLproject"),
        file => dir.join(file),
    };
    let text = std::fs::read_to_string(&mlproject_file)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", mlproject_file.display()))?;

    let (image, why) = match args.image.as_deref() {
        Some(choice) => (image_for(choice), "from --image".to_string()),
        None => match default_flavour(&text) {
            Some((flavour, command)) => (
                image_for(flavour),
                format!("from the train command `{command}`; --image picks another"),
            ),
            None => {
                return Err(ChapError::Usage(format!(
                    "chap-devops cannot tell the language of the train command in {}; give \
                     `--image py-data`, `r-tidyverse`, `r-inla`, or an image name",
                    mlproject_file.display()
                ))
                .into());
            }
        },
    };
    output::notice(&format!(
        "serving {} with `chapkit mlproject run` in {image} ({why})",
        dir.display()
    ));
    ensure_image(&image)?;

    let place = target(ctx, None)?;
    let project = match place.group.is_none() && place.dir.join(CHAP_DEVOPS_DIR).is_dir() {
        true => Some(Project::load(&place.dir)?),
        false => None,
    };
    let port = free_port(project.as_ref())?;
    let revision = git_revision(&dir);
    match &revision {
        Some(Revision { dirty: true, .. }) => output::warn(&format!(
            "{} has changes that are not committed, and the service reports the last commit; \
             commit them, and raise `version` in MLproject, before chap-core stores this version",
            dir.display()
        )),
        None if project.is_some() => output::warn(&format!(
            "{} is not a git checkout, so chap-core cannot store the model; run `git init`, \
             and commit, to add configured models",
            dir.display()
        )),
        _ => {}
    }
    let registration = project.as_ref().and_then(registration_of);
    let key = project.as_ref().and_then(registration_key);

    interrupt::install();
    let mut guard = Throwaway::new("chap-devops-mlproject");
    let serve = Serve {
        name: guard.name().to_string(),
        image: image.clone(),
        dir,
        argument,
        port,
        registration: registration.clone().map(|registration| Registration {
            key: key.is_some(),
            ..registration
        }),
        tty: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        pass: request.pass,
        revision: revision.map(|revision| revision.commit),
    };
    let url = format!("http://localhost:{port}");
    output::notice(&format!(
        "starting it on {url}; the logs follow, and Ctrl-C stops it"
    ));

    let mut command = Command::new("docker");
    command
        .args(docker_args(&serve))
        .env("DOCKER_CLI_HINTS", "false");
    if let Some(value) = &key {
        command.env(crate::auth::REGISTRATION_KEY_ENV_VAR, value);
    }
    let mut child = command
        .spawn()
        .map_err(|e| anyhow::anyhow!("could not run docker: {e}"))?;
    let code = watch(&mut child, &url, serve.registration.is_some(), &serve.name)?;

    if code == 0 || interrupt::requested() {
        if code == 0 && !interrupt::requested() {
            guard.finished();
        }
        output::notice(&format!("stopped the MLproject service on {url}"));
        return Ok(());
    }
    if code == STOPPED_BY_SIGTERM {
        output::notice(&format!(
            "the MLproject service on {url} stopped, because its container got SIGTERM \
             (from `docker stop` or a restart of docker)"
        ));
        return Ok(());
    }
    match code {
        DOCKER_RUN_FAILED => Err(ChapError::Exited {
            command: "docker run",
            code,
            next: format!("docker could not start {image}, and its message is above"),
        }
        .into()),
        COMMAND_NOT_FOUND => Err(ChapError::Exited {
            command: "chapkit",
            code,
            next: format!(
                "{image} may have no `chapkit`; use `--image py-data`, `r-tidyverse`, `r-inla`, \
                 or an image built on one of them"
            ),
        }
        .into()),
        _ => Err(ChapError::Exited {
            command: "chapkit",
            code,
            next: "its own message is above".to_string(),
        }
        .into()),
    }
}

/// Wait for the container to stop, and say once when the service answers.
///
/// A Ctrl-C in a terminal reaches `docker run` too, but a signal sent to
/// chap-devops alone (`kill -INT`, a process manager) does not: so on a request
/// to stop, chap-devops stops the container itself.
fn watch(child: &mut std::process::Child, url: &str, registers: bool, name: &str) -> Result<i32> {
    let started = Instant::now();
    let mut answered = false;
    let mut slow = false;
    let mut stopping = false;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| anyhow::anyhow!("waiting for docker: {e}"))?
        {
            return Ok(status.code().unwrap_or(interrupt::EXIT_CODE));
        }
        if !stopping && interrupt::requested() {
            stopping = true;
            stop_container(name);
            continue;
        }
        if !answered && !interrupt::requested() {
            let health = format!("{url}{}", crate::status::HEALTH_PATH);
            if crate::chapcore::get_raw(&health, POLL).is_ok() {
                answered = true;
                let joined = match registers {
                    true => format!(
                        "; it registers with chap-core in this deployment, which reaches it at \
                         http://{name}:{SERVICE_PORT}"
                    ),
                    false => String::new(),
                };
                output::notice(&format!("the MLproject service answers at {url}{joined}"));
            } else if !slow && started.elapsed() > SLOW_START {
                slow = true;
                output::warn(&format!(
                    "the MLproject service does not answer at {url} yet; read the logs above"
                ));
            }
        }
        std::thread::sleep(POLL);
    }
}

/// `docker stop <name>`, best-effort and silent: chapkit gets SIGTERM and
/// shuts down, and `--rm` removes the container.
fn stop_container(name: &str) {
    output::verbose(&format!("$ docker stop {name}"));
    let _ = Command::new("docker")
        .args(["stop", name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Run `chapkit mlproject run --help` in the image, so the help matches it.
fn help(choice: Option<&str>) -> Result<()> {
    let image = image_for(choice.unwrap_or("py"));
    ensure_image(&image)?;
    let mut args: Vec<String> = ["run", "--rm"].map(str::to_string).to_vec();
    if amd64_only(&image) {
        args.extend(["--platform", crate::compose::AMD64_PLATFORM].map(str::to_string));
    }
    args.push(image);
    args.extend(["chapkit", "mlproject", "run", "--help"].map(str::to_string));
    match docker::run_plain(&args)? {
        0 => Ok(()),
        code => Err(ChapError::Exited {
            command: "chapkit",
            code,
            next: "its own message is above".to_string(),
        }
        .into()),
    }
}

/// The git commit of a model directory, and whether it has changes that
/// are not committed.
struct Revision {
    commit: String,
    dirty: bool,
}

/// The revision of `dir`; `None` when it is not a git checkout, or git is
/// not there.
fn git_revision(dir: &Path) -> Option<Revision> {
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let commit = git(&["rev-parse", "HEAD"]).filter(|commit| !commit.is_empty())?;
    let dirty = git(&["status", "--porcelain", "--", "."]).is_some_and(|out| !out.is_empty());
    Some(Revision { commit, dirty })
}

/// A free host port: from the deployment's range when there is one, else
/// from the default model range.
fn free_port(project: Option<&Project>) -> Result<u16> {
    match project {
        Some(project) => crate::compose::ports::allocator_for(project, &BTreeSet::new())?
            .allocate(&ports::is_busy),
        None => {
            let (lo, hi) = DEFAULT_PORT_RANGE;
            ports::first_free(lo, hi, &ports::is_busy).ok_or_else(|| {
                ChapError::Usage(format!(
                    "no port from {lo} to {hi} is free on this machine; stop a service that \
                     uses one, then try again"
                ))
                .into()
            })
        }
    }
}

/// The registration in a deployment that runs chap-core itself and whose
/// network exists; `None` otherwise.
fn registration_of(project: &Project) -> Option<Registration> {
    let components = &project.state.components;
    if !components.chap_core.enabled || components.chap_core_external.is_some() {
        return None;
    }
    let name = project.compose_project_name()?;
    docker::default_network_exists(&name).then(|| Registration {
        network: format!("{name}_default"),
        key: false,
    })
}

/// The registration key that `.env` of the deployment sets, if any.
fn registration_key(project: &Project) -> Option<String> {
    let body = std::fs::read_to_string(project.dir.join(".env")).ok()?;
    crate::auth::active_value(&body, crate::auth::REGISTRATION_KEY_ENV_VAR)
}

#[cfg(test)]
mod tests;
