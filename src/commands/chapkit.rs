//! `chap-devops chapkit`: the chapkit CLI, run in the chap-core image.
//!
//! chapkit is a dependency of chap-core, so the chap-core image has its CLI,
//! and there is no other image to pin. The run is the one `chap-devops chap`
//! makes: the current directory is mounted at the same path, and the run is
//! the user of the host, so every file chapkit writes is on the host. See
//! `docs/chap-cli.md`.
//!
//! `mlproject run` is the exception: it serves a model, which needs the
//! model's language and a port, so [`serve`] runs it in a chapkit `-cli`
//! image instead.

use super::chap::files::Snapshot;
use super::chap::{host_user, plan, resolve_tag};
use crate::cli::{ChapImage, ChapkitArgs};
use crate::commands::Ctx;
use crate::commands::run::target;
use crate::docker;
use crate::error::{ChapError, Result};
use crate::output;
use crate::project::Project;
use plan::RunSpec;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

/// The exit code `docker run` gives when it could not start the container.
const DOCKER_RUN_FAILED: i32 = 125;

/// The exit code chapkit gives when its arguments are not complete or not
/// correct, after it prints its usage.
const USAGE_ERROR: i32 = 2;

/// How many written files the closing line names.
const WRITTEN_LIMIT: usize = 8;

/// Run the chapkit CLI in the chap-core image, and say what it wrote.
pub fn run(ctx: &Ctx, args: &ChapkitArgs) -> Result<()> {
    if args.image.as_deref() == Some("") {
        return serve::list(ctx);
    }
    if ctx.out.json {
        return Err(ChapError::Usage(
            "`chap-devops chapkit` prints chapkit's own output, so --json does not apply; drop it"
                .to_string(),
        )
        .into());
    }
    let chapkit_args = with_group_help(&args.args);
    if let Some(rest) = serve_arguments(&chapkit_args) {
        return serve::run(ctx, args, rest);
    }
    if args.image.is_some() {
        return Err(ChapError::Usage(
            "--image picks the image of `mlproject run` only; drop it, or use \
             `chap-devops chapkit --image IMAGE mlproject run DIR`"
                .to_string(),
        )
        .into());
    }
    if let Some(why) = refused(&chapkit_args) {
        return Err(ChapError::Usage(why).into());
    }
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("reading the current directory: {e}"))?;
    let place = target(ctx, None)?;
    let deployment =
        match place.group.is_none() && place.dir.join(crate::project::CHAP_DEVOPS_DIR).is_dir() {
            true => Some(Project::load(&place.dir)?),
            false => None,
        };
    let tag = resolve_tag(ctx, args.tag.as_deref(), deployment.as_ref(), false)?;
    let reference = format!("{}:{tag}", plan::CORE_REPOSITORY);
    crate::commands::chap::pull_moving_tag(ctx, args.tag.as_deref(), &reference);

    let outside = plan::outside_paths(&chapkit_args, &cwd);
    let mounts =
        plan::mount_dirs(&outside, std::slice::from_ref(&cwd), Path::exists).map_err(|path| {
            ChapError::Usage(format!(
                "`{}` is outside the current directory, and no directory above it exists to \
                 mount; create its directory first, or use a path in {}",
                path.display(),
                cwd.display()
            ))
        })?;
    let data = crate::paths::data_dir().join("chap");
    std::fs::create_dir_all(data.join("cache"))
        .map_err(|e| anyhow::anyhow!("creating {}: {e}", data.join("cache").display()))?;

    let help = plan::is_help(&chapkit_args);
    let command = command_name(&chapkit_args);
    let pull = match docker::image_is_local(&reference) {
        true => String::new(),
        false => format!(
            "; the first run pulls it, about {}",
            plan::approximate_size(ChapImage::Core)
        ),
    };
    output::notice(&format!("running `chapkit {command}` in {reference}{pull}"));
    if !help {
        let mut seen = vec![cwd.display().to_string()];
        seen.extend(mounts.iter().map(|dir| dir.display().to_string()));
        output::notice(&format!(
            "files: chapkit reads and writes in {}",
            seen.join(", ")
        ));
    }

    let spec = RunSpec {
        image: reference.clone(),
        cwd: cwd.clone(),
        mounts: mounts.clone(),
        data,
        user: host_user(),
        network: None,
        docker: None,
        tty: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        program: "chapkit",
        args: chapkit_args.clone(),
    };
    let roots: Vec<PathBuf> = std::iter::once(cwd.clone()).chain(mounts).collect();
    let before = Snapshot::take(&roots, &[]);
    let code = docker::run_plain(&plan::docker_args(&spec))?;
    let written = before.written(&Snapshot::take(&roots, &[]));

    match code {
        0 => {
            if !help {
                report(&written, &cwd);
                for dir in needs_lock(&written) {
                    lock(&spec, &dir, &cwd);
                }
            }
            Ok(())
        }
        USAGE_ERROR => Err(ChapError::Usage(usage_hint(&chapkit_args)).into()),
        DOCKER_RUN_FAILED => Err(ChapError::Exited {
            command: "docker run",
            code,
            next: format!(
                "docker could not start {reference}, and its message is above; \
                 `chap-devops chapkit --tag TAG` picks another chap-core tag"
            ),
        }
        .into()),
        _ => {
            if !written.is_empty() {
                report(&written, &cwd);
            }
            Err(ChapError::Exited {
                command: "chapkit",
                code,
                next: "its own message is above".to_string(),
            }
            .into())
        }
    }
}

