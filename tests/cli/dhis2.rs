use crate::common::*;
use crate::dhis2_stand_in::*;
#[cfg(unix)]
use predicates::prelude::{Predicate, PredicateBooleanExt};
use serde_json::Value as Json;
use std::path::PathBuf;
use tempfile::TempDir;

mod answers;
mod external;

/// The `dhis2` row, out of a `chap-devops status --json` document.
#[cfg(unix)]
fn dhis2_row(report: &Json) -> &Json {
    report["components"]
        .as_array()
        .expect("a component list")
        .iter()
        .find(|row| row["name"] == "dhis2")
        .expect("a dhis2 row")
}

/// A DHIS2 whose container is up and whose `/api/ping` answers is the one shape
/// that counts as `up`.
#[cfg(unix)]
#[test]
fn status_calls_dhis2_up_once_api_ping_answers() {
    let port = dhis2_lookalike();
    let (sandbox, dir) = dhis2_sandbox(port);
    let (_temp, bin) = docker_running("dhis2");

    let mut status = chap_with_docker(
        &sandbox,
        &dir,
        &bin,
        &["status", "--json", "--timeout", "2"],
    );
    status.env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1");
    let out = status.assert().get_output().stdout.clone();
    let report: Json = serde_json::from_slice(&out).expect("status --json is one document");
    let row = dhis2_row(&report);
    assert_eq!(row["state"], "up", "{report}");
    assert_eq!(row["reach"], format!("http://localhost:{port}"));
    assert_eq!(
        row["health_url"],
        format!("http://localhost:{port}/api/ping")
    );
    // None of the OCS-only fields turns up on it.
    assert_eq!(row["datasets"], Json::Null);
    assert_eq!(row["read_only"], false);

    // And the human line says the same.
    let mut status = chap_with_docker(&sandbox, &dir, &bin, &["status", "--timeout", "2"]);
    status.env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1");
    let out = status.assert().get_output().stdout.clone();
    let text = String::from_utf8_lossy(&out).into_owned();
    assert!(
        // Padded to chap-core's `down`, so the two addresses line up.
        text.contains(&format!("dhis2       up     http://localhost:{port}")),
        "{text}"
    );
}

/// Two deployments made with the same ports take turns on them, and the one
/// that is up answers on the port for both. With this deployment's `chap`
/// container not running, a chap-core answering on its port is someone
/// else's: `chap-devops status` says this one is not running rather than `up`.
#[cfg(unix)]
#[test]
fn status_does_not_claim_another_deployments_chap_core() {
    let dhis2_port = dhis2_lookalike();
    let api_port = chap_core_lookalike();
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--api-port", &api_port.to_string()])
        .assert()
        .success();
    sandbox
        .components(&["enable", "dhis2", "--port", &dhis2_port.to_string()])
        .assert()
        .success();
    let (_temp, bin) = docker_running("dhis2");
    let mut cmd = chap_with_docker(&sandbox, &dir, &bin, &["status", "--timeout", "2"]);
    cmd.env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1");
    let out = cmd.assert().failure().get_output().clone();
    let text = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("chap-core   down"), "{text}");
    assert!(
        err.contains("this deployment's chap-core is not running"),
        "{err}"
    );
}

/// The gap this closes. A deployment can sit for good with chap-core `up`, the
/// `dhis2` row `up` and the Modeling App unable to reach Chap at all, because
/// the only line that ever named `chap-devops dhis2 connect` was printed minutes
/// earlier, when the component was added and DHIS2 did not exist yet.
///
/// So `chap-devops status` says it under its verdict while `.chap-devops/components.yaml`
/// records no connect - and stops the moment one is recorded. It is local
/// knowledge and no request: what chap-devops has recorded, never what DHIS2 is.
#[cfg(unix)]
#[test]
fn status_names_the_connect_a_running_dhis2_has_not_had() {
    let dhis2_port = dhis2_lookalike();
    let api_port = chap_core_lookalike();
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--api-port", &api_port.to_string()])
        .assert()
        .success();
    sandbox
        .components(&["enable", "dhis2", "--port", &dhis2_port.to_string()])
        .assert()
        .success();
    // chap-core's own container running too: the lookalike on its port is
    // then this deployment's chap-core, not another one's.
    let (_temp, bin) = docker_running_all(&["chap", "dhis2"]);
    let status = |sandbox: &Sandbox| {
        let mut cmd = chap_with_docker(sandbox, &dir, &bin, &["status", "--timeout", "2"]);
        cmd.env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1");
        String::from_utf8_lossy(&cmd.assert().get_output().stdout).into_owned()
    };

    let text = status(&sandbox);
    assert!(
        text.contains(&format!("dhis2       up   http://localhost:{dhis2_port}")),
        "{text}"
    );
    assert!(
        text.contains(
            "chap-devops has not connected this DHIS2 to Chap; run `chap-devops dhis2 connect`"
        ),
        "{text}"
    );
    // Under the verdict, with the model hints, rather than on the row.
    let verdict = text.find("no models enabled").expect(&text);
    let hint = text.find("chap-devops has not connected").expect(&text);
    assert!(hint > verdict, "{text}");

    // A record of a connect stops it. Written here the way `chap-devops dhis2
    // connect` writes it, because this test has no DHIS2 to connect to.
    let components = dir.join(".chap-devops").join("components.yaml");
    let body = std::fs::read_to_string(&components).expect("components.yaml");
    std::fs::write(
        &components,
        body.replace("connected_at: null", "connected_at: 2026-09-27T09:12:33Z"),
    )
    .expect("the record");

    let text = status(&sandbox);
    assert!(
        text.contains(&format!("dhis2       up   http://localhost:{dhis2_port}")),
        "{text}"
    );
    assert!(!text.contains("chap-devops dhis2 connect"), "{text}");
}

