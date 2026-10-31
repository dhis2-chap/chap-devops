use crate::common::*;
use predicates::prelude::PredicateBooleanExt;
use serde_json::Value as Json;
use serde_yaml_ng::Value as Yaml;
use std::path::Path;

/// There is no default model: a plain `init` enables none and says how to add
/// one, so a model the marketplace drops cannot stop it.
#[test]
fn init_with_no_models_option_enables_no_model() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&[])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "no models are enabled; run `chap-devops models enable ID` to add one",
        ));
    let models = read(&sandbox.project().join(".chap-devops/models.yaml"));
    assert!(!models.contains("chapkit_"), "{models}");
}

#[test]
fn init_writes_every_file_of_a_deployment() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();

    for name in [
        "compose.yml",
        "compose.chap-devops.yml",
        ".env",
        "compose.marketplace.yml",
        "compose.chapkit-ewars-model.yml",
        ".chap-devops/project.yaml",
        ".chap-devops/models.yaml",
    ] {
        assert!(dir.join(name).is_file(), "{name} was not written");
    }
    assert!(
        !dir.join("chap-devops.json").exists(),
        "the JSON state file is gone"
    );
    // Both state files open with the one line that says who manages them.
    const MANAGED: &str =
        "# Managed by chap-devops; change it with the chap-devops commands, not by hand.\n";
    assert!(read(&dir.join(".chap-devops/project.yaml")).starts_with(MANAGED));
    assert!(read(&dir.join(".chap-devops/models.yaml")).starts_with(MANAGED));

    let state = state(&dir);
    assert_eq!(state["schema_version"], 1);
    assert_eq!(state["chap_image_tag"], "latest");
    assert_eq!(
        state["compose_files"],
        serde_json::json!([
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.marketplace.yml"
        ]),
        "the chap-devops overrides need their own -f entry, after the base file"
    );
    assert_eq!(
        state["rendered_files"],
        serde_json::json!([
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.chapkit-ewars-model.yml",
            "compose.marketplace.yml"
        ]),
        "compose.yml is rendered by sync like the overlays"
    );
    assert_eq!(state["api_port"], 8700);
    assert_eq!(
        state["chap_compose_source"],
        serde_json::json!({"kind": "embedded"}),
        "--offline cannot fetch chap-core's own compose.ghcr.yml"
    );
    assert_eq!(state["port_range"], serde_json::json!([5001, 5999]));
    let model = &state["models"]["chapkit_ewars_model"];
    // A model publishes no host port until someone asks for one.
    assert_eq!(model["host_port"], Json::Null);
    assert_eq!(model["service_id"], "chapkit-ewars-model");
    assert_eq!(model["image_tag"], stable_pin("chapkit_ewars_model").1);
    assert_eq!(model["data_dir"], "/app/data");
    // `--offline` with no local image: the built-in table, whose `chapkit` is
    // recorded as the numbers the overlay renders.
    assert_eq!(model["user"], "1000:1000");
    assert_eq!(model["user_from"], "table");
    assert_eq!(model["platform"], "linux/amd64");

    assert_eq!(includes(&dir), vec!["compose.chapkit-ewars-model.yml"]);
    let overlay = yaml(&dir.join("compose.chapkit-ewars-model.yml"));
    let svc = &overlay["services"]["chapkit-ewars-model"];
    assert_eq!(svc["expose"][0].as_str().unwrap(), "8000");
    assert!(
        svc.get("ports").is_none(),
        "a model service publishes nothing by default"
    );

    // chap's port is the only published one, and it comes from a chap-devops-owned
    // override rather than an edit of the upstream base file.
    let chap_devops_overlay = read(&dir.join("compose.chap-devops.yml"));
    assert!(chap_devops_overlay.contains("ports: !override"));
    assert!(chap_devops_overlay.contains("\"${CHAP_API_PORT:-8700}:8000\""));

    // The generated .env carries a fresh password and the model's pin.
    let env = read(&dir.join(".env"));
    let password = env
        .lines()
        .find_map(|l| l.strip_prefix("POSTGRES_PASSWORD="))
        .expect("a password line");
    assert_eq!(password.len(), 32);
    assert!(env.contains(&format!(
        "# CHAPKIT_EWARS_MODEL_IMAGE_TAG={}",
        stable_pin("chapkit_ewars_model").1
    )));
    assert!(env.contains("# CHAP_IMAGE_TAG=latest"));
    // Active even at the default, so the port is where you would look for it.
    assert!(env.contains("\nCHAP_API_PORT=8700\n"), "{env}");
}

/// The shape of a generated compose project name: a slug of the directory
/// name, then six hex characters of its own.
///
/// Compose's own rule as well: lowercase, `[a-z0-9-]`, and it starts with a
/// letter or a digit.
const PROJECT_NAME: &str = "^[a-z0-9][a-z0-9-]*-[0-9a-f]{6}$";

/// The `name:` of a rendered compose file, which is the compose project name.
fn compose_name(path: &Path) -> String {
    yaml(path)["name"]
        .as_str()
        .unwrap_or_else(|| panic!("{} has no name: key", path.display()))
        .to_string()
}

