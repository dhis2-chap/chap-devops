use crate::common::*;
use predicates::prelude::PredicateBooleanExt;
use serde_json::Value as Json;
use serde_yaml_ng::Value as Yaml;

/// A server that answers a component's port the way a running OCS would, on a
/// port of its own.
///
/// Every 200 counts as an answer to `/health`, and the body is the dataset list
/// `chap-devops status` counts - so a state or a count taken from this server is proof
/// that it was asked something. Nothing here needs docker, so the verdict is the
/// same on every machine.
fn ocs_lookalike() -> u16 {
    server(
        "application/json",
        r#"{"kind":"DatasetList","items":[{"dataset_id":"worldpop"}]}"#,
    )
}

#[test]
fn init_with_ocs_writes_the_component_and_its_scaffold() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["-v", "--models", "none", "--with", "ocs"])
        .assert()
        .success()
        .stdout(predicates::str::contains("components: chap-core, ocs"))
        .stdout(predicates::str::contains("OCS: http://localhost:8790"))
        // The heads-up about the object store OCS will need, once.
        .stdout(predicates::str::contains(
            "chap-devops components enable s3",
        ));

    for name in [
        "compose.ocs.yml",
        "ocs/climate-service.yaml",
        ".chap-devops/components.yaml",
    ] {
        assert!(dir.join(name).is_file(), "{name} was not written");
    }
    assert!(
        !dir.join("compose.s3.yml").exists(),
        "the object store was not asked for"
    );

    // The component file sits between the chap-devops override and the umbrella.
    let state = state(&dir);
    assert_eq!(
        state["compose_files"],
        serde_json::json!([
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.ocs.yml",
            "compose.marketplace.yml"
        ])
    );
    assert!(
        state["rendered_files"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("compose.ocs.yml"))
    );

    let components = yaml(&dir.join(".chap-devops/components.yaml"));
    assert_eq!(components["chap-core"]["enabled"], Yaml::Bool(true));
    assert_eq!(components["ocs"]["enabled"], Yaml::Bool(true));
    assert_eq!(components["ocs"]["port"].as_u64(), Some(8790));
    assert_eq!(components["ocs"]["image_tag"].as_str(), Some("main"));
    assert_eq!(components["s3"]["enabled"], Yaml::Bool(false));

    // The service itself: published, no S3 variables yet, no platform pin.
    let ocs = yaml(&dir.join("compose.ocs.yml"));
    let svc = &ocs["services"]["ocs"];
    assert_eq!(
        svc["image"].as_str(),
        Some("ghcr.io/dhis2/open-climate-service:${OCS_IMAGE_TAG:-main}")
    );
    assert_eq!(svc["expose"][0].as_str(), Some("9000"));
    assert_eq!(svc["ports"][0].as_str(), Some("8790:9000"));
    assert!(svc.get("platform").is_none());
    assert!(svc["environment"].get("S3_ENDPOINT").is_none());
    // The five dataset credentials are always passed, empty until .env has
    // them, and so is the base URL: OCS reads an empty value as absent.
    let env_block = &svc["environment"];
    for var in [
        "ECMWF_DATASTORES_URL",
        "ECMWF_DATASTORES_KEY",
        "EDH_API_KEY",
        "CDSE_S3_ACCESS_KEY",
        "CDSE_S3_SECRET_KEY",
        "CLIMATE_SERVICE_BASE_URL",
    ] {
        assert_eq!(
            env_block[var].as_str(),
            Some(format!("${{{var}:-}}").as_str()),
            "{var}"
        );
    }
    // Nothing to mount: the plugin directory is opt-in.
    assert_eq!(svc["volumes"].as_sequence().unwrap().len(), 2);

    // The scaffold is the Laos example, and says so where doctor can see it.
    let config = read(&dir.join("ocs/climate-service.yaml"));
    assert!(config.contains("Laos example values"), "{config}");
    let parsed = yaml(&dir.join("ocs/climate-service.yaml"));
    assert_eq!(parsed["extent"]["country_code"].as_str(), Some("LAO"));
    assert_eq!(parsed["data_dir"].as_str(), Some("/app/data"));

    // The image pin reaches .env as a commented line, like a model's, and so
    // does the data source section: placeholders, for the operator to fill in.
    let env = sandbox.env();
    assert!(env.contains("\n# OCS_IMAGE_TAG=main\n"), "{env}");
    assert!(
        env.contains(
            "\n# OCS data sources (optional): ERA5-Land needs one or both of ECMWF_DATASTORES_*\n\
             # and EDH_API_KEY, per dataset; WorldPop and CHIRPS3 need none.\n"
        ),
        "{env}"
    );
    assert!(
        env.contains("\n# ECMWF_DATASTORES_URL=https://cds.climate.copernicus.eu/api\n"),
        "{env}"
    );
    for var in [
        "ECMWF_DATASTORES_KEY",
        "EDH_API_KEY",
        "CDSE_S3_ACCESS_KEY",
        "CDSE_S3_SECRET_KEY",
    ] {
        assert!(env.contains(&format!("\n# {var}=\n")), "{var}: {env}");
    }
    assert!(
        !env.contains("\nS3_ACCESS_KEY="),
        "no store, no credentials"
    );
}

