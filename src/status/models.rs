//! The model rows: what each enabled model is to chap-core, or to its own
//! `/health` on a deployment without chap-core.

use super::time::ago;
use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Where one model row stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelState {
    /// chap-core knows it: the model works.
    Registered,
    /// Its container is up but chap-core never heard from it. chapkit gives
    /// up registering five attempts into its startup, so a model that came up
    /// before chap-core was healthy stays invisible until it is restarted.
    RunningNotRegistered,
    /// No container, so nothing could have registered.
    NotRunning,
    /// Its container is paused, as an interrupted `chap-devops backup create` can
    /// leave it: it is there and does not answer.
    Paused,
    /// Registered with chap-core, but not a model this project enables.
    Unmanaged,
    /// Without chap-core: its container is up and its own `/health` answered
    /// on its host port.
    Up,
    /// Without chap-core: its container is up and its `/health` did not
    /// answer, which is a service still starting or one that failed to.
    RunningNotAnswering,
    /// chap-core knows it, and cannot reach it: its proxy to the model's own
    /// `/health` failed. The usual cause is an address that works from where
    /// the model registered and not from where chap-core runs, such as
    /// `localhost` seen from inside chap-core's container.
    Unreachable,
    /// chap-core knows it, and has no configured model of the version it
    /// registered with, so nothing can run it and the Modeling App does not
    /// list it. chap-core 2.4 and later makes none from a registration;
    /// `chap-devops models configs sync` makes them.
    NotConfigured,
}

impl ModelState {
    /// The STATE cell.
    pub fn label(self) -> &'static str {
        match self {
            ModelState::Registered => "registered",
            ModelState::RunningNotRegistered => "running, not registered",
            ModelState::NotRunning => "not running",
            ModelState::Paused => "paused",
            ModelState::Unmanaged => "unmanaged",
            ModelState::Up => "up",
            ModelState::RunningNotAnswering => "running, not answering",
            ModelState::Unreachable => "registered, unreachable",
            ModelState::NotConfigured => "registered, not configured",
        }
    }

    /// Whether this row is something to do about.
    pub fn is_problem(self) -> bool {
        matches!(
            self,
            ModelState::RunningNotRegistered
                | ModelState::NotRunning
                | ModelState::Paused
                | ModelState::RunningNotAnswering
                | ModelState::Unreachable
        )
    }
}

/// One row of the model table.
#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    /// Compose service name, which is also the id the model registers with.
    pub id: String,
    pub state: ModelState,
    /// Where a human reaches it: a host port, `internal`, or - for an
    /// unmanaged service - the URL chap-core has for it.
    pub reach: String,
    /// The host port itself, for the table, which says `port 5010` where
    /// `--json` says the whole URL. Not serialised: [`ModelStatus::reach`] is
    /// what a script reads, and it has not changed.
    #[serde(skip)]
    pub host_port: Option<u16>,
    /// How long ago chap-core last heard from it, `None` when never.
    pub last_ping: Option<String>,
    /// For a model that is running and not registered: the id its own
    /// container did register under, when an unmanaged row turned out to be
    /// it. That is a service id that does not match, not a model that failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registered_as: Option<String>,
    /// Whether its container has been up for under two minutes, which makes
    /// "not registered yet" a wait rather than a fault. Filled in by the
    /// caller, which has docker.
    #[serde(skip)]
    pub young: bool,
    /// For a model added with `chap-devops models add`: its model id and the source
    /// it was added from, so a hint can spell out removing and adding it again.
    #[serde(skip)]
    pub added_from: Option<(String, String)>,
    /// For an [`ModelState::Unreachable`] row: the URL it registered under
    /// and what chap-core's proxy answered, for the hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unreachable: Option<Unreachable>,
}

/// Why chap-core could not reach a model it has registered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unreachable {
    /// The URL the model registered under, which is where chap-core calls it.
    pub registered_url: String,
    /// What the proxy request came back with: `HTTP 502`, or the error.
    pub answer: String,
}

/// The path, under chap-core, that proxies to a registered model's `/health`.
pub fn proxied_health_path(service_id: &str) -> String {
    format!("/v2/services/{service_id}/run{MODEL_HEALTH_PATH}")
}