/// The failure the request exists to catch. A DHIS2 container reports itself
/// healthy while its Spring context has failed and every `/api/*` request 404s,
/// so the container alone is not evidence of anything: `up` is the one answer
/// this must not give.
#[cfg(unix)]
#[test]
fn status_does_not_call_a_dhis2_up_that_serves_pages_but_no_api() {
    let port = dhis2_without_an_api();
    let (sandbox, dir) = dhis2_sandbox(port);
    let (_temp, bin) = docker_running("dhis2");

    let mut status = chap_with_docker(
        &sandbox,
        &dir,
        &bin,
        &["status", "--json", "--timeout", "2"],
    );
    status.env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1");
    let out = status.assert().get_output().stdout.clone();
    let report: Json = serde_json::from_slice(&out).expect("status --json is one document");
    let row = dhis2_row(&report);
    assert_ne!(row["state"], "up", "{report}");
    assert_eq!(row["state"], "starting", "{report}");
    // The address is recorded state, so it says the same thing either way.
    assert_eq!(
        row["health_url"],
        format!("http://localhost:{port}/api/ping")
    );

    let mut status = chap_with_docker(&sandbox, &dir, &bin, &["status", "--timeout", "2"]);
    status.env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1");
    let out = status.assert().get_output().stdout.clone();
    let text = String::from_utf8_lossy(&out).into_owned();
    assert!(text.contains("dhis2       starting"), "{text}");
    assert!(!text.contains("dhis2       up"), "{text}");
}

/// A deployment that is DHIS2 and nothing else: the row is there, with the
/// address it will answer on, before anything has ever been started.
#[test]
fn status_json_carries_the_dhis2_row_on_a_dhis2_only_deployment() {
    let port = dhis2_lookalike();
    let (sandbox, dir) = dhis2_sandbox(port);
    sandbox
        .components(&["disable", "chap-core"])
        .assert()
        .success();

    let out = sandbox
        .chap()
        .arg("-C")
        .arg(&dir)
        .args(["status", "--json", "--timeout", "2"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let report: Json = serde_json::from_slice(&out).expect("status --json is one document");
    let components = report["components"].as_array().expect("a component list");
    assert_eq!(components.len(), 1, "{report}");
    let row = &components[0];
    assert_eq!(row["name"], "dhis2");
    // Nothing this deployment owns is running, so nothing was asked - and the
    // stand-in holding that port did not become this deployment's answer.
    assert_eq!(row["state"], "not-running", "{report}");
    assert_eq!(
        row["health_url"],
        format!("http://localhost:{port}/api/ping")
    );
    assert_eq!(row["reach"], format!("http://localhost:{port}"));
    assert_eq!(report["api"]["state"], "off", "{report}");
}

/// The three things `chap-devops doctor` has to say about a DHIS2 deployment: the
/// component and its seed, the image it pulls, and the one file without which
/// DHIS2 will not start at all.
#[test]
fn doctor_reports_the_dhis2_component_its_image_and_a_missing_config() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&[
            "--models",
            "none",
            "--with",
            "dhis2",
            "--dhis2-port",
            &free_port().to_string(),
            "--api-port",
            &free_port().to_string(),
        ])
        .assert()
        .success();

    let out = sandbox
        .chap()
        .arg("-C")
        .arg(&dir)
        .arg("doctor")
        .assert()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out).into_owned();
    // The scaffolded file is there, and the seed is reported rather than judged.
    assert!(
        text.contains("chap-core, dhis2; dhis2/dhis.conf present; seed: default ("),
        "{text}"
    );
    // The image the component pulls, at the tag it records. Offline this is a
    // skip, which still proves the line is asked for.
    assert!(text.contains("image dhis2"), "{text}");
    assert!(text.contains("port dhis2"), "{text}");

    // Without `dhis2/dhis.conf` DHIS2 throws on startup, so its absence is a
    // fault with `chap-devops sync` as the way out.
    std::fs::remove_file(dir.join("dhis2").join("dhis.conf")).expect("the scaffolded config");
    let assert = sandbox
        .chap()
        .arg("-C")
        .arg(&dir)
        .arg("doctor")
        .assert()
        .failure();
    let text = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        text.contains("fail  components") && text.contains("dhis2/dhis.conf is missing"),
        "{text}"
    );
    assert!(
        text.contains("run `chap-devops sync` to scaffold"),
        "{text}"
    );
}

/// A DHIS2 added or kept after `init` picks its version with
/// `components enable dhis2 --tag`, and an unreleased one with `--image`.
#[test]
fn components_enable_dhis2_picks_its_version() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();
    sandbox
        .components(&["enable", "dhis2", "--tag", "2.43"])
        .assert()
        .success();
    let compose = read(&dir.join("compose.dhis2.yml"));
    assert!(
        compose.contains("dhis2/core:${DHIS2_IMAGE_TAG:-2.43}"),
        "{compose}"
    );

    sandbox
        .components(&[
            "enable",
            "dhis2",
            "--image",
            "dhis2/core-dev",
            "--tag",
            "master",
        ])
        .assert()
        .success();
    let compose = read(&dir.join("compose.dhis2.yml"));
    assert!(
        compose.contains("dhis2/core-dev:${DHIS2_IMAGE_TAG:-master}"),
        "{compose}"
    );

    sandbox
        .components(&["enable", "dhis2", "--image", "dhis2/core-dev:master"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "`--image dhis2/core-dev --tag master`",
        ));
    sandbox
        .components(&["enable", "ocs", "--tag", "2.43"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("DHIS2 settings"));
}

/// `chap-devops update` re-pulls every enabled component's image, DHIS2's among them,
/// and says so before it pulls anything.
///
/// The warning beside it is the part DHIS2 needs and the others do not. Nothing
/// in the plan moves - `dhis2/core:2.42` is a minor line, so the same tag is a
/// newer patch release tomorrow - and a newer DHIS2 migrates `dhis2_db` forward
/// only, so the line names the archive that makes it recoverable.
#[cfg(unix)]
#[test]
fn update_lists_the_dhis2_image_and_warns_about_the_migration() {
    let (sandbox, dir, port, _temp, bin) = pinned_sandbox();
    sandbox
        .components(&["enable", "dhis2", "--port", &free_port().to_string()])
        .assert()
        .success();
    assert!(dir.join("compose.dhis2.yml").is_file());

    let assert = online_update(&sandbox, port, &bin, &["--dry-run"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "dhis2      2.42  moving tag, would be re-pulled",
        ))
        // No pin moves: the component follows a moving tag, so the plan says
        // nothing would change and the warning beside it is the whole point.
        .stdout(predicates::str::contains("already up to date"));
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    // The fake docker answers every `volume inspect` with success, so `dhis2_db`
    // is there as far as this run can tell - which is when the line is earned.
    assert!(stderr.contains("`dhis2/core:2.42`"), "{stderr}");
    assert!(stderr.contains("chap-devops backup create"), "{stderr}");
    assert!(stderr.contains("404s"), "{stderr}");
    // Not the `components enable dhis2` wording, which names two tags.
    assert!(!stderr.contains("moves from"), "{stderr}");

    // A pin an operator set by hand is re-pulled at that tag, and said to be.
    let env = read(&dir.join(".env"));
    std::fs::write(dir.join(".env"), format!("{env}\nDHIS2_IMAGE_TAG=2.41\n"))
        .expect("an .env with a pin in it");
    online_update(&sandbox, port, &bin, &["--dry-run"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "dhis2      2.41  pinned in .env, would be re-pulled at that tag",
        ));
}

