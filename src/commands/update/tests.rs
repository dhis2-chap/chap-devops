use super::chap_core::*;
use super::components::*;
use super::models::*;
use super::report::*;
use super::restart::*;
use super::tags::*;
use super::*;
use crate::chapcore;
use crate::compose::apply::apply_with;
use crate::compose::resolve::UserSource;
use crate::compose::{EnableRequest, Selection};
use crate::output::{Out, Report};
use crate::project::ManualModel;
use crate::project::ProjectState;
use crate::registry::{Channel, load_embedded};
use crate::registry::{Registry, VersionSelector};
use std::collections::BTreeMap;

fn project_with(ids: &[&str]) -> (tempfile::TempDir, Project, Registry) {
    let dir = tempfile::tempdir().unwrap();
    let registry = load_embedded().unwrap();
    let mut project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState::default(),
    };
    let sel = Selection {
        enable: ids.iter().map(|id| EnableRequest::new(*id)).collect(),
        ..Selection::default()
    };
    apply_with(
        &mut project,
        &registry,
        &sel,
        &|_| false,
        &crate::compose::resolve::from_table,
    )
    .unwrap();
    (dir, project, registry)
}

/// A lookup nobody is expected to make: every test that plans only
/// marketplace models passes this, so a manual row would be obvious.
fn no_lookup(id: &str, _: &ManualModel) -> Option<(String, String)> {
    panic!("{id} is not a manual model");
}

#[test]
fn plan_reports_unchanged_when_the_channel_still_points_at_the_pin() {
    let (_dir, project, registry) = project_with(&["chapkit_ewars_model"]);
    let plan = plan(&project, &registry, &no_lookup).unwrap();
    assert_eq!(plan.len(), 1);
    assert!(!plan[0].changed && !plan[0].pinned);
    assert_eq!(plan[0].old_tag, plan[0].new_tag);
}

#[test]
fn plan_moves_a_channel_pin_and_skips_an_exact_one() {
    let (_dir, mut project, mut registry) =
        project_with(&["chapkit_ewars_model", "auto_arima_chapkit"]);
    // Pretend the project was pinned to an older build of ewars.
    {
        let ewars = project.state.models.get_mut("chapkit_ewars_model").unwrap();
        ewars.version = "0.9.0".into();
        ewars.image_tag = "sha-0000000".into();
        ewars.channel = Some(Channel::Stable);
        let arima = project.state.models.get_mut("auto_arima_chapkit").unwrap();
        arima.channel = None;
        arima.image_tag = "sha-1111111".into();
    }
    // And that the marketplace moved auto_arima too; a pinned model ignores it.
    let arima = registry
        .models
        .iter_mut()
        .find(|m| m.id == "auto_arima_chapkit")
        .unwrap();
    arima.versions[0].image_tag = "sha-2222222".into();

    let plan = plan(&project, &registry, &no_lookup).unwrap();
    // Where stable points in the snapshot, whatever a marketplace
    // refresh last made it.
    let stable = registry
        .get("chapkit_ewars_model")
        .unwrap()
        .resolve(&VersionSelector::Channel(Channel::Stable))
        .unwrap();
    let ewars = plan.iter().find(|m| m.id == "chapkit_ewars_model").unwrap();
    assert!(ewars.changed);
    assert_eq!(ewars.old_version, "0.9.0");
    assert_eq!(ewars.new_version, stable.version);
    assert_eq!(ewars.new_tag, stable.image_tag);

    let arima = plan.iter().find(|m| m.id == "auto_arima_chapkit").unwrap();
    assert!(arima.pinned && !arima.changed);
    assert_eq!(arima.new_tag, "sha-1111111");
}

/// The marketplace can pin a new digest under the same tag and version; that
/// moves the pin as well, and the plan carries the digest to record.
#[test]
fn plan_moves_a_pin_whose_digest_changed() {
    let (_dir, project, mut registry) = project_with(&["chapkit_ewars_model"]);
    let entry = &project.state.models["chapkit_ewars_model"];
    let digest = format!("sha256:{}", "e".repeat(64));
    let model = registry
        .models
        .iter_mut()
        .find(|m| m.id == "chapkit_ewars_model")
        .unwrap();
    let stable = model.channels.stable.clone();
    let version = model
        .versions
        .iter_mut()
        .find(|v| v.version == stable)
        .unwrap();
    assert_eq!(version.image_tag, entry.image_tag);
    version.image_digest = Some(digest.clone());

    let plan = plan(&project, &registry, &no_lookup).unwrap();
    assert!(plan[0].changed);
    assert_eq!(plan[0].old_tag, plan[0].new_tag);
    assert_eq!(plan[0].old_digest, entry.image_digest);
    assert_eq!(plan[0].new_digest.as_deref(), Some(digest.as_str()));
    assert_eq!(plan[0].new_pin(), format!("{}@{digest}", entry.image_tag));
    let version = format!("v{}", entry.version);
    assert_eq!(
        digest_cells(&plan[0]),
        Some((
            format!("{version} ({}@no digest)", entry.image_tag),
            format!("{version} ({}@eeeeeeeeeeee)", entry.image_tag),
        ))
    );
}

/// A project with one manually added model, enabled.
fn project_with_manual(follow: Option<&str>) -> (tempfile::TempDir, Project, Registry) {
    let dir = tempfile::tempdir().unwrap();
    let mut project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState::default(),
    };
    project.state.manual.insert(
        "chapkit_example_manual_model".to_string(),
        ManualModel {
            reads_port: false,
            service_id: "chapkit-example-manual-model".into(),
            display_name: "chapkit_example_manual_model".into(),
            repository: Some("https://github.com/example/chapkit_example_manual_model".into()),
            image: "ghcr.io/example/chapkit_example_manual_model".into(),
            tag: "sha-1eb8cf1".into(),
            commit: None,
            follow: follow.map(str::to_string),
            data_dir: Some("/work/data".into()),
            user: Some("10001:10001".into()),
            runtime_amd64: true,
            added: "2026-09-24".into(),
        },
    );
    let mut registry = load_embedded().unwrap();
    assert!(registry.with_manual(&project.state.manual).is_empty());
    let mut request = EnableRequest::new("chapkit_example_manual_model");
    request.selector = match follow {
        Some(_) => VersionSelector::Channel(Channel::Latest),
        None => VersionSelector::Exact("sha-1eb8cf1".into()),
    };
    let sel = Selection {
        enable: vec![request],
        ..Selection::default()
    };
    apply_with(
        &mut project,
        &registry,
        &sel,
        &|_| false,
        &crate::compose::resolve::from_table,
    )
    .unwrap();
    (dir, project, registry)
}

