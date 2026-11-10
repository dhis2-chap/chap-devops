use super::*;

fn job(kind: &str, status: &str, start: Option<&str>, result: Option<&str>) -> Job {
    Job {
        id: format!("{kind}-{status}"),
        kind: kind.to_string(),
        status: status.to_string(),
        start_time: start.map(str::to_string),
        result: result.map(str::to_string),
        ..Job::default()
    }
}

const SINCE: &str = "2026-10-15T16:15:35";

fn since() -> u64 {
    crate::status::parse_rfc3339(SINCE).expect("a time")
}

#[test]
fn the_finished_baselines_of_the_run_give_their_backtests() {
    let list = [
        job(
            "run_baseline",
            "SUCCESS",
            Some("2026-10-15T16:15:38"),
            Some("3"),
        ),
        job(
            "run_baseline",
            "SUCCESS",
            Some("2026-10-15T16:15:39"),
            Some("2"),
        ),
        // A baseline of an earlier run, and a backtest of the model itself.
        job(
            "run_baseline",
            "SUCCESS",
            Some("2026-10-15T10:00:00"),
            Some("1"),
        ),
        job(
            "create_backtest",
            "SUCCESS",
            Some("2026-10-15T16:15:36"),
            Some("4"),
        ),
        // A failed baseline wrote no backtest.
        job("run_baseline", "FAILURE", Some("2026-10-15T16:15:38"), None),
    ];
    assert_eq!(of_run(&list, since()), (false, vec![3, 2]));
}

#[test]
fn a_queued_or_running_baseline_is_waited_for() {
    let queued = [job("run_baseline", "PENDING", None, None)];
    assert_eq!(of_run(&queued, since()), (true, vec![]));
    let running = [job(
        "run_baseline",
        "STARTED",
        Some("2026-10-15T16:15:38"),
        None,
    )];
    assert!(of_run(&running, since()).0);
    // One that runs for an earlier run is not this run's to wait for.
    let earlier = [job(
        "run_baseline",
        "STARTED",
        Some("2026-10-15T10:00:00"),
        None,
    )];
    assert!(!of_run(&earlier, since()).0);
}

#[test]
fn chap_core_before_the_baselines_has_none() {
    let list = [job(
        "create_backtest",
        "SUCCESS",
        Some("2026-10-15T16:15:36"),
        Some("4"),
    )];
    assert_eq!(of_run(&list, since()), (false, vec![]));
}