#[test]
fn init_gives_the_deployment_a_compose_project_name_of_its_own() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    // `status` below really calls the API port, so this deployment gets one
    // nothing on this machine answers on rather than the default 8000.
    let api_port = free_port();
    sandbox
        .init(&["--models", "none", "--api-port", &api_port.to_string()])
        .assert()
        .success();

    // Without it compose would name the project after the directory, and two
    // deployments in directories both called `chapx` would share every named
    // volume - including the database.
    let name = state(&dir)["compose_project"]
        .as_str()
        .expect("init records a compose project name")
        .to_string();
    let shape = regex::Regex::new(PROJECT_NAME).unwrap();
    assert!(shape.is_match(&name), "unexpected project name {name:?}");
    assert!(name.starts_with("chapx-"), "{name}");

    // Both chap-devops-owned files carry it: the umbrella is the one always in the
    // `-f` list, and compose takes the `name:` of the last file that sets one.
    assert_eq!(compose_name(&dir.join("compose.chap-devops.yml")), name);
    assert_eq!(compose_name(&dir.join("compose.marketplace.yml")), name);

    // A second deployment of the same directory name gets a different one.
    let other = Sandbox::new();
    other.init(&["--models", "none"]).assert().success();
    let second = state(&other.project())["compose_project"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(shape.is_match(&second), "{second}");
    assert_ne!(name, second, "two `chapx` directories, two project names");

    // `chap-devops status --json` and `chap-devops doctor` both report it.
    let out = sandbox
        .chap()
        .arg("-C")
        .arg(&dir)
        .args(["status", "--json", "--timeout", "1"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let report: Json = serde_json::from_slice(&out).expect("status --json is one document");
    assert_eq!(report["project"], Json::String(name.clone()));

    let out = sandbox
        .chap()
        .arg("-C")
        .arg(&dir)
        .args(["--json", "doctor"])
        .output()
        .expect("doctor runs");
    let report: Json = serde_json::from_slice(&out.stdout).expect("doctor --json parses");
    assert_eq!(doctor_status(&report, "compose-project"), "ok", "{report}");
    assert!(
        doctor_check(&report, "compose-project")["detail"]
            .as_str()
            .unwrap()
            .starts_with(&name),
        "{report}"
    );
}

#[test]
fn force_keeps_the_compose_project_name_it_found() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();
    let name = state(&dir)["compose_project"].as_str().unwrap().to_string();

    // A new name would leave the running deployment's containers and volumes
    // behind under the old one, which is the trap this whole thing avoids.
    sandbox
        .init(&["--models", "none", "--force"])
        .assert()
        .success();
    assert_eq!(state(&dir)["compose_project"].as_str(), Some(name.as_str()));
    assert_eq!(compose_name(&dir.join("compose.chap-devops.yml")), name);
}

#[test]
fn docker_accepts_the_stack_with_the_chap_devops_override() {
    if !docker_ready() {
        return;
    }
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();

    // `!override` needs Compose 2.24+; this is what says the rendered stack is
    // a document the installed Docker actually accepts.
    let out = std::process::Command::new("docker")
        .args([
            "compose",
            "-f",
            "compose.yml",
            "-f",
            "compose.chap-devops.yml",
        ])
        .args(["-f", "compose.marketplace.yml", "config"])
        .current_dir(&dir)
        .output()
        .expect("docker compose config runs");
    assert!(
        out.status.success(),
        "docker compose config failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // chap is published exactly once, on the API port; the model not at all.
    let merged: Yaml = serde_yaml_ng::from_slice(&out.stdout).expect("config is YAML");
    // The project name compose resolves to is this deployment's own, from the
    // `name:` in the second `-f` file rather than from the directory.
    let name = state(&dir)["compose_project"].as_str().unwrap().to_string();
    assert_eq!(
        merged["name"].as_str(),
        Some(name.as_str()),
        "compose takes the name: of a file that is not the first -f"
    );
    // And chap-core is handed the registration key: upstream's compose.ghcr.yml
    // passes only CHAP_API_TOKEN, so without this the API answers every model
    // registration with 401.
    assert_eq!(
        merged["services"]["chap"]["environment"]["SERVICEKIT_REGISTRATION_KEY"].as_str(),
        Some(""),
        "the variable reaches the container, empty while .env sets none"
    );
    assert!(
        merged["services"]["chap"]["environment"]
            .get("CHAP_API_TOKEN")
            .is_some(),
        "adding to environment must not replace upstream's own variables"
    );
    // And gunicorn's control socket lands on the tmpfs rather than on the
    // read-only root, which it otherwise logs an error about on every start.
    assert_eq!(
        merged["services"]["chap"]["environment"]["XDG_RUNTIME_DIR"].as_str(),
        Some("/tmp")
    );
    assert!(
        merged["services"]["worker"]["environment"]
            .get("XDG_RUNTIME_DIR")
            .is_none(),
        "the worker runs celery, not gunicorn"
    );
    let ports = merged["services"]["chap"]["ports"]
        .as_sequence()
        .expect("chap publishes a port");
    assert_eq!(ports.len(), 1, "!override replaced the mapping: {ports:?}");
    assert_eq!(ports[0]["published"].as_str(), Some("8700"));
    assert_eq!(ports[0]["target"].as_u64(), Some(8000));
    assert!(
        merged["services"]["chapkit-ewars-model"]
            .get("ports")
            .is_none(),
        "the model is internal"
    );
}

#[test]
fn init_with_no_models_still_writes_a_valid_umbrella() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();

    let umbrella = yaml(&dir.join("compose.marketplace.yml"));
    assert!(
        umbrella.get("include").is_none(),
        "an empty include list is rejected by compose"
    );
    assert_eq!(umbrella["services"], Yaml::Mapping(Default::default()));
    assert_eq!(state(&dir)["models"], serde_json::json!({}));
    assert!(read(&dir.join(".env")).contains("none yet"));
}

#[test]
fn init_can_skip_the_env_file() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "none", "--no-env"])
        .assert()
        .success();
    assert!(!sandbox.project().join(".env").exists());
}

