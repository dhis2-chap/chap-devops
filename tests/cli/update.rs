use crate::common::*;
#[cfg(unix)]
use predicates::prelude::PredicateBooleanExt;

#[test]
fn update_needs_the_network_even_for_a_dry_run() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();
    let before = read(&dir.join(".chap-devops/models.yaml"));

    let mut update = sandbox.chap();
    update.arg("-C").arg(&dir).args(["update", "--dry-run"]);
    update.assert().failure().stderr(predicates::str::contains(
        "`chap-devops update` refreshes the marketplace registry, which needs the network; drop \
             --offline",
    ));
    assert_eq!(read(&dir.join(".chap-devops/models.yaml")), before);
}

/// A deployment created with `--registry-url` keeps that registry: the flag is
/// recorded in `.chap-devops/project.yaml`, and a later command without it used to
/// fall back to the default marketplace - here, the snapshot built into the
/// binary - so the custom registry's models were unknown.
#[test]
fn the_registry_a_deployment_was_created_with_is_the_one_it_uses() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let port = Hub::new().start();
    let custom = format!("http://127.0.0.1:{port}/registry.yaml");
    sandbox
        .online_init(port, &["--models", "none", "--chap-tag", "v2.3.1"])
        .assert()
        .success();

    // No `--registry-url`: the recorded one, from the cache `init` filled.
    let report = json_of(&mut chap_in(
        &sandbox,
        &dir,
        &["--json", "registry", "show"],
    ));
    assert_eq!(report["url"], custom.as_str(), "{report}");

    // The flag still wins for the run it is typed on.
    let other = "http://127.0.0.1:1/other.yaml";
    let report = json_of(&mut chap_in(
        &sandbox,
        &dir,
        &["--json", "--registry-url", other, "registry", "show"],
    ));
    assert_ne!(report["url"], custom.as_str(), "{report}");
    let report = json_of(&mut chap_in(
        &sandbox,
        &dir,
        &["--json", "registry", "show", "--registry-url", other],
    ));
    assert_ne!(report["url"], custom.as_str(), "{report}");
}

#[cfg(unix)]
#[test]
fn update_switches_chap_core_to_a_moving_tag_and_back() {
    let (sandbox, dir, port, _temp, bin) = pinned_sandbox();

    // Forwards, onto a tag that is ahead of every release: no confirmation,
    // and the compose file of that branch comes with it.
    online_update(&sandbox, port, &bin, &["--chap-tag", "dev"])
        .assert()
        .success()
        .stdout(predicates::str::contains("chap-core  v2.3.1 -> dev"))
        .stdout(predicates::str::contains("updated chap-core v2.3.1 -> dev"))
        .stderr(predicates::str::contains("backup").not());

    let moved = state(&dir);
    assert_eq!(moved["chap_image_tag"], "dev");
    assert_eq!(moved["chap_compose_source"]["tag"], "dev");
    assert!(
        moved["chap_compose_source"]["url"]
            .as_str()
            .unwrap()
            .ends_with("/dhis2-chap/chap-core/dev/compose.ghcr.yml")
    );
    assert!(dir.join(".chap-devops/compose.chap-core.dev.yml").is_file());
    assert!(
        read(&dir.join(".chap-devops/compose.chap-core.dev.yml"))
            .contains("CHAP_DEVOPS_TEST_REF: dev"),
        "the fetched copy is the dev one"
    );
    // The rendered base follows it, and so does the line compose reads.
    let base = read(&dir.join("compose.yml"));
    assert!(base.contains("CHAP_DEVOPS_TEST_REF: dev"), "{base}");
    assert!(
        base.contains("# chap-core compose.ghcr.yml at dev\n"),
        "{base}"
    );
    assert_eq!(env_value(&sandbox.env(), "CHAP_IMAGE_TAG"), Some("dev"));

    // Asking for the tag it already runs changes nothing and says so.
    online_update(&sandbox, port, &bin, &["--chap-tag", "dev"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "chap-core  dev  already the pin, nothing to switch",
        ))
        .stdout(predicates::str::contains("already up to date"));
    assert_eq!(state(&dir)["chap_image_tag"], "dev");

    // And back to the release, which is the direction that needs an answer.
    online_update(&sandbox, port, &bin, &["--chap-tag", "v2.3.1", "--yes"])
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "moving chap-core from dev to v2.3.1 can run an older schema against a database \
             migrated by the newer one; run `chap-devops backup create` first",
        ))
        .stdout(predicates::str::contains("chap-core  dev -> v2.3.1"))
        .stdout(predicates::str::contains("updated chap-core dev -> v2.3.1"));

    assert_eq!(state(&dir)["chap_image_tag"], "v2.3.1");
    assert_eq!(state(&dir)["chap_compose_source"]["tag"], "v2.3.1");
    assert!(read(&dir.join("compose.yml")).contains("CHAP_DEVOPS_TEST_REF: v2.3.1"));
    assert_eq!(env_value(&sandbox.env(), "CHAP_IMAGE_TAG"), Some("v2.3.1"));
}