#[test]
fn the_ocs_flags_fill_the_scaffold_instead_of_the_example() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&[
            "--models",
            "none",
            "--with",
            "ocs",
            "--ocs-name",
            "Malawi",
            "--ocs-country",
            "mwi",
            "--ocs-bbox",
            "32.6,-17.2,35.9,-9.3",
        ])
        .assert()
        .success();

    let path = dir.join("ocs/climate-service.yaml");
    let config = yaml(&path);
    assert_eq!(config["id"].as_str(), Some("malawi-climate-service"));
    assert_eq!(config["name"].as_str(), Some("Malawi Climate Service"));
    assert_eq!(config["extent"]["name"].as_str(), Some("Malawi"));
    assert_eq!(config["extent"]["country_code"].as_str(), Some("MWI"));
    assert_eq!(config["extent"]["bbox"][0].as_f64(), Some(32.6));
    assert!(
        !read(&path).contains("Laos example values"),
        "these are the operator's own values"
    );
}

#[test]
fn enabling_the_object_store_adds_its_file_its_secrets_and_the_ocs_variables() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--with", "ocs"])
        .assert()
        .success();

    sandbox
        .components(&["enable", "s3", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains("enabled s3"))
        .stdout(predicates::str::contains("hint: wrote compose.s3.yml"));

    assert!(dir.join("compose.s3.yml").is_file());
    let s3 = yaml(&dir.join("compose.s3.yml"));
    let svc = &s3["services"]["s3"];
    assert_eq!(
        svc["environment"]["RUSTFS_ACCESS_KEY"].as_str(),
        Some("${S3_ACCESS_KEY:-}")
    );
    assert_eq!(svc["expose"][0].as_str(), Some("9000"));
    assert!(svc.get("ports").is_none(), "internal by default");
    assert!(s3["services"]["s3-init"].is_mapping());

    // OCS is re-rendered with the (forward-looking) S3 variables.
    let ocs = yaml(&dir.join("compose.ocs.yml"));
    let env = &ocs["services"]["ocs"]["environment"];
    assert_eq!(env["S3_ENDPOINT"].as_str(), Some("http://s3:9000"));
    assert_eq!(env["S3_BUCKET"].as_str(), Some("ocs"));

    // The credentials are generated once and land in .env, nowhere else.
    let body = sandbox.env();
    let access = env_value(&body, "S3_ACCESS_KEY").expect("an access key");
    let secret = env_value(&body, "S3_SECRET_KEY").expect("a secret key");
    assert_eq!(access.len(), 32);
    assert_eq!(secret.len(), 32);
    assert_ne!(access, secret);
    assert!(body.contains("\n# S3_IMAGE_TAG=latest\n"), "{body}");
    assert!(
        !read(&dir.join("compose.s3.yml")).contains(access),
        "the compose file substitutes them, it does not hold them"
    );

    // A second enable changes nothing, credentials least of all.
    sandbox.components(&["enable", "s3"]).assert().success();
    assert_eq!(env_value(&sandbox.env(), "S3_ACCESS_KEY"), Some(access));

    assert_eq!(
        state(&dir)["compose_files"],
        serde_json::json!([
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.ocs.yml",
            "compose.s3.yml",
            "compose.marketplace.yml"
        ])
    );
}

#[test]
fn a_component_port_is_recorded_and_published() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--with", "ocs,s3"])
        .assert()
        .success();

    sandbox
        .components(&["enable", "ocs", "--port", "9010"])
        .assert()
        .success()
        .stdout(predicates::str::contains("http://localhost:9010"));
    assert_eq!(
        yaml(&dir.join("compose.ocs.yml"))["services"]["ocs"]["ports"][0].as_str(),
        Some("9010:9000")
    );

    sandbox
        .components(&["enable", "s3", "--port", "9002"])
        .assert()
        .success();
    assert_eq!(
        yaml(&dir.join("compose.s3.yml"))["services"]["s3"]["ports"][0].as_str(),
        Some("9002:9000")
    );

    let listed = sandbox.components(&["list"]).assert().success();
    let text = String::from_utf8_lossy(&listed.get_output().stdout).into_owned();
    assert!(text.contains("http://localhost:9010"), "{text}");
    assert!(text.contains("http://localhost:9002"), "{text}");
}

