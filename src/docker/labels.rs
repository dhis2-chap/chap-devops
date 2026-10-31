//! The labels chap-devops puts on every container it starts, and the one query that
//! finds all of them on a machine.

use super::docker_capture;
use std::collections::BTreeMap;

/// What the container is: `chap-core`, `model`, `ocs`, `s3` or `dhis2`.
pub const ROLE_LABEL: &str = "io.github.dhis2-chap.chap-devops.role";
/// The marketplace id of a model service, and of its init container.
pub const MODEL_LABEL: &str = "io.github.dhis2-chap.chap-devops.model";
/// `run` for a `chap-devops run` group, `init` for a deployment of its own, and
/// `cli` for the one-shot container `chap-devops chap` runs.
pub const KIND_LABEL: &str = "io.github.dhis2-chap.chap-devops.kind";
/// The group of a `chap-devops run` deployment.
pub const GROUP_LABEL: &str = "io.github.dhis2-chap.chap-devops.group";

/// Compose's own labels, which say which deployment and service it is.
pub const COMPOSE_PROJECT_LABEL: &str = "com.docker.compose.project";
pub const COMPOSE_SERVICE_LABEL: &str = "com.docker.compose.service";
pub const COMPOSE_DIR_LABEL: &str = "com.docker.compose.project.working_dir";

/// One container `docker ps -a` lists, with its labels read apart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labeled {
    pub id: String,
    pub name: String,
    /// `running`, `exited`, ...
    pub state: String,
    /// docker's own `Up 5 minutes (healthy)`, which is where health shows.
    pub status: String,
    pub labels: BTreeMap<String, String>,
}

impl Labeled {
    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels.get(key).map(String::as_str)
    }

    /// `healthy`, `unhealthy` or `starting` when the image has a healthcheck.
    pub fn health(&self) -> Option<&'static str> {
        ["healthy", "unhealthy", "health: starting"]
            .into_iter()
            .find(|h| self.status.contains(&format!("({h})")))
            .map(|h| h.trim_start_matches("health: "))
    }
}

/// Every container on this machine that carries [`ROLE_LABEL`], running or
/// not. Empty when docker cannot be asked.
pub fn chap_devops_containers() -> Vec<Labeled> {
    let args = [
        "ps",
        "-a",
        "--filter",
        &format!("label={ROLE_LABEL}"),
        "--format",
        "{{json .}}",
    ]
    .map(str::to_string);
    docker_capture(&args)
        .map(|text| parse_labeled(&text))
        .unwrap_or_default()
}

/// One JSON object per line, as `--format '{{json .}}'` writes them.
pub(super) fn parse_labeled(text: &str) -> Vec<Labeled> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .map(|row| {
            let field = |key: &str| {
                row.get(key)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            Labeled {
                id: field("ID"),
                name: field("Names"),
                state: field("State"),
                status: field("Status"),
                labels: parse_label_list(&field("Labels")),
            }
        })
        .collect()
}

/// docker's `k=v,k=v` label list. A value may hold commas of its own - the
/// compose config file list does - so a piece that does not start with a label
/// key and `=` belongs to the value before it. A key may hold `/`, as Docker
/// Desktop's `desktop.docker.io/ports/8000/tcp` does, but never starts with
/// one, which is what tells a config file path from a key.
pub(super) fn parse_label_list(text: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut last: Option<String> = None;
    for piece in text.split(',') {
        match piece.split_once('=') {
            Some((key, value)) if is_label_key(key) => {
                out.insert(key.to_string(), value.to_string());
                last = Some(key.to_string());
            }
            _ => {
                if let Some(key) = &last
                    && let Some(value) = out.get_mut(key)
                {
                    value.push(',');
                    value.push_str(piece);
                }
            }
        }
    }
    out
}

/// Whether `key` can be a label key: starts with a letter or digit and holds
/// only letters, digits, `.`, `-`, `_` and `/`.
fn is_label_key(key: &str) -> bool {
    key.starts_with(|c: char| c.is_ascii_alphanumeric())
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-_/".contains(c))
}
