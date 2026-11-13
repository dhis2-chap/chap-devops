use super::naming::*;
use super::store::*;
use super::*;
use crate::components::COMPONENTS_FILE;
use crate::error::ChapError;

#[test]
fn a_manual_models_source_is_what_models_add_takes() {
    let mut model = ManualModel {
        repository: None,
        image: "my-model".into(),
        tag: "dev".into(),
        ..manual()
    };
    assert_eq!(model.source(), "my-model:dev");
    model.tag = "@sha256:abc".into();
    assert_eq!(model.source(), "my-model@sha256:abc");
    model.repository = Some("https://github.com/o/r".into());
    assert_eq!(model.source(), "https://github.com/o/r");
}

fn enabled(port: Option<u16>) -> EnabledModel {
    EnabledModel {
        previous: None,
        reads_port: false,
        service_id: "chapkit-ewars-model".into(),
        image: "ghcr.io/chap-models/chapkit_ewars_model".into(),
        image_tag: "sha-fa880a1".into(),
        image_digest: None,
        version: "1.0.0".into(),
        channel: Some(Channel::Stable),
        host_port: port,
        bind: None,
        data_dir: "/app/data".into(),
        user: "chapkit:chapkit".into(),
        user_from: Default::default(),
        platform: Some("linux/amd64".into()),
        multi_arch: false,
        compose_file: "compose.chapkit-ewars-model.yml".into(),
    }
}

/// The definition `chap-devops models add https://github.com/chap-models/\
/// chapkit_ghr_model` records.
fn manual() -> ManualModel {
    ManualModel {
        reads_port: false,
        service_id: "chapkit-ghr-model".into(),
        display_name: "chapkit_ghr_model".into(),
        repository: Some("https://github.com/chap-models/chapkit_ghr_model".into()),
        image: "ghcr.io/chap-models/chapkit_ghr_model".into(),
        tag: "sha-b1d6c31".into(),
        commit: Some("b1d6c31f4b2f0d8a0f4c6e2a5e9d3c7b8a1f0e2d".into()),
        follow: Some("main".into()),
        data_dir: Some("/work/data".into()),
        user: Some("10001:10001".into()),
        runtime_amd64: true,
        added: "2026-09-24".into(),
    }
}

#[test]
fn a_manual_definition_round_trips_in_a_file_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            manual: ManualModels::from([("chapkit_ghr_model".to_string(), manual())]),
            ..ProjectState::default()
        },
    };
    project.save().unwrap();

    let path = dir.path().join(CHAP_DEVOPS_DIR).join(MANUAL_MODELS_FILE);
    let body = std::fs::read_to_string(&path).expect("models-manual.yaml is written");
    assert!(body.starts_with(MANAGED_HEADER), "{body}");
    assert!(body.contains("\nchapkit_ghr_model:\n"), "{body}");
    assert!(body.contains("  follow: main\n"), "{body}");
    assert!(body.contains("  added: 2026-09-24\n"), "{body}");
    // A definition is not an enablement: models.yaml stays empty.
    let models =
        std::fs::read_to_string(dir.path().join(CHAP_DEVOPS_DIR).join(MODELS_FILE)).unwrap();
    assert!(!models.contains("chapkit_ghr_model"), "{models}");

    let loaded = Project::load(dir.path()).unwrap();
    assert_eq!(loaded.state.manual["chapkit_ghr_model"], manual());
    assert!(loaded.state.models.is_empty());
}

#[test]
fn a_project_that_added_no_model_of_its_own_has_no_such_file() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState::default(),
    };
    project.save().unwrap();
    let path = dir.path().join(CHAP_DEVOPS_DIR).join(MANUAL_MODELS_FILE);
    assert!(!path.exists(), "an empty file would be one to explain");
    assert!(Project::load(dir.path()).unwrap().state.manual.is_empty());

    // A file that exists keeps being written, even once it is empty: the
    // operator can see it, so it must not turn stale.
    std::fs::write(&path, "# nothing added\n").unwrap();
    assert!(Project::load(dir.path()).unwrap().state.manual.is_empty());
    project.save().unwrap();
    assert!(path.is_file());
}

