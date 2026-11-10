//! Checks of the deployment directory: its files, name, volumes, `.env`, ports and components.

use super::*;

/// The files a deployment directory holds, as `chap-devops init` writes them: the
/// `.chap-devops/` state, `.env` and the compose files rendered from them.
///
/// The base stack is only one of them when chap-core is a component of this
/// deployment: `--without chap-core` renders neither `compose.yml` nor the
/// chap-devops-owned override, so looking for them would report a deployment that is
/// exactly as asked for as broken.
///
/// What a component keeps in its own directory
/// ([`crate::components::Component::dir`]) is deliberately not in here. Those
/// files are the operator's rather than rendered, and the `components` line
/// already judges each of them with the one thing to do about it - `chap-devops sync`
/// scaffolds a missing `dhis2/dhis.conf`, and it says why DHIS2 will not start
/// without it - so counting them here would report one fault on two lines and
/// offer `chap-devops init --force` for a file `sync` writes on its own.
/// [`files_verdict`] names the directories instead, so the count cannot be read
/// as covering them.
pub fn project_files(components: &Components) -> Vec<String> {
    let mut files = vec![
        format!("{CHAP_DEVOPS_DIR}/{PROJECT_FILE}"),
        format!("{CHAP_DEVOPS_DIR}/{MODELS_FILE}"),
        format!("{CHAP_DEVOPS_DIR}/{COMPONENTS_FILE}"),
        ENV_FILE.to_string(),
    ];
    if components.chap_core.enabled {
        files.push(BASE_COMPOSE.to_string());
        files.push(CHAP_DEVOPS_COMPOSE.to_string());
    }
    files.extend(components.compose_files());
    files.push(MARKETPLACE_COMPOSE.to_string());
    files
}

/// Whether every file a deployment is made of is still there.
///
/// `elsewhere` is the directories of the enabled components, which this check
/// does not look in. They are named on the `ok` line so the count is read for
/// what it is: `all 8 present` on a deployment whose `dhis2/dhis.conf` had been
/// deleted was a true count of a set that did not include the one file DHIS2
/// cannot start without, which is a line stating something untrue while the
/// `components` line right under it fails.
///
/// The `fail` carries none of it, for the reason the one in `ocs_part` (`components.rs`) does not: the
/// one thing to do is the fix, and a deployment missing its rendered files is
/// told to render them again whatever else is also gone.
pub fn files_verdict(
    total: usize,
    missing: &[String],
    elsewhere: &[&str],
) -> (Status, String, Option<String>) {
    if missing.is_empty() {
        return (
            Status::Ok,
            format!("all {total} present{}", elsewhere_note(elsewhere)),
            None,
        );
    }
    (
        Status::Fail,
        format!("missing {}", missing.join(", ")),
        Some(
            "run `chap-devops sync` to render the compose files again, or `chap-devops init --force` here \
             to write the whole deployment"
                .to_string(),
        ),
    )
}

/// The tail that keeps `all N present` from claiming the component directories
/// it did not count: `; ocs/ and dhis2/ are on the `components` line`.
///
/// Empty for a deployment that has no such component, where there is nothing
/// left out and a tail would be noise.
fn elsewhere_note(dirs: &[&str]) -> String {
    let named: Vec<String> = dirs.iter().map(|dir| format!("{dir}/")).collect();
    let Some((last, rest)) = named.split_last() else {
        return String::new();
    };
    if rest.is_empty() {
        return format!("; {last} is on the `components` line");
    }
    format!(
        "; {} and {last} are on the `components` line",
        rest.join(", ")
    )
}

/// The `project-files` line for a directory on disk.
pub fn files_check(dir: &Path, components: &Components) -> Check {
    let wanted = project_files(components);
    let missing: Vec<String> = wanted
        .iter()
        .filter(|name| !dir.join(name).is_file())
        .cloned()
        .collect();
    // Only the enabled ones: the `components` line reports a component that is
    // on, so pointing at it for a directory left behind by one that is off would
    // send the reader to a line that says nothing about it.
    let elsewhere: Vec<&str> = Component::ALL
        .iter()
        .filter(|component| components.is_enabled(**component))
        .filter_map(|component| component.dir())
        .collect();
    Check::from_verdict(
        "project-files",
        "deployment files",
        files_verdict(wanted.len(), &missing, &elsewhere),
    )
}