#[test]
fn a_following_manual_model_moves_when_its_branch_has_a_newer_build() {
    let (_dir, project, registry) = project_with_manual(Some("main"));
    let plan = plan(&project, &registry, &|id, entry| {
        assert_eq!(id, "chapkit_example_manual_model");
        assert_eq!(entry.follow.as_deref(), Some("main"));
        Some(("b1d6c31deadbeef".to_string(), "sha-b1d6c31".to_string()))
    })
    .unwrap();
    assert_eq!(plan.len(), 1);
    let row = &plan[0];
    assert!(row.manual && row.changed && row.checked && !row.pinned);
    assert_eq!(row.follow.as_deref(), Some("main"));
    assert_eq!(row.old_tag, "sha-1eb8cf1");
    assert_eq!(row.new_tag, "sha-b1d6c31");
    // The version of a manual entry is its tag, which is what the line
    // prints in place of a version number.
    assert_eq!(row.new_version, "sha-b1d6c31");
    assert_eq!(row.new_commit.as_deref(), Some("b1d6c31deadbeef"));
    let text = plan_text(
        &UpdateReport {
            registry: RegistryInfo {
                url: "https://example.test/registry.yaml".into(),
                provenance: Provenance::Network,
            },
            models: plan,
            chap_core: Some(chap_core("latest", "latest", None)),
            components: Vec::new(),
            pulled: false,
            pulled_new: Vec::new(),
            restart_needed: Vec::new(),
            stack_running: Some(false),
            dry_run: true,
        },
        &Out::default(),
    );
    assert!(
        text.contains("  chapkit_example_manual_model  sha-1eb8cf1 -> sha-b1d6c31\n"),
        "{text}"
    );
}

#[test]
fn a_following_manual_model_that_has_not_moved_is_unchanged() {
    let (_dir, project, registry) = project_with_manual(Some("main"));
    let plan = plan(&project, &registry, &|_, _| {
        Some(("1eb8cf1".to_string(), "sha-1eb8cf1".to_string()))
    })
    .unwrap();
    assert!(plan[0].manual && !plan[0].changed && plan[0].checked);
    assert_eq!(state_of(&plan[0]), "unchanged");
}

/// A repository that would not answer leaves the pin alone, and the row
/// says that nothing was established rather than that nothing moved.
#[test]
fn a_manual_model_whose_lookup_failed_says_it_could_not_check() {
    let (_dir, project, registry) = project_with_manual(Some("main"));
    let plan = plan(&project, &registry, &|_, _| None).unwrap();
    assert!(plan[0].manual && !plan[0].changed && !plan[0].checked);
    assert_eq!(state_of(&plan[0]), "unchanged (could not check)");
    assert_eq!(plan[0].new_tag, "sha-1eb8cf1");
}

#[test]
fn a_pinned_manual_model_is_never_looked_up() {
    let (_dir, project, registry) = project_with_manual(None);
    let plan = plan(&project, &registry, &|id, _| {
        panic!("{id} is pinned and must not be checked")
    })
    .unwrap();
    assert!(plan[0].manual && plan[0].pinned && !plan[0].changed);
    assert_eq!(state_of(&plan[0]), "pinned, skipped");
    assert_eq!(
        version_cell(&plan[0], "sha-1eb8cf1", "sha-1eb8cf1"),
        "sha-1eb8cf1"
    );
}

/// A manual entry is never an unknown model: the deployment is where its
/// definition lives, so there is nothing the catalogue could have dropped.
#[test]
fn a_manual_model_the_marketplace_never_had_is_not_an_unknown_model() {
    let (_dir, project, _) = project_with_manual(Some("main"));
    // The catalogue without the manual entry folded in, which is what a
    // registry refresh hands back before `with_manual` runs.
    let registry = load_embedded().unwrap();
    assert!(registry.get("chapkit_example_manual_model").is_none());
    let plan = plan(&project, &registry, &|_, _| None).unwrap();
    assert_eq!(plan.len(), 1);
    assert!(plan[0].manual);
}

#[test]
fn plan_fails_when_a_channel_model_left_the_registry() {
    let (_dir, mut project, registry) = project_with(&["chapkit_ewars_model"]);
    let entry = project.state.models.remove("chapkit_ewars_model").unwrap();
    project.state.models.insert("vanished".into(), entry);
    let err = plan(&project, &registry, &no_lookup).expect_err("unknown model");
    assert!(matches!(
        err.downcast_ref::<ChapError>(),
        Some(ChapError::UnknownModel(id)) if id == "vanished"
    ));
}