/// Every path that resolves a model - `enable`, `sync`, `update`, the
/// browser - asks a channel or an exact version for a [`Version`]. A
/// manual entry has one version, so both answers are it.
#[test]
fn the_synthesised_entry_resolves_every_selector_to_the_recorded_tag() {
    use crate::registry::{AssessedStatus, Channel as C, Kind, VersionSelector};

    let model = manual().to_model("chapkit_ghr_model");
    assert!(model.manual);
    assert_eq!(model.id, "chapkit_ghr_model");
    assert_eq!(model.service_id, "chapkit-ghr-model");
    assert_eq!(model.kind, Kind::Model);
    assert!(!model.is_starter());
    assert_eq!(model.assessed_status, AssessedStatus::Gray);
    assert_eq!(
        model.summary,
        "added manually from https://github.com/chap-models/chapkit_ghr_model"
    );
    for channel in [C::Stable, C::Latest] {
        let version = model.resolve(&VersionSelector::Channel(channel)).unwrap();
        assert_eq!(version.image_tag, "sha-b1d6c31", "{channel:?}");
        assert_eq!(version.version, "sha-b1d6c31", "{channel:?}");
    }
    let exact = model
        .resolve(&VersionSelector::Exact("sha-b1d6c31".into()))
        .unwrap();
    assert_eq!(model.image_ref(exact), manual().image_ref());
    // amd64 only, so it reports the R-INLA runtime the overlays pin for.
    assert!(model.needs_amd64());

    // An entry added from an image names the image where a repository
    // would have been, and says nothing about the runtime.
    let model = ManualModel {
        repository: None,
        runtime_amd64: false,
        ..manual()
    }
    .to_model("chapkit_ghr_model");
    assert_eq!(
        model.summary,
        "added manually from ghcr.io/chap-models/chapkit_ghr_model:sha-b1d6c31"
    );
    assert_eq!(
        model.source.repository,
        model.source.image.clone() + ":sha-b1d6c31"
    );
    assert!(!model.needs_amd64());
}

#[test]
fn a_digest_pin_synthesises_a_digest_reference() {
    let entry = ManualModel {
        tag: format!("@sha256:{}", "c".repeat(64)),
        follow: None,
        ..manual()
    };
    assert_eq!(
        entry.image_ref(),
        format!(
            "ghcr.io/chap-models/chapkit_ghr_model@sha256:{}",
            "c".repeat(64)
        )
    );
    let model = entry.to_model("chapkit_ghr_model");
    assert_eq!(model.versions[0].image_tag, entry.tag);
}

#[test]
fn save_then_load_round_trips_both_files() {
    let dir = tempfile::tempdir().unwrap();
    let state = ProjectState {
        chap_image_tag: "v1.2.3".into(),
        rendered_files: vec!["compose.marketplace.yml".into()],
        models: BTreeMap::from([("chapkit_ewars_model".to_string(), enabled(Some(5001)))]),
        ..ProjectState::default()
    };
    let project = Project {
        dir: dir.path().to_path_buf(),
        state,
    };
    project.save().unwrap();

    let chap_devops = dir.path().join(CHAP_DEVOPS_DIR);
    let project_body = std::fs::read_to_string(chap_devops.join(PROJECT_FILE)).unwrap();
    assert!(project_body.starts_with(MANAGED_HEADER), "{project_body}");
    assert!(project_body.contains("\nschema_version: 1\n"));
    assert!(project_body.contains("chap_image_tag: v1.2.3"));
    assert!(project_body.contains("\napi_port: 8700\n"));
    assert!(
        !project_body.contains("models:"),
        "models live in their own file"
    );
    let models_body = std::fs::read_to_string(chap_devops.join(MODELS_FILE)).unwrap();
    assert!(models_body.starts_with(MANAGED_HEADER), "{models_body}");
    assert!(models_body.contains("\nchapkit_ewars_model:\n"));
    assert!(models_body.contains("host_port: 5001"));
    assert!(models_body.contains("channel: stable"));
    assert!(
        std::fs::read_dir(&chap_devops).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")),
        "temp files are renamed away"
    );

    let loaded = Project::load(dir.path()).unwrap();
    assert_eq!(loaded.dir, dir.path());
    assert_eq!(loaded.state.chap_image_tag, "v1.2.3");
    assert_eq!(loaded.state.port_range, DEFAULT_PORT_RANGE);
    assert_eq!(loaded.state.api_port, DEFAULT_API_PORT);
    assert_eq!(loaded.state.rendered_files, vec!["compose.marketplace.yml"]);
    assert_eq!(
        loaded.state.models["chapkit_ewars_model"],
        enabled(Some(5001))
    );
}