/// The compose project name, and whether it is this deployment's own.
///
/// Compose names a project after its directory unless a file says otherwise,
/// so two deployments in directories both called `demo` share every container
/// name and every named volume - a fresh `chap-devops up` in the second one finds
/// the first one's database, with a password it has never seen. `chap-devops init`
/// records a name of its own; an empty one is a hand edit, and this warns
/// about it.
///
/// `others` are the other directories that record the same name, as a
/// restore with `--adopt-identity` on the same machine leaves them: they use
/// the same containers and volumes.
pub fn project_name_verdict(
    recorded: Option<&str>,
    others: &[PathBuf],
) -> (Status, String, Option<String>) {
    if let (Some(name), Some(first)) = (recorded, others.first()) {
        let dirs: Vec<String> = others.iter().map(|d| d.display().to_string()).collect();
        return (
            Status::Warn,
            format!(
                "{name}, and {} {} the same name, so these directories use the same \
                 containers and volumes",
                dirs.join(", "),
                if dirs.len() == 1 { "records" } else { "record" }
            ),
            Some(format!(
                "use only one of these directories, or remove {}",
                first.display()
            )),
        );
    }
    if let Some(name) = recorded {
        return (
            Status::Ok,
            format!("{name} (recorded in {CHAP_DEVOPS_DIR}/{PROJECT_FILE})"),
            None,
        );
    }
    (
        Status::Warn,
        format!(
            "`compose_project` is empty in {CHAP_DEVOPS_DIR}/{PROJECT_FILE}; compose names the \
             deployment after its directory"
        ),
        Some(format!(
            "set `compose_project:` in {CHAP_DEVOPS_DIR}/{PROJECT_FILE}, then run `chap-devops sync`"
        )),
    )
}

/// The `project` line. `daemon` is whether docker may be asked which
/// deployments it knows.
pub fn project_check(project: &Project, daemon: bool) -> Check {
    let others = match project.compose_project() {
        Some(name) => crate::known::claimed_elsewhere(name, &project.dir, &|| {
            daemon.then(docker::compose_ls_json).flatten()
        }),
        None => Vec::new(),
    };
    Check::from_verdict(
        "compose-project",
        "compose project",
        project_name_verdict(project.compose_project(), &others),
    )
}

/// Whether the named volumes this deployment would use are its own.
///
/// `created` is when `.chap-devops/project.yaml` was created and each volume's is
/// when docker created it. A database volume older than the deployment
/// directory itself was made by something else - an earlier deployment of the
/// same name, most often - and it still holds that deployment's role password,
/// which is exactly the state in which chap-core cannot log in.
///
/// Both times are optional: a filesystem that does not record a creation time,
/// and a docker that did not say, each cost the comparison and nothing else.
///
/// `ocs_data` is the OCS data volume and what it holds, when that could be
/// measured: the one volume under this prefix whose size is usually worth
/// knowing, because it is what a `chap-devops down --volumes` would destroy.
pub fn volume_verdict(
    prefix: &str,
    db_volume: &str,
    volumes: &[(String, Option<u64>)],
    created: Option<u64>,
    leftover: &[String],
    ocs_data: Option<(&str, u64)>,
) -> (Status, String, Option<String>) {
    if volumes.is_empty() {
        return (
            Status::Ok,
            format!("no {prefix}* volume yet; `chap-devops up` creates them"),
            None,
        );
    }
    let held = match ocs_data {
        Some((name, bytes)) => format!("; {name} holds {}", crate::backup::human_size(bytes)),
        None => String::new(),
    };
    let count = format!(
        "{} volume{} named {prefix}*{held}",
        volumes.len(),
        if volumes.len() == 1 { "" } else { "s" }
    );
    let older = volumes.iter().find(|(name, at)| {
        name == db_volume && matches!((at, created), (Some(a), Some(c)) if *a < c)
    });
    // The database one first: a deployment that cannot log in is a stack that
    // does not come up, where a volume nobody reads any more costs disk.
    if let Some((name, at)) = older {
        return (
            Status::Warn,
            format!(
                "the database volume {name} predates this deployment; if chap-core cannot log \
                 in, it belongs to an earlier deployment with the same name (volume {}, \
                 deployment {})",
                crate::backup::timestamp(at.unwrap_or_default()),
                crate::backup::timestamp(created.unwrap_or_default())
            ),
            Some(
                "remove it with `chap-devops down --volumes` if this deployment's data can go, \
                 or keep both by giving one of them a name of its own"
                    .to_string(),
            ),
        );
    }
    if !leftover.is_empty() {
        return (
            Status::Warn,
            format!(
                "leftover volumes from disabled models or components: {}{}",
                leftover.join(", "),
                // The OCS size only when that volume is one of them: after
                // this list, the size of one still in use reads as another
                // leftover.
                match ocs_data {
                    Some((name, _)) if leftover.iter().any(|l| l == name) => held.as_str(),
                    _ => "",
                }
            ),
            Some(LEFTOVER_FIX.to_string()),
        );
    }
    (Status::Ok, count, None)
}