/// No route at all: one is created, and the report says so and proves the path
/// through it. `--offline` is the whole run: nothing here needs the network.
#[cfg(unix)]
#[test]
fn dhis2_route_creates_the_route_when_there_is_none() {
    let stand_in = Dhis2StandIn::new();
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, None, &["route", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "created the `chap` route at http://chap:8000/**; chap-core answered through it",
        ))
        .stdout(predicates::str::contains("hint: chap-core said: healthy"))
        .stdout(predicates::str::contains(
            "DHIS2 2.42.6 at http://localhost:",
        ))
        .stdout(predicates::str::contains(
            "hint: the credential is the DHIS2 default password",
        ));

    // The payload is the one that works, and the target is the compose alias.
    let route = stand_in.route().expect("a route was written");
    assert_eq!(route["code"], "chap");
    assert_eq!(route["url"], "http://chap:8000/**");
    assert_eq!(route["authorities"][0], "F_CHAP_MODELING_APP");
    assert_eq!(route["headers"]["Content-Type"], "application/json");
    assert_eq!(route["responseTimeoutSeconds"], 30);
    assert!(
        stand_in.was_asked("POST /api/routes"),
        "{:?}",
        stand_in.asked()
    );
    // The whole path was proved, not just the row.
    assert!(
        stand_in.was_asked("GET /api/routes/chap/run/health"),
        "{:?}",
        stand_in.asked()
    );
    // And the credentials went out as HTTP Basic for admin:district.
    assert_eq!(stand_in.authorization(), "Basic YWRtaW46ZGlzdHJpY3Q=");
}

/// A chap-core with an API token: the route carries the token in its `auth`,
/// the check goes past the open `/health`, and a route written before the token
/// existed is rewritten rather than left reaching `/health` only.
#[cfg(unix)]
#[test]
fn dhis2_route_carries_chap_cores_token() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        chap_token: Some("s3cret".to_string()),
        route: Some(serde_json::json!({
            "id": "route-old", "code": "chap", "url": "http://chap:8000/**",
            "authorities": ["F_CHAP_MODELING_APP"],
        })),
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);
    let env = dir.join(".env");
    let mut text = std::fs::read_to_string(&env).unwrap_or_default();
    text.push_str("CHAP_API_TOKEN=s3cret\n");
    std::fs::write(&env, text).expect("write .env");

    dhis2_chap(&sandbox, &dir, &bin, None, &["route", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: the route was rewritten because it carried no chap-core API token",
        ))
        .stdout(predicates::str::contains("hint: chap-core said: healthy"))
        .stdout(predicates::str::contains("s3cret").not());

    let route = stand_in.route().expect("the route");
    assert_eq!(route["auth"]["type"], "api-headers");
    assert_eq!(route["auth"]["headers"]["Authorization"], "Bearer s3cret");
    assert!(route["headers"].get("Authorization").is_none(), "{route}");
    assert!(
        stand_in.was_asked("GET /api/routes/chap/run/v2/services"),
        "{:?}",
        stand_in.asked()
    );

    // Now it is right, and a second run leaves it alone.
    dhis2_chap(&sandbox, &dir, &bin, None, &["route"])
        .assert()
        .success()
        .stdout(predicates::str::contains("already points at"));
}

/// A route carrying a token chap-core no longer takes: DHIS2 hides the value,
/// so `show` finds it by asking through the route, and names the command.
#[cfg(unix)]
#[test]
fn dhis2_show_names_a_route_whose_token_chap_core_refuses() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        chap_token: Some("new".to_string()),
        route: Some(serde_json::json!({
            "id": "route-old", "code": "chap", "url": "http://chap:8000/**",
            "authorities": ["F_CHAP_MODELING_APP"],
            "auth": {"type": "api-headers", "headers": {"Authorization": "Bearer old"}},
        })),
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);
    let env = dir.join(".env");
    let mut text = std::fs::read_to_string(&env).unwrap_or_default();
    text.push_str("CHAP_API_TOKEN=new\n");
    std::fs::write(&env, text).expect("write .env");

    dhis2_chap(&sandbox, &dir, &bin, None, &["show"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "the `chap` route does not carry chap-core's API token",
        ))
        .stdout(predicates::str::contains("chap-devops dhis2 connect"));

    dhis2_chap(&sandbox, &dir, &bin, None, &["route", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: the route was rewritten because chap-core refused the API token it carried",
        ));
    assert_eq!(
        stand_in.route().expect("the route")["auth"]["headers"]["Authorization"],
        "Bearer new"
    );
}

/// The trap the step exists for: the demo dumps ship a `chap` route aimed at an
/// external Chap server, with the right code and the right authority, so a
/// "create if absent" implementation would leave the deployment sending its
/// data to a stranger.
#[cfg(unix)]
#[test]
fn dhis2_route_repoints_a_route_that_points_at_another_chap_core() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        route: Some(external_chap_route()),
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, None, &["route", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "repointed the `chap` route at http://chap:8000/**",
        ))
        .stdout(predicates::str::contains(
            "hint: the route was rewritten because it pointed at http://158.39.75.126/stable/**",
        ));

    assert_eq!(
        stand_in.route().expect("the route")["url"],
        "http://chap:8000/**"
    );
    // Replaced in place, by id: the id is what DHIS2's own rows point at.
    assert!(
        stand_in.was_asked("PUT /api/routes/route-demo"),
        "{:?}",
        stand_in.asked()
    );
    assert!(
        !stand_in.was_asked("POST /api/routes"),
        "{:?}",
        stand_in.asked()
    );
}

