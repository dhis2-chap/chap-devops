use super::measure::*;
use super::plan::*;
use super::write::*;
use serde_json::json;

#[test]
fn the_newest_backtest_is_the_highest_id() {
    let list = json!([{"id": 3}, {"id": 12}, {"id": 7}]);
    assert_eq!(newest_backtest(&list), Some(12));
    assert_eq!(newest_backtest(&json!([])), None);
    assert_eq!(newest_backtest(&json!({"detail": "x"})), None);
}

#[test]
fn the_target_reads_camel_case_fields() {
    let info = json!({
        "id": 12,
        "datasetId": 4,
        "specificationId": 9,
        "orgUnits": ["a", "b"],
    });
    let t = target(12, &info);
    assert_eq!(t.dataset_id, Some(4));
    assert_eq!(t.specification_id, Some(9));
    assert_eq!(t.org_units, vec!["a".to_string(), "b".to_string()]);

    // The dataset can also come as a nested summary.
    let t = target(12, &json!({"dataset": {"id": 5}}));
    assert_eq!(t.dataset_id, Some(5));
    assert!(t.org_units.is_empty());
}

#[test]
fn a_subplot_body_takes_the_first_value_of_each_dimension() {
    let coords = json!({"location": ["x", "y"], "horizon": [1, 2]});
    let body: serde_json::Value =
        serde_json::from_str(&subplot_body(&coords).expect("a body")).expect("json");
    assert_eq!(body, json!({"location": "x", "horizon": 1}));
    assert_eq!(subplot_body(&json!({"location": []})), None);
    assert_eq!(plot_ids(&json!([{"id": "a"}, {"id": "b"}])), vec!["a", "b"]);
}

#[test]
fn the_routes_follow_the_modeling_app() {
    let units: Vec<String> = (0..14).map(|i| format!("ou{i}")).collect();
    let t = Target {
        id: 12,
        dataset_id: Some(4),
        specification_id: Some(9),
        org_units: units,
    };
    let plots = vec![
        ("bias".to_string(), Some("{\"h\":1}".to_string())),
        ("grid".to_string(), None),
    ];
    let routes = routes(&t, &plots);
    let labels: Vec<&str> = routes.iter().map(|r| r.label.as_str()).collect();
    assert!(labels.contains(&"backtest-specification"), "{labels:?}");
    assert!(labels.contains(&"evaluation-entry adm0"), "{labels:?}");
    assert!(
        labels.contains(&"evaluation-entry 10 org units"),
        "{labels:?}"
    );
    assert!(labels.contains(&"dataset"), "{labels:?}");
    assert!(labels.contains(&"bias subplot"), "{labels:?}");
    assert!(labels.contains(&"grid facet-coords"), "{labels:?}");
    assert!(!labels.contains(&"grid subplot"), "{labels:?}");

    let page = routes
        .iter()
        .find(|r| r.label == "evaluation-entry 10 org units")
        .expect("the page request");
    assert!(page.path.contains("orgUnits=ou9"), "{}", page.path);
    assert!(!page.path.contains("orgUnits=ou10"), "{}", page.path);
    assert!(page.path.contains("quantiles=0.5"), "{}", page.path);

    let subplot = routes
        .iter()
        .find(|r| r.label == "bias subplot")
        .expect("a subplot");
    assert_eq!(subplot.method, "POST");
    assert_eq!(subplot.body.as_deref(), Some("{\"h\":1}"));
}

#[test]
fn a_backtest_without_org_units_or_dataset_leaves_those_out() {
    let t = Target {
        id: 1,
        dataset_id: None,
        specification_id: None,
        org_units: Vec::new(),
    };
    let labels: Vec<String> = routes(&t, &[]).into_iter().map(|r| r.label).collect();
    assert!(
        !labels.iter().any(|l| l.contains("org units")),
        "{labels:?}"
    );
    assert!(!labels.contains(&"dataset".to_string()), "{labels:?}");
    assert!(!labels.contains(&"backtest-specification".to_string()));
}

