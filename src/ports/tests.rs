use super::*;
use crate::project::{CHAP_DEVOPS_COMPOSE, EnabledModel, ProjectState};
use std::collections::{BTreeMap, BTreeSet};

/// A listener on a port the kernel picked, so the test never fights
/// another process over a fixed number.
fn bound(addr: Ipv4Addr) -> (TcpListener, u16) {
    let listener = TcpListener::bind((addr, 0)).expect("a free port");
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

#[test]
fn a_loopback_only_listener_is_busy() {
    // The case a single wildcard bind misses on macOS, because std sets
    // SO_REUSEADDR and BSD then allows the wildcard next to it.
    let (listener, port) = bound(Ipv4Addr::LOCALHOST);
    assert!(is_busy(port), "port {port} has a listener on it");
    drop(listener);
}

#[test]
fn a_wildcard_listener_is_busy_too() {
    let (listener, port) = bound(Ipv4Addr::UNSPECIFIED);
    assert!(is_busy(port), "port {port} is published on every address");
    drop(listener);
}

#[test]
fn a_port_nothing_holds_is_free() {
    let (listener, port) = bound(Ipv4Addr::LOCALHOST);
    drop(listener);
    let answer = is_busy(port);
    // Tests run in parallel, and the released port is back in the
    // ephemeral range, so another test in this run may have taken it
    // between the drop and the probe. Taking it ourselves is what says the
    // probe was asked about a port nobody held.
    match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Ok(_) => assert!(!answer, "nothing was listening on port {port}"),
        Err(_) => eprintln!("skipping: port {port} was claimed by another test"),
    }
}

#[test]
fn port_zero_is_never_reported_busy() {
    // Binding port 0 always succeeds, so the question has no answer; the
    // honest one is "no conflict".
    assert!(!is_busy(0));
}

#[test]
fn first_free_walks_upwards() {
    let busy = |port: u16| (8000..8003).contains(&port);
    assert_eq!(first_free(8000, 8100, &busy), Some(8003));
    assert_eq!(first_free(8004, 8100, &busy), Some(8004));
    assert_eq!(first_free(8000, 8002, &busy), None);
}

fn enabled(service_id: &str, port: Option<u16>) -> EnabledModel {
    EnabledModel {
        previous: None,
        reads_port: false,
        service_id: service_id.to_string(),
        image: "ghcr.io/chap-models/x".into(),
        image_tag: "sha-1111111".into(),
        image_digest: None,
        version: "1.0.0".into(),
        channel: None,
        host_port: port,
        bind: None,
        data_dir: "/app/data".into(),
        user: "chapkit:chapkit".into(),
        user_from: Default::default(),
        platform: None,
        compose_file: format!("compose.{service_id}.yml"),
    }
}

/// A project directory with one published and one internal overlay.
fn project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("compose.yml"),
        "services:\n  chap:\n    ports:\n      - \"8000:8000\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("compose.chap-devops.yml"),
        "services:\n  chap:\n    ports: !override\n      - \"${CHAP_API_PORT:-8123}:8000\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("compose.loud.yml"),
        "services:\n  loud:\n    expose:\n      - \"8000\"\n    ports:\n      - \"5001:8000\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("compose.quiet.yml"),
        "services:\n  quiet:\n    expose:\n      - \"8000\"\n",
    )
    .unwrap();
    // Never in the -f list, so never probed.
    std::fs::write(
        dir.path().join("compose.stranger.yml"),
        "services:\n  stranger:\n    ports:\n      - \"5999:8000\"\n",
    )
    .unwrap();
    let state = ProjectState {
        api_port: 8123,
        rendered_files: vec![
            "compose.yml".into(),
            "compose.chap-devops.yml".into(),
            "compose.loud.yml".into(),
            "compose.quiet.yml".into(),
            "compose.marketplace.yml".into(),
        ],
        models: BTreeMap::from([
            ("loud".to_string(), enabled("loud", Some(5001))),
            ("quiet".to_string(), enabled("quiet", None)),
        ]),
        ..ProjectState::default()
    };
    let project = Project {
        dir: dir.path().to_path_buf(),
        state,
    };
    (dir, project)
}