/// A bare `chap-devops open` answers the question it is asking - what is there to
/// open - rather than printing a usage error, and it says something about every
/// component, including the ones it cannot open.
#[test]
fn a_bare_open_lists_what_there_is_to_open() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "none", "--with", "ocs,s3"])
        .assert()
        .success();

    let listed = sandbox.open(&[]).assert().success();
    let text = String::from_utf8_lossy(&listed.get_output().stdout).into_owned();
    assert!(
        text.contains("COMPONENT") && text.contains("OPENS"),
        "{text}"
    );
    // chap-core's page is its API documentation, not the origin.
    assert!(text.contains("http://localhost:8700/docs"), "{text}");
    assert!(text.contains("http://localhost:8790"), "{text}");
    assert!(text.contains("no web interface to open"), "{text}");
    assert!(
        text.contains("not a component of this deployment"),
        "{text}"
    );
    assert!(text.contains("run `chap-devops open NAME`"), "{text}");
}

/// A deployment of models alone has no component to open, but each model
/// that publishes a host port has its API documentation there.
#[test]
fn a_bare_open_lists_the_models_that_publish_a_host_port() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--only", "none", "--models", "chapkit_ewars_model"])
        .assert()
        .success();
    let listed = sandbox.open(&[]).assert().success();
    let text = String::from_utf8_lossy(&listed.get_output().stdout).into_owned();
    assert!(text.contains("MODEL"), "{text}");
    assert!(
        text.contains("chapkit_ewars_model") && text.contains("/docs"),
        "{text}"
    );
    assert!(text.contains("1 of them can be opened"), "{text}");
    assert!(!text.contains("nothing in this deployment"), "{text}");
}

/// The three answers that open nothing are refusals with the way out on the
/// same line, and none of them hands a browser an address this deployment has
/// nobody on.
#[test]
fn open_refuses_a_component_that_is_off_and_the_object_store() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "none", "--with", "s3"])
        .assert()
        .success();

    // Off: the deployment does not have it at all.
    sandbox
        .open(&["dhis2"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "dhis2 is not a component of this deployment, so there is nothing to open",
        ))
        .stderr(predicates::str::contains(
            "run `chap-devops components enable dhis2`",
        ));

    // On, publishing nothing, and still never opened: the object store speaks
    // the S3 API and has no interface to show.
    sandbox
        .open(&["s3"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("serves no web interface"))
        .stderr(predicates::str::contains(
            "`chap-devops components enable s3 --port N`",
        ));

    // And a name that is not a component at all is the same refusal every
    // other component command gives.
    sandbox
        .open(&["nope"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("unknown component `nope`"));
}

/// A component with no host port is reached inside the deployment, so the
/// refusal names that address and the command that publishes one.
#[test]
fn open_names_the_internal_address_of_a_component_with_no_host_port() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "none", "--with", "ocs", "--ocs-port", "none"])
        .assert()
        .success();

    sandbox
        .open(&["ocs"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("ocs publishes no host port"))
        .stderr(predicates::str::contains("http://ocs:9000"))
        .stderr(predicates::str::contains(
            "run `chap-devops components enable ocs --port N`",
        ));

    // The listing says the same thing in its own column, so the two cannot
    // drift apart.
    let listed = sandbox.open(&[]).assert().success();
    let text = String::from_utf8_lossy(&listed.get_output().stdout).into_owned();
    assert!(
        text.contains("reached at http://ocs:9000 inside the deployment"),
        "{text}"
    );
}

/// `--no-browser`, and `--json` without it, give the address and never hand it to
/// a browser. PATH is emptied so a regression finds no opener to launch.
#[test]
fn open_no_browser_and_json_give_the_address_without_a_browser() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "none", "--with", "ocs", "--ocs-port", "18790"])
        .assert()
        .success();

    let printed = sandbox
        .open(&["ocs", "--no-browser"])
        .env("PATH", "")
        .assert()
        .success();
    let text = String::from_utf8_lossy(&printed.get_output().stdout).into_owned();
    assert!(
        text.starts_with("the OCS web interface is at http://localhost:18790\n"),
        "{text}"
    );
    assert!(!text.contains("to open it with"), "{text}");

    let report = json_of(sandbox.open(&["ocs", "--json"]).env("PATH", ""));
    assert_eq!(report["url"], "http://localhost:18790");
    assert_eq!(report["no_browser"], true);
    assert_eq!(report["opened"], false);
}

/// `open` needs a deployment, so outside one it says which file is missing
/// rather than opening anything.
#[test]
fn open_outside_a_deployment_says_there_is_none() {
    let sandbox = Sandbox::new();
    sandbox
        .chap()
        .arg("open")
        .arg("ocs")
        .assert()
        .failure()
        .stderr(predicates::str::contains("not a chap-devops deployment"));
}

/// A western extent starts with a minus sign, and the spelling without `=` is
/// the one anyone types first.
#[test]
fn init_takes_a_negative_bbox_without_the_equals_sign() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&[
            "--models",
            "none",
            "--with",
            "ocs",
            "--ocs-name",
            "Sierra Leone",
            "--ocs-country",
            "SLE",
            "--ocs-bbox",
            "-13.5,6.9,-10.1,10.0",
        ])
        .assert()
        .success();

    let config = yaml(&dir.join("ocs/climate-service.yaml"));
    assert_eq!(config["extent"]["bbox"][0].as_f64(), Some(-13.5));
    assert_eq!(config["extent"]["bbox"][2].as_f64(), Some(-10.1));
    assert!(
        !read(&dir.join("ocs/climate-service.yaml")).contains("Laos example values"),
        "these are the operator's own values, even where they match the example"
    );
}

