//! `chap-devops chap`: the chap CLI in a container, against a stand-in `docker`
//! that logs every call, so Unix only.
#![cfg(unix)]

use crate::common::*;
use predicates::prelude::PredicateBooleanExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A `docker` that lists `v2.3.1` and `master` as the local chap-core tags
/// and `v2.2.0` as the worker's, answers `network inspect` when `NETWORK_UP`
/// is set, and for `run` touches `$WRITE` in the current directory and exits
/// `$CHAP_EXIT`, the way chap does when it writes a file. The chapkit probe
/// exits `$PROBE_EXIT`; compose lists `$RUNNING` as running, and
/// `$SERVICES` as its services. Any other image is local when `$IMAGE_HERE`
/// is set, and `pull` exits `$PULL_EXIT`.
pub(crate) fn fake_docker() -> (TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().expect("a directory for the fake docker");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("a bin directory");
    let log = temp.path().join("calls.log");
    let script = format!(
        "#!/bin/sh\n\
         echo \"$*\" >> '{log}'\n\
         case \"$*\" in\n\
         'image ls ghcr.io/dhis2-chap/chap-core'*) printf 'master\\nv2.3.1\\n<none>\\n'; exit 0;;\n\
         'image ls ghcr.io/dhis2-chap/chap-worker'*) printf 'v2.2.0\\n'; exit 0;;\n\
         'network inspect'*) [ -n \"$NETWORK_UP\" ] && exit 0; exit 1;;\n\
         'image inspect --format {{{{.Id}}}} ghcr.io/dhis2-chap/chap-worker'*) \
         [ -n \"$WORKER_HERE\" ] && echo sha256:worker && exit 0; exit 1;;\n\
         'image inspect --format {{{{.Id}}}} ghcr.io/dhis2-chap/chap-core'*) \
         [ -n \"$CORE_HERE\" ] && echo sha256:core && exit 0; exit 1;;\n\
         'image inspect --format {{{{.Id}}}} '*) \
         [ -n \"$IMAGE_HERE\" ] && echo sha256:image && exit 0; exit 1;;\n\
         pull*) exit \"${{PULL_EXIT:-0}}\";;\n\
         'run --rm -v /var/run/docker.sock'*) echo 0; exit 0;;\n\
         'run --rm --platform'*) exit \"${{PROBE_EXIT:-0}}\";;\n\
         run*) echo \"hints=$DOCKER_CLI_HINTS\" >> '{log}'; \
         [ -n \"$WRITE\" ] && touch \"$WRITE\"; exit \"${{CHAP_EXIT:-0}}\";;\n\
         compose*' ps --format json'*) [ -n \"$RUNNING\" ] && \
         printf '{{\"Service\":\"%s\",\"State\":\"running\"}}\\n' \"$RUNNING\"; exit 0;;\n\
         compose*' config --services'*) printf '%s\\n' $SERVICES; exit 0;;\n\
         compose*) exit 0;;\n\
         esac\n\
         exit 1\n",
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

/// The arguments of the one `docker run` that runs chap.
pub(crate) fn chap_run(log: &Path) -> Vec<String> {
    let calls = std::fs::read_to_string(log).unwrap_or_default();
    let line = calls
        .lines()
        .find(|line| line.starts_with("run --rm -i"))
        .unwrap_or_else(|| panic!("no chap run in:\n{calls}"));
    line.split(' ').map(str::to_string).collect()
}

/// The value after `flag` in a run's arguments.
pub(crate) fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

/// A working directory of its own, outside any deployment.
pub(crate) fn workdir(sandbox: &Sandbox) -> PathBuf {
    let dir = sandbox.home.path().join("work");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

#[test]
fn a_command_without_a_model_runs_in_the_core_image_and_names_what_it_wrote() {
    let sandbox = Sandbox::new();
    let work = workdir(&sandbox);
    let (_temp, bin, log) = fake_docker();

    let out = chap_with_docker(
        &sandbox,
        &work,
        &bin,
        &["chap", "plot-backtest", "x.nc", "--output-file", "x.html"],
    )
    .env("WRITE", "x.html")
    .assert()
    .success()
    .get_output()
    .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);

    let args = chap_run(&log);
    assert!(args.contains(&"ghcr.io/dhis2-chap/chap-core:v2.3.1".to_string()));
    assert_eq!(
        args[args.len() - 5..],
        ["chap", "plot-backtest", "x.nc", "--output-file", "x.html"]
    );
    let workdir = work.display().to_string();
    assert_eq!(value_of(&args, "-w"), Some(workdir.as_str()));
    assert!(args.contains(&format!("{workdir}:{workdir}")));
    assert!(value_of(&args, "--network").is_none());
    assert!(
        stderr.contains("running `chap plot-backtest` in ghcr.io/dhis2-chap/chap-core:v2.3.1"),
        "{stderr}"
    );
    assert!(
        stderr.contains("chap finished; it wrote x.html"),
        "{stderr}"
    );
}

#[test]
fn help_after_the_chap_command_is_chap_help() {
    let sandbox = Sandbox::new();
    let work = workdir(&sandbox);
    let (_temp, bin, log) = fake_docker();

    chap_with_docker(&sandbox, &work, &bin, &["chap", "eval", "--help"])
        .assert()
        .success();
    let args = chap_run(&log);
    assert_eq!(args[args.len() - 3..], ["chap", "eval", "--help"]);
}

#[test]
fn a_model_from_a_repository_runs_in_the_worker_image() {
    let sandbox = Sandbox::new();
    let work = workdir(&sandbox);
    let (_temp, bin, log) = fake_docker();

    chap_with_docker(
        &sandbox,
        &work,
        &bin,
        &[
            "-v",
            "chap",
            "eval",
            "--model-name",
            "https://github.com/dhis2-chap/minimalist_example_r",
            "--output-file",
            "r.nc",
        ],
    )
    .env("WRITE", "r.nc")
    .assert()
    .success()
    .stderr(predicates::str::contains("chap finished; it wrote r.nc"))
    .stderr(predicates::str::contains(
        "hint: `chap-devops chap plot-backtest r.nc --output-file r.html` plots it",
    ));
    let args = chap_run(&log);
    assert!(args.contains(&"ghcr.io/dhis2-chap/chap-worker:v2.2.0".to_string()));
}

#[test]
fn a_docker_env_model_is_refused_without_the_socket_and_given_it_with_docker() {
    let sandbox = Sandbox::new();
    let work = workdir(&sandbox);
    let model = work.join("ewars");
    std::fs::create_dir_all(&model).unwrap();
    std::fs::write(
        model.join("MLproject"),
        "name: ewars\ndocker_env:\n  image: ivargr/r_inla:latest\n",
    )
    .unwrap();
    let (_temp, bin, log) = fake_docker();
    let eval = [
        "chap",
        "eval",
        "--model-name",
        "ewars",
        "--output-file",
        "e.nc",
    ];

    chap_with_docker(&sandbox, &work, &bin, &eval)
        .assert()
        .code(2)
        .stderr(predicates::str::contains(
            "runs in docker (`docker_env` in its MLproject)",
        ))
        .stderr(predicates::str::contains("chap-devops chap --docker"));
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!calls.contains("run --rm -i"), "nothing ran: {calls}");

    let mut with_socket = vec!["chap", "--docker"];
    with_socket.extend_from_slice(&eval[1..]);
    chap_with_docker(&sandbox, &work, &bin, &with_socket)
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "--docker gives the container control of docker",
        ));
    let args = chap_run(&log);
    assert!(args.contains(&"/var/run/docker.sock:/var/run/docker.sock".to_string()));
    assert_eq!(value_of(&args, "--group-add"), Some("0"));
}

