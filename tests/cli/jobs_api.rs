use crate::common::*;
use predicates::prelude::PredicateBooleanExt;
use serde_json::Value as Json;
use std::path::PathBuf;

/// A project whose API port is the one the stand-in chap-core listens on.
fn served_project(sandbox: &Sandbox) -> PathBuf {
    let port = chap_core_server();
    sandbox
        .init(&["--models", "none", "--api-port", &port.to_string()])
        .assert()
        .success();
    sandbox.project()
}

/// Every request to chap-core takes the token the way `chap-devops api` does:
/// `.env` first, then an exported `CHAP_API_TOKEN`. `jobs` used to read `.env`
/// alone, so on a deployment whose token lives in the shell `chap-devops api GET
/// /v1/jobs` worked and `chap-devops jobs` was refused.
#[test]
fn jobs_sends_the_exported_token_when_env_has_none() {
    let sandbox = Sandbox::new();
    let port = protected_chap_core_server();
    sandbox
        .init(&["--models", "none", "--api-port", &port.to_string()])
        .assert()
        .success();
    let dir = sandbox.project();

    chap_in(&sandbox, &dir, &["jobs"])
        .env_remove("CHAP_API_TOKEN")
        .assert()
        .failure();
    chap_in(&sandbox, &dir, &["jobs"])
        .env("CHAP_API_TOKEN", "from-the-shell")
        .assert()
        .success()
        .stdout(predicates::str::contains("3 jobs"));
}

#[test]
fn jobs_lists_what_chap_core_has_run_newest_first() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    let out = chap_in(&sandbox, &dir, &["jobs"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).expect("the table is text");

    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("ID  "), "{text}");
    for header in ["TYPE", "NAME", "STATUS", "STARTED", "DURATION"] {
        assert!(lines[0].contains(header), "{header} is missing:\n{text}");
    }
    // Newest first, and the ids are short because eight characters tell these
    // three apart.
    assert!(lines[1].starts_with("33333333..."), "{text}");
    assert!(lines[2].starts_with("22222222..."), "{text}");
    assert!(lines[3].starts_with("11111111..."), "{text}");
    assert!(
        !text.contains(RUNNING_ID),
        "the full id is not needed:\n{text}"
    );

    // A running job has no duration; a finished one has the time it took.
    assert!(lines[1].trim_end().ends_with('-'), "{text}");
    assert!(lines[2].trim_end().ends_with("10s"), "{text}");
    assert!(lines[3].trim_end().ends_with("41s"), "{text}");
    // And the STARTED column is a relative time.
    let ago = regex::Regex::new(r"\d+[smhd] ago").expect("a valid pattern");
    assert!(ago.is_match(lines[1]), "{text}");

    // The line it adds up to, and the one thing to do about it.
    assert!(
        text.contains("3 jobs: 1 running, 1 done, 1 failed"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "run `chap-devops jobs logs {FAILED_ID}` to see why"
        )),
        "{text}"
    );
}