/// A route that already points here is left alone - and said to be, because a
/// command that found nothing to do still reports.
#[cfg(unix)]
#[test]
fn dhis2_route_leaves_a_route_that_already_matches_alone() {
    let mut ours = external_chap_route();
    ours["url"] = serde_json::json!("http://chap:8000/**");
    let stand_in = Dhis2StandIn::with(Dhis2State {
        route: Some(ours),
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, None, &["route"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "the `chap` route already points at http://chap:8000/**; chap-core answered \
             through it",
        ));

    let asked = stand_in.asked();
    assert!(
        !asked.iter().any(|seen| seen.starts_with("POST /api/routes")
            || seen.starts_with("PUT /api/routes")),
        "{asked:?}"
    );
    // It is still verified: the point is whether the path works, not whether
    // this run wrote anything.
    assert!(
        stand_in.was_asked("GET /api/routes/chap/run/health"),
        "{asked:?}"
    );
}

/// The refusal `route.remote_servers_allowed` produces, measured: this is the
/// body a live DHIS2 2.42.6 answers with.
///
/// Up to and including 0.4.0 none of the three phrases `chap-devops` looked for -
/// `remote server`, `not allowed`, `allowlist` - were anywhere in it, so the
/// branch never fired and the operator got the passthrough:
/// `answered HTTP 409 Conflict: Route URL is not permitted`, which is true and
/// names neither the cause nor the file to change.
#[cfg(unix)]
#[test]
fn dhis2_route_names_the_allowlist_and_the_restart_that_applies_it() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        route_not_permitted: true,
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    let assert = dhis2_chap(&sandbox, &dir, &bin, None, &["route"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "only allow the origins `route.remote_servers_allowed` lists",
        ))
        .stderr(predicates::str::contains(
            "http://chap:8000 has to be one of them",
        ))
        .stderr(predicates::str::contains("`dhis2/dhis.conf`"))
        // A plain restart applies it: `restart` recreates a service whose
        // mounted config changed since it started.
        .stderr(predicates::str::contains("run `chap-devops restart dhis2`"));

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(!stderr.contains("--all"), "{stderr}");
    // And not the passthrough it used to be.
    assert!(!stderr.contains("answered HTTP 409"), "{stderr}");
    assert!(
        stand_in.was_asked("POST /api/routes"),
        "{:?}",
        stand_in.asked()
    );
}

/// A 401 says which two variables to set and names neither the password nor a
/// default that would be one.
#[cfg(unix)]
#[test]
fn dhis2_reports_what_to_fix_when_the_credentials_are_refused() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        unauthorized: true,
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    let assert = dhis2_chap(&sandbox, &dir, &bin, None, &["route"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "did not accept the password for `admin`",
        ))
        .stderr(predicates::str::contains("DHIS2_ADMIN_USERNAME"))
        .stderr(predicates::str::contains("DHIS2_ADMIN_PASSWORD"))
        .stderr(predicates::str::contains("CHAP_DEVOPS_DHIS2_PASSWORD"));
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(!stderr.contains("district"), "{stderr}");
    assert!(!stderr.contains("request failed"), "{stderr}");

    // `--user` names another user, and admin's password is not sent in that
    // user's name: nothing is asked until there is a password that is theirs.
    let asked = stand_in.asked().len();
    dhis2_chap(&sandbox, &dir, &bin, None, &["route", "--user", "ops"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "chap-devops has no password for DHIS2 user `ops`",
        ))
        .stderr(predicates::str::contains("CHAP_DEVOPS_DHIS2_PASSWORD"));
    assert_eq!(stand_in.asked().len(), asked, "{:?}", stand_in.asked());

    // One exported for the run is theirs, and the refusal then names them.
    dhis2_chap(&sandbox, &dir, &bin, None, &["route", "--user", "ops"])
        .env("CHAP_DEVOPS_DHIS2_PASSWORD", "theirs")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "did not accept the password for `ops` (password from CHAP_DEVOPS_DHIS2_PASSWORD)",
        ));
}

/// The analytics request is the one that was measured: tracked entities out,
/// and **no `lastYears`**, which wrote zero rows into every table while
/// reporting success.
#[cfg(unix)]
#[test]
fn dhis2_analytics_runs_the_request_that_populates_and_waits_for_it() {
    let stand_in = Dhis2StandIn::new();
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, None, &["analytics", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains("analytics finished in"))
        .stdout(predicates::str::contains(
            "hint: DHIS2 said: Analytics tables updated",
        ))
        // The stand-in is a UTC server; the time is on the clock of this machine.
        .stdout(
            predicates::str::is_match(
                r"hint: DHIS2 records its last analytics success as 2026-09-2\d \d\d:\d\d\n",
            )
            .unwrap(),
        );

    let asked = stand_in.asked();
    assert!(
        asked
            .iter()
            .any(|seen| seen == "POST /api/resourceTables/analytics?skipTrackedEntities=true"),
        "{asked:?}"
    );
    assert!(
        !asked.iter().any(|seen| seen.contains("lastYears")),
        "lastYears populates nothing: {asked:?}"
    );
    // The job was polled, by the id the answer carried.
    assert!(
        stand_in.was_asked("GET /api/system/tasks/ANALYTICS_TABLE/job-1"),
        "{asked:?}"
    );
}

/// DHIS2 runs one analytics job at a time, so a run that is already going is
/// watched rather than queued behind.
#[cfg(unix)]
#[test]
fn dhis2_analytics_watches_a_run_that_is_already_going() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        analytics_running: true,
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, None, &["analytics"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "the analytics run that was already going finished",
        ));

    let asked = stand_in.asked();
    assert!(
        !asked
            .iter()
            .any(|seen| seen.starts_with("POST /api/resourceTables/analytics")),
        "a second run would have queued behind the first: {asked:?}"
    );
    assert!(
        stand_in.was_asked("GET /api/system/tasks/ANALYTICS_TABLE/job-0"),
        "{asked:?}"
    );
}

/// `--no-wait` starts the run and says how to watch it, rather than sitting on
/// it for an hour.
#[cfg(unix)]
#[test]
fn dhis2_analytics_no_wait_starts_the_run_and_names_the_way_back() {
    let stand_in = Dhis2StandIn::new();
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        None,
        &["analytics", "--no-wait", "-v"],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains(
        "started the analytics run (job job-1)",
    ))
    .stdout(predicates::str::contains(
        "hint: run `chap-devops dhis2 analytics` again to watch the same run",
    ));

    assert!(
        !stand_in.was_asked("GET /api/system/tasks/ANALYTICS_TABLE/job-1"),
        "{:?}",
        stand_in.asked()
    );
}

