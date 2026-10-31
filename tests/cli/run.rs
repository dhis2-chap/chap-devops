//! `chap-devops run`, `ps` and `stop`: one model at a time, in a group chap-devops keeps
//! under its data directory or in the deployment the command is inside. Unix
//! only: the stand-in docker is a shell script.

#![cfg(unix)]

use crate::common::*;
use predicates::prelude::PredicateBooleanExt;
use serde_json::Value as Json;
use std::path::{Path, PathBuf};

#[test]
fn run_outside_a_deployment_starts_the_model_in_a_group_on_loopback() {
    let sandbox = Sandbox::new();
    let (_fake, bin) = docker_running_services(&["chapkit-ewars-model"]);
    let cwd = sandbox.home.path();

    let doc = run_json(
        &sandbox,
        cwd,
        &bin,
        &["run", "chapkit_ewars_model", "--no-wait"],
    );
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["id"], "chapkit_ewars_model");
    assert_eq!(doc["group"], "default");
    assert_eq!(doc["bind"], "127.0.0.1");
    let port = doc["port"].as_u64().expect("a host port");
    assert_eq!(doc["url"], format!("http://localhost:{port}"));
    assert_eq!(doc["messages"][0]["level"], "info", "{doc}");
    assert_eq!(
        doc["messages"][0]["text"],
        format!("started chapkit_ewars_model on http://localhost:{port}")
    );

    let dir = data(&sandbox).join("run").join("default");
    assert_eq!(PathBuf::from(doc["project_dir"].as_str().unwrap()), dir);
    let overlay = read(&dir.join("compose.chapkit-ewars-model.yml"));
    assert!(
        overlay.contains(&format!("\"127.0.0.1:{port}:8000\"")),
        "{overlay}"
    );
    // Nothing lands in the working directory.
    assert!(!cwd.join(".chap-devops").exists());
    assert!(!cwd.join("compose.yml").exists());

    // A second run of the same model starts it as it is.
    let again = run_json(
        &sandbox,
        cwd,
        &bin,
        &["run", "chapkit-ewars-model", "--no-wait"],
    );
    assert_eq!(again["enabled"], false);
    assert_eq!(again["port"], port);

    // So does a run by its repository: that is the same marketplace model,
    // not a new enable on the next free port.
    let by_url = run_json(
        &sandbox,
        cwd,
        &bin,
        &[
            "run",
            "https://github.com/chap-models/chapkit_ewars_model",
            "--no-wait",
        ],
    );
    assert_eq!(by_url["enabled"], false, "{by_url}");
    assert_eq!(by_url["id"], "chapkit_ewars_model");
    assert_eq!(by_url["port"], port);
}

