use crate::common::*;
use serde_json::Value as Json;
use std::path::Path;
use std::path::PathBuf;

#[test]
fn disable_accepts_the_service_id_and_removes_the_overlay() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model,auto_arima_chapkit"])
        .assert()
        .success();
    assert_eq!(includes(&dir).len(), 2);

    sandbox
        .models(&["disable", "chapkit-ewars-model"])
        .assert()
        .success()
        .stdout(predicates::str::contains("disabled chapkit_ewars_model"));

    assert!(!dir.join("compose.chapkit-ewars-model.yml").exists());
    assert_eq!(includes(&dir), vec!["compose.auto-arima-chapkit.yml"]);
    assert!(state(&dir)["models"].get("chapkit_ewars_model").is_none());
}

#[test]
fn disabling_a_model_that_is_not_enabled_fails() {
    let sandbox = Sandbox::new();
    sandbox.init(&["--models", "none"]).assert().success();
    sandbox
        .models(&["disable", "auto_arima_chapkit"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("unknown model"));
}

/// `models add` names `--allow-starter` when its source is a starter, so it has
/// to take the flag.
#[test]
fn a_starter_added_by_its_repository_needs_allow_starter() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();
    let url = "https://github.com/chap-models/chapkit_minimalist_example_py";

    sandbox
        .models(&["add", url])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--allow-starter"));
    sandbox
        .models(&["add", url, "--allow-starter"])
        .assert()
        .success();
    assert!(
        dir.join("compose.chapkit-minimalist-example-py.yml")
            .is_file()
    );
}

#[test]
fn a_starter_needs_allow_starter() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();

    sandbox
        .models(&["enable", "chapkit_minimalist_example_py"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--allow-starter"));
    assert!(
        !dir.join("compose.chapkit-minimalist-example-py.yml")
            .exists()
    );

    sandbox
        .models(&["enable", "chapkit_minimalist_example_py", "--allow-starter"])
        .assert()
        .success();
    assert!(
        dir.join("compose.chapkit-minimalist-example-py.yml")
            .is_file()
    );
    // The starter's image is built from /work, unlike ewars, and it runs as
    // root - so its overlay hands it no user, and its init container chowns
    // the volume to root rather than to an account the image does not use.
    let model = &state(&dir)["models"]["chapkit_minimalist_example_py"];
    assert_eq!(model["data_dir"], "/work/data");
    assert_eq!(model["user"], "root");
    let overlay = yaml(&dir.join("compose.chapkit-minimalist-example-py.yml"));
    assert!(
        overlay["services"]["chapkit-minimalist-example-py"]
            .get("user")
            .is_none()
    );
    assert_eq!(
        overlay["services"]["chapkit-minimalist-example-py-init"]["command"][2].as_str(),
        Some("chown -R 0:0 /work/data")
    );
}

#[test]
fn enable_takes_an_explicit_port_and_rejects_a_taken_one() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let base = port_base();
    // A port of this test's own, above the base so `--port auto` below still
    // has the base itself to hand out.
    let fixed = base + 5;
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
        .models(&["enable", "auto_arima_chapkit", "--port", &fixed.to_string()])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "http://localhost:{fixed}"
        )));
    assert_eq!(
        state(&dir)["models"]["auto_arima_chapkit"]["host_port"],
        fixed
    );
    let svc = &yaml(&dir.join("compose.auto-arima-chapkit.yml"))["services"]["auto-arima-chapkit"];
    let published = format!("{fixed}:8000");
    assert_eq!(svc["ports"][0].as_str(), Some(published.as_str()));
    assert_eq!(
        svc["expose"][0].as_str(),
        Some("8000"),
        "expose documents the container port either way"
    );

    sandbox
        .models(&[
            "enable",
            "chapkit_simple_multistep_model",
            "--port",
            &fixed.to_string(),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "already in use by another compose file",
        ));

    // `--port auto` takes the lowest free one instead.
    sandbox
        .models(&["enable", "chapkit_simple_multistep_model", "--port", "auto"])
        .assert()
        .success();
    assert_eq!(
        state(&dir)["models"]["chapkit_simple_multistep_model"]["host_port"],
        base
    );

    // Anything that is neither a number nor `auto` is a parse error.
    sandbox
        .models(&["enable", "auto_arima_chapkit", "--port", "nope"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("`auto`"));
}