#[test]
fn enabling_a_second_model_publishes_nothing_and_sorts_the_umbrella() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();

    sandbox
        .models(&["enable", "auto_arima_chapkit"])
        .assert()
        .success()
        .stdout(predicates::str::contains("chap-devops up"))
        // The proxy URL is how a model with no host port is reached.
        .stdout(predicates::str::contains(
            "http://localhost:8700/v2/services/auto-arima-chapkit/run/",
        ));

    assert_eq!(
        state(&dir)["models"]["auto_arima_chapkit"]["host_port"],
        Json::Null
    );
    assert_eq!(
        includes(&dir),
        vec![
            "compose.auto-arima-chapkit.yml",
            "compose.chapkit-ewars-model.yml",
        ],
        "includes follow the marketplace id order"
    );
    assert!(read(&dir.join(".env")).contains(&format!(
        "# AUTO_ARIMA_CHAPKIT_IMAGE_TAG={}",
        stable_pin("auto_arima_chapkit").1
    )));
}

#[test]
fn json_errors_are_reported_as_json_on_stdout() {
    let sandbox = Sandbox::new();
    sandbox.init(&["--models", "none"]).assert().success();
    let out = sandbox
        .models(&["enable", "nope", "--json"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let value: Json = serde_json::from_slice(&out).expect("the error is JSON");
    assert!(value["error"].as_str().unwrap().contains("unknown model"));
}

/// A token `.env` cannot hold is refused before `--force` removes anything:
/// the deployment it was run in keeps its overlay, its state and its `.env`.
#[test]
fn a_rejected_api_token_leaves_a_forced_init_undone() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let base = port_base();
    sandbox
        .init(&[
            "--models",
            "chapkit_ewars_model",
            "--port-base",
            &base.to_string(),
        ])
        .assert()
        .success();
    let overlay = dir.join("compose.chapkit-ewars-model.yml");
    let env_before = read(&dir.join(".env"));
    assert!(overlay.exists());

    sandbox
        .init(&[
            "--models",
            "none",
            "--force",
            "--fresh-env",
            "--api-token",
            "has\"quote",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--api-token contains a quote"));
    assert!(overlay.exists(), "the overlay outlived the rejected token");
    assert!(state(&dir)["models"].get("chapkit_ewars_model").is_some());
    assert_eq!(read(&dir.join(".env")), env_before);
}

#[test]
fn a_second_init_needs_force() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let base = port_base();
    sandbox
        .init(&[
            "--models",
            "chapkit_ewars_model",
            "--port-base",
            &base.to_string(),
        ])
        .assert()
        .success();

    sandbox
        .init(&["--models", "none"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--force"));
    // The failed run changed nothing.
    assert!(state(&dir)["models"].get("chapkit_ewars_model").is_some());

    sandbox
        .init(&[
            "--models",
            "none",
            "--force",
            "--port-base",
            &base.to_string(),
        ])
        .assert()
        .success();
    assert_eq!(state(&dir)["models"], serde_json::json!({}));
    // --force rewrites the state, so the old overlay is dropped, not orphaned.
    assert!(includes(&dir).is_empty());
    assert!(!dir.join("compose.chapkit-ewars-model.yml").exists());

    // A host port the old overlay held is free again for the next model.
    sandbox
        .models(&["enable", "auto_arima_chapkit", "--port", "auto"])
        .assert()
        .success();
    assert_eq!(
        state(&dir)["models"]["auto_arima_chapkit"]["host_port"],
        base
    );
}

#[test]
fn force_keeps_the_env_file_it_found() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();
    let before = read(&dir.join(".env"));

    // .env holds the database password the postgres volume was created with,
    // the API token and the registration key: --force re-renders everything
    // else, but rewriting this file locks the operator out of their own data.
    sandbox
        .init(&["-v", "--models", "none", "--force"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: kept .env (already present)",
        ));

    let after = read(&dir.join(".env"));
    assert_eq!(
        password_line(&after),
        password_line(&before),
        "--force rotated POSTGRES_PASSWORD"
    );
    assert_eq!(after, before, "--force rewrote .env");

    // A kept file is reported as kept rather than as one of init's writes.
    let out = sandbox
        .init(&["--models", "none", "--force", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Json = serde_json::from_slice(&out).expect("init --json is JSON");
    assert_eq!(value["env"], "kept");
    let written: Vec<&str> = value["written"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        !written
            .iter()
            .any(|p| Path::new(p).file_name() == Some(".env".as_ref())),
        "the kept .env is listed as written: {written:?}"
    );
    assert_eq!(read(&dir.join(".env")), before);
}

#[test]
fn fresh_env_rewrites_the_env_file_and_warns() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();
    let before = read(&dir.join(".env"));

    let out = sandbox
        .init(&["--models", "chapkit_ewars_model", "--force", "--fresh-env"])
        .assert()
        .success()
        .stderr(predicates::str::contains("POSTGRES_PASSWORD"))
        .stderr(predicates::str::contains("chap-devops down --volumes"))
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).expect("utf-8 stdout");
    assert!(!stdout.contains("kept .env"), "{stdout}");

    let after = read(&dir.join(".env"));
    assert_ne!(
        password_line(&after),
        password_line(&before),
        "--fresh-env kept the old password"
    );
    // The file is a complete render again, pin comment included.
    assert!(after.contains(&format!(
        "# CHAPKIT_EWARS_MODEL_IMAGE_TAG={}",
        stable_pin("chapkit_ewars_model").1
    )));
}

#[test]
fn fresh_env_and_no_env_are_mutually_exclusive() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "none", "--fresh-env", "--no-env"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
    assert!(!sandbox.project().exists());
}

#[test]
fn the_chap_tag_reaches_the_env_file_and_the_state() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["-v", "--models", "none", "--chap-tag", "v1.2.3"])
        .assert()
        .success()
        // Offline, the tag is taken as given but the compose file that tag
        // publishes cannot be downloaded; the summary and a warning say so.
        .stdout(predicates::str::contains(
            "hint: chap-core v1.2.3 is from the compose.ghcr.yml built into this binary",
        ))
        .stderr(predicates::str::contains("--offline"))
        .stderr(predicates::str::contains("compose.ghcr.yml"));

    assert_eq!(state(&dir)["chap_image_tag"], "v1.2.3");
    assert_eq!(
        state(&dir)["chap_compose_source"],
        serde_json::json!({"kind": "embedded"})
    );
    let env = read(&dir.join(".env"));
    assert!(env.contains("\nCHAP_IMAGE_TAG=v1.2.3\n"));
    assert!(!env.contains("# CHAP_IMAGE_TAG"));
    // The base file still reads the variable with `latest` as its default.
    assert!(read(&dir.join("compose.yml")).contains("${CHAP_IMAGE_TAG:-latest}"));
    // Nothing was cached, so nothing but the three state files is in .chap-devops/
    // - and the lock every command that writes them takes.
    let mut names: Vec<String> = std::fs::read_dir(dir.join(".chap-devops"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "lock")
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["components.yaml", "models.yaml", "project.yaml"]
    );
}