#[test]
fn the_plan_lists_each_model_before_anything_is_pulled() {
    let report = UpdateReport {
        registry: RegistryInfo {
            url: "https://example.test/registry.yaml".into(),
            provenance: Provenance::Network,
        },
        models: vec![
            ModelUpdate {
                id: "a".into(),
                old_version: "1.0.0".into(),
                old_tag: "sha-1111111".into(),
                new_version: "1.1.0".into(),
                new_tag: "sha-2222222".into(),
                changed: true,
                pinned: false,
                ..row("a")
            },
            ModelUpdate {
                id: "b".into(),
                old_version: "1.0.0".into(),
                old_tag: "sha-3333333".into(),
                new_version: "1.0.0".into(),
                new_tag: "sha-3333333".into(),
                changed: false,
                pinned: true,
                ..row("b")
            },
        ],
        chap_core: Some(chap_core("latest", "latest", None)),
        components: Vec::new(),
        pulled: false,
        pulled_new: Vec::new(),
        restart_needed: Vec::new(),
        stack_running: Some(true),
        dry_run: true,
    };
    let text = plan_text(&report, &Out::default());
    // Where the registry came from is a hint under the closing lines, not
    // a line of the plan.
    assert!(!text.contains("registry"), "{text}");
    let mut lines = Report::default();
    registry_hint(&report, &mut lines);
    assert_eq!(
        lines.text(),
        "hint: the registry is https://example.test/registry.yaml (network)\n"
    );
    // The names are padded to the width of "chap-core", so the versions
    // line up in one column.
    assert!(text.contains("  a          v1.0.0 (sha-1111111) -> v1.1.0 (sha-2222222)\n"));
    assert!(text.contains("  b          v1.0.0 (sha-3333333)  pinned, skipped\n"));
    assert!(text.contains(
        "  chap-core  latest  moving tag, would be re-pulled; pin it with \
             `chap-devops update --pin-chap-core`\n"
    ));
    // The plan is the plan: nothing in it claims anything has happened.
    assert!(!text.contains("restart"), "{text}");
    assert!(!text.contains("pulled the"), "{text}");

    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["stack_running"], true);
    assert_eq!(value["restart_needed"], serde_json::json!([]));
    assert_eq!(value["pulled_new"], serde_json::json!([]));
    assert_eq!(value["pulled"], false);
    assert!(
        value.get("restarted").is_none() && value.get("restart").is_none(),
        "update no longer touches containers: {value}"
    );
    assert_eq!(value["models"][0]["changed"], true);
    assert_eq!(value["registry"]["provenance"]["kind"], "network");
    assert_eq!(value["chap_core"]["old_tag"], "latest");
    assert_eq!(value["chap_core"]["changed"], false);
    assert_eq!(value["chap_core"]["moving"], true);
    assert_eq!(value["chap_core"]["compose_source"]["kind"], "embedded");
}

/// A new tag can be a new user, and the plan says so where it happened.
#[test]
fn a_row_whose_image_changed_its_user_says_so_under_the_pin() {
    let moved = ModelUpdate {
        old_version: "1.0.0".into(),
        old_tag: "sha-1111111".into(),
        new_version: "1.1.0".into(),
        new_tag: "sha-2222222".into(),
        changed: true,
        old_user: "1000:1000".into(),
        new_user: "root".into(),
        user_from: UserSource::ImageConfig,
        ..row("chapkit_rwanda_malaria_bym_model")
    };
    assert_eq!(
        user_line(&moved).as_deref(),
        Some("user 1000:1000 -> root (image config)")
    );

    let report = UpdateReport {
        registry: RegistryInfo {
            url: "https://example.test/registry.yaml".into(),
            provenance: Provenance::Network,
        },
        models: vec![moved.clone()],
        chap_core: Some(chap_core("latest", "latest", None)),
        components: Vec::new(),
        pulled: false,
        pulled_new: Vec::new(),
        restart_needed: Vec::new(),
        stack_running: Some(true),
        dry_run: true,
    };
    let text = plan_text(&report, &Out::default());
    assert!(
            text.contains(
                "  chapkit_rwanda_malaria_bym_model  v1.0.0 (sha-1111111) -> v1.1.0 (sha-2222222)\n    user 1000:1000 -> root (image config)\n"
            ),
            "{text}"
        );

    // An image that kept its user has nothing to report, and a row that is
    // not moving was never asked.
    let same = ModelUpdate {
        new_user: "1000:1000".into(),
        ..moved
    };
    assert_eq!(user_line(&same), None);
    assert_eq!(user_line(&row("chapkit_ewars_model")), None);

    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["models"][0]["old_user"], "1000:1000");
    assert_eq!(value["models"][0]["new_user"], "root");
    assert_eq!(value["models"][0]["user_from"], "image-config");
}

/// A row with everything a marketplace model's is, for the fields a test
/// does not care about.
fn row(id: &str) -> ModelUpdate {
    ModelUpdate::unchanged(
        id,
        &crate::project::EnabledModel {
            previous: None,
            reads_port: false,
            service_id: id.to_string(),
            image: format!("ghcr.io/chap-models/{id}"),
            image_tag: "sha-0000000".into(),
            image_digest: None,
            version: "1.0.0".into(),
            channel: Some(Channel::Stable),
            host_port: None,
            bind: None,
            data_dir: "/work/data".into(),
            user: "chapkit:chapkit".into(),
            user_from: Default::default(),
            platform: None,
            multi_arch: false,
            compose_file: format!("compose.{id}.yml"),
        },
    )
}

/// A [`ServiceCheck`] as `service_checks` would have built it.
fn check(service: &str, images: (&str, &str), hashes: (&str, &str)) -> ServiceCheck {
    ServiceCheck {
        service: service.to_string(),
        running_image: images.0.to_string(),
        pulled_image: images.1.to_string(),
        running_hash: hashes.0.to_string(),
        current_hash: hashes.1.to_string(),
    }
}

#[test]
fn a_service_is_stale_when_its_image_or_its_configuration_moved() {
    // Everything as it was created.
    assert!(!needs_restart(&check(
        "chap",
        ("sha256:a", "sha256:a"),
        ("h1", "h1")
    )));
    // The pull moved the tag under it.
    assert!(needs_restart(&check(
        "ocs",
        ("sha256:a", "sha256:b"),
        ("h1", "h1")
    )));
    // The files changed under it: a pin moved, a port, a secret.
    assert!(needs_restart(&check(
        "chap",
        ("sha256:a", "sha256:a"),
        ("h1", "h2")
    )));
    // Both at once is still one restart.
    assert!(needs_restart(&check(
        "chap",
        ("sha256:a", "sha256:b"),
        ("h1", "h2")
    )));
}

#[test]
fn an_answer_docker_did_not_give_is_never_a_reason_to_restart() {
    for c in [
        check("chap", ("", "sha256:b"), ("h1", "h1")),
        check("chap", ("sha256:a", ""), ("h1", "h1")),
        check("chap", ("sha256:a", "sha256:a"), ("", "h2")),
        check("chap", ("sha256:a", "sha256:a"), ("h1", "")),
        check("chap", ("", ""), ("", "")),
    ] {
        assert!(!needs_restart(&c), "{c:?}");
    }
}