/// The user and where it came from are recorded.
#[test]
fn the_resolved_user_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let entry = EnabledModel {
        previous: None,
        user: "root".into(),
        user_from: UserSource::ImageConfig,
        ..enabled(None)
    };
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            models: BTreeMap::from([("chapkit_ewars_model".to_string(), entry.clone())]),
            ..ProjectState::default()
        },
    };
    project.save().unwrap();
    let path = dir.path().join(CHAP_DEVOPS_DIR).join(MODELS_FILE);
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("  user: root\n"), "{body}");
    assert!(body.contains("  user_from: image-config\n"), "{body}");
    assert_eq!(
        Project::load(dir.path()).unwrap().state.models["chapkit_ewars_model"],
        entry
    );
}

#[test]
fn a_model_with_no_published_port_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            models: BTreeMap::from([("chapkit_ewars_model".to_string(), enabled(None))]),
            ..ProjectState::default()
        },
    };
    project.save().unwrap();
    let body = std::fs::read_to_string(dir.path().join(CHAP_DEVOPS_DIR).join(MODELS_FILE)).unwrap();
    assert!(body.contains("host_port: null"), "{body}");

    let loaded = Project::load(dir.path()).unwrap();
    assert_eq!(loaded.state.models["chapkit_ewars_model"].host_port, None);
    assert!(loaded.used_ports().is_empty(), "nothing is published");
}

#[test]
fn a_compose_file_name_is_one_file_in_the_directory() {
    assert!(is_plain_file_name("compose.chapkit-ewars-model.yml"));
    for name in [
        "",
        ".",
        "..",
        "/etc/compose.x.yml",
        "../compose.x.yml",
        "compose./../x.yml",
        "sub\\compose.x.yml",
        "C:compose.x.yml",
    ] {
        assert!(!is_plain_file_name(name), "{name:?}");
    }
}

/// State from elsewhere - a restored backup above all - reaches `sync`,
/// which writes `dir.join(compose_file)` and removes
/// `dir.join(rendered_file)`. A name that leaves the directory is refused
/// when the state is read, before anything joins it.
#[test]
fn load_refuses_a_compose_file_outside_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let mut model = enabled(None);
    model.compose_file = "/tmp/compose.evil.yml".into();
    Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            models: BTreeMap::from([("chapkit_ewars_model".to_string(), model)]),
            ..ProjectState::default()
        },
    }
    .save()
    .unwrap();
    let err = Project::load(dir.path()).expect_err("an absolute compose file");
    let text = err.to_string();
    assert!(
        text.contains("`.chap-devops/models.yaml` gives chapkit_ewars_model"),
        "{text}"
    );
    assert!(text.contains("`/tmp/compose.evil.yml`"), "{text}");

    Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            rendered_files: vec!["compose./../../victim.yml".into()],
            ..ProjectState::default()
        },
    }
    .save()
    .unwrap();
    let err = Project::load(dir.path()).expect_err("a rendered file that climbs out");
    assert!(
        err.to_string()
            .contains("`.chap-devops/project.yaml` lists"),
        "{err}"
    );
}

#[test]
fn the_api_and_proxy_urls_follow_the_api_port() {
    let project = Project {
        dir: PathBuf::from("/tmp/chapx"),
        state: ProjectState {
            api_port: 8123,
            ..ProjectState::default()
        },
    };
    assert_eq!(project.api_url(), "http://localhost:8123");
    assert_eq!(
        project.proxy_url("chapkit-ewars-model"),
        "http://localhost:8123/v2/services/chapkit-ewars-model/run/"
    );
}