#[test]
fn claims_are_the_api_port_plus_the_published_overlays() {
    let (_dir, project) = project();
    assert_eq!(
        claims(&project),
        vec![
            PortClaim {
                service: "chap".into(),
                port: 8123
            },
            PortClaim {
                service: "loud".into(),
                port: 5001
            },
        ],
        "an internal model publishes nothing, and a stray compose file is not ours"
    );
}

/// The preflight has to reserve the port the stack will really publish,
/// which is the one `.env` names: checking the recorded port instead lets
/// `chap-devops up` sail past a conflict and hand it to Docker.
#[test]
fn the_api_claim_follows_the_env_override() {
    let (dir, project) = project();
    std::fs::write(dir.path().join(".env"), "CHAP_API_PORT=18000\n").unwrap();
    assert!(claims(&project).contains(&PortClaim {
        service: "chap".into(),
        port: 18000
    }));
    assert!(
        !claims(&project).iter().any(|c| c.port == 8123),
        "the recorded port is not published any more"
    );

    // A commented line is not an override, and the recorded port stands.
    std::fs::write(dir.path().join(".env"), "# CHAP_API_PORT=18000\n").unwrap();
    assert!(claims(&project).contains(&PortClaim {
        service: "chap".into(),
        port: 8123
    }));
}

#[test]
fn a_component_claims_its_port_before_the_first_sync() {
    let (_dir, mut project) = project();
    project.state.components.ocs.enabled = true;
    project.state.components.ocs.port = Some(9010);
    project.state.components.s3.enabled = true;

    let found = claims(&project);
    assert!(found.contains(&PortClaim {
        service: "ocs".into(),
        port: 9010
    }));
    assert!(
        !found.iter().any(|c| c.service == "s3"),
        "the store publishes nothing by default"
    );

    project.state.components.s3.port = Some(9002);
    assert!(claims(&project).contains(&PortClaim {
        service: "s3".into(),
        port: 9002
    }));

    // With chap-core off there is no API port to claim at all.
    project.state.components.chap_core.enabled = false;
    let found = claims(&project);
    assert!(!found.iter().any(|c| c.service == "chap"), "{found:?}");
}

#[test]
fn a_component_port_is_claimed_once_however_many_files_publish_it() {
    let (dir, mut project) = project();
    project.state.components.ocs.enabled = true;
    project.state.components.ocs.port = Some(9010);
    std::fs::write(
        dir.path().join("compose.ocs.yml"),
        "services:\n  ocs:\n    ports:\n      - \"9010:9000\"\n",
    )
    .unwrap();
    project.state.rendered_files.push("compose.ocs.yml".into());
    let ocs: Vec<PortClaim> = claims(&project)
        .into_iter()
        .filter(|c| c.service == "ocs")
        .collect();
    assert_eq!(ocs.len(), 1, "{ocs:?}");
    assert_eq!(ocs[0].port, 9010);
}

#[test]
fn a_component_message_points_at_the_command_that_moves_its_port() {
    let claim = PortClaim {
        service: "ocs".into(),
        port: 9000,
    };
    let line = busy_line(&claim, Some(9001));
    assert!(line.contains("(needed by ocs)"), "{line}");
    assert!(
        line.contains("`chap-devops components enable ocs --port 9001`"),
        "{line}"
    );
    assert!(busy_line(&claim, None).contains("--port <free>"));
    assert!(!line.contains("--api-port"), "{line}");
    assert!(!line.contains("models unexpose"), "{line}");
}

/// `components enable ocs --port 18010` on a machine where something else
/// is listening: the same line `init` would have printed, so the two places
/// a component's port is decided read alike.
#[test]
fn a_component_port_warning_names_the_listener_and_the_way_out() {
    let (_dir, project) = project();
    let free = |_: u16| false;
    assert_eq!(
        component_port_line(
            &project,
            Component::Ocs,
            18010,
            &free,
            &OwnPorts::default(),
            &[]
        ),
        None,
        "nothing is listening, so there is nothing to say"
    );

    let line = component_port_line(
        &project,
        Component::Ocs,
        18010,
        &|port| port == 18010,
        &OwnPorts::default(),
        &[],
    )
    .expect("a warning");
    assert_eq!(
        line,
        busy_line(
            &PortClaim {
                service: "ocs".into(),
                port: 18010
            },
            Some(18011)
        )
    );
    assert!(
        line.contains("`chap-devops components enable ocs --port 18011`"),
        "{line}"
    );
}