#[test]
fn a_run_in_a_deployment_uses_its_tag_and_reaches_its_models() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();
    let project = state(&dir)["compose_project"].as_str().unwrap().to_string();
    let tag = state(&dir)["chap_image_tag"].as_str().unwrap().to_string();
    let service = state(&dir)["models"]["chapkit_ewars_model"]["service_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_temp, bin, log) = fake_docker();

    chap_with_docker(
        &sandbox,
        &dir,
        &bin,
        &["chap", "eval", "--model-name", "http://x:8000"],
    )
    .env("NETWORK_UP", "1")
    .env("WORKER_HERE", "1")
    .assert()
    .success()
    .stderr(predicates::str::contains(format!(
        "models: chapkit_ewars_model at http://{service}:8000"
    )));
    let args = chap_run(&log);
    assert!(args.contains(&format!("ghcr.io/dhis2-chap/chap-worker:{tag}")));
    assert_eq!(
        value_of(&args, "--network"),
        Some(format!("{project}_default").as_str())
    );

    // Help touches no model, so it says nothing about the models or files.
    chap_with_docker(&sandbox, &dir, &bin, &["chap", "--version"])
        .env("NETWORK_UP", "1")
        .assert()
        .success()
        .stderr(predicates::str::contains("models:").not())
        .stderr(predicates::str::contains("files:").not())
        .stderr(predicates::str::contains("chap finished").not());
}

