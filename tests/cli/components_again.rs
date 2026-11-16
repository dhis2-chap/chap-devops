//! `chap-devops components enable` on a component that is already enabled: what
//! changed, and when there is nothing for `chap-devops up` to apply.

use crate::common::*;
use predicates::prelude::PredicateBooleanExt;

/// The same `--port` again changes nothing and names no apply step.
#[test]
fn the_same_port_again_changes_nothing() {
    let sandbox = Sandbox::new();
    let ocs_port = free_port().to_string();
    sandbox.init(&["--models", "none"]).assert().success();
    sandbox
        .components(&["enable", "ocs", "--port", &ocs_port])
        .assert()
        .success()
        .stdout(predicates::str::contains("run `chap-devops up` to apply"));
    sandbox
        .components(&["enable", "ocs", "--port", &ocs_port])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "ocs was already enabled on http://localhost:{ocs_port}; nothing changed"
        )))
        .stdout(predicates::str::contains("chap-devops up").not());
}

/// `--read-write` on a running read-only OCS edits its config file, and a
/// `chap-devops up` after it says that the running ocs may still use the old
/// settings, with `chap-devops restart ocs` as the way out.
#[cfg(unix)]
#[test]
fn up_after_a_read_mode_change_names_the_restart() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let api_port = free_port().to_string();
    let ocs_port = free_port().to_string();
    sandbox
        .init(&["--models", "none", "--api-port", &api_port])
        .assert()
        .success();
    sandbox
        .components(&["enable", "ocs", "--port", &ocs_port, "--read-only"])
        .assert()
        .success();
    sandbox
        .components(&["enable", "ocs", "--read-write"])
        .assert()
        .success();

    // The ocs container was created long before the config file changed.
    let (_fake, bin) = docker_with_ps_rows(&[
        r#"{"Service":"ocs","State":"running","CreatedAt":"2020-01-01 00:00:00 +0000 UTC"}"#,
    ]);
    chap_with_docker(&sandbox, &dir, &bin, &["up", "--no-preflight"])
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "`ocs/climate-service.yaml` changed after the ocs container started, so ocs may \
             still use the old settings; run `chap-devops restart ocs` to apply it",
        ));
}

/// chap-core back after a disable: the models keep the host ports the
/// disable gave them, and the enable says so with the way out.
#[test]
fn chap_core_back_names_the_host_ports_the_models_keep() {
    let sandbox = Sandbox::new();
    let base = port_base();
    sandbox
        .init(&[
            "--models",
            "chapkit_ewars_model",
            "--port-base",
            &base.to_string(),
        ])
        .assert()
        .success();
    sandbox
        .components(&["disable", "chap-core"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "chapkit_ewars_model now registers nowhere and is published on \
             http://localhost:{base}"
        )));
    sandbox
        .components(&["enable", "chap-core"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "the models keep their host ports (chapkit_ewars_model on \
             http://localhost:{base}); `chap-devops models unexpose ID` removes one"
        )));
    // Once chap-core runs, an enable again does not repeat it.
    sandbox
        .components(&["enable", "chap-core"])
        .assert()
        .success()
        .stdout(predicates::str::contains("host ports").not());
}

/// `--base-url` says what it set; the same value again changes nothing.
#[test]
fn a_new_base_url_is_named_and_the_same_one_changes_nothing() {
    let sandbox = Sandbox::new();
    let ocs_port = free_port().to_string();
    sandbox.init(&["--models", "none"]).assert().success();
    sandbox
        .components(&["enable", "ocs", "--port", &ocs_port])
        .assert()
        .success();
    let args = ["enable", "ocs", "--base-url", "https://ocs.example.org"];
    sandbox
        .components(&args)
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "set the ocs base URL to https://ocs.example.org",
        ))
        .stdout(predicates::str::contains("run `chap-devops up` to apply"));
    sandbox
        .components(&args)
        .assert()
        .success()
        .stdout(predicates::str::contains("nothing changed"))
        .stdout(predicates::str::contains("base URL").not())
        .stdout(predicates::str::contains("chap-devops up").not());
}

/// `up` resumes what an interrupted backup left paused. When the unpause
/// fails, it says so with the command that tries again.
#[cfg(unix)]
#[test]
fn up_reports_a_paused_service_it_could_not_resume() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--api-port", &free_port().to_string()])
        .assert()
        .success();
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let docker = bin.join("docker");
    std::fs::write(
        &docker,
        "#!/bin/sh\ncase \"$*\" in\n\
         *' ps '*) printf '{\"Service\":\"chap\",\"State\":\"paused\"}\\n'; exit 0;;\n\
         *' unpause '*) echo 'Error: daemon said no' >&2; exit 1;;\n\
         esac\nexit 0\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    chap_with_docker(&sandbox, &dir, &bin, &["up", "--no-preflight"])
        .assert()
        .stderr(predicates::str::contains(
            "could not resume chap (exit 1: Error: daemon said no); run \
             `chap-devops docker run -- unpause chap`",
        ));
}
