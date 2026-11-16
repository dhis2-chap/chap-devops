//! `chap-devops init DIR --adopt OLD`: a deployment that takes over one that
//! chap-core's compose files and `chap-admin` set up. Docker is a fake on
//! PATH that knows the old project, so no test depends on this machine.

use crate::common::*;
use std::path::{Path, PathBuf};

/// The compose project compose gives the old directory, `chap-admin-site`.
const PROJECT: &str = "chap-admin-site";

const OVERLAY: &str = r#"services:
  marketplace-chapkit-simple-multistep-model:
    image: ghcr.io/chap-models/chapkit_simple_multistep_model:sha-1111111
    x-chap-custom: false
    x-chap-version: 0.1.2
    x-chap-registry: https://raw.githubusercontent.com/dhis2-chap/model-marketplace/main
  marketplace-retired-model:
    image: ghcr.io/chap-models/retired_model:sha-964eea8
    x-chap-custom: false
    x-chap-version: 1.0.4
    x-chap-registry: https://raw.githubusercontent.com/dhis2-chap/model-marketplace/main
volumes:
  marketplace-chapkit-simple-multistep-model-data: {}
"#;

/// The directory `chap-admin` ran in: chap-core's compose file, its `.env`
/// and the overlay it wrote.
fn old_deployment(sandbox: &Sandbox) -> PathBuf {
    let old = sandbox.home.path().join("Chap-Admin-Site");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("compose.yml"), "services: {}\n").unwrap();
    std::fs::write(
        old.join(".env"),
        "POSTGRES_USER=chap\nPOSTGRES_PASSWORD=kept-password\nPOSTGRES_DB=chap_core\n",
    )
    .unwrap();
    std::fs::write(old.join("compose.marketplace.yml"), OVERLAY).unwrap();
    old
}

/// A `docker` that answers for the old project: `containers` for `ps`, the
/// database and the model's data volume for `volume inspect`, and success
/// for the volume create and the copy. Every call is logged.
#[cfg(unix)]
fn fake_docker(containers: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = temp.path().join("calls.log");
    let script = format!(
        r#"#!/bin/sh
echo "$*" >> {log}
case "$1 $2" in
  "ps "*) printf '{containers}' ;;
  "compose ls") echo '[]' ;;
  "volume inspect")
    case "$3" in
      {PROJECT}_chap-db|{PROJECT}_marketplace-chapkit-simple-multistep-model-data) exit 0 ;;
      *) exit 1 ;;
    esac ;;
  "volume create"|"run --rm") exit 0 ;;
  *) exit 1 ;;
esac
"#,
        log = log.display()
    );
    let docker = bin.join("docker");
    std::fs::write(&docker, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();
    (temp, bin, log)
}

#[cfg(unix)]
fn adopt(sandbox: &Sandbox, old: &Path, bin: &Path) -> assert_cmd::Command {
    let mut cmd = sandbox.init(&[
        "--adopt",
        old.to_str().unwrap(),
        "--api-port",
        &free_port().to_string(),
    ]);
    cmd.env(
        "PATH",
        format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    );
    cmd
}

#[cfg(unix)]
#[test]
fn adopt_keeps_the_database_the_secrets_and_the_models() {
    let sandbox = Sandbox::new();
    let old = old_deployment(&sandbox);
    let (_temp, bin, log) = fake_docker("");

    let output = adopt(&sandbox, &old, &bin)
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains(&format!(
            "took over the deployment in {} (compose project {PROJECT})",
            old.display()
        )),
        "{stdout}"
    );
    let new_volume = format!("{PROJECT}_ck_chapkit_simple_multistep_model_data");
    assert!(
        stdout.contains(&format!(
            "copied the data of chapkit_simple_multistep_model from \
             {PROJECT}_marketplace-chapkit-simple-multistep-model-data to {new_volume}"
        )),
        "{stdout}"
    );
    // Offline, the model the marketplace no longer lists cannot be read from
    // its registry, so it is a warning with the command that adds it later.
    assert!(
        stderr.contains("could not add retired_model: ")
            && stderr.contains(
                "run `chap-devops models add ghcr.io/chap-models/retired_model:sha-964eea8 --id \
             retired_model --service-id retired-model` in the new deployment"
            ),
        "{stderr}"
    );

    // The database volume is the old one, because the project name is.
    let dir = sandbox.project();
    assert_eq!(state(&dir)["compose_project"], PROJECT);
    let env = read(&dir.join(".env"));
    assert!(env.contains("POSTGRES_PASSWORD=kept-password"), "{env}");
    let models = read(&dir.join(".chap-devops").join("models.yaml"));
    assert!(
        models.contains("chapkit_simple_multistep_model"),
        "{models}"
    );
    assert!(models.contains("channel: stable"), "{models}");

    // The new volume carries compose's labels, and the old one is only read.
    let calls = read(&log);
    assert!(
        calls.contains(&format!(
            "volume create --label com.docker.compose.project={PROJECT} --label \
             com.docker.compose.volume=ck_chapkit_simple_multistep_model_data {new_volume}"
        )),
        "{calls}"
    );
    assert!(
        calls.contains(&format!(
            "run --rm -v {PROJECT}_marketplace-chapkit-simple-multistep-model-data:/from:ro -v \
             {new_volume}:/to busybox:1.38 cp -a /from/. /to/"
        )),
        "{calls}"
    );
    // The model that did not come over gets no volume.
    assert!(!calls.contains("ck_retired_model_data"), "{calls}");
    // The old directory is not written.
    assert_eq!(read(&old.join("compose.marketplace.yml")), OVERLAY);
}