/// A deployment with one model, its compose project and the model's service.
fn deployment(sandbox: &Sandbox) -> (PathBuf, String, String) {
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();
    let project = state(&dir)["compose_project"].as_str().unwrap().to_string();
    let service = state(&dir)["models"]["chapkit_ewars_model"]["service_id"]
        .as_str()
        .unwrap()
        .to_string();
    (dir, project, service)
}

#[test]
fn a_model_id_in_a_deployment_that_is_down_starts_only_that_model() {
    let sandbox = Sandbox::new();
    let (dir, project, service) = deployment(&sandbox);
    let eval = dir.join("eval");
    std::fs::create_dir_all(&eval).unwrap();
    let (_temp, bin, log) = fake_docker();
    let args = [
        "chap",
        "eval",
        "--model-name",
        "chapkit_ewars_model",
        "--output-file",
        "e.nc",
    ];

    chap_with_docker(&sandbox, &eval, &bin, &args)
        .env("NETWORK_UP", "1")
        .env("SERVICES", format!("chap {service} {service}-init"))
        .env("WRITE", "e.nc")
        .assert()
        .success()
        .stderr(predicates::str::contains(format!(
            "starting {service} for this run, without chap-core"
        )))
        .stderr(predicates::str::contains(format!(
            "model: chapkit_ewars_model at http://{service}:8000 answers"
        )))
        .stderr(predicates::str::contains(format!(
            "{service} keeps running"
        )))
        .stderr(predicates::str::contains("chap finished; it wrote e.nc"));

    let calls = std::fs::read_to_string(&log).unwrap();
    let up: Vec<&str> = calls.lines().filter(|line| line.contains(" up ")).collect();
    assert_eq!(up.len(), 2, "{calls}");
    assert!(
        up[0].ends_with(&format!("up --no-deps {service}-init")),
        "{calls}"
    );
    assert!(
        up[1].ends_with(&format!("up -d --no-deps {service}")),
        "{calls}"
    );
    // The start carries the file that takes the registration away, so the
    // model does not wait on a chap-core that does not run.
    assert!(
        up[1].contains(&format!("compose.{project}.{service}.yml")),
        "{calls}"
    );
    assert!(!calls.contains(" stop "), "nothing stopped: {calls}");
    let run = chap_run(&log);
    assert_eq!(
        value_of(&run, "--model-name"),
        Some(format!("http://{service}:8000").as_str())
    );
    assert!(calls.contains("hints=false"), "{calls}");
}

#[test]
fn stop_stops_what_the_run_started_and_nothing_that_ran_before() {
    let sandbox = Sandbox::new();
    let (dir, _project, service) = deployment(&sandbox);
    let (_temp, bin, log) = fake_docker();
    let args = [
        "chap",
        "--stop",
        "eval",
        "--model-name",
        "chapkit_ewars_model",
    ];

    chap_with_docker(&sandbox, &dir, &bin, &args)
        .env("NETWORK_UP", "1")
        .assert()
        .success()
        .stderr(predicates::str::contains(format!(
            "stopped {service}, as --stop asked"
        )));
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.contains(&format!(" stop {service}")), "{calls}");

    // Running already: nothing is started, and --stop leaves it alone.
    std::fs::write(&log, "").unwrap();
    chap_with_docker(&sandbox, &dir, &bin, &args)
        .env("NETWORK_UP", "1")
        .env("RUNNING", &service)
        .assert()
        .success()
        .stderr(predicates::str::contains("starting").not());
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(!calls.contains(" up "), "{calls}");
    assert!(!calls.contains(" stop "), "{calls}");
}