#[test]
fn the_stale_services_keep_the_order_they_were_checked_in() {
    let checks = vec![
        check("chap", ("sha256:a", "sha256:b"), ("h1", "h1")),
        check("postgres", ("sha256:c", "sha256:c"), ("h2", "h2")),
        check("worker", ("sha256:a", "sha256:a"), ("h3", "h4")),
    ];
    assert_eq!(restart_needed(&checks), vec!["chap", "worker"]);
    assert!(restart_needed(&[]).is_empty());
}

/// Everything `service_checks` reads, for a two-service project whose
/// `chap` image moved under it and whose `redis` did not.
struct Maps {
    builds: BTreeMap<String, docker::ContainerBuild>,
    images: BTreeMap<String, String>,
    ids: BTreeMap<String, String>,
    hashes: BTreeMap<String, String>,
}

fn maps() -> Maps {
    let build = |image: &str, hash: &str| docker::ContainerBuild {
        image_id: image.to_string(),
        config_hash: hash.to_string(),
    };
    Maps {
        builds: BTreeMap::from([
            ("id-chap".to_string(), build("sha256:old", "h1")),
            ("id-redis".to_string(), build("sha256:r", "h2")),
        ]),
        images: BTreeMap::from([
            ("chap".to_string(), "ghcr.io/chap:latest".to_string()),
            ("redis".to_string(), "valkey:8".to_string()),
        ]),
        ids: BTreeMap::from([
            ("ghcr.io/chap:latest".to_string(), "sha256:new".to_string()),
            ("valkey:8".to_string(), "sha256:r".to_string()),
        ]),
        hashes: BTreeMap::from([
            ("chap".to_string(), "h1".to_string()),
            ("redis".to_string(), "h2".to_string()),
        ]),
    }
}

fn container(service: &str, id: &str) -> docker::Container {
    docker::Container {
        service: service.to_string(),
        id: id.to_string(),
        state: "running".to_string(),
        ..docker::Container::default()
    }
}

#[test]
fn every_running_service_is_paired_with_what_it_would_be_made_of_today() {
    let m = maps();
    let running = vec![container("chap", "id-chap"), container("redis", "id-redis")];
    let checks = service_checks(&running, &m.builds, &m.images, &m.ids, &m.hashes);
    assert_eq!(checks.len(), 2);
    assert_eq!(
        checks[0],
        check("chap", ("sha256:old", "sha256:new"), ("h1", "h1"))
    );
    assert_eq!(
        checks[1],
        check("redis", ("sha256:r", "sha256:r"), ("h2", "h2"))
    );
    assert_eq!(restart_needed(&checks), vec!["chap"]);
}

#[test]
fn a_container_docker_would_not_describe_costs_only_its_own_row() {
    let m = maps();
    // A service the files no longer mention, and one `docker inspect`
    // said nothing about: neither is a reason to claim a restart.
    let running = vec![
        container("gone", "id-gone"),
        container("chap", "id-unknown"),
        // A stopped container is not a running service.
        docker::Container {
            state: "exited".to_string(),
            ..container("redis", "id-redis")
        },
    ];
    let checks = service_checks(&running, &m.builds, &m.images, &m.ids, &m.hashes);
    assert_eq!(checks.len(), 2, "the exited one is left out");
    assert!(restart_needed(&checks).is_empty(), "{checks:?}");
}

#[test]
fn the_pull_is_new_only_when_the_image_id_moved() {
    let images = BTreeMap::from([
        ("ocs".to_string(), "ghcr.io/ocs:main".to_string()),
        ("s3".to_string(), "minio:latest".to_string()),
        ("chap".to_string(), "ghcr.io/chap:v2.3.1".to_string()),
        ("gone".to_string(), "ghcr.io/nowhere:1".to_string()),
    ]);
    let before = BTreeMap::from([
        ("ghcr.io/ocs:main".to_string(), "sha256:a".to_string()),
        ("minio:latest".to_string(), "sha256:b".to_string()),
    ]);
    let after = BTreeMap::from([
        // The moving tag moved.
        ("ghcr.io/ocs:main".to_string(), "sha256:c".to_string()),
        // And this one pointed at the same image as yesterday.
        ("minio:latest".to_string(), "sha256:b".to_string()),
        // This one was not on the machine at all before.
        ("ghcr.io/chap:v2.3.1".to_string(), "sha256:d".to_string()),
    ]);
    // `gone` is absent from both: the pull did not fetch it, which is not
    // the same as it having fetched something new.
    assert_eq!(pulled_new(&images, &before, &after), vec!["chap", "ocs"]);
}

/// A [`ChapCoreUpdate`] as `plan_chap_core` would have built it.
fn chap_core(old: &str, new: &str, latest: Option<&str>) -> ChapCoreUpdate {
    ChapCoreUpdate {
        old_tag: old.to_string(),
        new_tag: new.to_string(),
        changed: old != new,
        moving: chapcore::is_moving_tag(old),
        latest_release: latest.map(str::to_string),
        compose_source: ComposeSource::Embedded,
        requested: None,
        compose_ref: (old != new).then(|| new.to_string()),
        backwards: chapcore::is_backwards(old, new),
    }
}

/// A project whose chap-core pin is `tag`.
fn pinned_to(tag: &str) -> Project {
    Project {
        dir: std::path::PathBuf::from("/nonexistent"),
        state: ProjectState {
            chap_image_tag: tag.to_string(),
            ..ProjectState::default()
        },
    }
}

#[test]
fn a_release_pin_moves_to_a_newer_release_and_no_further() {
    let plan = plan_chap_core(&pinned_to("v2.3.0"), Some("v2.3.1".into()), false, None);
    assert!(plan.changed && !plan.moving);
    assert_eq!(
        (plan.old_tag.as_str(), plan.new_tag.as_str()),
        ("v2.3.0", "v2.3.1")
    );
    assert_eq!(plan.latest_release.as_deref(), Some("v2.3.1"));

    // Already current, and never backwards.
    let plan = plan_chap_core(&pinned_to("v2.3.1"), Some("v2.3.1".into()), false, None);
    assert!(!plan.changed);
    let plan = plan_chap_core(&pinned_to("v2.4.0"), Some("v2.3.1".into()), false, None);
    assert!(!plan.changed, "a release ahead of the newest one stays");

    // A failed lookup leaves everything where it is.
    let plan = plan_chap_core(&pinned_to("v2.3.0"), None, false, None);
    assert!(!plan.changed);
    assert_eq!(plan.new_tag, "v2.3.0");
}