#[test]
fn jobs_filters_limits_and_hands_back_the_raw_list_as_json() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    // `--status` is a query parameter chap-core applies, and the closing line
    // counts what came back.
    let text = chap_in(&sandbox, &dir, &["jobs", "list", "--status", "success"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(text).expect("text");
    assert!(text.contains("eval-ewars"), "{text}");
    assert!(!text.contains("eval-bym"), "{text}");
    assert!(text.contains("1 job: 1 done"), "{text}");

    // `--limit` is applied after the ordering, so it is the newest N.
    let text = chap_in(&sandbox, &dir, &["jobs", "--limit", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(text).expect("text");
    assert!(text.contains("eval-live"), "{text}");
    assert!(!text.contains("eval-ewars"), "{text}");

    // `--json` is chap-core's own documents, in the table's order.
    let list = json_of(&mut chap_in(&sandbox, &dir, &["--json", "jobs"]));
    assert_eq!(list[0]["id"], RUNNING_ID);
    assert_eq!(list[1]["id"], FAILED_ID);
    assert_eq!(list[2]["id"], DONE_ID);
    // Fields this CLI never renders survive the round trip.
    assert_eq!(list[0]["prediction_setup_id"], 1);
    assert_eq!(list[2]["result"], "3");
}

#[test]
fn jobs_says_where_a_job_would_come_from_when_there_are_none() {
    let sandbox = Sandbox::new();
    // A chap-core that has run nothing answers with an empty list, and the
    // deployment is pointed at the port it answers on.
    let empty = server("application/json", "[]");
    let dir = sandbox.home.path().join("chap-devops-empty");
    let mut init = sandbox.chap();
    init.arg("init")
        .arg(&dir)
        .args(["--models", "none", "--api-port", &empty.to_string()]);
    init.assert().success();

    chap_in(&sandbox, &dir, &["jobs"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "no jobs yet; a backtest or prediction started from the Modeling App",
        ));
}

#[test]
fn jobs_logs_prints_the_log_on_stdout_and_the_reason_on_stderr() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    let assert = chap_in(&sandbox, &dir, &["jobs", "logs", FAILED_ID])
        .assert()
        .success();
    let out = assert.get_output();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    // stdout is the log and nothing else, so it can be piped.
    assert!(stdout.contains("Starting backtest for model"), "{stdout}");
    assert!(stdout.contains("--- stderr ---"), "{stdout}");
    assert!(!stdout.contains("job 22222222"), "{stdout}");

    // stderr says which job it is, and what the stderr section blames.
    assert!(
        stderr.contains(&format!(
            "job {FAILED_ID} FAILURE (create_backtest eval-bym)"
        )),
        "{stderr}"
    );
    assert!(
        stderr.contains("stderr: Error in predict_chap() : the inla program crashed"),
        "{stderr}"
    );

    // `--tail` cuts the log from the end, and keeps the hint.
    let assert = chap_in(&sandbox, &dir, &["jobs", "logs", FAILED_ID, "--tail", "2"])
        .assert()
        .success();
    let out = assert.get_output();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.lines().count(), 2, "{stdout}");
    assert!(stdout.contains("Execution halted"), "{stdout}");
    assert!(!stdout.contains("Starting backtest"), "{stdout}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("stderr: Error in predict_chap()"),
        "the hint is read from the whole log, not from the tail"
    );

    // A job that worked gets no hint at all.
    let assert = chap_in(&sandbox, &dir, &["jobs", "logs", DONE_ID])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(stderr.contains("SUCCESS"), "{stderr}");
    assert!(!stderr.contains("stderr:"), "{stderr}");
}

#[test]
fn jobs_takes_an_id_prefix_and_says_when_it_matches_nothing() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    // Eight characters is what the table printed, so eight characters work.
    chap_in(&sandbox, &dir, &["jobs", "show", "11111111"])
        .assert()
        .success()
        .stdout(predicates::str::contains(DONE_ID))
        .stdout(predicates::str::contains("Database result  4"))
        .stdout(predicates::str::contains("hint:").not());
    // The command that reads the result is a hint, shown with `-v`.
    chap_in(&sandbox, &dir, &["-v", "jobs", "show", "11111111"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: the result is row 4 in chap-core's database",
        ));

    // `-v` says which job the prefix landed on.
    chap_in(&sandbox, &dir, &["-vv", "jobs", "show", "11111111"])
        .assert()
        .success()
        .stderr(predicates::str::contains(format!("matched {DONE_ID}")));

    // An id that names nothing is an error with the way back in it.
    chap_in(&sandbox, &dir, &["jobs", "show", "zzzz"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains(
            "job zzzz not found; run `chap-devops jobs` to list them",
        ));

    // And one that names several says so rather than picking one.
    chap_in(&sandbox, &dir, &["jobs", "logs", ""])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("not found"));
}

#[test]
fn jobs_cancel_and_delete_report_what_chap_core_said() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    chap_in(&sandbox, &dir, &["jobs", "cancel", RUNNING_ID])
        .assert()
        .success()
        .stdout("Job cancelled\n");

    chap_in(&sandbox, &dir, &["jobs", "delete", DONE_ID])
        .assert()
        .success()
        .stdout(predicates::str::contains("Job deleted"));

    // chap-core refuses to forget a job it is still working on; the verb that
    // does apply is named.
    chap_in(&sandbox, &dir, &["jobs", "delete", RUNNING_ID])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains(format!(
            "job {RUNNING_ID} is still running; cancel it first"
        )));
}

