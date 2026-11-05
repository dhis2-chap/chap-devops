//! What `chap-devops chap` asks docker for: the image, the mounts and the argument
//! list. Pure functions only, so the tests can check every case without a
//! docker daemon.

use crate::cli::ChapImage;
use std::path::{Component, Path, PathBuf};

/// The image with chap-core's API, and the chap CLI without R.
pub const CORE_REPOSITORY: &str = "ghcr.io/dhis2-chap/chap-core";

/// The image chap-core's worker runs models in: uv, R and INLA as well.
pub const WORKER_REPOSITORY: &str = "ghcr.io/dhis2-chap/chap-worker";

/// The prefix of a model built into chap-core, such as `builtin:persistence`.
pub const BUILTIN_PREFIX: &str = "builtin:";

/// The port a chapkit model answers on inside its container.
pub const MODEL_PORT: u16 = 8000;

/// The repository of one of the two images.
pub fn repository(image: ChapImage) -> &'static str {
    match image {
        ChapImage::Core => CORE_REPOSITORY,
        ChapImage::Worker => WORKER_REPOSITORY,
    }
}

/// About how much a first pull of the image downloads, for the line before
/// the run. Measured on v2.3.1.
pub fn approximate_size(image: ChapImage) -> &'static str {
    match image {
        ChapImage::Core => "2.4 GB",
        ChapImage::Worker => "12 GB",
    }
}

/// What `--model-name` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelKind {
    /// No `--model-name`: `plot-backtest`, `export-metrics` and the like.
    None,
    /// A chapkit service, which chap talks to over HTTP.
    Chapkit(String),
    /// A GitHub repository, at a commit when the name carries `@<ref>`.
    GitHub {
        owner: String,
        repo: String,
        git_ref: Option<String>,
    },
    /// A model built into chap-core, `builtin:<name>[@<version>]`. It runs
    /// in chap itself, in either image.
    Builtin(String),
    /// A model directory on this machine.
    Local(PathBuf),
}

/// The value of `--model-name` in the chap arguments, in either spelling.
pub fn model_name(args: &[String]) -> Option<&str> {
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--model-name" {
            return rest.next().map(String::as_str);
        }
        if let Some(value) = arg.strip_prefix("--model-name=") {
            return Some(value);
        }
    }
    None
}

/// What kind of model a `--model-name` value is, the way chap-core decides
/// it: `builtin:` names a model in chap-core, a GitHub URL is cloned, any
/// other URL is a chapkit service, and anything else is a directory.
pub fn model_kind(name: Option<&str>) -> ModelKind {
    let Some(name) = name else {
        return ModelKind::None;
    };
    let lower = name.to_ascii_lowercase();
    if lower.starts_with(BUILTIN_PREFIX) {
        return ModelKind::Builtin(name.to_string());
    }
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    match without_scheme {
        Some(rest) if rest.starts_with("github.com/") || rest.starts_with("www.github.com/") => {
            github_repo(name).unwrap_or_else(|| ModelKind::Chapkit(name.to_string()))
        }
        Some(_) => ModelKind::Chapkit(name.to_string()),
        None => ModelKind::Local(PathBuf::from(name)),
    }
}

/// `https://github.com/<owner>/<repo>[@<ref>]`, with the case kept.
fn github_repo(name: &str) -> Option<ModelKind> {
    let (_, rest) = name.split_once("://")?;
    let rest = rest.trim_start_matches("www.");
    let rest = rest.get("github.com/".len()..)?;
    let (path, git_ref) = match rest.split_once('@') {
        Some((path, git_ref)) => (path, Some(git_ref.to_string())),
        None => (rest, None),
    };
    let mut parts = path.trim_end_matches('/').split('/');
    let owner = parts.next().filter(|p| !p.is_empty())?.to_string();
    let repo = parts
        .next()
        .filter(|p| !p.is_empty())?
        .trim_end_matches(".git")
        .to_string();
    Some(ModelKind::GitHub {
        owner,
        repo,
        git_ref,
    })
}

/// The port of a URL on this machine (`localhost`, `127.0.0.1`, `[::1]`,
/// `0.0.0.0`), or `None` for a URL on another host. In the container these
/// names are the container itself, so such a URL reaches nothing.
pub fn loopback_port(url: &str) -> Option<u16> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split('/').next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => (host, port.parse().ok()),
        _ => (authority, None),
    };
    let host = host.to_ascii_lowercase();
    let loopback = matches!(
        host.as_str(),
        "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0"
    );
    loopback.then(|| port.unwrap_or(80))
}