/// The arguments after `run` when they ask to serve an MLproject:
/// `mlproject run ...`, or chapkit's older spelling `run ...`.
fn serve_arguments(args: &[String]) -> Option<&[String]> {
    let positions: Vec<usize> = args
        .iter()
        .enumerate()
        .filter(|(_, arg)| !arg.starts_with('-'))
        .map(|(index, _)| index)
        .take(2)
        .collect();
    match positions.as_slice() {
        [first, ..] if args[*first] == "run" => Some(&args[first + 1..]),
        [first, second] if args[*first] == "mlproject" && args[*second] == "run" => {
            Some(&args[second + 1..])
        }
        _ => None,
    }
}

/// Why chap-devops refuses the chapkit arguments, with the chap-devops command that
/// does the work: `chapkit test` needs a service that the container can
/// reach, and the container is on no network of a model.
fn refused(args: &[String]) -> Option<String> {
    let words: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|arg| !arg.starts_with('-'))
        .take(2)
        .collect();
    match words.as_slice() {
        ["test", ..] => Some(
            "`chapkit test` in this container cannot reach a model service; use \
             `chap-devops models test ID`, which runs it in the model's own container"
                .to_string(),
        ),
        _ => None,
    }
}

/// The chapkit command groups: with no command after them, chapkit prints
/// their help and exits with 2, as Click does for `no_args_is_help`.
const GROUPS: &[&str] = &["artifact", "mlproject"];

/// The chapkit command the arguments name, for the line before the run:
/// `mlproject export` for a command in a group, `init` for one that is not.
fn command_name(args: &[String]) -> String {
    match args.first().map(String::as_str) {
        None => "--help".to_string(),
        Some(group) if GROUPS.contains(&group) => match args.get(1) {
            Some(next) if !next.starts_with('-') => format!("{group} {next}"),
            _ => group.to_string(),
        },
        Some(first) => first.to_string(),
    }
}

/// The arguments with `--help` added after a group with nothing after it,
/// so chapkit prints the same help and exits with 0.
fn with_group_help(args: &[String]) -> Vec<String> {
    let mut args = args.to_vec();
    if let [group] = args.as_slice()
        && GROUPS.contains(&group.as_str())
    {
        args.push("--help".to_string());
    }
    args
}

/// The error after a usage error of chapkit, with the way out: the help of
/// the chapkit command the arguments name, which lists what it takes. The
/// exit code stays 2, as for a usage error of chap-devops.
fn usage_hint(args: &[String]) -> String {
    let words: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .take_while(|arg| !arg.starts_with('-'))
        .take(2)
        .collect();
    let command = match words.as_slice() {
        [] => String::new(),
        words => format!(" {}", words.join(" ")),
    };
    format!(
        "chapkit did not accept the arguments, and its usage is above; \
         `chap-devops chapkit{command} --help` lists what the command takes"
    )
}

/// The project directories in `written` whose `Dockerfile` copies a
/// `uv.lock` that is not there: what `chapkit init` and `chapkit mlproject
/// migrate` write. chapkit says to run `uv lock` on the host; chap-devops runs it
/// in the same image, so the build needs no uv on the host.
fn needs_lock(written: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = written
        .iter()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name == "pyproject.toml")
        })
        .filter_map(|path| path.parent().map(Path::to_path_buf))
        .filter(|dir| !dir.join("uv.lock").exists())
        .filter(|dir| {
            std::fs::read_to_string(dir.join("Dockerfile")).is_ok_and(|text| copies_lock(&text))
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Whether a `COPY` line of the Dockerfile names `uv.lock`.
fn copies_lock(dockerfile: &str) -> bool {
    dockerfile.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("COPY") && line.split_whitespace().any(|word| word == "uv.lock")
    })
}

/// Run `uv lock` for `dir` in the image of `spec`, and say what came of it.
fn lock(spec: &RunSpec, dir: &Path, cwd: &Path) {
    let shown = dir.strip_prefix(cwd).unwrap_or(dir).display().to_string();
    let (shown, file) = match shown.is_empty() {
        true => (".".to_string(), "uv.lock".to_string()),
        false => (shown.clone(), format!("{shown}/uv.lock")),
    };
    let run = RunSpec {
        tty: false,
        program: "uv",
        args: vec![
            "lock".to_string(),
            "--quiet".to_string(),
            "--directory".to_string(),
            dir.to_string_lossy().into_owned(),
        ],
        ..spec.clone()
    };
    match docker::run_plain(&plan::docker_args(&run)) {
        Ok(0) if dir.join("uv.lock").exists() => output::notice(&format!(
            "wrote {file} with `uv lock` in the same image, so `docker build` works"
        )),
        _ => output::warn(&format!(
            "could not write {file}, and the `Dockerfile` copies it; run \
             `chap-devops chapkit --help` to make sure the image runs, then `uv lock` in {shown}"
        )),
    }
}

/// The closing line: what the run wrote.
fn report(written: &[PathBuf], cwd: &Path) {
    if written.is_empty() {
        output::notice(&format!(
            "chapkit finished; it wrote no file in {}",
            cwd.display()
        ));
        return;
    }
    output::notice(&format!(
        "chapkit finished; it wrote {}",
        super::chap::files::describe(written, cwd, WRITTEN_LIMIT)
    ));
}

mod serve;

#[cfg(test)]
mod tests;