/// The reverse-proxy shape, set at init and then changed: no host port, a base
/// URL in its place, and ingestion over HTTP refused.
#[test]
fn ocs_can_be_put_behind_a_proxy_and_made_read_only() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&[
            "--models",
            "none",
            "--with",
            "ocs",
            "--ocs-base-url",
            "https://ocs.example.org",
        ])
        .assert()
        .success();
    assert_eq!(
        yaml(&dir.join("compose.ocs.yml"))["services"]["ocs"]["environment"]
            ["CLIMATE_SERVICE_BASE_URL"]
            .as_str(),
        Some("${CLIMATE_SERVICE_BASE_URL:-https://ocs.example.org}")
    );

    sandbox
        .components(&[
            "enable",
            "ocs",
            "--port",
            "none",
            "--base-url",
            "https://climate.example.org/",
            "--read-only",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "reached through the proxy at https://climate.example.org",
        ))
        .stdout(predicates::str::contains("read_only: true"))
        // Not a plain `chap-devops restart`: the config is a bind mount, so compose
        // compares nothing that changed and recreates nothing.
        .stdout(predicates::str::contains(
            "`chap-devops restart ocs` applies it",
        ));

    // The overlay: exposed on the compose network, published nowhere.
    let svc = yaml(&dir.join("compose.ocs.yml"))["services"]["ocs"].clone();
    assert_eq!(svc["expose"][0].as_str(), Some("9000"));
    assert!(svc.get("ports").is_none(), "nothing is bound on the host");
    assert_eq!(
        svc["environment"]["CLIMATE_SERVICE_BASE_URL"].as_str(),
        Some("${CLIMATE_SERVICE_BASE_URL:-https://climate.example.org}"),
        "the trailing slash goes: OCS appends a path to this"
    );

    // The instance config: one key added, the rest of the file untouched.
    let config = read(&dir.join("ocs/climate-service.yaml"));
    assert!(config.ends_with("read_only: true\n"), "{config}");
    assert!(config.contains("laos-climate-service"), "{config}");

    // And both facts in the record and in `status --json`.
    let components = yaml(&dir.join(".chap-devops/components.yaml"));
    assert_eq!(components["ocs"]["port"], Yaml::Null);
    assert_eq!(
        components["ocs"]["base_url"].as_str(),
        Some("https://climate.example.org")
    );
    assert_eq!(components["ocs"]["read_only"], Yaml::Bool(true));

    let status = json_of(
        sandbox
            .chap()
            .arg("-C")
            .arg(&dir)
            .args(["status", "--json"]),
    );
    let ocs = &status["components"][0];
    assert_eq!(ocs["name"].as_str(), Some("ocs"));
    assert_eq!(
        ocs["reach"].as_str(),
        Some("internal (proxy: https://climate.example.org)")
    );
    assert_eq!(ocs["read_only"], serde_json::json!(true));
    assert_eq!(ocs["health_url"], serde_json::Value::Null);

    // --read-write puts it back, editing the one key in place. The read mode
    // is all that changed, so the closing line names the restart that
    // applies it, and `chap-devops up`, which would not, is not named.
    sandbox
        .components(&["enable", "ocs", "--read-write"])
        .assert()
        .success()
        .stdout(predicates::str::starts_with("ocs is now read-write\n"))
        .stdout(predicates::str::contains(
            "run `chap-devops restart ocs` to apply",
        ))
        .stdout(predicates::str::contains("chap-devops up").not());
    // Again, and there is nothing to apply.
    sandbox
        .components(&["enable", "ocs", "--read-write"])
        .assert()
        .success()
        .stdout(predicates::str::starts_with("ocs is already read-write\n"))
        .stdout(predicates::str::contains("chap-devops restart").not());
    assert_eq!(
        read(&dir.join("ocs/climate-service.yaml")),
        config.replace("read_only: true", "read_only: false")
    );
    assert_eq!(
        yaml(&dir.join(".chap-devops/components.yaml"))["ocs"]["read_only"],
        Yaml::Bool(false)
    );

    // A port again, and the address is the address: it is where this machine
    // reaches it, whatever the proxy is called.
    sandbox
        .components(&["enable", "ocs", "--port", "9010"])
        .assert()
        .success()
        .stdout(predicates::str::contains("http://localhost:9010"));
    assert_eq!(
        yaml(&dir.join("compose.ocs.yml"))["services"]["ocs"]["ports"][0].as_str(),
        Some("9010:9000")
    );
}