/// What to do about the leftover volumes the check found.
///
/// `down --volumes` is not among the answers on purpose: it only removes the
/// volumes the compose files still declare, which is exactly the set these are
/// not in.
const LEFTOVER_FIX: &str = "remove each with `chap-devops models disable <id> --purge` or `chap-devops components disable <name> \
     --purge`, or `docker volume rm <name>`; keep them to have the data back when the model or \
     component is enabled again";

/// The volumes under this deployment's prefix that belong to nothing it still
/// enables.
///
/// A model's volume is `ck_<id>_data`, so a name of that shape whose id is no
/// longer in `.chap-devops/models.yaml` is a disabled model's data; the component
/// ones are named outright, and are leftovers while the component is off.
/// Every other name - the database, chap-core's own - belongs to the base
/// stack, which is nobody's leftover.
///
/// A component is asked whether the volume is among the ones it keeps, not
/// whether it is *the* one: a component with two volumes would otherwise have
/// the second reported as a leftover while the component is enabled, which is
/// an invitation to delete live data.
///
/// Pure, and injected with both lists: the verdict is decided here and the
/// docker call that gathers the names is [`volumes_check`]'s.
pub fn leftover_volumes(
    prefix: &str,
    volumes: &[String],
    models: &[String],
    components: &Components,
) -> Vec<String> {
    volumes
        .iter()
        .filter(|name| {
            let Some(bare) = name.strip_prefix(prefix) else {
                return false;
            };
            if let Some(id) = crate::compose::volume_model_id(bare) {
                return !models.iter().any(|enabled| enabled == id);
            }
            Component::ALL.iter().any(|component| {
                component.volumes().contains(&bare) && !components.is_enabled(*component)
            })
        })
        .cloned()
        .collect()
}

/// The `volumes` line: what docker holds under this deployment's name.
///
/// `running` decides whether the OCS data volume is measured here: while its
/// container is up, the `components` line has the size from inside it, and
/// starting a second container to measure what the first is writing to would
/// be both slower and less true. While it is down, this is the line that says
/// how much data a `chap-devops down --volumes` would destroy.
pub(super) fn volumes_check(project: &Project, running: &BTreeSet<String>) -> Check {
    const ID: &str = "volumes";
    const NAME: &str = "volumes";
    let Some(prefix) = project.volume_prefix() else {
        return Check::skip(ID, NAME, "this directory has no compose project name");
    };
    let volumes: Vec<(String, Option<u64>)> = docker::volumes_with_prefix(&prefix)
        .into_iter()
        .map(|v| (v.name, crate::status::parse_rfc3339(&v.created_at)))
        .collect();
    let created = compared_creation(project);
    let names: Vec<String> = volumes.iter().map(|(name, _)| name.clone()).collect();
    let models: Vec<String> = project.state.models.keys().cloned().collect();
    let leftover = leftover_volumes(&prefix, &names, &models, &project.state.components);
    let ocs_volume = format!("{prefix}{}", crate::compose::render::OCS_VOLUME);
    let ocs_data = (!running.contains(crate::compose::OCS_SERVICE) && names.contains(&ocs_volume))
        .then(|| docker::volume_size_bytes(&ocs_volume))
        .flatten()
        .map(|bytes| (ocs_volume.as_str(), bytes));
    Check::from_verdict(
        ID,
        NAME,
        volume_verdict(
            &prefix,
            &format!("{prefix}chap-db"),
            &volumes,
            created,
            &leftover,
            ocs_data,
        ),
    )
}