/// Turn every registered row chap-core cannot reach into
/// [`ModelState::Unreachable`].
///
/// A registration is a heartbeat the model sends; it says nothing about the
/// way back. `probe` asks chap-core's proxy for the model's own `/health` and
/// answers `Some(what came back)` when that failed in a way that means
/// chap-core could not get to the model (a 5xx, or no answer), and `None`
/// otherwise - a 404 included, since a chap-core without the proxy route
/// cannot say either way.
pub fn mark_unreachable(
    rows: &mut [ModelStatus],
    registered: &[RegisteredService],
    probe: &dyn Fn(&str) -> Option<String>,
) {
    for row in rows
        .iter_mut()
        .filter(|row| row.state == ModelState::Registered)
    {
        if let Some(answer) = probe(&row.id) {
            row.state = ModelState::Unreachable;
            row.unreachable = Some(Unreachable {
                registered_url: registered
                    .iter()
                    .find(|s| s.id == row.id)
                    .map(|s| s.url.clone())
                    .unwrap_or_default(),
                answer,
            });
        }
    }
}

/// Turn every registered row that chap-core has no configured model for into
/// [`ModelState::NotConfigured`].
///
/// `configured` is chap-core's listing of configured models, which a
/// chap-core that could not be asked does not have: the caller then does not
/// call this, and the rows stay as they are. The rule is
/// [`crate::configs::sync::is_configured`], with the version the model
/// registered with.
pub fn mark_unconfigured(
    rows: &mut [ModelStatus],
    registered: &[RegisteredService],
    configured: &[crate::modeltest::ConfiguredModel],
) {
    for row in rows
        .iter_mut()
        .filter(|row| row.state == ModelState::Registered)
    {
        let version = registered
            .iter()
            .find(|s| s.id == row.id)
            .map(|s| s.version.as_str());
        if !crate::configs::sync::is_configured(configured, &row.id, version) {
            row.state = ModelState::NotConfigured;
        }
    }
}

/// Tie each unmanaged row to the model whose container it is, when it is one.
///
/// chap-core records the URL a service registered from, and a container's
/// hostname is the start of its id - so `http://ee5e63ab53bd:8000` is the
/// container `ee5e63ab53bd...`. When that container is one of this
/// deployment's models and that model is `running, not registered`, the
/// model did register, under an id of its own: the fix is `--service-id`, and
/// a restart would change nothing. `containers` is `(service, id)`.
pub fn link_strays(rows: &mut [ModelStatus], containers: &[(String, String)]) {
    let strays: Vec<(String, String)> = rows
        .iter()
        .filter(|row| row.state == ModelState::Unmanaged)
        .filter_map(|row| Some((row.id.clone(), url_host(&row.reach)?)))
        .collect();
    for (stray, host) in strays {
        // Twelve hex digits is how docker names a container's host; anything
        // shorter would match too much.
        if host.len() < 12 || !host.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let Some((service, _)) = containers.iter().find(|(_, id)| id.starts_with(&host)) else {
            continue;
        };
        if let Some(row) = rows
            .iter_mut()
            .find(|row| &row.id == service && row.state == ModelState::RunningNotRegistered)
        {
            row.registered_as = Some(stray);
        }
    }
}

/// `ee5e63ab53bd` out of `http://ee5e63ab53bd:8000/`.
fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', ':']).next()?;
    (!host.is_empty()).then(|| host.to_string())
}

/// One entry of `GET /v2/services`.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisteredService {
    pub id: String,
    pub url: String,
    pub display_name: String,
    pub version: String,
    pub last_ping_at: String,
    pub expires_at: String,
    /// The `git_revision` of its `info`: `Some(None)` when chap-core sent the
    /// key with no value, and `None` when it sent no key at all. A chap-core
    /// before 2.4 sends no key. chap-core 2.4 sends what the service sent,
    /// and chapkit 2.1 sends no key when it has no revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_revision: Option<Option<String>>,
}

impl RegisteredService {
    /// Whether it reports no git revision: a `null`, or no key from a
    /// chap-core that passes the key on (see [`sends_revision`]).
    pub fn reports_no_revision(&self, sends_revision: bool) -> bool {
        match &self.git_revision {
            Some(None) => true,
            Some(Some(_)) => false,
            None => sends_revision,
        }
    }
}

