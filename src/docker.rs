//! Thin wrappers around `docker compose`.
//!
//! Every invocation passes the project's explicit `-f` list, so the wrappers
//! work from any working directory and never depend on compose's own file
//! discovery.

mod captured;
mod images;
mod labels;
mod leftovers;
mod oneshot;
mod plain;
mod ps;
mod query;
mod stats;
mod version;
mod volumes;

pub use captured::compose_captured;
pub use images::{
    ContainerBuild, container_builds, image_command, image_config, image_ids, pull_image,
    running_build, short_digest, uid_gid_in_image,
};
pub use labels::{
    COMPOSE_DIR_LABEL, COMPOSE_PROJECT_LABEL, COMPOSE_SERVICE_LABEL, GROUP_LABEL, KIND_LABEL,
    Labeled, MODEL_LABEL, ROLE_LABEL, chap_devops_containers,
};
pub use leftovers::{default_network_exists, project_has_containers, volume_in_use};
pub use oneshot::{
    DOCKER_SOCKET, chapkit_answers, image_is_local, local_tags, run_plain, socket_gid,
};
pub use plain::{run_compose_plain, strip_ansi};
pub use ps::{
    Container, all_containers, all_containers_or_why, diff_containers, ps_entries,
    running_containers, running_containers_or_why, running_of, running_services,
    service_is_healthy, service_names,
};
pub use query::{
    compose_ls_dirs, compose_ls_json, config_hashes, config_services, container_publishing,
    dhis2_seed_mark, image_count, service_images, service_logs,
};
pub use stats::{Usage, container_usage};
pub use version::{check_compose_version, compose_version};
pub use volumes::{
    Removal, copy_volume, ocs_data_bytes, remove_volume, volume_exists, volume_names_of_project,
    volume_names_with_prefix, volume_size_bytes, volumes_with_prefix,
};

use crate::error::{ChapError, Result};
use crate::project::Project;
use std::process::{Command, ExitStatus, Stdio};

/// Minimum compose version that understands everything `chap-devops sync` renders.
///
/// 2.24.4 is the release the `!override` YAML tag arrived in, and every
/// `compose.chap-devops.yml` uses it to replace chap's own port mapping rather than
/// add to it. `include:`, which the marketplace umbrella needs, is older:
/// [`INCLUDE_COMPOSE_VERSION`]. So a Compose between the two loads the model
/// overlays and then publishes the API on two ports, which is the more
/// confusing of the two failures.
pub const MIN_COMPOSE_VERSION: (u32, u32, u32) = (2, 24, 4);

/// Compose version `include:` arrived in.
///
/// Named alongside [`MIN_COMPOSE_VERSION`] in the warning because it is the
/// older of the two requirements, and an operator on something older again
/// loses the model overlays entirely rather than just the port override.
pub const INCLUDE_COMPOSE_VERSION: (u32, u32, u32) = (2, 20, 0);

/// Exit code used when the `docker` binary itself cannot be run, mirroring the
/// shell's "command not found".
pub const DOCKER_NOT_FOUND: i32 = 127;

/// Echo a `docker` invocation under `-v`, the way a person would have typed it.
///
/// Every spawn in this module goes through it, so `-v` is a complete record of
/// what `chap-devops` asked Docker to do.
fn trace_command(args: &[String]) {
    crate::output::verbose(&format!("$ docker {}", args.join(" ")));
}

/// The leading `docker` arguments for a project: `compose -f ... -f ...`.
///
/// The paths are absolute so the child process does not depend on its working
/// directory, and they are in the order recorded in `.chap-devops/project.yaml`: later files
/// override earlier ones.
pub fn compose_args(project: &Project) -> Vec<String> {
    let mut args = Vec::with_capacity(1 + project.state.compose_files.len() * 2);
    args.push("compose".to_string());
    for path in project.compose_file_paths() {
        args.push("-f".to_string());
        args.push(path.to_string_lossy().into_owned());
    }
    args
}

/// Whether compose is told to print no progress: when the output is not a
/// terminal and `-vv` was not given.
pub(crate) fn quiet_progress(terminal: bool, verbose: bool) -> bool {
    !terminal && !verbose
}

