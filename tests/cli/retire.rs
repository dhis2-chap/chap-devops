//! `chap-devops models disable --retire` and `chap-devops models remove`, which
//! retire the model's template in chap-core the way `chap-admin uninstall`
//! does, against a stand-in chap-core in the shape of 2.4.

use crate::common::*;
use predicates::prelude::PredicateBooleanExt;

const REPO_URL: &str = "https://github.com/example/chapkit_example_manual_model";

/// A deployment of `models` pointed at a chap-core 2.4 stand-in.
fn fresh_project(sandbox: &Sandbox, models: &str) -> std::path::PathBuf {
    let port = fresh_chap_core_server();
    sandbox
        .init(&["--models", models, "--api-port", &port.to_string()])
        .assert()
        .success();
    sandbox.project()
}

#[test]
fn disable_retire_retires_every_version_of_the_template() {
    let sandbox = Sandbox::new();
    let dir = fresh_project(&sandbox, "chapkit_ewars_model");

    chap_in(
        &sandbox,
        &dir,
        &["models", "disable", "chapkit_ewars_model", "--retire"],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains(
        "retired chapkit_ewars_model in chap-core; its configured models are archived\n",
    ));
    let seen = recorded(&sandbox, &dir);
    assert_eq!(
        seen["retired"],
        serde_json::json!([format!(
            "/v1/crud/model-templates/{TEMPLATE_IDS}?allVersions=true"
        )])
    );
    assert_eq!(state(&dir)["models"], serde_json::json!({}));
}

#[test]
fn disable_without_retire_leaves_chap_core_alone() {
    let sandbox = Sandbox::new();
    let dir = fresh_project(&sandbox, "chapkit_ewars_model");

    chap_in(
        &sandbox,
        &dir,
        &["models", "disable", "chapkit_ewars_model"],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains("retired").not());
    assert_eq!(recorded(&sandbox, &dir)["retired"], serde_json::json!([]));
}

#[test]
fn disable_retire_says_when_chap_core_has_no_template() {
    let sandbox = Sandbox::new();
    let dir = fresh_project(&sandbox, "chapkit_simple_multistep_model");

    let report = json_of(&mut chap_in(
        &sandbox,
        &dir,
        &[
            "--json",
            "models",
            "disable",
            "chapkit_simple_multistep_model",
            "--retire",
        ],
    ));
    assert_eq!(report["retired"]["state"], "no-template");
    let texts: Vec<&str> = report["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter_map(|m| m["text"].as_str())
        .collect();
    assert!(
        texts.contains(
            &"chap-core has no live template of chapkit_simple_multistep_model, so there was \
              nothing to retire"
        ),
        "{texts:?}"
    );
}

/// The template is looked up before anything changes, so a chap-core that
/// does not answer leaves the model enabled.
#[test]
fn disable_retire_refuses_while_chap_core_does_not_answer() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&[
            "--models",
            "chapkit_ewars_model",
            "--api-port",
            &free_port().to_string(),
        ])
        .assert()
        .success();
    let dir = sandbox.project();

    chap_in(
        &sandbox,
        &dir,
        &["models", "disable", "chapkit_ewars_model", "--retire"],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains(
        "could not find the template of chapkit_ewars_model in chap-core, so nothing changed",
    ))
    .stderr(predicates::str::contains(
        "start chap-core with `chap-devops up`, or run it without --retire",
    ));
    assert!(state(&dir)["models"]["chapkit_ewars_model"].is_object());
}

#[test]
fn disable_retire_of_a_model_that_is_not_enabled_is_refused() {
    let sandbox = Sandbox::new();
    let dir = fresh_project(&sandbox, "none");

    chap_in(
        &sandbox,
        &dir,
        &["models", "disable", "chapkit_ewars_model", "--retire"],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains(
        "chapkit_ewars_model is not enabled here, so there is no model to retire",
    ));
}

#[test]
fn remove_retires_unless_keep_in_chap() {
    let sandbox = Sandbox::new();
    fresh_project(&sandbox, "none");
    let hub = Hub::new().start();
    sandbox
        .online(hub)
        .args(["models", "add", "--yes", REPO_URL])
        .assert()
        .success();

    // The stand-in lists no template of this model, which is the answer
    // the line gives; the lookup itself is what `remove` asks by default.
    sandbox
        .models(&["remove", "chapkit-example-manual-model"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "chap-core has no live template of chapkit_example_manual_model, so there was \
             nothing to retire",
        ));
}

#[test]
fn remove_refuses_while_chap_core_does_not_answer() {
    let (sandbox, _dir, hub) = added_sandbox(Hub::new());
    sandbox
        .online(hub)
        .args(["models", "add", "--yes", REPO_URL])
        .assert()
        .success();

    sandbox
        .models(&["remove", "chapkit-example-manual-model"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "start chap-core with `chap-devops up`, or pass --keep-in-chap",
        ));
    // Nothing changed, so `--keep-in-chap` still finds it.
    sandbox
        .models(&["remove", "chapkit-example-manual-model", "--keep-in-chap"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "removed chapkit_example_manual_model\n",
        ));
}
