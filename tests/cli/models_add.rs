use crate::common::*;
use predicates::prelude::PredicateBooleanExt;
use serde_json::Value as Json;
use std::path::Path;

/// `.chap-devops/models-manual.yaml` as JSON, or `Json::Null` when the deployment
/// has added no model of its own.
fn manual_models(dir: &Path) -> Json {
    let path = dir.join(".chap-devops").join("models-manual.yaml");
    if !path.is_file() {
        return Json::Null;
    }
    serde_json::to_value(yaml(&path)).expect("models-manual.yaml maps to JSON")
}

/// The repository `models add` is pointed at.
const REPO_URL: &str = "https://github.com/example/chapkit_example_manual_model";

/// The image that repository publishes to.
const IMAGE: &str = "ghcr.io/example/chapkit_example_manual_model";

/// `models enable` reads the user off the image's own config, not off the
/// table compiled into the binary.
#[test]
fn enable_reads_the_user_from_the_image_config() {
    let (sandbox, dir, port) = added_sandbox(
        Hub::new()
            .publishing(MARKETPLACE_TAG)
            .running_as("root")
            .working_in("/work"),
    );
    sandbox
        .online(port)
        .args(["models", "enable", "chapkit_ewars_model"])
        .assert()
        .success()
        .stdout(predicates::str::contains("enabled chapkit_ewars_model"))
        // Nothing was guessed at, so nothing is reported as guessed at.
        .stdout(predicates::str::contains("built-in table").not());

    let model = &state(&dir)["models"]["chapkit_ewars_model"];
    assert_eq!(model["user"], "root", "the image config says root");
    assert_eq!(model["user_from"], "image-config");
    assert_eq!(model["data_dir"], "/work/data", "from its WorkingDir");

    // The overlay overrides nothing on a root image, and hands its volume to
    // root: docker creates a fresh one root-owned already, but a volume
    // carried over from a deployment that ran this model as an account is
    // owned by that account, and root cannot write to it with every
    // capability dropped.
    let overlay = yaml(&dir.join("compose.chapkit-ewars-model.yml"));
    assert!(
        overlay["services"]["chapkit-ewars-model"]
            .get("user")
            .is_none(),
        "the model service overrides nothing"
    );
    assert_eq!(
        overlay["services"]["chapkit-ewars-model-init"]["command"][2].as_str(),
        Some("chown -R 0:0 /work/data")
    );

    // `models info` says where the answer came from.
    sandbox
        .models(&["info", "chapkit_ewars_model"])
        .assert()
        .success()
        .stdout(predicates::str::contains("user      root  (image config)"));

    // An image that runs as an account gets the numeric pair in both places.
    let (sandbox, dir, port) = added_sandbox(
        Hub::new()
            .publishing(MARKETPLACE_TAG)
            .running_as("chapkit")
            .working_in("/app"),
    );
    sandbox
        .online(port)
        .args(["models", "enable", "chapkit_ewars_model"])
        .assert()
        .success();
    let model = &state(&dir)["models"]["chapkit_ewars_model"];
    assert_eq!(model["user"], "1000:1000");
    assert_eq!(model["user_from"], "image-config");
    assert_eq!(model["data_dir"], "/app/data");
    let overlay = read(&dir.join("compose.chapkit-ewars-model.yml"));
    assert!(overlay.contains("    user: 1000:1000\n"), "{overlay}");
    assert!(
        overlay.contains("chown -R 1000:1000 /app/data"),
        "{overlay}"
    );
}

