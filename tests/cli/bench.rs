//! `chap-devops bench` against the stand-in chap-core: which requests it
//! sends for one backtest, the table, and what it says about a failure.

use crate::common::*;
use serde_json::Value as Json;

/// `chap-devops <args>` in `cwd`, without docker: `bench` asks docker how
/// many processes run, and a test must not ask the docker of this machine.
fn bench_in(sandbox: &Sandbox, cwd: &std::path::Path, args: &[&str]) -> assert_cmd::Command {
    let mut cmd = chap_in(sandbox, cwd, args);
    cmd.env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1");
    cmd
}

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

    let output = bench_in(
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
    assert!(stdout.starts_with("PAGE "), "{stdout}");
    let header = stdout.lines().next().unwrap_or_default();
    assert!(
        header.contains(" SIZE ") && header.ends_with("GZIP"),
        "{header}"
    );
    for row in [
        "evaluation  backtest info",
        "evaluation  evaluation-entry adm0",
        "compare     evaluation-entry 2 org units",
        "compare     baseline 8 info",
        "benchmark   backtest-specification",
        "plots       bias facet-coords",
        "plots       bias subplot",
        "dataset     dataset",
        "prediction  actualCases 3 org units",
    ] {
        assert!(stdout.contains(row), "{row} is missing:\n{stdout}");
    }
    assert!(
        stdout.contains(&format!(
            "on 6 pages (backtest {BENCH_BACKTEST}, prediction {BENCH_PREDICTION}), 2 times each, 3 at a time"
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains("chap-core time per page, from the p50s: evaluation "),
        "{stdout}"
    );
    assert!(stdout.contains("slowest: "), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "measured on chap-core {CHAP_CORE_VERSION} ({CHAP_CORE_REVISION}), answers packed with gzip"
        )),
        "{stdout}"
    );
    assert!(
        stderr.contains("`compare compatible-backtests` failed 2 of 2 times: HTTP 500"),
        "{stderr}"
    );
    assert!(stderr.contains("database is gone"), "{stderr}");
    assert!(stderr.contains("`chap-devops logs chap`"), "{stderr}");
}

#[test]
fn bench_json_has_a_row_per_request_with_its_numbers() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    let value: Json = json_of(&mut bench_in(
        &sandbox,
        &dir,
        &["--json", "bench", "9", "--repeat", "3"],
    ));
    assert_eq!(value["backtest"], 9);
    assert_eq!(value["parallel"], 1);
    assert_eq!(value["server"]["chap_core_version"], CHAP_CORE_VERSION);
    assert_eq!(value["server"]["revision"], CHAP_CORE_REVISION);
    // The sandbox has no docker, so the process counts are not known.
    assert_eq!(value["server"]["api_workers"], Json::Null);
    assert_eq!(value["server"]["gzip"], true);
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
fn bench_times_the_prediction_page_with_every_org_unit_of_the_dataset() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    let value: Json = json_of(&mut bench_in(
        &sandbox,
        &dir,
        &["--json", "bench", "--page", "prediction", "--repeat", "1"],
    ));
    assert_eq!(value["prediction"], BENCH_PREDICTION);
    assert_eq!(value["backtest"], Json::Null);
    let routes = value["routes"].as_array().expect("routes");
    assert!(routes.iter().all(|r| r["page"] == "prediction"), "{value}");
    let cases = routes
        .iter()
        .find(|r| r["label"] == "actualCases 3 org units")
        .expect("the cases of the dataset");
    assert_eq!(
        cases["path"],
        "/v1/analytics/actualCases/3?orgUnits=ou1&orgUnits=ou2&orgUnits=ou3&isDatasetId=true"
    );
    assert_eq!(cases["step"], 2);
    let pages = value["pages"].as_array().expect("pages");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0]["page"], "prediction");
    assert_eq!(pages[0]["steps"], 3);
}

#[test]
fn bench_warns_about_a_page_it_was_asked_for_and_cannot_time() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    // Backtest 7 names no specification, so there is no benchmark page.
    bench_in(
        &sandbox,
        &dir,
        &["bench", "7", "--page", "benchmark,dataset", "--repeat", "1"],
    )
    .assert()
    .success()
    .stdout(predicates::str::is_match(r"(?m)^dataset +dataset ").expect("a regex"))
    .stderr(predicates::str::contains("did not time the benchmark page"));
}

#[test]
fn bench_names_a_prediction_chap_core_does_not_have() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    bench_in(
        &sandbox,
        &dir,
        &["bench", "--page", "prediction", "--prediction", "41"],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains("could not read prediction 41"))
    .stderr(predicates::str::contains("without --prediction"));
}