/// The version id is resolved from the App Hub and DHIS2 is told to install it:
/// no download here and no multipart upload.
///
/// The App Hub stand-in answers in the shape the real one does, blank
/// `maxDhisVersion` strings and all, so this is also the regression test for
/// the bug where no app could ever be installed.
#[cfg(unix)]
#[test]
fn dhis2_apps_resolves_the_version_on_the_app_hub_and_has_dhis2_install_it() {
    let stand_in = Dhis2StandIn::new();
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);
    let hub = Some(stand_in.port);

    dhis2_chap(&sandbox, &dir, &bin, hub, &["apps", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains("installed Modeling App 7.1.0"))
        .stdout(predicates::str::contains(
            "installed DHIS2 Climate App 1.16.2",
        ))
        .stdout(predicates::str::contains(
            "hint: open DHIS2 with `chap-devops open dhis2`",
        ))
        // The sentence the blank bound produced, which was false.
        .stdout(predicates::str::contains("no version of").not());

    let asked = stand_in.asked();
    assert!(
        asked
            .iter()
            .any(|seen| seen == "GET /apphub/a29851f9-82a7-4ecd-8b2c-58e0f220bc75"),
        "{asked:?}"
    );
    assert!(
        asked.iter().any(|seen| seen == "POST /api/appHub/mv-7.1.0"),
        "{asked:?}"
    );
    assert!(
        asked
            .iter()
            .any(|seen| seen == "POST /api/appHub/cv-1.16.2"),
        "{asked:?}"
    );

    // And a second run installs nothing: the version this instance can run is
    // already there.
    dhis2_chap(&sandbox, &dir, &bin, hub, &["apps"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Modeling App 7.1.0 is already installed",
        ));
}

/// Installing needs the App Hub twice - here for the version, and from DHIS2
/// for the app - so `--offline` refuses it and says what it would have needed.
#[cfg(unix)]
#[test]
fn dhis2_apps_is_refused_offline_and_says_why() {
    let stand_in = Dhis2StandIn::new();
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, None, &["apps"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("App Hub"))
        .stderr(predicates::str::contains("`--offline`"))
        .stderr(predicates::str::contains("App Management"));

    // Refused before a single request: the deployment was never even asked.
    assert!(stand_in.asked().is_empty(), "{:?}", stand_in.asked());
}

/// `show` changes nothing and names each piece that is missing, plus the one
/// command that does all three.
#[cfg(unix)]
#[test]
fn dhis2_show_reports_every_piece_that_is_missing() {
    let stand_in = Dhis2StandIn::new();
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, None, &["show"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "missing: there is no `chap` route",
        ))
        .stdout(predicates::str::contains(
            "missing: analytics has never run",
        ))
        .stdout(predicates::str::contains(
            "missing: the Modeling App is not installed",
        ))
        // Both apps are a row, absent as much as present.
        .stdout(predicates::str::contains(
            "apps       Modeling App not installed, DHIS2 Climate App not installed",
        ))
        .stdout(predicates::str::contains(
            "run `chap-devops dhis2 connect` to do the rest",
        ));

    let asked = stand_in.asked();
    assert!(
        !asked.iter().any(|seen| seen.starts_with("POST")
            || seen.starts_with("PUT")
            || seen.starts_with("DELETE")),
        "show writes nothing: {asked:?}"
    );
}

/// The `apps` row is the two apps chap-devops installs, and never the instance's own
/// thirty. A real 2.42.6 lists 29 bundled apps, and they buried the answer.
#[cfg(unix)]
#[test]
fn dhis2_show_reports_only_the_two_apps_chap_devops_installs() {
    let bundled = |name: &str, key: &str, version: &str| serde_json::json!({"name": name, "key": key, "version": version, "bundled": true});
    let stand_in = Dhis2StandIn::with(Dhis2State {
        apps: vec![
            bundled("Reports", "reports", "100.2.4"),
            bundled("Cache Cleaner", "cache-cleaner", "100.2.2"),
            bundled("Maintenance app", "maintenance", "32.34.1-v42.0"),
            // The instance's own spelling of the Modeling App, which is the
            // App Hub's and not chap-devops'.
            serde_json::json!({"name": "Modeling", "key": "modeling", "version": "7.1.0"}),
        ],
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, None, &["show"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "apps       Modeling App 7.1.0, DHIS2 Climate App not installed",
        ))
        .stdout(predicates::str::contains(
            "missing: the Climate App is not installed",
        ))
        // DHIS2's own bundled apps are DHIS2's business.
        .stdout(predicates::str::contains("Reports").not())
        .stdout(predicates::str::contains("Cache Cleaner").not())
        .stdout(predicates::str::contains("100.2.4").not())
        // And no count of them, which would be a number that never means
        // anything.
        .stdout(predicates::str::contains("3 others").not());

    let report = json_of(&mut dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        None,
        &["show", "--json"],
    ));
    let apps = report["apps"].as_array().expect("an apps array");
    assert_eq!(apps.len(), 2, "{report}");
    assert_eq!(apps[0]["name"], "Modeling App");
    assert_eq!(apps[0]["installed"], true);
    assert_eq!(apps[0]["version"], "7.1.0");
    assert_eq!(apps[1]["name"], "DHIS2 Climate App");
    assert_eq!(apps[1]["installed"], false);
}

/// A seeded deployment inherits `lastAnalyticsTableSuccess` from the dump, so
/// the timestamp is a fact about somebody else's database.
///
/// Measured on a real 2.42.6 with the Laos climate demo: `show` reported a last
/// success of 2026-06-16 while `analytics_2024` did not exist at all - the
/// table was absent, not empty. chap-devops cannot look at the tables, so it says
/// what it checked and puts the question in front of the operator.
#[cfg(unix)]
#[test]
fn dhis2_show_does_not_call_an_inherited_analytics_timestamp_evidence() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        inherited_analytics: true,
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, None, &["show"])
        .assert()
        .success()
        .stdout(
            predicates::str::is_match(
                r"analytics  2026-06-1\d \d\d:\d\d \(unconfirmed on a seeded database\)",
            )
            .unwrap(),
        )
        .stdout(predicates::str::contains(
            "missing: analytics may never have run on this deployment",
        ))
        .stdout(predicates::str::contains("`chap-devops dhis2 analytics`"))
        // Not the sentence for an instance that has never run it: it may well
        // have, and chap-devops has no way to tell.
        .stdout(predicates::str::contains("analytics has never run").not());

    let report = json_of(&mut dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        None,
        &["show", "--json"],
    ));
    assert_eq!(report["analytics"], "unconfirmed", "{report}");
    // 07:51 on a UTC server, in the zone of this machine, with its offset.
    let last = report["last_analytics"].as_str().unwrap_or_default();
    assert!(
        predicates::str::is_match(r"^2026-06-1\dT\d\d:\d\d:00[+-]\d\d:\d\d$")
            .unwrap()
            .eval(last),
        "{last}"
    );

    // The notifier is what settles it, and `show` asks: a run it remembers is
    // a run that happened on this DHIS2, because the notifier is in the
    // process and nothing a dump carries can put an entry in it.
    assert!(
        stand_in.was_asked("GET /api/system/tasks/ANALYTICS_TABLE"),
        "{:?}",
        stand_in.asked()
    );
}