/// `CHAP_ROOT_PATH` moves every route chap-core serves, so the addresses a
/// person is sent to have to carry it. Nothing writes that line, so the
/// usual answer is no prefix at all.
#[test]
fn the_root_path_comes_from_env_and_is_normalised() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            api_port: 8123,
            ..ProjectState::default()
        },
    };

    // No `.env`, and an `.env` that does not mention it: no prefix.
    assert_eq!(project.root_path(), "");
    assert_eq!(project.api_base(), "http://localhost:8123");
    let env = dir.path().join(ENV_FILE);
    std::fs::write(
        &env,
        "CHAP_API_PORT=18000
",
    )
    .unwrap();
    assert_eq!(project.root_path(), "");
    assert_eq!(project.api_base(), "http://localhost:18000");

    // Every spelling of the same prefix reads as one leading slash and no
    // trailing one, so it concatenates.
    for body in [
        "CHAP_ROOT_PATH=/master\n",
        "CHAP_ROOT_PATH=master\n",
        "CHAP_ROOT_PATH=/master/\n",
        "CHAP_ROOT_PATH=\"/master\"\n",
    ] {
        std::fs::write(&env, body).unwrap();
        assert_eq!(project.root_path(), "/master", "{body:?}");
        assert_eq!(
            project.api_base(),
            "http://localhost:8123/master",
            "{body:?}"
        );
    }

    // A commented, empty or slash-only line is not a prefix.
    for body in [
        "# CHAP_ROOT_PATH=/master\n",
        "CHAP_ROOT_PATH=\n",
        "CHAP_ROOT_PATH=/\n",
    ] {
        std::fs::write(&env, body).unwrap();
        assert_eq!(project.root_path(), "", "{body:?}");
    }
}

/// `.env` is what compose reads, so it is what every URL and every port
/// reservation in this CLI has to follow. The recorded value is the
/// fallback, not the answer.
#[test]
fn the_api_port_in_effect_comes_from_env_before_the_recorded_state() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            api_port: 8123,
            ..ProjectState::default()
        },
    };

    // No `.env` at all: a project written with `init --no-env`.
    assert_eq!(project.api_port_in_effect(), (8123, ApiPortSource::Project));

    let env = dir.path().join(ENV_FILE);
    std::fs::write(&env, "CHAP_API_PORT=18000\n").unwrap();
    assert_eq!(project.api_port_in_effect(), (18000, ApiPortSource::Env));
    assert_eq!(project.effective_api_port(), 18000);
    assert_eq!(project.api_url(), "http://localhost:18000");
    assert_eq!(
        project.proxy_url("chapkit-ewars-model"),
        "http://localhost:18000/v2/services/chapkit-ewars-model/run/"
    );

    // The shapes compose accepts are the shapes this reads.
    for body in [
        "export CHAP_API_PORT=18000\n",
        "CHAP_API_PORT=\"18000\"\n",
        "CHAP_API_PORT=8000\nCHAP_API_PORT=18000\n",
        "CHAP_API_PORT = 18000\n",
    ] {
        std::fs::write(&env, body).unwrap();
        assert_eq!(
            project.api_port_in_effect(),
            (18000, ApiPortSource::Env),
            "{body:?}"
        );
    }

    // And anything that is not a usable port leaves the recorded one in
    // place: a commented line, an empty assignment, a number no port can
    // hold, 0 (which means "any port" to the kernel and nothing to
    // compose) and plain nonsense.
    for body in [
        "# CHAP_API_PORT=18000\n",
        "CHAP_API_PORT=\n",
        "CHAP_API_PORT=70000\n",
        "CHAP_API_PORT=0\n",
        "CHAP_API_PORT=eight thousand\n",
        "POSTGRES_DB=chap_core\n",
        "",
    ] {
        std::fs::write(&env, body).unwrap();
        assert_eq!(
            project.api_port_in_effect(),
            (8123, ApiPortSource::Project),
            "{body:?}"
        );
    }
}

