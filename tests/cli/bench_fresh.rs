//! `chap-devops bench --fresh-database`: empty chap-core's database before
//! a `--write` run, with a fake docker that logs every call.

use crate::common::*;

fn served_project(sandbox: &Sandbox) -> std::path::PathBuf {
    let port = chap_core_server();
    sandbox
        .init(&["--models", "none", "--api-port", &port.to_string()])
        .assert()
        .success();
    sandbox.project()
}

fn bench_in(sandbox: &Sandbox, cwd: &std::path::Path, args: &[&str]) -> assert_cmd::Command {
    let mut cmd = chap_in(sandbox, cwd, args);
    cmd.env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1");
    cmd
}

#[cfg(unix)]
#[test]
fn bench_write_fresh_database_empties_the_database_before_the_run() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);
    let (_temp, bin, log) = quiet_docker();

    let output = chap_with_docker(
        &sandbox,
        &dir,
        &bin,
        &[
            "--json",
            "bench",
            "--write",
            "--model",
            OLD_CHAPKIT_MODEL,
            "--backtests",
            "1",
            "--locations",
            "3",
            "--periods",
            "12",
            "--period-type",
            "month",
            "--fresh-database",
        ],
    )
    .env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1")
    .assert()
    .success()
    .get_output()
    .clone();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["fresh_database"], serde_json::json!(true), "{value}");
    let messages = value["messages"].to_string();
    assert!(
        messages.contains("emptied chap-core's database before the run"),
        "{messages}"
    );
    let calls = std::fs::read_to_string(&log).expect("the docker log");
    let stop = calls.find("stop chap worker").expect(&calls);
    let drop = calls.find("DROP DATABASE").expect(&calls);
    let start = calls.find("start chap worker").expect(&calls);
    assert!(stop < drop && drop < start, "{calls}");
}

#[test]
fn bench_fresh_database_needs_write_and_this_deployment() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    bench_in(&sandbox, &dir, &["bench", "--fresh-database"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("--write"));
    bench_in(
        &sandbox,
        &dir,
        &[
            "bench",
            "--write",
            "--model",
            "m",
            "--dataset",
            "5",
            "--fresh-database",
        ],
    )
    .assert()
    .code(2)
    .stderr(predicates::str::contains("--dataset"));
    bench_in(
        &sandbox,
        &dir,
        &[
            "bench",
            "--write",
            "--model",
            "m",
            "--fresh-database",
            "--url",
            "http://localhost:18998",
        ],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains("cannot be used with --url"));
    bench_in(
        &sandbox,
        &dir,
        &[
            "bench",
            "--write",
            "--model",
            "m",
            "--sizes",
            "3x12m",
            "--fresh-database",
            "--url",
            "http://localhost:18998",
        ],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains("cannot be used with --url"));
}