/// A database DHIS2 migrated from empty has no dump to inherit a timestamp
/// from, so what it records was recorded against it and stands as it is.
#[cfg(unix)]
#[test]
fn dhis2_show_trusts_the_timestamp_where_no_dump_could_have_brought_it() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        inherited_analytics: true,
        ..Dhis2State::default()
    });
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&[
            "--models",
            "none",
            "--api-port",
            &free_port().to_string(),
            "--with",
            "dhis2",
            "--dhis2-port",
            &stand_in.port.to_string(),
            "--dhis2-seed",
            "none",
        ])
        .assert()
        .success();
    let (_temp, bin) = docker_running("dhis2");

    dhis2_chap(&sandbox, &dir, &bin, None, &["show"])
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"analytics  2026-06-1\d \d\d:\d\d\n").unwrap())
        .stdout(predicates::str::contains("unconfirmed").not())
        .stdout(predicates::str::contains("missing: analytics").not());

    let report = json_of(&mut dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        None,
        &["show", "--json"],
    ));
    assert_eq!(report["analytics"], "recorded", "{report}");
}

/// Once a run has finished on this DHIS2, the same timestamp is worth
/// something and `show` says which of the two it is.
#[cfg(unix)]
#[test]
fn dhis2_show_credits_an_analytics_run_that_finished_on_this_deployment() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        inherited_analytics: true,
        analytics_started: true,
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, None, &["show"])
        .assert()
        .success()
        .stdout(
            predicates::str::is_match(
                r"analytics  2026-09-2\d \d\d:\d\d \(a run finished on this deployment\)",
            )
            .unwrap(),
        )
        .stdout(predicates::str::contains("unconfirmed").not())
        .stdout(predicates::str::contains("missing: analytics").not());
}

/// A route pointing at another chap-core is marked as such in the row, not only
/// in the list of what is missing.
#[cfg(unix)]
#[test]
fn dhis2_show_marks_a_route_that_points_somewhere_else() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        route: Some(external_chap_route()),
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    let report = json_of(&mut dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        None,
        &["show", "--json"],
    ));
    assert_eq!(report["route"]["url"], "http://158.39.75.126/stable/**");
    assert_eq!(report["route"]["ours"], false);
    assert_eq!(report["route"]["verified"], false);
    assert_eq!(report["target"], "http://chap:8000/**");
    assert_eq!(report["instance"]["version"], "2.42.6");
    assert_eq!(report["instance"]["user"], "admin");
    assert_eq!(report["instance"]["credential_from"], "default");
    assert_eq!(report["instance"]["auth"], "basic");
    assert_eq!(report["instance"]["external"], false);
    // Never the password, in any field of the document.
    assert!(!report.to_string().contains("district"), "{report}");
    let missing = report["missing"].to_string();
    assert!(missing.contains("not at this deployment"), "{missing}");

    // Nothing was proxied: an answer from somebody else's chap-core would say
    // nothing about this deployment.
    assert!(
        !stand_in.was_asked("GET /api/routes/chap/run/health"),
        "{:?}",
        stand_in.asked()
    );
}

/// A route that is right in every field is not a route that works: when
/// nothing answers through it, `show` says so rather than "the Modeling App can
/// reach Chap".
#[cfg(unix)]
#[test]
fn dhis2_show_counts_a_route_nothing_answers_through_as_missing() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        route: Some(serde_json::json!({
            "id": "route-1",
            "code": "chap",
            "url": "http://chap:8000/**",
            "disabled": false,
            "authorities": ["F_CHAP_MODELING_APP"],
        })),
        proxy_fails: true,
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    let report = json_of(&mut dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        None,
        &["show", "--json"],
    ));
    assert_eq!(report["route"]["ours"], true);
    assert_eq!(report["route"]["verified"], false);
    let missing = report["missing"].to_string();
    assert!(
        missing.contains("nothing answered through the `chap` route"),
        "{missing}"
    );
    assert!(missing.contains("502"), "{missing}");
    assert!(
        !report["next"].as_str().unwrap().contains("can reach Chap"),
        "{report}"
    );
}

/// `connect` does the three steps in order, and `--offline` turns the one that
/// needs the App Hub into a reported skip rather than a failure.
#[cfg(unix)]
#[test]
fn dhis2_connect_does_the_route_the_apps_and_then_analytics() {
    let stand_in = Dhis2StandIn::new();
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, Some(stand_in.port), &["connect"])
        .assert()
        .success()
        .stdout(predicates::str::contains("created the `chap` route"))
        .stdout(predicates::str::contains("installed Modeling App 7.1.0"))
        .stdout(predicates::str::contains("analytics finished in"))
        .stdout(predicates::str::contains(
            "the Modeling App can reach Chap; open DHIS2 with `chap-devops open dhis2`",
        ));

    // The order is the route, then the apps, then the long one.
    let asked = stand_in.asked();
    let at = |request: &str| {
        asked
            .iter()
            .position(|seen| seen == request)
            .unwrap_or_else(|| panic!("{request} was never asked: {asked:?}"))
    };
    assert!(
        at("POST /api/routes") < at("POST /api/appHub/mv-7.1.0"),
        "{asked:?}"
    );
    assert!(
        at("POST /api/appHub/mv-7.1.0")
            < at("POST /api/resourceTables/analytics?skipTrackedEntities=true"),
        "{asked:?}"
    );
}