/// The other half of what `init` covers: a port no listener holds, but the
/// deployment next door publishes. Nothing is wrong today, which is exactly
/// why nothing else would catch it until the second `chap-devops up`.
#[test]
fn a_component_port_another_deployment_publishes_is_reported_too() {
    let (_dir, project) = project();
    let claim = PortClaim {
        service: "ocs".into(),
        port: 18010,
    };
    let neighbour = Deployment {
        dir: PathBuf::from("/t/hello1"),
        claims: vec![claim.clone()],
    };
    let line = component_port_line(
        &project,
        Component::Ocs,
        18010,
        &|_| false,
        &OwnPorts::default(),
        std::slice::from_ref(&neighbour),
    )
    .expect("a warning");
    assert_eq!(line, claimed_line(&claim, &[&neighbour], Some(18011)));

    // A deployment that publishes some other port is no one's problem.
    assert_eq!(
        component_port_line(
            &project,
            Component::Ocs,
            18011,
            &|_| false,
            &OwnPorts::default(),
            std::slice::from_ref(&neighbour)
        ),
        None
    );
}

/// The port our own running OCS is listening on is ours, so re-running
/// `components enable ocs` with the port it already has must say nothing -
/// while a port it is being *moved* onto is a conflict however much of this
/// deployment is up.
#[test]
fn a_component_keeps_the_port_its_own_running_service_holds() {
    let (_dir, mut project) = project();
    project.state.components.ocs.enabled = true;
    project.state.components.ocs.port = Some(18010);
    let running = OwnPorts::services(&["ocs"]);

    assert_eq!(
        component_port_line(&project, Component::Ocs, 18010, &|_| true, &running, &[]),
        None,
        "that listener is our own instance"
    );
    assert!(
        component_port_line(
            &project,
            Component::Ocs,
            18011,
            &|port| port == 18011,
            &running,
            &[]
        )
        .is_some(),
        "a port we are moving onto is somebody else's listener"
    );
    // And with nothing of ours up, our own recorded port is contested like
    // any other.
    assert!(
        component_port_line(
            &project,
            Component::Ocs,
            18010,
            &|_| true,
            &OwnPorts::default(),
            &[]
        )
        .is_some()
    );
}

/// chap-core publishes the API port, which `--api-port` moves, and port 0
/// is a question [`is_busy`] has no answer to.
#[test]
fn a_component_with_no_host_port_of_its_own_is_never_warned_about() {
    let (_dir, project) = project();
    let all_busy = |_: u16| true;
    assert_eq!(
        component_port_line(
            &project,
            Component::ChapCore,
            18010,
            &all_busy,
            &OwnPorts::default(),
            &[]
        ),
        None
    );
    assert_eq!(
        component_port_line(
            &project,
            Component::S3,
            0,
            &all_busy,
            &OwnPorts::default(),
            &[]
        ),
        None
    );
}

#[test]
fn a_running_service_keeps_its_own_port() {
    let (_dir, project) = project();
    let claims = claims(&project);
    let all_busy = |_: u16| true;

    assert_eq!(
        busy_claims(&claims, &OwnPorts::default(), &all_busy).len(),
        2,
        "nothing of ours is up, so both conflicts are real"
    );
    let running = OwnPorts::services(&["chap"]);
    let busy = busy_claims(&claims, &running, &all_busy);
    assert_eq!(busy.len(), 1);
    assert_eq!(busy[0].service, "loud");

    let nothing_busy = |_: u16| false;
    assert!(busy_claims(&claims, &OwnPorts::default(), &nothing_busy).is_empty());
}

