use super::*;

/// Epoch seconds for `2026-09-24T16:45:00Z`, the moment these tests are
/// read from.
const NOW: u64 = 1_790_268_300;

fn at(offset: i64) -> String {
    // The wire format chap-core writes: no zone, which is UTC.
    let secs = (NOW as i64 + offset) as u64;
    let stamp = crate::backup::timestamp(secs);
    stamp.trim_end_matches('Z').to_string()
}

fn job(id: &str, status: &str, started: i64, ran_for: Option<i64>) -> Job {
    Job {
        id: id.to_string(),
        kind: "create_backtest".to_string(),
        name: "eval".to_string(),
        status: status.to_string(),
        start_time: Some(at(started)),
        end_time: ran_for.map(|d| at(started + d)),
        result: None,
        prediction_setup_id: None,
    }
}

#[test]
fn the_wire_description_parses_with_its_snake_case_names() {
    let value: Job = serde_json::from_str(
        r#"{"id":"f293520a-196a-480b-b4e0-67fc9bb67b25","type":"create_prediction",
                "name":"eval-make-prediction-ewars","status":"SUCCESS",
                "start_time":"2026-09-24T16:34:12.911182",
                "end_time":"2026-09-24T16:34:27.411712","result":"2",
                "prediction_setup_id":7}"#,
    )
    .expect("the shape chap-core sends");
    assert_eq!(value.kind, "create_prediction");
    assert_eq!(value.name, "eval-make-prediction-ewars");
    assert_eq!(value.prediction_setup_id, Some(7));
    assert_eq!(value.duration(), Some(Duration::from_secs(15)));
    assert_eq!(value.outcome(), Outcome::Done);

    // A description with only the required fields still reads, and one
    // with a field this CLI has never seen does too.
    let sparse: Job = serde_json::from_str(
        r#"{"id":"x","type":"t","name":"n","status":"PENDING","start_time":null,
                "end_time":null,"result":null,"queue":"celery"}"#,
    )
    .expect("unknown fields are chap-core's to add");
    assert_eq!(sparse.started_at(), None);
    assert_eq!(sparse.duration(), None);
}

#[test]
fn the_statuses_fall_into_four_buckets() {
    assert_eq!(Outcome::of("SUCCESS"), Outcome::Done);
    assert_eq!(Outcome::of("FAILURE"), Outcome::Failed);
    assert_eq!(Outcome::of("REVOKED"), Outcome::Cancelled);
    for running in ["PENDING", "STARTED", "RETRY", "pending", "SOMETHING_NEW"] {
        assert_eq!(Outcome::of(running), Outcome::Running, "{running}");
        assert!(Outcome::of(running).is_running());
    }
}

#[test]
fn the_newest_job_is_first_and_a_running_one_leads_its_second() {
    let mut jobs = vec![
        job("cccccccc-3", "SUCCESS", -600, Some(30)),
        job("aaaaaaaa-1", "SUCCESS", -60, Some(10)),
        job("bbbbbbbb-2", "STARTED", -60, None),
        job("dddddddd-4", "PENDING", 0, None),
    ];
    // The queued job has no start time at all.
    jobs[3].start_time = None;
    let sorted: Vec<&str> = order(&jobs).iter().map(|i| jobs[*i].id.as_str()).collect();
    assert_eq!(
        sorted,
        vec!["dddddddd-4", "bbbbbbbb-2", "aaaaaaaa-1", "cccccccc-3"]
    );
}

#[test]
fn ids_are_shortened_only_while_eight_characters_tell_them_apart() {
    let distinct = vec![
        job("f293520a-196a-480b", "SUCCESS", -60, Some(1)),
        job("ed3a0719-9eb1-4f1f", "SUCCESS", -60, Some(1)),
    ];
    assert!(ids_are_short(&distinct));
    assert_eq!(id_cell(&distinct[0].id, true), "f293520a...");
    assert_eq!(id_cell(&distinct[0].id, false), "f293520a-196a-480b");

    let colliding = vec![
        job("f293520a-196a-480b", "SUCCESS", -60, Some(1)),
        job("f293520a-9eb1-4f1f", "SUCCESS", -60, Some(1)),
    ];
    assert!(!ids_are_short(&colliding), "the prefixes are the same");

    // And an id that is already short is never cut.
    let tiny = vec![job("abc", "SUCCESS", -60, Some(1))];
    assert!(!ids_are_short(&tiny));
    assert_eq!(id_cell("abc", true), "abc");
}