/// The directory is the whole declaration: a project that has one gets the
/// mount and the `plugins_dir` key, and one that does not gets neither.
#[test]
fn an_ocs_plugin_directory_is_mounted_and_configured_by_sync() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&[
            "--models",
            "none",
            "--with",
            "ocs",
            "--ocs-name",
            "Malawi",
            "--ocs-country",
            "MWI",
        ])
        .assert()
        .success();
    assert!(!read(&dir.join("compose.ocs.yml")).contains("/app/plugins"));
    assert!(!read(&dir.join("ocs/climate-service.yaml")).contains("\nplugins_dir:"));

    std::fs::create_dir_all(dir.join("ocs/plugins/datasets")).unwrap();
    std::fs::write(dir.join("ocs/plugins/datasets/clms_gpp.py"), "# a plugin\n").unwrap();
    sandbox
        .chap()
        .arg("-C")
        .arg(&dir)
        .arg("sync")
        .assert()
        .success();

    assert_eq!(
        yaml(&dir.join("compose.ocs.yml"))["services"]["ocs"]["volumes"][1].as_str(),
        Some("./ocs/plugins:/app/plugins:ro")
    );
    let config = read(&dir.join("ocs/climate-service.yaml"));
    assert!(config.ends_with("plugins_dir: /app/plugins\n"), "{config}");
    assert!(config.contains("malawi-climate-service"), "{config}");

    // `doctor` reports the count, and it is not a problem.
    let checks = json_of(
        sandbox
            .chap()
            .arg("-C")
            .arg(&dir)
            .args(["doctor", "--json"]),
    );
    let line = checks["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "components")
        .expect("a components line")
        .clone();
    assert_eq!(line["status"].as_str(), Some("ok"));
    assert!(
        line["detail"]
            .as_str()
            .unwrap()
            .ends_with("plugins/: 1 file"),
        "{line}"
    );
    assert!(
        line["detail"]
            .as_str()
            .unwrap()
            .contains("ERA5-Land: credentials unset (WorldPop and CHIRPS3 work without them)"),
        "{line}"
    );

    // A second sync changes nothing.
    sandbox
        .chap()
        .arg("-C")
        .arg(&dir)
        .args(["sync", "--check"])
        .assert()
        .success();
}

#[test]
fn disabling_a_component_removes_its_file_and_its_place_in_the_f_list() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--with", "ocs,s3"])
        .assert()
        .success();

    sandbox
        .components(&["disable", "ocs", "-v"])
        .assert()
        .success()
        .stdout(predicates::str::contains("disabled ocs"))
        .stdout(predicates::str::contains("hint: removed compose.ocs.yml"));

    assert!(!dir.join("compose.ocs.yml").exists());
    assert!(
        dir.join("ocs/climate-service.yaml").is_file(),
        "the operator's own config is never deleted"
    );
    assert_eq!(
        state(&dir)["compose_files"],
        serde_json::json!([
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.s3.yml",
            "compose.marketplace.yml"
        ])
    );
    assert_eq!(
        yaml(&dir.join(".chap-devops/components.yaml"))["ocs"]["enabled"],
        Yaml::Bool(false)
    );

    // Enabling it again restores the file and keeps the config that is there.
    std::fs::write(dir.join("ocs/climate-service.yaml"), "id: mine\n").unwrap();
    sandbox.components(&["enable", "ocs"]).assert().success();
    assert!(dir.join("compose.ocs.yml").is_file());
    assert_eq!(read(&dir.join("ocs/climate-service.yaml")), "id: mine\n");
}

#[test]
fn chap_core_can_be_disabled_under_an_enabled_model() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();

    // The model stays and runs on its own; the chap-core files go.
    sandbox
        .components(&["disable", "chap-core"])
        .assert()
        .success();
    assert!(!sandbox.project().join("compose.yml").exists());
    assert!(!sandbox.project().join("compose.chap-devops.yml").exists());
    assert!(
        sandbox
            .project()
            .join("compose.chapkit-ewars-model.yml")
            .is_file()
    );
}

#[test]
fn a_standalone_ocs_deployment_leaves_chap_core_out() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--with", "ocs", "--without", "chap-core"])
        .assert()
        .success()
        .stdout(predicates::str::contains("components: ocs"))
        .stdout(predicates::str::contains("OCS:"))
        .stdout(predicates::str::contains("API:").not())
        // No chap-core, so no chap-core release is looked up, not even to
        // say that --offline could not.
        .stderr(predicates::str::contains("chap-core release").not());

    assert!(!dir.join("compose.yml").exists());
    assert!(!dir.join("compose.chap-devops.yml").exists());
    assert!(dir.join("compose.ocs.yml").is_file());
    assert_eq!(
        state(&dir)["compose_files"],
        serde_json::json!(["compose.ocs.yml", "compose.marketplace.yml"])
    );

    // The commands only chap-core can answer say so, and name the way back.
    let in_project = |args: &[&str]| {
        let mut cmd = sandbox.chap();
        cmd.arg("-C").arg(sandbox.project()).args(args);
        cmd
    };
    for args in [
        &["jobs"][..],
        &["api", "GET", "/health"],
        &["models", "test", "--all", "--backtest"],
        &["update", "--chap-tag", "v1.0.0"],
    ] {
        in_project(args)
            .assert()
            .failure()
            .stderr(predicates::str::contains(
                "this deployment has no chap-core",
            ))
            .stderr(predicates::str::contains(
                "`chap-devops components enable chap-core`",
            ));
    }
    // A backup has no database to dump, and says why rather than blaming a
    // flag. That is a hint, so it shows with `-v`.
    in_project(&["-v", "backup", "create", "--no-models", "--no-components"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "this deployment has no chap-core",
        ));

    // The model level runs inside the model's container, so it is not
    // refused for want of chap-core: here it only finds no model to test.
    in_project(&["models", "test", "--all"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "this deployment enables no models",
        ))
        .stderr(predicates::str::contains("no chap-core").not());

    // A model asked for at the same time runs on its own, on a host port:
    // under a port base of its own, clear of the default 5001 range.
    let other = Sandbox::new();
    other
        .init(&[
            "--only",
            "ocs",
            "--models",
            "chapkit_ewars_model",
            "--port-base",
            "18140",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("http://localhost:18140"));
    assert!(!other.project().join("compose.yml").exists());
}

