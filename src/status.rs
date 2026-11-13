//! `chap-devops status`: chap-core health plus the services it has registered.
//!
//! Lenient about the fields chap-core sends - it may add fields, rename
//! optional ones or leave one empty, and none of that should turn
//! `chap-devops status` into a crash - and strict about who is answering. A 200
//! from something that is not chap-core is reported as down: `up` has to mean
//! that the deployment works, not that the port is taken.

mod components;
mod lines;
mod models;
mod probe;
mod time;

pub use components::{ComponentState, ComponentStatus, ocs_datasets};
pub use lines::{
    EMPTY, EXTERNAL_REGISTRATION_LOG, NOTHING_RUNNING, TEST_HINT, closing_line,
    external_registration_hints, hints, revision_line, standalone_closing_lines, standalone_hints,
    test_hint,
};
pub use models::{
    MODEL_HEALTH_PATH, ModelState, ModelStatus, RegisteredService, RevisionProblem,
    RevisionWarning, enabled_models, link_strays, mark_unconfigured, mark_unreachable, missing_ids,
    model_rows, pin_warnings, proxied_health_path, reach, revision_warnings, sends_revision,
    standalone_model_rows,
};
pub use probe::{
    body_description, parse_health, parse_services, services_are_not_chap_core, token_rejected,
};
pub use time::{parse_rfc3339, parse_rfc3339_precise};

use components::component_rows;
pub use components::{mark_paused, mark_unhealthy, paused_line};
use probe::{Failure, agent, get, version_of};
use time::now;

use crate::project::{ApiPortSource, Project};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

/// Path of the chap-core health endpoint.
pub const HEALTH_PATH: &str = "/health";
/// Path of the service registry chapkit models register themselves with.
pub const SERVICES_PATH: &str = "/v2/services";
/// Path of chap-core's own version. It is not required: a chap-core that does
/// not answer it is still up, and the version falls back to the tag the
/// project pins.
pub const INFO_PATH: &str = "/system/info";

/// Path of the OCS dataset list. `f=json` because the same path serves the
/// landing page as HTML, and the landing page is not something to count.
pub const DATASETS_PATH: &str = "/datasets?f=json";

/// The one route DHIS2 answers without credentials: `PingController`, which
/// `DhisWebApiWebSecurityConfig` permits. So it can be asked of an instance
/// this CLI holds no login for, which is every instance.
///
/// It is also the route `compose.dhis2.yml` builds the container's own
/// healthcheck out of, and judged the same way: the status code, not the body.
pub const DHIS2_PING_PATH: &str = "/api/ping";

/// How long the OCS dataset count may take.
///
/// Its own bound rather than `--timeout`: this is one extra fact on a line
/// that is already complete without it, so it gets a short leash and gives up
/// silently.
pub const OCS_TIMEOUT: Duration = Duration::from_secs(3);