/// An `--offline` enable has no image to read, and says which answer it fell
/// back to rather than passing the table off as the image's word.
#[test]
fn an_offline_enable_falls_back_to_the_table_and_says_so() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--api-port", &free_port().to_string()])
        .assert()
        .success();

    sandbox
        .models(&["enable", "auto_arima_chapkit"])
        .assert()
        .success()
        .stderr(predicates::str::contains(format!(
            "warning: nothing could say what \
             ghcr.io/chap-models/auto_arima_chapkit:{} runs as",
            stable_pin("auto_arima_chapkit").1
        )))
        .stderr(predicates::str::contains("the built-in table"))
        .stderr(predicates::str::contains("chap-devops models enable"));

    // The table's own answer for that image: root, which is the whole point.
    let model = &state(&dir)["models"]["auto_arima_chapkit"];
    assert_eq!(model["user"], "root");
    assert_eq!(model["user_from"], "table");
    let overlay = yaml(&dir.join("compose.auto-arima-chapkit.yml"));
    let service = &overlay["services"]["auto-arima-chapkit"];
    assert!(
        service.get("user").is_none(),
        "the model service overrides nothing"
    );
    assert_eq!(
        overlay["services"]["auto-arima-chapkit-init"]["command"][2].as_str(),
        Some("chown -R 0:0 /work/data")
    );
}

/// A public repository whose package ghcr will not hand out: the answer is
/// what to do instead, not the raw 403 from the token endpoint.
#[test]
fn models_add_from_a_repository_without_a_public_image_says_to_build_one() {
    let (sandbox, _dir, port) = added_sandbox(Hub {
        denied: true,
        ..Hub::new()
    });
    sandbox
        .online(port)
        .args(["models", "add", REPO_URL])
        .assert()
        .failure()
        .stderr(predicates::str::contains(format!(
            "{REPO_URL} publishes no public image at"
        )))
        .stderr(predicates::str::contains("HTTP 403"))
        .stderr(predicates::str::contains(
            "`chap-devops models add NAME:dev`",
        ))
        .stderr(predicates::str::contains("/token?").not());
}

/// GitHub answers 404 for a repository that does not exist and for a
/// private one, so the error names both and the way out of each.
#[test]
fn models_add_from_a_repository_github_does_not_know_says_what_404_means() {
    let (sandbox, _dir, port) = added_sandbox(Hub::new());
    sandbox
        .online(port)
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .args(["models", "add", "https://github.com/example/does-not-exist"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "GitHub has no repository example/does-not-exist, or it is private; check the \
             URL, or set GITHUB_TOKEN to a token that can read it if it is private",
        ))
        .stderr(predicates::str::contains("HTTP 404").not());
}

#[test]
fn models_add_from_a_repository_pins_the_newest_published_build() {
    let (sandbox, dir, port) = added_sandbox(Hub::new());
    sandbox
        .online(port)
        .args(["-v", "models", "add", REPO_URL])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "added chapkit_example_manual_model (chapkit-example-manual-model)",
        ))
        // The newest commit has no published build, so the pin lands on the
        // newest build there is.
        .stdout(predicates::str::contains(format!("hint: pin: {OLD_TAG}")))
        .stdout(predicates::str::contains("hint: follows: main"))
        .stdout(predicates::str::contains(
            "hint: data dir: /work/data (from the image config)",
        ))
        .stdout(predicates::str::contains(
            "hint: user: 10001:10001 (from the image config)",
        ))
        .stdout(predicates::str::contains("must register with chap-core as"))
        .stdout(predicates::str::contains("run `chap-devops up` to apply"));

    // The definition, in a file of its own.
    let manual = manual_models(&dir);
    let entry = &manual["chapkit_example_manual_model"];
    assert_eq!(entry["service_id"], "chapkit-example-manual-model");
    assert_eq!(entry["image"], IMAGE);
    assert_eq!(entry["tag"], OLD_TAG);
    assert_eq!(entry["commit"], OLD_SHA);
    assert_eq!(entry["follow"], "main");
    assert_eq!(entry["repository"], REPO_URL);
    assert_eq!(entry["runtime_amd64"], true);
    assert_eq!(entry["added"].as_str().map(str::len), Some(10));

    // And the enablement, in the file every other model uses.
    let enabled = &state(&dir)["models"]["chapkit_example_manual_model"];
    assert_eq!(enabled["image_tag"], OLD_TAG);
    assert_eq!(enabled["version"], OLD_TAG);
    assert_eq!(enabled["channel"], "latest");
    assert_eq!(enabled["host_port"], Json::Null);
    assert_eq!(enabled["data_dir"], "/work/data");
    assert_eq!(enabled["user"], "10001:10001");
    assert_eq!(
        enabled["compose_file"],
        "compose.chapkit-example-manual-model.yml"
    );

    // The overlay is rendered like any other model's, chown and all.
    let overlay = read(&dir.join("compose.chapkit-example-manual-model.yml"));
    assert!(
        overlay.contains(&format!(
            "image: {IMAGE}:${{CHAPKIT_EXAMPLE_MANUAL_MODEL_IMAGE_TAG:-{OLD_TAG}}}"
        )),
        "{overlay}"
    );
    assert!(
        overlay.contains("chown -R 10001:10001 /work/data"),
        "{overlay}"
    );
    assert!(overlay.contains("user: 10001:10001"), "{overlay}");
    assert_eq!(
        includes(&dir),
        vec!["compose.chapkit-example-manual-model.yml"]
    );
    assert!(
        sandbox.env().contains(&format!(
            "# CHAPKIT_EXAMPLE_MANUAL_MODEL_IMAGE_TAG={OLD_TAG}"
        )),
        "{}",
        sandbox.env()
    );
}