/// Whether this chap-core passes on the `git_revision` that a service sends,
/// so that a missing key means the service sent none.
///
/// chap-core 2.4 does, and so does a chap-core that sent the key for another
/// service. `version` is what chap-core reports, when it is known.
pub fn sends_revision(registered: &[RegisteredService], version: Option<&str>) -> bool {
    registered.iter().any(|s| s.git_revision.is_some())
        || version
            .and_then(crate::chapcore::release_version)
            .is_some_and(|version| version >= (2, 4, 0))
}

/// Why chap-core cannot run a model registered from outside the deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RevisionProblem {
    /// It reports no git revision, so chap-core stores no template for it.
    NoRevision,
    /// chap-core stores its template under this version from another git
    /// revision.
    RevisionMismatch,
    /// chap-core has configured models of it of other versions only, so a
    /// backtest of the version it registered with has nothing to run.
    OtherVersionOnly,
    /// A model of this deployment registered a `git_revision` that is not
    /// the commit its `sha-` image tag names.
    NotThePin { revision: String, image_tag: String },
}

/// One model with a [`RevisionProblem`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RevisionWarning {
    pub id: String,
    pub version: String,
    pub problem: RevisionProblem,
    /// Whether this deployment runs the model, which changes the way out:
    /// its image is built again, not started somewhere else.
    pub managed: bool,
}

/// The unmanaged rows that chap-core cannot run, as far as chap-devops can see it:
/// a registration with an empty `git_revision`, a configured model whose
/// `healthStatus` is `revision_mismatch`, or configured models of other
/// versions only.
///
/// The unmanaged rows, and a model of this deployment that chap-core has no
/// configured model for because it reports no revision: `chap-devops models
/// configs sync` cannot fix that one, so its row gets this way out instead. A
/// template stored from another revision with no configured model is not
/// seen, because only the template listing says that, and that listing
/// changes chap-core when it is read.
///
/// `sends_revision` is [`sends_revision`]: whether a missing `git_revision`
/// key is a model that reports none.
pub fn revision_warnings(
    rows: &[ModelStatus],
    registered: &[RegisteredService],
    configured: &[crate::modeltest::ConfiguredModel],
    sends_revision: bool,
) -> Vec<RevisionWarning> {
    rows.iter()
        .filter(|row| matches!(row.state, ModelState::Unmanaged | ModelState::NotConfigured))
        .filter_map(|row| {
            let service = registered.iter().find(|s| s.id == row.id)?;
            let prefix = format!("{}:", row.id);
            let managed = row.state == ModelState::NotConfigured;
            let problem = if service.reports_no_revision(sends_revision) {
                RevisionProblem::NoRevision
            } else if managed {
                return None;
            } else if configured.iter().any(|model| {
                !model.archived
                    && (model.name == row.id || model.name.starts_with(&prefix))
                    && model.health.as_deref() == Some("revision_mismatch")
            }) {
                RevisionProblem::RevisionMismatch
            } else if crate::configs::sync::is_configured(configured, &row.id, None)
                && !crate::configs::sync::is_configured(configured, &row.id, Some(&service.version))
            {
                RevisionProblem::OtherVersionOnly
            } else {
                return None;
            };
            Some(RevisionWarning {
                id: row.id.clone(),
                version: service.version.clone(),
                problem,
                managed,
            })
        })
        .collect()
}
/// The models of this deployment that registered a `git_revision` that is
/// not the commit of their pin, as `chap-admin` checks after an install. Only
/// a `sha-` image tag names a commit, so another tag is not checked.
pub fn pin_warnings<'a>(
    models: impl IntoIterator<Item = &'a crate::project::EnabledModel>,
    registered: &[RegisteredService],
) -> Vec<RevisionWarning> {
    models
        .into_iter()
        .filter_map(|model| {
            let prefix = model.image_tag.strip_prefix("sha-")?;
            let service = registered.iter().find(|s| s.id == model.service_id)?;
            let revision = service.git_revision.clone().flatten()?;
            if revision.starts_with(prefix) || prefix.starts_with(revision.as_str()) {
                return None;
            }
            Some(RevisionWarning {
                id: model.service_id.clone(),
                version: service.version.clone(),
                problem: RevisionProblem::NotThePin {
                    revision,
                    image_tag: model.image_tag.clone(),
                },
                managed: true,
            })
        })
        .collect()
}