/// What `chap-devops status` reports.
#[derive(Debug, Serialize)]
pub struct StatusReport {
    /// The compose project name: the prefix every container and every named
    /// volume of this deployment carries. `None` only for a deployment whose
    /// directory name compose can make no project name out of.
    pub project: Option<String>,
    pub api_url: String,
    /// The host port this deployment publishes chap-core's API on. `--url`
    /// does not change it: it says which port the deployment uses, not which
    /// one this run happened to ask.
    pub api_port: u16,
    /// Which file [`StatusReport::api_port`] came from. `.env` wins, as it
    /// does for compose, so a deployment whose `.env` moved the port reports
    /// `env` and the port `.chap-devops/project.yaml` records is not the one in use.
    pub api_port_source: ApiPortSource,
    pub api: ApiHealth,
    /// Which chap-core this is, and whether the API said so itself.
    pub version: ApiVersion,
    /// The chap-core tag this deployment pins, and whether it is one that can
    /// point at a different image tomorrow.
    pub chap_tag: String,
    pub chap_tag_moving: bool,
    /// The build chap-core's container is running, as a short digest.
    ///
    /// Filled in by the caller, which is the half that has docker, and only
    /// asked for when the tag is a moving one: a release tag names its image
    /// already. `None` is "not asked" or "docker could not say".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chap_build: Option<String>,
    /// Services chap-core currently knows about, from `/v2/services`.
    pub registered: Vec<RegisteredService>,
    /// Service ids the project expects to be registered.
    pub expected: Vec<String>,
    /// Expected ids that are not registered. Empty without chap-core,
    /// where no registration is expected.
    pub missing: Vec<String>,
    /// How a human reaches each service this project enabled, by service id:
    /// its own host port, or chap-core's proxy for a service that publishes
    /// none. The URL chap-core reports in `registered` is the internal one and
    /// only resolves inside the compose network.
    pub reach: BTreeMap<String, String>,
    /// One row per model: everything this project enables, plus every
    /// registered service it does not know about.
    pub models: Vec<ModelStatus>,
    /// Registered service ids the project does not enable.
    pub unmanaged: Vec<String>,
    /// The models that chap-core cannot run, and why: a template it
    /// refuses, or configured models of other versions only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub revision_warnings: Vec<RevisionWarning>,
    /// Whether `.env` sets an API token, which is also whether these requests
    /// carried one.
    pub auth: bool,
    /// One row per enabled component other than chap-core, which has the
    /// chap-core line of its own. Empty on a deployment that has none.
    pub components: Vec<ComponentStatus>,
    /// Whether this deployment has a DHIS2 that no `chap-devops dhis2 connect` has
    /// been recorded for, from [`Components::dhis2_needs_connecting`].
    ///
    /// Read off `.chap-devops/components.yaml` and nothing else. It says what chap-devops
    /// has recorded, never what DHIS2 has: the route can have been deleted,
    /// repointed or disabled since, and `false` here is not evidence that the
    /// Modeling App can reach Chap. `chap-devops dhis2 show` is the command that
    /// asks DHIS2.
    ///
    /// [`Components::dhis2_needs_connecting`]: crate::components::Components::dhis2_needs_connecting
    pub dhis2_needs_connecting: bool,
    /// Whether chap-core is one this deployment does not run, recorded with
    /// `--chap-core-url` or `chap-devops components enable chap-core --url`.
    pub chap_core_elsewhere: bool,
    /// Whether chap-core's container started moments ago or reports its
    /// healthcheck as still starting, which makes an API that does not answer
    /// `starting` rather than `down`. Filled in by the caller, which has docker.
    pub api_starting: bool,
    /// The URL of the DHIS2 recorded with `chap-devops dhis2 use`, which has no row
    /// of its own: chap-devops does not run it, and asking it would need its
    /// credentials. `chap-devops dhis2 show` is the command that does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhis2_external: Option<String>,
    /// Containers of this deployment that are failing, with the lines of their
    /// logs that say why.
    ///
    /// Filled in by the caller, which is the half that has docker: a container
    /// that is unhealthy is why the API is not answering, and the reason is in
    /// its log rather than anywhere this probe can reach.
    pub unhealthy: Vec<crate::diagnose::Unhealthy>,
    /// Whose chap-core answered on this deployment's port while this
    /// deployment's own was not running, as the sentence that says so.
    ///
    /// Two deployments made with the same ports take turns on them, and the
    /// one that is up answers for both. Its health and its registry say
    /// nothing about this deployment, so [`StatusReport::api`] is reported
    /// down and this names the deployment that did answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_elsewhere: Option<String>,
}

impl StatusReport {
    /// Whether the API answered its health check as chap-core.
    #[cfg(test)]
    pub fn is_up(&self) -> bool {
        matches!(self.api, ApiHealth::Up { .. })
    }

    /// Whether everything the project enabled has registered.
    #[cfg(test)]
    pub fn is_complete(&self) -> bool {
        self.is_up() && self.missing.is_empty()
    }

    /// Whether chap-core's own container is up and failing its healthcheck.
    ///
    /// This is the difference between "nothing is listening on that port" and
    /// "chap-core is there and cannot start", which is the question an
    /// operator staring at a down API is actually asking.
    pub fn api_container_unhealthy(&self) -> bool {
        self.unhealthy
            .iter()
            .any(|entry| entry.service == crate::compose::API_SERVICE && entry.unhealthy)
    }
}