#[test]
fn a_manually_added_model_is_marked_in_the_list_and_on_its_page() {
    let (sandbox, dir, port) = added_sandbox(Hub::new());
    sandbox
        .online(port)
        .args(["models", "add", REPO_URL])
        .assert()
        .success();

    // The kind column appears because there is something to tell apart, and
    // the marketplace row keeps its own word for what it is.
    sandbox
        .online(port)
        .args(["models", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("KIND"))
        .stdout(predicates::str::contains("manual"))
        .stdout(predicates::str::contains("chapkit_ewars_model"));

    let out = sandbox
        .online(port)
        .args(["--json", "models", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Json = serde_json::from_slice(&out).expect("models list --json is one document");
    let manual = rows
        .as_array()
        .expect("an array")
        .iter()
        .find(|row| row["id"] == "chapkit_example_manual_model")
        .expect("the added model is listed");
    assert_eq!(manual["manual"], true);
    assert_eq!(manual["enabled"], true);

    // The page says where it came from and what it follows.
    let date = manual_models(&dir)["chapkit_example_manual_model"]["added"]
        .as_str()
        .expect("a date")
        .to_string();
    sandbox
        .online(port)
        .args(["models", "info", "chapkit_example_manual_model"])
        .assert()
        .success()
        .stdout(predicates::str::contains("kind        manual"))
        .stdout(predicates::str::contains(format!(
            "source      manual (`chap-devops models add`, {date})"
        )))
        .stdout(predicates::str::contains("follows     main"))
        .stdout(predicates::str::contains(format!(
            "image       {IMAGE}:{OLD_TAG}"
        )));
}

#[test]
fn update_moves_a_following_manual_model_when_a_newer_build_appears() {
    let (sandbox, _dir, port) = added_sandbox(Hub::new());
    sandbox
        .online(port)
        .args(["models", "add", REPO_URL])
        .assert()
        .success();

    // The same hub: the branch has published nothing new.
    sandbox
        .online(port)
        .args(["update", "--dry-run"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "chapkit_example_manual_model  {OLD_TAG}  unchanged"
        )))
        .stdout(predicates::str::contains("already up to date"));

    // And once the newer commit is published, the pin would move to it.
    let advanced = Hub::new().advanced().start();
    sandbox
        .online(advanced)
        .args(["update", "--dry-run"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "chapkit_example_manual_model  {OLD_TAG} -> {NEW_TAG}"
        )))
        .stdout(predicates::str::contains("would update 1 model pin"));

    // A repository that will not answer leaves the pin alone and says that
    // nothing was established, rather than that nothing moved. The registry
    // itself still answers: `update` has no fallback for that one.
    sandbox
        .online(port)
        .env(
            "CHAP_DEVOPS_GITHUB_API",
            format!("http://127.0.0.1:{}", free_port()),
        )
        .args(["update", "--dry-run"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "chapkit_example_manual_model  {OLD_TAG}  unchanged (could not check)"
        )))
        .stderr(predicates::str::contains("could not check"));
}

