//! A registered model that chap-core has no configured model for.

use super::*;

fn row(name: &str, version: Option<&str>) -> crate::modeltest::ConfiguredModel {
    crate::modeltest::ConfiguredModel {
        id: 1,
        name: name.to_string(),
        archived: false,
        covariates: Vec::new(),
        version: version.map(str::to_string),
        health: None,
    }
}

#[test]
fn a_registered_model_without_a_configured_model_is_not_configured() {
    let registered = [
        registered("chapkit-ewars-model", 5),
        registered("auto-arima-chapkit", 5),
    ];
    let mut rows = model_rows(&enabled(), &registered, &BTreeSet::new(), NOW);
    // The ewars model has a configured model of an older version only.
    let configured = [
        row("chapkit-ewars-model:monthly_climate", Some("0.9.0")),
        row("auto-arima-chapkit", Some("1.0.0")),
    ];
    mark_unconfigured(&mut rows, &registered, &configured);
    assert_eq!(rows[0].state, ModelState::NotConfigured);
    assert_eq!(rows[0].state.label(), "registered, not configured");
    // It is something to do, and not a failure of `chap-devops status`.
    assert!(!rows[0].state.is_problem());
    // A row that is not registered stays as it is.
    assert_eq!(rows[1].state, ModelState::NotRunning);
    assert_eq!(rows[2].state, ModelState::Registered);

    assert_eq!(
        hints(&rows, false, None, &[]),
        [
            "chapkit-ewars-model: chap-core has no configured model for it, so nothing can run \
             it; run `chap-devops models configs sync`",
            "chapkit-rwanda-malaria-bym-model: start Chap with `chap-devops up`, then `chap-devops logs \
             chapkit-rwanda-malaria-bym-model`",
        ]
    );
    // A model that nothing can run is not one to test.
    assert_eq!(test_hint(&rows), None);
}

/// `null` and a missing `git_revision` are two answers: chap-core 2.4 sends
/// the key, and only there does `null` mean that the model reports none.
#[test]
fn a_null_git_revision_is_kept_apart_from_a_missing_one() {
    let body = r#"{"count":3,"services":[
        {"id":"a","url":"u","info":{"id":"a","version":"1","git_revision":null}},
        {"id":"b","url":"u","info":{"id":"b","version":"1","git_revision":"abc"}},
        {"id":"c","url":"u","info":{"id":"c","version":"1"}}]}"#;
    let services = parse_services(body).expect("a registry");
    assert_eq!(services[0].git_revision, Some(None));
    assert_eq!(services[1].git_revision, Some(Some("abc".to_string())));
    assert_eq!(services[2].git_revision, None);
}

/// A model registered from outside whose template chap-core refuses gets a
/// warning with the way out; a model of this deployment does not.
#[test]
fn an_unmanaged_model_whose_template_chap_core_refuses_is_named() {
    let mut no_revision = registered("host-model", 5);
    no_revision.git_revision = Some(None);
    let mut moved = registered("moved-model", 5);
    moved.git_revision = Some(Some("def".to_string()));
    let mut fine = registered("fine-model", 5);
    fine.git_revision = Some(Some("abc".to_string()));
    let mut mine = registered("chapkit-ewars-model", 5);
    mine.git_revision = Some(None);
    let services = [no_revision, moved, fine, mine];
    let rows = model_rows(&enabled(), &services, &BTreeSet::new(), NOW);

    let mut mismatch = row("moved-model:dev", Some("1.0.0"));
    mismatch.health = Some("revision_mismatch".to_string());
    let mut live = row("fine-model:dev", Some("1.0.0"));
    live.health = Some("live".to_string());
    let warnings = revision_warnings(&rows, &services, &[mismatch, live], false);
    let lines: Vec<String> = warnings.iter().map(revision_line).collect();
    assert_eq!(
        lines,
        [
            "host-model: it reports no git revision, so chap-core stores no model template for \
             it; set `GIT_REVISION` where it runs, then start it again",
            "moved-model: chap-core stores its model template 1.0.0 from another git revision \
             and refuses to run it; set a new version in the model, then start it again",
        ]
    );
}

/// A model of this deployment that registers with no git revision and has
/// no configured model gets the no-revision line, and not the `chap-devops models
/// configs sync` line, which cannot fix it.
#[test]
fn a_managed_model_with_no_revision_and_no_configured_model_is_told_to_rebuild() {
    let mut mine = registered("chapkit-ewars-model", 5);
    mine.git_revision = Some(None);
    let services = [mine];
    let mut rows = model_rows(&enabled(), &services, &BTreeSet::new(), NOW);
    mark_unconfigured(&mut rows, &services, &[]);
    let warnings = revision_warnings(&rows, &services, &[], false);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].managed);
    assert_eq!(
        revision_line(&warnings[0]),
        "chapkit-ewars-model: it reports no git revision, so chap-core stores no model template \
         for it and `chap-devops models configs sync` cannot configure it; build its image again with \
         `--build-arg GIT_REVISION=$(git rev-parse HEAD)`, then run `chap-devops restart`"
    );
    let fixes = hints(&rows, false, None, &warnings);
    assert!(
        !fixes
            .iter()
            .any(|fix| fix.contains("chap-devops models configs sync")),
        "{fixes:?}"
    );
    // Without the warning, the row still names the step.
    let fixes = hints(&rows, false, None, &[]);
    assert!(
        fixes
            .iter()
            .any(|fix| fix.ends_with("run `chap-devops models configs sync`")),
        "{fixes:?}"
    );
}