#[cfg(unix)]
#[test]
fn adopt_refuses_while_the_old_deployment_has_containers() {
    let sandbox = Sandbox::new();
    let old = old_deployment(&sandbox);
    let (_temp, bin, _log) = fake_docker("4f2a9c\\n");

    adopt(&sandbox, &old, &bin)
        .assert()
        .failure()
        .stderr(predicates::str::contains(format!(
            "stop it with `docker compose -p {PROJECT} down`, which keeps its volumes, then \
             run this again"
        )));
    assert!(!sandbox.project().join(".chap-devops").exists());
}

#[test]
fn adopt_refuses_the_flags_it_decides_itself() {
    let sandbox = Sandbox::new();
    let old = old_deployment(&sandbox);
    sandbox
        .init(&[
            "--adopt",
            old.to_str().unwrap(),
            "--models",
            "chapkit_ewars_model",
        ])
        .assert()
        .code(2)
        .stderr(predicates::str::contains(
            "--adopt takes the models and the secrets from the old deployment, so --models has \
             nothing to do",
        ));
}

/// chap-core's compose files publish the API on 8000, and a DHIS2 route or a
/// script calls it there, so the takeover keeps that port unless
/// `--api-port` names another.
#[cfg(unix)]
#[test]
fn adopt_keeps_the_api_on_the_port_chap_core_published_it_on() {
    let sandbox = Sandbox::new();
    let old = old_deployment(&sandbox);
    let (_temp, bin, _log) = fake_docker("");
    let mut cmd = sandbox.init(&["--adopt", old.to_str().unwrap()]);
    cmd.env(
        "PATH",
        format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    );
    cmd.assert().success();
    let dir = sandbox.project();
    assert_eq!(state(&dir)["api_port"], 8000);
    assert!(read(&dir.join(".env")).contains("CHAP_API_PORT=8000"));
}

/// A password that needs quotes keeps them in the new `.env`, so compose
/// reads the same password the database was made with.
#[cfg(unix)]
#[test]
fn adopt_keeps_a_password_that_needs_quotes() {
    let sandbox = Sandbox::new();
    let old = old_deployment(&sandbox);
    std::fs::write(
        old.join(".env"),
        "POSTGRES_USER=chap\nPOSTGRES_PASSWORD='pa$word #1'\nPOSTGRES_DB=chap_core\n",
    )
    .unwrap();
    let (_temp, bin, _log) = fake_docker("");
    adopt(&sandbox, &old, &bin).assert().success();
    let env = std::fs::read_to_string(sandbox.project().join(".env")).unwrap();
    assert!(env.contains("\nPOSTGRES_PASSWORD='pa$word #1'\n"), "{env}");

    // A quote cannot be written into `.env` at all, so it is refused.
    let sandbox = Sandbox::new();
    let old = old_deployment(&sandbox);
    std::fs::write(old.join(".env"), "POSTGRES_PASSWORD=\"it's\"\n").unwrap();
    adopt(&sandbox, &old, &bin)
        .assert()
        .failure()
        .stderr(predicates::str::contains("POSTGRES_PASSWORD in "))
        .stderr(predicates::str::contains("change it in the database"));
}