#[test]
fn models_add_from_an_image_reference_is_pinned() {
    let (sandbox, dir, port) = added_sandbox(Hub::new());
    sandbox
        .online(port)
        .args(["-v", "models", "add", &format!("{IMAGE}:{NEW_TAG}")])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!("hint: pin: {NEW_TAG}")))
        .stdout(predicates::str::contains("hint: follows: nothing (pinned)"));

    let entry = &manual_models(&dir)["chapkit_example_manual_model"];
    assert_eq!(entry["tag"], NEW_TAG);
    assert_eq!(entry["follow"], Json::Null);
    assert_eq!(entry["repository"], Json::Null);
    assert_eq!(entry["commit"], Json::Null);
    // An exact pin follows no channel, and `update` says so.
    assert_eq!(
        state(&dir)["models"]["chapkit_example_manual_model"]["channel"],
        Json::Null
    );
    sandbox
        .online(Hub::new().advanced().start())
        .args(["update", "--dry-run"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "chapkit_example_manual_model  {NEW_TAG}  pinned, skipped"
        )))
        .stdout(predicates::str::contains("already up to date"));
}

#[test]
fn models_add_accepts_a_digest_and_renders_a_digest_reference() {
    let (sandbox, dir, port) = added_sandbox(Hub::new());
    let digest = format!("sha256:{}", "a".repeat(64));
    sandbox
        .online(port)
        .args(["-v", "models", "add", &format!("{IMAGE}@{digest}")])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!("hint: pin: @{digest}")));

    assert_eq!(
        manual_models(&dir)["chapkit_example_manual_model"]["tag"],
        format!("@{digest}")
    );
    // A digest carries its own separator, so the rendered reference has no
    // colon in front of it.
    let overlay = read(&dir.join("compose.chapkit-example-manual-model.yml"));
    assert!(
        overlay.contains(&format!(
            "image: {IMAGE}${{CHAPKIT_EXAMPLE_MANUAL_MODEL_IMAGE_TAG:-@{digest}}}"
        )),
        "{overlay}"
    );
    assert!(!overlay.contains(&format!("{IMAGE}:@")), "{overlay}");
}

#[test]
fn models_add_keeps_a_user_it_cannot_resolve_and_says_what_it_will_chown() {
    let (sandbox, dir, port) = added_sandbox(Hub::new().running_as("app"));
    sandbox
        .online(port)
        .args(["-v", "models", "add", REPO_URL])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: user: app (from the image config)",
        ))
        // The note says what the init container will do instead, and how to
        // decide it properly.
        .stdout(predicates::str::contains("`app` has no uid this CLI knows"))
        .stdout(predicates::str::contains("--user <uid>:<gid>"))
        .stdout(predicates::str::contains("chown 1000:1000").not());

    let overlay = read(&dir.join("compose.chapkit-example-manual-model.yml"));
    assert!(overlay.contains("user: app"), "{overlay}");
    assert!(
        overlay.contains("chown -R 1000:1000 /work/data"),
        "{overlay}"
    );

    // And `--user` is what settles it.
    sandbox
        .online(port)
        .args([
            "-v",
            "models",
            "add",
            REPO_URL,
            "--id",
            "second_manual_model",
            "--service-id",
            "second-manual-model",
            "--user",
            "10001:10001",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: user: 10001:10001 (given on the command line)",
        ));
    assert!(
        read(&dir.join("compose.second-manual-model.yml"))
            .contains("chown -R 10001:10001 /work/data"),
        "the flag reaches the init container"
    );
}