/// When this deployment directory was created, for the comparison with the
/// database volume. `None` after a takeover with `--adopt-identity`: the
/// volumes are then older than this directory on purpose.
pub(super) fn compared_creation(project: &Project) -> Option<u64> {
    match project.state.adopted_identity {
        true => None,
        false => deployment_created_at(project),
    }
}

/// When this deployment directory was created, in Unix seconds.
///
/// The `.chap-devops/` directory rather than `project.yaml` inside it: every save
/// writes that file to a temporary sibling and renames it into place, so its
/// creation time is the time of the last `chap-devops sync` and not the time the
/// deployment was made. The directory is created once by `chap-devops init` and
/// never replaced.
///
/// `None` where the filesystem records no creation time, and the check that
/// uses it simply does not make the comparison.
fn deployment_created_at(project: &Project) -> Option<u64> {
    let chap_devops = project.chap_devops_dir();
    file_created_at(&chap_devops).or_else(|| file_created_at(&chap_devops.join(PROJECT_FILE)))
}

/// When a file or directory was created, in Unix seconds, when the filesystem
/// records it.
fn file_created_at(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .and_then(|m| m.created())
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// What `.env` says about the two things that bite later.
///
/// The default database password is the one a deployment reachable from
/// anywhere but the loopback should not keep, and a missing `CHAP_IMAGE_TAG`
/// line means the pin comments `sync` writes are gone. Whether authentication
/// is on is reported either way: "off" is a fact an operator has to be able to
/// see, not a fault.
pub fn env_verdict(body: Option<&str>) -> (Status, String, Option<String>) {
    let Some(body) = body else {
        return (Status::Skip, "no .env to read".to_string(), None);
    };
    let auth_on = auth::active_value(body, auth::API_TOKEN_ENV_VAR).is_some();
    let password = auth::active_value(body, "POSTGRES_PASSWORD");
    let tag_line = body.lines().any(|line| {
        line.trim_start()
            .trim_start_matches('#')
            .trim_start()
            .starts_with(&format!("{CHAP_TAG_ENV_VAR}="))
    });

    let mut facts = vec![format!("auth {}", if auth_on { "on" } else { "off" })];
    let mut fixes = Vec::new();
    match password.as_deref() {
        None => {
            facts.push("POSTGRES_PASSWORD unset, so compose uses the default `chap`".to_string());
            fixes.push(PASSWORD_FIX);
        }
        Some("chap") => {
            facts.push("POSTGRES_PASSWORD is the default `chap`".to_string());
            fixes.push(PASSWORD_FIX);
        }
        Some(_) => facts.push("POSTGRES_PASSWORD set".to_string()),
    }
    if tag_line {
        facts.push(format!("{CHAP_TAG_ENV_VAR} present"));
    } else {
        facts.push(format!("no {CHAP_TAG_ENV_VAR} line"));
        fixes.push("run `chap-devops sync` to write the image pin comments back");
    }

    let detail = facts.join(", ");
    if fixes.is_empty() {
        return (Status::Ok, detail, None);
    }
    (Status::Warn, detail, Some(fixes.join("; ")))
}

/// What to do about a database password anyone can guess.
const PASSWORD_FIX: &str = "set POSTGRES_PASSWORD in .env to a value of your own; a database volume that already \
     exists keeps the old password until `ALTER USER` changes it";

/// The `.env` line.
pub fn env_check(body: Option<&str>) -> Check {
    Check::from_verdict("env", ".env", env_verdict(body))
}

/// Whether chap-core can run the number of API workers `.env` asks for.
///
/// More than one worker needs chap-core newer than v2.7.0: older versions give
/// the workers the database connections of the gunicorn main process, and
/// requests fail just after a start (CLIM-1808). The same release added
/// `WEB_CONCURRENCY` to chap-core's own compose file (CLIM-1809), so the base
/// `compose.yml` of the deployment tells the two apart without the network,
/// for a moving tag such as `latest` too.
pub fn api_workers_verdict(
    env: Option<&str>,
    base_compose: Option<&str>,
) -> (Status, String, Option<String>) {
    let value = env.and_then(|body| crate::dotenv::non_empty(body, API_WORKERS_ENV_VAR));
    let Some(value) = value else {
        return (
            Status::Ok,
            format!("1 ({API_WORKERS_ENV_VAR} not set)"),
            None,
        );
    };
    let workers = match value.parse::<u32>() {
        Ok(n) if n >= 1 => n,
        _ => {
            return (
                Status::Warn,
                format!("{API_WORKERS_ENV_VAR}={value} is not a number of 1 or more"),
                Some(format!(
                    "set {API_WORKERS_ENV_VAR} in `.env` to a whole number such as 2, then run `chap-devops up`"
                )),
            );
        }
    };
    let supported = base_compose.is_none_or(|text| text.contains("WEB_CONCURRENCY"));
    if workers == 1 || supported {
        return (Status::Ok, workers.to_string(), None);
    }
    (
        Status::Warn,
        format!(
            "{workers}, but this chap-core is v2.7.0 or older, so the workers share database connections"
        ),
        Some(format!(
            "set {API_WORKERS_ENV_VAR}=1 in `.env`, or move to a newer chap-core with \
             `chap-devops update --chap-tag master`; then run `chap-devops up`"
        )),
    )
}

/// The API workers line.
pub fn api_workers_check(env: Option<&str>, base_compose: Option<&str>) -> Check {
    Check::from_verdict(
        "api-workers",
        "api workers",
        api_workers_verdict(env, base_compose),
    )
}

/// Whether one host port the stack publishes is free to publish on.
///
/// `busy` is injected so the verdict can be tested without binding anything,
/// and the fix is the very sentence `chap-devops up`'s preflight would have failed
/// with, so the two never drift apart.
///
/// `note` is appended in parentheses to whatever the line says about the port:
/// [`api_port_note`] uses it to name the file that moved the API port, without
/// which the number looks wrong against `.chap-devops/project.yaml`.
pub fn port_check(
    claim: &PortClaim,
    own: &ports::OwnPorts,
    busy: &dyn Fn(u16) -> bool,
    suggestion: Option<u16>,
    note: Option<&str>,
) -> Check {
    let is_api = claim.service == API_SERVICE;
    let id = if is_api {
        "api-port".to_string()
    } else {
        format!("port-{}", claim.service)
    };
    let name = if is_api {
        "api port".to_string()
    } else {
        format!("port {}", claim.service)
    };
    let detail = |what: &str| match note {
        Some(note) => format!("{} {what} ({note})", claim.port),
        None => format!("{} {what}", claim.port),
    };
    if own.holds(claim) {
        return Check::ok(
            id,
            name,
            detail("is held by this deployment's own container"),
        );
    }
    if busy(claim.port) {
        return Check::fail(
            id,
            name,
            detail("is in use by something else"),
            ports::busy_line(claim, suggestion),
        );
    }
    Check::ok(id, name, detail("is free"))
}

/// What the `api port` line says about where its number came from, when that
/// is not what `.chap-devops/project.yaml` records.
///
/// compose reads `.env` last, so a `CHAP_API_PORT=` line there moves the
/// published port. The checklist has to name the file that did it: `18000 is
/// free` next to a recorded `api_port: 8000` otherwise reads as a bug in
/// `chap-devops` rather than as a deliberate override. The ordinary case, where the
/// two agree, says nothing.
pub fn api_port_note(project: &Project) -> Option<String> {
    let (port, source) = project.api_port_in_effect();
    (source == ApiPortSource::Env && port != project.state.api_port).then(|| {
        format!(
            "{}, over the {} recorded in .chap-devops/project.yaml",
            source.label(),
            project.state.api_port
        )
    })
}

#[cfg(test)]
mod tests;