/// Put `--progress quiet` after `compose` for a run whose output goes to the
/// user. Without a terminal, compose prints a line for each step of each image
/// pull, hundreds of lines in a log; chap-devops says what happened itself, and
/// `-vv` keeps the lines.
pub(crate) fn with_quiet_progress(args: &mut Vec<String>) {
    let terminal = std::io::IsTerminal::is_terminal(&std::io::stdout());
    if quiet_progress(terminal, crate::output::verbose_enabled()) {
        args.splice(1..1, ["--progress".to_string(), "quiet".to_string()]);
    }
}

/// Run `docker compose <args> <extra>` with inherited stdio, returning the
/// child's exit code.
///
/// The child runs in the project directory so compose picks up `.env` and
/// names the project after the directory, exactly as a hand-written
/// `docker compose` would.
pub fn run_compose(project: &Project, extra: &[String]) -> Result<i32> {
    let mut args = compose_args(project);
    with_quiet_progress(&mut args);
    args.extend(extra.iter().cloned());
    trace_command(&args);

    let status = Command::new("docker")
        .args(&args)
        .current_dir(&project.dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| spawn_error(&e))?;

    Ok(exit_code(status))
}

/// Run `docker compose <args> <extra>` with stdout inherited and stderr teed:
/// everything docker writes there goes to this process's stderr as it
/// arrives, and is kept.
///
/// Compose says why a service would not start on stderr and nowhere else -
/// `dependency failed to start: container x-chap-1 is unhealthy` is the whole
/// of it - so a wrapper that wants to explain that failure has to have read
/// it. What is read is passed on byte for byte as it arrives, so the command
/// still reads as it runs and nothing docker wrote is reformatted, reordered
/// or held back.
///
/// The one visible difference from [`run_compose`] is compose's own doing: a
/// pipe is not a terminal, so it prints its progress as one line per step
/// rather than redrawing a block in place.
pub fn run_compose_teed(project: &Project, extra: &[String]) -> Result<Piped> {
    run_compose_teed_with(project, &[], extra)
}

/// [`run_compose_teed`] with more compose files after the project's own, for
/// a change that holds for this one command only.
pub fn run_compose_teed_with(
    project: &Project,
    files: &[std::path::PathBuf],
    extra: &[String],
) -> Result<Piped> {
    use std::io::{Read, Write};

    let mut args = compose_args(project);
    for file in files {
        args.push("-f".to_string());
        args.push(file.to_string_lossy().into_owned());
    }
    args.extend(extra.iter().cloned());
    trace_command(&args);

    let mut child = Command::new("docker")
        .args(&args)
        .current_dir(&project.dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| spawn_error(&e))?;

    let mut raw: Vec<u8> = Vec::new();
    if let Some(mut stream) = child.stderr.take() {
        let mut buffer = [0u8; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let chunk = &buffer[..read];
                    let mut err = std::io::stderr();
                    let _ = err.write_all(chunk);
                    let _ = err.flush();
                    raw.extend_from_slice(chunk);
                }
            }
        }
    }
    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("waiting for docker: {e}"))?;
    Ok(Piped {
        code: exit_code(status),
        stderr: String::from_utf8_lossy(&raw).into_owned(),
    })
}
/// Why a short `docker compose` query went unanswered.
///
/// "exited with status 1" is no use on its own: a daemon that is not running,
/// a daemon in Windows-containers mode and a stack file compose will not read
/// all look the same from the exit code. Docker says which it is on stderr,
/// and its first line is what this carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryFailure {
    /// The query as it was typed after `docker compose`, such as `ps -a`.
    pub query: String,
    /// The status docker exited with, or `None` when it could not be run.
    pub code: Option<i32>,
    /// Docker's own first words: the first line of its stderr with anything
    /// on it, or the reason it could not be started.
    pub detail: String,
}

impl QueryFailure {
    /// Docker ran and refused to answer.
    fn refused(query: &str, code: i32, stderr: &str) -> QueryFailure {
        QueryFailure {
            query: query.to_string(),
            code: Some(code),
            detail: first_stderr_line(stderr).unwrap_or_default(),
        }
    }