#[test]
fn offline_leaves_the_default_tag_moving_and_says_so() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    // The default is `latest`, which init resolves to the newest release when
    // it can reach GitHub. It cannot here, so the literal tag survives and the
    // warning spells out what that means.
    sandbox
        .init(&["-v", "--models", "none"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: chap-core latest is from the compose.ghcr.yml built into this binary",
        ))
        .stderr(predicates::str::contains(
            "the chap-core tag stays `latest`",
        ))
        .stderr(predicates::str::contains("--chap-tag vX.Y.Z"));

    assert_eq!(state(&dir)["chap_image_tag"], "latest");
    assert_eq!(
        state(&dir)["chap_compose_source"],
        serde_json::json!({"kind": "embedded"})
    );
    // A moving tag is the compose default, so the .env line stays commented.
    let env = read(&dir.join(".env"));
    assert!(env.contains("\n# CHAP_IMAGE_TAG=latest\n"), "{env}");
}

#[test]
fn init_reuses_a_cached_compose_file_and_sync_renders_from_it() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();

    // Seed the cache the way an online `init --chap-tag v9.9.9` would have,
    // then re-init at that tag: offline, the copy on disk is used.
    let cached = dir.join(".chap-devops/compose.chap-core.v9.9.9.yml");
    let body = "services:\n  chap:\n    image: ghcr.io/x/chap-core:${CHAP_IMAGE_TAG:-latest}\n";
    std::fs::write(&cached, body).unwrap();
    sandbox
        .init(&["-v", "--models", "none", "--force", "--chap-tag", "v9.9.9"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: chap-core v9.9.9 is from chap-core compose.ghcr.yml at v9.9.9",
        ));

    let state = state(&dir);
    assert_eq!(state["chap_image_tag"], "v9.9.9");
    assert_eq!(state["chap_compose_source"]["kind"], "fetched");
    assert_eq!(state["chap_compose_source"]["tag"], "v9.9.9");
    assert!(
        state["chap_compose_source"]["url"]
            .as_str()
            .unwrap()
            .ends_with("/dhis2-chap/chap-core/v9.9.9/compose.ghcr.yml")
    );
    assert_eq!(
        state["chap_compose_source"]["sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );

    // compose.yml is the cached body behind two header lines.
    let base = read(&dir.join("compose.yml"));
    assert!(
        base.starts_with("# Generated by chap-devops from .chap-devops/; edit there and run `chap-devops sync`.\n"),
        "{base}"
    );
    assert!(base.contains("# chap-core compose.ghcr.yml at v9.9.9\n"));
    assert!(base.ends_with(body));
    assert_eq!(read(&cached), body, "the raw copy is left as fetched");

    // And a later sync renders the same file from the same copy.
    let mut sync = sandbox.chap();
    sync.arg("-C").arg(&dir).args(["sync", "--check"]);
    sync.assert().success().stdout(predicates::str::contains(
        "in sync: 0 to write, 3 unchanged, 0 to remove",
    ));
}

#[test]
fn sync_restores_a_hand_edited_compose_yml() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();
    let base = dir.join("compose.yml");
    let original = read(&base);

    std::fs::write(&base, "services: {}\n").unwrap();
    let mut check = sandbox.chap();
    check.arg("-C").arg(&dir).args(["sync", "--check"]);
    check
        .assert()
        .failure()
        .stdout(predicates::str::contains("would write compose.yml\n"))
        .stderr(predicates::str::contains("chap-devops sync"));
    assert_eq!(read(&base), "services: {}\n", "--check writes nothing");

    let mut sync = sandbox.chap();
    sync.arg("-C").arg(&dir).args(["-v", "sync"]);
    sync.assert()
        .success()
        .stdout(predicates::str::contains("hint: wrote compose.yml\n"));
    assert_eq!(
        read(&base),
        original,
        "the base stack is rendered, not held"
    );
}

#[test]
fn a_cached_compose_file_that_goes_missing_leaves_compose_yml_alone() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();
    let cached = dir.join(".chap-devops/compose.chap-core.v9.9.9.yml");
    std::fs::write(
        &cached,
        "services:\n  chap:\n    image: x:${CHAP_IMAGE_TAG:-latest}\n",
    )
    .unwrap();
    sandbox
        .init(&["--models", "none", "--force", "--chap-tag", "v9.9.9"])
        .assert()
        .success();
    let before = read(&dir.join("compose.yml"));

    std::fs::remove_file(&cached).unwrap();
    let mut sync = sandbox.chap();
    sync.arg("-C").arg(&dir).args(["-v", "sync"]);
    let out = sync
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "warning: .chap-devops/compose.chap-core.v9.9.9.yml is missing",
        ))
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).expect("utf-8 stdout");
    assert!(
        !stdout.lines().any(|l| l.ends_with(" compose.yml")),
        "a base file we cannot reproduce is not reported as written: {stdout}"
    );
    assert_eq!(read(&dir.join("compose.yml")), before);
}

