//! How status asks a component: OCS and DHIS2 probed against stand-ins.

use super::*;

/// A deployment with OCS enabled, published on `port` when it has one.
fn ocs_project(port: Option<u16>) -> Project {
    let mut state = crate::project::ProjectState::default();
    state.components.ocs.enabled = true;
    state.components.ocs.port = port;
    Project {
        dir: std::path::PathBuf::from("/tmp/chapx"),
        state,
    }
}

/// A deployment with DHIS2 enabled, published on `port` when it has one.
fn dhis2_project(port: Option<u16>) -> Project {
    let mut state = crate::project::ProjectState::default();
    state.components.dhis2.enabled = true;
    state.components.dhis2.port = port;
    Project {
        dir: std::path::PathBuf::from("/tmp/chapx"),
        state,
    }
}

/// An OCS: `/health` and the dataset list, both 200.
fn stand_in_ocs() -> StandIn {
    stand_in(|path| match path.starts_with("/datasets") {
        true => (200, DATASETS),
        false => (200, r#"{"status":"success","message":"healthy"}"#),
    })
}

/// A DHIS2 whose API layer is alive: `/api/ping` answers, as the container's
/// own healthcheck asks it to.
fn stand_in_dhis2() -> StandIn {
    stand_in(|path| match path == DHIS2_PING_PATH {
        true => (200, "pong"),
        false => (404, r#"{"httpStatusCode":404}"#),
    })
}

/// The falsely healthy DHIS2: Tomcat is up and serving, and every `/api/*`
/// request answers 404 because the Spring context never came up.
fn stand_in_broken_dhis2() -> StandIn {
    stand_in(|_| (404, "<html><body>Not Found</body></html>"))
}

/// A port nothing is listening on: taken to learn a free number, then let
/// go, so a connection to it is refused rather than left hanging.
fn closed_port() -> u16 {
    let listener =
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("a loopback port");
    listener.local_addr().expect("the bound address").port()
}

/// Short, because none of these tests waits for anything: the one request
/// that fails is refused, not timed out.
fn probe_agent() -> crate::status::probe::Prober {
    agent(Duration::from_millis(500))
}

/// The row of a deployment that has not been started costs no request at
/// all. Nothing is there to answer, so the only thing a probe could buy is
/// a timeout on every `chap-devops status` - or an answer from whatever else
/// holds that host port, which is how a stopped OCS came to read as `up`
/// beside another deployment's OCS on the same default 9000.
#[test]
fn a_component_that_is_not_running_is_not_asked_and_still_reports_its_health_url() {
    // Answering, and deliberately not this deployment's.
    let stand_in = stand_in_ocs();
    let rows = component_rows(
        &ocs_project(Some(stand_in.port)),
        &probe_agent(),
        &BTreeSet::new(),
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, ComponentState::NotRunning);
    assert!(
        stand_in.asked().is_empty(),
        "no request was worth making: {:?}",
        stand_in.asked()
    );
    // `--json` keeps every field it had: the address is known from the
    // record, so it is reported whether the instance is up or not.
    assert_eq!(
        rows[0].health_url.as_deref(),
        Some(format!("http://localhost:{}/health", stand_in.port).as_str())
    );
    assert_eq!(rows[0].reach, format!("http://localhost:{}", stand_in.port));
    assert!(!rows[0].read_only);
    // Nothing was asked, so there is nothing it holds to report.
    assert_eq!(rows[0].datasets, None);
    assert_eq!(rows[0].data_bytes, None);
}

/// The distinction the probe is there for: a container that is up and
/// answering is `up`, and it is the answer that carries the dataset count.
#[test]
fn a_running_component_is_asked_and_reports_what_it_holds() {
    let stand_in = stand_in_ocs();
    let rows = component_rows(
        &ocs_project(Some(stand_in.port)),
        &probe_agent(),
        &running(&[crate::compose::OCS_SERVICE]),
    );
    assert_eq!(rows[0].state, ComponentState::Up);
    assert_eq!(rows[0].datasets, Some(3));
    assert_eq!(
        stand_in.asked(),
        vec![HEALTH_PATH.to_string(), DATASETS_PATH.to_string()]
    );
    assert_eq!(
        rows[0].health_url.as_deref(),
        Some(format!("http://localhost:{}/health", stand_in.port).as_str())
    );
}

/// The other half of that distinction: the container is up but nothing is
/// answering on its port yet, which is a wait rather than a fault.
#[test]
fn a_running_component_that_does_not_answer_yet_is_starting() {
    let port = closed_port();
    let rows = component_rows(
        &ocs_project(Some(port)),
        &probe_agent(),
        &running(&[crate::compose::OCS_SERVICE]),
    );
    assert_eq!(rows[0].state, ComponentState::Starting);
    // Asked and not answered, so there is still no count to put on the line.
    assert_eq!(rows[0].datasets, None);
    assert_eq!(
        rows[0].health_url.as_deref(),
        Some(format!("http://localhost:{port}/health").as_str())
    );
}

/// An instance that publishes no host port is judged by its container
/// alone, as it always has been: there is no address out here to ask.
#[test]
fn an_instance_with_no_host_port_is_judged_by_its_container() {
    let project = ocs_project(None);
    let rows = component_rows(
        &project,
        &probe_agent(),
        &running(&[crate::compose::OCS_SERVICE]),
    );
    assert_eq!(rows[0].state, ComponentState::Up);
    assert_eq!(rows[0].reach, "internal");
    assert_eq!(rows[0].health_url, None);
    assert_eq!(rows[0].datasets, None);

    let rows = component_rows(&project, &probe_agent(), &BTreeSet::new());
    assert_eq!(rows[0].state, ComponentState::NotRunning);
    assert_eq!(rows[0].health_url, None);
}

/// The DHIS2 row is judged the same way the OCS one is: the container first,
/// then one request - and the request is `/api/ping`, which is the only
/// route an instance this CLI has no login for will answer.
#[test]
fn a_running_dhis2_is_up_only_once_api_ping_answers() {
    let stand_in = stand_in_dhis2();
    let rows = component_rows(
        &dhis2_project(Some(stand_in.port)),
        &probe_agent(),
        &running(&[crate::compose::DHIS2_SERVICE]),
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "dhis2");
    assert_eq!(rows[0].state, ComponentState::Up);
    assert_eq!(stand_in.asked(), vec![DHIS2_PING_PATH.to_string()]);
    assert_eq!(
        rows[0].health_url.as_deref(),
        Some(format!("http://localhost:{}/api/ping", stand_in.port).as_str())
    );
    assert_eq!(rows[0].reach, format!("http://localhost:{}", stand_in.port));
    // None of the OCS fields is a DHIS2 fact, and none of them appears.
    assert!(!rows[0].read_only);
    assert_eq!(rows[0].datasets, None);
    assert_eq!(rows[0].data_bytes, None);
}

/// The failure the request exists to catch, and the whole reason the row
/// does not stop at the container: DHIS2 reports itself healthy while every
/// `/api/*` request 404s, which is what a failed Spring context looks like
/// from outside. `up` would be the one wrong answer here.
#[test]
fn a_dhis2_that_serves_pages_but_no_api_is_not_up() {
    let stand_in = stand_in_broken_dhis2();
    let rows = component_rows(
        &dhis2_project(Some(stand_in.port)),
        &probe_agent(),
        &running(&[crate::compose::DHIS2_SERVICE]),
    );
    assert_eq!(rows[0].state, ComponentState::Starting);
    assert_ne!(rows[0].state, ComponentState::Up);
    assert_eq!(stand_in.asked(), vec![DHIS2_PING_PATH.to_string()]);
    // The address is recorded state, so the field says the same thing
    // whatever the instance answered.
    assert_eq!(
        rows[0].health_url.as_deref(),
        Some(format!("http://localhost:{}/api/ping", stand_in.port).as_str())
    );
}

/// A DHIS2 that was never started costs no request, for the reason the OCS
/// row costs none: the only thing a probe could buy is a timeout, or an
/// answer from whatever else holds that host port.
#[test]
fn a_dhis2_that_is_not_running_is_not_asked() {
    let stand_in = stand_in_dhis2();
    let rows = component_rows(
        &dhis2_project(Some(stand_in.port)),
        &probe_agent(),
        &BTreeSet::new(),
    );
    assert_eq!(rows[0].state, ComponentState::NotRunning);
    assert!(
        stand_in.asked().is_empty(),
        "no request was worth making: {:?}",
        stand_in.asked()
    );
    assert_eq!(
        rows[0].health_url.as_deref(),
        Some(format!("http://localhost:{}/api/ping", stand_in.port).as_str())
    );
}

/// On a seeded deployment `dhis2` waits for `dhis2-db`, which restores the
/// dump first. A running database without its DHIS2 is `starting`, and
/// nothing is asked of a DHIS2 that has no container yet.
#[test]
fn a_dhis2_whose_database_runs_is_starting_not_stopped() {
    let stand_in = stand_in_dhis2();
    let rows = component_rows(
        &dhis2_project(Some(stand_in.port)),
        &probe_agent(),
        &running(&["dhis2-db"]),
    );
    assert_eq!(rows[0].state, ComponentState::Starting);
    assert!(stand_in.asked().is_empty(), "{:?}", stand_in.asked());
    assert_eq!(
        components_closing_line(&rows),
        "1 of 1 component is still starting; run `chap-devops status` again in a moment"
    );
}

/// An instance behind a reverse proxy publishes no host port, so there is no
/// address out here to ask and the container is the whole answer.
#[test]
fn a_dhis2_with_no_host_port_is_judged_by_its_container() {
    let project = dhis2_project(None);
    let rows = component_rows(
        &project,
        &probe_agent(),
        &running(&[crate::compose::DHIS2_SERVICE]),
    );
    assert_eq!(rows[0].state, ComponentState::Up);
    assert_eq!(rows[0].reach, "internal");
    assert_eq!(rows[0].health_url, None);

    let rows = component_rows(&project, &probe_agent(), &BTreeSet::new());
    assert_eq!(rows[0].state, ComponentState::NotRunning);
    assert_eq!(rows[0].health_url, None);
}

/// The rows come in the order the components are rendered, which is the
/// order the lines are printed in.
#[test]
fn the_component_rows_follow_the_render_order() {
    let mut state = crate::project::ProjectState::default();
    state.components.ocs.enabled = true;
    state.components.ocs.port = None;
    state.components.s3.enabled = true;
    state.components.dhis2.enabled = true;
    state.components.dhis2.port = None;
    let project = Project {
        dir: std::path::PathBuf::from("/tmp/chapx"),
        state,
    };
    let names: Vec<String> = component_rows(&project, &probe_agent(), &BTreeSet::new())
        .into_iter()
        .map(|row| row.name)
        .collect();
    assert_eq!(names, vec!["ocs", "s3", "dhis2"]);
}

/// A container that docker reports unhealthy is broken, not starting, and
/// the closing line says where to look.
#[test]
fn an_unhealthy_component_is_not_reported_as_starting() {
    let row = |name: &str, state| components::ComponentStatus {
        name: name.to_string(),
        state,
        reach: String::new(),
        health_url: None,
        read_only: false,
        datasets: None,
        data_bytes: None,
    };
    let mut rows = vec![
        row("dhis2", ComponentState::Starting),
        row("ocs", ComponentState::Starting),
        row("s3", ComponentState::Up),
    ];
    let unhealthy: BTreeSet<String> = ["dhis2".to_string(), "s3".to_string()].into();
    mark_unhealthy(&mut rows, &unhealthy);
    assert_eq!(rows[0].state, ComponentState::Unhealthy);
    // Not unhealthy in docker: still starting.
    assert_eq!(rows[1].state, ComponentState::Starting);
    // Answering wins over a stale health state.
    assert_eq!(rows[2].state, ComponentState::Up);
    assert!(ComponentState::Unhealthy.is_problem());
    assert_eq!(ComponentState::Unhealthy.label(), "unhealthy");
}