/// Whether the chap arguments only ask for help or the version: no
/// arguments, or `--help`, `-h` or `--version` anywhere. Such a run touches
/// no model and writes no file, so chap-devops says nothing about either.
pub fn is_help(args: &[String]) -> bool {
    args.is_empty()
        || args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--help" | "-h" | "--version"))
}

/// A `--model-name` that can only be a model id: one word, no scheme, no
/// path separator, and no file or directory of that name here.
pub fn id_candidate(kind: &ModelKind, exists: bool) -> Option<String> {
    match kind {
        ModelKind::Local(path) if !exists => {
            let text = path.to_string_lossy();
            let word = !text.is_empty() && !text.starts_with('.') && !text.contains(['/', '\\']);
            word.then(|| text.into_owned())
        }
        _ => None,
    }
}

/// The host of a chapkit URL, the service name on a compose network:
/// `chapkit-ewars-model` for `http://chapkit-ewars-model:8000/`.
pub fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split('/').next()?;
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => host,
        _ => authority,
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// The note for a chapkit URL that does not answer: what is wrong, and the
/// way out that fits the URL.
pub fn unreachable_message(url: &str) -> String {
    let head = format!(
        "the model server at {url} is not running (no answer on /api/v1/info), so chap was \
         not started"
    );
    match url_host(url) {
        Some(host) if !host.contains('.') => format!(
            "{head}; `{host}` is not a model of this deployment or group. Give a model id \
             instead (`--model-name auto_arima_chapkit`) and chap-devops starts it, or start one \
             with `chap-devops run ID`; the `models:` line lists the URLs that answer"
        ),
        _ => format!("{head}; check that the server runs and that this machine can reach it"),
    }
}

/// The options that name a file chap writes: `--output-file`, and
/// `--out-file` of `chap simulate`.
const OUTPUT_OPTIONS: &[&str] = &["--output-file", "--out-file"];

/// The values of the output options in the chap arguments, in either
/// spelling.
pub fn output_files(args: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        for option in OUTPUT_OPTIONS {
            if arg == option {
                if let Some(value) = rest.next() {
                    found.push(value.clone());
                }
            } else if let Some(value) = arg
                .strip_prefix(option)
                .and_then(|tail| tail.strip_prefix('='))
            {
                found.push(value.to_string());
            }
        }
    }
    found
}

/// Why a chap command cannot work in a container, when chap-devops knows it does
/// not: `plot-dataset` shows its plot in a browser and writes no file.
pub fn needs_a_browser(args: &[String]) -> Option<String> {
    (args.first().map(String::as_str) == Some("plot-dataset") && !is_help(args)).then(|| {
        "`chap plot-dataset` shows its plot in a browser and writes no file, and the \
         container has no browser; `chap-devops chap plot-backtest` writes a plot of an \
         evaluation to a file"
            .to_string()
    })
}

/// The image a run uses, from what it needs and what this machine has.
///
/// The models that are not chapkit services run in the worker image, as
/// chap-core's own worker runs them: that image has uv and R. A chapkit
/// service is plain HTTP, and so is every command without a model, so either
/// image does those. They take the one that is on this machine already, so
/// the run downloads nothing: the worker image when only it is here, as in a
/// deployment that runs chap-core, and else the core image, which is the
/// smaller download. `core_here` and `worker_here` say which images this
/// machine has at the tag of the run.
pub fn image_for(kind: &ModelKind, core_here: bool, worker_here: bool) -> ChapImage {
    match kind {
        ModelKind::GitHub { .. } | ModelKind::Local(_) => ChapImage::Worker,
        // A model id resolves to a chapkit service before the run, and a
        // built-in model runs in chap itself.
        ModelKind::None | ModelKind::Builtin(_) | ModelKind::Chapkit(_) => {
            match (core_here, worker_here) {
                (false, true) => ChapImage::Worker,
                _ => ChapImage::Core,
            }
        }
    }
}

/// Whether the chap arguments ask chap to start the MLproject as a chapkit
/// service (`--run-config.as-chapkit`, chap-core v2.5.0 and later). chap
/// refuses `docker_env` itself in that mode.
pub fn as_chapkit(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "--run-config.as-chapkit"
            || arg
                .strip_prefix("--run-config.as-chapkit=")
                .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    })
}