#[test]
fn a_moving_tag_only_moves_when_asked_to() {
    for tag in ["latest", "master", "dev"] {
        let plan = plan_chap_core(&pinned_to(tag), Some("v2.3.1".into()), false, None);
        assert!(!plan.changed, "{tag} follows itself");
        assert!(plan.moving, "{tag}");
        assert_eq!(plan.new_tag, tag);

        let plan = plan_chap_core(&pinned_to(tag), Some("v2.3.1".into()), true, None);
        assert!(plan.changed, "--pin-chap-core pins {tag}");
        assert_eq!(plan.new_tag, "v2.3.1");
        assert!(plan.moving, "the tag it came from was a moving one");
        // `latest` is the newest release, so pinning it is the same image;
        // `dev` and `master` are ahead of every release.
        assert_eq!(plan.backwards, tag != "latest", "{tag}");
    }

    // Nothing to pin it to: the flag cannot invent a release.
    let plan = plan_chap_core(&pinned_to("latest"), None, true, None);
    assert!(!plan.changed);
}

#[test]
fn a_tag_that_is_neither_a_release_nor_moving_is_left_alone() {
    let plan = plan_chap_core(&pinned_to("sha-abcdef0"), Some("v2.3.1".into()), true, None);
    assert!(!plan.changed && !plan.moving);
    assert_eq!(plan.new_tag, "sha-abcdef0");
}

#[test]
fn the_release_lookup_is_skipped_when_it_cannot_matter() {
    assert!(needs_release_lookup("v2.3.0", false, None));
    assert!(needs_release_lookup("v2.3.0", true, None));
    assert!(!needs_release_lookup("latest", false, None));
    assert!(needs_release_lookup("latest", true, None));
    assert!(!needs_release_lookup("sha-abcdef0", false, None));
    assert!(!needs_release_lookup("sha-abcdef0", true, None));

    // A run that was told where to go asks for one reason only: `latest`
    // is an image tag, and the compose file that goes with it is the one
    // the newest release publishes.
    assert!(needs_release_lookup("v2.3.0", false, Some("latest")));
    assert!(!needs_release_lookup("v2.3.0", false, Some("dev")));
    assert!(!needs_release_lookup("dev", false, Some("v2.3.1")));
    assert!(!needs_release_lookup("v2.3.0", false, Some("v2.3.1")));
}

#[test]
fn a_requested_tag_decides_where_the_pin_goes() {
    // Forward, backwards and sideways: the tag that was asked for wins,
    // and the newest release does not enter into it.
    let plan = plan_chap_core(
        &pinned_to("v2.3.1"),
        Some("v2.3.1".into()),
        false,
        Some("dev"),
    );
    assert!(plan.changed && !plan.moving && !plan.backwards);
    assert_eq!(plan.new_tag, "dev");
    assert_eq!(plan.compose_ref.as_deref(), Some("dev"));
    assert_eq!(plan.requested.as_deref(), Some("dev"));

    let plan = plan_chap_core(&pinned_to("dev"), None, false, Some("v2.3.1"));
    assert!(plan.changed && plan.moving && plan.backwards);
    assert_eq!(plan.new_tag, "v2.3.1");
    assert_eq!(plan.compose_ref.as_deref(), Some("v2.3.1"));

    let plan = plan_chap_core(&pinned_to("v2.3.1"), None, false, Some("v2.3.0"));
    assert!(plan.changed && plan.backwards, "an older release");

    let plan = plan_chap_core(&pinned_to("v2.3.0"), None, false, Some("v2.3.1"));
    assert!(plan.changed && !plan.backwards, "a newer release");

    // The tag it already runs: nothing moves, and nothing is fetched.
    let plan = plan_chap_core(&pinned_to("dev"), None, false, Some("dev"));
    assert!(!plan.changed && !plan.backwards);
    assert_eq!(plan.compose_ref, None);
    assert_eq!(
        chap_core_line(&plan),
        "chap-core  dev  already the pin, nothing to switch"
    );
}

#[test]
fn the_compose_file_of_latest_is_the_one_the_newest_release_publishes() {
    // `latest` is an image tag, not a ref of the repository.
    assert_eq!(
        compose_ref("latest", Some("v2.3.1")).as_deref(),
        Some("v2.3.1")
    );
    assert_eq!(compose_ref("latest", None), None);
    // Everything else is a ref: a release tag or a branch.
    assert_eq!(compose_ref("v2.3.1", None).as_deref(), Some("v2.3.1"));
    assert_eq!(compose_ref("dev", Some("v2.3.1")).as_deref(), Some("dev"));
    assert_eq!(compose_ref("master", None).as_deref(), Some("master"));

    let plan = plan_chap_core(
        &pinned_to("v2.3.0"),
        Some("v2.3.1".into()),
        false,
        Some("latest"),
    );
    assert_eq!(plan.new_tag, "latest");
    assert_eq!(plan.compose_ref.as_deref(), Some("v2.3.1"));
    // The deployment has the compose file of v2.3.0, so this run brings
    // the one of v2.3.1 and the row says so.
    let fetched = |plan: &ChapCoreUpdate, tag: &str| ChapCoreUpdate {
        compose_source: ComposeSource::Fetched {
            url: chapcore::compose_url(tag),
            tag: tag.into(),
            sha256: "a".repeat(64),
        },
        ..plan.clone()
    };
    assert_eq!(
        chap_core_line(&fetched(&plan, "v2.3.0")),
        "chap-core  v2.3.0 -> latest  (compose.ghcr.yml too)"
    );
    // From v2.3.1 to `latest`, which is v2.3.1: the compose file stays.
    let same = plan_chap_core(
        &pinned_to("v2.3.1"),
        Some("v2.3.1".into()),
        false,
        Some("latest"),
    );
    assert_eq!(
        chap_core_line(&fetched(&same, "v2.3.1")),
        "chap-core  v2.3.1 -> latest"
    );
    // From `latest` to a branch: the branch brings its own compose file.
    let branch = plan_chap_core(&pinned_to("latest"), None, false, Some("master"));
    assert_eq!(
        chap_core_line(&fetched(&branch, "v2.3.1")),
        "chap-core  latest -> master  (compose.ghcr.yml too)"
    );
}