#[test]
fn expose_and_unexpose_move_a_models_host_port_without_moving_its_pin() {
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
    let pinned = read(&dir.join(".chap-devops/models.yaml"));

    sandbox
        .models(&["expose", "chapkit_ewars_model", "--port", "auto", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "exposed chapkit-ewars-model on http://localhost:{base}",
        )))
        .stdout(predicates::str::contains(
            "hint: wrote compose.chapkit-ewars-model.yml",
        ));
    assert_eq!(
        state(&dir)["models"]["chapkit_ewars_model"]["host_port"],
        base
    );
    let svc =
        &yaml(&dir.join("compose.chapkit-ewars-model.yml"))["services"]["chapkit-ewars-model"];
    let published = format!("{base}:8000");
    assert_eq!(svc["ports"][0].as_str(), Some(published.as_str()));

    // Only the port moved: the version and the image pin are untouched.
    let after = read(&dir.join(".chap-devops/models.yaml"));
    assert_eq!(
        after.replace(&format!("host_port: {base}"), "host_port: null"),
        pinned,
        "expose re-resolved the version"
    );

    // The service id works too, and unexpose names the way back in.
    sandbox
        .models(&["unexpose", "chapkit-ewars-model"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "unexposed chapkit-ewars-model; it stays registered with chap-core and reachable \
             at http://localhost:8700/v2/services/chapkit-ewars-model/run/",
        ));
    assert_eq!(
        state(&dir)["models"]["chapkit_ewars_model"]["host_port"],
        Json::Null
    );
    let svc =
        &yaml(&dir.join("compose.chapkit-ewars-model.yml"))["services"]["chapkit-ewars-model"];
    assert!(svc.get("ports").is_none());
    assert_eq!(read(&dir.join(".chap-devops/models.yaml")), pinned);

    // A model this project never enabled cannot be exposed.
    sandbox
        .models(&["expose", "auto_arima_chapkit"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("unknown model"));
}