/// The first chap-core release with built-in models, `--run-config.as-chapkit`
/// and `chap simulate`.
pub const FIRST_WITH_V2_5: (u64, u64, u64) = (2, 5, 0);

/// What the arguments ask for that only chap-core v2.5.0 and later have, if
/// anything, as the start of a sentence.
pub fn needs_v2_5(args: &[String], kind: &ModelKind) -> Option<&'static str> {
    let command = args.iter().find(|arg| !arg.starts_with('-'));
    if command.is_some_and(|command| command == "simulate") {
        Some("`chap simulate` needs")
    } else if matches!(kind, ModelKind::Builtin(_)) {
        Some("built-in models need")
    } else if as_chapkit(args) {
        Some("`--run-config.as-chapkit` needs")
    } else {
        None
    }
}

/// Why a run cannot work at `tag`: it asks for something newer than the
/// release. A moving tag, such as `master`, is not checked.
pub fn too_old(args: &[String], kind: &ModelKind, tag: &str) -> Option<String> {
    let what = needs_v2_5(args, kind)?;
    let version = crate::chapcore::release_version(tag)?;
    (version < FIRST_WITH_V2_5).then(|| {
        format!(
            "{what} chap-core v2.5.0 or later, and this run uses {tag}; give a newer \
             tag, such as `--tag v2.5.0`"
        )
    })
}

/// Whether an `MLproject` file asks for `docker_env`, which needs the docker
/// socket.
pub fn uses_docker_env(mlproject: &str) -> bool {
    mlproject
        .lines()
        .any(|line| line.trim_start().starts_with("docker_env:") && !line.starts_with('#'))
}

/// The newest of a list of image tags: the highest `vX.Y.Z` release, else
/// the first tag in the list.
pub fn newest_tag(tags: &[String]) -> Option<String> {
    let version = |tag: &str| -> Option<(u64, u64, u64)> {
        let mut parts = tag.strip_prefix('v').unwrap_or(tag).split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        parts.next().is_none().then_some((major, minor, patch))
    };
    tags.iter()
        .filter_map(|tag| version(tag).map(|v| (v, tag)))
        .max()
        .map(|(_, tag)| tag.clone())
        .or_else(|| tags.first().cloned())
}

/// `path` with `.` and `..` taken out, without asking the filesystem.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The paths in the chap arguments that are outside `cwd`: an absolute path,
/// a `../` path, and the same after `=` in `--flag=PATH`. A URL is not a
/// path.
pub fn outside_paths(args: &[String], cwd: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for arg in args {
        let value = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => value,
            _ => arg.as_str(),
        };
        if value.contains("://") {
            continue;
        }
        let looks_like_path =
            Path::new(value).is_absolute() || value == ".." || value.starts_with("../");
        if !looks_like_path {
            continue;
        }
        let path = normalize(&cwd.join(value));
        if !path.starts_with(cwd) && !found.contains(&path) {
            found.push(path);
        }
    }
    found
}

/// The directories to mount for `paths`: the nearest parent of each one that
/// exists, without the ones another mount covers already.
///
/// `Err` names a path whose only existing parent is the root directory,
/// which is not a thing to hand a container.
pub fn mount_dirs(
    paths: &[PathBuf],
    covered: &[PathBuf],
    exists: impl Fn(&Path) -> bool,
) -> Result<Vec<PathBuf>, PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for path in paths {
        let mut dir = path.clone();
        while !exists(&dir) {
            if !dir.pop() {
                break;
            }
        }
        if dir.parent().is_none() {
            return Err(path.clone());
        }
        dirs.push(dir);
    }
    dirs.sort();
    dirs.dedup();
    let mut kept: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        let inside = covered
            .iter()
            .chain(kept.iter())
            .any(|outer| dir.starts_with(outer));
        if !inside {
            kept.push(dir);
        }
    }
    Ok(kept)
}

/// Whether the run pulls its image first: only for a moving tag (`master`,
/// `dev`, `latest`) that `--tag` asked for, and never `--offline`.
///
/// `docker run` uses the local image of a tag when it has one, so without
/// this pull a `--tag master` runs the master of the day it was first
/// pulled. A tag that comes from the deployment is left alone: the run then
/// uses the same chap as the deployment, and `chap-devops update` re-pulls it.
pub fn pulls_first(asked: Option<&str>, offline: bool) -> bool {
    !offline && asked.is_some_and(crate::chapcore::is_moving_tag)
}