#[cfg(unix)]
#[test]
fn a_backwards_switch_without_an_answer_is_refused() {
    let (sandbox, dir, port, _temp, bin) = pinned_sandbox();
    online_update(&sandbox, port, &bin, &["--chap-tag", "v2.3.0"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "moving chap-core from v2.3.1 to v2.3.0",
        ))
        .stderr(predicates::str::contains("not a terminal"))
        .stderr(predicates::str::contains("--yes"));
    // Refused before anything was written.
    assert_eq!(state(&dir)["chap_image_tag"], "v2.3.1");
    assert_eq!(env_value(&sandbox.env(), "CHAP_IMAGE_TAG"), Some("v2.3.1"));
}

#[cfg(unix)]
#[test]
fn update_refuses_a_chap_tag_that_was_never_released() {
    let (sandbox, dir, port, _temp, bin) = pinned_sandbox();
    let before = read(&dir.join("compose.yml"));
    online_update(&sandbox, port, &bin, &["--chap-tag", "v9.9.9"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("chap-core has no release v9.9.9"))
        .stderr(predicates::str::contains("chap-devops update --list-tags"));
    assert_eq!(state(&dir)["chap_image_tag"], "v2.3.1");
    assert_eq!(read(&dir.join("compose.yml")), before);

    // The two ways of deciding chap-core's tag cannot both be given.
    online_update(
        &sandbox,
        port,
        &bin,
        &["--chap-tag", "dev", "--pin-chap-core"],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains("cannot be used with"));
}

/// `latest` is the newest release, so `--pin-chap-core` from it pins the same
/// image and asks nothing. From `dev`, the refusal names the flag it came from.
#[cfg(unix)]
#[test]
fn pin_chap_core_from_latest_is_not_a_move_backwards() {
    let (sandbox, dir, port, _temp, bin) = pinned_sandbox();
    online_update(&sandbox, port, &bin, &["--chap-tag", "latest"])
        .assert()
        .success();
    online_update(&sandbox, port, &bin, &["--pin-chap-core"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "updated chap-core latest -> v2.3.1",
        ))
        .stderr(predicates::str::contains("backup").not());
    assert_eq!(state(&dir)["chap_image_tag"], "v2.3.1");

    online_update(&sandbox, port, &bin, &["--chap-tag", "dev"])
        .assert()
        .success();
    online_update(&sandbox, port, &bin, &["--pin-chap-core"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "run `chap-devops update --pin-chap-core --yes` to confirm it",
        ));
    assert_eq!(state(&dir)["chap_image_tag"], "dev");
}

/// `--pin-chap-core` without the newest release has nothing to pin to: the
/// run fails and says so, rather than ending on "already up to date".
#[cfg(unix)]
#[test]
fn pin_chap_core_fails_when_the_release_lookup_fails() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--chap-tag", "latest"])
        .assert()
        .success();
    let port = Hub {
        releases: Vec::new(),
        ..Hub::new()
    }
    .start();
    let (_temp, bin, _) = quiet_docker();
    for args in [
        &["--pin-chap-core"][..],
        &["--pin-chap-core", "--dry-run"][..],
    ] {
        online_update(&sandbox, port, &bin, args)
            .assert()
            .failure()
            .stderr(predicates::str::contains(
                "could not resolve the newest chap-core release",
            ))
            .stderr(predicates::str::contains("--pin-chap-core pinned nothing"))
            .stdout(predicates::str::contains("already up to date").not());
    }
    assert_eq!(state(&dir)["chap_image_tag"], "latest");
}

