use super::measure::*;
use super::plan::*;
use super::write::*;
use crate::cli::BenchPage;
use serde_json::json;

#[test]
fn the_newest_is_the_highest_id() {
    let list = json!([{"id": 3}, {"id": 12}, {"id": 7}]);
    assert_eq!(newest_id(&list), Some(12));
    assert_eq!(newest_id(&json!([])), None);
    assert_eq!(newest_id(&json!({"detail": "x"})), None);
}

#[test]
fn the_version_reads_release_and_dev_versions() {
    let v = |text: &str| version(&json!({ "chap_core_version": text }));
    assert_eq!(v("2.7.1"), Some((2, 7, 1)));
    assert_eq!(v("v2.6.0"), Some((2, 6, 0)));
    assert_eq!(v("2.8.0.dev3"), Some((2, 8, 0)));
    assert_eq!(v("2.5"), Some((2, 5, 0)));
    assert_eq!(v("main"), None);
    assert_eq!(version(&json!({})), None);
}

#[test]
fn the_backtest_reads_camel_case_fields() {
    let info = json!({
        "id": 12,
        "datasetId": 4,
        "specificationId": 9,
        "orgUnits": ["a", "b"],
        "aggregateMetrics": {"crps": 1.0},
    });
    let b = backtest(12, &info);
    assert_eq!(b.dataset_id, Some(4));
    assert_eq!(b.specification_id, Some(9));
    assert_eq!(b.org_units, vec!["a".to_string(), "b".to_string()]);
    assert!(b.has_metrics);

    // The dataset can also come as a nested summary.
    let b = backtest(12, &json!({"dataset": {"id": 5}, "aggregateMetrics": {}}));
    assert_eq!(b.dataset_id, Some(5));
    assert!(b.org_units.is_empty());
    assert!(!b.has_metrics);
}

#[test]
fn the_specification_lists_the_baselines_that_are_done() {
    let spec = json!({
        "metrics": null,
        "backtests": [{"id": 31}, {"id": 32}],
        "baselines": [
            {"backtestId": 40, "status": "done"},
            {"backtestId": 41, "status": "pending"},
            {"backtestId": 42, "status": "done"},
        ],
    });
    let s = specification(9, &spec);
    assert!(!s.has_metrics);
    assert_eq!(s.first_backtest, Some(31));
    assert_eq!(s.baselines_done, vec![40, 42]);
    assert!(specification(9, &json!({"metrics": []})).has_metrics);
}

#[test]
fn the_prediction_takes_its_own_org_units_else_its_datasets() {
    let own = prediction(
        3,
        &json!({"orgUnits": ["a"], "dataset": {"id": 4, "orgUnits": ["a", "b"]}}),
    );
    assert_eq!(own.dataset_id, Some(4));
    assert_eq!(own.org_units, vec!["a".to_string()]);
    let all = prediction(
        3,
        &json!({"orgUnits": [], "datasetId": 4, "dataset": {"orgUnits": ["a", "b"]}}),
    );
    assert_eq!(all.dataset_id, Some(4));
    assert_eq!(all.org_units.len(), 2);
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

fn facts(version: Option<(u64, u64, u64)>) -> Facts {
    Facts {
        version,
        backtest: Some(super::plan::Backtest {
            id: 12,
            dataset_id: Some(4),
            specification_id: Some(9),
            org_units: (0..14).map(|i| format!("ou{i}")).collect(),
            has_metrics: true,
        }),
        specification: Some(Specification {
            id: 9,
            has_metrics: false,
            first_backtest: Some(12),
            baselines_done: vec![40],
        }),
        prediction: Some(Prediction {
            id: 3,
            dataset_id: Some(4),
            org_units: (0..18).map(|i| format!("p{i}")).collect(),
        }),
        plots: vec![
            ("bias".to_string(), Some("{\"h\":1}".to_string())),
            ("grid".to_string(), None),
        ],
    }
}

fn find<'a>(routes: &'a [Route], page: BenchPage, label: &str) -> Option<&'a Route> {
    routes.iter().find(|r| r.page == page && r.label == label)
}

