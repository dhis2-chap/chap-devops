use crate::common::*;
use predicates::prelude::PredicateBooleanExt;
use serde_json::Value as Json;
use std::path::PathBuf;
#[cfg(unix)]
use tempfile::TempDir;

/// A project with the three stand-in models enabled, pointed at a chap-core
/// that answers the `models test --backtest` path.
fn tested_project(sandbox: &Sandbox) -> (PathBuf, u16) {
    let port = chap_core_server();
    sandbox
        .init(&[
            "--models",
            "chapkit_ewars_model,chapkit_rwanda_malaria_bym_model,auto_arima_chapkit",
            "--api-port",
            &port.to_string(),
        ])
        .assert()
        .success();
    (sandbox.project(), port)
}

/// chap-core in a container with models registered as `localhost:<port>`:
/// registered, and every call back to them a 502. The backtest says so before
/// it builds anything, and points at `chap-devops status` for the fix.
#[test]
fn models_test_backtest_skips_a_model_chap_core_cannot_reach() {
    let sandbox = Sandbox::new();
    let port = unreachable_models_chap_core_server();
    sandbox
        .init(&[
            "--models",
            "chapkit_ewars_model",
            "--api-port",
            &port.to_string(),
        ])
        .assert()
        .success();
    let dir = sandbox.project();

    let out = chap_in(&sandbox, &dir, &["models", "test", "--all", "--backtest"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).expect("the rows are text");
    assert!(
        text.contains(
            "chap-core cannot reach it (HTTP 502 Bad Gateway from \
             /v2/services/chapkit-ewars-model/run/health)"
        ),
        "{text}"
    );
    assert!(text.contains("run `chap-devops status`"), "{text}");
    assert!(!text.contains("no configured model"), "{text}");
}

/// A model run from its checkout registers without being enabled here. It can
/// be named by its service id, and the model level, which needs a container,
/// sends it to `--backtest`.
#[test]
fn models_test_takes_a_model_registered_from_outside_the_deployment() {
    let sandbox = Sandbox::new();
    let port = chap_core_server();
    sandbox
        .init(&["--models", "none", "--api-port", &port.to_string()])
        .assert()
        .success();
    let dir = sandbox.project();

    chap_in(&sandbox, &dir, &["models", "test", "--all"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "`chap-devops models test ID --backtest` tests one of the 4 registered from outside it \
             through chap-core",
        ))
        .stderr(predicates::str::contains(HOST_RUN_MODEL));

    chap_in(&sandbox, &dir, &["models", "test", HOST_RUN_MODEL])
        .assert()
        .stdout(predicates::str::contains(
            "chap-devops does not run it, so there is no container to test it in",
        ))
        .stdout(predicates::str::contains(format!(
            "run `chap-devops models test {HOST_RUN_MODEL} --backtest`"
        )));
}

#[test]
fn models_test_backtest_reports_scores_a_failure_and_a_skip() {
    let sandbox = Sandbox::new();
    let (dir, _) = tested_project(&sandbox);

    let out = chap_in(&sandbox, &dir, &["models", "test", "--all", "--backtest"])
        .assert()
        // One model failed, so the run did.
        .failure()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).expect("the rows are text");

    // The header says which of the two levels this was.
    assert!(
        text.starts_with("testing 3 models (through chap-core:"),
        "{text}"
    );
    // A model that backtested: the three scores, in chap-core's own numbers.
    assert!(
        text.contains("chapkit-ewars-model                 pass"),
        "{text}"
    );
    assert!(text.contains("crps 20.0  mae 29.0  rmse 34.9"), "{text}");
    // A model whose prediction crashed: the reason out of the job log, and
    // the command that shows the whole of it.
    assert!(
        text.contains("chapkit-rwanda-malaria-bym-model    FAIL"),
        "{text}"
    );
    assert!(
        text.contains("the inla program crashed"),
        "the stderr hint is the reason:\n{text}"
    );
    assert!(
        text.contains(&format!("run `chap-devops jobs logs bt-{FAILING_MODEL}`")),
        "{text}"
    );
    // A model whose chapkit predates the route: a skip that names the version
    // it needs and the one the service reports.
    assert!(
        text.contains("auto-arima-chapkit                  skip"),
        "{text}"
    );
    assert!(
        text.contains("needs chapkit 1.1.0, this one reports 1.0.0"),
        "{text}"
    );
    // And the line the run adds up to. The way to the full output is under
    // the failing row already, so the closing line is the count alone.
    assert!(text.ends_with("\n1 pass, 1 fail, 1 skipped\n"), "{text}");

    let recorded = recorded(&sandbox, &dir);
    // The dataset is named after the service and the moment, and the org
    // units arrived: `importedCount` is the count of features with a
    // top-level id, so a zero here would mean chap-devops forgot to set them.
    let names: Vec<&str> = recorded["datasets"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|name| name.as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        names.len(),
        2,
        "the skipped model posted nothing: {names:?}"
    );
    let ewars = names
        .iter()
        .position(|name| name.starts_with("chap-devops-test-chapkit-ewars-model-"))
        .unwrap_or_else(|| panic!("{names:?}"));
    // The covariate its configured model asks for and the sample lacks took
    // over the spare `feature_0`; the other model's config asks for none, so
    // its dataset is the sample as it came.
    let features = |at: usize| -> Vec<&str> {
        recorded["features"][at]
            .as_array()
            .expect("a list")
            .iter()
            .map(|name| name.as_str().unwrap_or_default())
            .collect()
    };
    assert!(
        features(ewars).contains(&"mean_relative_humidity")
            && !features(ewars).contains(&"feature_0"),
        "{:?}",
        features(ewars)
    );
    assert!(
        features(1 - ewars).contains(&"feature_0"),
        "{:?}",
        features(1 - ewars)
    );
    // The backtest asked for is the small rolling one, against the dataset
    // the job wrote.
    let backtest = &recorded["backtests"][0];
    // The configured model chap-core would run, by its integer id: the row
    // named plainly after the service is archived, and the live ones are the
    // synced configs, of which the `test_config_` is the wrong answer.
    assert_eq!(backtest["modelId"], Json::from(PASSING_CONFIGURED));
    assert_eq!(backtest["datasetId"], Json::from(TEST_DATASET));
    assert_eq!(backtest["nPeriods"], Json::from(3));
    assert_eq!(backtest["nSplits"], Json::from(2));
    assert_eq!(backtest["stride"], Json::from(1));
    // Everything this run made is gone again, and nothing else was touched.
    let deleted: Vec<&str> = recorded["deleted"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|path| path.as_str().unwrap_or_default())
        .collect();
    assert!(
        deleted.contains(&format!("/v1/crud/backtests/{TEST_BACKTEST}").as_str()),
        "{deleted:?}"
    );
    assert!(
        deleted.contains(&format!("/v1/crud/datasets/{TEST_DATASET}").as_str()),
        "{deleted:?}"
    );
    assert!(
        deleted.iter().all(|path| path.starts_with("/v1/crud/")),
        "only its own rows: {deleted:?}"
    );
}