/// A restore of `.chap-devops/components.yaml` can move a running DHIS2 from 8781
/// to 8780. The recreate publishes 8780, so the check asks about 8780 and
/// does not let the container on 8781 vouch for it.
#[test]
fn a_running_service_is_checked_on_the_port_the_recreate_moves_it_to() {
    let (_dir, mut project) = project();
    project.state.components.dhis2.enabled = true;
    project.state.components.dhis2.port = Some(18780);
    let claims = claims(&project);
    let all_busy = |_: u16| true;

    let moved = OwnPorts::with(&[("chap", &[8123]), ("dhis2", &[18781])]);
    let busy = busy_claims(&claims, &moved, &all_busy);
    let ports: Vec<u16> = busy.iter().map(|claim| claim.port).collect();
    assert!(ports.contains(&18780), "{busy:?}");
    assert!(!ports.contains(&8123), "{busy:?}");

    let kept = OwnPorts::with(&[("chap", &[8123]), ("dhis2", &[18780])]);
    let busy = busy_claims(&claims, &kept, &all_busy);
    assert!(!busy.iter().any(|claim| claim.port == 18780), "{busy:?}");
}

/// `components enable dhis2 --port 18781` while this deployment's own DHIS2
/// runs on 18781: the listener is ours, even when `.chap-devops/components.yaml`
/// records another port.
#[test]
fn a_component_moved_onto_the_port_its_own_container_publishes_is_not_warned_about() {
    let (_dir, mut project) = project();
    project.state.components.dhis2.enabled = true;
    project.state.components.dhis2.port = Some(18780);
    let own = OwnPorts::with(&[("dhis2", &[18781])]);
    assert_eq!(
        component_port_line(&project, Component::Dhis2, 18781, &|_| true, &own, &[]),
        None
    );
    // Another service of ours on that port does not make it free for DHIS2.
    let own = OwnPorts::with(&[("ocs", &[18781])]);
    assert!(component_port_line(&project, Component::Dhis2, 18781, &|_| true, &own, &[]).is_some());
}

#[test]
fn the_api_message_names_a_free_port_when_one_was_found() {
    let claim = PortClaim {
        service: "chap".into(),
        port: 8000,
    };
    let line = busy_line(&claim, Some(8010));
    assert!(line.starts_with(
        "port 8000 is already in use on this machine (needed by chap); free it, or set"
    ));
    // Never `init --api-port --force`, which cannot move a port `.env` sets.
    assert!(!line.contains("--api-port"), "{line}");
    assert!(line.contains("set CHAP_API_PORT=8010 in `.env`"));

    // Without a suggestion the placeholder stays: naming a port we have
    // not probed would be a guess.
    assert!(busy_line(&claim, None).contains("CHAP_API_PORT=<free>"));
}

#[test]
fn a_model_message_points_at_expose_and_unexpose() {
    let claim = PortClaim {
        service: "chapkit-ewars-model".into(),
        port: 5001,
    };
    let line = busy_line(&claim, Some(8010));
    assert!(line.contains("(needed by chapkit-ewars-model)"));
    assert!(line.contains("`chap-devops models unexpose chapkit-ewars-model`"));
    assert!(line.contains("`chap-devops models expose chapkit-ewars-model --port auto`"));
    assert!(!line.contains("--api-port"), "{line}");
}

#[test]
fn the_preflight_message_counts_and_offers_the_escape_hatch() {
    let claim = |service: &str, port: u16| PortClaim {
        service: service.into(),
        port,
    };
    let one = preflight_message(&[Conflict {
        claim: claim("chap", 8000),
        suggestion: Some(8001),
        holders: Vec::new(),
    }]);
    assert!(
        one.starts_with(
            "1 host port this deployment needs is already in use; nothing was started\n"
        )
    );
    assert!(one.contains("CHAP_API_PORT=8001"), "{one}");
    assert!(one.ends_with("or run `chap-devops up --no-preflight` to hand the conflict to Docker"));

    let two = preflight_message(&[
        Conflict {
            claim: claim("chap", 8000),
            suggestion: None,
            holders: Vec::new(),
        },
        Conflict {
            claim: claim("loud", 5001),
            suggestion: None,
            holders: Vec::new(),
        },
    ]);
    assert!(two.starts_with("2 host ports this deployment needs are already in use"));
    assert_eq!(two.lines().count(), 4);
    assert!(two.contains("\n  port 5001 is already in use"));
}