    /// Docker could not be started at all.
    fn unusable(query: &str, err: &std::io::Error) -> QueryFailure {
        QueryFailure {
            query: query.to_string(),
            code: None,
            detail: err.to_string(),
        }
    }

    /// Whether docker ran at all, as opposed to not being there to run.
    ///
    /// A missing `docker` is not worth a word from a command that only asked
    /// it in passing; a docker that answered and refused is.
    pub fn ran(&self) -> bool {
        self.code.is_some()
    }
}

impl std::fmt::Display for QueryFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let detail = if self.detail.is_empty() {
            String::new()
        } else {
            format!(": {}", self.detail)
        };
        match self.code {
            Some(code) => write!(
                f,
                "`docker compose {}` exited with status {code}{detail}",
                self.query
            ),
            None => write!(f, "`docker` could not be run{detail}"),
        }
    }
}

/// The first line of docker's stderr with anything on it.
///
/// Compose pads its diagnostics with blank lines and with progress it has
/// already erased; the first line with words on it names the problem.
fn first_stderr_line(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

/// Run a short `docker compose` query and return its stdout, or why it went
/// unanswered.
///
/// Nothing is printed on failure: every caller has a better thing to say than
/// docker's own noise, and the [`QueryFailure`] is how the callers that want
/// a word of it get one.
fn compose_query(project: &Project, extra: &[&str]) -> std::result::Result<String, QueryFailure> {
    let mut args = compose_args(project);
    args.extend(extra.iter().map(|a| a.to_string()));
    trace_command(&args);
    let query = extra.join(" ");
    let out = Command::new("docker")
        .args(&args)
        .current_dir(&project.dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| QueryFailure::unusable(&query, &e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let failure = QueryFailure::refused(&query, exit_code(out.status), &stderr);
        crate::output::verbose(&format!("  {failure}"));
        return Err(failure);
    }
    let body = String::from_utf8_lossy(&out.stdout).into_owned();
    crate::output::debug(&format!(
        "  docker said:\n{}",
        crate::output::trace_body(body.trim_end())
    ));
    Ok(body)
}

/// Run a short `docker compose` query and return its stdout, or `None` when
/// docker could not be run or answered non-zero.
///
/// The best-effort half of [`compose_query`], for the callers that have
/// nothing to say about a failure beyond not having an answer.
fn compose_capture(project: &Project, extra: &[&str]) -> Option<String> {
    compose_query(project, extra).ok()
}
/// Run `docker compose <args>` and capture everything it prints.
///
/// For the short, machine-read commands (`ps --format json`, a `psql -tAc`
/// one-liner): both streams are pipes, so this must stay away from anything
/// that produces real volume. Returns the exit code, stdout and stderr; a
/// non-zero exit is the caller's to interpret.
pub fn compose_output(project: &Project, extra: &[String]) -> Result<(i32, String, String)> {
    compose_output_with(project, &[], extra)
}

/// [`compose_output`] with more compose files after the project's own.
pub fn compose_output_with(
    project: &Project,
    files: &[std::path::PathBuf],
    extra: &[String],
) -> Result<(i32, String, String)> {
    let mut args = compose_args(project);
    for file in files {
        args.push("-f".to_string());
        args.push(file.to_string_lossy().into_owned());
    }
    args.extend(extra.iter().cloned());
    trace_command(&args);
    let out = Command::new("docker")
        .args(&args)
        .current_dir(&project.dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| spawn_error(&e))?;
    Ok((
        exit_code(out.status),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

/// What a redirected compose command did.
#[derive(Debug, Clone)]
pub struct Piped {
    pub code: i32,
    /// Everything the command wrote to stderr.
    pub stderr: String,
}

/// Run `docker compose <args>` with its payload streams wired to files.
///
/// This is how the dumps travel: `pg_dump` writes gigabytes to `stdout`, and a
/// model tar arrives on `stdin`, so neither may be a pipe this process has to
/// drain - a full pipe buffer with nobody reading it is a deadlock. Only
/// stderr is a pipe, and it is read while the child runs.
pub fn run_compose_piped(
    project: &Project,
    extra: &[String],
    stdin: Option<&std::path::Path>,
    stdout: Option<&std::path::Path>,
) -> Result<Piped> {
    use std::io::Read;

    let mut args = compose_args(project);
    args.extend(extra.iter().cloned());

    trace_command(&args);
    let mut cmd = Command::new("docker");
    cmd.args(&args).current_dir(&project.dir);
    match stdin {
        Some(path) => {
            let file = std::fs::File::open(path)
                .map_err(|e| anyhow::anyhow!("opening {}: {e}", path.display()))?;
            cmd.stdin(Stdio::from(file));
        }
        None => {
            cmd.stdin(Stdio::null());
        }
    }
    match stdout {
        Some(path) => {
            let file = std::fs::File::create(path)
                .map_err(|e| anyhow::anyhow!("creating {}: {e}", path.display()))?;
            cmd.stdout(Stdio::from(file));
        }
        None => {
            cmd.stdout(Stdio::null());
        }
    }
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| spawn_error(&e))?;
    let mut stderr = String::new();
    if let Some(mut stream) = child.stderr.take() {
        let _ = stream.read_to_string(&mut stderr);
    }
    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("waiting for docker: {e}"))?;
    Ok(Piped {
        code: exit_code(status),
        stderr,
    })
}
/// Remove a compose project's default network, which `compose down` leaves
/// behind once the compose files have no services left to name it. Best
/// effort: a network that is not there is what was wanted.
pub fn remove_default_network(project_name: &str) {
    let args = ["network", "rm", &format!("{project_name}_default")].map(str::to_string);
    let _ = docker_capture(&args);
}

/// Run a plain `docker` command (no `compose`, no project) and return its
/// stdout, or `None` when it could not be run or answered non-zero.
///
/// Best-effort like [`compose_capture`]: every caller is sharpening an answer
/// it can give without docker's help.
fn docker_capture(args: &[String]) -> Option<String> {
    let out = docker_run(args)?;
    if !out.ok {
        verbose_exit(args, out.code);
        return None;
    }
    Some(out.stdout)
}

/// What one plain `docker` run said.
///
/// Both streams, not just stdout: a caller that has a second way of asking
/// has to read the refusal to know whether to take it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DockerRun {
    /// Whether docker exited zero.
    pub ok: bool,
    /// The exit code, or `None` when there was no status to read.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// How [`image_config_with`] runs docker: the arguments as they would be
/// typed, and `None` when the binary could not be run at all.
///
/// [`image_config_with`]: images::image_config_with
pub type RunFn<'a> = &'a dyn Fn(&[String]) -> Option<DockerRun>;

/// Run a plain `docker` command and return both its streams, whatever it
/// exited with. `None` is "the binary could not be run".
fn docker_run(args: &[String]) -> Option<DockerRun> {
    trace_command(args);
    let out = Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    Some(DockerRun {
        ok: out.status.success(),
        code: Some(exit_code(out.status)),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// The `-v` line for a `docker` run that answered non-zero.
fn verbose_exit(args: &[String], code: Option<i32>) {
    crate::output::verbose(&format!(
        "  `docker {}` exited with status {}",
        args.first().map(String::as_str).unwrap_or_default(),
        code.unwrap_or(DOCKER_NOT_FOUND)
    ));
}
/// Turn a spawn failure into an error that keeps the shell's "not found" exit
/// code, so `chap-devops up` behaves like the command it wraps.
fn spawn_error(err: &std::io::Error) -> anyhow::Error {
    if err.kind() == std::io::ErrorKind::NotFound {
        anyhow::Error::new(ChapError::DockerFailed(DOCKER_NOT_FOUND)).context(
            "`docker` was not found on PATH; install Docker Desktop or the docker CLI \
             (https://docs.docker.com/get-docker/)",
        )
    } else {
        anyhow::anyhow!("could not run `docker`: {err}")
    }
}

/// The child's exit code, mapping death by signal to `128 + signal` the way a
/// shell does.
fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    1
}

#[cfg(test)]
mod tests;