#[test]
fn models_list_and_info_name_the_proxy_until_a_port_is_published() {
    let sandbox = Sandbox::new();
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
        .models(&["list", "--enabled"])
        .assert()
        .success()
        .stdout(predicates::str::contains("via chap-core"));
    sandbox
        .models(&["info", "chapkit_ewars_model"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "reach     internal (proxy: http://localhost:8700/v2/services/chapkit-ewars-model/run/)",
        ));

    sandbox
        .models(&["expose", "chapkit_ewars_model"])
        .assert()
        .success();
    let listed = String::from_utf8(
        sandbox
            .models(&["list", "--enabled"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .expect("the table is text");
    assert!(listed.contains(&base.to_string()), "{listed}");
    assert!(!listed.contains("via chap-core"), "{listed}");
    sandbox
        .models(&["info", "chapkit_ewars_model"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "reach     http://localhost:{base}"
        )));
}

#[test]
fn json_output_parses_for_init_and_enable() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let base = port_base();
    let out = sandbox
        .init(&[
            "--models",
            "chapkit_ewars_model",
            "--json",
            "--port-base",
            &base.to_string(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Json = serde_json::from_slice(&out).expect("init --json is JSON");
    assert_eq!(PathBuf::from(value["dir"].as_str().unwrap()), dir);
    let written: Vec<PathBuf> = value["written"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            Path::new(v.as_str().unwrap())
                .strip_prefix(&dir)
                .unwrap()
                .to_path_buf()
        })
        .collect();
    for name in [
        "compose.yml",
        ".env",
        "compose.chapkit-ewars-model.yml",
        "compose.marketplace.yml",
        ".chap-devops/project.yaml",
        ".chap-devops/models.yaml",
    ] {
        // Compared as paths: the separator the CLI prints is the platform's.
        assert!(
            written.contains(&PathBuf::from(name)),
            "{name} not reported"
        );
    }
    assert_eq!(value["report"]["enabled"][0][0], "chapkit_ewars_model");
    assert_eq!(value["report"]["enabled"][0][1]["host_port"], Json::Null);
    assert_eq!(value["env"], "written");
    assert_eq!(value["api_port"], 8700);
    assert_eq!(value["api_url"], "http://localhost:8700");

    let out = sandbox
        .models(&["enable", "auto_arima_chapkit", "--port", "auto", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Json = serde_json::from_slice(&out).expect("enable --json is JSON");
    assert_eq!(value["enabled"][0][1]["host_port"], base);
    assert!(value["disabled"].as_array().unwrap().is_empty());

    // expose/unexpose have a JSON shape of their own.
    let out = sandbox
        .models(&["unexpose", "auto_arima_chapkit", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Json = serde_json::from_slice(&out).expect("unexpose --json is JSON");
    assert_eq!(value["id"], "auto_arima_chapkit");
    assert_eq!(value["service_id"], "auto-arima-chapkit");
    assert_eq!(value["host_port"], Json::Null);
    assert_eq!(value["previous"], base);
    assert_eq!(
        value["url"],
        "http://localhost:8700/v2/services/auto-arima-chapkit/run/"
    );
}

#[test]
fn forcing_the_same_model_starts_its_state_over() {
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
        .models(&["expose", "chapkit_ewars_model", "--port", &base.to_string()])
        .assert()
        .success();

    sandbox
        .init(&[
            "--models",
            "chapkit_ewars_model",
            "--force",
            "--port-base",
            &base.to_string(),
        ])
        .assert()
        .success();
    // `--force` re-renders the whole state, and `init` publishes no model
    // ports, so a port someone exposed by hand goes with it. Re-expose it.
    assert_eq!(
        state(&dir)["models"]["chapkit_ewars_model"]["host_port"],
        Json::Null
    );
    assert!(!dir.join("compose.auto-arima-chapkit.yml").exists());
    assert_eq!(includes(&dir), vec!["compose.chapkit-ewars-model.yml"]);

    // And the port it used to hold is free, not orphaned in a stale overlay.
    sandbox
        .models(&["expose", "chapkit_ewars_model", "--port", &base.to_string()])
        .assert()
        .success();
}

#[test]
fn enable_outside_a_project_says_so() {
    let sandbox = Sandbox::new();
    sandbox
        .models(&["enable", "chapkit_ewars_model"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not a chap-devops deployment"));
}

#[test]
fn an_exact_version_pins_without_a_channel() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let pinned = stable_pin("auto_arima_chapkit").0;
    sandbox.init(&["--models", "none"]).assert().success();
    sandbox
        .models(&["enable", "auto_arima_chapkit", "--version", &pinned])
        .assert()
        .success();

    let model = &state(&dir)["models"]["auto_arima_chapkit"];
    assert_eq!(model["version"], pinned);
    assert_eq!(model["channel"], Json::Null);

    sandbox
        .models(&["enable", "auto_arima_chapkit", "--version", "9.9.9"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("no version `9.9.9`"));
}

#[test]
fn bind_puts_the_host_address_in_front_of_the_published_port() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let base = port_base();
    sandbox
        .init(&["--models", "none", "--port-base", &base.to_string()])
        .assert()
        .success();

    sandbox
        .models(&[
            "enable",
            "chapkit_ewars_model",
            "--port",
            "auto",
            "--bind",
            "127.0.0.1",
        ])
        .assert()
        .success();
    assert_eq!(
        state(&dir)["models"]["chapkit_ewars_model"]["bind"],
        "127.0.0.1"
    );
    let overlay = read(&dir.join("compose.chapkit-ewars-model.yml"));
    assert!(
        overlay.contains(&format!("\"127.0.0.1:{base}:8000\"")),
        "{overlay}"
    );

    // expose moves the address along with the port, and keeps it when no
    // address is given.
    sandbox
        .models(&["expose", "chapkit_ewars_model", "--bind", "0.0.0.0"])
        .assert()
        .success();
    assert_eq!(
        state(&dir)["models"]["chapkit_ewars_model"]["bind"],
        "0.0.0.0"
    );
    sandbox
        .models(&["expose", "chapkit_ewars_model"])
        .assert()
        .success();
    assert_eq!(
        state(&dir)["models"]["chapkit_ewars_model"]["bind"],
        "0.0.0.0"
    );
    let overlay = read(&dir.join("compose.chapkit-ewars-model.yml"));
    assert!(overlay.contains("\"0.0.0.0:"), "{overlay}");

    sandbox
        .models(&["enable", "auto_arima_chapkit", "--bind", "not-an-address"])
        .assert()
        .failure();
}

#[test]
fn changing_commands_say_ok_and_name_the_model_under_json() {
    let sandbox = Sandbox::new();
    let base = port_base();
    sandbox
        .init(&["--models", "none", "--port-base", &base.to_string()])
        .assert()
        .success();

    let doc = json_of(&mut sandbox.models(&[
        "enable",
        "chapkit_ewars_model",
        "--port",
        "auto",
        "--json",
    ]));
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["models"][0]["id"], "chapkit_ewars_model");
    assert_eq!(doc["models"][0]["service_id"], "chapkit-ewars-model");
    assert_eq!(doc["models"][0]["port"], base);
    assert_eq!(doc["models"][0]["url"], format!("http://localhost:{base}"));

    let doc = json_of(&mut sandbox.models(&["expose", "chapkit_ewars_model", "--json"]));
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["service_id"], "chapkit-ewars-model");

    let doc = json_of(&mut sandbox.models(&["disable", "chapkit_ewars_model", "--json"]));
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["disabled"][0], "chapkit_ewars_model");

    // A refusal is JSON too, with the way out on its own.
    let out = sandbox
        .models(&[
            "add",
            "ghcr.io/chap-models/chapkit_ewars_model:sha-0000000",
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let doc: Json = serde_json::from_slice(&out).expect("the error is one JSON document");
    assert_eq!(doc["ok"], false);
    assert!(
        doc["hint"]
            .as_str()
            .unwrap_or_default()
            .contains("--id auto"),
        "{doc}"
    );
}

/// Asking again for what the deployment already has changes nothing, says
/// so, and does not ask for `chap-devops up`.
#[test]
fn a_second_enable_or_expose_says_nothing_changed() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();
    let out = sandbox
        .models(&["enable", "chapkit_ewars_model"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains("chapkit_ewars_model v") && text.contains("is already enabled"),
        "{text}"
    );
    assert!(text.contains("nothing changed"), "{text}");
    assert!(!text.contains("chap-devops up"), "{text}");
    assert!(!text.contains("updated"), "{text}");

    sandbox
        .models(&["expose", "chapkit_ewars_model", "--port", "auto"])
        .assert()
        .success()
        .stdout(predicates::str::contains("run `chap-devops up` to apply"));
    let port = state(&dir)["models"]["chapkit_ewars_model"]["host_port"].clone();
    let out = sandbox
        .models(&["expose", "chapkit_ewars_model", "--port", "auto"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains(&format!(
            "chapkit-ewars-model is already exposed on http://localhost:{port}; nothing changed"
        )),
        "{text}"
    );
    assert!(!text.contains("run `chap-devops up` to apply"), "{text}");
    assert_eq!(
        state(&dir)["models"]["chapkit_ewars_model"]["host_port"],
        port
    );
}

#[test]
fn enable_takes_several_ids_and_refuses_the_options_of_one_model() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();

    sandbox
        .models(&["enable", "chapkit_ewars_model,auto_arima_chapkit"])
        .assert()
        .success();
    sandbox
        .models(&["enable", "chapkit_simple_multistep_model"])
        .assert()
        .success();
    let models = state(&dir)["models"].clone();
    for id in [
        "chapkit_ewars_model",
        "auto_arima_chapkit",
        "chapkit_simple_multistep_model",
    ] {
        assert!(!models[id].is_null(), "{id} is enabled: {models}");
    }
    assert_eq!(includes(&dir).len(), 3);

    sandbox
        .models(&[
            "enable",
            "chapkit_rwanda_malaria_bym_model",
            "chapkit_ghr_model",
            "--version",
            "1.0.0",
        ])
        .assert()
        .code(2)
        .stderr(predicates::str::contains(
            "--version is for one model, and this names 2",
        ));
    assert!(state(&dir)["models"]["chapkit_ghr_model"].is_null());
}