#[test]
fn models_test_keep_leaves_the_dataset_and_says_so() {
    let sandbox = Sandbox::new();
    let (dir, _) = tested_project(&sandbox);

    let out = chap_in(
        &sandbox,
        &dir,
        &[
            "models",
            "test",
            "chapkit_ewars_model",
            "--backtest",
            "--keep",
        ],
    )
    .assert()
    .success()
    .get_output()
    .clone();
    let text = String::from_utf8(out.stdout).expect("text");
    assert!(text.contains("1 of 1 model passes"), "{text}");
    // A single row is not padded past its own name.
    assert!(text.contains("chapkit-ewars-model    pass"), "{text}");
    // What was kept is a closing line, on stdout like the count.
    assert!(
        text.contains(&format!(
            "kept backtest {TEST_BACKTEST} and dataset {TEST_DATASET}; remove them with \
             `chap-devops api DELETE /v1/crud/backtests/{TEST_BACKTEST}` then \
             `chap-devops api DELETE /v1/crud/datasets/{TEST_DATASET}`"
        )),
        "{text}"
    );

    let recorded = recorded(&sandbox, &dir);
    assert_eq!(recorded["deleted"], Json::Array(Vec::new()));
    // One observation per value, and neither index column became one: three
    // periods times two locations times five feature columns.
    assert_eq!(recorded["observations"], Json::from(30));
    assert_eq!(recorded["sampled"], serde_json::json!([PASSING_MODEL]));
}

