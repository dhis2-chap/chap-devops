use super::super::test_support::*;
use super::*;
use crate::status::{ApiVersion, ModelStatus};
use std::collections::BTreeMap;

/// The tag the `user` checks are asked about in these tests.
const IMAGE_REF: &str = "ghcr.io/chap-models/chapkit_ewars_model:sha-fa880a1";

/// The `user <service>` line: what the overlay runs the model as, against
/// what the pulled image says it runs as.
#[test]
fn a_models_user_is_checked_against_the_image_it_pins() {
    // The image declares `USER chapkit`; the entry records the numbers
    // that resolves to, so the two agree.
    let ok = user_check(
        "chapkit_ewars_model",
        "chapkit-ewars-model",
        Some(IMAGE_REF),
        "1000:1000",
        Some("chapkit"),
    );
    assert_eq!(ok.status, Status::Ok);
    assert_eq!(ok.id, "user-chapkit-ewars-model");
    assert_eq!(ok.name, "user chapkit-ewars-model");
    assert!(ok.detail.contains("as the image declares"), "{ok:?}");
    assert_eq!(ok.fix, None);

    // Root, however it is spelled on either side. An empty `User` is one
    // of those spellings: the parser has already ruled out the empty
    // config that only looks like root.
    for declared in ["root", "", "0:0"] {
        let check = user_check("m", "m", Some(IMAGE_REF), "root", Some(declared));
        assert_eq!(check.status, Status::Ok, "{declared:?} {check:?}");
    }

    // The bug this check exists for: an image that runs as root under an
    // overlay that hands it an unprivileged uid.
    let wrong = user_check(
        "auto_arima_chapkit",
        "auto-arima-chapkit",
        Some(IMAGE_REF),
        "1000:1000",
        Some("root"),
    );
    assert_eq!(wrong.status, Status::Warn);
    assert!(
        wrong
            .detail
            .contains("1000:1000, but the image runs as root"),
        "{wrong:?}"
    );
    assert_eq!(
        wrong.fix.as_deref(),
        Some(
            "run `chap-devops models enable auto_arima_chapkit` to read the \
                 user off the image again, or `chap-devops update`"
        )
    );

    // Nothing to ask: the amd64 variant is not in the image store, which
    // is also what an arm64 host that has pulled nothing yet looks like.
    let absent = user_check("m", "m", Some(IMAGE_REF), "1000:1000", None);
    assert_eq!(absent.status, Status::Skip);
    assert!(
        absent
            .detail
            .contains(&format!("the amd64 variant of {IMAGE_REF} is not in the")),
        "{absent:?}"
    );
    assert_eq!(
        absent.fix.as_deref(),
        Some(
            "run `chap-devops docker pull`, or `docker pull --platform linux/amd64 \
                 ghcr.io/chap-models/chapkit_ewars_model:sha-fa880a1`"
        )
    );

    // No daemon to ask is a different reason, and no pull would fix it.
    let no_cli = user_check("m", "m", None, "1000:1000", None);
    assert_eq!(no_cli.status, Status::Skip);
    assert!(no_cli.detail.contains("no docker daemon"), "{no_cli:?}");
    assert_eq!(no_cli.fix, None);

    // And an account name only the image can resolve proves nothing about
    // the numbers recorded beside it.
    let opaque = user_check("m", "m", Some(IMAGE_REF), "10001:10001", Some("app"));
    assert_eq!(opaque.status, Status::Skip);
    assert!(opaque.detail.contains("`app`"), "{opaque:?}");
}