/// GitHub can mark a Helm chart release as the latest, and no image has its tag.
#[cfg(unix)]
#[test]
fn a_helm_chart_release_is_not_the_newest_chap_core_release() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--chap-tag", "latest"])
        .assert()
        .success();
    let mut hub = Hub::new();
    hub.releases
        .insert(0, ("chap-1.2.0".into(), "2026-10-29T14:39:27Z".into()));
    let port = hub.start();
    let (_temp, bin, _) = quiet_docker();
    online_update(&sandbox, port, &bin, &["--pin-chap-core"])
        .assert()
        .success();
    assert_eq!(state(&dir)["chap_image_tag"], "v2.3.1");
}

/// `--chap-tag latest` moves the pin even when the newest release cannot be
/// looked up, so the warning says that, and not that the pin stays.
#[cfg(unix)]
#[test]
fn chap_tag_latest_without_the_release_lookup_says_the_pin_moves() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--chap-tag", "v2.3.1"])
        .assert()
        .success();
    let port = Hub {
        releases: Vec::new(),
        ..Hub::new()
    }
    .start();
    let (_temp, bin, _) = quiet_docker();
    online_update(&sandbox, port, &bin, &["--dry-run", "--chap-tag", "latest"])
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "the pin would move to `latest`, and compose.yml keeps the layout it has",
        ))
        .stderr(predicates::str::contains("stays at").not());
    assert_eq!(state(&dir)["chap_image_tag"], "v2.3.1");
    online_update(&sandbox, port, &bin, &["--chap-tag", "latest"])
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "the pin moves to `latest`, and compose.yml keeps the layout it has",
        ))
        .stderr(predicates::str::contains("stays at").not())
        .stderr(predicates::str::contains("no chap-core ref").not());
    assert_eq!(state(&dir)["chap_image_tag"], "latest");
}

/// A tag that is neither a release nor a moving tag is named as an exact pin
/// only when the pin moves to it, never next to the error that refuses it.
#[cfg(unix)]
#[test]
fn the_exact_pin_warning_comes_only_with_a_move() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let port = Hub {
        built: vec!["sha-fa880a1".to_string()],
        ..Hub::new()
    }
    .start();
    sandbox
        .online_init(port, &["--models", "none", "--chap-tag", "v2.3.1"])
        .assert()
        .success();
    let (_temp, bin, _) = quiet_docker();
    online_update(&sandbox, port, &bin, &["--chap-tag", "nonsense"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "ghcr.io has no image chap-core:nonsense",
        ))
        .stderr(predicates::str::contains("exact pin").not());
    assert_eq!(state(&dir)["chap_image_tag"], "v2.3.1");

    online_update(
        &sandbox,
        port,
        &bin,
        &["--dry-run", "--chap-tag", "sha-fa880a1"],
    )
    .assert()
    .success()
    .stderr(predicates::str::contains(
        "so it would be recorded as an exact pin",
    ));
    online_update(&sandbox, port, &bin, &["--chap-tag", "sha-fa880a1"])
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "so it is recorded as an exact pin",
        ));
    assert_eq!(state(&dir)["chap_image_tag"], "sha-fa880a1");
}

/// `dev` is a branch of chap-core, but ghcr has no image for it. The tag is
/// refused before the pin moves, and the dry run says the same.
#[cfg(unix)]
#[test]
fn update_refuses_a_chap_tag_ghcr_has_no_image_for() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let port = Hub {
        unbuilt: vec!["dev".to_string()],
        ..Hub::new()
    }
    .start();
    sandbox
        .online_init(port, &["--models", "none", "--chap-tag", "v2.3.1"])
        .assert()
        .success();
    let (_temp, bin, _) = quiet_docker();
    let before = (read(&dir.join(".chap-devops/project.yaml")), sandbox.env());
    for args in [
        &["--dry-run", "--chap-tag", "dev"][..],
        &["--chap-tag", "dev"][..],
    ] {
        online_update(&sandbox, port, &bin, args)
            .assert()
            .failure()
            .stderr(predicates::str::contains(
                "ghcr.io has no image chap-core:dev and chap-worker:dev, so the pull would fail",
            ))
            .stderr(predicates::str::contains("chap-devops update --list-tags"));
        assert_eq!(read(&dir.join(".chap-devops/project.yaml")), before.0);
        assert_eq!(sandbox.env(), before.1);
    }
}