#[test]
fn only_is_the_whole_component_set() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--only", "dhis2", "--dhis2-seed", "none"])
        .assert()
        .success()
        .stdout(predicates::str::contains("components: dhis2"))
        .stdout(predicates::str::contains("API:").not());
    assert!(!dir.join("compose.yml").exists());
    assert!(dir.join("compose.dhis2.yml").is_file());

    // --only replaces --with and --without rather than mixing with them.
    Sandbox::new()
        .init(&["--only", "ocs", "--with", "s3"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--only"));

    // No component at all is a deployment to add to later.
    let empty = Sandbox::new();
    empty.init(&["--only", "none"]).assert().success();
    assert_eq!(
        state(&empty.project())["compose_files"],
        serde_json::json!(["compose.marketplace.yml"])
    );
}

/// A local image that is not in the local store is refused with the build
/// command, before anything is written.
#[test]
fn a_local_image_that_was_never_built_is_refused_with_the_build_command() {
    let sandbox = Sandbox::new();
    sandbox.init(&["--models", "none"]).assert().success();
    sandbox
        .models(&["add", "chap-devops-test-never-built:dev"])
        .env("PATH", "")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "docker build --platform linux/amd64 -t chap-devops-test-never-built:dev .",
        ));
    let manual = std::fs::read_to_string(sandbox.project().join(".chap-devops/models-manual.yaml"))
        .unwrap_or_default();
    assert!(!manual.contains("never_built"), "nothing recorded");
}

/// `--source` builds chap-core from a checkout: compose.yml comes from the
/// checkout's own compose.ghcr.yml, compose.chap-devops.yml builds chap and worker,
/// and `update` has no pin to move.
#[test]
fn init_source_builds_chap_core_from_a_checkout() {
    let sandbox = Sandbox::new();
    let checkout = sandbox.home.path().join("chap-core");
    std::fs::create_dir_all(&checkout).unwrap();

    // Not a checkout yet: refused, naming what is missing, before anything is written.
    sandbox
        .init(&["--source", checkout.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("is not a chap-core checkout"))
        .stderr(predicates::str::contains("Dockerfile.worker"));
    assert!(!sandbox.project().exists());

    std::fs::write(checkout.join("Dockerfile"), "FROM scratch\n").unwrap();
    std::fs::write(checkout.join("Dockerfile.worker"), "FROM scratch\n").unwrap();
    std::fs::write(
        checkout.join("compose.ghcr.yml"),
        "services:\n  chap:\n    image: ghcr.io/dhis2-chap/chap-core:latest\n  \
         worker:\n    image: ghcr.io/dhis2-chap/chap-worker:latest\n",
    )
    .unwrap();
    sandbox
        .init(&[
            "-v",
            "--source",
            checkout.to_str().unwrap(),
            "--models",
            "none",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("the chap-core checkout at"));
    let dir = sandbox.project();
    assert!(read(&dir.join("compose.yml")).contains("ghcr.io/dhis2-chap/chap-worker:latest"));
    let chap_devops = read(&dir.join("compose.chap-devops.yml"));
    assert!(
        chap_devops.contains("dockerfile: Dockerfile.worker"),
        "{chap_devops}"
    );
    assert!(chap_devops.contains("pull_policy: build"), "{chap_devops}");
    // The API image is built for this machine, so it runs without
    // emulation. The worker's base image is amd64 only.
    let native = match std::env::consts::ARCH {
        "aarch64" => "linux/arm64",
        _ => "linux/amd64",
    };
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&chap_devops).expect("YAML");
    assert_eq!(doc["services"]["chap"]["platform"].as_str(), Some(native));
    assert!(
        doc["services"]["worker"].get("platform").is_none(),
        "{chap_devops}"
    );

    let mut update = sandbox.chap();
    update
        .arg("-C")
        .arg(&dir)
        .args(["update", "--chap-tag", "v1.0.0"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "builds chap-core from the checkout",
        ));
}

#[test]
fn an_unknown_component_name_is_reported_before_anything_is_written() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--with", "nope"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("unknown component `nope`"));
    assert!(!sandbox.project().exists());
}