/// One line per enabled model, and none at all without a docker CLI to
/// ask.
#[test]
fn the_user_checks_cover_every_enabled_model() {
    let entry = crate::project::EnabledModel {
        previous: None,
        reads_port: false,
        service_id: "chapkit-ewars-model".into(),
        image: "ghcr.io/chap-models/chapkit_ewars_model".into(),
        image_tag: "sha-fa880a1".into(),
        version: "1.0.0".into(),
        channel: None,
        host_port: None,
        bind: None,
        data_dir: "/app/data".into(),
        user: "1000:1000".into(),
        user_from: Default::default(),
        platform: None,
        compose_file: "compose.chapkit-ewars-model.yml".into(),
    };
    let mut project = Project {
        dir: std::path::PathBuf::from("/tmp/chapx"),
        state: crate::project::ProjectState {
            models: std::collections::BTreeMap::from([("chapkit_ewars_model".to_string(), entry)]),
            ..crate::project::ProjectState::default()
        },
    };
    let checks = user_checks_with(&project, true, &|reference| {
        reference
            .contains("ewars")
            .then(|| ("root".to_string(), "/app".to_string()))
    });
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].id, "user-chapkit-ewars-model");
    assert_eq!(checks[0].status, Status::Warn);

    // Without a daemon nothing is asked, but the line is still there.
    let skipped = user_checks_with(&project, false, &|_| panic!("no daemon, no question"));
    assert_eq!(skipped[0].status, Status::Skip);
    assert!(
        skipped[0].detail.contains("no docker daemon"),
        "{skipped:?}"
    );

    // A daemon that cannot answer for the amd64 variant - an arm64 host
    // that has pulled nothing, or has pulled only its own architecture -
    // names the pull that would let the line be answered.
    let unknown = user_checks_with(&project, true, &|_| None);
    assert_eq!(unknown[0].status, Status::Skip);
    assert!(
        unknown[0]
            .detail
            .contains("the amd64 variant of ghcr.io/chap-models/chapkit_ewars_model:sha-fa880a1"),
        "{unknown:?}"
    );
    assert!(
        unknown[0]
            .fix
            .as_deref()
            .is_some_and(|fix| fix.contains("chap-devops docker pull")),
        "{unknown:?}"
    );

    // A deployment with no models has no such lines.
    project.state.models.clear();
    assert!(user_checks_with(&project, true, &|_| None).is_empty());
}

#[test]
fn the_component_images_are_the_ones_the_enabled_set_pulls() {
    assert!(component_images(&Components::default(), "").is_empty());

    let mut components = Components::default();
    components.set_enabled(crate::components::Component::Ocs, true);
    components.set_enabled(crate::components::Component::S3, true);
    components.set_enabled(Component::Dhis2, true);
    assert_eq!(
        component_images(&components, ""),
        vec![
            (
                "ocs".to_string(),
                "ghcr.io/dhis2/open-climate-service:main".to_string()
            ),
            ("s3".to_string(), "rustfs/rustfs:latest".to_string()),
            // The web image, at the tag the component records. Its PostGIS
            // database and its alpine one-shot are exact references in this
            // binary, so there is no tag of theirs to get wrong.
            ("dhis2".to_string(), "dhis2/core:2.42".to_string()),
        ]
    );
    assert!(
        !component_images(&components, "")
            .iter()
            .any(|(_, reference)| reference.contains("postgis")),
        "the database image has no tag a deployment can move"
    );
}

#[test]
fn a_component_image_is_checked_for_existence_not_for_amd64() {
    // Both component images are multi-arch and no overlay pins a platform,
    // so a manifest that answers at all is the whole verdict.
    let arm_only = r#"{"manifests":[{"platform":{"os":"linux","architecture":"arm64"}}]}"#;
    let check = component_image_verdict(
        "ocs",
        "ghcr.io/dhis2/open-climate-service:main",
        &Outcome::Done {
            ok: true,
            stdout: arm_only.to_string(),
            stderr: String::new(),
        },
    );
    assert_eq!(check.status, Status::Ok);
    assert_eq!(check.id, "image-ocs");

    let missing = component_image_verdict(
        "ocs",
        "ghcr.io/dhis2/open-climate-service:nope",
        &Outcome::Done {
            ok: false,
            stdout: String::new(),
            stderr: "manifest unknown".to_string(),
        },
    );
    assert_eq!(missing.status, Status::Fail);
    assert!(missing.detail.contains("manifest unknown"));
    assert!(missing.fix.unwrap().contains("OCS_IMAGE_TAG"));

    assert_eq!(
        component_image_verdict("s3", "rustfs/rustfs:latest", &Outcome::Missing).status,
        Status::Skip
    );
}

#[test]
fn the_stack_line_judges_a_deployment_without_chap_core_by_its_components() {
    use crate::status::{ComponentState, ComponentStatus};

    let component = |name: &str, state: ComponentState| ComponentStatus {
        name: name.to_string(),
        state,
        reach: "http://localhost:9000".to_string(),
        health_url: None,
        read_only: false,
        datasets: None,
        data_bytes: None,
    };
    let mut report = status_report(ApiHealth::Off, &[], &[]);
    report.components = vec![component("ocs", ComponentState::Up)];
    let (status, detail, fix) = stack_verdict(&report);
    assert_eq!(status, Status::Ok);
    assert_eq!(detail, "no chap-core here; up: ocs");
    assert_eq!(fix, None);

    report.components = vec![
        component("ocs", ComponentState::Starting),
        component("s3", ComponentState::Up),
    ];
    let (status, detail, fix) = stack_verdict(&report);
    assert_eq!(status, Status::Warn);
    assert_eq!(detail, "no chap-core here; not up: ocs");
    assert!(fix.unwrap().contains("chap-devops status"));
}