#[test]
fn the_api_port_source_names_its_file() {
    assert_eq!(ApiPortSource::Env.label(), "from .env");
    assert_eq!(
        ApiPortSource::Project.label(),
        "from .chap-devops/project.yaml"
    );
    // It is a field of `status --json`, so the spelling is part of that
    // document.
    assert_eq!(
        serde_json::to_value(ApiPortSource::Env).unwrap(),
        serde_json::json!("env")
    );
    assert_eq!(
        serde_json::to_value(ApiPortSource::Project).unwrap(),
        serde_json::json!("project")
    );
}

#[test]
fn the_compose_source_round_trips_both_ways() {
    let dir = tempfile::tempdir().unwrap();
    let fetched = ComposeSource::Fetched {
        url: "https://raw.githubusercontent.com/dhis2-chap/chap-core/v2.3.1/compose.ghcr.yml"
            .into(),
        tag: "v2.3.1".into(),
        sha256: "a".repeat(64),
    };
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            chap_image_tag: "v2.3.1".into(),
            chap_compose_source: fetched.clone(),
            ..ProjectState::default()
        },
    };
    project.save().unwrap();

    let body =
        std::fs::read_to_string(dir.path().join(CHAP_DEVOPS_DIR).join(PROJECT_FILE)).unwrap();
    assert!(body.contains("chap_compose_source:"));
    assert!(body.contains("kind: fetched"));
    assert!(body.contains("tag: v2.3.1"));

    let loaded = Project::load(dir.path()).unwrap();
    assert_eq!(loaded.state.chap_compose_source, fetched);
    assert_eq!(
        loaded.cached_compose_path(),
        Some(
            dir.path()
                .join(CHAP_DEVOPS_DIR)
                .join("compose.chap-core.v2.3.1.yml")
        )
    );

    // The embedded default writes its own tag and has no cached copy.
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState::default(),
    };
    project.save().unwrap();
    let body =
        std::fs::read_to_string(dir.path().join(CHAP_DEVOPS_DIR).join(PROJECT_FILE)).unwrap();
    assert!(body.contains("kind: embedded"));
    let loaded = Project::load(dir.path()).unwrap();
    assert_eq!(loaded.state.chap_compose_source, ComposeSource::Embedded);
    assert_eq!(loaded.cached_compose_path(), None);
}

#[test]
fn the_auth_block_round_trips_as_two_booleans_and_no_secret() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            auth: AuthState {
                api_token: true,
                registration_key: true,
            },
            ..ProjectState::default()
        },
    };
    project.save().unwrap();

    let body =
        std::fs::read_to_string(dir.path().join(CHAP_DEVOPS_DIR).join(PROJECT_FILE)).unwrap();
    assert!(body.contains("auth:\n"), "{body}");
    assert!(body.contains("  api_token: true\n"), "{body}");
    assert!(body.contains("  registration_key: true\n"), "{body}");

    let loaded = Project::load(dir.path()).unwrap();
    assert!(loaded.state.auth.api_token && loaded.state.auth.registration_key);
    assert!(loaded.state.auth.is_on());

    // One half on is still on.
    assert!(
        AuthState {
            api_token: false,
            registration_key: true,
        }
        .is_on()
    );
}

#[test]
fn the_compose_project_name_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            compose_project: "mychap-1ab2c3".into(),
            ..ProjectState::default()
        },
    };
    project.save().unwrap();
    let body =
        std::fs::read_to_string(dir.path().join(CHAP_DEVOPS_DIR).join(PROJECT_FILE)).unwrap();
    assert!(
        body.contains("\ncompose_project: mychap-1ab2c3\n"),
        "{body}"
    );

    let loaded = Project::load(dir.path()).unwrap();
    assert_eq!(loaded.compose_project(), Some("mychap-1ab2c3"));
    assert_eq!(
        loaded.compose_project_name().as_deref(),
        Some("mychap-1ab2c3")
    );
    assert_eq!(loaded.volume_prefix().as_deref(), Some("mychap-1ab2c3_"));
    assert_eq!(
        loaded
            .prefixed_volume(&crate::compose::volume_name("chapkit_ewars_model"))
            .as_deref(),
        Some("mychap-1ab2c3_ck_chapkit_ewars_model_data")
    );
    assert_eq!(
        loaded.prefixed_volume("ocs_data").as_deref(),
        Some("mychap-1ab2c3_ocs_data")
    );
}