#[cfg(unix)]
#[test]
fn a_dry_run_switch_writes_nothing() {
    let (sandbox, dir, port, _temp, bin) = pinned_sandbox();
    let before = (
        read(&dir.join("compose.yml")),
        read(&dir.join(".chap-devops/project.yaml")),
        sandbox.env(),
    );

    online_update(&sandbox, port, &bin, &["--dry-run", "--chap-tag", "dev"])
        .assert()
        .success()
        .stdout(predicates::str::contains("chap-core  v2.3.1 -> dev"))
        .stdout(predicates::str::ends_with(
            "would update chap-core v2.3.1 -> dev\n",
        ));

    assert_eq!(read(&dir.join("compose.yml")), before.0);
    assert_eq!(read(&dir.join(".chap-devops/project.yaml")), before.1);
    assert_eq!(sandbox.env(), before.2);
    assert!(!dir.join(".chap-devops/compose.chap-core.dev.yml").exists());

    // A dry run backwards says what it would cost and still writes nothing,
    // without an answer: there is nothing yet to confirm.
    online_update(&sandbox, port, &bin, &["--dry-run", "--chap-tag", "v2.3.0"])
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "moving chap-core from v2.3.1 to v2.3.0",
        ));
    assert_eq!(read(&dir.join(".chap-devops/project.yaml")), before.1);
}