#[test]
fn status_reports_every_enabled_component() {
    let sandbox = Sandbox::new();
    // Ports of this test's own, never the defaults: `status` really calls
    // `/health`, and OCS's default 9000 is a port a developer running an OCS
    // of their own would answer on.
    let api_port = free_port().to_string();
    let ocs_port = free_port();
    sandbox
        .init(&["--models", "none", "--api-port", &api_port])
        .assert()
        .success();
    // Enabled here rather than with `init --with`, because this is where the
    // host port can be named, and init would probe 9000 on the way.
    sandbox
        .components(&["enable", "ocs", "--port", &ocs_port.to_string()])
        .assert()
        .success();
    sandbox.components(&["enable", "s3"]).assert().success();

    // Nothing is running, so this exits non-zero; the document is the point.
    let out = sandbox
        .chap()
        .arg("-C")
        .arg(sandbox.project())
        .args(["status", "--json", "--timeout", "1"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let report: Json = serde_json::from_slice(&out).expect("status --json is one document");
    let components = report["components"].as_array().expect("a component list");
    assert_eq!(components.len(), 2);
    assert_eq!(components[0]["name"], "ocs");
    assert_eq!(
        components[0]["reach"],
        format!("http://localhost:{ocs_port}")
    );
    assert_eq!(
        components[0]["health_url"],
        format!("http://localhost:{ocs_port}/health")
    );
    // The port was free when this test took it and nothing bound it since.
    assert_eq!(components[0]["state"], "not-running");
    assert_eq!(components[1]["name"], "s3");
    assert_eq!(components[1]["reach"], "internal");
    assert_eq!(components[1]["health_url"], Json::Null);
}

/// The other half of the same report: the container this deployment owns is
/// what decides, not whoever holds its host port. Something else answering
/// there - most realistically another deployment's OCS on the same port - used
/// to be reported as this one being `up`, and cost every `chap-devops status` the
/// requests to find out.
#[test]
fn status_does_not_call_a_component_up_because_something_else_answers_its_port() {
    let sandbox = Sandbox::new();
    let api_port = free_port().to_string();
    // A stand-in OCS on a port of its own, answering for as long as this test
    // runs.
    let ocs_port = ocs_lookalike();
    sandbox
        .init(&["--models", "none", "--api-port", &api_port])
        .assert()
        .success();
    sandbox
        .components(&["enable", "ocs", "--port", &ocs_port.to_string()])
        .assert()
        .success();

    let out = sandbox
        .chap()
        .arg("-C")
        .arg(sandbox.project())
        .args(["status", "--json", "--timeout", "5"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let report: Json = serde_json::from_slice(&out).expect("status --json is one document");
    let components = report["components"].as_array().expect("a component list");
    assert_eq!(components.len(), 1, "{report}");
    assert_eq!(components[0]["name"], "ocs");
    // The address is recorded state, so it is reported either way; the state is
    // the container's, and this deployment has none running.
    assert_eq!(
        components[0]["health_url"],
        format!("http://localhost:{ocs_port}/health")
    );
    assert_eq!(components[0]["state"], "not-running", "{report}");
    // Nothing was asked, so the dataset list that server would have answered
    // with is not counted onto this deployment's line either.
    assert_eq!(components[0]["datasets"], Json::Null, "{report}");
}

#[test]
fn doctor_checks_the_components_and_the_files_they_add() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--with", "ocs"])
        .assert()
        .success();

    let out = sandbox
        .chap()
        .arg("-C")
        .arg(&dir)
        .arg("doctor")
        .assert()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out).into_owned();
    // The scaffold is still the example, which is a warning with a fix.
    assert!(
        text.contains("components") && text.contains("still holds OCS's example values"),
        "{text}"
    );
    assert!(text.contains("port ocs"), "{text}");
    assert!(text.contains("deployment files"), "{text}");

    // Editing the config and deleting the note turns the line green.
    std::fs::write(
        dir.join("ocs/climate-service.yaml"),
        "id: mine\nname: Mine\nextent:\n  name: Mine\n  bbox: [0, 0, 1, 1]\n  \
         country_code: MWI\ndata_dir: /app/data\n",
    )
    .unwrap();
    let out = sandbox
        .chap()
        .arg("-C")
        .arg(&dir)
        .arg("doctor")
        .assert()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out).into_owned();
    assert!(
        text.contains("chap-core, ocs; ocs/climate-service.yaml present"),
        "{text}"
    );
}

#[test]
fn docker_accepts_a_deployment_with_both_components() {
    if !docker_ready() {
        return;
    }
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--with", "ocs,s3", "--api-port", "8123"])
        .assert()
        .success();

    let out = std::process::Command::new("docker")
        .args([
            "compose",
            "-f",
            "compose.yml",
            "-f",
            "compose.chap-devops.yml",
        ])
        .args(["-f", "compose.ocs.yml", "-f", "compose.s3.yml"])
        .args(["-f", "compose.marketplace.yml", "config"])
        .current_dir(&dir)
        .output()
        .expect("docker compose config runs");
    assert!(
        out.status.success(),
        "docker compose config failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let merged: Yaml = serde_yaml_ng::from_slice(&out.stdout).expect("config is YAML");
    let ocs = &merged["services"]["ocs"];
    assert_eq!(ocs["ports"][0]["published"].as_str(), Some("8790"));
    assert_eq!(ocs["ports"][0]["target"].as_u64(), Some(9000));
    // The .env values reached the merged document rather than an empty string.
    let key = ocs["environment"]["S3_ACCESS_KEY"].as_str().unwrap();
    assert_eq!(key.len(), 32, "compose substituted S3_ACCESS_KEY");
    assert_eq!(
        merged["services"]["s3"]["environment"]["RUSTFS_ACCESS_KEY"].as_str(),
        Some(key)
    );
    assert!(
        merged["services"]["s3"].get("ports").is_none(),
        "the store is internal"
    );
    assert_eq!(
        merged["services"]["s3-init"]["restart"].as_str(),
        Some("no")
    );
}

/// A chap-core elsewhere that is a container on this machine changes where
/// it calls the models back, so `components enable chap-core --url` says it
/// as a warning, the way `chap-devops init` does.
#[cfg(unix)]
#[test]
fn enable_chap_core_url_warns_when_the_chap_core_is_a_container() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "none", "--only", "none"])
        .assert()
        .success();
    let dir = sandbox.project();
    let temp = tempfile::tempdir().expect("a directory for the fake docker");
    let docker = temp.path().join("docker");
    std::fs::write(
        &docker,
        "#!/bin/sh\ncase \"$*\" in\n  *publish=*) echo own-68575e-chap-1 ;;\nesac\nexit 0\n",
    )
    .expect("the fake docker");
    std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755))
        .expect("an executable fake docker");

    let port = free_port();
    let url = format!("http://localhost:{port}");
    chap_with_docker(
        &sandbox,
        &dir,
        temp.path(),
        &["components", "enable", "chap-core", "--url", &url],
    )
    .assert()
    .success()
    .stderr(predicates::str::contains(format!(
        "warning: chap-core at {url} is the container `own-68575e-chap-1`"
    )));
}