/// A busy port another chap-devops deployment publishes names that deployment,
/// the command that stops it, and a real free port for a component.
#[test]
fn a_port_held_by_another_deployment_names_it_and_a_free_port() {
    let other = Deployment {
        dir: PathBuf::from("/srv/oa"),
        claims: vec![PortClaim {
            service: "ocs".into(),
            port: 9000,
        }],
    };
    let text = preflight_message(&[Conflict {
        claim: PortClaim {
            service: "ocs".into(),
            port: 9000,
        },
        suggestion: Some(9001),
        holders: vec![&other],
    }]);
    assert!(text.contains("oa (/srv/oa) publishes it too"), "{text}");
    assert!(text.contains("`chap-devops -C /srv/oa down`"), "{text}");
    assert!(
        text.contains("`chap-devops components enable ocs --port 9001`"),
        "{text}"
    );
    assert!(!text.contains("<free>"), "{text}");
}

/// Only a deployment whose service runs holds the port, and the line about
/// several of them agrees with the number.
#[test]
fn a_stopped_deployment_does_not_hold_a_port_and_several_publish_it() {
    let at = |name: &str| Deployment {
        dir: PathBuf::from(format!("/srv/{name}")),
        claims: vec![PortClaim {
            service: "dhis2".into(),
            port: 8780,
        }],
    };
    let a = at("a");
    let running = BTreeSet::from(["dhis2".to_string()]);
    assert!(a.holds_now(8780, &running));
    assert!(!a.holds_now(8780, &BTreeSet::new()));
    assert!(!a.holds_now(8781, &running));

    let claim = PortClaim {
        service: "dhis2".into(),
        port: 8780,
    };
    let one = held_line(&claim, &[&a], None);
    assert!(
        one.contains("a (/srv/a) publishes it too; stop it with"),
        "{one}"
    );
    let b = at("b");
    let two = held_line(&claim, &[&a, &b], None);
    assert!(
        two.contains("a (/srv/a), b (/srv/b) publish it too; stop it with"),
        "{two}"
    );
}

/// A deployment directory with nothing in it but the state files that
/// make it one: `api_port` recorded, and an optional `.env` line over it.
fn deployment(parent: &Path, name: &str, recorded: u16, env: Option<u16>) -> PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(dir.join(crate::project::CHAP_DEVOPS_DIR)).unwrap();
    let state = ProjectState {
        api_port: recorded,
        ..ProjectState::default()
    };
    std::fs::write(
        dir.join(crate::project::CHAP_DEVOPS_DIR)
            .join(crate::project::PROJECT_FILE),
        serde_yaml_ng::to_string(&state).unwrap(),
    )
    .unwrap();
    if let Some(port) = env {
        std::fs::write(
            dir.join(crate::project::ENV_FILE),
            format!("POSTGRES_DB=chap_core\n{API_PORT_ENV_VAR}={port}\n"),
        )
        .unwrap();
    }
    dir
}

/// No docker to ask.
fn no_docker() -> Option<String> {
    None
}

/// One entry of `docker compose ls -a --format json`: `ConfigFiles` is
/// the comma-separated list of absolute paths docker prints, built from
/// the paths themselves so the parser is handed the host's own
/// separators, and serialized by serde, since a Windows path pasted into
/// a JSON literal is not JSON at all (`\U` is no escape).
fn ls_entry(name: &str, dir: &Path, files: &[&str]) -> serde_json::Value {
    let config_files: Vec<String> = files
        .iter()
        .map(|file| dir.join(file).display().to_string())
        .collect();
    serde_json::json!({
        "Name": name,
        "Status": "exited(0)",
        "ConfigFiles": config_files.join(","),
    })
}

