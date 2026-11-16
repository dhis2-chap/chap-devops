//! The deployments chap-devops has written on this machine, by compose project.
//!
//! A compose project name ends in random characters
//! ([`crate::project::new_compose_project_name`]), so a volume docker holds
//! cannot be traced back to the directory that made it. `chap-devops cleanup` needs
//! exactly that to tell the data of a deployment that was deleted from the
//! data of one that is only down, so every save of a project records its
//! name and directory here, in `<data dir>/deployments.yaml`.
//!
//! Best-effort throughout: a record that could not be written only means
//! `chap-devops cleanup` leaves that deployment's volumes alone, which is the safe
//! way to be wrong.

use crate::project::Project;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The file the record lives in.
pub fn file() -> PathBuf {
    crate::paths::data_dir().join("deployments.yaml")
}

/// Every recorded deployment: compose project name to directory. Empty when
/// nothing was recorded or the file cannot be read.
pub fn load() -> BTreeMap<String, PathBuf> {
    load_from(&file())
}

fn load_from(path: &Path) -> BTreeMap<String, PathBuf> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_yaml_ng::from_str(&text).ok())
        .unwrap_or_default()
}

/// Record the deployment in `project`, unless it is recorded already.
///
/// Unit tests save projects in temporary directories by the hundred, and none
/// of them may touch the record of the machine they run on.
pub fn record(project: &Project) {
    if cfg!(test) {
        return;
    }
    let Some(name) = project.compose_project_name() else {
        return;
    };
    let dir = std::path::absolute(&project.dir).unwrap_or_else(|_| project.dir.clone());
    let path = file();
    let mut known = load_from(&path);
    if known.get(&name) == Some(&dir) {
        return;
    }
    known.insert(name, dir);
    // Best effort: the record only helps `top` and `cleanup` find the
    // deployment later, so a failure is a trace line, not an error.
    if let Err(err) = write(&path, &known) {
        crate::output::verbose(&format!(
            "could not record this deployment in {}: {err:#}",
            path.display()
        ));
    }
}

/// Drop these projects from the record: what `chap-devops cleanup` took away.
pub fn forget(names: &[String]) -> crate::error::Result<()> {
    let path = file();
    let mut known = load_from(&path);
    let before = known.len();
    known.retain(|name, _| !names.contains(name));
    if known.len() == before {
        return Ok(());
    }
    write(&path, &known)
}

/// Whether the deployment recorded as `name` in `dir` is still there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    /// The directory holds it.
    Present,
    /// Provably gone: the directory was deleted from a parent that is still
    /// there, or it now holds a deployment of another name (`chap-devops init
    /// --force` wrote over it).
    Gone,
    /// Neither can be proven, and why. Kept, because deleting a live
    /// deployment's data is the one mistake that cannot be undone.
    Unsure(String),
}

/// Where the deployment recorded as `name` in `dir` stands. Only the name in
/// `project.yaml` is read, so a typo in another state file does not make a
/// deployment look gone.
pub fn presence(name: &str, dir: &Path) -> Presence {
    if dir.exists() {
        return match Project::recorded_name(dir) {
            Ok(Some(recorded)) if recorded == name => Presence::Present,
            Ok(Some(_)) => Presence::Gone,
            Ok(None) => Presence::Unsure(format!(
                "{} records no compose project name; `chap-devops -C {} sync` writes it",
                dir.display(),
                dir.display()
            )),
            Err(err) => Presence::Unsure(format!("{err:#}; fix that file, then run this again")),
        };
    }
    match dir.parent() {
        Some(parent) if parent.exists() => Presence::Gone,
        _ => Presence::Unsure(format!(
            "{} is missing along with its parent directory, as on a disk that is not mounted; \
             mount it, or remove its volumes with `docker volume rm` once you are sure",
            dir.display()
        )),
    }
}

/// The other directories that record the compose project name `name`: the
/// recorded deployments, the directories beside `dir` and, when
/// `compose_ls` answers, the deployments docker knows. Two such directories
/// share every container and volume, so `chap-devops down --volumes` in one of
/// them deletes the data of both.
pub fn claimed_elsewhere(
    name: &str,
    dir: &Path,
    compose_ls: &dyn Fn() -> Option<String>,
) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = load().into_values().collect();
    candidates.extend(crate::ports::sibling_dirs(dir));
    if let Some(text) = compose_ls() {
        candidates.extend(crate::docker::compose_ls_dirs(&text));
    }
    same_name(name, dir, candidates)
}

/// The `candidates` other than `dir` whose `project.yaml` records `name`,
/// each one once, in the resolved spelling.
pub fn same_name(name: &str, dir: &Path, candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    let own = crate::ports::real_path(dir);
    let mut found: Vec<PathBuf> = Vec::new();
    for path in candidates {
        let real = crate::ports::real_path(&path);
        if real == own || found.contains(&real) {
            continue;
        }
        if matches!(Project::recorded_name(&real), Ok(Some(recorded)) if recorded == name) {
            found.push(real);
        }
    }
    found
}

/// The warning for a directory that records the same compose project name
/// as this deployment, with the way out.
pub fn shared_name_line(name: &str, other: &Path) -> String {
    format!(
        "{} also records the compose project name {name}, so the two directories use the \
         same containers and volumes; use only one of them, or remove {}",
        other.display(),
        other.display()
    )
}

/// Write the record through a temporary file of this process's own, so two
/// chap-devops saving at once cannot rename each other's half-written file.
fn write(path: &Path, known: &BTreeMap<String, PathBuf>) -> crate::error::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("creating {}: {e}", parent.display()))?;
    }
    let body = serde_yaml_ng::to_string(known)?;
    let tmp = path.with_file_name(format!(".deployments.{}.tmp", std::process::id()));
    std::fs::write(&tmp, body).map_err(|e| anyhow::anyhow!("writing {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| anyhow::anyhow!("renaming {} to {}: {e}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests;