/// Result of `GET /health`.
#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ApiHealth {
    Up {
        status: String,
        message: String,
    },
    Down {
        error: String,
    },
    /// chap-core answered and refused the API token: it is up, and nothing it
    /// guards - the service registry among it - could be read. Kept apart from
    /// [`ApiHealth::Down`] because the cure is the token, not the container,
    /// and "down" would send an operator to logs that say it started fine.
    Rejected {
        error: String,
    },
    /// chap-core is not a component of this deployment, so there is no API to
    /// ask about. Not a failure: `chap-devops components disable chap-core` is how a
    /// deployment becomes, say, OCS on its own.
    Off,
}
/// The version `chap-devops status` puts next to the API URL.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApiVersion {
    /// What the API reported, or the tag `.chap-devops/project.yaml` pins when it
    /// reported nothing.
    pub value: String,
    /// True when `value` is that pin rather than the API's own answer.
    pub pinned: bool,
    /// The commit that build came from, where `/system/info` said. `None`
    /// when it did not, which is every chap-core built without `GIT_REVISION`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

impl ApiVersion {
    /// The cell for the chap-core line, empty when nothing is known at all.
    /// A moving tag such as `latest` is not called pinned: it is only where
    /// the value came from.
    pub fn label(&self) -> String {
        match (self.value.is_empty(), self.pinned) {
            (true, _) => String::new(),
            (false, true) if crate::chapcore::is_moving_tag(&self.value) => {
                format!("{} (from the pin)", self.value)
            }
            (false, true) => format!("{} (pinned)", self.value),
            (false, false) => self.value.clone(),
        }
    }
}

/// A commit as the line prints it: the first twelve characters of a full SHA,
/// and anything else as it came.
///
/// chap-core reports the whole forty-character commit, which is thirty more
/// than anyone reads off a status line and the same length the image digest
/// is already shortened to.
pub fn short_revision(revision: &str) -> String {
    let revision = revision.trim();
    let sha = revision.len() > 12 && revision.chars().all(|c| c.is_ascii_hexdigit());
    match sha {
        true => revision.chars().take(12).collect(),
        false => revision.to_string(),
    }
}