/// The apps step is the only one that cannot work offline, so `connect
/// --offline` does the other two and says which one it left out.
#[cfg(unix)]
#[test]
fn dhis2_connect_offline_skips_the_apps_and_reports_the_skip() {
    let stand_in = Dhis2StandIn::new();
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, None, &["connect"])
        .assert()
        .success()
        .stdout(predicates::str::contains("created the `chap` route"))
        .stdout(predicates::str::contains("analytics finished in"))
        .stdout(predicates::str::contains("skipped:"))
        .stdout(predicates::str::contains("App Hub"))
        .stdout(predicates::str::contains(
            "run `chap-devops dhis2 show` to see what is still missing",
        ));

    assert!(
        !stand_in
            .asked()
            .iter()
            .any(|seen| seen.starts_with("GET /apphub/")),
        "{:?}",
        stand_in.asked()
    );
}

/// The record `connect` leaves behind, which is the whole of what stops
/// `chap-devops up` and `chap-devops status` asking for it again.
///
/// It is written only when the run got as far as a verified route and both
/// apps, it says in the report what it is worth, and it names `chap-devops dhis2
/// show` as the thing that actually asks DHIS2.
#[cfg(unix)]
#[test]
fn dhis2_connect_records_that_it_ran_and_says_what_the_record_is_worth() {
    let stand_in = Dhis2StandIn::new();
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);
    let recorded = || {
        let body = std::fs::read_to_string(dir.join(".chap-devops").join("components.yaml"))
            .expect("components.yaml");
        body.lines()
            .find(|line| line.trim_start().starts_with("connected_at:"))
            .expect(&body)
            .trim()
            .to_string()
    };

    // Nothing has connected it yet, and the file says so in the words every
    // other field in it is written in.
    assert_eq!(recorded(), "connected_at: null");

    dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        Some(stand_in.port),
        &["connect", "-v"],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains(
        "hint: recorded the connect in `.chap-devops/components.yaml`, so `chap-devops up` and \
             `chap-devops status` stop asking for it",
    ))
    // The caveat sits where the record is, because this is the one moment
    // it could be taken for a verdict.
    .stdout(predicates::str::contains(
        "hint: the record says that this ran, not that the route is still right; \
             `chap-devops dhis2 show` asks DHIS2",
    ));

    let at = recorded();
    assert!(at.starts_with("connected_at: 20"), "{at}");
    assert!(at.ends_with('Z'), "a UTC timestamp: {at}");

    // And `chap-devops status` stops asking for it. chap-core is not answering in
    // this sandbox, so the row is what proves the state was read at all.
    let mut status = chap_with_docker(
        &sandbox,
        &dir,
        &bin,
        &["status", "--json", "--timeout", "2"],
    );
    status.env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1");
    let out = status.assert().get_output().stdout.clone();
    let report: Json = serde_json::from_slice(&out).expect("status --json is one document");
    assert_eq!(report["dhis2_needs_connecting"], false, "{report}");
}

/// A `connect --offline` skips the app install and reports the skip, so it
/// knows nothing about the apps in DHIS2 - and writes nothing in either
/// direction.
///
/// A deployment nothing has connected keeps its empty record, because one step
/// is not a connection. A deployment that **was** connected keeps the record it
/// has: the apps are still in DHIS2, this run simply did not look, and clearing
/// on a flag that turned off an unrelated step would make `chap-devops up` say chap-devops
/// has not connected a DHIS2 chap-devops connected.
#[cfg(unix)]
#[test]
fn dhis2_connect_offline_leaves_the_record_as_it_found_it() {
    let stand_in = Dhis2StandIn::new();
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);
    let recorded = || {
        std::fs::read_to_string(dir.join(".chap-devops").join("components.yaml"))
            .expect("components.yaml")
    };

    dhis2_chap(&sandbox, &dir, &bin, None, &["connect", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains("created the `chap` route"))
        .stdout(predicates::str::contains("skipped:"))
        .stdout(predicates::str::contains("recorded the connect").not())
        .stdout(predicates::str::contains("cleared the earlier").not());
    assert!(recorded().contains("connected_at: null"), "{}", recorded());

    // The three single-step verbs record nothing either: one step is not a
    // connection.
    dhis2_chap(&sandbox, &dir, &bin, Some(stand_in.port), &["apps"])
        .assert()
        .success();
    assert!(recorded().contains("connected_at: null"), "{}", recorded());

    // Now a full connect, and then the same offline run on top of it: the
    // record it wrote is still there afterwards, and nothing was said about it.
    dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        Some(stand_in.port),
        &["connect", "-v"],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains("recorded the connect"));
    let at = recorded()
        .lines()
        .find(|line| line.trim_start().starts_with("connected_at:"))
        .expect("a recorded connect")
        .trim()
        .to_string();
    assert!(at.starts_with("connected_at: 20"), "{at}");

    dhis2_chap(&sandbox, &dir, &bin, None, &["connect", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains("skipped:"))
        .stdout(predicates::str::contains("cleared the earlier").not());
    assert!(recorded().contains(&at), "{}", recorded());
}

/// The record is about a DHIS2 instance, so it dies with the component.
///
/// `chap-devops components disable dhis2 --purge` removes `dhis2_db`; re-enabling
/// restores the seed dump, **which ships its own `chap` route pointing at an
/// external server**. A record that survived that would suppress the one line
/// asking the operator to repoint it, so `disable` forgets it either way round
/// and says it did.
#[cfg(unix)]
#[test]
fn disabling_dhis2_forgets_the_connect_it_had_recorded() {
    let stand_in = Dhis2StandIn::new();
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);

    dhis2_chap(&sandbox, &dir, &bin, Some(stand_in.port), &["connect"])
        .assert()
        .success();
    let body = std::fs::read_to_string(dir.join(".chap-devops").join("components.yaml"))
        .expect("components.yaml");
    assert!(!body.contains("connected_at: null"), "{body}");

    sandbox
        .components(&["disable", "dhis2", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: the record of `chap-devops dhis2 connect` is forgotten with the component; \
             a DHIS2 enabled here again is asked to connect afresh",
        ));

    let body = std::fs::read_to_string(dir.join(".chap-devops").join("components.yaml"))
        .expect("components.yaml");
    assert!(body.contains("connected_at: null"), "{body}");

    // And the DHIS2 that comes back is asked to connect afresh.
    sandbox
        .components(&["enable", "dhis2", "--port", &stand_in.port.to_string()])
        .assert()
        .success();
    let body = std::fs::read_to_string(dir.join(".chap-devops").join("components.yaml"))
        .expect("components.yaml");
    assert!(body.contains("connected_at: null"), "{body}");

    // A deployment that was never connected has nothing to report, so the line
    // is not printed on every disable.
    sandbox
        .components(&["disable", "dhis2"])
        .assert()
        .success()
        .stdout(predicates::str::contains("chap-devops dhis2 connect").not());
}