#[test]
fn list_tags_names_the_moving_tags_the_releases_and_the_pin() {
    let sandbox = Sandbox::new();
    let port = Hub::new().start();
    sandbox
        .online_init(port, &["--models", "none", "--chap-tag", "v2.3.0"])
        .assert()
        .success();

    let mut cmd = sandbox.online(port);
    cmd.args(["update", "--list-tags"]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(out).expect("text");
    assert!(text.contains("TAG") && text.contains("KIND"), "{text}");
    assert!(
        text.contains("PUBLISHED") && text.contains("NOTE"),
        "{text}"
    );
    for row in [
        "dev     moving   2026-09-24  -",
        "master  moving   2026-09-24  -",
        "latest  moving   2026-09-21  -",
        "v2.3.1  release  2026-09-21  newest",
        "v2.3.0  release  2026-09-11  pinned",
    ] {
        assert!(text.contains(row), "missing row `{row}` in:\n{text}");
    }
    // The pin is the result; the way to move it is a hint.
    assert!(
        text.ends_with("\nchap-core is pinned to v2.3.0\n"),
        "{text}"
    );
    assert!(!text.contains("--chap-tag <TAG>"), "{text}");

    // The same as JSON, which is the list a script reads.
    let mut cmd = sandbox.online(port);
    cmd.args(["--json", "update", "--list-tags"]);
    let list = json_of(&mut cmd);
    assert_eq!(list["pin"], "v2.3.0");
    assert_eq!(list["releases_listed"], true);
    let tags: Vec<&str> = list["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["tag"].as_str().unwrap())
        .collect();
    assert_eq!(tags, vec!["dev", "master", "latest", "v2.3.1", "v2.3.0"]);
    assert_eq!(list["tags"][3]["newest"], true);
    assert_eq!(list["tags"][4]["pinned"], true);
    assert_eq!(list["messages"][0]["level"], "info");
    assert_eq!(list["messages"][0]["text"], "chap-core is pinned to v2.3.0");

    // It writes nothing: the listing is a question, not a change.
    assert_eq!(state(&sandbox.project())["chap_image_tag"], "v2.3.0");
}

#[test]
fn list_tags_works_offline_with_what_it_has() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--models", "none", "--chap-tag", "v1.2.3"])
        .assert()
        .success();

    let mut cmd = sandbox.chap();
    cmd.arg("-C")
        .arg(sandbox.project())
        .args(["update", "--list-tags"]);
    cmd.assert()
        .success()
        .stderr(predicates::str::contains(
            "--offline: the chap-core releases were not listed",
        ))
        .stdout(predicates::str::contains("dev     moving   -          -"))
        .stdout(predicates::str::contains(
            "v1.2.3  release  -          pinned",
        ))
        .stdout(predicates::str::contains("chap-core is pinned to v1.2.3"));

    // `--dry-run` has nothing to say about a command that writes nothing.
    let mut dry = sandbox.chap();
    dry.arg("-C")
        .arg(sandbox.project())
        .args(["update", "--list-tags", "--dry-run"]);
    dry.assert().success().stdout(predicates::str::contains(
        "v1.2.3  release  -          pinned",
    ));
}

/// Without a deployment there is no pin to move, but `chap-devops update` still
/// refreshes this machine's marketplace registry and says whether it changed.
#[test]
fn update_without_a_deployment_refreshes_the_registry() {
    let sandbox = Sandbox::new();
    let port = Hub::new().start();
    let base = format!("http://127.0.0.1:{port}");

    let update = || {
        let mut update = assert_cmd::Command::cargo_bin("chap-devops").unwrap();
        update
            .env("CHAP_DEVOPS_CACHE_DIR", sandbox.cache.path())
            .env("CHAP_DEVOPS_DATA_DIR", sandbox.cache.path().join("data"))
            .env("CHAP_DEVOPS_NO_UPDATE_CHECK", "1")
            .env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1")
            .current_dir(sandbox.home.path())
            .arg("--registry-url")
            .arg(format!("{base}/registry.yaml"))
            .arg("update");
        update
    };
    // No cached copy to compare with.
    update()
        .assert()
        .success()
        .stdout("fetched the marketplace registry: 1 model\n");
    // The same registry a second time is no change, and says so.
    update()
        .assert()
        .success()
        .stdout("the marketplace registry did not change: 1 model\n");
    let report = json_of(update().arg("--json"));
    assert_eq!(
        report["changes"],
        serde_json::json!({"added": [], "removed": [], "changed": []})
    );
}

#[test]
fn update_without_a_deployment_refuses_the_flags_that_move_pins() {
    let sandbox = Sandbox::new();
    chap_in(
        &sandbox,
        sandbox.home.path(),
        &["update", "--chap-tag", "master"],
    )
    .assert()
    .code(2)
    .stderr(predicates::str::contains(
        "--chap-tag moves the pins of a deployment",
    ));
    // The listing moves nothing, and the refusal says what it does.
    chap_in(&sandbox, sandbox.home.path(), &["update", "--list-tags"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains(
            "--list-tags lists the chap-core tags a deployment can move to, and this directory \
             is not one",
        ));

    // Offline, a refresh cannot happen, and --dry-run reports the cached one.
    chap_in(&sandbox, sandbox.home.path(), &["update"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains(
            "drop --offline, or use --dry-run",
        ));
    chap_in(&sandbox, sandbox.home.path(), &["update", "--dry-run"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--dry-run fetched nothing"));
}

/// `registry update` cannot refresh offline: a usage error, like `update`,
/// with the way out on the same line.
#[test]
fn registry_update_offline_says_to_drop_offline() {
    let sandbox = Sandbox::new();
    sandbox
        .chap()
        .args(["registry", "update"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains(
            "needs the network; drop --offline",
        ));
}

/// A corrupt cache is ignored with a warning that says how to write it again.
#[test]
fn a_corrupt_registry_cache_names_registry_update() {
    let (sandbox, dir, port) = added_sandbox(Hub::new());
    sandbox
        .online(port)
        .args(["registry", "update"])
        .assert()
        .success();
    let registries = sandbox.cache.path().join("registries");
    // The sandbox also holds the frozen marketplace data, under the default
    // registry URL; this entry is the one the stand-in's URL wrote.
    let entry = std::fs::read_dir(&registries)
        .expect("registry update wrote a cache")
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().contains("127_0_0_1"))
        .expect("a cache entry for the stand-in");
    std::fs::write(entry.join("meta.json"), "not json").unwrap();
    let out = chap_in(
        &sandbox,
        &dir,
        &[
            "registry",
            "show",
            "--registry-url",
            &format!("http://127.0.0.1:{port}/registry.yaml"),
        ],
    )
    .assert()
    .success()
    .get_output()
    .stderr
    .clone();
    let stderr = String::from_utf8_lossy(&out);
    assert!(stderr.contains("meta.json is corrupt"), "{stderr}");
    assert!(
        stderr.contains("run `chap-devops registry update` to write it again"),
        "{stderr}"
    );
}