/// Every model this project enables, as `(service id, host port)`, in the
/// order `models.yaml` records them.
pub fn enabled_models(project: &Project) -> Vec<(String, Option<u16>)> {
    project
        .state
        .models
        .values()
        .map(|m| (m.service_id.clone(), m.host_port))
        .collect()
}

/// Build the table: one row per enabled model, then every registered service
/// the project does not know as `unmanaged`.
///
/// `now` is Unix seconds, passed in so the relative times are testable.
pub fn model_rows(
    enabled: &[(String, Option<u16>)],
    registered: &[RegisteredService],
    running: &BTreeSet<String>,
    now: u64,
) -> Vec<ModelStatus> {
    let mut rows: Vec<ModelStatus> = enabled
        .iter()
        .map(|(id, host_port)| {
            let found = registered.iter().find(|s| &s.id == id);
            let state = match (found.is_some(), running.contains(id)) {
                (true, _) => ModelState::Registered,
                (false, true) => ModelState::RunningNotRegistered,
                (false, false) => ModelState::NotRunning,
            };
            ModelStatus {
                id: id.clone(),
                state,
                reach: match host_port {
                    Some(port) => format!("http://localhost:{port}"),
                    None => "internal".to_string(),
                },
                host_port: *host_port,
                last_ping: found.and_then(|s| ago(now, &s.last_ping_at)),
                registered_as: None,
                young: false,
                added_from: None,
                unreachable: None,
            }
        })
        .collect();

    let mut strangers: Vec<&RegisteredService> = registered
        .iter()
        .filter(|s| !enabled.iter().any(|(id, _)| id == &s.id))
        .collect();
    strangers.sort_by(|a, b| a.id.cmp(&b.id));
    rows.extend(strangers.into_iter().map(|s| ModelStatus {
        id: s.id.clone(),
        state: ModelState::Unmanaged,
        // Whatever chap-core has for it: an unmanaged service is not ours to
        // describe, and its URL is the only handle anyone has on it.
        reach: s.url.clone(),
        // Not this deployment's port to know.
        host_port: None,
        last_ping: ago(now, &s.last_ping_at),
        registered_as: None,
        young: false,
        added_from: None,
        unreachable: None,
    }));
    rows
}

/// The path a chapkit model service answers its own health on.
pub const MODEL_HEALTH_PATH: &str = "/health";

/// The model table for a deployment without chap-core: one row per enabled
/// model, judged by its container and then by its own `/health`.
///
/// `answers` is asked only about a model whose container is up and that has a
/// host port, so a stopped deployment costs no timeouts. A model with no host
/// port (one enabled with `--port none`) cannot be asked from out here, so its
/// container is the whole answer.
pub fn standalone_model_rows(
    enabled: &[(String, Option<u16>)],
    running: &BTreeSet<String>,
    answers: &dyn Fn(u16) -> bool,
) -> Vec<ModelStatus> {
    enabled
        .iter()
        .map(|(id, host_port)| {
            let state = match (running.contains(id), host_port) {
                (false, _) => ModelState::NotRunning,
                (true, Some(port)) if !answers(*port) => ModelState::RunningNotAnswering,
                (true, _) => ModelState::Up,
            };
            ModelStatus {
                id: id.clone(),
                state,
                reach: match host_port {
                    Some(port) => format!("http://localhost:{port}"),
                    None => "internal".to_string(),
                },
                host_port: *host_port,
                last_ping: None,
                registered_as: None,
                young: false,
                added_from: None,
                unreachable: None,
            }
        })
        .collect()
}
/// Where a human reaches one model service from this machine.
///
/// A published host port is the direct answer; without one the way in is
/// chap-core's read-only proxy, which reaches a registered service over the
/// compose network without any port of its own.
pub fn reach(project: &Project, host_port: Option<u16>, service_id: &str) -> String {
    match host_port {
        Some(port) => format!("http://localhost:{port}"),
        None => format!("internal (proxy: {})", project.proxy_url(service_id)),
    }
}

/// Expected service ids that nothing has registered.
pub fn missing_ids(expected: &[String], registered: &[RegisteredService]) -> Vec<String> {
    expected
        .iter()
        .filter(|id| !registered.iter().any(|s| &&s.id == id))
        .cloned()
        .collect()
}