#[test]
fn a_row_holds_the_six_columns_in_the_headers_order() {
    let jobs = vec![job("f293520a-196a-480b", "SUCCESS", -180, Some(129))];
    let rows = rows(&jobs, NOW);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), HEADERS.len());
    assert_eq!(
        rows[0],
        vec![
            "f293520a...",
            "create_backtest",
            "eval",
            "SUCCESS",
            "3m ago",
            "2m 9s",
        ]
    );
}

#[test]
fn a_running_job_has_no_duration_and_a_queued_one_has_no_start() {
    let mut running = job("aaaaaaaa-1", "STARTED", -45, None);
    running.name = String::new();
    let mut queued = job("bbbbbbbb-2", "PENDING", 0, None);
    queued.start_time = None;
    let rows = rows(&[running, queued], NOW);
    assert_eq!(rows[0][2], "-", "an empty name is a dash, not a blank");
    assert_eq!(rows[0][4], "45s ago");
    assert_eq!(rows[0][5], "-");
    assert_eq!(rows[1][4], "queued");
    assert_eq!(rows[1][5], "-");
}

#[test]
fn a_duration_shows_two_units_at_most() {
    assert_eq!(took_text(Duration::from_secs(0)), "0s");
    assert_eq!(took_text(Duration::from_secs(59)), "59s");
    assert_eq!(took_text(Duration::from_secs(60)), "1m 0s");
    assert_eq!(took_text(Duration::from_secs(129)), "2m 9s");
    assert_eq!(took_text(Duration::from_secs(3599)), "59m 59s");
    assert_eq!(took_text(Duration::from_secs(3600)), "1h 0m");
    assert_eq!(took_text(Duration::from_mins(123)), "2h 3m");
}

#[test]
fn the_closing_line_counts_only_the_buckets_that_hold_something() {
    let jobs = vec![
        job("a", "STARTED", -10, None),
        job("b", "SUCCESS", -20, Some(1)),
        job("c", "SUCCESS", -30, Some(1)),
        job("d", "FAILURE", -40, Some(1)),
    ];
    assert_eq!(summary(&jobs), "4 jobs: 1 running, 2 done, 1 failed");
    assert_eq!(
        failure_hint(&jobs).unwrap(),
        "run `chap-devops jobs logs d` to see why"
    );

    let clean = vec![job("a", "SUCCESS", -10, Some(1))];
    assert_eq!(summary(&clean), "1 job: 1 done");
    assert_eq!(failure_hint(&clean), None);

    assert_eq!(summary(&[]), "0 jobs");

    let two_bad = vec![
        job("a", "FAILURE", -10, Some(1)),
        job("b", "FAILURE", -20, Some(1)),
        job("c", "REVOKED", -30, Some(1)),
    ];
    assert_eq!(summary(&two_bad), "3 jobs: 2 failed, 1 cancelled");
    assert_eq!(
        failure_hint(&two_bad).unwrap(),
        "run `chap-devops jobs logs <id>` to see why"
    );
}

#[test]
fn an_id_matches_exactly_or_by_a_unique_prefix() {
    let jobs = vec![
        job("f293520a-196a-480b", "SUCCESS", -10, Some(1)),
        job("f29ffffe-9eb1-4f1f", "SUCCESS", -20, Some(1)),
        job("ed3a0719-0000-0000", "SUCCESS", -30, Some(1)),
    ];
    assert_eq!(
        resolve(&jobs, "f293520a-196a-480b"),
        Matched::One("f293520a-196a-480b".into())
    );
    assert_eq!(
        resolve(&jobs, "f293"),
        Matched::One("f293520a-196a-480b".into())
    );
    assert_eq!(
        resolve(&jobs, "ED3A"),
        Matched::One("ed3a0719-0000-0000".into()),
        "a hex id pasted back in upper case still matches"
    );
    assert_eq!(resolve(&jobs, "f29"), Matched::Many(2));
    assert_eq!(resolve(&jobs, "zzz"), Matched::None);
    assert_eq!(resolve(&jobs, "  "), Matched::None);
    assert_eq!(resolve(&[], "f293"), Matched::None);

    let err = no_such_job("zzz", &Matched::None).to_string();
    assert_eq!(
        err,
        "job zzz not found; run `chap-devops jobs` to list them"
    );
    let err = no_such_job("f29", &Matched::Many(2)).to_string();
    assert!(err.starts_with("job f29 matches 2 jobs;"), "{err}");
}