#[test]
fn the_prediction_page_asks_for_the_cases_of_every_org_unit_of_the_dataset() {
    let routes = routes(&facts(None), &[BenchPage::Prediction]);
    assert!(routes.iter().all(|r| r.page == BenchPage::Prediction));
    let cases = find(&routes, BenchPage::Prediction, "actualCases 18 org units").expect("cases");
    assert!(
        cases
            .path
            .starts_with("/v1/analytics/actualCases/4?orgUnits=p0&"),
        "{}",
        cases.path
    );
    assert!(
        cases.path.contains("orgUnits=p17&isDatasetId=true"),
        "{}",
        cases.path
    );
    assert_eq!(cases.step, 2);
    assert_eq!(
        find(&routes, BenchPage::Prediction, "system info")
            .expect("info")
            .step,
        0
    );
    assert_eq!(
        find(&routes, BenchPage::Prediction, "prediction")
            .expect("row")
            .step,
        1
    );
    let entry = find(&routes, BenchPage::Prediction, "prediction-entry").expect("entry");
    assert!(
        entry.path.contains("/prediction-entry/3?quantiles=0.1"),
        "{}",
        entry.path
    );
    assert!(find(&routes, BenchPage::Prediction, "alerts").is_some());
}

#[test]
fn the_evaluation_page_asks_for_the_whole_country_only() {
    let routes = routes(&facts(None), &[BenchPage::Evaluation]);
    let entry = find(&routes, BenchPage::Evaluation, "evaluation-entry adm0").expect("entry");
    assert!(entry.path.contains("quantiles=0.5"), "{}", entry.path);
    assert!(entry.path.ends_with("orgUnits=adm0"), "{}", entry.path);
    let cases = find(&routes, BenchPage::Evaluation, "actualCases adm0").expect("cases");
    assert_eq!(cases.path, "/v1/analytics/actualCases/12?orgUnits=adm0");
    let spec = find(&routes, BenchPage::Evaluation, "backtest-specification").expect("spec");
    assert!(
        spec.path
            .ends_with("includeComparisonModels=true&includeArchivedModels=true")
    );
    // The specification has no metrics and the backtest has: the metrics
    // request of a chap-core before 2.6.
    assert!(find(&routes, BenchPage::Evaluation, "metrics").is_some());
    // The dataset with every observation is the dataset page, not this one.
    assert!(
        !routes
            .iter()
            .any(|r| r.path.starts_with("/v1/crud/datasets"))
    );
}

#[test]
fn the_compare_page_takes_the_first_10_org_units_and_the_baselines() {
    let routes = routes(&facts(None), &[BenchPage::Compare]);
    let entry = find(&routes, BenchPage::Compare, "evaluation-entry 10 org units").expect("entry");
    assert!(entry.path.contains("orgUnits=ou9"), "{}", entry.path);
    assert!(!entry.path.contains("orgUnits=ou10"), "{}", entry.path);
    assert!(find(&routes, BenchPage::Compare, "baseline 40 info").is_some());
    assert!(find(&routes, BenchPage::Compare, "compatible-backtests").is_some());
}

#[test]
fn the_plots_page_has_a_subplot_only_where_the_coordinates_give_one() {
    let routes = routes(&facts(None), &[BenchPage::Plots]);
    let subplot = find(&routes, BenchPage::Plots, "bias subplot").expect("a subplot");
    assert_eq!(subplot.method, "POST");
    assert_eq!(subplot.body.as_deref(), Some("{\"h\":1}"));
    assert!(find(&routes, BenchPage::Plots, "grid facet-coords").is_some());
    assert!(find(&routes, BenchPage::Plots, "grid subplot").is_none());
}

#[test]
fn an_older_chap_core_gets_only_the_requests_its_version_has() {
    let old = routes(&facts(Some((2, 3, 0))), &BenchPage::ALL);
    let new = routes(&facts(Some((2, 6, 0))), &BenchPage::ALL);
    for label in [
        "backtest-parameters",
        "weather-providers",
        "alerts",
        "outbreak-metrics",
    ] {
        assert!(!old.iter().any(|r| r.label == label), "{label}");
        assert!(new.iter().any(|r| r.label == label), "{label}");
    }
}

#[test]
fn a_page_without_what_it_needs_is_left_out_with_the_reason() {
    let mut f = facts(None);
    f.prediction = None;
    f.specification = None;
    f.backtest.as_mut().expect("a backtest").dataset_id = None;
    assert_eq!(
        missing(BenchPage::Prediction, &f),
        Some("there is no prediction")
    );
    assert!(missing(BenchPage::Benchmark, &f).is_some());
    assert_eq!(
        missing(BenchPage::Dataset, &f),
        Some("the backtest names no dataset")
    );
    let pages: Vec<BenchPage> = routes(&f, &BenchPage::ALL).iter().map(|r| r.page).collect();
    assert!(pages.contains(&BenchPage::Evaluation));
    assert!(!pages.contains(&BenchPage::Prediction));
    assert!(!pages.contains(&BenchPage::Dataset));
}