#[test]
fn a_generated_name_is_a_slug_and_six_hex_characters() {
    let dir = std::path::PathBuf::from("/srv/My Chap (prod)");
    let name = new_compose_project_name(&dir).unwrap();
    let (slug, suffix) = name.rsplit_once('-').expect("a suffix");
    assert_eq!(slug, "my-chap-prod");
    assert_eq!(suffix.len(), PROJECT_SUFFIX_BYTES * 2);
    assert!(
        suffix
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    );

    // Compose's own rule: lowercase, and it starts with a letter or digit.
    let first = name.chars().next().unwrap();
    assert!(
        first.is_ascii_lowercase() || first.is_ascii_digit(),
        "{name}"
    );
    assert!(
        name.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
        "{name}"
    );

    // Two deployments of the same name get two different names, which is
    // the whole point of the suffix.
    let again = new_compose_project_name(&dir).unwrap();
    assert_ne!(name, again);
}

#[test]
fn the_slug_is_what_compose_accepts_whatever_the_directory_is_called() {
    assert_eq!(project_slug("demo"), "demo");
    assert_eq!(project_slug("My Chap"), "my-chap");
    assert_eq!(project_slug("chap_prod.eu"), "chap-prod-eu");
    assert_eq!(project_slug("--weird--"), "weird");
    assert_eq!(project_slug("2026-deploy"), "2026-deploy");
    // Nothing usable at all still has to yield a legal name.
    assert_eq!(project_slug(""), FALLBACK_SLUG);
    assert_eq!(project_slug("深度"), FALLBACK_SLUG);
    assert_eq!(project_slug("..."), FALLBACK_SLUG);
    // And a very long one is cut where it still reads.
    let long = project_slug(&"a".repeat(100));
    assert_eq!(long.len(), MAX_SLUG);

    assert_eq!(compose_project_name("demo", "1ab2c3"), "demo-1ab2c3");
}

#[test]
fn the_cached_file_name_is_tagged_and_safe() {
    assert_eq!(
        cached_compose_file("v2.3.1"),
        "compose.chap-core.v2.3.1.yml"
    );
    assert_eq!(
        cached_compose_file("master"),
        "compose.chap-core.master.yml"
    );
    assert_eq!(
        cached_compose_file("feature/x y"),
        "compose.chap-core.feature_x_y.yml"
    );
}

#[test]
fn a_missing_or_blank_models_file_means_no_models() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState::default(),
    };
    project.save().unwrap();
    let models = dir.path().join(CHAP_DEVOPS_DIR).join(MODELS_FILE);
    assert!(models.is_file());
    assert!(Project::load(dir.path()).unwrap().state.models.is_empty());

    std::fs::write(&models, "# nothing enabled\n\n").unwrap();
    assert!(Project::load(dir.path()).unwrap().state.models.is_empty());

    std::fs::remove_file(&models).unwrap();
    assert!(Project::load(dir.path()).unwrap().state.models.is_empty());
}

#[test]
fn the_components_file_round_trips_and_shapes_the_f_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut components = Components::default();
    components.ocs.enabled = true;
    components.ocs.port = Some(9010);
    components.s3.enabled = true;
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            components: components.clone(),
            ..ProjectState::default()
        },
    };
    project.save().unwrap();

    let body = std::fs::read_to_string(dir.path().join(CHAP_DEVOPS_DIR).join(COMPONENTS_FILE))
        .expect("components.yaml is written beside the others");
    assert!(body.starts_with(MANAGED_HEADER), "{body}");
    assert!(body.contains("\nchap-core:\n"), "{body}");
    assert!(body.contains("  port: 9010\n"), "{body}");

    let loaded = Project::load(dir.path()).unwrap();
    assert_eq!(loaded.state.components, components);
    assert_eq!(
        compose_files_for(&loaded.state.components),
        vec![
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.ocs.yml",
            "compose.s3.yml",
            "compose.marketplace.yml"
        ]
    );
    // The component ports are claimed against the model allocator too.
    assert_eq!(loaded.used_ports(), BTreeSet::from([9010]));
    assert_eq!(
        loaded.ocs_config_path(),
        dir.path().join("ocs").join("climate-service.yaml")
    );
}