#[test]
fn bench_names_a_backtest_chap_core_does_not_have() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    bench_in(&sandbox, &dir, &["bench", "41"])
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
    bench_in(sandbox, dir, &args)
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
        3,
        "{stdout}"
    );
    // Each job with its own run time from /v1/jobs, and the baseline that
    // the runs queued.
    assert!(stdout.contains(" 2.50 s "), "{stdout}");
    assert!(stdout.contains("persistence (baseline)"), "{stdout}");
    assert!(
        stdout.contains("1 of 1 baseline backtests passed"),
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
    // 3 locations by 12 months by the target and 3 covariates, one object
    // each, as the Modeling App sends them.
    assert_eq!(seen["observations"], 3 * 12 * 4);
    assert_eq!(seen["backtests"].as_array().expect("backtests").len(), 2);
    assert_eq!(
        seen["backtests"][0]["modelIds"],
        serde_json::json!([OLD_CHAPKIT_MODEL])
    );
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
fn bench_write_predict_makes_a_prediction_as_the_modeling_app_and_cleans_up() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    let output = bench_write(
        &sandbox,
        &dir,
        &[
            "--model",
            OLD_CHAPKIT_MODEL,
            "--backtests",
            "1",
            "--predict",
        ],
    )
    .assert()
    .success()
    .get_output()
    .clone();
    let stdout = String::from_utf8(output.stdout).expect("text");
    assert!(
        stdout.contains(&format!(
            "prediction {WRITE_PREDICTION} from backtest {TEST_BACKTEST} stored 9 forecasts"
        )),
        "{stdout}"
    );

    let seen = recorded(&sandbox, &dir);
    assert_eq!(seen["setups"][0]["backtestId"], TEST_BACKTEST);
    // 3 locations by 12 months by the target and 3 covariates, one object each.
    assert_eq!(seen["predicted"], 3 * 12 * 4);
    // The prediction, its dataset and the setup go before the backtest the
    // setup holds.
    assert_eq!(
        seen["deleted"],
        serde_json::json!([
            format!("/v1/crud/predictions/{WRITE_PREDICTION}"),
            format!("/v1/crud/datasets/{WRITE_PREDICTION_DATASET}"),
            format!("/v1/crud/prediction-setups/{WRITE_SETUP}"),
            format!("/v1/crud/backtests/{TEST_BACKTEST}"),
            format!("/v1/crud/datasets/{TEST_DATASET}"),
        ])
    );
}

#[test]
fn bench_write_predict_keep_says_how_to_time_the_prediction_page() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    bench_write(
        &sandbox,
        &dir,
        &[
            "--model",
            OLD_CHAPKIT_MODEL,
            "--backtests",
            "1",
            "--predict",
            "--keep",
        ],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains(format!(
        "`chap-devops bench --page prediction --prediction {WRITE_PREDICTION}` times its page"
    )));
    assert_eq!(recorded(&sandbox, &dir)["deleted"], serde_json::json!([]));
}

#[test]
fn bench_write_runs_several_models_under_one_request_as_the_benchmark_page() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    let value: Json = json_of(&mut bench_in(
        &sandbox,
        &dir,
        &[
            "--json",
            "bench",
            "--write",
            "--locations",
            "3",
            "--periods",
            "12",
            "--period-type",
            "month",
            "--model",
            &format!("{OLD_CHAPKIT_MODEL},{PASSING_CONFIGURED}"),
            "--backtests",
            "1",
        ],
    ));
    let seen = recorded(&sandbox, &dir);
    assert_eq!(seen["backtests"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        seen["backtests"][0]["modelIds"],
        serde_json::json!([
            OLD_CHAPKIT_MODEL,
            format!("{PASSING_MODEL}:{PASSING_MODEL}_1790267117082761")
        ])
    );
    let rows = value["backtests"].as_array().expect("rows");
    let own: Vec<&Json> = rows.iter().filter(|r| r["baseline"] == false).collect();
    assert_eq!(own.len(), 2, "{value}");
    assert!(own.iter().all(|r| r["number"] == 1), "{value}");
    assert_eq!(own[0]["seconds"], 2.5);
    assert!(own[0]["request_seconds"].as_f64().is_some(), "{value}");
    assert_eq!(rows.iter().filter(|r| r["baseline"] == true).count(), 1);
}

#[test]
fn bench_write_keeps_a_dataset_it_was_given() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    let value: Json = json_of(&mut bench_in(
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
        "backtest 1 of {FAILING_MODEL} (job bt-{FAILING_MODEL})"
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

#[test]
fn bench_outside_a_deployment_names_url() {
    let sandbox = Sandbox::new();
    bench_in(&sandbox, sandbox.home.path(), &["bench"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("is not a chap-devops deployment"))
        .stderr(predicates::str::contains("run `chap-devops bench` in one"))
        .stderr(predicates::str::contains("`--url URL`"));
}

#[test]
fn bench_workers_refuses_a_chap_core_elsewhere() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    bench_in(
        &sandbox,
        &dir,
        &[
            "bench",
            "--url",
            "http://localhost:18999",
            "--workers",
            "1,2",
        ],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains("cannot be used with --url"));
}

#[test]
fn bench_workers_refuses_more_than_one_on_an_old_chap_core_before_any_run() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);
    let (_temp, bin, log) = quiet_docker();

    chap_with_docker(&sandbox, &dir, &bin, &["bench", "--workers", "1,4"])
        .env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "4 API workers would share database connections",
        ));
    // Nothing was recreated and nothing was timed.
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!calls.contains(" up "), "{calls}");
}