#[test]
fn api_pretty_prints_json_and_reads_a_json_string_as_text() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);
    let url = read(&dir.join(".env"));
    let port = url
        .lines()
        .find_map(|line| line.strip_prefix("CHAP_API_PORT="))
        .expect("the api port is in .env")
        .trim()
        .to_string();
    let url = format!("http://127.0.0.1:{port}");

    // An object comes back indented with two spaces.
    let text = chap_in(&sandbox, &dir, &["api", "get", "/v1/whoami"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        String::from_utf8(text).expect("text"),
        "{\n  \"auth\": false\n}\n"
    );

    // A JSON string is its text, which is what makes a log readable.
    let text = chap_in(
        &sandbox,
        &dir,
        &["api", "GET", &format!("/v1/jobs/{DONE_ID}")],
    )
    .assert()
    .success()
    .get_output()
    .stdout
    .clone();
    assert_eq!(String::from_utf8(text).expect("text"), "SUCCESS\n");

    // `--raw` is the bytes as they arrived, quotes and all.
    let text = chap_in(
        &sandbox,
        &dir,
        &["api", "GET", &format!("/v1/jobs/{DONE_ID}"), "--raw"],
    )
    .assert()
    .success()
    .get_output()
    .stdout
    .clone();
    assert_eq!(String::from_utf8(text).expect("text"), "\"SUCCESS\"");

    // A body that is not JSON is printed as it came.
    chap_in(&sandbox, &dir, &["api", "GET", "/v1/text"])
        .assert()
        .success()
        .stdout("plain text, not JSON\n");

    // Outside a project `--url` is the address, and it works there.
    chap_in(
        &sandbox,
        sandbox.home.path(),
        &["api", "GET", "/v1/whoami", "--url", &url],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains("\"auth\": false"));

    // Without one there is nothing to aim at, and the error says both ways out.
    chap_in(&sandbox, sandbox.home.path(), &["api", "GET", "/v1/whoami"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--url"));
}

#[test]
fn api_sends_a_body_from_a_file_from_stdin_and_inline() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    let body = sandbox.home.path().join("dataset.json");
    std::fs::write(&body, "{\"name\": \"eval\"}").unwrap();
    chap_in(
        &sandbox,
        &dir,
        &[
            "api",
            "POST",
            "/v1/echo",
            "--data",
            &format!("@{}", body.display()),
        ],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains("\"name\": \"eval\""));

    chap_in(
        &sandbox,
        &dir,
        &["api", "PUT", "/v1/echo", "--data", r#"{"n":1}"#],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains("\"n\": 1"));

    chap_in(&sandbox, &dir, &["api", "POST", "/v1/echo", "--data", "-"])
        .write_stdin("{\"from\":\"stdin\"}")
        .assert()
        .success()
        .stdout(predicates::str::contains("\"from\": \"stdin\""));

    // A body that is not JSON is caught here rather than at the other end.
    chap_in(
        &sandbox,
        &dir,
        &["api", "POST", "/v1/echo", "--data", "{n:1}"],
    )
    .assert()
    .failure()
    .code(2)
    .stderr(predicates::str::contains("not valid JSON"));

    // So is a method chap-devops does not send, and a path with no leading slash.
    chap_in(&sandbox, &dir, &["api", "BREW", "/v1/echo"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("GET, POST, PUT, PATCH, DELETE"));
    chap_in(&sandbox, &dir, &["api", "GET", "v1/echo"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("starts with `/`"));
}

#[test]
fn api_exits_one_on_an_http_error_and_two_when_chap_core_is_not_there() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    // The status line is on stderr and the body is still on stdout: a 404
    // from chap-core says what it did not find.
    let assert = chap_in(&sandbox, &dir, &["api", "GET", "/v1/jobs/nope"])
        .assert()
        .failure()
        .code(1);
    let out = assert.get_output();
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Job 'nope' not found"),
        "the body is the answer"
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stderr).trim(),
        "HTTP 404 Not Found"
    );

    // Under --json stdout is still the body, and stderr has the lines with
    // their levels.
    let assert = chap_in(&sandbox, &dir, &["--json", "api", "GET", "/v1/jobs/nope"])
        .assert()
        .failure()
        .code(1);
    let out = assert.get_output();
    let body: Json = serde_json::from_slice(&out.stdout).expect("the body is JSON");
    assert!(body.to_string().contains("Job 'nope' not found"), "{body}");
    let lines: Json = serde_json::from_slice(&out.stderr).expect("the lines are JSON");
    assert_eq!(
        lines,
        serde_json::json!({"messages": [{"level": "info", "text": "HTTP 404 Not Found"}]})
    );

    // Nothing listening at all is a different exit code, with the sentence
    // `chap-devops status` uses.
    chap_in(
        &sandbox,
        &dir,
        &[
            "api",
            "GET",
            "/v1/jobs",
            "--url",
            "http://127.0.0.1:9",
            "--timeout",
            "2",
        ],
    )
    .assert()
    .failure()
    .code(2)
    .stderr(predicates::str::contains("is not responding"))
    .stderr(predicates::str::contains("run `chap-devops status`"));
}

#[test]
fn api_and_jobs_send_the_token_this_deployment_holds() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);

    // Nothing is protected yet, so nothing is sent.
    chap_in(&sandbox, &dir, &["api", "GET", "/v1/whoami"])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"auth\": false"));

    sandbox.auth(&["enable"]).assert().success();
    chap_in(&sandbox, &dir, &["api", "GET", "/v1/whoami"])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"auth\": true"));

    // And `-v` narrates the header without the value in it.
    let token = env_value(&sandbox.env(), "CHAP_API_TOKEN")
        .expect("a token")
        .to_string();
    let assert = chap_in(&sandbox, &dir, &["-vv", "api", "GET", "/v1/whoami"])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(stderr.contains("Authorization: Bearer <token>"), "{stderr}");
    assert!(!stderr.contains(&token), "the token leaked:\n{stderr}");
}