#[test]
fn the_port_base_moves_the_whole_range() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let base = port_base();
    sandbox
        .init(&[
            "--models",
            "chapkit_ewars_model",
            "--port-base",
            &base.to_string(),
        ])
        .assert()
        .success();
    assert_eq!(state(&dir)["port_range"], serde_json::json!([base, 5999]));

    // The range only matters once a port is asked for.
    sandbox
        .models(&["expose", "chapkit_ewars_model"])
        .assert()
        .success();
    assert_eq!(
        state(&dir)["models"]["chapkit_ewars_model"]["host_port"],
        base
    );
}

#[test]
fn the_api_port_reaches_the_env_file_the_state_and_status() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    // A port of this test's own: `status` below probes it for real, so a
    // fixed number would be a question about the developer's machine.
    let port = free_port();
    sandbox
        .init(&["--models", "none", "--api-port", &port.to_string()])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "chap-core: latest, API on http://localhost:{port}"
        )));

    assert_eq!(state(&dir)["api_port"], port);
    let env = read(&dir.join(".env"));
    assert!(env.contains(&format!("\nCHAP_API_PORT={port}\n")), "{env}");
    assert!(
        read(&dir.join("compose.chap-devops.yml"))
            .contains(&format!("${{CHAP_API_PORT:-{port}}}:8000"))
    );

    // `status --url` defaults to the port the project records. The API is not
    // running, so the report is a down one - at the right URL.
    let mut status = sandbox.chap();
    status.arg("-C").arg(&dir).args(["status", "--json"]);
    let out = status.assert().failure().get_output().stdout.clone();
    let value: Json = serde_json::from_slice(&out).expect("status --json is JSON");
    assert_eq!(value["api_url"], format!("http://localhost:{port}"));
    assert_eq!(value["api"]["state"], "down");
}

#[test]
fn a_kept_env_file_that_pins_another_api_port_is_reported() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    // Two ports of this test's own: the last assertion is that nothing was
    // said about CHAP_API_PORT, and `init` warns about a port something else
    // on this machine is holding in those very words.
    let pinned = free_port();
    let moved = free_port();
    sandbox
        .init(&["--models", "none", "--api-port", &pinned.to_string()])
        .assert()
        .success();

    // `--force` keeps .env, and compose reads .env after the compose files, so
    // the line in it wins over the new --api-port. Say so.
    sandbox
        .init(&[
            "-v",
            "--models",
            "none",
            "--force",
            "--api-port",
            &moved.to_string(),
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: kept .env (already present)",
        ))
        .stderr(predicates::str::contains(format!(
            "warning: .env already sets CHAP_API_PORT={pinned}"
        )))
        .stderr(predicates::str::contains(format!(
            "the API stays on {pinned}"
        )));
    // The recorded intent did move, and so did the rendered override.
    assert_eq!(state(&dir)["api_port"], moved);
    assert!(
        read(&dir.join("compose.chap-devops.yml"))
            .contains(&format!("${{CHAP_API_PORT:-{moved}}}"))
    );
    assert!(read(&dir.join(".env")).contains(&format!("\nCHAP_API_PORT={pinned}\n")));

    // Re-initialising at the port .env already holds says nothing.
    let out = sandbox
        .init(&[
            "--models",
            "none",
            "--force",
            "--api-port",
            &pinned.to_string(),
        ])
        .assert()
        .success()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(out).expect("utf-8 stderr");
    assert!(!stderr.contains("CHAP_API_PORT"), "{stderr}");
}