#[test]
fn a_windows_verbatim_prefix_comes_off_before_two_paths_are_compared() {
    // String work either way, so what `canonicalize` answers on Windows
    // is checked on every host.
    assert_eq!(
        plain(PathBuf::from(r"\\?\C:\work\hello1")),
        PathBuf::from(r"C:\work\hello1")
    );
    assert_eq!(
        plain(PathBuf::from(r"\\?\UNC\server\share\hello1")),
        PathBuf::from(r"\\server\share\hello1")
    );
    // A path that never had one is handed back as it came.
    let already = PathBuf::from("/work/hello1");
    assert_eq!(plain(already.clone()), already);
}

#[test]
fn a_deployment_is_named_by_the_resolved_directory_and_a_missing_one_as_it_came() {
    let home = tempfile::tempdir().unwrap();
    let dir = deployment(home.path(), "hello1", 8000, None);
    assert_eq!(resolved(&dir), plain(dir.canonicalize().unwrap()));
    // Nothing to resolve, so nothing is changed: a name is better than
    // none.
    let gone = home.path().join("nope");
    assert_eq!(resolved(&gone), gone);
}

#[test]
fn the_deployments_beside_a_new_one_are_found_and_read_like_any_project() {
    let home = tempfile::tempdir().unwrap();
    // One claims the port in `.chap-devops/project.yaml`, the other overrides a
    // different recorded port from `.env` - which is the port that would
    // really be published, so it is the one that has to be found.
    let recorded = deployment(home.path(), "hello1", 8000, None);
    let overridden = deployment(home.path(), "hello2", 9999, Some(8000));
    // Not a deployment, and not a directory: neither is ours.
    std::fs::create_dir_all(home.path().join("notes")).unwrap();
    std::fs::write(home.path().join("README"), "hi").unwrap();

    let found = other_deployments(&home.path().join("hello3"), &no_docker);
    let dirs: Vec<&PathBuf> = found.iter().map(|d| &d.dir).collect();
    let (recorded, overridden) = (resolved(&recorded), resolved(&overridden));
    assert_eq!(dirs, vec![&recorded, &overridden], "{found:?}");
    assert!(found.iter().all(|d| d.holds(8000)), "{found:?}");
    assert_eq!(found[0].name(), "hello1");
    assert!(!found[1].holds(9999), "the recorded port is not published");
}

#[test]
fn a_deployment_never_counts_as_another_one_of_itself() {
    let home = tempfile::tempdir().unwrap();
    let one = deployment(home.path(), "hello1", 8000, None);
    // `init --force` over a deployment that is already there: its own old
    // files are on disk, and docker remembers it too.
    let json = serde_json::json!([ls_entry(
        "hello1",
        &one,
        &["compose.yml", CHAP_DEVOPS_COMPOSE]
    )])
    .to_string();
    let docker = || Some(json.clone());
    assert!(
        other_deployments(&one, &docker).is_empty(),
        "the deployment being written is not another deployment"
    );
}

#[test]
fn docker_known_deployments_are_found_and_deduplicated() {
    let home = tempfile::tempdir().unwrap();
    // Beside the new directory, so both searches find it.
    let sibling = deployment(home.path(), "hello1", 8000, None);
    // Somewhere else entirely: only docker knows about this one.
    let elsewhere = tempfile::tempdir().unwrap();
    let far = deployment(elsewhere.path(), "hello2", 8000, None);
    let json = serde_json::json!([
        ls_entry("hello1", &sibling, &["compose.yml", CHAP_DEVOPS_COMPOSE]),
        ls_entry("hello2", &far, &[CHAP_DEVOPS_COMPOSE]),
        ls_entry(
            "something-else",
            &home.path().join("other"),
            &["docker-compose.yml"]
        ),
        ls_entry("deleted", &home.path().join("gone"), &[CHAP_DEVOPS_COMPOSE]),
    ])
    .to_string();
    let found = other_deployments(&home.path().join("hello3"), &|| Some(json.clone()));
    let names: Vec<String> = found.iter().map(Deployment::name).collect();
    assert_eq!(
        names,
        vec!["hello1", "hello2"],
        "a sibling docker also knows is one deployment; a compose project \
             with no compose.chap-devops.yml is not ours, and a directory that is gone \
             has nothing left to warn about"
    );
}