/// Everything one `docker run` of the chap CLI needs.
#[derive(Debug, Clone)]
pub struct RunSpec {
    /// `<repository>:<tag>`.
    pub image: String,
    /// The directory chap runs in, mounted at the same path.
    pub cwd: PathBuf,
    /// More directories mounted at the same path, for the paths outside `cwd`.
    pub mounts: Vec<PathBuf>,
    /// chap-devops's own directory for the runs and the caches.
    pub data: PathBuf,
    /// `uid:gid` to run as; `None` keeps the image's own user.
    pub user: Option<(u32, u32)>,
    /// The compose network to join, `<project>_default`.
    pub network: Option<String>,
    /// Mount the docker socket, with this group added when it is known.
    pub docker: Option<Option<u32>>,
    /// Whether stdin is a terminal, which is when `-t` is asked for.
    pub tty: bool,
    /// The command to run in the image: `chap` or `chapkit`.
    pub program: &'static str,
    /// The arguments for the command.
    pub args: Vec<String>,
}

/// The environment that puts every cache and every run under `data`, so the
/// second run of a model installs nothing again, and nothing is written
/// where the container's user may not write.
pub fn environment(data: &Path) -> Vec<(&'static str, String)> {
    let at = |rest: &str| data.join(rest).to_string_lossy().into_owned();
    vec![
        ("HOME", "/tmp".to_string()),
        // Kept, so matplotlib builds its font cache once and not on every run.
        ("MPLCONFIGDIR", at("cache/matplotlib")),
        ("CHAP_RUNS_DIR", at("runs")),
        ("XDG_CACHE_HOME", at("cache")),
        ("UV_CACHE_DIR", at("cache/uv")),
        ("UV_PYTHON_INSTALL_DIR", at("cache/python")),
        ("RENV_PATHS_ROOT", at("cache/renv")),
        // renv makes its sandbox read-only, which leaves a cache nobody can
        // delete without a chmod first.
        ("RENV_CONFIG_SANDBOX_ENABLED", "FALSE".to_string()),
    ]
}

/// The arguments after `docker`.
pub fn docker_args(spec: &RunSpec) -> Vec<String> {
    let mut out: Vec<String> = ["run", "--rm", "-i"].map(str::to_string).to_vec();
    if spec.tty {
        out.push("-t".to_string());
    }
    out.push("--init".to_string());
    out.push("--platform".to_string());
    out.push(crate::compose::AMD64_PLATFORM.to_string());
    if let Some((uid, gid)) = spec.user {
        out.push("--user".to_string());
        out.push(format!("{uid}:{gid}"));
    }
    let same_path = |dir: &Path| {
        let dir = dir.to_string_lossy();
        format!("{dir}:{dir}")
    };
    for dir in std::iter::once(&spec.cwd)
        .chain(spec.mounts.iter())
        .chain(std::iter::once(&spec.data))
    {
        out.push("-v".to_string());
        out.push(same_path(dir));
    }
    out.push("-w".to_string());
    out.push(spec.cwd.to_string_lossy().into_owned());
    for (name, value) in environment(&spec.data) {
        out.push("-e".to_string());
        out.push(format!("{name}={value}"));
    }
    if let Some(network) = &spec.network {
        out.push("--network".to_string());
        out.push(network.clone());
    }
    // Docker Desktop has this name already; on Linux, this adds it, so a
    // service published on this machine is reachable from the container.
    out.push("--add-host".to_string());
    out.push(format!("{}:host-gateway", crate::components::HOST_GATEWAY));
    if let Some(group) = spec.docker {
        let socket = crate::docker::DOCKER_SOCKET;
        out.push("-v".to_string());
        out.push(format!("{socket}:{socket}"));
        if let Some(gid) = group {
            out.push("--group-add".to_string());
            out.push(gid.to_string());
        }
    }
    out.push("--label".to_string());
    out.push(format!("{}=cli", crate::docker::KIND_LABEL));
    out.push(spec.image.clone());
    out.push(spec.program.to_string());
    if spec.args.is_empty() {
        out.push("--help".to_string());
    } else {
        out.extend(spec.args.iter().cloned());
    }
    out
}