#[test]
fn a_page_opens_in_the_slowest_request_of_each_step_added_up() {
    let routes = routes(&facts(None), &[BenchPage::Dataset, BenchPage::Prediction]);
    let p50: Vec<f64> = routes
        .iter()
        .map(|r| match r.label.as_str() {
            "system info" => 5.0,
            "prediction" => 20.0,
            "configured-models" => 30.0,
            l if l.starts_with("actualCases") => 600.0,
            "dataset" => 900.0,
            _ => 10.0,
        })
        .collect();
    assert_eq!(
        page_ms(&routes, &p50, BenchPage::Prediction),
        5.0 + 30.0 + 600.0
    );
    assert_eq!(page_ms(&routes, &p50, BenchPage::Dataset), 5.0 + 900.0);
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
        wire_bytes: 4,
        error: None,
    };
    let timings = vec![
        ok(30.0),
        ok(10.0),
        Timing {
            ms: 120_000.0,
            status: None,
            bytes: 0,
            wire_bytes: 0,
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

#[test]
fn a_prediction_has_one_forecast_for_each_org_unit_and_period() {
    use super::predict::{check, expected_entries};
    let row = json!({"orgUnits": ["a", "b", "c"], "nPeriods": 4});
    assert_eq!(expected_entries(&row), Some(12));
    assert_eq!(check(Some(12), Some(12)), None);
    assert!(check(Some(0), Some(12)).is_some_and(|p| p.contains("no forecasts")));
    assert!(check(Some(10), Some(12)).is_some_and(|p| p.contains("make 12")));
    assert!(check(None, Some(12)).is_some());
    assert_eq!(check(Some(5), None), None);
}

/// `docker compose top chap worker`, cut down from chap-core v2.7.0 on
/// Apple Silicon: gunicorn with one worker, celery with three.
const TOP: &str = "\
SERVICE  #   UID   PID   PPID  C   STIME  TTY  TIME      CMD
chap     1   1000  3481  3458  0   11:01  ?    00:00:00  /sbin/docker-init -- gunicorn -c gunicorn.conf.py --workers 1
chap     1   1000  3512  3481  3   11:01  ?    00:00:06  /run/rosetta/rosetta /app/.venv/bin/python /app/.venv/bin/gunicorn -c gunicorn.conf.py
chap     1   1000  3601  3512  14  11:01  ?    00:00:26  /run/rosetta/rosetta /app/.venv/bin/python /app/.venv/bin/gunicorn -c gunicorn.conf.py
worker   1   1002  3424  3399  0   11:01  ?    00:00:00  /sbin/docker-init -- /app/.venv/bin/celery -A chap_core.rest_api.celery_tasks worker
worker   1   1002  3449  3424  7   11:01  ?    00:00:14  /app/.venv/bin/python /app/.venv/bin/celery -A chap_core.rest_api.celery_tasks worker
worker   1   1002  3592  3449  0   11:01  ?    00:00:00  /app/.venv/bin/python /app/.venv/bin/celery -A chap_core.rest_api.celery_tasks worker
worker   1   1002  3593  3449  0   11:01  ?    00:00:00  /app/.venv/bin/python /app/.venv/bin/celery -A chap_core.rest_api.celery_tasks worker
worker   1   1002  5046  3449  0   11:03  ?    00:00:00  /app/.venv/bin/python /app/.venv/bin/celery -A chap_core.rest_api.celery_tasks worker
";

#[test]
fn the_workers_are_the_children_of_the_main_process() {
    use super::server::workers;
    assert_eq!(workers(TOP, "chap"), Some(1));
    assert_eq!(workers(TOP, "worker"), Some(3));
    assert_eq!(workers(TOP, "postgres"), None);
    // Without docker's init, the main process is the one whose parent is
    // not in the service.
    let no_init: String = TOP
        .lines()
        .filter(|l| !l.contains("docker-init"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(workers(&no_init, "worker"), Some(3));
    assert_eq!(workers("", "chap"), None);
}

#[test]
fn the_server_line_says_what_was_measured() {
    use super::server::{Server, line};
    let full = Server {
        chap_core_version: Some("2.7.0".to_string()),
        revision: Some("62bae69ca9350f06".to_string()),
        api_workers: Some(1),
        worker_processes: Some(12),
        machine: "macos aarch64".to_string(),
        cpus: Some(12),
        docker_cpus: Some(12),
        docker_memory_bytes: Some(33_598_132_224),
        gzip: None,
        transport: Default::default(),
    };
    assert_eq!(
        line(&full),
        "measured on chap-core 2.7.0 (62bae69c), 1 API worker, 12 worker processes, \
         docker with 12 CPUs and 33.6 GB, macos aarch64 with 12 CPUs"
    );
    let remote = Server {
        chap_core_version: Some("2.8.0".to_string()),
        machine: "linux x86_64".to_string(),
        ..Server::default()
    };
    assert_eq!(line(&remote), "measured on chap-core 2.8.0, linux x86_64");

    // Through a proxy that unpacks: the line says so, and names the proxy.
    let proxied = Server {
        gzip: Some(false),
        transport: crate::api::Transport {
            server: Some("nginx/1.27".to_string()),
            via: Some("1.1 dhis2-route".to_string()),
            ..Default::default()
        },
        ..remote
    };
    assert_eq!(
        line(&proxied),
        "measured on chap-core 2.8.0, answers not packed, though gzip was asked for, \
         server `nginx/1.27`, via `1.1 dhis2-route`, linux x86_64"
    );
}

#[test]
fn an_answer_says_whether_it_came_packed_only_when_it_is_large_enough() {
    use super::server::encoding;
    let answer = |size: usize, encoding: Option<&str>| crate::api::Answer {
        status: 200,
        reason: "OK".to_string(),
        content_type: "application/json".to_string(),
        body: vec![b' '; size],
        wire_bytes: size,
        transport: crate::api::Transport {
            content_encoding: encoding.map(str::to_string),
            ..Default::default()
        },
    };
    assert_eq!(encoding(&answer(5000, Some("gzip"))).0, Some(true));
    assert_eq!(encoding(&answer(5000, None)).0, Some(false));
    // chap-core does not pack an answer under 1,000 bytes.
    assert_eq!(encoding(&answer(200, None)).0, None);
}

#[test]
fn a_job_runs_from_its_start_to_its_end_to_the_millisecond() {
    use super::runs::job_times;
    let job = crate::jobs::Job {
        start_time: Some("2026-11-13T11:00:00.250000".to_string()),
        end_time: Some("2026-11-13T11:00:02.750000+00:00".to_string()),
        ..crate::jobs::Job::default()
    };
    let (start, end) = job_times(&job);
    assert_eq!(end.zip(start).map(|(e, s)| e - s), Some(2.5));
    let queued = crate::jobs::Job::default();
    assert_eq!(job_times(&queued), (None, None));
}

#[test]
fn a_child_run_has_the_same_arguments_without_workers_and_with_json() {
    use super::compare::child_args;
    let args: Vec<String> = [
        "bench",
        "--workers",
        "1,4",
        "--page",
        "prediction",
        "--json",
    ]
    .map(str::to_string)
    .to_vec();
    assert_eq!(
        child_args(&args),
        ["bench", "--page", "prediction", "--json"]
    );
    let args: Vec<String> = ["-C", "x", "bench", "--workers=1,2"]
        .map(str::to_string)
        .to_vec();
    assert_eq!(child_args(&args), ["-C", "x", "bench", "--json"]);
}

#[test]
fn the_comparison_has_a_row_for_each_measure_and_a_column_for_each_count() {
    use super::compare::rows;
    let one = json!({"pages": [{"page": "prediction", "ms": 900.0}],
        "routes": [{"page": "prediction", "label": "prediction-entry", "p50_ms": 400.0}],
        "seconds": 3.0});
    let four =
        json!({"pages": [{"page": "prediction", "ms": 500.0}], "routes": [], "seconds": 2.0});
    let rows = rows(&[(1, 1, one), (4, 4, four)]);
    let find = |label: &str| {
        rows.iter()
            .find(|(l, _)| l == label)
            .map(|(_, c)| c.clone())
    };
    assert_eq!(
        find("prediction page"),
        Some(vec![Some(900.0), Some(500.0)])
    );
    assert_eq!(
        find("prediction prediction-entry p50"),
        Some(vec![Some(400.0), None])
    );
    assert_eq!(find("whole run"), Some(vec![Some(3000.0), Some(2000.0)]));
}