#[test]
fn percentiles_use_the_nearest_rank() {
    let sorted: Vec<f64> = (1..=20).map(f64::from).collect();
    assert_eq!(percentile(&sorted, 50.0), 10.0);
    assert_eq!(percentile(&sorted, 95.0), 19.0);
    assert_eq!(percentile(&sorted, 100.0), 20.0);
    assert_eq!(percentile(&[7.0], 95.0), 7.0);
    assert_eq!(percentile(&[], 50.0), 0.0);
}

#[test]
fn a_summary_counts_the_failures_and_keeps_their_time() {
    let ok = |ms| Timing {
        ms,
        status: Some(200),
        bytes: 10,
        error: None,
    };
    let timings = vec![
        ok(30.0),
        ok(10.0),
        Timing {
            ms: 120_000.0,
            status: None,
            bytes: 0,
            error: Some("no answer".to_string()),
        },
    ];
    let r = summarize("x", "GET", "/x", &timings);
    assert_eq!(r.requests, 3);
    assert_eq!(r.failed, 1);
    assert_eq!(r.min_ms, 10.0);
    assert_eq!(r.p50_ms, 30.0);
    assert_eq!(r.max_ms, 120_000.0);
    assert_eq!(r.statuses.get("200"), Some(&2));
    assert_eq!(r.statuses.get("no answer"), Some(&1));
    assert_eq!(r.error.as_deref(), Some("no answer"));
}

#[test]
fn durations_and_sizes_read_well() {
    assert_eq!(human_ms(12.4), "12 ms");
    assert_eq!(human_ms(1240.0), "1.24 s");
    assert_eq!(human_bytes(512), "512 B");
    assert_eq!(human_bytes(12_300), "12.3 kB");
    assert_eq!(human_bytes(4_500_000), "4.5 MB");
}

#[test]
fn one_round_reads_once_each() {
    assert_eq!(times_each(1), "once each");
    assert_eq!(times_each(3), "3 times each");
}

#[test]
fn a_backtest_has_one_forecast_for_each_org_unit_split_and_period() {
    let row = json!({"orgUnits": ["a", "b", "c"], "splitPeriods": ["1", "2"], "nPeriods": 12});
    assert_eq!(expected_forecasts(&row), Some(72));
    assert_eq!(expected_forecasts(&json!({"orgUnits": []})), None);
}

#[test]
fn a_backtest_fails_on_missing_or_wrong_forecasts_or_no_metrics() {
    assert_eq!(check(Some(72), Some(72), 13), None);
    assert!(check(None, Some(72), 13).unwrap().contains("did not give"));
    assert!(
        check(Some(0), Some(72), 13)
            .unwrap()
            .contains("no forecasts")
    );
    assert_eq!(
        check(Some(70), Some(72), 13).unwrap(),
        "it stored 70 forecasts, and its org units, splits and periods make 72"
    );
    assert!(
        check(Some(72), Some(72), 0)
            .unwrap()
            .contains("no aggregate metrics")
    );
    // Without the row's own numbers, any forecasts at all will do.
    assert_eq!(check(Some(5), None, 1), None);
}

#[test]
fn the_cleanup_line_counts_each_backtest_once() {
    assert_eq!(
        deleted_line(Some(3), &[7, 7, 8]),
        "deleted dataset 3 and the 2 backtests the run made; --keep keeps them"
    );
    assert_eq!(
        deleted_line(None, &[7]),
        "deleted the 1 backtest the run made; --keep keeps them"
    );
}

#[test]
fn an_empty_list_names_the_hidden_baselines_and_the_way_out() {
    let text = no_backtest("http://localhost:8700");
    assert!(text.contains("lists no backtest to time"), "{text}");
    assert!(text.contains("seasonal_median"), "{text}");
    assert!(text.contains("`chap-devops bench ID`"), "{text}");
}