#[test]
fn models_test_json_is_one_object_per_model() {
    let sandbox = Sandbox::new();
    let (dir, _) = tested_project(&sandbox);

    let doc = json_of(&mut chap_in(
        &sandbox,
        &dir,
        &[
            "--json",
            "models",
            "test",
            "chapkit_ewars_model",
            "--backtest",
        ],
    ));
    assert_eq!(doc["ok"], Json::from(true));
    let row = &doc["models"][0];
    assert_eq!(row["id"], Json::from("chapkit_ewars_model"));
    assert_eq!(row["service_id"], Json::from(PASSING_MODEL));
    assert_eq!(row["level"], Json::from("backtest"));
    assert_eq!(row["result"], Json::from("pass"));
    assert!(row["seconds"].is_number(), "{row}");
    assert_eq!(row["summary"], Json::from("crps 20.0  mae 29.0  rmse 34.9"));
    assert_eq!(row["backtest_id"], Json::from(TEST_BACKTEST));
    assert_eq!(row["job_id"], Json::from(format!("bt-{PASSING_MODEL}")));
    // The scores are handed back whole, not just the three the row prints.
    assert_eq!(row["metrics"]["mape"], Json::from(12.98));
    // The closing line rides along with its level.
    assert_eq!(doc["messages"][0]["level"], Json::from("info"));
    assert_eq!(
        doc["messages"][0]["text"],
        Json::from("1 of 1 model passes")
    );
}

#[test]
fn models_test_needs_an_id_or_all_and_the_model_has_to_be_enabled() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();

    // Neither an id nor --all: a usage error that names both.
    chap_in(&sandbox, &dir, &["models", "test"])
        .assert()
        .code(2)
        .stderr(
            predicates::str::contains("name a model to test")
                .and(predicates::str::contains("--all")),
        );

    // An id the catalogue has never heard of.
    chap_in(&sandbox, &dir, &["models", "test", "nope"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "unknown model `nope`; run `chap-devops models list` to see the model ids",
        ));

    // An id that is a model, but not one this deployment enables.
    chap_in(&sandbox, &dir, &["models", "test", "auto_arima_chapkit"])
        .assert()
        .failure()
        .stderr(
            predicates::str::contains("auto_arima_chapkit is not enabled in this deployment").and(
                predicates::str::contains("chap-devops models enable auto_arima_chapkit"),
            ),
        );

    // And the two ways of saying which models cannot be combined.
    chap_in(
        &sandbox,
        &dir,
        &["models", "test", "chapkit_ewars_model", "--all"],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains("cannot be used with"));
}

/// The rows with each duration as `<t>`: a run takes whole seconds of real
/// time, so a busy machine prints `1s` where an idle one prints `0s`.
#[cfg(unix)]
fn rows_without_time(text: &str) -> String {
    regex::Regex::new(r"(pass|skip|fail) +[0-9]+s +")
        .expect("a valid pattern")
        .replace_all(text, "$1 <t> ")
        .into_owned()
}

