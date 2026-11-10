use crate::common::*;
use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use std::path::PathBuf;

/// The named volumes docker holds whose names start with `prefix`.
///
/// `docker volume ls --filter name=` matches substrings, so the prefix is
/// checked again here: one deployment's name must not answer for another's.
fn docker_volumes(prefix: &str) -> Vec<String> {
    let out = std::process::Command::new("docker")
        .args(["volume", "ls", "--filter"])
        .arg(format!("name={prefix}"))
        .args(["--format", "{{.Name}}"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("docker volume ls runs");
    assert!(
        out.status.success(),
        "docker volume ls failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|name| name.starts_with(prefix))
        .map(str::to_string)
        .collect()
}

/// `chap-devops models disable --purge` removes the volume docker really holds.
///
/// Which is the whole reason the flag exists: the overlay that declared the
/// volume is gone by then, so `down --volumes` cannot reach it and only its
/// name can. The volume is created here rather than by `chap-devops up`, which would
/// pull the entire stack for one `docker volume rm`; compose creates it under
/// exactly this name, and `live_up_then_purge_removes_the_model_volume`
/// below is the same thing through a real deployment.
#[test]
fn models_disable_purge_removes_the_volume_docker_holds() {
    if !docker_ready() {
        return;
    }
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    // A directory of its own: this asks docker about the compose project
    // name, which is derived from the directory the deployment lives in.
    let dir = dir.with_file_name("chap-devops-purged");
    let mut init = sandbox.chap();
    init.arg("init")
        .arg(&dir)
        .args(["--models", "chapkit_ewars_model", "--api-port"])
        .arg(free_port().to_string());
    init.assert().success();

    let project = state(&dir)["compose_project"]
        .as_str()
        .expect("init records a compose project name")
        .to_string();
    let volume = format!("{project}_ck_chapkit_ewars_model_data");
    let _created = Volume::create(&volume);
    assert_eq!(
        docker_volumes(&format!("{project}_")),
        std::slice::from_ref(&volume)
    );

    chap_in(
        &sandbox,
        &dir,
        &["models", "disable", "chapkit_ewars_model", "--purge"],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains(format!(
        "removed volume {volume}"
    )))
    .stdout(predicates::str::contains("kept volume").not());
    assert!(
        docker_volumes(&format!("{project}_")).is_empty(),
        "the model's data volume is still there"
    );

    // And `doctor` no longer has a leftover to warn about, because there is
    // nothing under this deployment's name at all.
    chap_in(&sandbox, &dir, &["doctor"])
        .assert()
        .stdout(predicates::str::contains("leftover volumes").not());
}

/// `chap-devops doctor` warns about the volume of a model this deployment no longer
/// enables, and names the command that removes it.
#[test]
fn doctor_warns_about_the_volume_of_a_disabled_model() {
    if !docker_ready() {
        return;
    }
    let sandbox = Sandbox::new();
    let dir = sandbox.project().with_file_name("chap-devops-leftover");
    let mut init = sandbox.chap();
    init.arg("init")
        .arg(&dir)
        .args(["--models", "chapkit_ewars_model", "--api-port"])
        .arg(free_port().to_string());
    init.assert().success();

    let project = state(&dir)["compose_project"]
        .as_str()
        .expect("init records a compose project name")
        .to_string();
    let volume = format!("{project}_ck_chapkit_ewars_model_data");
    let _created = Volume::create(&volume);

    // While the model is enabled the volume is its data, not a leftover.
    chap_in(&sandbox, &dir, &["doctor"])
        .assert()
        .stdout(predicates::str::contains("1 volume named"))
        .stdout(predicates::str::contains("leftover volumes").not());

    chap_in(
        &sandbox,
        &dir,
        &["models", "disable", "chapkit_ewars_model"],
    )
    .assert()
    .success();
    chap_in(&sandbox, &dir, &["doctor"])
        .assert()
        .stdout(predicates::str::contains(format!(
            "leftover volumes from disabled models or components: {volume}"
        )))
        .stdout(predicates::str::contains(
            "chap-devops models disable <id> --purge",
        ));
}

/// The live version: `chap-devops up` creates the model's volume, and
/// `models disable --purge` takes it away again.
///
/// Ignored by default because `up` pulls chap-core, PostgreSQL, Valkey and
/// the model image - gigabytes on a cold cache, and minutes either way, which
/// is not what `cargo test` is for. Run it by hand on a machine with Docker:
///
/// ```sh
/// cargo test --test cli -- --ignored live_up_then_purge
/// ```
#[test]
#[ignore = "pulls the whole stack; run with `cargo test -- --ignored`"]
fn live_up_then_purge_removes_the_model_volume() {
    if !docker_ready() {
        return;
    }
    let sandbox = Sandbox::new();
    let dir = sandbox.project().with_file_name("chap-devops-live-purge");
    let mut init = sandbox.chap();
    init.arg("init")
        .arg(&dir)
        .args(["--models", "chapkit_ewars_model", "--api-port"])
        .arg(free_port().to_string());
    init.assert().success();

    let project = state(&dir)["compose_project"]
        .as_str()
        .expect("init records a compose project name")
        .to_string();
    let volume = format!("{project}_ck_chapkit_ewars_model_data");

    /// Whatever this test does next, the stack it started goes away with it,
    /// volumes and all.
    ///
    /// `--yes` because nothing here is watching a prompt: this is the drop
    /// that has to happen whether or not the assertions above it held.
    struct Started {
        dir: PathBuf,
        cache: PathBuf,
    }
    impl Drop for Started {
        fn drop(&mut self) {
            let mut down =
                Command::cargo_bin("chap-devops").expect("the chap-devops binary is built");
            let _ = down
                .env("CHAP_DEVOPS_CACHE_DIR", &self.cache)
                .env("CHAP_DEVOPS_DATA_DIR", self.cache.join("data"))
                .current_dir(&self.dir)
                .args(["--offline", "down", "--volumes", "--yes"])
                .output();
        }
    }
    let _started = Started {
        dir: dir.clone(),
        cache: sandbox.cache.path().to_path_buf(),
    };

    // Ten minutes: the images are several gigabytes on a cold cache.
    chap_in(&sandbox, &dir, &["up"])
        .timeout(std::time::Duration::from_secs(600))
        .assert()
        .success();
    assert!(
        docker_volumes(&format!("{project}_")).contains(&volume),
        "compose did not create {volume}"
    );

    chap_in(
        &sandbox,
        &dir,
        &["models", "disable", "chapkit_ewars_model", "--purge"],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains(format!(
        "removed volume {volume}"
    )))
    .stdout(predicates::str::contains("stopped and removed"));

    assert!(
        !docker_volumes(&format!("{project}_")).contains(&volume),
        "docker volume ls still lists {volume}"
    );
}

/// `-v` after `down` is refused, in whichever way it was typed.
///
/// It is the global `--verbose` flag, so compose never sees it: passing it
/// through would have been a lie either way round, and a `down` that quietly
/// kept the data of someone who asked for it to go is exactly the failure
/// `--volumes` exists to prevent. Refused before the project is loaded and
/// before docker is asked anything, so the answer is the same on a machine
/// with no daemon.
#[test]
fn a_volume_flag_after_down_says_which_flag_to_use() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();

    for argv in [
        vec!["down", "-v"],
        vec!["down", "--", "-v"],
        vec!["down", "--", "--volumes"],
    ] {
        chap_in(&sandbox, &dir, &argv)
            .assert()
            // Clap's own code for a usage error, because that is what it is.
            .code(2)
            .stderr(predicates::str::contains(
                "chap-devops's own --verbose flag",
            ))
            .stderr(predicates::str::contains("`chap-devops down --volumes`"))
            .stderr(predicates::str::contains("`chap-devops -v down`"));
    }

    // And the verbose stop it could have meant is still a verbose stop: the
    // flag before the subcommand is unambiguous, so it runs (and fails on
    // whatever docker says, if anything).
    chap_in(&sandbox, &dir, &["-v", "down"])
        .assert()
        .stderr(predicates::str::contains("--verbose flag").not());
}

/// `down --volumes` destroys data, so a run nobody can answer refuses.
///
/// No terminal on stdin here, which is every script and every CI job: the
/// prompt would be a hang, and going ahead unasked would take the database of
/// whatever deployment the script was standing in.
#[test]
fn down_volumes_refuses_to_destroy_data_nobody_confirmed() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();

    chap_in(&sandbox, &dir, &["down", "--volumes"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("pass --yes"));

    // `--json` is the same kind of run: nobody is watching it either.
    chap_in(&sandbox, &dir, &["--json", "down", "--volumes"])
        .assert()
        .failure()
        .stdout(predicates::str::contains("pass --yes"));
}

/// `chap-devops down --volumes --yes` removes the volumes docker holds, and names
/// them.
///
/// The volume is created here rather than by `chap-devops up`, which would pull the
/// whole stack for one `docker volume rm`; compose declares the database
/// under exactly this name, and `chap-db` is the one whose loss is the point
/// of the flag.
#[test]
fn down_volumes_removes_the_database_volume_docker_holds() {
    if !docker_ready() {
        return;
    }
    let sandbox = Sandbox::new();
    // A directory of its own: the volumes carry the compose project name,
    // which is derived from the directory the deployment lives in.
    let dir = sandbox.project().with_file_name("chap-devops-down-volumes");
    let mut init = sandbox.chap();
    init.arg("init")
        .arg(&dir)
        .args(["--models", "none", "--api-port"])
        .arg(free_port().to_string());
    init.assert().success();

    let project = state(&dir)["compose_project"]
        .as_str()
        .expect("init records a compose project name")
        .to_string();
    let volume = format!("{project}_chap-db");
    let _created = Volume::create_by_compose(&volume, &project, "chap-db");
    assert_eq!(
        docker_volumes(&format!("{project}_")),
        std::slice::from_ref(&volume)
    );

    chap_in(&sandbox, &dir, &["down", "--volumes", "--yes"])
        .assert()
        .success()
        // With `--yes` there is no question to ask, so it is named once, as
        // it goes.
        .stdout(predicates::str::contains("docker holds").not())
        .stdout(predicates::str::contains(format!(
            "removed 1 volume ({volume})"
        )))
        .stdout(predicates::str::contains("volumes kept").not());

    assert!(
        docker_volumes(&format!("{project}_")).is_empty(),
        "docker volume ls still lists {volume}"
    );
}

/// `down --volumes` also removes the volume of a model that `chap-devops models
/// remove` took out, when docker labels it with this deployment's compose
/// project. A volume under the prefix without that label stays, named in a
/// warning with the command that removes it.
#[test]
fn down_volumes_removes_a_leftover_volume_of_this_compose_project() {
    if !docker_ready() {
        return;
    }
    let sandbox = Sandbox::new();
    let dir = sandbox
        .project()
        .with_file_name("chap-devops-down-leftover");
    let mut init = sandbox.chap();
    init.arg("init")
        .arg(&dir)
        .args(["--models", "none", "--api-port"])
        .arg(free_port().to_string());
    init.assert().success();
    let project = state(&dir)["compose_project"]
        .as_str()
        .expect("init records a compose project name")
        .to_string();
    let leftover = format!("{project}_ck_ewars_data");
    let _leftover = Volume::create_for_project(&leftover, &project);
    let other = format!("{project}_hand_made");
    let _other = Volume::create(&other);

    chap_in(&sandbox, &dir, &["down", "--volumes", "--yes"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "removed 1 volume ({leftover})"
        )))
        .stderr(predicates::str::contains(format!(
            "volume {other} is still there; remove it with `docker volume rm {other}`"
        )));
    assert_eq!(
        docker_volumes(&format!("{project}_")),
        std::slice::from_ref(&other)
    );
}
