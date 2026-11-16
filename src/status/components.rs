//! The component rows: OCS, the object store and DHIS2, each judged by its
//! container and then by its own health endpoint.

use super::probe::{agent, get};
use super::{DATASETS_PATH, DHIS2_PING_PATH, HEALTH_PATH, OCS_TIMEOUT};
use crate::project::Project;
use serde::Serialize;
use std::collections::BTreeSet;

/// Where one component other than chap-core stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentState {
    /// Answering its health endpoint, or - for a service this CLI does not
    /// probe over HTTP - simply running.
    Up,
    /// Its container is up but it is not answering yet.
    Starting,
    /// Its container is up, it does not answer, and the health check of its
    /// container says it failed: not starting any more, but broken.
    Unhealthy,
    /// No container, so nothing to answer.
    NotRunning,
    /// Its container is paused, as an interrupted `chap-devops backup create` can
    /// leave it: it is there and does not answer.
    Paused,
}

impl ComponentState {
    /// The STATE cell.
    pub fn label(self) -> &'static str {
        match self {
            ComponentState::Up => "up",
            ComponentState::Starting => "starting",
            ComponentState::Unhealthy => "unhealthy",
            ComponentState::NotRunning => "not running",
            ComponentState::Paused => "paused",
        }
    }

    /// Whether this row is something to do about: anything that is not `up`.
    ///
    /// A component that is not running is as much a problem as a model that is
    /// not registered, and it is read the same way - by a script polling
    /// `chap-devops status` for a deployment that has stopped being what it should
    /// be. This is the predicate that exit code is made of, and nothing else:
    /// the STATE cell is coloured from the three states directly, because
    /// `starting` is amber and `not running` is red while both are failures.
    pub fn is_problem(self) -> bool {
        matches!(
            self,
            ComponentState::Starting
                | ComponentState::Unhealthy
                | ComponentState::NotRunning
                | ComponentState::Paused
        )
    }
}

/// One row of the component table.
#[derive(Debug, Clone, Serialize)]
pub struct ComponentStatus {
    /// Component name, which is also the compose service name.
    pub name: String,
    pub state: ComponentState,
    /// Where a human reaches it from this machine, or `internal` for a
    /// component that publishes no host port - with the proxy named when the
    /// component records one, since that is the address that does work.
    pub reach: String,
    /// Where this component answers its health endpoint: its own host port
    /// plus the path. `None` for an instance that publishes no host port,
    /// which is one that cannot be asked from out here at all.
    ///
    /// Derived from the address rather than from a request, so it says the same
    /// thing whether the instance is running or not: a consumer keying off it
    /// must not watch it appear and disappear with the container.
    pub health_url: Option<String>,
    /// Whether this OCS instance refuses ingestion over HTTP. Always false for
    /// every other component, none of which has the setting.
    pub read_only: bool,
    /// How many datasets this OCS instance holds, from its own JSON API.
    /// `None` for every other component, and for an instance that was not
    /// asked or did not answer.
    pub datasets: Option<u32>,
    /// How much its data directory holds, in bytes.
    ///
    /// Filled in by the caller, which is the half that has docker: it is read
    /// from inside the running container, so it is `None` here and `None`
    /// altogether for an instance that is not running.
    pub data_bytes: Option<u64>,
}
/// The database service of the `dhis2` component.
const DHIS2_DB_SERVICE: &str = "dhis2-db";