/// A `docker` on PATH that answers the three questions the model level asks.
///
/// The model level is `docker compose exec` and nothing else, so there is no
/// way to cover it without either a Docker with the marketplace images pulled
/// or a stand-in. This is the stand-in: it reports one running service, prints
/// whatever `CHAPKIT_OUT` holds for `chapkit test`, and records every
/// invocation so a test can check what was asked and that the cleanup
/// happened.
#[cfg(unix)]
fn fake_docker(out: &str, code: i32) -> (TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("a directory for the fake docker");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("a bin directory");
    let log = temp.path().join("calls.log");
    let chapkit = temp.path().join("chapkit.out");
    std::fs::write(&chapkit, out).expect("the chapkit output");
    let script = format!(
        "#!/bin/sh\n\
         echo \"$*\" >> {log}\n\
         case \"$*\" in\n\
         *' ps --format json'*) \
           printf '{{\"Service\":\"{PASSING_MODEL}\",\"State\":\"running\"}}\\n'; exit 0;;\n\
         *'chapkit test'*) cat {chapkit}; exit {code};;\n\
         *'-X DELETE'*) exit 0;;\n\
         *curl*) printf '[]'; exit 0;;\n\
         esac\n\
         exit 1\n",
        log = log.display(),
        chapkit = chapkit.display(),
    );
    let docker = bin.join("docker");
    std::fs::write(&docker, script).expect("the fake docker");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755))
            .expect("an executable fake docker");
    }
    (temp, log)
}

#[cfg(unix)]
#[test]
fn models_test_runs_chapkit_test_in_the_container_and_cleans_up_after_it() {
    let sandbox = Sandbox::new();
    let (dir, _) = tested_project(&sandbox);
    let passed = "\
==================================================
TEST SUMMARY
==================================================
Elapsed time:          16.85s
Configs created:       1
Trainings completed:   1
Trainings failed:      0
Predictions completed: 1
Predictions failed:    0
Validations run:       2
Validations failed:    0

Result: ALL TESTS PASSED
";
    let (fake, log) = fake_docker(passed, 0);
    let bin = fake.path().join("bin");

    let out = chap_with_docker(&sandbox, &dir, &bin, &["models", "test", "--all"])
        .assert()
        // A skip is not a failure: two containers were not up, and that is a
        // question this run could not ask rather than a bad answer.
        .success()
        .get_output()
        .stdout
        .clone();
    let text = rows_without_time(&String::from_utf8(out).expect("the rows are text"));

    assert!(
        text.starts_with("testing 3 models (model level; add --backtest"),
        "{text}"
    );
    assert!(
        text.contains("chapkit-ewars-model                 pass <t> 1 training, 1 prediction"),
        "{text}"
    );
    // The two that are not up say so, and say the way out.
    assert!(
        text.contains("auto-arima-chapkit                  skip <t> its container is not running"),
        "{text}"
    );
    assert!(text.contains("run `chap-devops up`"), "{text}");
    assert!(text.contains("1 pass, 2 skipped"), "{text}");

    let calls = read(&log);
    // The invocation is the documented one, non-interactive, with the
    // deadline handed to chapkit as well as kept here.
    assert!(
        calls.contains(&format!(
            "exec -T {PASSING_MODEL} chapkit test --url http://127.0.0.1:8000 --timeout 300"
        )),
        "{calls}"
    );
    // The config `chapkit test` left behind is gone, deleted through the
    // service's own API because chap-core's proxy is read-only.
    assert!(
        calls.contains(&format!(
            "exec -T {PASSING_MODEL} curl -fsS -X DELETE \
             http://127.0.0.1:8000/api/v1/configs/{TEST_CONFIG}"
        )),
        "{calls}"
    );
    // The artifact went with it - chapkit cascades - so nothing asked for it.
    assert!(!calls.contains(TEST_ARTIFACT), "{calls}");
}