/// An exact id always wins, even when it is also the prefix of another.
#[test]
fn an_exact_id_is_never_read_as_a_prefix() {
    let jobs = vec![
        job("abcd", "SUCCESS", -10, Some(1)),
        job("abcdef", "SUCCESS", -20, Some(1)),
    ];
    assert_eq!(resolve(&jobs, "abcd"), Matched::One("abcd".into()));
    assert_eq!(resolve(&jobs, "abcde"), Matched::One("abcdef".into()));
}

#[test]
fn the_tail_is_the_last_lines_and_the_whole_of_a_shorter_log() {
    let log = "one\ntwo\nthree\nfour\n";
    assert_eq!(tail(log, 2), "three\nfour\n");
    assert_eq!(tail(log, 10), log);
    assert_eq!(tail(log, 0), "");
    assert_eq!(tail("only", 2), "only");
}

#[test]
fn the_failure_hint_is_the_last_error_line_of_the_stderr_section() {
    // The shape a real R model failure has: the error, the call, then a
    // page of warnings and a bare `Execution halted`.
    let log = "\
Fitting INLA model...

--- stderr ---
Loading required package: Matrix
sh: 1: /usr/local/lib/R/site-library/INLA/bin/linux/64bit/inla.mkl.run: Permission denied
Error in inla.inlaprogram.has.crashed() :
  The inla-program exited with an error.
Calls: predict_chap -> inla -> inla.core
In addition: Warning messages:
1: In poly2nb(polygons, queen = FALSE) :
  some observations have no neighbours;
Execution halted
";
    let hint = stderr_hint(log).expect("a stderr section");
    assert_eq!(hint, "The inla-program exited with an error.");

    // No stderr section at all: nothing to say.
    assert_eq!(stderr_hint("Traceback (most recent call last):"), None);

    // A section with no error-looking line falls back to its last useful
    // line rather than saying nothing.
    assert_eq!(
        stderr_hint("--- stderr ---\nloading\nall done\n\n").unwrap(),
        "all done"
    );

    // And one that holds only warnings and a terminator has nothing left.
    assert_eq!(
        stderr_hint("--- stderr ---\nWarning: x\nExecution halted\n"),
        None
    );
}

#[test]
fn a_very_long_stderr_line_is_cut_rather_than_printed_whole() {
    let long = "x".repeat(400);
    let hint = stderr_hint(&format!("--- stderr ---\nError: {long}\n")).unwrap();
    assert_eq!(hint.chars().count(), MAX_HINT + 3);
    assert!(hint.ends_with("..."));
}

#[test]
fn the_headline_names_the_job_and_survives_missing_fields() {
    let job = job("f293520a", "FAILURE", -10, Some(1));
    assert_eq!(
        job.headline(),
        "job f293520a FAILURE (create_backtest eval)"
    );
    let bare = Job {
        id: "x".into(),
        ..Job::default()
    };
    assert_eq!(bare.headline(), "job x - (- -)");
}

/// chap-core after v2.5.1 answers with the whole job; a status taken from the
/// raw text would then never read as finished.
#[test]
fn the_status_is_read_from_a_whole_job_and_from_a_bare_string() {
    assert_eq!(status_in(r#""SUCCESS""#), "SUCCESS");
    assert_eq!(
        status_in(r#"{"id":"j","type":"dataset","status":"SUCCESS","start_time":null}"#),
        "SUCCESS"
    );
    assert_eq!(status_in("SUCCESS\n"), "SUCCESS");
    assert_eq!(status_in(r#"{"id":"j"}"#), "");
}
