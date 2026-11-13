//! Tests of what `bench` says about the server, and of the series:
//! `--workers`, `--sizes`, `--resources` and `--second-user`.

use serde_json::json;

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
    use super::super::server::workers;
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
    use super::super::server::{Server, line};
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
    use super::super::server::encoding;
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
    use super::super::runs::job_times;
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
    use super::super::compare::child_args;
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
    use super::super::compare::rows;
    let one = json!({"pages": [{"page": "prediction", "ms": 900.0}],
        "routes": [{"page": "prediction", "label": "prediction-entry", "p50_ms": 400.0}],
        "seconds": 3.0});
    let four =
        json!({"pages": [{"page": "prediction", "ms": 500.0}], "routes": [], "seconds": 2.0});
    let rows = rows(&[one, four]);
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

#[test]
fn docker_stats_numbers_read_as_cores_and_bytes() {
    use super::super::resources::{parse_memory, parse_percent};
    assert_eq!(parse_percent("32.25%"), Some(32.25));
    assert_eq!(parse_memory("1.5GiB / 31.29GiB"), Some(1_610_612_736));
    assert_eq!(parse_memory("370.4MiB / 31.29GiB"), Some(388_392_550));
    assert_eq!(parse_memory("12kB / 1GB"), Some(12_000));
    assert_eq!(parse_memory("--"), None);
}

#[test]
fn a_phase_uses_the_samples_inside_it() {
    use super::super::resources::{Sample, usage};
    let sample = |at: f64, cores: f64, memory_bytes: u64| Sample {
        at,
        service: "worker".to_string(),
        cores,
        memory_bytes,
    };
    let samples = vec![
        sample(0.5, 9.0, 9),
        sample(1.0, 2.0, 100),
        sample(2.0, 4.0, 300),
        sample(3.0, 0.0, 200),
        sample(9.0, 9.0, 999),
    ];
    let u = usage(&samples, "worker", 1.0, 3.0);
    assert_eq!(u.samples, 3);
    assert_eq!(u.average_cores, 2.0);
    assert_eq!(u.cpu_seconds, 4.0);
    assert_eq!(u.peak_cores, 4.0);
    assert_eq!(u.peak_memory_bytes, 300);
    assert_eq!(usage(&samples, "chap", 0.0, 9.0).samples, 0);
}

#[test]
fn postgres_and_valkey_counters_give_what_grew_over_a_phase() {
    use super::super::resources::{delta, parse_postgres, parse_valkey};
    let before = parse_postgres("711|5057577|4715737|2989531|209|250326|59963|30486876|0\n");
    let after = parse_postgres("720|5060000|4715737|3000000|209|250326|59963|30486900|4096\n");
    let grew = delta(&before, &after);
    assert_eq!(grew["xact_commit"], 9);
    assert_eq!(grew["tup_inserted"], 10469);
    assert_eq!(grew["temp_bytes"], 4096);

    let info = "# Stats\r\ntotal_commands_processed:9510\r\nused_memory_peak:806966976\r\nredis_version:7.2.4\r\n";
    let valkey = parse_valkey(info);
    assert_eq!(valkey["total_commands_processed"], 9510);
    assert_eq!(valkey["used_memory_peak"], 806966976);
    assert!(!valkey.contains_key("redis_version"));
    // The peak is a level, not a counter: it is not a difference.
    assert!(!delta(&valkey, &valkey).contains_key("used_memory_peak"));
}

#[test]
fn a_size_reads_as_locations_periods_and_a_period_type() {
    use crate::cli::Size;
    use crate::synthetic::PeriodType;
    let size: Size = "1500x365w".parse().expect("a size");
    assert_eq!(
        (size.locations, size.periods, size.period_type),
        (1500, 365, PeriodType::Week)
    );
    assert_eq!(size.to_string(), "1500x365w");
    let size: Size = "3000X120m".parse().expect("a size");
    assert_eq!(size.period_type, PeriodType::Month);
    for wrong in ["1500x365", "x10w", "10x1w", "0x10w", "big"] {
        assert!(wrong.parse::<Size>().is_err(), "{wrong}");
    }
    assert_eq!(
        super::super::sizes::size_args(&"20x30m".parse().expect("a size")),
        [
            "--locations",
            "20",
            "--periods",
            "30",
            "--period-type",
            "month"
        ]
    );
}

#[test]
fn a_child_run_leaves_out_the_options_of_the_series() {
    use super::super::compare::child_args;
    let args: Vec<String> = [
        "bench",
        "--write",
        "--sizes",
        "1x2w,3x4m",
        "--fresh-database",
        "--locations=9",
        "--periods",
        "8",
        "--model",
        "m",
    ]
    .map(str::to_string)
    .to_vec();
    assert_eq!(
        child_args(&args),
        ["bench", "--write", "--model", "m", "--json"]
    );
}

#[test]
fn the_second_user_sees_each_phase_from_the_requests_that_started_in_it() {
    use super::super::second_user::{Request, line, summarize};
    let request = |at: f64, ms: f64, ok: bool| Request { at, ms, ok };
    let requests = vec![
        request(0.0, 10.0, true),
        request(0.5, 4000.0, true),
        request(1.0, 20.0, false),
        request(5.0, 15.0, true),
    ];
    let phases = vec![("import".to_string(), 0.0), ("backtests".to_string(), 2.0)];
    let seen = summarize(&requests, &phases);
    assert_eq!(seen[0].requests, 3);
    assert_eq!(seen[0].failed, 1);
    assert_eq!(seen[0].max_ms, 4000.0);
    assert_eq!(seen[1].requests, 1);
    assert_eq!(
        line(&seen[0]),
        "second user, import: 3 requests of configured-models, p50 20 ms, p95 4.00 s, max 4.00 s, 1 failed"
    );
}