/// `--url` aimed at another server from inside a protected deployment does not
/// hand it this deployment's token; aimed at this deployment's own API, by
/// `127.0.0.1` rather than `localhost`, it still does.
#[test]
fn api_url_to_another_server_does_not_carry_this_deployments_token() {
    let sandbox = Sandbox::new();
    let dir = served_project(&sandbox);
    sandbox.auth(&["enable"]).assert().success();
    let own = env_value(&sandbox.env(), "CHAP_API_PORT")
        .expect("the api port is in .env")
        .to_string();
    let other = chap_core_server();

    chap_in(
        &sandbox,
        &dir,
        &[
            "api",
            "GET",
            "/v1/whoami",
            "--url",
            &format!("http://127.0.0.1:{other}"),
        ],
    )
    .env_remove("CHAP_API_TOKEN")
    .assert()
    .success()
    .stdout(predicates::str::contains("\"auth\": false"));

    chap_in(
        &sandbox,
        &dir,
        &[
            "api",
            "GET",
            "/v1/whoami",
            "--url",
            &format!("http://127.0.0.1:{own}"),
        ],
    )
    .env_remove("CHAP_API_TOKEN")
    .assert()
    .success()
    .stdout(predicates::str::contains("\"auth\": true"));
}

/// A redirect is followed, and the token goes along only to the same host:
/// an http URL behind a redirect keeps working, and another server never sees
/// the credentials.
#[test]
fn api_follows_a_redirect_and_keeps_the_token_only_for_the_same_host() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_lowercase();
            let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
            let response = match path.as_str() {
                "/same" => format!(
                    "HTTP/1.1 301 Moved Permanently\r\nLocation: http://127.0.0.1:{port}/final\r\n\
                     Content-Length: 0\r\nConnection: close\r\n\r\n"
                ),
                "/other" => format!(
                    "HTTP/1.1 301 Moved Permanently\r\nLocation: http://localhost:{port}/final\r\n\
                     Content-Length: 0\r\nConnection: close\r\n\r\n"
                ),
                _ => {
                    let body = format!(
                        "{{\"auth\":{}}}",
                        request.contains("authorization: bearer ")
                    );
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                }
            };
            let _ = stream.write_all(response.as_bytes());
        }
    });
    let sandbox = Sandbox::new();
    let url = format!("http://127.0.0.1:{port}");
    for (path, kept) in [("/same", true), ("/other", false)] {
        chap_in(
            &sandbox,
            sandbox.home.path(),
            &["api", "GET", path, "--url", &url],
        )
        .env("CHAP_API_TOKEN", "sekret")
        .assert()
        .success()
        .stdout(predicates::str::contains(format!("\"auth\": {kept}")));
    }
}
