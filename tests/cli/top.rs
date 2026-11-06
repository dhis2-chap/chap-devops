//! `chap-devops top` off a terminal: one snapshot of every chap-devops deployment, found
//! by the labels on its containers. Unix only: the stand-in docker is a shell
//! script.

#![cfg(unix)]

use crate::common::*;
use std::path::PathBuf;
use tempfile::TempDir;

/// A `docker` that lists two labelled containers of one deployment, and
/// what they use.
fn labelled_docker(dir: &std::path::Path) -> (TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("a directory for the fake docker");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("a bin directory");
    let labels = |service: &str, role: &str| {
        format!(
            "com.docker.compose.project=demo-ab12cd,com.docker.compose.service={service},\
             com.docker.compose.project.working_dir={},io.github.dhis2-chap.chap-devops.role={role}",
            dir.display()
        )
    };
    let ps = format!(
        "{{\"ID\":\"a\",\"Names\":\"demo-ab12cd-chap-1\",\"State\":\"running\",\
         \"Status\":\"Up 3 minutes (healthy)\",\"Labels\":\"{}\"}}\\n\
         {{\"ID\":\"b\",\"Names\":\"demo-ab12cd-dhis2-1\",\"State\":\"exited\",\
         \"Status\":\"Exited (1) 1 minute ago\",\"Labels\":\"{}\"}}\\n",
        labels("chap", "chap-core"),
        labels("dhis2", "dhis2")
    );
    let stats = "{\"Name\":\"demo-ab12cd-chap-1\",\"CPUPerc\":\"2.00%%\",\
                 \"MemUsage\":\"300MiB / 8GiB\"}\\n";
    let script = format!(
        "#!/bin/sh\n\
         case \"$*\" in\n\
         *'ps -a --filter label=io.github.dhis2-chap.chap-devops.role'*) printf '{ps}'; exit 0;;\n\
         *'stats --no-stream'*) printf '{stats}'; exit 0;;\n\
         esac\n\
         exit 0\n"
    );
    let docker = bin.join("docker");
    std::fs::write(&docker, script).expect("the fake docker");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755))
            .expect("an executable fake docker");
    }
    (temp, bin)
}

#[test]
fn top_off_a_terminal_prints_one_snapshot_of_the_tree() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "none", "--with", "dhis2"])
        .assert()
        .success();
    let dir = sandbox.project();
    let (_fake, bin) = labelled_docker(&dir);
    let cwd = sandbox.home.path();

    let out = chap_with_docker(&sandbox, cwd, &bin, &["top"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).expect("text");
    assert!(text.contains("chapx  init  1/2 running"), "{text}");
    assert!(text.contains("running (healthy)"), "{text}");
    assert!(text.contains("2.00%"), "{text}");
    assert!(text.contains("exited"), "{text}");
    // That it is a snapshot is background, shown with `-v`.
    assert!(!text.contains("a snapshot"), "{text}");
    let out = chap_with_docker(&sandbox, cwd, &bin, &["-v", "top"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).expect("text");
    assert!(text.contains("hint: a snapshot"), "{text}");

    let out = chap_with_docker(&sandbox, cwd, &bin, &["--json", "top"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc: serde_json::Value = serde_json::from_slice(&out).expect("one JSON document");
    let node = &doc["deployments"][0];
    assert_eq!(node["kind"], "init");
    assert_eq!(node["services"][0]["service"], "chap");
    assert_eq!(node["services"][0]["role"], "chap-core");
    assert_eq!(node["services"][1]["role"], "dhis2");
}

/// `--interval` is bounded, so the collector's wait cannot overflow.
#[test]
fn top_refuses_an_interval_over_an_hour() {
    let sandbox = Sandbox::new();
    chap_in(
        &sandbox,
        sandbox.home.path(),
        &["top", "--interval", "3601"],
    )
    .assert()
    .code(2)
    .stderr(predicates::str::contains("3601"));
}

#[test]
fn top_record_writes_one_json_line_per_service_and_says_how_many() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "none", "--with", "dhis2"])
        .assert()
        .success();
    let dir = sandbox.project();
    let (_fake, bin) = labelled_docker(&dir);
    let file = sandbox.home.path().join("usage.jsonl");
    let file_arg = file.to_string_lossy().to_string();

    let out = chap_with_docker(
        &sandbox,
        sandbox.home.path(),
        &bin,
        &[
            "top",
            "--record",
            &file_arg,
            "--interval",
            "1",
            "--duration",
            "1",
        ],
    )
    .assert()
    .success()
    .get_output()
    .stdout
    .clone();
    let text = String::from_utf8(out).expect("text");
    assert!(text.contains("recorded "), "{text}");
    assert!(text.contains("usage.jsonl"), "{text}");

    let written = std::fs::read_to_string(&file).expect("the recording");
    let rows: Vec<serde_json::Value> = written
        .lines()
        .map(|l| serde_json::from_str(l).expect("one JSON object per line"))
        .collect();
    assert!(rows.len() >= 2, "{written}");
    let chap = rows
        .iter()
        .find(|r| r["service"] == "chap")
        .expect("a chap sample");
    assert_eq!(chap["deployment"], "chapx");
    assert_eq!(chap["cpu_percent"], 2.0);
    assert_eq!(chap["memory_bytes"], 300u64 * 1024 * 1024);
    assert_eq!(chap["memory_limit_bytes"], 8u64 * 1024 * 1024 * 1024);
}

#[test]
fn top_duration_needs_record() {
    let sandbox = Sandbox::new();
    sandbox
        .chap()
        .args(["top", "--duration", "5"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--record"));
}