/// One row per enabled component other than chap-core.
///
/// OCS and DHIS2 are asked over HTTP, because each publishes a host port and an
/// endpoint of its own that answers without credentials; the object store
/// publishes nothing by default, so the only thing that can be said about it
/// from out here is whether its container is up. A component with no container
/// at all is `not running` rather than down: there is nothing wrong with a
/// deployment that has not been started.
///
/// Nothing is asked of a component whose container is not running. `chap-devops
/// status` is the command run most often, and an instance that is not there
/// would cost it a timeout to say what the container already said.
pub(super) fn component_rows(
    project: &Project,
    agent: &super::probe::Prober,
    running: &BTreeSet<String>,
) -> Vec<ComponentStatus> {
    let components = &project.state.components;
    let mut rows = Vec::new();
    if components.ocs.enabled {
        // An instance with no host port cannot be asked from out here at all:
        // the only way in is the compose network or whatever proxy sits in
        // front of it, and neither is something this probe can assume. So it
        // is judged by its container, exactly as the object store is.
        let url = components.ocs_url();
        let up = running.contains(crate::compose::OCS_SERVICE);
        let state = match (&url, up) {
            // The same reasoning gates the request on the container: with
            // nothing running there is no answer to wait for, only a timeout to
            // spend on a port this deployment has nobody on - and whatever else
            // is holding it would answer in its place, which is how a stopped
            // OCS came to be reported `up` on a machine running a second
            // deployment's OCS on the default 9000.
            (_, false) => ComponentState::NotRunning,
            (Some(url), true) => component_state(get(agent, url, HEALTH_PATH, None).is_ok(), up),
            (None, true) => ComponentState::Up,
        };
        // What the instance holds is only asked for when it has just
        // answered: a second request to an instance that is down would spend
        // another timeout to learn the same thing.
        let datasets = url
            .as_deref()
            .filter(|_| state == ComponentState::Up)
            .and_then(ocs_datasets);
        rows.push(ComponentStatus {
            name: crate::compose::OCS_SERVICE.to_string(),
            state,
            reach: components.ocs_reach(),
            // The address, not the answer: the field says where this instance
            // answers, and that is as true of one that is switched off.
            health_url: url.map(|url| format!("{url}{HEALTH_PATH}")),
            read_only: components.ocs.read_only,
            datasets,
            data_bytes: None,
        });
    }
    if components.s3.enabled {
        let up = running.contains(crate::compose::S3_SERVICE);
        rows.push(ComponentStatus {
            name: crate::compose::S3_SERVICE.to_string(),
            state: component_state(up, up),
            reach: match components.s3.port {
                Some(port) => format!("http://localhost:{port}"),
                None => "internal".to_string(),
            },
            health_url: None,
            read_only: false,
            datasets: None,
            data_bytes: None,
        });
    }
    if components.dhis2.enabled {
        // Judged by its container first and only then asked anything, exactly
        // as OCS is - and then asked, for a reason OCS does not have.
        //
        // A DHIS2 container can report healthy while the instance is entirely
        // broken. A Spring context that failed to come up - an unreadable
        // `dhis2/dhis.conf`, a PostgreSQL extension the image cannot find, a
        // Flyway checksum that does not match the database - leaves Tomcat
        // running and serving pages, and every `/api/*` request then answers
        // 404. So a 200 from `/api/ping` is the only honest evidence that this
        // instance works, and a container that is up while `/api/ping` does not
        // answer is `starting` at best.
        //
        // Nothing beyond that is asked, because nothing more can honestly be
        // had: what is worth knowing past up or down is the version the
        // instance is running, and `/api/system/info` answers that only to a
        // session, which this CLI holds no credentials for. Its 401 would prove
        // the API layer is alive, which is what a 404 disproves - and
        // `/api/ping` has established exactly that already, for one request.
        let url = components
            .port_of(crate::components::Component::Dhis2)
            .map(|port| format!("http://localhost:{port}"));
        let up = running.contains(crate::compose::DHIS2_SERVICE);
        // `dhis2` is created only when `dhis2-db` is healthy, and on a seeded
        // deployment that is after the restore, which can take many minutes.
        // A running database without its DHIS2 is a DHIS2 that is starting,
        // not one that `chap-devops up` has to start.
        let database_up = running.contains(DHIS2_DB_SERVICE);
        let state = match (&url, up) {
            (_, false) if database_up => ComponentState::Starting,
            // The same reasoning that gates the OCS request on its container:
            // with nothing running there is no answer to wait for, only a
            // timeout to spend on a port this deployment has nobody on - and
            // whatever else holds it would answer in its place.
            (_, false) => ComponentState::NotRunning,
            (Some(url), true) => {
                component_state(get(agent, url, DHIS2_PING_PATH, None).is_ok(), up)
            }
            (None, true) => ComponentState::Up,
        };
        rows.push(ComponentStatus {
            name: crate::compose::DHIS2_SERVICE.to_string(),
            state,
            reach: components.dhis2_reach(),
            // The address, not the answer, as the OCS row reports it: the field
            // says where this instance answers, and that is as true of one that
            // is switched off.
            health_url: url.map(|url| format!("{url}{DHIS2_PING_PATH}")),
            // None of the three is a DHIS2 fact, and each is reported on the
            // same terms it is for the object store: absent, whatever the
            // container is doing, rather than a field that comes and goes.
            read_only: false,
            datasets: None,
            data_bytes: None,
        });
    }
    rows
}