#[test]
fn a_switch_reports_the_way_the_rest_of_the_update_does() {
    let plan = plan_chap_core(&pinned_to("v2.3.1"), None, false, Some("dev"));
    assert_eq!(
        chap_core_line(&plan),
        "chap-core  v2.3.1 -> dev  (compose.ghcr.yml too)"
    );
    assert_eq!(
        updated_phrase(0, Some(("v2.3.1", "dev")), &[]).as_deref(),
        Some("chap-core v2.3.1 -> dev")
    );
    assert_eq!(
        closing_text(
            updated_phrase(
                0,
                Some(("v2.3.1", "dev")),
                &["chap".into(), "worker".into()]
            )
            .as_deref(),
            &["chap".to_string(), "worker".to_string()],
            Some(true)
        ),
        "updated chap-core v2.3.1 -> dev and pulled new images for chap, worker; \
             restart needed: chap, worker\nrun `chap-devops restart` to apply\n"
    );
    // A dry run says the same thing in the conditional, and nothing about
    // restarting: nothing was written for anything to be behind.
    let text = dry_run_text(updated_phrase(0, Some(("v2.3.1", "dev")), &[]).as_deref());
    assert_eq!(
        text,
        "would update chap-core v2.3.1 -> dev\nhint: `chap-devops update` does it\n"
    );
    assert!(!text.contains("restart"));
}

#[test]
fn a_pull_that_fails_after_a_switch_says_where_the_pin_is() {
    assert_eq!(
        pull_failed_after_switch("v2.3.1", "dev"),
        "the pull failed after the pins moved; chap-core is now pinned to dev, and \
             `chap-devops update --chap-tag v2.3.1 --yes` puts it back"
    );
}

#[test]
fn the_backwards_warning_names_the_move_and_the_way_out() {
    assert_eq!(
        backwards_warning("dev", "v2.3.1"),
        "moving chap-core from dev to v2.3.1 can run an older schema against a database \
             migrated by the newer one; run `chap-devops backup create` first"
    );
}

/// The DHIS2 warning is its own line, not the one `components enable`
/// prints: there is no move to name here, and the risk is the pull itself.
#[test]
fn the_dhis2_pull_warning_names_the_archive_and_no_move() {
    let warning = dhis2_pull_warning("dhis2/core", "2.42");
    assert_eq!(
        warning,
        "this pull can bring a newer `dhis2/core:2.42`, and a newer DHIS2 migrates \
             `dhis2_db` irreversibly on the next `chap-devops up`: run `chap-devops backup create` first - \
             an older image on a migrated database answers healthy while every API request 404s"
    );
    // Not the `components enable dhis2` wording, which names two tags.
    assert!(!warning.contains("moves from"), "{warning}");
}

/// Every enabled component gets a row, in the order they are rendered, and
/// DHIS2's tag comes from the component rather than from a constant.
#[test]
fn the_component_rows_cover_every_enabled_component() {
    let dir = tempfile::tempdir().expect("a directory");
    let mut state = crate::project::ProjectState::default();
    state.components.ocs.enabled = true;
    state.components.s3.enabled = true;
    state.components.dhis2.enabled = true;
    state.components.dhis2.image_tag = "2.41".to_string();
    let project = Project {
        dir: dir.path().to_path_buf(),
        state,
    };

    let rows = plan_components(&project);
    let named: Vec<(&str, &str, &str, bool)> = rows
        .iter()
        .map(|row| {
            (
                row.name.as_str(),
                row.image.as_str(),
                row.tag.as_str(),
                row.pinned,
            )
        })
        .collect();
    assert_eq!(
        named,
        vec![
            ("ocs", crate::components::OCS_IMAGE, "main", false),
            ("s3", crate::components::S3_IMAGE, "latest", false),
            ("dhis2", crate::components::DHIS2_IMAGE, "2.41", false),
        ]
    );
    assert_eq!(
        component_line(&rows[2]),
        "dhis2  2.41  moving tag, re-pulled"
    );

    // An active `.env` line is the operator's own pin, and is reported as
    // such rather than quietly overridden - exactly as OCS's is.
    std::fs::write(
        project.dir.join(crate::project::ENV_FILE),
        format!("{}=2.42.1.0\n", crate::components::DHIS2_TAG_ENV_VAR),
    )
    .expect("an .env");
    let rows = plan_components(&project);
    assert_eq!(rows[2].tag, "2.42.1.0");
    assert!(rows[2].pinned);
    assert_eq!(
        component_line(&rows[2]),
        "dhis2  2.42.1.0  pinned in .env, re-pulled at that tag"
    );
}

#[test]
fn a_requested_tag_is_checked_before_anything_happens() {
    // Offline: nothing can be looked up, and the tag is taken as given.
    let offline = std::time::Duration::from_secs(1);
    assert_eq!(
        check_chap_tag("v9.9.9", true, offline, &mut Vec::new()).unwrap(),
        "v9.9.9"
    );
    // A moving tag needs no lookup at all, online or not.
    for tag in chapcore::MOVING_TAGS {
        assert_eq!(
            &check_chap_tag(tag, false, offline, &mut Vec::new()).unwrap(),
            tag
        );
    }
    // Nor does a tag that is neither, which is recorded as an exact pin.
    let mut warnings = Vec::new();
    assert_eq!(
        check_chap_tag("sha-fa880a1", false, offline, &mut warnings).unwrap(),
        "sha-fa880a1"
    );
    // The warning that it will never move comes only once the pin moves, so
    // the check itself says nothing: the ghcr check after it can still fail.
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(exact_pin_warning("sha-fa880a1", false).contains("it is recorded as an exact pin"));
    assert!(exact_pin_warning("sha-fa880a1", true).contains("it would be recorded as an exact"));
    // An empty tag is the one thing this refuses without asking anyone.
    let err = check_chap_tag("", false, offline, &mut Vec::new()).expect_err("nothing to move to");
    assert!(err.to_string().contains("--chap-tag needs a tag"), "{err}");
}