#[test]
fn a_marketplace_id_the_deployment_does_not_have_is_refused() {
    let sandbox = Sandbox::new();
    let (dir, _project, _service) = deployment(&sandbox);
    let (_temp, bin, log) = fake_docker();

    chap_with_docker(
        &sandbox,
        &dir,
        &bin,
        &["chap", "eval", "--model-name", "auto_arima_chapkit"],
    )
    .assert()
    .code(2)
    .stderr(predicates::str::contains(
        "`chap-devops models enable auto_arima_chapkit` adds it",
    ));
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!calls.contains("run --rm -i"), "nothing ran: {calls}");
}

#[test]
fn a_chapkit_url_that_does_not_answer_is_a_note_and_not_a_traceback() {
    let sandbox = Sandbox::new();
    let work = workdir(&sandbox);
    let (_temp, bin, log) = fake_docker();

    chap_with_docker(
        &sandbox,
        &work,
        &bin,
        &["chap", "eval", "--model-name", "http://nothing:8000"],
    )
    .env("PROBE_EXIT", "7")
    .assert()
    .code(2)
    .stderr(predicates::str::contains(
        "the model server at http://nothing:8000 is not running",
    ))
    .stderr(predicates::str::contains("--model-name auto_arima_chapkit"));
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!calls.contains("run --rm -i"), "chap never ran: {calls}");

    chap_with_docker(
        &sandbox,
        &work,
        &bin,
        &["chap", "eval", "--model-name", "https://models.example.org"],
    )
    .env("PROBE_EXIT", "6")
    .assert()
    .code(2)
    .stderr(predicates::str::contains(
        "check that the server runs and that this machine can reach it",
    ));
}

#[test]
fn a_run_in_the_deployment_directory_suggests_a_subdirectory() {
    let sandbox = Sandbox::new();
    let (dir, _project, _service) = deployment(&sandbox);
    let (_temp, bin, _log) = fake_docker();

    chap_with_docker(&sandbox, &dir, &bin, &["chap", "validate", "data.csv"])
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "this is the deployment directory",
        ));
    let eval = dir.join("eval");
    std::fs::create_dir_all(&eval).unwrap();
    chap_with_docker(&sandbox, &eval, &bin, &["chap", "validate", "data.csv"])
        .assert()
        .success()
        .stderr(predicates::str::contains("this is the deployment directory").not());
}

#[test]
fn chap_exit_status_is_the_exit_status_of_chap_devops() {
    let sandbox = Sandbox::new();
    let work = workdir(&sandbox);
    let (_temp, bin, _log) = fake_docker();

    chap_with_docker(&sandbox, &work, &bin, &["chap", "validate", "data.csv"])
        .env("CHAP_EXIT", "3")
        .assert()
        .code(3)
        .stderr(predicates::str::contains(
            "chap exited with status 3; its own message is above",
        ));
    chap_with_docker(&sandbox, &work, &bin, &["chap", "validate", "data.csv"])
        .env("CHAP_EXIT", "125")
        .assert()
        .code(125)
        .stderr(predicates::str::contains("docker could not start"))
        .stderr(predicates::str::contains("--tag TAG"));
}

#[test]
fn a_path_outside_the_current_directory_is_mounted_where_it_is() {
    let sandbox = Sandbox::new();
    let work = workdir(&sandbox);
    let out = std::fs::canonicalize(sandbox.home.path())
        .unwrap()
        .join("results");
    std::fs::create_dir_all(&out).unwrap();
    let (_temp, bin, log) = fake_docker();

    chap_with_docker(
        &sandbox,
        &work,
        &bin,
        &[
            "chap",
            "plot-backtest",
            "x.nc",
            "--output-file",
            "../results/x.html",
        ],
    )
    .assert()
    .success();
    let args = chap_run(&log);
    let mounted = out.display().to_string();
    assert!(args.contains(&format!("{mounted}:{mounted}")), "{args:?}");
}