/// A deployment this DHIS2 is not part of, and one whose DHIS2 keeps its port
/// to itself: each refusal names what is true instead and the way out.
#[test]
fn dhis2_refuses_a_deployment_without_a_reachable_dhis2() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "none", "--api-port", &free_port().to_string()])
        .assert()
        .success();

    // No component at all.
    sandbox
        .chap()
        .arg("-C")
        .arg(sandbox.project())
        .args(["dhis2", "show"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "dhis2 is not a component of this deployment, so there is no DHIS2 to ask",
        ))
        .stderr(predicates::str::contains(
            "`chap-devops components enable dhis2`",
        ));

    // Enabled, publishing no host port: there is nothing to reach from here.
    sandbox
        .components(&["enable", "dhis2", "--port", "none"])
        .assert()
        .success();
    sandbox
        .chap()
        .arg("-C")
        .arg(sandbox.project())
        .args(["dhis2", "show"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "publishes no host port, so its API cannot be reached from this machine",
        ))
        .stderr(predicates::str::contains("http://dhis2:8080"))
        .stderr(predicates::str::contains(
            "`chap-devops components enable dhis2 --port N`",
        ));
}

/// Outside a deployment there is no DHIS2 to talk to, and the bare group says
/// so rather than listing subcommands that cannot run.
#[test]
fn dhis2_outside_a_project_says_so() {
    let sandbox = Sandbox::new();
    chap_in(&sandbox, sandbox.home.path(), &["dhis2"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not a chap-devops deployment"));
    chap_in(&sandbox, sandbox.home.path(), &["dhis2", "show"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not a chap-devops deployment"));
}

/// Enabling the component says the two halves cannot talk yet and names the one
/// command that connects them. `chap-devops up` does not do it: see the chapter.
#[test]
fn enabling_dhis2_names_the_command_that_connects_it_to_chap() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "none", "--api-port", &free_port().to_string()])
        .assert()
        .success();
    sandbox
        .components(&["enable", "dhis2", "--port", &free_port().to_string()])
        .assert()
        .success()
        .stdout(predicates::str::contains("`chap-devops dhis2 connect`"))
        .stdout(predicates::str::contains(
            "the Modeling App reaches chap-core through a DHIS2 route",
        ));

    // And the login variables are in `.env`, commented out, so the two names
    // are somewhere an operator will find them.
    let env = read(&sandbox.project().join(".env"));
    assert!(env.contains("# DHIS2_ADMIN_USERNAME=admin"), "{env}");
    assert!(env.contains("# DHIS2_ADMIN_PASSWORD=district"), "{env}");
    assert_eq!(env_value(&env, "DHIS2_ADMIN_PASSWORD"), None, "{env}");

    // A new port on a DHIS2 that a connect is recorded for leaves the route
    // as it is, so the note does not come back.
    let components = sandbox
        .project()
        .join(".chap-devops")
        .join("components.yaml");
    let body = read(&components);
    std::fs::write(
        &components,
        body.replace("connected_at: null", "connected_at: 2026-09-27T09:12:33Z"),
    )
    .expect("the record");
    let assert = sandbox
        .components(&["enable", "dhis2", "--port", &free_port().to_string()])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(!stdout.contains("through a DHIS2 route"), "{stdout}");
}

/// `components enable dhis2 --seed` sets the seed that the messages name, so
/// nobody has to edit `.chap-devops/components.yaml` by hand.
#[test]
fn enable_dhis2_seed_sets_the_seed_of_a_new_database() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&[
            "--models",
            "none",
            "--only",
            "dhis2",
            "--dhis2-port",
            &free_port().to_string(),
            "--dhis2-tag",
            "2.43",
        ])
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "`chap-devops components enable dhis2 --seed URL`",
        ))
        .stderr(predicates::str::contains("components.yaml").not());
    let components = sandbox
        .project()
        .join(".chap-devops")
        .join("components.yaml");

    sandbox
        .components(&["enable", "dhis2", "--seed", "dumps/mine.sql.gz"])
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "or run `chap-devops components enable dhis2 --seed none`",
        ));
    assert!(
        read(&components).contains("seed: dumps/mine.sql.gz"),
        "{}",
        read(&components)
    );

    sandbox
        .components(&["enable", "dhis2", "--seed", "none"])
        .assert()
        .success();
    assert!(
        read(&components).contains("seed: none"),
        "{}",
        read(&components)
    );

    // `--offline` is on in the sandbox, so a URL is refused before anything
    // is written, and another component has no seed.
    sandbox
        .components(&["enable", "dhis2", "--seed", "https://example.org/x.sql.gz"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("ask for opposite things"));
    assert!(
        read(&components).contains("seed: none"),
        "{}",
        read(&components)
    );
    sandbox
        .components(&["enable", "ocs", "--seed", "none"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--seed is a DHIS2 setting"));
}

/// A dump that is a file gives `dhis2-db` a restore time that grows with its
/// size, and `DHIS2_DB_START_PERIOD` in `.env` can replace it.
#[test]
fn a_large_seed_file_gives_the_restore_more_time() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&[
            "--models",
            "none",
            "--only",
            "dhis2",
            "--dhis2-port",
            &free_port().to_string(),
            "--dhis2-tag",
            "2.42",
        ])
        .assert()
        .success();
    let compose = sandbox.project().join("compose.dhis2.yml");
    assert!(
        read(&compose).contains("start_period: ${DHIS2_DB_START_PERIOD:-600s}"),
        "{}",
        read(&compose)
    );

    // Sparse: 2 GiB on paper and nothing on disk.
    let dumps = sandbox.project().join("dumps");
    std::fs::create_dir_all(&dumps).expect("a dumps directory");
    std::fs::File::create(dumps.join("big.sql.gz"))
        .and_then(|file| file.set_len(2 << 30))
        .expect("a sparse dump");
    sandbox
        .components(&["enable", "dhis2", "--seed", "dumps/big.sql.gz"])
        .assert()
        .success();
    assert!(
        read(&compose).contains("start_period: ${DHIS2_DB_START_PERIOD:-4096s}"),
        "{}",
        read(&compose)
    );
}