#[test]
fn a_missing_or_blank_components_file_is_chap_core_alone() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            compose_project: "mychap-1ab2c3".into(),
            ..ProjectState::default()
        },
    };
    project.save().unwrap();
    let components = dir.path().join(CHAP_DEVOPS_DIR).join(COMPONENTS_FILE);
    std::fs::remove_file(&components).unwrap();

    let loaded = Project::load(dir.path()).unwrap();
    assert_eq!(loaded.state.components, Components::default());
    assert!(loaded.state.components.chap_core.enabled);
    assert!(!loaded.state.components.ocs.enabled);
    assert_eq!(
        compose_files_for(&loaded.state.components),
        default_compose_files()
    );

    // A comment-only file reads the same way.
    std::fs::write(&components, "# nothing set\n\n").unwrap();
    let loaded = Project::load(dir.path()).unwrap();
    assert_eq!(loaded.state.components, Components::default());
}

#[test]
fn a_deployment_without_chap_core_drops_the_base_files_from_the_list() {
    let mut components = Components::default();
    components.chap_core.enabled = false;
    components.ocs.enabled = true;
    assert_eq!(
        compose_files_for(&components),
        vec!["compose.ocs.yml", "compose.marketplace.yml"]
    );
}

#[test]
fn load_on_an_empty_dir_is_not_a_project() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!Project::exists(dir.path()));
    let err = Project::load(dir.path()).expect_err("empty dir is not a project");
    match err.downcast_ref::<ChapError>() {
        Some(ChapError::NotAProject(p)) => assert_eq!(p, dir.path()),
        other => panic!("wrong error: {other:?}"),
    }
}

#[test]
fn find_walks_up_to_the_nearest_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("deploy");
    let nested = root.join("a").join("b");
    std::fs::create_dir_all(&nested).unwrap();
    Project {
        dir: root.clone(),
        state: ProjectState {
            chap_image_tag: "v9".into(),
            ..ProjectState::default()
        },
    }
    .save()
    .unwrap();

    for start in [&root, &root.join("a"), &nested] {
        let found = Project::find(start).unwrap();
        assert_eq!(found.dir, root, "from {}", start.display());
        assert_eq!(found.state.chap_image_tag, "v9");
    }
    assert_eq!(Project::find_root(&nested).as_deref(), Some(root.as_path()));

    // A closer project wins over a farther one.
    Project {
        dir: nested.clone(),
        state: ProjectState::default(),
    }
    .save()
    .unwrap();
    assert_eq!(Project::find(&nested).unwrap().dir, nested);
    assert_eq!(Project::find(&root.join("a")).unwrap().dir, root);
}

#[test]
fn find_names_the_start_directory_when_nothing_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let start = dir.path().join("x").join("y");
    std::fs::create_dir_all(&start).unwrap();
    let err = Project::find(&start).expect_err("no project above a temp dir");
    match err.downcast_ref::<ChapError>() {
        Some(ChapError::NotAProject(p)) => assert_eq!(p, &start),
        other => panic!("wrong error: {other:?}"),
    }
    assert!(Project::find_root(&start).is_none());
}

#[test]
fn exists_and_helpers_reflect_the_state() {
    let dir = tempfile::tempdir().unwrap();
    let state = ProjectState {
        models: BTreeMap::from([
            ("a".to_string(), enabled(Some(5001))),
            ("b".to_string(), enabled(Some(5004))),
            ("c".to_string(), enabled(None)),
        ]),
        ..ProjectState::default()
    };
    let project = Project {
        dir: dir.path().to_path_buf(),
        state,
    };
    project.save().unwrap();

    assert!(Project::exists(dir.path()));
    assert_eq!(project.used_ports(), BTreeSet::from([5001, 5004]));
    assert_eq!(
        project.compose_file_paths(),
        vec![
            dir.path().join(BASE_COMPOSE),
            dir.path().join(CHAP_DEVOPS_COMPOSE),
            dir.path().join(MARKETPLACE_COMPOSE),
        ]
    );
}