/// chapkit 2.1 sends no `git_revision` key when it has no revision, and
/// chap-core 2.4 passes on what it sent. A missing key is then no revision.
#[test]
fn a_missing_git_revision_is_no_revision_on_a_chap_core_that_passes_the_key_on() {
    let services = [registered("host-model", 5)];
    assert_eq!(services[0].git_revision, None);
    let rows = model_rows(&enabled(), &services, &BTreeSet::new(), NOW);

    // An older chap-core, or one that cannot say: no warning.
    assert!(!sends_revision(&services, None));
    assert!(!sends_revision(&services, Some("2.3.1")));
    assert!(!sends_revision(&services, Some("latest")));
    assert!(revision_warnings(&rows, &services, &[], false).is_empty());

    // chap-core 2.4 or later, by its version or by the key it sent for
    // another service.
    assert!(sends_revision(&services, Some("2.4.0")));
    assert!(sends_revision(&services, Some("v2.5.1-rc.1")));
    let mut other = registered("fine-model", 5);
    other.git_revision = Some(Some("abc".to_string()));
    assert!(sends_revision(&[registered("host-model", 5), other], None));

    let warnings = revision_warnings(&rows, &services, &[], true);
    let lines: Vec<String> = warnings.iter().map(revision_line).collect();
    assert_eq!(
        lines,
        [
            "host-model: it reports no git revision, so chap-core stores no model template for \
          it; set `GIT_REVISION` where it runs, then start it again"
        ]
    );
}

/// A model registered from outside that registers with a new version keeps
/// the configured models of the old one, and nothing can run the new one.
#[test]
fn an_unmanaged_model_with_configured_models_of_another_version_only_is_named() {
    let mut moved = registered("host-model", 5);
    moved.version = "1.0.2".to_string();
    moved.git_revision = Some(Some("abc".to_string()));
    let services = [moved];
    let rows = model_rows(&enabled(), &services, &BTreeSet::new(), NOW);

    let old = [
        row("host-model", Some("1.0.1")),
        row("host-model:test_config_1", Some("1.0.2")),
    ];
    let warnings = revision_warnings(&rows, &services, &old, true);
    let lines: Vec<String> = warnings.iter().map(revision_line).collect();
    assert_eq!(
        lines,
        [
            "host-model: chap-core has configured models of it of other versions only, not of \
          1.0.2; run `chap-devops models configs sync host-model`"
        ]
    );

    // A configured model of the new version, or none at all: no warning.
    let new = [
        row("host-model", Some("1.0.1")),
        row("host-model:a", Some("1.0.2")),
    ];
    assert!(revision_warnings(&rows, &services, &new, true).is_empty());
    assert!(revision_warnings(&rows, &services, &[], true).is_empty());
}

/// A model of this deployment that registered from another commit than its
/// `sha-` pin is named; a matching or a shorter revision, a model with no
/// revision and a tag that names no commit are not.
#[test]
fn a_revision_that_is_not_the_pin_is_named() {
    let model = |service_id: &str, image_tag: &str| crate::project::EnabledModel {
        previous: None,
        service_id: service_id.to_string(),
        image: "ghcr.io/chap-models/x".to_string(),
        image_tag: image_tag.to_string(),
        version: "1.0.0".to_string(),
        channel: None,
        host_port: None,
        bind: None,
        data_dir: "/app/data".to_string(),
        user: "chapkit:chapkit".to_string(),
        user_from: Default::default(),
        platform: None,
        compose_file: format!("compose.{service_id}.yml"),
        reads_port: false,
    };
    let with = |id: &str, revision: Option<&str>| RegisteredService {
        git_revision: Some(revision.map(str::to_string)),
        ..registered(id, 5)
    };
    let models = [
        model("wrong", "sha-1111111"),
        model("right", "sha-2222222"),
        model("short", "sha-3333333aa"),
        model("none", "sha-4444444"),
        model("moving", "latest"),
    ];
    let registered = [
        with("wrong", Some("9999999abcdef")),
        with("right", Some("2222222abcdef")),
        with("short", Some("3333333")),
        with("none", None),
        with("moving", Some("5555555")),
    ];
    let warnings = pin_warnings(&models, &registered);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(
        revision_line(&warnings[0]),
        "wrong: it registered git revision 9999999abcdef, which is not the commit of its image \
         tag sha-1111111; `chap-devops up` starts the pinned image, and if this line stays, \
         report it to the marketplace"
    );
}
