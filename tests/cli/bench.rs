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
    let header = stdout.lines().next().unwrap_or_default();
    assert!(
        header.contains(" SIZE ") && header.ends_with("GZIP"),
        "{header}"
    );
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
    // Asked for gzip and unpacked: the size is the JSON, the wire size the
    // gzip of it.
    let body = r#"{"mark":"line"}"#;
    assert_eq!(subplot["bytes"], body.len());
    assert_eq!(subplot["wire_bytes"], gzipped(body).len());
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

/// The flags of a small dataset, so a test sends little.
const SMALL: &[&str] = &[
    "--locations",
    "3",
    "--periods",
    "12",
    "--period-type",
    "month",
];

fn bench_write(sandbox: &Sandbox, dir: &std::path::Path, extra: &[&str]) -> assert_cmd::Command {
    let mut args = vec!["bench", "--write"];
    args.extend_from_slice(SMALL);
    args.extend_from_slice(extra);
    chap_in(sandbox, dir, &args)
}

#[test]
fn bench_write_imports_a_dataset_runs_the_backtests_and_cleans_up() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    let output = bench_write(
        &sandbox,
        &dir,
        &["--model", OLD_CHAPKIT_MODEL, "--backtests", "2"],
    )
    .assert()
    .success()
    .get_output()
    .clone();
    let stdout = String::from_utf8(output.stdout).expect("text");
    assert!(stdout.starts_with("# "), "{stdout}");
    assert_eq!(
        stdout.lines().filter(|l| l.ends_with(" ok")).count(),
        2,
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("{BACKTEST_FORECASTS}/{BACKTEST_FORECASTS}")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "2 of 2 backtests of {OLD_CHAPKIT_MODEL} on dataset {TEST_DATASET} passed"
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "deleted dataset {TEST_DATASET} and the 1 backtest the run made"
        )),
        "{stdout}"
    );

    let seen = recorded(&sandbox, &dir);
    // 3 locations by 12 months, sent as columns.
    assert_eq!(seen["observations"], 36);
    assert_eq!(seen["backtests"].as_array().expect("backtests").len(), 2);
    assert_eq!(seen["backtests"][0]["modelId"], OLD_CHAPKIT_MODEL);
    assert_eq!(seen["backtests"][0]["datasetId"], TEST_DATASET);
    // The two backtests wrote one row, which goes once, then the dataset.
    assert_eq!(
        seen["deleted"],
        serde_json::json!([
            format!("/v1/crud/backtests/{TEST_BACKTEST}"),
            format!("/v1/crud/datasets/{TEST_DATASET}"),
        ])
    );
}

#[test]
fn bench_write_keeps_a_dataset_it_was_given() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    let value: Json = json_of(&mut chap_in(
        &sandbox,
        &dir,
        &[
            "--json",
            "bench",
            "--write",
            "--model",
            OLD_CHAPKIT_MODEL,
            "--backtests",
            "1",
            "--dataset",
            &TEST_DATASET.to_string(),
        ],
    ));
    assert_eq!(value["dataset_imported"], false);
    assert_eq!(value["failed"], 0);
    assert_eq!(value["backtests"][0]["forecasts"], BACKTEST_FORECASTS);
    let seen = recorded(&sandbox, &dir);
    assert_eq!(seen["datasets"], serde_json::json!([]));
    assert_eq!(
        seen["deleted"],
        serde_json::json!([format!("/v1/crud/backtests/{TEST_BACKTEST}")])
    );
}

#[test]
fn bench_write_fails_when_a_backtest_fails_and_says_why() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    bench_write(
        &sandbox,
        &dir,
        &["--model", FAILING_MODEL, "--backtests", "1"],
    )
    .assert()
    .code(1)
    .stdout(predicates::str::contains("0 of 1 backtests"))
    .stderr(predicates::str::contains(format!(
        "backtest 1 (job bt-{FAILING_MODEL})"
    )))
    .stderr(predicates::str::contains("`chap-devops jobs logs bt-"));
}

#[test]
fn bench_write_needs_a_model_chap_core_has() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    bench_write(&sandbox, &dir, &[])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("--write needs --model"))
        .stderr(predicates::str::contains(OLD_CHAPKIT_MODEL));
    bench_write(&sandbox, &dir, &["--model", "nope"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("no configured model `nope`"));
    // Archived models are not offered.
    bench_write(&sandbox, &dir, &["--model", "14"])
        .assert()
        .code(2);
    let seen = recorded(&sandbox, &dir);
    assert_eq!(seen["datasets"], serde_json::json!([]));
}

#[test]
fn the_write_options_need_write() {
    let sandbox = Sandbox::new();
    sandbox
        .chap()
        .args(["bench", "--model", "x"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("--write"));
    sandbox
        .chap()
        .args(["bench", "5", "--write"])
        .assert()
        .code(2);
}
