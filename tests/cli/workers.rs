//! `chap-devops restart --api-workers`: set the API workers of chap-core,
//! with a fake docker that answers `compose top` and logs every call.

use crate::common::*;
use std::path::{Path, PathBuf};

/// A fake docker whose `compose top` shows gunicorn with `workers` workers.
fn docker_with_workers(workers: usize) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().expect("a directory for the fake docker");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("a bin directory");
    let log = temp.path().join("calls.log");
    let mut top = String::from("SERVICE  #  UID  PID  PPID  C  STIME  TTY  TIME  CMD\n");
    top.push_str("chap  1  1000  10  9  0  11:01  ?  00:00:00  /sbin/docker-init -- gunicorn\n");
    top.push_str("chap  1  1000  11  10  0  11:01  ?  00:00:00  python gunicorn\n");
    for i in 0..workers {
        top.push_str(&format!(
            "chap  1  1000  {}  11  0  11:01  ?  00:00:00  python gunicorn\n",
            20 + i
        ));
    }
    let script = format!(
        "#!/bin/sh\necho \"$*\" >> {log}\ncase \"$*\" in\n  *\" top \"*) cat <<'TOP'\n{top}TOP\n;;\nesac\nexit 0\n",
        log = log.display()
    );
    let docker = bin.join("docker");
    std::fs::write(&docker, script).expect("the fake docker");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755))
            .expect("an executable fake docker");
    }
    (temp, bin, log)
}

fn deployment(sandbox: &Sandbox) -> PathBuf {
    let port = chap_core_server();
    sandbox
        .init(&["--models", "none", "--api-port", &port.to_string()])
        .assert()
        .success();
    sandbox.project()
}

fn env_of(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(".env")).expect("the .env")
}

#[test]
fn workers_sets_the_count_and_recreates_only_chap() {
    let sandbox = Sandbox::new();
    let dir = deployment(&sandbox);
    // A chap-core newer than v2.7.0 maps WEB_CONCURRENCY in its own file.
    let base = dir.join("compose.yml");
    let text = std::fs::read_to_string(&base).expect("compose.yml");
    std::fs::write(&base, format!("{text}\n# WEB_CONCURRENCY\n")).expect("compose.yml");
    let (_temp, bin, log) = docker_with_workers(3);

    chap_with_docker(&sandbox, &dir, &bin, &["restart", "--api-workers", "3"])
        .assert()
        .success()
        .stdout(predicates::str::contains("chap-core runs 3 API workers"))
        .stdout(predicates::str::contains("recreated only `chap`"));
    assert!(
        env_of(&dir).contains("CHAP_API_WORKERS=3"),
        "{}",
        env_of(&dir)
    );
    let calls = std::fs::read_to_string(&log).expect("the docker log");
    assert!(
        calls.contains("up -d --no-deps --force-recreate chap"),
        "{calls}"
    );
}

#[test]
fn workers_refuses_more_than_one_on_an_old_chap_core() {
    let sandbox = Sandbox::new();
    let dir = deployment(&sandbox);
    let (_temp, bin, log) = docker_with_workers(1);

    chap_with_docker(&sandbox, &dir, &bin, &["restart", "--api-workers", "4"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("v2.7.0 or older"));
    assert!(!env_of(&dir).contains("CHAP_API_WORKERS=4"));
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!calls.contains(" up "), "{calls}");
}