#[test]
fn groups_keep_models_apart_and_ps_and_stop_find_them() {
    let sandbox = Sandbox::new();
    let (_fake, bin) = docker_running_services(&["chapkit-ewars-model", "auto-arima-chapkit"]);
    let cwd = sandbox.home.path();

    run_json(
        &sandbox,
        cwd,
        &bin,
        &["run", "chapkit_ewars_model", "--no-wait"],
    );
    run_json(
        &sandbox,
        cwd,
        &bin,
        &["run", "auto_arima_chapkit", "--group", "trial", "--no-wait"],
    );

    let ps = run_json(&sandbox, cwd, &bin, &["ps"]);
    let models = ps["models"].as_array().expect("a list");
    assert_eq!(models.len(), 2, "{ps}");
    // Nothing really listens on the first group's port, and the second group
    // still does not hand it out again.
    assert_ne!(models[0]["port"], models[1]["port"], "{ps}");
    assert_eq!(models[0]["group"], "default");
    assert_eq!(models[1]["group"], "trial");
    assert_eq!(models[1]["id"], "auto_arima_chapkit");

    let text = String::from_utf8(
        chap_with_docker(&sandbox, cwd, &bin, &["ps"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .expect("text");
    assert!(text.starts_with("GROUP"), "{text}");

    let only = run_json(&sandbox, cwd, &bin, &["ps", "--group", "trial"]);
    assert_eq!(only["models"].as_array().unwrap().len(), 1);

    // stop finds the model in whichever group has it.
    let stopped = run_json(&sandbox, cwd, &bin, &["stop", "auto_arima_chapkit"]);
    assert_eq!(stopped["ok"], true);
    assert_eq!(stopped["stopped"][0]["group"], "trial");
    assert_eq!(
        stopped["messages"][0]["text"], "stopped auto_arima_chapkit (group trial)",
        "{stopped}"
    );

    let stopped = run_json(&sandbox, cwd, &bin, &["stop", "--all"]);
    assert_eq!(stopped["stopped"][0]["id"], "chapkit_ewars_model");
    let ps = run_json(&sandbox, cwd, &bin, &["ps"]);
    assert!(ps["models"].as_array().unwrap().is_empty(), "{ps}");
    assert_eq!(ps["messages"][0]["level"], "info", "{ps}");
}

#[test]
fn a_group_labels_its_containers_with_the_run_kind_and_its_name() {
    let sandbox = Sandbox::new();
    let (_fake, bin) = docker_running_services(&["chapkit-ewars-model"]);
    let cwd = sandbox.home.path();
    run_json(
        &sandbox,
        cwd,
        &bin,
        &[
            "run",
            "chapkit_ewars_model",
            "--group",
            "trial",
            "--no-wait",
        ],
    );

    let dir = data(&sandbox).join("run").join("trial");
    let project = read(&dir.join(".chap-devops").join("project.yaml"));
    assert!(project.contains("\ngroup: trial\n"), "{project}");
    let overlay = read(&dir.join("compose.chapkit-ewars-model.yml"));
    for line in [
        "      io.github.dhis2-chap.chap-devops.role: model\n",
        "      io.github.dhis2-chap.chap-devops.model: \"chapkit_ewars_model\"\n",
        "      io.github.dhis2-chap.chap-devops.kind: run\n",
        "      io.github.dhis2-chap.chap-devops.group: \"trial\"\n",
    ] {
        // Once on the model and once on its init container.
        assert_eq!(overlay.matches(line).count(), 2, "{line}{overlay}");
    }
}

#[test]
fn run_inside_a_deployment_uses_it_and_refuses_a_group() {
    let sandbox = Sandbox::new();
    let (_fake, bin) = docker_running_services(&["auto-arima-chapkit"]);
    sandbox
        .init(&["--models", "none", "--port-base", &port_base().to_string()])
        .assert()
        .success();
    let dir = sandbox.project();

    let doc = run_json(
        &sandbox,
        &dir,
        &bin,
        &["run", "auto_arima_chapkit", "--no-wait"],
    );
    assert_eq!(doc["group"], Json::Null);
    let canonical = |p: &Path| std::fs::canonicalize(p).expect("an existing path");
    assert_eq!(
        canonical(Path::new(doc["project_dir"].as_str().unwrap())),
        canonical(&dir)
    );
    assert!(state(&dir)["models"]["auto_arima_chapkit"].is_object());
    // A deployment of the caller's own is not a group, and its labels say so.
    let overlay = read(&dir.join("compose.auto-arima-chapkit.yml"));
    assert!(
        overlay.contains("io.github.dhis2-chap.chap-devops.kind: init\n"),
        "{overlay}"
    );
    assert!(
        !overlay.contains("io.github.dhis2-chap.chap-devops.group"),
        "{overlay}"
    );
    assert!(!read(&dir.join(".chap-devops").join("project.yaml")).contains("group:"));
    assert!(!data(&sandbox).join("run").exists());

    chap_with_docker(
        &sandbox,
        &dir,
        &bin,
        &["run", "auto_arima_chapkit", "--group", "x"],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains(
        "--group names a `chap-devops run` group",
    ));
}

#[test]
fn run_of_an_unknown_id_says_so_and_makes_no_group() {
    let sandbox = Sandbox::new();
    let (_fake, bin) = docker_running_services(&[]);
    let cwd = sandbox.home.path();
    let out = chap_with_docker(&sandbox, cwd, &bin, &["--json", "run", "does_not_exist"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let doc: Json = serde_json::from_slice(&out).expect("one JSON document");
    assert_eq!(doc["ok"], false);
    assert_eq!(
        doc["hint"], "`chap-devops models search does_not_exist` finds one",
        "{doc}"
    );
    assert!(!data(&sandbox).join("run").join("default").exists());
}

#[test]
fn a_usage_error_under_json_is_json() {
    let sandbox = Sandbox::new();
    let out = chap_in(&sandbox, sandbox.home.path(), &["--json", "stop"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let doc: Json = serde_json::from_slice(&out).expect("one JSON document");
    assert_eq!(doc["ok"], false);
    assert!(
        doc["error"]
            .as_str()
            .unwrap()
            .contains("required arguments were not provided"),
        "{doc}"
    );
    assert_eq!(doc["hint"], "`chap-devops stop --help` lists what it takes");
}

#[test]
fn a_run_that_cannot_start_takes_its_model_back_out() {
    let sandbox = Sandbox::new();
    let (_fake, bin) = docker_failing_up();
    let cwd = sandbox.home.path();
    let out = chap_with_docker(
        &sandbox,
        cwd,
        &bin,
        &["--json", "run", "chapkit_ewars_model", "--group", "trial"],
    )
    .assert()
    .failure()
    .get_output()
    .stdout
    .clone();
    let doc: Json = serde_json::from_slice(&out).expect("one JSON document");
    assert!(
        doc["error"]
            .as_str()
            .unwrap()
            .contains("the registry answered `denied` for ghcr.io/"),
        "{doc}"
    );
    assert_eq!(
        doc["hint"],
        "check the reference, or run `docker login ghcr.io`, then \
         `chap-devops run chapkit_ewars_model --group trial` tries again"
    );
    // The line compose said is in the message, and not a cause as well.
    assert_eq!(
        doc["causes"],
        serde_json::json!(["docker compose exited with status 1"]),
        "{doc}"
    );
    let dir = data(&sandbox).join("run").join("trial");
    let models = &state(&dir)["models"];
    assert!(models.as_object().is_none_or(|m| m.is_empty()), "{models}");
}

#[test]
fn parallel_runs_into_a_new_group_all_land_in_it() {
    let sandbox = Sandbox::new();
    let (_fake, bin) = docker_running_services(&["chapkit-ewars-model", "auto-arima-chapkit"]);
    let cwd = sandbox.home.path();
    let codes: Vec<Option<i32>> = std::thread::scope(|scope| {
        let handles: Vec<_> = ["chapkit_ewars_model", "auto_arima_chapkit"]
            .into_iter()
            .map(|model| {
                let (sandbox, bin) = (&sandbox, &bin);
                scope.spawn(move || {
                    chap_with_docker(
                        sandbox,
                        cwd,
                        bin,
                        &["--json", "run", model, "--group", "fresh", "--no-wait"],
                    )
                    .output()
                    .expect("chap-devops ran")
                    .status
                    .code()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(codes, vec![Some(0), Some(0)]);
    let ps = run_json(&sandbox, cwd, &bin, &["ps", "--group", "fresh"]);
    assert_eq!(ps["models"].as_array().unwrap().len(), 2, "{ps}");
}

/// `models remove` has no `--group`, so an error from inside a group names it
/// with the `-C <group dir>` that reaches the group from anywhere.
#[test]
fn an_error_inside_a_group_names_models_remove_with_the_group_dir() {
    let sandbox = Sandbox::new();
    let (_fake, bin) = docker_with_local_images();
    let cwd = sandbox.home.path();
    run_json(
        &sandbox,
        cwd,
        &bin,
        &[
            "run",
            "localone:1",
            "--id",
            "mine",
            "--group",
            "trial",
            "--no-wait",
        ],
    );
    let out = chap_with_docker(
        &sandbox,
        cwd,
        &bin,
        &[
            "--json",
            "run",
            "localtwo:1",
            "--id",
            "mine",
            "--group",
            "trial",
            "--no-wait",
        ],
    )
    .assert()
    .failure()
    .get_output()
    .stdout
    .clone();
    let doc: Json = serde_json::from_slice(&out).expect("one JSON document");
    let dir = data(&sandbox).join("run").join("trial");
    let said = format!("{} {}", doc["error"], doc["hint"]);
    assert!(
        said.contains(&format!(
            "`chap-devops -C {} models remove mine`",
            dir.display()
        )),
        "{doc}"
    );
}

/// `logs` has no `--group` either: a run into a group names its log with `-C`.
#[test]
fn a_run_into_a_group_names_its_log_with_the_group_dir() {
    let sandbox = Sandbox::new();
    let (_fake, bin) = docker_running_services(&["chapkit-ewars-model"]);
    let cwd = sandbox.home.path();
    let out = chap_with_docker(
        &sandbox,
        cwd,
        &bin,
        &[
            "-v",
            "run",
            "chapkit_ewars_model",
            "--group",
            "trial",
            "--no-wait",
        ],
    )
    .assert()
    .success()
    .get_output()
    .stdout
    .clone();
    let text = String::from_utf8(out).expect("text");
    let dir = data(&sandbox).join("run").join("trial");
    assert!(
        text.contains(&format!(
            "`chap-devops -C {} logs chapkit-ewars-model`",
            dir.display()
        )),
        "{text}"
    );
    assert!(
        text.contains("`chap-devops stop chapkit_ewars_model --group trial`"),
        "{text}"
    );
}

/// Under `--json` the way out is the hint and only the hint: the error stops
/// before it, so a reader that shows both does not say it twice.
#[test]
fn a_json_error_does_not_repeat_its_hint() {
    let sandbox = Sandbox::new();
    let (_fake, bin) = docker_running_services(&["chapkit-ewars-model"]);
    let cwd = sandbox.home.path();
    run_json(
        &sandbox,
        cwd,
        &bin,
        &["run", "chapkit_ewars_model", "--no-wait"],
    );
    let out = chap_with_docker(&sandbox, cwd, &bin, &["--json", "stop", "nope"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let doc: Json = serde_json::from_slice(&out).expect("one JSON document");
    let hint = doc["hint"].as_str().expect("a hint");
    assert!(hint.contains("`chap-devops ps`"), "{doc}");
    assert!(!doc["error"].as_str().unwrap().contains(hint), "{doc}");
}

/// A `docker` whose compose commands succeed, and that sends Ctrl-C to chap-devops
/// (its parent) at one point: `up` while the model starts, or `logs` while
/// the foreground follows it. Every call goes to `calls.log`.
fn docker_pressing_ctrl_c_at(
    step: &str,
) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().expect("a directory for the fake docker");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("a bin directory");
    let log = temp.path().join("calls.log");
    let script = format!(
        "#!/bin/sh\n\
         echo \"$*\" >> '{log}'\n\
         case \"$*\" in\n\
         *' {step} '*) kill -INT $PPID; sleep 1; exit 130;;\n\
         esac\n\
         exit 0\n",
        log = log.display()
    );
    let docker = bin.join("docker");
    std::fs::write(&docker, script).expect("the fake docker");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755))
            .expect("an executable fake docker");
    }
    (temp, bin, log)
}

#[test]
fn ctrl_c_in_the_foreground_stops_the_model_and_keeps_its_data() {
    let sandbox = Sandbox::new();
    let (_temp, bin, log) = docker_pressing_ctrl_c_at("logs");

    chap_with_docker(
        &sandbox,
        sandbox.home.path(),
        &bin,
        &["run", "chapkit_ewars_model", "--no-wait", "--attach"],
    )
    .assert()
    .success()
    .stderr(predicates::str::contains(
        "following the log of chapkit-ewars-model; Ctrl-C stops it",
    ))
    .stderr(predicates::str::contains(
        "stopped chapkit_ewars_model; its data stays",
    ));
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains("logs -f --tail 20 chapkit-ewars-model"),
        "{calls}"
    );
    assert!(!calls.contains("volume rm"), "the data stays: {calls}");
    chap_with_docker(&sandbox, sandbox.home.path(), &bin, &["ps"])
        .assert()
        .success()
        .stdout(predicates::str::contains("chapkit_ewars_model").not());
}

/// `--rm` cleans up as `chap-devops stop --purge` does: the group the model leaves
/// empty goes too, with its network and its directory.
#[test]
fn ctrl_c_in_the_foreground_with_rm_removes_the_emptied_group() {
    let sandbox = Sandbox::new();
    let (_temp, bin, log) = docker_pressing_ctrl_c_at("logs");

    chap_with_docker(
        &sandbox,
        sandbox.home.path(),
        &bin,
        &[
            "run",
            "chapkit_ewars_model",
            "--no-wait",
            "--attach",
            "--rm",
        ],
    )
    .assert()
    .success()
    .stderr(predicates::str::contains(
        "stopped chapkit_ewars_model, and removed its data",
    ))
    .stderr(predicates::str::contains("removed group default"));
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.contains("down --remove-orphans --volumes"), "{calls}");
    assert!(!data(&sandbox).join("run").join("default").exists());
}

#[test]
fn ctrl_c_while_the_model_starts_takes_it_back_out() {
    let sandbox = Sandbox::new();
    let (_temp, bin, _log) = docker_pressing_ctrl_c_at("up");

    chap_with_docker(
        &sandbox,
        sandbox.home.path(),
        &bin,
        &["run", "chapkit_ewars_model"],
    )
    .assert()
    .code(130)
    .stderr(predicates::str::contains(
        "stopped by Ctrl-C; chapkit_ewars_model is taken back out",
    ));
    chap_with_docker(&sandbox, sandbox.home.path(), &bin, &["ps"])
        .assert()
        .success()
        .stdout(predicates::str::contains("chapkit_ewars_model").not());
}

#[test]
fn without_attach_run_returns_and_leaves_the_model_running() {
    let sandbox = Sandbox::new();
    let (_temp, bin) = docker_running_services(&["chapkit-ewars-model"]);

    // Without -a there is no foreground, as with `chap-devops up`: the command
    // returns, and under -v a hint names the stop.
    chap_with_docker(
        &sandbox,
        sandbox.home.path(),
        &bin,
        &["-v", "run", "chapkit_ewars_model", "--no-wait"],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains(
        "hint: `chap-devops stop chapkit_ewars_model` stops it",
    ))
    .stderr(predicates::str::contains("following the log").not());
}

#[test]
fn chap_core_points_the_group_at_a_chap_core_elsewhere() {
    let sandbox = Sandbox::new();
    let (_temp, bin) = docker_running_services(&["chapkit-ewars-model"]);

    chap_with_docker(
        &sandbox,
        sandbox.home.path(),
        &bin,
        &[
            "run",
            "chapkit_ewars_model",
            "--no-wait",
            "--chap-core",
            "http://localhost:18999",
            "--models-host",
            "localhost",
        ],
    )
    .assert()
    .success();

    let group = data(&sandbox).join("run").join("default");
    let components = std::fs::read_to_string(group.join(".chap-devops/components.yaml")).unwrap();
    assert!(
        components.contains("url: http://localhost:18999"),
        "{components}"
    );
    let overlay = std::fs::read_to_string(group.join("compose.chapkit-ewars-model.yml")).unwrap();
    // The model registers there over the host gateway, on its host port, and
    // the ewars image fixes --port 8000, so the host port maps to 8000.
    assert!(
        overlay.contains(
            "SERVICEKIT_ORCHESTRATOR_URL: http://host.docker.internal:18999/v2/services/$$register"
        ),
        "{overlay}"
    );
    assert!(overlay.contains(":8000\""), "{overlay}");
    assert!(!overlay.contains("      PORT: "), "{overlay}");
}

#[test]
fn chap_core_in_a_deployment_of_its_own_is_refused_with_the_command_to_use() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();
    let (_temp, bin) = docker_running_services(&["chapkit-ewars-model"]);

    chap_with_docker(
        &sandbox,
        &sandbox.project(),
        &bin,
        &[
            "run",
            "chapkit_ewars_model",
            "--chap-core",
            "http://localhost:18999",
        ],
    )
    .assert()
    .code(2)
    .stderr(predicates::str::contains(
        "`chap-devops components enable chap-core --url http://localhost:18999`",
    ));
}