#[test]
fn the_nothing_running_fix_names_chap_only_with_a_chap_core_of_its_own() {
    assert_eq!(start_fix(true), "run `chap-devops up` to start Chap");
    assert_eq!(
        start_fix(false),
        "run `chap-devops up` to start the deployment"
    );
}

#[test]
fn the_stack_line_says_when_the_container_itself_is_unhealthy() {
    let mut report = status_report(
        ApiHealth::Down {
            error: "connection refused".to_string(),
        },
        &[],
        &[],
    );
    // Without a container to blame it is the port that is not answering.
    let (status, detail, fix) = stack_verdict(&report);
    assert_eq!(status, Status::Fail);
    assert!(detail.contains("is down: connection refused"), "{detail}");
    assert!(fix.unwrap().contains("chap-devops logs chap"));

    // With one, the check says which half is wrong and what its log said.
    report.unhealthy = vec![crate::diagnose::Unhealthy::of(
        API_SERVICE,
        "FATAL:  password authentication failed for user \"chap\"\n",
    )];
    let (status, detail, fix) = stack_verdict(&report);
    assert_eq!(status, Status::Fail);
    assert!(
        detail.starts_with("the chap container is up and unhealthy:"),
        "{detail}"
    );
    assert!(
        detail.contains("password authentication failed"),
        "{detail}"
    );
    assert_eq!(detail.lines().count(), 1, "one line per check: {detail}");
    assert!(fix.unwrap().contains("chap-devops down --volumes"));
}

#[test]
fn an_image_that_is_not_there_names_the_model_to_re_resolve() {
    let check = image_verdict(
        "chapkit_ewars_model",
        "chapkit-ewars-model",
        "ghcr.io/chap-models/chapkit_ewars_model:sha-fa880a1",
        &done(false, "", "manifest unknown"),
    );
    assert_eq!(check.status, Status::Fail);
    assert_eq!(check.id, "image-chapkit-ewars-model");
    assert!(
        check.detail.contains("manifest unknown"),
        "{}",
        check.detail
    );
    assert_eq!(
        check.fix.unwrap(),
        "run `chap-devops models enable chapkit_ewars_model` to resolve the pin again"
    );

    let list = r#"{"manifests":[{"platform":{"architecture":"amd64","os":"linux"}}]}"#;
    let check = image_verdict("m", "svc", "ghcr.io/x:1", &done(true, list, ""));
    assert_eq!(check.status, Status::Ok);
    assert!(check.detail.contains("linux/amd64"));

    let arm = r#"{"manifests":[{"platform":{"architecture":"arm64","os":"linux"}}]}"#;
    let check = image_verdict("m", "svc", "ghcr.io/x:1", &done(true, arm, ""));
    assert_eq!(check.status, Status::Warn);
    assert!(check.fix.unwrap().contains("chap-devops models enable m"));

    // An old CLI that hides `docker manifest` cannot answer, which is not
    // the same as the tag being gone.
    let check = image_verdict(
        "m",
        "svc",
        "ghcr.io/x:1",
        &done(
            false,
            "",
            "docker manifest is only supported on a Docker cli with \
                  experimental cli features enabled",
        ),
    );
    assert_eq!(check.status, Status::Skip);
    assert_eq!(
        image_verdict("m", "svc", "r", &Outcome::TimedOut).status,
        Status::Skip
    );
    assert_eq!(
        image_verdict("m", "svc", "r", &Outcome::Missing).status,
        Status::Skip
    );
}

/// A `StatusReport` with the fields the stack check reads.
fn status_report(api: ApiHealth, expected: &[&str], missing: &[&str]) -> StatusReport {
    StatusReport {
        project: Some("mychap-1ab2c3".to_string()),
        api_url: "http://localhost:8000".to_string(),
        api_port: 8000,
        api_port_source: ApiPortSource::Project,
        api,
        version: ApiVersion {
            value: "v2.3.1".to_string(),
            pinned: false,
            revision: None,
        },
        chap_tag: "v2.3.1".to_string(),
        chap_tag_moving: false,
        chap_build: None,
        registered: Vec::new(),
        expected: expected.iter().map(|s| s.to_string()).collect(),
        missing: missing.iter().map(|s| s.to_string()).collect(),
        reach: BTreeMap::new(),
        models: Vec::<ModelStatus>::new(),
        unmanaged: Vec::new(),
        revision_warnings: Vec::new(),
        auth: false,
        components: Vec::new(),
        dhis2_needs_connecting: false,
        chap_core_elsewhere: false,
        api_starting: false,
        dhis2_external: None,
        api_elsewhere: None,
        unhealthy: Vec::new(),
    }
}

