//! `chap-devops bench dataset`: the files, and the import into the
//! stand-in chap-core.

use crate::common::*;
use serde_json::Value as Json;

#[test]
fn generate_writes_a_csv_and_a_geojson_of_the_same_name() {
    let sandbox = Sandbox::new();
    let out = sandbox.home.path().join("data");
    let args = [
        "bench",
        "dataset",
        "--locations",
        "4",
        "--periods",
        "6",
        "--period-type",
        "month",
        "--covariates",
        "2",
        "--out",
    ];
    sandbox
        .chap()
        .args(args)
        .arg(&out)
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "wrote 24 rows (4 locations, 6 monthly periods, 2 covariates)",
        ));
    let csv = out.join("synthetic-4x6m-c100-k2-s1.csv");
    let geo = out.join("synthetic-4x6m-c100-k2-s1.geojson");
    let text = std::fs::read_to_string(&csv).expect("the CSV");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "time_period,location,disease_cases,rainfall,mean_temperature"
    );
    assert_eq!(lines.len(), 25);
    assert!(lines[1].starts_with("2024-07,loc01,"), "{}", lines[1]);
    let geo: Json =
        serde_json::from_str(&std::fs::read_to_string(&geo).expect("the GeoJSON")).expect("JSON");
    assert_eq!(geo["features"].as_array().expect("features").len(), 4);
    assert_eq!(geo["features"][3]["id"], "loc04");

    // The same options give the same bytes.
    let first = text.clone();
    sandbox.chap().args(args).arg(&out).assert().success();
    assert_eq!(std::fs::read_to_string(&csv).expect("the CSV"), first);
}

#[test]
fn generate_import_stores_the_dataset_in_chap_core() {
    let sandbox = Sandbox::new();
    let port = chap_core_server();
    sandbox
        .init(&["--models", "none", "--api-port", &port.to_string()])
        .assert()
        .success();
    let dir = sandbox.project();

    let value: Json = json_of(&mut chap_in(
        &sandbox,
        &dir,
        &[
            "--json",
            "bench",
            "dataset",
            "--import",
            "--locations",
            "5",
            "--periods",
            "8",
            "--name",
            "chap-devops-test-five",
        ],
    ));
    assert_eq!(value["dataset_id"], TEST_DATASET);
    assert_eq!(value["files"], Json::Null);
    assert_eq!(value["rows"], 40);
    let seen = recorded(&sandbox, &dir);
    assert_eq!(
        seen["datasets"],
        serde_json::json!(["chap-devops-test-five"])
    );
    assert_eq!(seen["observations"], 40);
    assert_eq!(
        seen["features"][0],
        serde_json::json!([
            "disease_cases",
            "mean_temperature",
            "population",
            "rainfall"
        ])
    );
    // Nothing was written next to the deployment.
    assert!(!dir.join("chap-devops-test-five.csv").exists());
}

#[test]
fn a_name_with_a_directory_in_it_is_refused() {
    let sandbox = Sandbox::new();
    sandbox
        .chap()
        .args(["bench", "dataset", "--name", "../x"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("cannot be a file name"));
}