/// The releases a listing is built from, newest first.
fn releases() -> Vec<chapcore::Release> {
    vec![
        chapcore::Release {
            tag: "v2.3.1".into(),
            published: "2026-09-21".into(),
        },
        chapcore::Release {
            tag: "v2.3.0".into(),
            published: "2026-09-11".into(),
        },
    ]
}

#[test]
fn the_tag_listing_marks_the_pin_and_the_newest_release() {
    let mut published = BTreeMap::new();
    published.insert("dev".to_string(), "2026-09-24".to_string());
    published.insert("latest".to_string(), "2026-09-21".to_string());

    let rows = tag_rows("dev", &releases(), &published, Some("7f3a1c2e9b4d"));
    let cells: Vec<(&str, &str, &str, String)> = rows
        .iter()
        .map(|row| {
            (
                row.tag.as_str(),
                row.kind,
                row.published.as_deref().unwrap_or("-"),
                tag_note(row),
            )
        })
        .collect();
    assert_eq!(
        cells,
        vec![
            (
                "dev",
                "moving",
                "2026-09-24",
                "pinned (moving, running 7f3a1c2e9b4d)".to_string()
            ),
            ("master", "moving", "-", "-".to_string()),
            ("latest", "moving", "2026-09-21", "-".to_string()),
            ("v2.3.1", "release", "2026-09-21", "newest".to_string()),
            ("v2.3.0", "release", "2026-09-11", "-".to_string()),
        ]
    );

    // A release pin is marked where it sits, and the newest release is
    // still named as such.
    let rows = tag_rows("v2.3.0", &releases(), &BTreeMap::new(), None);
    let note = |tag: &str| tag_note(rows.iter().find(|row| row.tag == tag).expect("a row"));
    assert_eq!(note("v2.3.0"), "pinned");
    assert_eq!(note("v2.3.1"), "newest");
    // No digest for a release pin: the tag already says which image it is.
    let rows = tag_rows(
        "v2.3.0",
        &releases(),
        &BTreeMap::new(),
        Some("7f3a1c2e9b4d"),
    );
    assert!(rows.iter().all(|row| row.running.is_none()));

    // The newest release wins on version, not on the order of the list.
    let listed = vec![releases()[1].clone(), releases()[0].clone()];
    let rows = tag_rows("dev", &listed, &BTreeMap::new(), None);
    let releases_only: Vec<&str> = rows
        .iter()
        .filter(|row| row.kind == "release")
        .map(|row| row.tag.as_str())
        .collect();
    assert_eq!(releases_only, vec!["v2.3.1", "v2.3.0"]);
    assert!(rows.iter().find(|row| row.tag == "v2.3.1").unwrap().newest);
}

#[test]
fn a_pin_the_listing_does_not_hold_gets_a_row_of_its_own() {
    // Offline, or a release older than the page: the deployment is still
    // somewhere, and the table says where.
    let rows = tag_rows("sha-fa880a1", &[], &BTreeMap::new(), None);
    let last = rows.last().expect("a row for the pin");
    assert_eq!((last.tag.as_str(), last.kind), ("sha-fa880a1", "exact"));
    assert_eq!(tag_note(last), "pinned");
    assert_eq!(rows.len(), chapcore::MOVING_TAGS.len() + 1);

    let rows = tag_rows("v1.0.0", &releases(), &BTreeMap::new(), None);
    assert_eq!(rows.last().unwrap().tag, "v1.0.0");
    assert_eq!(rows.last().unwrap().kind, "release");
    assert_eq!(tag_note(rows.last().unwrap()), "pinned");

    let list = TagList {
        pin: "dev".to_string(),
        releases_listed: false,
        tags: rows,
    };
    let mut lines = Report::default();
    tag_closing(&list, &mut lines);
    assert_eq!(
        lines.text(),
        "chap-core is pinned to dev\nhint: `chap-devops update --chap-tag <TAG>` moves it\n"
    );
}

#[test]
fn the_chap_core_line_says_what_happened() {
    let mut moved = chap_core("v2.3.0", "v2.3.1", Some("v2.3.1"));
    // The run fetches the compose file of v2.3.1, which the deployment does
    // not have yet.
    assert_eq!(
        chap_core_line(&moved),
        "chap-core  v2.3.0 -> v2.3.1  (compose.ghcr.yml too)"
    );
    moved.compose_source = ComposeSource::Fetched {
        url: chapcore::compose_url("v2.3.1"),
        tag: "v2.3.1".into(),
        sha256: "a".repeat(64),
    };
    assert_eq!(chap_core_line(&moved), "chap-core  v2.3.0 -> v2.3.1");
    let current = chap_core("v2.3.1", "v2.3.1", Some("v2.3.1"));
    assert_eq!(
        chap_core_line(&current),
        "chap-core  v2.3.1  unchanged, the newest release"
    );
    let ahead = chap_core("v2.4.0", "v2.4.0", Some("v2.3.1"));
    assert_eq!(
        chap_core_line(&ahead),
        "chap-core  v2.4.0  unchanged (newest release: v2.3.1)"
    );

    let unknown = chap_core("sha-abcdef0", "sha-abcdef0", None);
    assert_eq!(
        chap_core_line(&unknown),
        "chap-core  sha-abcdef0  unchanged"
    );
}

#[test]
fn the_updated_phrase_names_only_what_actually_moved() {
    assert_eq!(updated_phrase(0, None, &[]), None);
    assert_eq!(updated_phrase(1, None, &[]).as_deref(), Some("1 model pin"));
    assert_eq!(
        updated_phrase(3, None, &[]).as_deref(),
        Some("3 model pins")
    );
    assert_eq!(
        updated_phrase(0, Some(("v2.3.0", "v2.3.1")), &[]).as_deref(),
        Some("chap-core v2.3.0 -> v2.3.1")
    );
    assert_eq!(
        updated_phrase(0, None, &["ocs".to_string()]).as_deref(),
        Some("pulled a new image for ocs")
    );
    assert_eq!(
        updated_phrase(
            2,
            Some(("v2.3.0", "v2.3.1")),
            &["chap".to_string(), "worker".to_string()]
        )
        .as_deref(),
        Some("2 model pins, chap-core v2.3.0 -> v2.3.1 and pulled new images for chap, worker")
    );
}