#[test]
fn a_directory_that_is_not_a_project_contributes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let empty = home.path().join("hello1");
    std::fs::create_dir_all(empty.join(crate::project::CHAP_DEVOPS_DIR)).unwrap();
    std::fs::write(
        empty
            .join(crate::project::CHAP_DEVOPS_DIR)
            .join(crate::project::PROJECT_FILE),
        "nonsense: [\n",
    )
    .unwrap();
    assert!(
        other_deployments(&home.path().join("hello2"), &no_docker).is_empty(),
        "an unreadable project.yaml is not a claim on anything"
    );
}

#[test]
fn a_claimed_port_names_the_deployment_and_the_way_out() {
    let claim = PortClaim {
        service: "chap".into(),
        port: 8000,
    };
    let hello1 = Deployment {
        dir: PathBuf::from("/Users/x/t/hello1"),
        claims: vec![claim.clone()],
    };
    let line = claimed_line(&claim, &[&hello1], Some(8001));
    assert_eq!(
        line,
        "port 8000 is also used by hello1 (/Users/x/t/hello1), which is not running; \
             run only one of them at a time, or set CHAP_API_PORT=8001 in `.env`"
    );
    // The same ways out as the live-listener line, so the two read alike.
    assert!(busy_line(&claim, Some(8001)).ends_with("set CHAP_API_PORT=8001 in `.env`"));
}

#[test]
fn several_deployments_are_named_three_at_a_time_and_then_counted() {
    let claim = PortClaim {
        service: "ocs".into(),
        port: 9000,
    };
    let held: Vec<Deployment> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|name| Deployment {
            dir: Path::new("/t").join(name),
            claims: vec![claim.clone()],
        })
        .collect();
    // The line prints the directory the way the host spells it, so the
    // expectation is built from the same paths rather than from `/t/a`.
    let named = |i: usize| format!("{} ({})", held[i].name(), held[i].dir.display());

    let two: Vec<&Deployment> = held.iter().take(2).collect();
    let line = claimed_line(&claim, &two, Some(9001));
    assert!(
        line.starts_with(&format!(
            "port 9000 is also used by {}, {}, which are not running; \
                 run only one of them at a time,",
            named(0),
            named(1),
        )),
        "{line}"
    );
    // A component's port moves with the command that set it, to the free
    // port found above this one.
    assert!(
        line.ends_with("at a time, or run `chap-devops components enable ocs --port 9001`"),
        "{line}"
    );

    let all: Vec<&Deployment> = held.iter().collect();
    let line = claimed_line(&claim, &all, None);
    assert!(
        line.contains(&format!(
            "{}, {}, {} and 2 more, which are not running",
            named(0),
            named(1),
            named(2),
        )),
        "{line}"
    );
}

/// A component asked onto a port one of this deployment's own models
/// publishes: whether or not the model is up, `chap-devops up` cannot start both.
#[test]
fn a_component_on_a_port_its_own_model_publishes_is_warned_about() {
    let (_dir, project) = project();
    let line = component_port_line(
        &project,
        Component::Ocs,
        5001,
        &|_| false,
        &OwnPorts::default(),
        &[],
    )
    .expect("a warning");
    assert!(line.contains("also published by loud"), "{line}");
    assert!(line.contains("--port 5002"), "{line}");
}

#[test]
fn a_printed_path_has_no_dot_dot_in_it() {
    let dir = tempfile::tempdir().unwrap();
    let real = plain(dir.path().canonicalize().unwrap());
    std::fs::create_dir_all(dir.path().join("chapx")).unwrap();
    std::fs::create_dir_all(dir.path().join("archives")).unwrap();
    // A parent that exists is resolved, and the name is put back on.
    assert_eq!(
        real_path(&dir.path().join("chapx/../archives/a.tar.gz")),
        real.join("archives").join("a.tar.gz")
    );
    // Nothing on disk to resolve against: the text decides.
    assert_eq!(
        real_path(&dir.path().join("chapx/../gone/./new/a.tar.gz")),
        lexical(&dir.path().join("gone/new/a.tar.gz"))
    );
    assert_eq!(
        lexical(Path::new("/srv/chapx/../archives/./a")),
        PathBuf::from("/srv/archives/a")
    );
}