/// Which build a moving chap-core tag is actually on.
///
/// `dev` is the same name today and tomorrow, so the tag says nothing about
/// what is running. These two do: the digest the image was pulled at, and the
/// commit chap-core reports for itself. Both are best effort - docker may not
/// be there and the build may carry no revision - and an empty one is left
/// off the line rather than printed as a hole.
pub fn moving_build_cell(tag: &str, digest: Option<&str>, revision: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(digest) = digest.filter(|d| !d.is_empty()) {
        parts.push(format!("running {digest}"));
    }
    if let Some(revision) = revision.filter(|r| !r.is_empty()) {
        parts.push(format!("revision {}", short_revision(revision)));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("{tag}: {}", parts.join(", "))
}
/// Probe chap-core and diff the registered services against the project.
///
/// Never fails: an unreachable API - or one that answers with something that
/// is not chap-core - is reported as [`ApiHealth::Down`]. `running` is the
/// set of compose services with a container that is up, which is what tells a
/// model that never started from one that started and did not register; an
/// empty set is a safe answer when docker cannot be asked.
///
/// `token` is the API token this deployment's `.env` sets, sent as
/// `Authorization: Bearer` on every request. It is needed for `/v2/services`
/// on a protected deployment; the health and info paths are open either way,
/// and a token they do not need does them no harm.
///
/// `own_api` says `api_url` is this deployment's own port and docker was asked
/// which of its containers run, so an answer there while its `chap` container
/// is not running can be recognised as another deployment's.
pub fn status(
    project: &Project,
    api_url: &str,
    timeout: Duration,
    running: &BTreeSet<String>,
    token: Option<&str>,
    own_api: bool,
) -> StatusReport {
    let base = api_url.trim_end_matches('/').to_string();
    let mut expected: Vec<String> = project
        .state
        .models
        .values()
        .map(|m| m.service_id.clone())
        .collect();
    expected.sort();
    expected.dedup();

    let agent = agent(timeout);
    // A chap-core elsewhere is asked exactly like this deployment's own: its
    // API answers health, the registry and the version the same way.
    let chap_core = project.state.components.has_chap_core_api();
    let mut api = if !chap_core {
        // Nothing to ask, and nothing to wait out: a deployment without
        // chap-core has no API on this port, and probing one would only spend
        // a timeout to say so.
        ApiHealth::Off
    } else {
        match get(&agent, &base, HEALTH_PATH, token) {
            Ok(answer) => parse_health(&base, &answer.content_type, &answer.body),
            // A 401 is about the token, not about who answered: saying "not
            // chap-core" here would send an operator hunting for a dev server
            // that is not there.
            Err(Failure::Unauthorized) => ApiHealth::Rejected {
                error: token_rejected(&base, HEALTH_PATH, token.is_some()),
            },
            Err(Failure::Other(error)) => ApiHealth::Down { error },
        }
    };

    // This deployment's chap-core cannot answer with its container stopped,
    // so whatever did is another deployment on the same port, and nothing it
    // says - health, version, registry - is about this one.
    let mut api_elsewhere = None;
    if own_api
        && project.state.components.chap_core.enabled
        && project.state.components.chap_core_external.is_none()
        && !running.contains(crate::compose::API_SERVICE)
        && matches!(api, ApiHealth::Up { .. })
    {
        // Which deployment it is takes docker, which is the caller's half:
        // [`name_elsewhere`] puts the name in.
        let line = answered_elsewhere(&base, project.api_port_in_effect().0, None);
        api = ApiHealth::Down {
            error: line.clone(),
        };
        api_elsewhere = Some(line);
    }

    // Only ask for the service list when health already looked like
    // chap-core: otherwise we would wait out a second timeout to learn the
    // same thing. The list is the second half of the identity check - a
    // service registry that does not parse means whatever answered is not
    // chap-core, whatever `/health` said.
    let mut registered = Vec::new();
    if matches!(api, ApiHealth::Up { .. }) {
        let wrong = match get(&agent, &base, SERVICES_PATH, token) {
            Ok(answer) => match parse_services(&answer.body) {
                Ok(services) => {
                    registered = services;
                    None
                }
                Err(_) => Some(ApiHealth::Down {
                    error: services_are_not_chap_core(
                        &base,
                        &body_description(&answer.content_type, &answer.body),
                    ),
                }),
            },
            // The registry is not an open path, so a 401 here is the one place
            // a wrong token usually shows up: `/health` answered happily a
            // moment ago, so this is chap-core, up, refusing the token.
            Err(Failure::Unauthorized) => Some(ApiHealth::Rejected {
                error: token_rejected(&base, SERVICES_PATH, token.is_some()),
            }),
            Err(Failure::Other(error)) => Some(ApiHealth::Down {
                error: services_are_not_chap_core(&base, &error),
            }),
        };
        if let Some(wrong) = wrong {
            api = wrong;
        }
    }

    // A chap-core elsewhere runs no image this deployment pins, so the only
    // version there is to show is the one it reports.
    let elsewhere = project.state.components.chap_core_external.is_some();
    let pin = match elsewhere {
        true => "",
        false => project.state.chap_image_tag.as_str(),
    };
    let version = version_of(&agent, &base, &api, pin, token);
    // Without chap-core, no model registers anywhere, so none is missing.
    let missing = match chap_core {
        true => missing_ids(&expected, &registered),
        false => Vec::new(),
    };
    let reach = project
        .state
        .models
        .values()
        .map(|m| {
            (
                m.service_id.clone(),
                reach(project, m.host_port, &m.service_id),
            )
        })
        .collect();
    let mut revisions = Vec::new();
    let models = if chap_core {
        let mut rows = model_rows(&enabled_models(project), &registered, running, now());
        mark_unreachable(&mut rows, &registered, &|id| match get(
            &agent,
            &base,
            &proxied_health_path(id),
            token,
        ) {
            Err(Failure::Other(error))
                if error.starts_with("HTTP 5") || !error.starts_with("HTTP ") =>
            {
                Some(error)
            }
            _ => None,
        });
        // Read-only: the listing says which models nothing can run, and
        // `chap-devops models configs sync` is what changes that. It also says which
        // template chap-core refuses.
        let asked =
            |row: &ModelStatus| matches!(row.state, ModelState::Registered | ModelState::Unmanaged);
        let configured = match rows.iter().any(asked) {
            true => get(
                &agent,
                &base,
                crate::configs::sync::CONFIGURED_MODELS_PATH,
                token,
            )
            .ok()
            .and_then(|answer| serde_json::from_str::<serde_json::Value>(&answer.body).ok())
            .map(|listed| crate::modeltest::configured_models(&listed)),
            false => None,
        };
        if let Some(configured) = &configured {
            mark_unconfigured(&mut rows, &registered, configured);
        }
        let sends = sends_revision(&registered, Some(&version.value));
        revisions = revision_warnings(
            &rows,
            &registered,
            configured.as_deref().unwrap_or(&[]),
            sends,
        );
        revisions.extend(pin_warnings(project.state.models.values(), &registered));
        rows
    } else {
        // Nothing registers anywhere, so each model is asked itself, on the
        // host port a model without chap-core always publishes.
        standalone_model_rows(&enabled_models(project), running, &|port| {
            get(
                &agent,
                &format!("http://localhost:{port}"),
                MODEL_HEALTH_PATH,
                None,
            )
            .is_ok()
        })
    };
    let unmanaged = models
        .iter()
        .filter(|m| m.state == ModelState::Unmanaged)
        .map(|m| m.id.clone())
        .collect();
    let components = component_rows(project, &agent, running);
    let (api_port, api_port_source) = project.api_port_in_effect();
    StatusReport {
        project: project.compose_project_name(),
        api_url: base,
        api_port,
        api_port_source,
        api,
        version,
        chap_tag_moving: !elsewhere
            && crate::chapcore::is_moving_tag(&project.state.chap_image_tag),
        chap_tag: project.state.chap_image_tag.clone(),
        chap_build: None,
        registered,
        expected,
        missing,
        reach,
        models,
        unmanaged,
        revision_warnings: revisions,
        auth: token.is_some(),
        components,
        dhis2_needs_connecting: project.state.components.dhis2_needs_connecting(),
        chap_core_elsewhere: elsewhere,
        api_starting: false,
        dhis2_external: project
            .state
            .components
            .dhis2_external
            .as_ref()
            .map(|external| external.url.clone()),
        unhealthy: Vec::new(),
        api_elsewhere,
    }
}

/// Put the name of the deployment that answered into a report whose API
/// answered from elsewhere, when another deployment on this machine publishes
/// that port. `holder` is that deployment, as [`crate::ports::other_deployments`]
/// finds it.
pub fn name_elsewhere(report: &mut StatusReport, holder: Option<&crate::ports::Deployment>) {
    if report.api_elsewhere.is_none() || holder.is_none() {
        return;
    }
    let line = answered_elsewhere(&report.api_url, report.api_port, holder);
    report.api = ApiHealth::Down {
        error: line.clone(),
    };
    report.api_elsewhere = Some(line);
}

/// The sentence for an API that answered while this deployment's chap-core
/// was not running: whose it is, when another deployment on this machine
/// publishes the port, and the two ways to put this one there instead.
pub fn answered_elsewhere(
    base: &str,
    port: u16,
    holder: Option<&crate::ports::Deployment>,
) -> String {
    match holder {
        Some(other) => format!(
            "this deployment's chap-core is not running; {base} is {name} ({dir}) answering on \
             the same port; stop it with `chap-devops -C {dir} down`, or run `chap-devops up --replace` here",
            name = other.name(),
            dir = other.dir.display(),
        ),
        None => format!(
            "this deployment's chap-core is not running; something else answers on port {port}, \
             and `chap-devops up` names it"
        ),
    }
}
/// Whether anything this deployment declares is not where it should be, which
/// is what a non-zero exit from `chap-devops status` means.
///
/// One rule for every deployment shape, because the command is a health gate
/// for a script and a script cannot know which shape it is polling. Before
/// this, a component was only ever consulted on a deployment without chap-core,
/// and even there only while it was `starting` - so an `ocs` container that
/// died left `chap-devops status` exiting 0, which is the one answer a monitor must
/// never get wrong.
///
/// The exit stays silent. The rows have already named what is wrong - the model
/// table which ones did not register, the component lines which one is not up -
/// and an error line on top of that would be the third telling. Only an API
/// that is not answering gets one, because nothing else on the screen says why.
pub fn exit_failure(report: &StatusReport) -> bool {
    let components_failing = report
        .components
        .iter()
        .any(|component| component.state.is_problem());
    match report.api {
        ApiHealth::Down { .. } | ApiHealth::Rejected { .. } => true,
        ApiHealth::Up { .. } => {
            !report.missing.is_empty()
                || components_failing
                || report
                    .models
                    .iter()
                    .any(|m| m.state == ModelState::Unreachable)
        }
        // chap-core is not part of this deployment, so its API not answering is
        // the expected state rather than a failure.
        ApiHealth::Off => components_failing || report.models.iter().any(|m| m.state.is_problem()),
    }
}

#[cfg(test)]
mod tests;