#[test]
fn the_stack_check_follows_what_status_found() {
    let up = || ApiHealth::Up {
        status: "ok".to_string(),
        message: String::new(),
    };

    let (status, detail, fix) = stack_verdict(&status_report(up(), &["a", "b"], &[]));
    assert_eq!(status, Status::Ok);
    assert_eq!(
        detail,
        "chap-core up at http://localhost:8000, 2 of 2 models registered"
    );
    // Green, and still with something to do: registration is a heartbeat.
    assert_eq!(fix.as_deref(), Some(crate::status::TEST_HINT));

    // A deployment with no models has nothing to test.
    let (status, _, fix) = stack_verdict(&status_report(up(), &[], &[]));
    assert_eq!(status, Status::Ok);
    assert_eq!(fix, None);

    let (status, detail, fix) = stack_verdict(&status_report(up(), &["a", "b"], &["b"]));
    assert_eq!(status, Status::Warn, "a missing model is not a dead stack");
    assert!(detail.ends_with("not registered: b"), "{detail}");
    assert!(fix.unwrap().contains("chap-devops status"));

    let down = ApiHealth::Down {
        error: "connection refused".to_string(),
    };
    let (status, detail, fix) = stack_verdict(&status_report(down, &["a"], &["a"]));
    assert_eq!(status, Status::Fail);
    assert!(detail.contains("connection refused"), "{detail}");
    assert!(fix.unwrap().contains("chap-devops logs chap"));
}

/// A registry that refuses or fails says nothing about the tag: a warning
/// that quotes it, while a manifest that does not exist still fails.
#[test]
fn only_a_missing_manifest_fails_an_image_line() {
    let limited = image_verdict(
        "chapkit_ewars_model",
        "chapkit-ewars-model",
        "ghcr.io/chap-models/chapkit_ewars_model:sha-1",
        &done(false, "", "toomanyrequests: retry later"),
    );
    assert_eq!(limited.status, Status::Warn, "{limited:?}");
    assert!(limited.detail.contains("toomanyrequests"), "{limited:?}");

    let missing = image_verdict(
        "chapkit_ewars_model",
        "chapkit-ewars-model",
        "ghcr.io/chap-models/chapkit_ewars_model:sha-1",
        &done(false, "", "manifest unknown: manifest unknown"),
    );
    assert_eq!(missing.status, Status::Fail, "{missing:?}");

    let hub = component_image_verdict("dhis2", "dhis2/core:2.42", &done(false, "", "unauthorized"));
    assert_eq!(hub.status, Status::Warn, "{hub:?}");
    assert!(manifest_missing("no such manifest: docker.io/x:y"));
}

/// The tag compose pulls is the one `.env` overrides it with, so that is the
/// one checked.
#[test]
fn an_env_tag_override_is_the_reference_checked() {
    let mut components = Components::default();
    components.ocs.enabled = true;
    let env = format!("{OCS_TAG_ENV_VAR}=bad\n");
    assert_eq!(
        component_images(&components, &env)[0].1,
        format!("{OCS_IMAGE}:bad")
    );

    let model = crate::project::EnabledModel {
        previous: None,
        reads_port: false,
        service_id: "m".into(),
        image: "ghcr.io/chap-models/m".into(),
        image_tag: "sha-1".into(),
        version: "1.0.0".into(),
        channel: None,
        host_port: None,
        bind: None,
        data_dir: "/app/data".into(),
        user: "1000:1000".into(),
        user_from: Default::default(),
        platform: None,
        compose_file: "compose.m.yml".into(),
    };
    let var = crate::compose::tag_env_var("m");
    assert_eq!(
        model_reference("m", &model, ""),
        "ghcr.io/chap-models/m:sha-1"
    );
    assert_eq!(
        model_reference("m", &model, &format!("{var}=sha-2\n")),
        "ghcr.io/chap-models/m:sha-2"
    );
}
