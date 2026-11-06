//! `chap-devops bench` against the stand-in chap-core: which requests it
//! sends for one backtest, the table, and what it says about a failure.

use crate::common::*;
use serde_json::Value as Json;

fn served_project(sandbox: &Sandbox) -> std::path::PathBuf {
    let port = chap_core_server();
    sandbox
        .init(&["--models", "none", "--api-port", &port.to_string()])
        .assert()
        .success();
    sandbox.project()
}

#[test]
fn bench_times_the_newest_backtest_and_warns_about_a_failed_request() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    let output = chap_in(
        &sandbox,
        &dir,
        &["bench", "--repeat", "2", "--parallel", "3"],
    )
    .assert()
    .success()
    .get_output()
    .clone();
    let stdout = String::from_utf8(output.stdout).expect("text");
    let stderr = String::from_utf8(output.stderr).expect("text");
    assert!(stdout.starts_with("REQUEST "), "{stdout}");
    for label in [
        "backtest info",
        "evaluation-entry adm0",
        "evaluation-entry 2 org units",
        "backtest-specification",
        "bias facet-coords",
        "bias subplot",
    ] {
        assert!(stdout.contains(label), "{label} is missing:\n{stdout}");
    }
    assert!(
        stdout.contains(&format!(
            "of backtest {BENCH_BACKTEST}, 2 times each, 3 at a time"
        )),
        "{stdout}"
    );
    assert!(stdout.contains("slowest: "), "{stdout}");
    assert!(
        stderr.contains("`compatible-backtests` failed 2 of 2 times: HTTP 500"),
        "{stderr}"
    );
    assert!(stderr.contains("database is gone"), "{stderr}");
    assert!(stderr.contains("`chap-devops logs chap`"), "{stderr}");
}

#[test]
fn bench_json_has_a_row_per_request_with_its_numbers() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    let value: Json = json_of(&mut chap_in(
        &sandbox,
        &dir,
        &["--json", "bench", "9", "--repeat", "3"],
    ));
    assert_eq!(value["backtest"], 9);
    assert_eq!(value["parallel"], 1);
    let routes = value["routes"].as_array().expect("routes");
    let subplot = routes
        .iter()
        .find(|r| r["label"] == "bias subplot")
        .expect("the subplot row");
    assert_eq!(subplot["method"], "POST");
    assert_eq!(subplot["requests"], 3);
    assert_eq!(subplot["failed"], 0);
    assert!(subplot["p95_ms"].as_f64().expect("a number") >= 0.0);
    let failed = routes
        .iter()
        .find(|r| r["label"] == "compatible-backtests")
        .expect("the failing row");
    assert_eq!(failed["failed"], 3);
    assert_eq!(failed["statuses"]["500"], 3);
    let levels: Vec<&str> = value["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter_map(|m| m["level"].as_str())
        .collect();
    assert!(levels.contains(&"warning"), "{value}");
}

#[test]
fn bench_names_a_backtest_chap_core_does_not_have() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    chap_in(&sandbox, &dir, &["bench", "41"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("could not read backtest 41"))
        .stderr(predicates::str::contains("`chap-devops bench` with no id"));
}