#[test]
fn models_add_from_a_repository_refuses_to_run_offline() {
    let sandbox = Sandbox::new();
    sandbox.init(&["--models", "none"]).assert().success();
    sandbox
        .models(&["add", REPO_URL])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--offline"))
        .stderr(predicates::str::contains(IMAGE));
    assert_eq!(manual_models(&sandbox.project()), Json::Null);
}

#[test]
fn models_add_refuses_a_name_the_marketplace_or_this_project_holds() {
    let (sandbox, dir, port) = added_sandbox(Hub::new());
    // An id the catalogue lists is a model to enable, not one to add.
    sandbox
        .online(port)
        .args(["models", "add", REPO_URL, "--id", "chapkit_ewars_model"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "the marketplace already lists chapkit_ewars_model",
        ))
        .stderr(predicates::str::contains("chap-devops models enable"));
    // And a compose service it already uses would be two overlays fighting.
    sandbox
        .online(port)
        .args([
            "models",
            "add",
            REPO_URL,
            "--service-id",
            "chapkit-ewars-model",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--service-id"));
    assert_eq!(manual_models(&dir), Json::Null, "nothing was written");

    // Nor one chap-devops uses for itself: `chap-devops` would be written over
    // `compose.chap-devops.yml`, `chap` merged into chap-core's own service.
    for reserved in ["chap-devops", "chap", "marketplace", "dhis2"] {
        sandbox
            .online(port)
            .args(["models", "add", REPO_URL, "--service-id", reserved])
            .assert()
            .failure()
            .stderr(predicates::str::contains(
                "chap-devops uses for its own services",
            ));
    }
    assert_eq!(manual_models(&dir), Json::Null, "nothing was written");

    // Adding the same source twice needs a name of its own.
    sandbox
        .online(port)
        .args(["models", "add", REPO_URL])
        .assert()
        .success();
    sandbox
        .online(port)
        .args(["models", "add", REPO_URL])
        .assert()
        .failure()
        .stderr(predicates::str::contains("was already added"))
        .stderr(predicates::str::contains("chap-devops models remove"));

    // A bare image name is a local image, and one with no tag pins nothing.
    sandbox
        .online(port)
        .args(["models", "add", "chapkit_example_manual_model"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("names no tag"));
}

#[test]
fn models_remove_takes_the_definition_and_the_overlay_with_it() {
    let (sandbox, dir, port) = added_sandbox(Hub::new());
    sandbox
        .online(port)
        .args(["models", "add", REPO_URL])
        .assert()
        .success();
    assert!(
        dir.join("compose.chapkit-example-manual-model.yml")
            .is_file()
    );

    // Offline: the definition is the deployment's own, so removing it needs
    // nothing from the network. `--keep-in-chap` leaves chap-core alone, so
    // nothing asks the port it would answer on.
    sandbox
        .models(&[
            "remove",
            "chapkit-example-manual-model",
            "--keep-in-chap",
            "-v",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "removed chapkit_example_manual_model\n",
        ))
        .stdout(predicates::str::contains(
            "hint: chapkit_example_manual_model is gone from `models-manual.yaml`",
        ))
        .stdout(predicates::str::contains(
            "disabled chapkit_example_manual_model",
        ))
        .stdout(predicates::str::contains(
            "removed compose.chapkit-example-manual-model.yml",
        ));

    assert!(
        !dir.join("compose.chapkit-example-manual-model.yml")
            .exists()
    );
    assert!(includes(&dir).is_empty());
    let manual = read(&dir.join(".chap-devops").join("models-manual.yaml"));
    assert!(!manual.contains("chapkit_example_manual_model"), "{manual}");
    assert_eq!(state(&dir)["models"], serde_json::json!({}));

    // A marketplace model has no local definition to remove.
    sandbox
        .online(port)
        .args(["models", "remove", "chapkit_ewars_model"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("is a marketplace model"))
        .stderr(predicates::str::contains("chap-devops models disable"));
    // And a name nothing answers to is the typo it looks like.
    sandbox
        .models(&["remove", "nope"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("unknown model `nope`"));
}

#[test]
fn a_manual_model_can_be_disabled_and_enabled_again_at_the_recorded_tag() {
    let (sandbox, dir, port) = added_sandbox(Hub::new());
    sandbox
        .online(port)
        .args(["models", "add", REPO_URL])
        .assert()
        .success();

    // Disabling keeps the definition, which is what makes it reversible.
    sandbox
        .models(&["disable", "chapkit_example_manual_model"])
        .assert()
        .success();
    assert_eq!(state(&dir)["models"], serde_json::json!({}));
    assert_eq!(
        manual_models(&dir)["chapkit_example_manual_model"]["tag"],
        OLD_TAG,
        "the definition outlives the enablement"
    );

    // And enabling it again needs no network at all: the deployment is where
    // its definition lives.
    sandbox
        .models(&["enable", "chapkit_example_manual_model"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "enabled chapkit_example_manual_model",
        ));
    let enabled = &state(&dir)["models"]["chapkit_example_manual_model"];
    assert_eq!(enabled["image_tag"], OLD_TAG);
    assert_eq!(enabled["data_dir"], "/work/data");
    assert_eq!(enabled["user"], "10001:10001");
    assert!(
        dir.join("compose.chapkit-example-manual-model.yml")
            .is_file()
    );

    // A forced re-init carries the definition over rather than dropping it.
    sandbox
        .init(&["--models", "none", "--force"])
        .assert()
        .success();
    assert_eq!(
        manual_models(&dir)["chapkit_example_manual_model"]["tag"],
        OLD_TAG
    );
}

#[test]
fn models_add_outside_a_project_says_so() {
    let sandbox = Sandbox::new();
    let port = Hub::new().start();
    sandbox
        .online(port)
        .args(["models", "add", REPO_URL])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not a chap-devops deployment"));
}

#[test]
fn models_add_of_a_marketplace_repository_enables_the_marketplace_model() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();
    // Offline is fine: nothing is resolved from GitHub, the catalogue has it.
    sandbox
        .models(&[
            "add",
            "https://github.com/chap-models/chapkit_ewars_model/",
            "--port",
            "auto",
        ])
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "is the marketplace model chapkit_ewars_model; enabling that",
        ));
    assert_eq!(
        state(&dir)["models"]["chapkit_ewars_model"]["channel"],
        "stable"
    );
    assert_eq!(manual_models(&dir), Json::Null);
}

#[test]
fn models_add_of_a_marketplace_image_pins_the_version_that_tag_is() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();
    sandbox
        .models(&["add", "ghcr.io/chap-models/chapkit_ewars_model:sha-24d58c0"])
        .assert()
        .success();
    let model = &state(&dir)["models"]["chapkit_ewars_model"];
    assert_eq!(model["version"], "1.0.3");
    assert_eq!(model["image_tag"], "sha-24d58c0");
    assert_eq!(model["channel"], Json::Null);
}

#[test]
fn models_add_with_id_auto_picks_a_free_id_beside_the_marketplace_one() {
    let (sandbox, dir, port) = added_sandbox(Hub::new());
    sandbox
        .online(port)
        .args(["models", "add", REPO_URL, "--id", "auto"])
        .assert()
        .success();
    // The first add took the plain id, so the second gets the suffix.
    sandbox
        .online(port)
        .args(["models", "add", REPO_URL, "--id", "auto"])
        .assert()
        .success();
    let manual = manual_models(&dir);
    assert!(
        manual.get("chapkit_example_manual_model_2").is_some(),
        "{manual}"
    );
}

/// A second copy of a marketplace model carries the marketplace model's id in
/// its image, so it may never register under its own service id. The add
/// says so as a warning, with a way out that works.
#[test]
fn models_add_of_a_marketplace_repository_under_its_own_id_warns_it_may_not_register() {
    let (sandbox, dir, port) = added_sandbox(Hub::new());
    let out = sandbox
        .online(port)
        .args([
            "--json",
            "models",
            "add",
            "https://github.com/chap-models/chapkit_ewars_model",
            "--id",
            "auto",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc: Json = serde_json::from_slice(&out).expect("one JSON document");
    let added = manual_models(&dir);
    let id = added
        .as_object()
        .and_then(|m| m.keys().next().cloned())
        .expect("one model added");
    let warning = doc["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|m| m["level"] == "warning")
        .unwrap_or_else(|| panic!("no warning in {doc}"));
    let text = warning["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("registers with chap-core as `chapkit-ewars-model`"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "run `chap-devops models remove {id}`, then `chap-devops models enable chapkit_ewars_model`"
        )),
        "{text}"
    );
    assert!(
        !doc.to_string().contains("--service-id <that id>"),
        "the way out that cannot work is gone: {doc}"
    );
}

/// An image on a registry other than ghcr: the token comes from the realm in
/// the challenge of a 401 on `/v2/`, and the image config is read the same
/// way as on ghcr, user, data directory and amd64 pin alike.
#[test]
fn models_add_reads_an_image_on_another_registry_through_its_token_challenge() {
    let hub = Hub::new().publishing("v1.2");
    let (sandbox, dir, _) = added_sandbox(Hub::new());
    let (port, seen) = hub.start_seen();
    sandbox
        .online(port)
        .args(["models", "add", "quay.io/someone/dengue_model:v1.2"])
        .assert()
        .success();

    let entry = &manual_models(&dir)["dengue_model"];
    assert_eq!(entry["image"], "quay.io/someone/dengue_model");
    assert_eq!(entry["tag"], "v1.2");
    let model = &state(&dir)["models"]["dengue_model"];
    assert_eq!(model["user"], "10001:10001");
    assert_eq!(model["data_dir"], "/work/data");
    let overlay = yaml(&dir.join("compose.dengue-model.yml"));
    let service = &overlay["services"]["dengue-model"];
    assert_eq!(
        service["image"],
        "quay.io/someone/dengue_model:${DENGUE_MODEL_IMAGE_TAG:-v1.2}"
    );
    assert_eq!(service["platform"], "linux/amd64");

    // The token the realm handed out went with the manifest request.
    let seen = seen.lock().expect("the request log");
    assert!(seen.iter().any(|(path, _)| path == "/v2/"), "{seen:?}");
    assert!(
        seen.iter().any(|(path, _)| path
            .starts_with("/token?service=stand-in&scope=repository:someone/dengue_model:pull")),
        "{seen:?}"
    );
    assert!(
        seen.iter().any(
            |(path, auth)| path == "/v2/someone/dengue_model/manifests/v1.2"
                && auth.as_deref() == Some("Bearer anonymous")
        ),
        "{seen:?}"
    );
}

/// A service id that is live in chap-core and that no model of this
/// deployment has is refused before anything is resolved, as `chap-admin`
/// refuses it.
#[test]
fn models_add_refuses_a_service_id_that_another_model_has_in_chap_core() {
    let sandbox = Sandbox::new();
    let chap_core = chap_core_server();
    sandbox
        .init(&["--models", "none", "--api-port", &chap_core.to_string()])
        .assert()
        .success();
    let port = Hub::new().start();
    sandbox
        .online(port)
        .args(["models", "add", REPO_URL, "--service-id", HOST_RUN_MODEL])
        .assert()
        .failure()
        .stderr(predicates::str::contains(format!(
            "chap-core already has a model registered as `{}` at http://{}:8000, and it is not \
             a model of this deployment; stop that model, or pass `--service-id <other>`",
            HOST_RUN_MODEL, HOST_RUN_MODEL
        )));
    assert!(
        !sandbox
            .project()
            .join(".chap-devops/models-manual.yaml")
            .exists()
    );
}