/// chap-core publishes the API port, so its enable line names it the way the
/// line of every other component names its own.
#[test]
fn enable_chap_core_names_the_api_port() {
    let sandbox = Sandbox::new();
    let port = free_port();
    sandbox
        .init(&[
            "--models",
            "none",
            "--only",
            "none",
            "--api-port",
            &port.to_string(),
        ])
        .assert()
        .success();
    let dir = sandbox.project();
    let output = chap_in(&sandbox, &dir, &["-v", "components", "enable", "chap-core"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.starts_with(&format!("enabled chap-core on http://localhost:{port}\n")),
        "{text}"
    );
    assert!(!text.contains("publishes no host port"), "{text}");
}

/// The volumes of chap-core are declared in `compose.yml`, so after a disable
/// `chap-devops down --volumes` does not reach them: the hint names the way that
/// does.
#[test]
fn disable_chap_core_names_a_way_to_remove_its_volumes() {
    let sandbox = Sandbox::new();
    sandbox.init(&["--models", "none"]).assert().success();
    let dir = sandbox.project();
    let output = chap_in(
        &sandbox,
        &dir,
        &["-v", "components", "disable", "chap-core"],
    )
    .assert()
    .success()
    .get_output()
    .clone();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains(
            "hint: kept the volumes of chap-core; to remove them, run `chap-devops components enable \
             chap-core`, then `chap-devops down --volumes`"
        ),
        "{text}"
    );
    assert!(!text.contains("left alone"), "{text}");
}

/// chap-devops supports DHIS2 2.41 and newer: an older tag is refused, and the
/// deployment keeps the tag it had.
#[test]
fn components_enable_dhis2_refuses_a_tag_older_than_chap_devops_supports() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&[
            "--models",
            "none",
            "--with",
            "dhis2",
            "--dhis2-seed",
            "none",
        ])
        .assert()
        .success();
    sandbox
        .components(&["enable", "dhis2", "--tag", "2.40"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "--tag 2.40 is older than DHIS2 2.41, the oldest version chap-devops supports; use \
             `--tag 2.41` or newer",
        ));
    let state = read(&sandbox.project().join(".chap-devops/components.yaml"));
    assert!(!state.contains("2.40"), "{state}");
    sandbox
        .init(&[
            "--force",
            "--models",
            "none",
            "--with",
            "dhis2",
            "--dhis2-tag",
            "2.39",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "--dhis2-tag 2.39 is older than DHIS2 2.41",
        ));
}