/// How many datasets an OCS instance holds, from `GET /datasets?f=json`.
///
/// Best-effort and bounded by [`OCS_TIMEOUT`]: an instance that does not
/// answer, answers something else, or is an older OCS without the JSON list
/// yields `None`, and the line it would have decorated is printed unchanged.
pub fn ocs_datasets(url: &str) -> Option<u32> {
    let agent = agent(OCS_TIMEOUT);
    let answer = get(&agent, url.trim_end_matches('/'), DATASETS_PATH, None).ok()?;
    parse_dataset_count(&answer.body)
}

/// The number of entries in an OCS `DatasetList`.
///
/// Strict about the envelope, for the same reason [`parse_services`] is: the
/// count goes on a line that says the instance is up, so a JSON body from
/// something else on that port must not become a number next to its name.
///
/// [`parse_services`]: super::parse_services
pub fn parse_dataset_count(body: &str) -> Option<u32> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    if value.get("kind")?.as_str()? != DATASET_LIST_KIND {
        return None;
    }
    u32::try_from(value.get("items")?.as_array()?.len()).ok()
}

/// The `kind` OCS puts on its dataset list.
const DATASET_LIST_KIND: &str = "DatasetList";

/// Mark the rows that do not answer and whose container docker reports
/// unhealthy: those are broken, not starting. `unhealthy` holds the services.
pub fn mark_unhealthy(rows: &mut [ComponentStatus], unhealthy: &BTreeSet<String>) {
    for row in rows {
        if row.state == ComponentState::Starting && unhealthy.contains(&row.name) {
            row.state = ComponentState::Unhealthy;
        }
    }
}

/// Mark the rows whose container docker reports paused. `paused` holds the
/// services.
pub fn mark_paused(report: &mut super::StatusReport, paused: &BTreeSet<String>) {
    for row in &mut report.components {
        if paused.contains(&row.name) {
            row.state = ComponentState::Paused;
        }
    }
    for row in &mut report.models {
        if paused.contains(&row.id) {
            row.state = super::ModelState::Paused;
        }
    }
}

/// The warning for the services docker reports paused, with the way out.
pub fn paused_line(paused: &BTreeSet<String>) -> Option<String> {
    let names: Vec<&str> = paused.iter().map(String::as_str).collect();
    match names.as_slice() {
        [] => None,
        [one] => Some(format!(
            "{one} is paused, so it does not answer; run `chap-devops up` to resume it"
        )),
        many => Some(format!(
            "{} are paused, so they do not answer; run `chap-devops up` to resume them",
            many.join(", ")
        )),
    }
}

/// Where one component stands, from whether it answered and whether its
/// container is up.
pub fn component_state(answered: bool, container_up: bool) -> ComponentState {
    match (answered, container_up) {
        (true, _) => ComponentState::Up,
        (false, true) => ComponentState::Starting,
        (false, false) => ComponentState::NotRunning,
    }
}
