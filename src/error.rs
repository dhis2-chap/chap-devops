//! Typed errors for the parts of `chap-devops` where the caller needs to react to the
//! kind of failure, plus the crate-wide [`Result`] alias.
//!
//! Everything that is merely context is an [`anyhow::Error`]; only the cases
//! listed in [`ChapError`] carry meaning (exit codes, TUI messages, tests).

use std::path::{Path, PathBuf};

/// Errors that callers are expected to match on.
#[derive(thiserror::Error, Debug)]
pub enum ChapError {
    #[error("{}", not_a_deployment(.0, &crate::paths::run_groups_dir()))]
    NotAProject(PathBuf),

    #[error("compose files are out of date with .chap-devops/; run `chap-devops sync`")]
    OutOfSync,

    #[error(
        "{0} already contains a chap-devops deployment; choose another directory, or use --force to \
         overwrite it"
    )]
    AlreadyInitialized(PathBuf),

    #[error("unknown model `{0}`; run `chap-devops models list` to see the model ids")]
    UnknownModel(String),

    #[error("unknown component `{0}`; the components are chap-core, ocs, s3 and dhis2")]
    UnknownComponent(String),

    #[error(
        "`{0}` is a starter, not a deployable model; pass `--allow-starter` to enable it anyway"
    )]
    IsStarter(String),

    #[error("model `{id}` has no version `{version}`")]
    UnknownVersion { id: String, version: String },

    #[error("version {version} of `{id}` is yanked")]
    YankedVersion { id: String, version: String },

    #[error(
        "version {version} of `{id}` has the image tag {image_tag}, which is not `sha-` and the \
         start of its commit, so the image can change after the review; pick another version \
         with `--version`"
    )]
    UnpinnedImage {
        id: String,
        version: String,
        image_tag: String,
    },

    #[error(
        "the stable version {version} of `{id}` is {status}, not verified; pass \
         `--version {version}` to enable it anyway"
    )]
    UnverifiedStable {
        id: String,
        version: String,
        status: String,
    },

    #[error(
        "host port {port} is already in use by {holder}; pick another with `--port <n>`, or `--port auto`"
    )]
    PortInUse { port: u16, holder: PortHolder },

    #[error(
        "host port {port} is outside this deployment's model port range {lo}-{hi}; \
         pick one inside it or `--port auto`, or widen `port_range` in `.chap-devops/project.yaml`"
    )]
    PortOutOfRange { port: u16, lo: u16, hi: u16 },

    #[error("model registry unavailable: {0}")]
    RegistryUnavailable(String),

    #[error("docker compose exited with status {0}")]
    DockerFailed(i32),

    /// Ctrl-C stopped the command, after it took back out what it started.
    #[error("stopped by Ctrl-C; {0}")]
    Interrupted(String),

    /// A program chap-devops ran in a one-shot container exited non-zero. Its own
    /// message is already on the screen, so this names the program, the
    /// status and the way out, and keeps the status as the exit code.
    #[error("{command} exited with status {code}; {next}")]
    Exited {
        command: &'static str,
        code: i32,
        next: String,
    },

    /// A mistake about `chap-devops`'s own command line that clap cannot catch,
    /// such as a `-v` meant for compose reaching `chap-devops down`'s passthrough.
    #[error("{0}")]
    Usage(String),

    #[error("HTTP {status} from {url}")]
    Http { url: String, status: u16 },

    /// chap-core could not be reached at all, as opposed to answering with
    /// something the caller did not want. Its own exit code, so a script can
    /// tell "Chap is not up" from "Chap said no".
    #[error("chap-core at {url} is not responding: {reason}; run `chap-devops status`")]
    Unreachable { url: String, reason: String },

    /// The same for DHIS2, in its own words: a DHIS2 that is not answering is
    /// a different problem from a chap-core that is not, and the sentence has
    /// to send the reader to the right container. Its own exit code for the
    /// same reason [`ChapError::Unreachable`] has one.
    ///
    /// `next` is the way out: for the deployment's own DHIS2 that is its
    /// container, and for an external one the URL `chap-devops dhis2 use` recorded.
    #[error("DHIS2 at {url} is not responding: {reason}; {next}")]
    Dhis2Unreachable {
        url: String,
        reason: String,
        next: &'static str,
    },
}

/// The message of [`ChapError::NotAProject`]. A directory under the `chap-devops
/// run` groups is a group that is not there, and `chap-devops init` is not the way
/// to make one again.
fn not_a_deployment(dir: &Path, groups: &Path) -> String {
    let group = dir
        .strip_prefix(groups)
        .ok()
        .and_then(|rest| rest.components().next())
        .map(|first| first.as_os_str().to_string_lossy().into_owned());
    match group {
        Some(name) => {
            let again = match name.as_str() {
                "default" => "chap-devops run <model>".to_string(),
                _ => format!("chap-devops run <model> --group {name}"),
            };
            format!(
                "{} is not a chap-devops deployment: the `chap-devops run` group {name} does not exist, or \
                 `chap-devops stop --purge` removed it; `{again}` makes it again",
                dir.display()
            )
        }
        None => format!(
            "{} is not a chap-devops deployment (no .chap-devops/project.yaml here or in a parent \
             directory); run `chap-devops init` first",
            dir.display()
        ),
    }
}

/// Who holds a host port that was asked for, so the error can say where to
/// look: inside the deployment, or outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortHolder {
    /// A service in this project, or any `compose*.yml` in its directory.
    ComposeFile,
    /// Something outside this project has a listener bound to it.
    Host,
}

impl std::fmt::Display for PortHolder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortHolder::ComposeFile => f.write_str("another compose file"),
            PortHolder::Host => f.write_str("the host (something is listening)"),
        }
    }
}

/// Crate-wide result type. Errors are [`anyhow::Error`]; downcast to
/// [`ChapError`] when the specific variant matters.
pub type Result<T> = anyhow::Result<T>;

#[cfg(test)]
mod tests;