#[test]
fn json_and_a_missing_group_and_a_missing_image_are_refused_with_the_way_out() {
    let sandbox = Sandbox::new();
    let work = workdir(&sandbox);
    let (_temp, bin, _log) = fake_docker();

    chap_with_docker(&sandbox, &work, &bin, &["--json", "chap", "--version"])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("--json does not apply"));
    chap_with_docker(
        &sandbox,
        &work,
        &bin,
        &["chap", "--group", "nope", "--version"],
    )
    .assert()
    .code(2)
    .stderr(predicates::str::contains(
        "no `chap-devops run` group called `nope`",
    ));

    // A docker with no chap-core image at all, offline: nothing to run.
    let empty = tempfile::tempdir().unwrap();
    let docker = empty.path().join("docker");
    std::fs::write(&docker, "#!/bin/sh\nexit 0\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    chap_with_docker(&sandbox, &work, empty.path(), &["chap", "--version"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "--offline needs a chap-core or chap-worker image on this machine",
        ));
}

/// The real image answers: chap runs as this user, in this directory.
#[test]
#[ignore = "pulls ghcr.io/dhis2-chap/chap-core (2.4 GB); run with `cargo test -- --ignored`"]
fn the_chap_cli_answers_from_the_real_image() {
    if !docker_ready() {
        return;
    }
    let sandbox = Sandbox::new();
    let work = workdir(&sandbox);
    // No arguments: chap's own help, not the help of chap-devops's options.
    chap_in(&sandbox, &work, &["chap", "--tag", "v2.3.1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("plot-backtest"));
}

/// `localhost` in the container is the container, so the URL `chap-devops ps`
/// prints for a model reaches nothing there; the refusal names the URL that
/// does.
#[test]
fn a_model_url_on_localhost_is_refused_before_the_run() {
    let sandbox = Sandbox::new();
    let work = workdir(&sandbox);
    let (_temp, bin, log) = fake_docker();

    chap_with_docker(
        &sandbox,
        &work,
        &bin,
        &["chap", "eval", "--model-name", "http://localhost:5001"],
    )
    .assert()
    .code(2)
    .stderr(predicates::str::contains(
        "in the container `localhost` is the container itself",
    ))
    .stderr(predicates::str::contains(
        "use `--model-name http://host.docker.internal:5001`",
    ))
    .stderr(predicates::str::contains("`auto_arima_chapkit`"));
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!calls.contains("run --rm -i"), "nothing ran: {calls}");
}

#[test]
fn chap_devops_says_first_what_chap_would_end_in_a_traceback_for() {
    let sandbox = Sandbox::new();
    let work = workdir(&sandbox);
    let (_temp, bin, log) = fake_docker();

    chap_with_docker(
        &sandbox,
        &work,
        &bin,
        &["chap", "eval", "--model-name", "./no_such_model"],
    )
    .assert()
    .code(2)
    .stderr(predicates::str::contains(
        "`./no_such_model` is not a directory here",
    ));
    chap_with_docker(&sandbox, &work, &bin, &["chap", "plot-dataset", "d.csv"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("the container has no browser"));
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!calls.contains("run --rm -i"), "chap never ran: {calls}");
}

#[test]
fn the_directory_of_an_output_file_is_made_before_the_run() {
    let sandbox = Sandbox::new();
    let work = workdir(&sandbox);
    let (_temp, bin, _log) = fake_docker();

    chap_with_docker(
        &sandbox,
        &work,
        &bin,
        &[
            "chap",
            "plot-backtest",
            "a.nc",
            "--output-file",
            "out/new/a.html",
        ],
    )
    .assert()
    .success();
    assert!(work.join("out/new").is_dir());
}

/// A deployment without chap-core has no worker image, so a run that needs
/// no R takes the core image there too, and pulls nothing large.
#[test]
fn a_run_takes_the_image_that_is_already_here() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--only", "none", "--models", "none"])
        .assert()
        .success();
    let tag = state(&dir)["chap_image_tag"].as_str().unwrap().to_string();
    let (_temp, bin, log) = fake_docker();
    chap_with_docker(&sandbox, &dir, &bin, &["chap", "eval", "--help"])
        .env("CORE_HERE", "1")
        .assert()
        .success();
    let args = chap_run(&log);
    assert!(
        args.contains(&format!("ghcr.io/dhis2-chap/chap-core:{tag}")),
        "{args:?}"
    );
}