/// The closing lines of a run, as the tests read them.
fn closing_text(what: Option<&str>, restart_needed: &[String], running: Option<bool>) -> String {
    let mut lines = Report::default();
    closing(&mut lines, what, restart_needed, running);
    lines.text()
}

/// The closing lines of a dry run, as the tests read them.
fn dry_run_text(what: Option<&str>) -> String {
    let mut lines = Report::default();
    dry_run(&mut lines, what);
    lines.text()
}

#[test]
fn the_closing_lines_are_one_of_four_things() {
    let stale = ["chap".to_string(), "worker".to_string()];

    // Nothing moved, nothing is stale: the short answer.
    assert_eq!(closing_text(None, &[], Some(true)), "already up to date\n");
    // A moving tag that pulled the image this machine already had counts
    // as nothing moving, because `updated` never names it.
    assert_eq!(closing_text(None, &[], Some(false)), "already up to date\n");

    // Something moved and running services are behind it: the restart is
    // the step the reader must do next.
    assert_eq!(
        closing_text(Some("1 model pin"), &stale, Some(true)),
        "updated 1 model pin; restart needed: chap, worker\nrun `chap-devops restart` to apply\n"
    );
    // Stale without this run having moved anything: someone edited `.env`
    // and never applied it. Still worth saying.
    assert_eq!(
        closing_text(None, &stale, Some(true)),
        "already up to date; restart needed: chap, worker\nrun `chap-devops restart` to apply\n"
    );

    // Something moved and there is nothing running to be behind it.
    assert_eq!(
        closing_text(Some("chap-core v2.3.0 -> v2.3.1"), &[], Some(false)),
        "updated chap-core v2.3.0 -> v2.3.1\n\
         hint: Chap is not running; `chap-devops up` starts the new versions\n"
    );
    // Something moved, the deployment is up, and none of it was affected.
    assert_eq!(
        closing_text(Some("1 model pin"), &[], Some(true)),
        "updated 1 model pin\nhint: nothing running needs a restart\n"
    );
    // A run that only pulled uses its own verb: the case a moving tag such
    // as `ocs:main` is in whenever upstream has published since.
    assert_eq!(
        closing_text(
            updated_phrase(0, None, &["ocs".to_string()]).as_deref(),
            &["ocs".to_string()],
            Some(true)
        ),
        "pulled a new image for ocs; restart needed: ocs\nrun `chap-devops restart` to apply\n"
    );
    // Docker would not say what is running, so neither do we.
    assert_eq!(
        closing_text(Some("1 model pin"), &[], None),
        "updated 1 model pin\nrun `chap-devops restart` to apply it to what is running\n"
    );
}

#[test]
fn a_dry_run_does_not_say_a_moving_tag_was_pulled() {
    let line = "ocs  main  moving tag, re-pulled".to_string();
    assert_eq!(
        would(line.clone(), true),
        "ocs  main  moving tag, would be re-pulled"
    );
    assert_eq!(would(line.clone(), false), line);
}

#[test]
fn a_dry_run_says_what_would_happen_and_claims_nothing_else() {
    assert_eq!(dry_run_text(None), "already up to date\n");
    assert_eq!(
        dry_run_text(Some("1 model pin")),
        "would update 1 model pin\nhint: `chap-devops update` does it\n"
    );
    // A dry run pulls nothing, so it never claims a restart is needed.
    assert!(!dry_run_text(Some("1 model pin")).contains("restart"));
}

#[test]
fn a_failed_release_lookup_says_where_the_pin_goes() {
    // Without --chap-tag the pin stays.
    assert!(lookup_failed("offline", "v2.4.0", None, false).ends_with("pin stays at `v2.4.0`"),);
    // With --chap-tag latest the pin moves anyway, and the warning says so.
    let text = lookup_failed("offline", "v2.4.0", Some("latest"), false);
    assert!(text.contains("the pin moves to `latest`"), "{text}");
    assert!(!text.contains("stays"), "{text}");
    let text = lookup_failed("offline", "v2.4.0", Some("latest"), true);
    assert!(text.contains("the pin would move to `latest`"), "{text}");
    // A tag that is already the pin needs no lookup at all.
    assert!(!needs_release_lookup("latest", false, Some("latest")));
    assert!(needs_release_lookup("v2.4.0", false, Some("latest")));
}

/// A moved pin keeps the one before it; a second move before `up` keeps the
/// first one, and a model from `models add` or a pin that did not move gets
/// none.
#[test]
fn a_moved_pin_keeps_the_one_before_it() {
    let model = || crate::project::EnabledModel {
        previous: None,
        reads_port: false,
        service_id: "m".to_string(),
        image: "ghcr.io/chap-models/m".into(),
        image_tag: "sha-1111111".into(),
        image_digest: None,
        version: "1.0.0".into(),
        channel: Some(Channel::Stable),
        host_port: None,
        bind: None,
        data_dir: "/work/data".into(),
        user: "chapkit:chapkit".into(),
        user_from: Default::default(),
        platform: None,
        multi_arch: false,
        compose_file: "compose.m.yml".into(),
    };

    let mut entry = model();
    record_previous(&mut entry, "sha-2222222", false);
    let previous = entry.previous.clone().expect("a previous pin");
    assert_eq!(
        (previous.version.as_str(), previous.image_tag.as_str()),
        ("1.0.0", "sha-1111111")
    );
    entry.version = "1.1.0".into();
    entry.image_tag = "sha-2222222".into();
    record_previous(&mut entry, "sha-3333333", false);
    assert_eq!(entry.previous, Some(previous));

    let mut manual = model();
    record_previous(&mut manual, "sha-2222222", true);
    assert_eq!(manual.previous, None);
    let mut same = model();
    record_previous(&mut same, "sha-1111111", false);
    assert_eq!(same.previous, None);

    // The same tag under a new digest is a move, and the digest is kept.
    let digest = format!("sha256:{}", "f".repeat(64));
    let mut pinned = model();
    pinned.image_digest = Some(digest.clone());
    record_previous(&mut pinned, &format!("sha-1111111@{digest}"), false);
    assert_eq!(pinned.previous, None);
    record_previous(&mut pinned, "sha-1111111", false);
    assert_eq!(pinned.previous.map(|p| p.image_digest), Some(Some(digest)));
}