#[test]
fn init_warns_when_something_is_already_listening_on_the_api_port() {
    // A listener on a port the kernel picked, so the test never fights another
    // process over a fixed number.
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
    let port = listener.local_addr().unwrap().port();

    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--api-port", &port.to_string()])
        .assert()
        // A warning, not a refusal: the process holding the port may be a
        // stack this deployment is meant to replace.
        .success()
        .stderr(predicates::str::contains(format!(
            "port {port} is already in use on this machine (needed by chap)"
        )))
        .stderr(predicates::str::contains("set CHAP_API_PORT="));

    // The directory is written all the same, at the port that was asked for.
    assert_eq!(state(&dir)["api_port"], port);
    let out = sandbox
        .init(&[
            "--models",
            "none",
            "--force",
            "--api-port",
            &port.to_string(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Json = serde_json::from_slice(&out).expect("init --json is JSON");
    assert_eq!(value["api_port_busy"], true);
    drop(listener);
}

/// The gap the live probe cannot see: `chap-devops init a && chap-devops init b` with
/// nothing running gives two deployments on one port, and nothing says so
/// until the second `chap-devops up`.
#[test]
fn init_warns_when_a_deployment_beside_it_already_uses_the_port() {
    let sandbox = Sandbox::new();
    // A port nothing listens on, so the only thing that can make it busy is
    // the deployment beside it - not a parallel test that happened to pick
    // the same fixed number.
    let port = free_port().to_string();
    let port = port.as_str();
    sandbox
        .chap()
        .arg("init")
        .arg("a")
        .args(["--models", "none", "--api-port", port])
        .assert()
        .success();
    // The CLI names the deployment by the resolved directory, whatever
    // spelling the shell it was started from happens to use.
    let first = resolved(&sandbox.home.path().join("a"));

    sandbox
        .chap()
        .arg("init")
        .arg("b")
        .args(["--models", "none", "--api-port", port])
        .assert()
        // A warning, not a refusal: two deployments on one port is a fine way
        // to take turns, and the directory is worth writing either way.
        .success()
        // The suggested port is the next free one, which this machine decides.
        .stderr(predicates::str::contains(format!(
            "port {port} is also used by a ({first}), which is not running; run only one of them \
             at a time, or set CHAP_API_PORT="
        )));

    // Written all the same, at the port that was asked for.
    assert_eq!(
        state(&sandbox.home.path().join("b"))["api_port"],
        port.parse::<u16>().unwrap()
    );
}

#[test]
fn init_on_a_port_no_other_deployment_uses_warns_about_nothing() {
    let sandbox = Sandbox::new();
    sandbox
        .chap()
        .arg("init")
        .arg("a")
        .args(["--models", "none", "--api-port", &free_port().to_string()])
        .assert()
        .success();
    sandbox
        .chap()
        .arg("init")
        .arg("b")
        .args(["--models", "none", "--api-port", &free_port().to_string()])
        .assert()
        .success()
        .stderr(predicates::str::contains("is also used by").not());
}

#[test]
fn re_enabling_keeps_the_port_and_applies_the_overrides() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let base = port_base();
    sandbox
        .init(&[
            "--models",
            "chapkit_ewars_model,auto_arima_chapkit",
            "--port-base",
            &base.to_string(),
        ])
        .assert()
        .success();
    sandbox
        .models(&["expose", "chapkit_ewars_model", "--port", "auto"])
        .assert()
        .success();
    assert_eq!(
        state(&dir)["models"]["chapkit_ewars_model"]["host_port"],
        base
    );

    sandbox
        .models(&[
            "enable",
            "chapkit_ewars_model",
            "--data-dir",
            "/srv/data",
            "--user",
            "1000:1000",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("updated chapkit_ewars_model"));

    let model = &state(&dir)["models"]["chapkit_ewars_model"];
    assert_eq!(model["host_port"], base, "the port survives a rewrite");
    assert_eq!(model["data_dir"], "/srv/data");
    assert_eq!(model["user"], "1000:1000");

    let overlay = yaml(&dir.join("compose.chapkit-ewars-model.yml"));
    let svc = &overlay["services"]["chapkit-ewars-model"];
    assert_eq!(svc["user"].as_str(), Some("1000:1000"));
    assert_eq!(svc["volumes"][1]["target"].as_str(), Some("/srv/data"));
}

#[test]
fn commands_find_the_project_from_a_subdirectory() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();
    let sub = dir.join("ops").join("notes");
    std::fs::create_dir_all(&sub).unwrap();

    chap_in(&sandbox, &sub, &["models", "list", "--enabled"])
        .assert()
        .success()
        .stdout(predicates::str::contains("chapkit_ewars_model"));
    chap_in(&sandbox, &sub, &["sync", "--check"])
        .assert()
        .success()
        .stdout(predicates::str::contains("in sync"));

    // Above the project there is nothing to find, and the error names where
    // the search started.
    chap_in(&sandbox, sandbox.home.path(), &["sync", "--check"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not a chap-devops deployment"))
        .stderr(predicates::str::contains(".chap-devops/project.yaml"));
}

#[test]
fn sync_check_is_clean_after_init_and_reports_drift_after_a_deletion() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();

    let mut check = sandbox.chap();
    check
        .arg("-C")
        .arg(&dir)
        .args(["sync", "--check", "--json"]);
    let out = check.assert().success().get_output().stdout.clone();
    let value: Json = serde_json::from_slice(&out).expect("sync --json is JSON");
    assert_eq!(value["drift"], false);
    assert_eq!(value["check"], true);
    assert!(value["written"].as_array().unwrap().is_empty());
    assert_eq!(value["unchanged"].as_array().unwrap().len(), 4);

    let overlay = dir.join("compose.chapkit-ewars-model.yml");
    std::fs::remove_file(&overlay).unwrap();
    let mut check = sandbox.chap();
    check.arg("-C").arg(&dir).args(["sync", "--check"]);
    check
        .assert()
        .failure()
        .stdout(predicates::str::contains(
            "would write compose.chapkit-ewars-model.yml\n",
        ))
        .stderr(predicates::str::contains("chap-devops sync"));
    assert!(!overlay.exists(), "--check writes nothing");

    // --json exits non-zero too, with the report as the only stdout document.
    let mut check = sandbox.chap();
    check
        .arg("-C")
        .arg(&dir)
        .args(["sync", "--check", "--json"]);
    let out = check.assert().failure().get_output().stdout.clone();
    let value: Json = serde_json::from_slice(&out).expect("one JSON document");
    assert_eq!(value["drift"], true);
}

#[test]
fn sync_recreates_a_deleted_overlay() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();
    let overlay = dir.join("compose.chapkit-ewars-model.yml");
    let original = read(&overlay);
    std::fs::remove_file(&overlay).unwrap();

    let mut sync = sandbox.chap();
    sync.arg("-C").arg(&dir).args(["-v", "sync"]);
    sync.assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: wrote compose.chapkit-ewars-model.yml\n",
        ))
        .stdout(predicates::str::contains(
            "1 written, 3 unchanged, 0 removed",
        ));
    assert_eq!(read(&overlay), original, "rendering is deterministic");

    let mut again = sandbox.chap();
    again.arg("-C").arg(&dir).arg("sync");
    again.assert().success().stdout(predicates::str::contains(
        "in sync: 0 written, 4 unchanged, 0 removed",
    ));
}