/// `--keep` at the model level: the kept config is a closing line, so
/// `--json` has it in `messages`, and it names the command that removes it.
#[cfg(unix)]
#[test]
fn models_test_keep_at_the_model_level_is_in_the_json_messages() {
    let sandbox = Sandbox::new();
    let (dir, _) = tested_project(&sandbox);
    let passed = "\
TEST SUMMARY
Trainings completed:   1
Trainings failed:      0
Predictions completed: 1
Predictions failed:    0

Result: ALL TESTS PASSED
";
    let (fake, log) = fake_docker(passed, 0);
    let bin = fake.path().join("bin");

    let out = chap_with_docker(
        &sandbox,
        &dir,
        &bin,
        &["--json", "models", "test", "chapkit_ewars_model", "--keep"],
    )
    .assert()
    .success()
    .get_output()
    .clone();
    let doc: Json = serde_json::from_slice(&out.stdout).expect("one JSON document");
    let kept = doc["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|m| m["text"].as_str().unwrap_or_default().contains("kept"))
        .unwrap_or_else(|| panic!("no kept line in {doc}"));
    assert_eq!(kept["level"], "info", "{kept}");
    let text = kept["text"].as_str().unwrap_or_default();
    assert!(
        text.contains(&format!(
            "remove it with `chap-devops docker exec {PASSING_MODEL} curl -fsS -X DELETE \
             http://127.0.0.1:8000/api/v1/configs/{TEST_CONFIG}`"
        )),
        "{text}"
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("kept"),
        "the line is not printed twice"
    );
    // Nothing was deleted.
    assert!(!read(&log).contains("-X DELETE"));
}

#[cfg(unix)]
#[test]
fn models_test_skips_an_image_that_has_no_chapkit_test() {
    let sandbox = Sandbox::new();
    let (dir, _) = tested_project(&sandbox);
    let (fake, _) = fake_docker(
        "OCI runtime exec failed: exec failed: unable to start container process: \
         exec: \"chapkit\": executable file not found in $PATH: unknown\n",
        1,
    );
    let bin = fake.path().join("bin");

    let out = chap_with_docker(
        &sandbox,
        &dir,
        &bin,
        &["models", "test", "chapkit_ewars_model"],
    )
    .assert()
    // A run that tested nothing proved nothing, and says so.
    .failure()
    .stderr(predicates::str::contains(
        "warning: no model was tested, because every one was skipped",
    ))
    .get_output()
    .stdout
    .clone();
    let text = String::from_utf8(out).expect("text");
    assert!(
        text.contains("skip") && text.contains("the image has no `chapkit test`"),
        "{text}"
    );
    // Which chapkit the service reports is the fact that says whether an
    // update would fix it.
    assert!(text.contains("the service reports chapkit 2.0.0"), "{text}");
    assert!(text.contains("--backtest"), "{text}");
    assert!(text.contains("0 pass, 1 skipped"), "{text}");
}

#[cfg(unix)]
#[test]
fn models_test_gives_the_count_options_to_chapkit_test() {
    let sandbox = Sandbox::new();
    let (dir, _) = tested_project(&sandbox);
    let passed = "\
TEST SUMMARY
Configs created:       2
Trainings completed:   4
Trainings failed:      0
Predictions completed: 4
Predictions failed:    0

Result: ALL TESTS PASSED
";
    let (fake, log) = fake_docker(passed, 0);
    let bin = fake.path().join("bin");

    let out = chap_with_docker(
        &sandbox,
        &dir,
        &bin,
        &[
            "models",
            "test",
            "chapkit_ewars_model",
            "--configs",
            "2",
            "--trainings",
            "2",
            "--predictions",
            "1",
            "--rows",
            "600",
            "--predict-rows",
            "300",
        ],
    )
    .assert()
    .success()
    .get_output()
    .stdout
    .clone();
    let text = rows_without_time(&String::from_utf8(out).expect("the rows are text"));
    assert!(
        text.contains("pass <t> 4 trainings, 4 predictions"),
        "{text}"
    );
    assert!(
        read(&log).contains(&format!(
            "exec -T {PASSING_MODEL} chapkit test --url http://127.0.0.1:8000 --timeout 300 \
             --configs 2 --trainings 2 --predictions 1 --rows 600 --predict-rows 300"
        )),
        "{}",
        read(&log)
    );
}