#[test]
fn sync_never_removes_a_hand_written_overlay() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model,auto_arima_chapkit"])
        .assert()
        .success();
    let custom = dir.join("compose.custom.yml");
    std::fs::write(&custom, "services:\n  mine:\n    image: busybox\n").unwrap();

    sandbox
        .models(&["disable", "auto_arima_chapkit", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: removed compose.auto-arima-chapkit.yml",
        ));
    assert!(!dir.join("compose.auto-arima-chapkit.yml").exists());
    assert!(custom.is_file());

    // A hand edit of models.yaml is picked up by sync the same way.
    let models = dir.join(".chap-devops/models.yaml");
    std::fs::write(&models, "# emptied by hand\n").unwrap();
    let mut sync = sandbox.chap();
    sync.arg("-C").arg(&dir).args(["-v", "sync"]);
    sync.assert().success().stdout(predicates::str::contains(
        "hint: removed compose.chapkit-ewars-model.yml\n",
    ));
    assert!(!dir.join("compose.chapkit-ewars-model.yml").exists());
    assert!(custom.is_file(), "compose.custom.yml is not ours to delete");
    assert!(includes(&dir).is_empty());
    assert_eq!(state(&dir)["models"], serde_json::json!({}));
}

#[test]
fn restart_outside_a_project_says_so() {
    let sandbox = Sandbox::new();
    chap_in(&sandbox, sandbox.home.path(), &["restart"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not a chap-devops deployment"));
}

#[test]
fn restart_says_what_it_is_for_and_what_it_leaves_alone() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();

    let help = chap_in(&sandbox, &dir, &["restart", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8(help).expect("help is text");
    assert!(help.contains("--all"), "{help}");
    assert!(help.contains("[SERVICE]"), "{help}");
    // One clause, and the clause is what the command is for. What it leaves
    // alone is the book's to explain, which the last line points at.
    assert!(
        help.contains("Recreate the named services even when nothing changed"),
        "{help}"
    );
    assert!(
        !help.contains("\n\n  It "),
        "no paragraphs in help:\n{help}"
    );
}

#[test]
fn restart_on_a_deployment_that_was_never_started_says_to_start_it() {
    if !docker_ready() {
        return;
    }
    let sandbox = Sandbox::new();
    // Its own directory: compose names the project after it, and this asks
    // docker about that name.
    let dir = sandbox.home.path().join("chap-devops-never-restarted");
    let mut init = sandbox.chap();
    init.arg("init").arg(&dir).args(["--models", "none"]);
    init.assert().success();

    chap_in(&sandbox, &dir, &["restart"])
        .assert()
        .failure()
        .stdout(predicates::str::contains(
            "Chap is not running; start it with `chap-devops up`",
        ));

    // And the same before it has any opinion about the service names it was
    // given: there is nothing to restart either way.
    chap_in(&sandbox, &dir, &["restart", "--all", "chap"])
        .assert()
        .failure()
        .stdout(predicates::str::contains("Chap is not running"));
}

#[test]
fn the_help_lists_only_the_commands_that_can_work_here() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();

    // Nothing has been created yet, so the whole home directory is outside a
    // project: only the commands that need no deployment are offered.
    let outside = chap_in(&sandbox, sandbox.home.path(), &["--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let outside = String::from_utf8(outside).expect("help is text");
    assert!(outside.contains("init"), "init has to be reachable");
    assert!(outside.contains("models"));
    assert!(outside.contains("registry"));
    assert!(
        !outside.contains("\n  up "),
        "up needs a project:\n{outside}"
    );
    assert!(
        !outside.contains("\n  docker "),
        "docker needs a project:\n{outside}"
    );
    assert!(
        !outside.contains("\n  backup "),
        "backup needs a project:\n{outside}"
    );
    assert!(
        !outside.contains("\n  auth "),
        "auth needs a project:\n{outside}"
    );
    // Hiding is all that changes: the help still ends on the docs link.
    assert!(
        outside
            .trim_end()
            .ends_with("Docs: https://dhis2-chap.github.io/chap-devops/"),
        "{outside}"
    );

    sandbox.init(&["--models", "none"]).assert().success();

    let inside = chap_in(&sandbox, &dir, &["--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inside = String::from_utf8(inside).expect("help is text");
    for name in [
        "up", "down", "logs", "restart", "docker", "backup", "status", "sync", "update", "ui",
        "auth",
    ] {
        assert!(
            inside.contains(&format!("\n  {name} ")),
            "{name} should be listed inside a project:\n{inside}"
        );
    }
    // And the mirror of it: here there is a deployment, so `init` is not the
    // command to run next and is not offered.
    assert!(
        !inside.contains("\n  init "),
        "init should not be listed inside a project:\n{inside}"
    );
    assert!(
        inside
            .trim_end()
            .ends_with("Docs: https://dhis2-chap.github.io/chap-devops/"),
        "{inside}"
    );

    // Hiding is cosmetic: a hidden command still runs, and `docker` reaches
    // its own help from a subdirectory of the project.
    let sub = dir.join("ops");
    std::fs::create_dir_all(&sub).unwrap();
    chap_in(&sandbox, &sub, &["docker", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("exec"))
        .stdout(predicates::str::contains("pull"));

    // And outside a project it is hidden, not removed.
    chap_in(&sandbox, sandbox.home.path(), &["docker", "ps"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not a chap-devops deployment"));
}

/// A deployment with nothing in it: `up` says so and names the ways to add
/// something, rather than passing compose's `no service selected` through.
#[test]
fn up_with_nothing_in_the_deployment_says_what_to_add() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--only", "none"]).assert().success();
    chap_in(&sandbox, &dir, &["up"])
        .assert()
        .success()
        .stdout(predicates::str::contains("nothing to start"))
        .stdout(predicates::str::contains("`chap-devops models add URL`"))
        .stdout(predicates::str::contains(
            "`chap-devops components enable NAME`",
        ))
        .stderr(predicates::str::contains("no service selected").not());
}

/// A deployment of nothing has nothing for `up` to start, so the next step
/// `init` names is putting something in it.
#[test]
fn init_of_an_empty_deployment_names_adding_a_model_next() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--only", "none"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "&& chap-devops models add URL` to add a model\n",
        ));
    let other = Sandbox::new();
    other
        .init(&["--only", "none", "--models", "chapkit_ewars_model"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "&& chap-devops up` to start the deployment\n",
        ));
}

/// `init --force` is the way out of broken state, and it keeps the compose
/// project name - and so the data - of the deployment it rebuilds, read from
/// `project.yaml` on its own even when another state file does not load.
#[test]
fn init_force_over_broken_state_keeps_the_compose_project_name() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--only", "none", "--models", "none"])
        .assert()
        .success();
    let dir = sandbox.project();
    let before = state(&dir)["compose_project"].as_str().unwrap().to_string();
    std::fs::write(
        dir.join(".chap-devops").join("components.yaml"),
        "chap_core: [oops",
    )
    .unwrap();

    sandbox
        .init(&["--only", "none", "--models", "none", "--force"])
        .assert()
        .success();
    assert_eq!(state(&dir)["compose_project"], before.as_str());
}

/// A `project.yaml` that cannot be read hides the name the data lives under,
/// so `init --force` stops rather than start the deployment over on a new one.
#[test]
fn init_force_refuses_when_the_compose_project_name_cannot_be_read() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--only", "none", "--models", "none"])
        .assert()
        .success();
    let dir = sandbox.project();
    let project = dir.join(".chap-devops").join("project.yaml");
    std::fs::write(&project, "compose_project: [oops").unwrap();

    sandbox
        .init(&["--only", "none", "--models", "none", "--force"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "fix `compose_project:` in that file",
        ));
    assert_eq!(read(&project), "compose_project: [oops");
}

/// `.env` holds the database password, the API token and the registration
/// key, so `init` writes it readable by its owner only.
#[cfg(unix)]
#[test]
fn init_writes_env_readable_by_its_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--only", "none", "--models", "none"])
        .assert()
        .success();
    let mode = std::fs::metadata(sandbox.project().join(".env"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

/// `init --force` keeps `.env` for its secrets, but the image pin in it is the
/// line `init` wrote. A deployment built from a checkout and then moved back
/// to a release must not keep `CHAP_IMAGE_TAG=checkout`, which no registry
/// publishes.
#[test]
fn init_force_moves_the_image_pin_of_a_kept_env() {
    let sandbox = Sandbox::new();
    let checkout = sandbox.home.path().join("chap-core");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::write(checkout.join("Dockerfile"), "FROM scratch\n").unwrap();
    std::fs::write(checkout.join("Dockerfile.worker"), "FROM scratch\n").unwrap();
    std::fs::write(
        checkout.join("compose.ghcr.yml"),
        "services:\n  chap:\n    image: ghcr.io/dhis2-chap/chap-core:${CHAP_IMAGE_TAG:-latest}\n  \
         worker:\n    image: ghcr.io/dhis2-chap/chap-worker:${CHAP_IMAGE_TAG:-latest}\n",
    )
    .unwrap();
    sandbox
        .init(&["--source", checkout.to_str().unwrap(), "--models", "none"])
        .assert()
        .success();
    let env = sandbox.env();
    assert_eq!(env_value(&env, "CHAP_IMAGE_TAG"), Some("checkout"));
    let password = password_line(&env).to_string();

    sandbox
        .init(&["--force", "--models", "none", "--chap-tag", "v1.2.3"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "moved CHAP_IMAGE_TAG in the kept .env from checkout to v1.2.3",
        ));
    let env = sandbox.env();
    assert_eq!(env_value(&env, "CHAP_IMAGE_TAG"), Some("v1.2.3"));
    assert_eq!(password_line(&env), password, "the secrets are kept");

    // The same tag again: nothing to move, and nothing said about it.
    sandbox
        .init(&["--force", "--models", "none", "--chap-tag", "v1.2.3"])
        .assert()
        .success()
        .stdout(predicates::str::contains("moved CHAP_IMAGE_TAG").not());
}

/// A gzipped dump whose newest Flyway migration is `version`.
fn dhis2_dump(path: &std::path::Path, version: &str) {
    use std::io::Write;
    let body = format!(
        "COPY public.flyway_schema_history (installed_rank, version, description) FROM stdin;\n\
         1\t{version}\tone\n\\.\n"
    );
    let file = std::fs::File::create(path).expect("the dump");
    let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
    gz.write_all(body.as_bytes()).expect("the dump body");
    gz.finish().expect("the dump is written");
}

/// A seed path is relative to the deployment directory, which is where the
/// bind mount reads it. A file of the same name in the working directory is
/// not the one the restore reads, so init does not take its version.
#[test]
fn init_reads_the_dump_version_only_from_the_deployment_directory() {
    let sandbox = Sandbox::new();
    dhis2_dump(&sandbox.home.path().join("laos.sql.gz"), "2.43.2");
    sandbox
        .init(&["--only", "dhis2", "--dhis2-seed", "laos.sql.gz"])
        .assert()
        .success()
        .stderr(predicates::str::contains("is DHIS2 2.43").not())
        .stderr(predicates::str::contains("laos.sql.gz is not in "))
        .stderr(predicates::str::contains(
            "so chap-devops did not read its DHIS2 version, and DHIS2 runs 2.42",
        ));

    let sandbox = Sandbox::new();
    std::fs::create_dir_all(sandbox.project()).unwrap();
    dhis2_dump(&sandbox.project().join("laos.sql.gz"), "2.41.5");
    dhis2_dump(&sandbox.home.path().join("laos.sql.gz"), "2.43.2");
    sandbox
        .init(&["--only", "dhis2", "--dhis2-seed", "laos.sql.gz"])
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "laos.sql.gz is DHIS2 2.41 (Flyway migration 2.41.5), so DHIS2 runs 2.41",
        ));
}
