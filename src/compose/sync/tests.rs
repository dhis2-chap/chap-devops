use super::config::{has_config_key, set_config_key};
use super::*;
use crate::components::{
    Components, DHIS2_DB_PASSWORD_ENV_VAR, DHIS2_DEFAULT_JAVA_OPTIONS,
    DHIS2_ENCRYPTION_PASSWORD_ENV_VAR, DHIS2_JAVA_ENV_VAR, DHIS2_SEED_ENV_VAR, OCS_CONFIG_FILE,
    OCS_DATA_SOURCE_ENV_VARS, OCS_DIR, OCS_PLUGINS_KEY, S3_ACCESS_KEY_ENV_VAR,
    S3_SECRET_KEY_ENV_VAR,
};
use crate::compose::apply::apply_with;
use crate::compose::spec::OcsConfigSpec;
use crate::compose::{EnableRequest, Selection};
use crate::project::ENV_FILE;
use crate::project::{ProjectState, default_compose_files};
use crate::registry::load_embedded;
use tempfile::TempDir;

fn project_with(ids: &[&str]) -> (TempDir, Project, Registry) {
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

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Write a cached `compose.ghcr.yml` into `.chap-devops/` and record it as the
/// project's compose source, the way `init` does after a fetch.
fn cache(project: &mut Project, tag: &str, body: &str) {
    let chap_devops = project.chap_devops_dir();
    std::fs::create_dir_all(&chap_devops).unwrap();
    std::fs::write(
        chap_devops.join(crate::project::cached_compose_file(tag)),
        body,
    )
    .unwrap();
    project.state.chap_compose_source = ComposeSource::Fetched {
        url: crate::chapcore::compose_url(tag),
        tag: tag.to_string(),
        sha256: crate::chapcore::sha256_hex(body.as_bytes()),
    };
}

fn names(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_second_sync_changes_nothing() {
    let (_dir, mut project, registry) = project_with(&["chapkit_ewars_model"]);
    let report = sync(&mut project, &registry, false).unwrap();
    assert!(!report.drift, "{report:?}");
    assert!(report.written.is_empty() && report.removed.is_empty());
    assert_eq!(
        names(&report.unchanged),
        vec![
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.chapkit-ewars-model.yml",
            "compose.marketplace.yml"
        ]
    );
    assert_eq!(
        project.state.rendered_files,
        vec![
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.chapkit-ewars-model.yml",
            "compose.marketplace.yml"
        ]
    );
    assert_eq!(report.summary(), "0 written, 4 unchanged, 0 removed");
}

#[test]
fn sync_renders_the_chap_devops_overlay_and_puts_it_in_the_f_list() {
    let (dir, mut project, registry) = project_with(&["chapkit_ewars_model"]);
    let overlay = dir.path().join(CHAP_DEVOPS_COMPOSE);
    assert!(overlay.is_file());
    assert!(read(&overlay).contains("${CHAP_API_PORT:-8700}:8000"));
    assert_eq!(
        project.state.compose_files,
        vec![
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.marketplace.yml"
        ],
        "the override needs its own -f entry, between the base and the umbrella"
    );

    // The API port lives in .chap-devops/project.yaml, so moving it is drift.
    project.state.api_port = 8123;
    let report = sync(&mut project, &registry, true).unwrap();
    assert!(report.drift);
    assert_eq!(names(&report.written), vec![CHAP_DEVOPS_COMPOSE]);
    assert!(
        read(&overlay).contains("8700}:8000"),
        "--check writes nothing"
    );

    let report = sync(&mut project, &registry, false).unwrap();
    assert_eq!(names(&report.written), vec![CHAP_DEVOPS_COMPOSE]);
    assert!(read(&overlay).contains("${CHAP_API_PORT:-8123}:8000"));
    assert!(!sync(&mut project, &registry, true).unwrap().drift);

    // It is never removed as if it were a model overlay.
    project.state.models.clear();
    sync(&mut project, &registry, false).unwrap();
    assert!(overlay.is_file());
}

#[test]
fn check_reports_drift_without_touching_anything() {
    let (dir, mut project, registry) = project_with(&["chapkit_ewars_model"]);
    let overlay = dir.path().join("compose.chapkit-ewars-model.yml");
    std::fs::remove_file(&overlay).unwrap();

    let report = sync(&mut project, &registry, true).unwrap();
    assert!(report.drift);
    assert!(report.check);
    assert_eq!(
        names(&report.written),
        vec!["compose.chapkit-ewars-model.yml"]
    );
    assert!(!overlay.exists(), "--check writes nothing");
    assert_eq!(report.summary(), "1 to write, 3 unchanged, 0 to remove");

    let report = sync(&mut project, &registry, false).unwrap();
    assert!(report.drift);
    assert!(overlay.is_file(), "a real sync re-creates the overlay");
    let again = sync(&mut project, &registry, true).unwrap();
    assert!(!again.drift);
}

#[test]
fn a_model_removed_from_the_state_loses_its_overlay_but_hand_written_files_stay() {
    let (dir, mut project, registry) = project_with(&["chapkit_ewars_model", "auto_arima_chapkit"]);
    let custom = dir.path().join("compose.custom.yml");
    std::fs::write(&custom, "services: {}\n").unwrap();

    // Simulate a hand edit of .chap-devops/models.yaml.
    project.state.models.remove("auto_arima_chapkit");
    let report = sync(&mut project, &registry, false).unwrap();
    assert_eq!(
        names(&report.removed),
        vec!["compose.auto-arima-chapkit.yml"]
    );
    assert!(!dir.path().join("compose.auto-arima-chapkit.yml").exists());
    assert!(custom.is_file(), "hand-written overlays are never removed");
    assert_eq!(names(&report.written), vec!["compose.marketplace.yml"]);

    // A listed name that is not an overlay shape is never removed either.
    std::fs::write(dir.path().join("notes.yml"), "x: 1\n").unwrap();
    project.state.rendered_files.push("notes.yml".into());
    let report = sync(&mut project, &registry, false).unwrap();
    assert!(report.removed.is_empty(), "{report:?}");
    assert!(dir.path().join("notes.yml").is_file());
}

#[test]
fn a_model_missing_from_the_registry_still_renders_with_a_warning() {
    let (dir, mut project, registry) = project_with(&["chapkit_ewars_model"]);
    let mut entry = project.state.models["chapkit_ewars_model"].clone();
    entry.service_id = "gone-model".into();
    entry.compose_file = "compose.gone-model.yml".into();
    project.state.models.insert("gone_model".into(), entry);

    let report = sync(&mut project, &registry, false).unwrap();
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("gone_model"));
    let body = std::fs::read_to_string(dir.path().join("compose.gone-model.yml")).unwrap();
    assert!(body.contains("  gone-model:\n"));
    // The header names the id, and the image where the repository would be.
    assert!(
        body.contains(&format!(
            "# gone_model {} (ghcr.io/chap-models/chapkit_ewars_model)",
            project.state.models["gone_model"].version
        )),
        "{body}"
    );
    assert!(!sync(&mut project, &registry, true).unwrap().drift);
}

#[test]
fn every_overlay_gets_the_registration_line_when_the_project_has_a_key() {
    let (dir, mut project, registry) = project_with(&["chapkit_ewars_model", "auto_arima_chapkit"]);
    let overlays = [
        dir.path().join("compose.chapkit-ewars-model.yml"),
        dir.path().join("compose.auto-arima-chapkit.yml"),
    ];
    // Off by default: the line is there, commented, as chap-core's own
    // example overlay ships it.
    for path in &overlays {
        let body = read(path);
        assert!(
            body.contains("      # SERVICEKIT_REGISTRATION_KEY:"),
            "{body}"
        );
    }

    // Turning it on in `.chap-devops/project.yaml` is drift in every overlay.
    project.state.auth.registration_key = true;
    let report = sync(&mut project, &registry, true).unwrap();
    assert!(report.drift);
    assert_eq!(
        names(&report.written),
        vec![
            "compose.auto-arima-chapkit.yml",
            "compose.chapkit-ewars-model.yml"
        ]
    );

    sync(&mut project, &registry, false).unwrap();
    for path in &overlays {
        let body = read(path);
        assert!(
            body.contains("      SERVICEKIT_REGISTRATION_KEY: ${SERVICEKIT_REGISTRATION_KEY:-}"),
            "{body}"
        );
        assert!(!body.contains("# SERVICEKIT_REGISTRATION_KEY:"), "{body}");
    }
    assert!(!sync(&mut project, &registry, true).unwrap().drift);

    // And off again returns every overlay to the commented form.
    project.state.auth.registration_key = false;
    sync(&mut project, &registry, false).unwrap();
    for path in &overlays {
        assert!(read(path).contains("      # SERVICEKIT_REGISTRATION_KEY:"));
    }
}

#[test]
fn a_user_with_no_known_ids_renders_with_a_warning() {
    let (dir, mut project, registry) = project_with(&["chapkit_ewars_model"]);
    let report = sync(&mut project, &registry, false).unwrap();
    assert!(report.warnings.is_empty(), "{report:?}");

    project
        .state
        .models
        .get_mut("chapkit_ewars_model")
        .unwrap()
        .user = "nobody".into();
    let report = sync(&mut project, &registry, false).unwrap();
    assert_eq!(report.warnings.len(), 1, "{report:?}");
    assert!(report.warnings[0].contains("chapkit_ewars_model runs as `nobody`"));
    assert!(report.warnings[0].contains("1000:1000"));

    // The overlay still renders, with the fallback ids in the chown.
    let body = std::fs::read_to_string(dir.path().join("compose.chapkit-ewars-model.yml")).unwrap();
    assert!(body.contains("chown -R 1000:1000 /app/data"), "{body}");
    assert!(body.contains("    user: nobody\n"));

    // A numeric user is understood and warns about nothing.
    project
        .state
        .models
        .get_mut("chapkit_ewars_model")
        .unwrap()
        .user = "1000:1000".into();
    let report = sync(&mut project, &registry, false).unwrap();
    assert!(report.warnings.is_empty(), "{report:?}");
}

/// A project with the given components, synced once.
fn project_with_components(components: Components) -> (TempDir, Project, Registry) {
    let (dir, mut project, registry) = project_with(&[]);
    project.state.components = components;
    sync(&mut project, &registry, false).unwrap();
    (dir, project, registry)
}

#[test]
fn a_component_is_rendered_listed_and_removed_again() {
    let mut components = Components::default();
    components.ocs.enabled = true;
    let (dir, mut project, registry) = project_with_components(components);

    let ocs = dir.path().join(OCS_COMPOSE);
    assert!(ocs.is_file());
    assert_eq!(
        project.state.compose_files,
        vec![
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.ocs.yml",
            "compose.marketplace.yml"
        ],
        "a component file sits between the override and the umbrella"
    );
    assert!(
        project
            .state
            .rendered_files
            .contains(&OCS_COMPOSE.to_string())
    );
    // The scaffold went in too, and a second sync leaves everything alone.
    assert!(project.ocs_config_path().is_file());
    assert!(!sync(&mut project, &registry, true).unwrap().drift);

    // Turning it off removes the file and takes it out of both lists.
    project.state.components.ocs.enabled = false;
    let report = sync(&mut project, &registry, false).unwrap();
    assert_eq!(names(&report.removed), vec![OCS_COMPOSE]);
    assert!(!ocs.exists());
    assert_eq!(project.state.compose_files, default_compose_files());
    assert!(
        !project
            .state
            .rendered_files
            .contains(&OCS_COMPOSE.to_string())
    );
    // The operator's own config file is not ours to delete.
    assert!(project.ocs_config_path().is_file());
    assert!(!sync(&mut project, &registry, true).unwrap().drift);
}

#[test]
fn the_object_store_reaches_the_ocs_file_as_well() {
    let mut components = Components::default();
    components.ocs.enabled = true;
    components.s3.enabled = true;
    let (dir, mut project, registry) = project_with_components(components);

    assert!(dir.path().join(S3_COMPOSE).is_file());
    assert!(read(&dir.path().join(OCS_COMPOSE)).contains("S3_ENDPOINT: http://s3:9000"));
    assert_eq!(
        project.state.compose_files,
        vec![
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.ocs.yml",
            "compose.s3.yml",
            "compose.marketplace.yml"
        ]
    );

    // Taking the store away rewrites the OCS file without those lines.
    project.state.components.s3.enabled = false;
    let report = sync(&mut project, &registry, false).unwrap();
    assert_eq!(names(&report.removed), vec![S3_COMPOSE]);
    assert!(names(&report.written).contains(&OCS_COMPOSE.to_string()));
    assert!(!read(&dir.path().join(OCS_COMPOSE)).contains("S3_ENDPOINT"));
}

#[test]
fn chap_core_off_leaves_only_the_component_files() {
    let mut components = Components::default();
    components.ocs.enabled = true;
    let (dir, mut project, registry) = project_with_components(components);
    assert!(dir.path().join(BASE_COMPOSE).is_file());

    project.state.components.chap_core.enabled = false;
    let report = sync(&mut project, &registry, false).unwrap();
    let removed = names(&report.removed);
    assert!(removed.contains(&BASE_COMPOSE.to_string()), "{removed:?}");
    assert!(
        removed.contains(&CHAP_DEVOPS_COMPOSE.to_string()),
        "{removed:?}"
    );
    assert!(!dir.path().join(BASE_COMPOSE).exists());
    assert!(!dir.path().join(CHAP_DEVOPS_COMPOSE).exists());
    assert_eq!(
        project.state.compose_files,
        vec!["compose.ocs.yml", "compose.marketplace.yml"]
    );
    assert!(!sync(&mut project, &registry, true).unwrap().drift);
}

/// The component is rendered, listed, scaffolded and removed again, exactly
/// as `ocs` is - and the file it scaffolds is the operator's from then on,
/// because DHIS2 does not start without it.
/// The empty-database warning comes with the sync that writes the DHIS2
/// compose file, not with every `chap-devops up` after it.
#[test]
fn the_unknown_seed_is_warned_about_once_not_on_every_sync() {
    let (_dir, mut project, registry) = project_with(&[]);
    project.state.components.dhis2.enabled = true;
    project.state.components.dhis2.image_tag = "2.43".to_string();
    let first = sync(&mut project, &registry, false).unwrap();
    assert!(
        first
            .warnings
            .iter()
            .any(|w| w.contains("no DHIS2 demo dump")),
        "{:?}",
        first.warnings
    );
    let again = sync(&mut project, &registry, false).unwrap();
    assert!(
        !again
            .warnings
            .iter()
            .any(|w| w.contains("no DHIS2 demo dump")),
        "{:?}",
        again.warnings
    );
}

#[test]
fn the_dhis2_component_is_rendered_scaffolded_and_removed_again() {
    let mut components = Components::default();
    components.dhis2.enabled = true;
    let (dir, mut project, registry) = project_with_components(components);

    let compose = dir.path().join(DHIS2_COMPOSE);
    assert!(compose.is_file());
    assert_eq!(
        project.state.compose_files,
        vec![
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.dhis2.yml",
            "compose.marketplace.yml"
        ],
        "a component file sits between the override and the umbrella"
    );
    assert!(
        project
            .state
            .rendered_files
            .contains(&DHIS2_COMPOSE.to_string())
    );
    // The four services and the config mount the file needs.
    let body = read(&compose);
    assert!(body.contains("\n  dhis2-db:\n"), "{body}");
    assert!(body.contains("\n  dhis2-dump:\n"), "{body}");
    assert!(body.contains("\n  dhis2-prep:\n"), "{body}");
    assert!(
        body.contains("./dhis2/dhis.conf:/opt/dhis2/dhis.conf:ro"),
        "{body}"
    );

    let config = dhis2_config_path(&project.dir);
    assert!(
        config.is_file(),
        "dhis.conf is mandatory, so it is scaffolded"
    );
    assert!(read(&config).contains("route.remote_servers_allowed = http://*,https://*"));
    assert!(!sync(&mut project, &registry, true).unwrap().drift);

    // An operator's own file is never rewritten, by this or by anything else.
    std::fs::write(&config, "connection.username = mine\n").unwrap();
    sync(&mut project, &registry, false).unwrap();
    assert_eq!(read(&config), "connection.username = mine\n");

    // Turning it off removes the compose file and leaves the directory.
    project.state.components.dhis2.enabled = false;
    let report = sync(&mut project, &registry, false).unwrap();
    assert_eq!(names(&report.removed), vec![DHIS2_COMPOSE]);
    assert!(!compose.exists());
    assert_eq!(project.state.compose_files, default_compose_files());
    assert!(
        config.is_file(),
        "the operator's file is not ours to delete"
    );
    assert!(!sync(&mut project, &registry, true).unwrap().drift);
}

/// A seed left at `default` on a minor line with no published dump is an
/// empty database, and the operator is told rather than left to find out.
#[test]
fn an_unknown_dhis2_minor_starts_empty_and_says_so() {
    let mut components = Components::default();
    components.dhis2.enabled = true;
    let (dir, mut project, registry) = project_with_components(components);
    let report = sync(&mut project, &registry, false).unwrap();
    assert!(report.warnings.is_empty(), "{report:?}");
    assert!(read(&dir.path().join(DHIS2_COMPOSE)).contains("dhis2-dump"));

    project.state.components.dhis2.image_tag = "2.40".to_string();
    let report = sync(&mut project, &registry, false).unwrap();
    assert_eq!(report.warnings.len(), 1, "{report:?}");
    assert!(report.warnings[0].contains("2.40"), "{report:?}");
    assert!(
        report.warnings[0].contains("`chap-devops components enable dhis2 --seed URL`"),
        "{report:?}"
    );
    // And the file rendered for it has no one-shot and no dump volume.
    let body = read(&dir.path().join(DHIS2_COMPOSE));
    assert!(!body.contains("dhis2-dump"), "{body}");
    assert!(!body.contains("dhis2_dump"), "{body}");

    // `seed: none` is the same rendering with nothing to report.
    project.state.components.dhis2.seed = crate::components::Dhis2Seed::None;
    let report = sync(&mut project, &registry, false).unwrap();
    assert!(report.warnings.is_empty(), "{report:?}");
}

/// A dump that is a file is a bind mount, and compose refuses to start a
/// service whose bind source is missing - so a path that is not there yet is
/// reported now instead of at `chap-devops up`.
#[test]
fn a_file_seed_that_is_not_there_yet_is_reported() {
    let mut components = Components::default();
    components.dhis2.enabled = true;
    components.dhis2.seed = crate::components::Dhis2Seed::From("dumps/laos.sql.gz".into());
    let (dir, mut project, registry) = project_with_components(components);

    let report = sync(&mut project, &registry, false).unwrap();
    assert_eq!(report.warnings.len(), 1, "{report:?}");
    assert!(
        report.warnings[0].contains("dumps/laos.sql.gz"),
        "{report:?}"
    );
    assert!(
        report.warnings[0].contains("`chap-devops components enable dhis2 --seed none`"),
        "{report:?}"
    );
    // The mount is rendered either way: the operator may be about to copy
    // the dump in, and a compose file that changed shape when a file
    // appeared would be drift nobody asked for.
    assert!(
        read(&dir.path().join(DHIS2_COMPOSE)).contains("- ./dumps/laos.sql.gz:/opt/seed.sql.gz:ro"),
    );

    std::fs::create_dir_all(dir.path().join("dumps")).unwrap();
    std::fs::write(dir.path().join("dumps/laos.sql.gz"), "not really a dump").unwrap();
    let report = sync(&mut project, &registry, false).unwrap();
    assert!(report.warnings.is_empty(), "{report:?}");
    assert!(!report.drift, "{report:?}");
}

/// The DHIS2 secrets are generated once and never rewritten - the database
/// volume was created with them - and the encryption password has to clear
/// DHIS2's own 24-character floor.
#[test]
fn the_dhis2_env_block_is_appended_once_with_secrets_long_enough_for_dhis2() {
    let (dir, mut project, registry) = project_with(&[]);
    let env = dir.path().join(ENV_FILE);
    std::fs::write(&env, "POSTGRES_PASSWORD=secret\n").unwrap();

    project.state.components.dhis2.enabled = true;
    let report = sync(&mut project, &registry, true).unwrap();
    assert!(report.drift, "the missing block is drift");
    assert_eq!(
        std::fs::read_to_string(&env).unwrap(),
        "POSTGRES_PASSWORD=secret\n",
        "--check writes nothing"
    );

    sync(&mut project, &registry, false).unwrap();
    let body = std::fs::read_to_string(&env).unwrap();
    assert!(body.starts_with("POSTGRES_PASSWORD=secret\n"));
    let password = crate::auth::active_value(&body, DHIS2_DB_PASSWORD_ENV_VAR).expect("a password");
    let encryption = crate::auth::active_value(&body, DHIS2_ENCRYPTION_PASSWORD_ENV_VAR)
        .expect("an encryption password");
    assert_eq!(password.len(), 32);
    assert_ne!(password, encryption);
    // Under 24 and DHIS2 stops on ENCRYPTION_PASSWORD_TOO_SHORT.
    assert!(encryption.len() >= 24, "{encryption}");
    // The pins an operator goes looking for, commented, each holding the
    // value the rendered file already defaults to.
    assert!(body.contains("\n# DHIS2_IMAGE_TAG=2.42\n"), "{body}");
    assert!(
        body.contains(&format!(
            "\n# {DHIS2_SEED_ENV_VAR}={}\n",
            crate::compose::render::DHIS2_DEFAULT_SEED_URL
        )),
        "{body}"
    );
    assert!(
        body.contains(&format!(
            "\n# {DHIS2_JAVA_ENV_VAR}={DHIS2_DEFAULT_JAVA_OPTIONS}\n"
        )),
        "{body}"
    );

    // A second sync adds nothing and rotates nothing.
    assert!(!sync(&mut project, &registry, true).unwrap().drift);
    sync(&mut project, &registry, false).unwrap();
    assert_eq!(std::fs::read_to_string(&env).unwrap(), body);
}

/// The login `chap-devops dhis2` authenticates with is named in `.env`, commented
/// out and holding the value the commands already fall back to - so the two
/// variables are discoverable and nothing that runs is changed by them.
#[test]
fn the_dhis2_login_variables_are_named_in_env_as_commented_placeholders() {
    let (dir, mut project, registry) = project_with(&[]);
    let env = dir.path().join(ENV_FILE);
    std::fs::write(&env, "POSTGRES_PASSWORD=secret\n").unwrap();

    project.state.components.dhis2.enabled = true;
    sync(&mut project, &registry, false).unwrap();
    let body = std::fs::read_to_string(&env).unwrap();
    assert!(body.contains("\n# DHIS2_ADMIN_USERNAME=admin\n"), "{body}");
    assert!(
        body.contains("\n# DHIS2_ADMIN_PASSWORD=district\n"),
        "{body}"
    );
    // Commented out means unset, so neither is a password on disk.
    assert_eq!(
        crate::auth::active_value(&body, crate::dhis2::ADMIN_PASSWORD_ENV_VAR),
        None
    );
    // And no container is passed either of them.
    let compose = read(&dir.path().join(DHIS2_COMPOSE));
    assert!(!compose.contains("DHIS2_ADMIN"), "{compose}");

    // A second sync adds nothing, and a value the operator filled in is
    // left exactly as it is.
    assert!(!sync(&mut project, &registry, true).unwrap().drift);
    let mine = body.replace(
        "# DHIS2_ADMIN_PASSWORD=district",
        "DHIS2_ADMIN_PASSWORD=mine",
    );
    std::fs::write(&env, &mine).unwrap();
    sync(&mut project, &registry, false).unwrap();
    assert_eq!(std::fs::read_to_string(&env).unwrap(), mine);
}

/// The token is named beside the login, and an external DHIS2 is given no
/// `district`: chap-devops did not create it, so that is nobody's password there.
#[test]
fn the_dhis2_token_is_named_and_an_external_dhis2_gets_no_default() {
    let (dir, mut project, registry) = project_with(&[]);
    let env = dir.path().join(ENV_FILE);
    std::fs::write(&env, "POSTGRES_PASSWORD=secret\n").unwrap();
    project.state.components.dhis2_external = Some(crate::components::ExternalDhis2 {
        url: "https://dhis2.example.org".into(),
        chap_url: "https://chap.example.org".into(),
        connected_at: None,
    });
    sync(&mut project, &registry, false).unwrap();
    let body = std::fs::read_to_string(&env).unwrap();
    assert!(body.contains("\n# DHIS2_API_TOKEN=\n"), "{body}");
    assert!(body.contains("\n# DHIS2_ADMIN_PASSWORD=\n"), "{body}");
    assert!(!body.contains("district"), "{body}");
    // No container, so no compose file for it.
    assert!(!dir.path().join(DHIS2_COMPOSE).exists());
}

/// The commented dump pin has to be the value the rendered file defaults to,
/// or uncommenting it breaks the seed: for a dump that is a file, that is the
/// path inside the container, not the one on the host.
#[test]
fn the_dump_pin_in_env_is_the_path_the_container_reads() {
    let (dir, mut project, registry) = project_with(&[]);
    let env = dir.path().join(ENV_FILE);
    std::fs::write(&env, "POSTGRES_PASSWORD=secret\n").unwrap();

    project.state.components.dhis2.enabled = true;
    project.state.components.dhis2.seed =
        crate::components::Dhis2Seed::From("dumps/laos.sql.gz".into());
    sync(&mut project, &registry, false).unwrap();

    let body = std::fs::read_to_string(&env).unwrap();
    let pinned = format!(
        "\n# {DHIS2_SEED_ENV_VAR}={}\n",
        crate::compose::render::DHIS2_SEED_MOUNT
    );
    assert!(body.contains(&pinned), "{body}");
    assert!(!body.contains("# DHIS2_DB_DUMP_URL=dumps/"), "{body}");
    // And it is the same string the compose file substitutes into.
    assert!(read(&dir.path().join(DHIS2_COMPOSE)).contains(&format!(
        "${{{DHIS2_SEED_ENV_VAR}:-{}}}",
        crate::compose::render::DHIS2_SEED_MOUNT
    )));
}

/// `.env` is written once by `init`, so a deployment that enables the
/// component later has the block appended below whatever is already there -
/// and nothing above it is touched.
#[test]
fn a_deployment_that_enables_dhis2_later_gets_the_block_appended() {
    let (dir, mut project, registry) = project_with(&[]);
    let env = dir.path().join(ENV_FILE);
    let before = "POSTGRES_PASSWORD=secret\nCHAP_API_TOKEN=abc\n";
    std::fs::write(&env, before).unwrap();

    project.state.components.dhis2.enabled = true;
    sync(&mut project, &registry, false).unwrap();
    let body = std::fs::read_to_string(&env).unwrap();
    assert!(body.starts_with(before), "{body}");
    assert!(
        body.contains("\n# DHIS2 (component). Generated once;"),
        "{body}"
    );
    assert_eq!(body.matches("DHIS2_DB_PASSWORD").count(), 1, "{body}");

    // And a password the operator replaced is theirs: the block is not
    // appended a second time, whatever else changed in the file.
    let mine = body.replace(
        &format!("{DHIS2_DB_PASSWORD_ENV_VAR}="),
        &format!("{DHIS2_DB_PASSWORD_ENV_VAR}=mine-"),
    );
    std::fs::write(&env, &mine).unwrap();
    sync(&mut project, &registry, false).unwrap();
    assert_eq!(std::fs::read_to_string(&env).unwrap(), mine);
}

#[test]
fn the_component_env_lines_are_appended_once() {
    let (dir, mut project, registry) = project_with(&[]);
    let env = dir.path().join(ENV_FILE);
    std::fs::write(
        &env,
        "POSTGRES_PASSWORD=secret
",
    )
    .unwrap();

    project.state.components.ocs.enabled = true;
    project.state.components.s3.enabled = true;
    let report = sync(&mut project, &registry, true).unwrap();
    assert!(report.drift, "the missing lines are drift");
    assert_eq!(
        std::fs::read_to_string(&env).unwrap(),
        "POSTGRES_PASSWORD=secret\n",
        "--check writes nothing"
    );

    sync(&mut project, &registry, false).unwrap();
    let body = std::fs::read_to_string(&env).unwrap();
    assert!(body.starts_with("POSTGRES_PASSWORD=secret\n"));
    assert!(body.contains("\n# OCS_IMAGE_TAG=main\n"), "{body}");
    assert!(body.contains("\n# S3_IMAGE_TAG=latest\n"), "{body}");
    let access = crate::auth::active_value(&body, S3_ACCESS_KEY_ENV_VAR).expect("a key");
    let secret = crate::auth::active_value(&body, S3_SECRET_KEY_ENV_VAR).expect("a secret");
    assert_eq!(access.len(), 32);
    assert_eq!(secret.len(), 32);
    assert_ne!(access, secret);

    // A second sync adds nothing, and never rewrites the credentials: the
    // volume was created with them.
    assert!(!sync(&mut project, &registry, true).unwrap().drift);
    sync(&mut project, &registry, false).unwrap();
    let again = std::fs::read_to_string(&env).unwrap();
    assert_eq!(again, body);
    assert_eq!(again.matches("OCS_IMAGE_TAG").count(), 1);
}

/// The data source section is placeholders, appended once, and never
/// rewritten: an operator who pasted a Copernicus key into it must not
/// find it commented out again by the next sync.
#[test]
fn the_ocs_data_source_placeholders_are_appended_once_and_never_rewritten() {
    let (dir, mut project, registry) = project_with(&[]);
    let env = dir.path().join(ENV_FILE);
    std::fs::write(&env, "POSTGRES_PASSWORD=secret\n").unwrap();

    project.state.components.ocs.enabled = true;
    sync(&mut project, &registry, false).unwrap();
    let body = std::fs::read_to_string(&env).unwrap();
    assert!(
        body.contains(
            "\n# OCS data sources (optional): ERA5-Land needs one or both of ECMWF_DATASTORES_*\n\
                 # and EDH_API_KEY, per dataset; WorldPop and CHIRPS3 need none.\n"
        ),
        "{body}"
    );
    // The heading is read in an editor, so it wraps where an editor does.
    // Neither line carries a `=`, which is what keeps both of them comments
    // to every reader in `crate::dotenv` as well as to compose.
    let heading: Vec<&str> = body
        .lines()
        .filter(|line| line.contains("ERA5-Land") || line.contains("CHIRPS3"))
        .collect();
    assert_eq!(heading.len(), 2, "{body}");
    for line in heading {
        assert!(line.len() <= 80, "{} columns: {line}", line.len());
        assert!(!line.contains('='), "{line}");
    }
    assert!(
        body.contains("# ECMWF_DATASTORES_URL=https://cds.climate.copernicus.eu/api\n"),
        "the endpoint is the same for everyone, so it is pre-filled: {body}"
    );
    for var in ["ECMWF_DATASTORES_KEY", "EDH_API_KEY", "CDSE_S3_SECRET_KEY"] {
        assert!(body.contains(&format!("# {var}=\n")), "{var}: {body}");
    }
    // Commented, so nothing is set: the compose file's `${VAR:-}` then
    // passes an empty value, which OCS reads as absent.
    for var in OCS_DATA_SOURCE_ENV_VARS {
        assert_eq!(crate::dotenv::non_empty(&body, var), None, "{var}");
    }

    // A second sync adds nothing.
    assert!(!sync(&mut project, &registry, true).unwrap().drift);

    // And a filled-in value keeps the section from being appended again.
    let filled = body.replace("# ECMWF_DATASTORES_KEY=", "ECMWF_DATASTORES_KEY=mine");
    std::fs::write(&env, &filled).unwrap();
    sync(&mut project, &registry, false).unwrap();
    assert_eq!(std::fs::read_to_string(&env).unwrap(), filled);
    assert_eq!(filled.matches("OCS data sources").count(), 1);
}

/// The mount and the config key arrive together: a plugin directory that
/// is mounted and not configured is a mount OCS never looks in.
#[test]
fn a_plugin_directory_adds_the_mount_and_the_config_key() {
    let mut components = Components::default();
    components.ocs.enabled = true;
    let (dir, mut project, registry) = project_with_components(components);
    let compose = dir.path().join(OCS_COMPOSE);
    assert!(!read(&compose).contains("/app/plugins"), "nothing to mount");

    std::fs::create_dir_all(project.ocs_plugins_path().join("datasets")).unwrap();
    let report = sync(&mut project, &registry, true).unwrap();
    assert!(report.drift, "the mount and the key are both missing");
    assert!(
        !read(&project.ocs_config_path()).contains(&format!("\n{OCS_PLUGINS_KEY}:")),
        "--check writes nothing"
    );

    sync(&mut project, &registry, false).unwrap();
    assert!(
        read(&compose).contains("- ./ocs/plugins:/app/plugins:ro"),
        "{}",
        read(&compose)
    );
    let config = read(&project.ocs_config_path());
    assert!(config.ends_with("plugins_dir: /app/plugins\n"), "{config}");
    assert!(
        config.contains("laos-climate-service"),
        "the rest of the operator's file is untouched: {config}"
    );

    // Idempotent, and an operator's own value is never moved.
    assert!(!sync(&mut project, &registry, true).unwrap().drift);
    std::fs::write(
        project.ocs_config_path(),
        "id: mine\nplugins_dir: /somewhere/else\n",
    )
    .unwrap();
    sync(&mut project, &registry, false).unwrap();
    assert_eq!(
        read(&project.ocs_config_path()),
        "id: mine\nplugins_dir: /somewhere/else\n"
    );
}

/// Only the one key changes, in a file that has it and in one that does
/// not: everything else in it is the operator's.
#[test]
fn the_read_only_switch_edits_one_key_and_leaves_the_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        set_read_only(dir.path(), true).unwrap(),
        None,
        "no file to edit yet"
    );

    let path = dir.path().join(OCS_DIR).join(OCS_CONFIG_FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = "# my instance\nid: mine\n\nextent:\n  read_only: true\n  name: Mine\n\n\
                        # a note about ingestion\n# read_only: true\n";
    std::fs::write(&path, original).unwrap();

    // Appended: neither the nested key nor the commented one is this key.
    assert_eq!(
        set_read_only(dir.path(), true).unwrap(),
        Some(KeyEdit::Appended)
    );
    let body = read(&path);
    assert!(body.starts_with(original), "{body}");
    assert!(body.ends_with("\nread_only: true\n"), "{body}");
    assert!(
        body.contains("  read_only: true\n  name: Mine\n"),
        "the nested key is someone else's: {body}"
    );

    // Rewritten in place, and nothing else moves.
    assert_eq!(
        set_read_only(dir.path(), false).unwrap(),
        Some(KeyEdit::Rewritten)
    );
    let flipped = read(&path);
    assert_eq!(
        flipped,
        body.replace("\nread_only: true\n", "\nread_only: false\n")
    );

    // Already that value: no write at all.
    assert_eq!(
        set_read_only(dir.path(), false).unwrap(),
        Some(KeyEdit::Unchanged)
    );
    assert_eq!(read(&path), flipped);
}

/// A trailing comment says why the value is what it is, so it survives.
#[test]
fn setting_a_key_keeps_its_trailing_comment_and_finds_the_first_one() {
    let (out, edit) = set_config_key("read_only: false  # public demo\n", "read_only", "true");
    assert_eq!(edit, KeyEdit::Rewritten);
    assert_eq!(out, "read_only: true  # public demo\n");

    // Two active assignments is not valid YAML, but the first is the one
    // a parser would report, so it is the one that is edited.
    let (out, _) = set_config_key("read_only: false\nread_only: false\n", "read_only", "true");
    assert_eq!(out, "read_only: true\nread_only: false\n");

    // A key that is a prefix of another is not that other key.
    assert!(!has_config_key("read_only_mode: true\n", "read_only"));
    assert!(has_config_key("read_only: true\n", "read_only"));
    assert!(!has_config_key("  read_only: true\n", "read_only"));
    assert!(!has_config_key("# read_only: true\n", "read_only"));
}

#[test]
fn the_scaffold_is_created_when_missing_and_never_overwritten() {
    let (dir, mut project, registry) = project_with(&[]);
    let path = project.ocs_config_path();

    // Someone else's file is kept byte for byte.
    std::fs::create_dir_all(dir.path().join(OCS_DIR)).unwrap();
    std::fs::write(&path, "id: mine\n").unwrap();
    assert!(
        write_ocs_config(dir.path(), &OcsConfigSpec::default())
            .unwrap()
            .is_none()
    );
    assert_eq!(read(&path), "id: mine\n");

    project.state.components.ocs.enabled = true;
    sync(&mut project, &registry, false).unwrap();
    assert_eq!(read(&path), "id: mine\n", "sync leaves it alone too");

    // A component enabled with no file at all gets the example.
    std::fs::remove_file(&path).unwrap();
    let report = sync(&mut project, &registry, true).unwrap();
    assert!(report.drift);
    assert!(!path.exists(), "--check writes nothing");
    sync(&mut project, &registry, false).unwrap();
    assert!(read(&path).contains("laos-climate-service"));
}

#[test]
fn refresh_env_pin_moves_only_the_commented_line() {
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join(ENV_FILE);
    std::fs::write(
        &env,
        "POSTGRES_PASSWORD=secret\n\
             # CHAPKIT_EWARS_MODEL_IMAGE_TAG=sha-1111111\n\
             AUTO_ARIMA_CHAPKIT_IMAGE_TAG=sha-2222222\n",
    )
    .unwrap();

    assert!(refresh_env_pin(dir.path(), "CHAPKIT_EWARS_MODEL_IMAGE_TAG", "sha-3333333").unwrap());
    let body = std::fs::read_to_string(&env).unwrap();
    assert_eq!(
        body,
        "POSTGRES_PASSWORD=secret\n\
             # CHAPKIT_EWARS_MODEL_IMAGE_TAG=sha-3333333\n\
             AUTO_ARIMA_CHAPKIT_IMAGE_TAG=sha-2222222\n"
    );

    // Already there: nothing to do. Active line: the operator's, untouched.
    assert!(!refresh_env_pin(dir.path(), "CHAPKIT_EWARS_MODEL_IMAGE_TAG", "sha-3333333").unwrap());
    assert!(!refresh_env_pin(dir.path(), "AUTO_ARIMA_CHAPKIT_IMAGE_TAG", "sha-4444444").unwrap());
    assert!(!refresh_env_pin(dir.path(), "NOPE_IMAGE_TAG", "sha-4444444").unwrap());
    assert_eq!(std::fs::read_to_string(&env).unwrap(), body);
    assert!(!refresh_env_pin(&dir.path().join("missing"), "X", "y").unwrap());
}

/// The commented pin in `.env` carries the digest, so uncommenting it keeps
/// the image the overlay pins.
#[test]
fn the_env_pin_carries_the_digest() {
    let (dir, mut project, registry) = project_with(&["chapkit_ewars_model"]);
    let env = dir.path().join(ENV_FILE);
    std::fs::write(&env, "POSTGRES_PASSWORD=secret\n").unwrap();
    let digest = format!("sha256:{}", "a".repeat(64));
    let model = project.state.models.get_mut("chapkit_ewars_model").unwrap();
    model.image_digest = Some(digest.clone());
    let tag = model.image_tag.clone();
    sync(&mut project, &registry, false).unwrap();
    assert!(std::fs::read_to_string(&env).unwrap().contains(&format!(
        "\n# CHAPKIT_EWARS_MODEL_IMAGE_TAG={tag}@{digest}\n"
    )));
}

#[test]
fn check_counts_a_missing_env_pin_as_drift() {
    let (dir, mut project, registry) = project_with(&["chapkit_ewars_model"]);
    let env = dir.path().join(ENV_FILE);
    std::fs::write(&env, "POSTGRES_PASSWORD=secret\n").unwrap();
    let report = sync(&mut project, &registry, true).unwrap();
    assert!(report.drift);
    assert_eq!(names(&report.written), vec![".env"]);
    assert_eq!(
        std::fs::read_to_string(&env).unwrap(),
        "POSTGRES_PASSWORD=secret\n"
    );

    sync(&mut project, &registry, false).unwrap();
    let pinned = project.state.models["chapkit_ewars_model"].pinned_tag();
    assert!(
        std::fs::read_to_string(&env)
            .unwrap()
            .contains(&format!("\n# CHAPKIT_EWARS_MODEL_IMAGE_TAG={pinned}\n"))
    );
    assert!(!sync(&mut project, &registry, true).unwrap().drift);
}

#[test]
fn the_base_file_is_rendered_from_the_embedded_copy_by_default() {
    let (dir, mut project, registry) = project_with(&["chapkit_ewars_model"]);
    let base = dir.path().join(BASE_COMPOSE);
    let original = read(&base);
    assert!(
        original.starts_with(
            "# Generated by chap-devops from .chap-devops/; edit there and run `chap-devops sync`.\nservices:\n"
        ),
        "{original}"
    );

    // A hand edit of compose.yml is drift, and a real sync restores it.
    std::fs::write(&base, "services: {}\n").unwrap();
    let report = sync(&mut project, &registry, true).unwrap();
    assert!(report.drift);
    assert_eq!(names(&report.written), vec!["compose.yml"]);
    assert_eq!(read(&base), "services: {}\n", "--check writes nothing");

    let report = sync(&mut project, &registry, false).unwrap();
    assert_eq!(names(&report.written), vec!["compose.yml"]);
    assert_eq!(read(&base), original);
    assert!(!sync(&mut project, &registry, true).unwrap().drift);
}

#[test]
fn a_fetched_source_renders_from_the_cached_copy() {
    let (dir, mut project, registry) = project_with(&["chapkit_ewars_model"]);
    let body = "services:\n  chap:\n    image: ghcr.io/x:${CHAP_IMAGE_TAG:-latest}\n";
    cache(&mut project, "v2.3.1", body);

    let report = sync(&mut project, &registry, false).unwrap();
    assert!(report.warnings.is_empty(), "{report:?}");
    // compose.chap-devops.yml labels the services the base file defines, and this
    // one defines chap alone.
    assert_eq!(
        names(&report.written),
        vec!["compose.yml", "compose.chap-devops.yml"]
    );
    let chap_devops = read(&dir.path().join(crate::project::CHAP_DEVOPS_COMPOSE));
    assert!(!chap_devops.contains("worker:"), "{chap_devops}");
    let base = read(&dir.path().join(BASE_COMPOSE));
    assert_eq!(
        base,
        format!(
            "# Generated by chap-devops from .chap-devops/; edit there and run `chap-devops sync`.\n\
                 # chap-core compose.ghcr.yml at v2.3.1\n\
                 {body}"
        )
    );
    assert!(!sync(&mut project, &registry, true).unwrap().drift);
}

#[test]
fn an_edited_cached_copy_is_followed_but_reported() {
    let (dir, mut project, registry) = project_with(&[]);
    cache(
        &mut project,
        "v2.3.1",
        "services:\n  chap:\n    image: x:${CHAP_IMAGE_TAG:-latest}\n",
    );
    sync(&mut project, &registry, false).unwrap();

    // The recorded checksum no longer matches, but the file on disk is
    // what the operator has: follow it, and say so.
    let cached = project.cached_compose_path().unwrap();
    std::fs::write(
        &cached,
        "services:\n  chap:\n    image: y:${CHAP_IMAGE_TAG:-latest}\n",
    )
    .unwrap();
    let report = sync(&mut project, &registry, false).unwrap();
    assert_eq!(report.warnings.len(), 1, "{report:?}");
    assert!(report.warnings[0].contains("compose.chap-core.v2.3.1.yml"));
    assert!(report.warnings[0].contains("checksum"));
    assert!(read(&dir.path().join(BASE_COMPOSE)).contains("image: y:"));

    // And a cached copy that is gone leaves compose.yml alone.
    let before = read(&dir.path().join(BASE_COMPOSE));
    std::fs::remove_file(&cached).unwrap();
    let report = sync(&mut project, &registry, false).unwrap();
    assert_eq!(report.warnings.len(), 1, "{report:?}");
    assert!(report.warnings[0].contains("is missing"));
    assert!(!names(&report.written).contains(&"compose.yml".to_string()));
    assert!(!names(&report.unchanged).contains(&"compose.yml".to_string()));
    assert_eq!(read(&dir.path().join(BASE_COMPOSE)), before);
    assert!(
        !project
            .state
            .rendered_files
            .contains(&BASE_COMPOSE.to_string())
    );
}

#[test]
fn set_env_chap_tag_moves_the_active_line_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join(ENV_FILE);
    std::fs::write(
        &env,
        "POSTGRES_PASSWORD=secret\n\
             CHAP_IMAGE_TAG=v2.3.0\n\
             # CHAPKIT_EWARS_MODEL_IMAGE_TAG=sha-1111111\n",
    )
    .unwrap();

    assert_eq!(
        set_env_chap_tag(dir.path(), "v2.3.0", "v2.3.1").unwrap(),
        EnvTag::Updated
    );
    assert_eq!(
        read(&env),
        "POSTGRES_PASSWORD=secret\n\
             CHAP_IMAGE_TAG=v2.3.1\n\
             # CHAPKIT_EWARS_MODEL_IMAGE_TAG=sha-1111111\n"
    );
    // Running it again is a no-op, not a second rewrite.
    assert_eq!(
        set_env_chap_tag(dir.path(), "v2.3.0", "v2.3.1").unwrap(),
        EnvTag::Updated
    );
    assert!(read(&env).contains("CHAP_IMAGE_TAG=v2.3.1\n"));
}

/// The tag the deployment runs is the one on the last active line, so that
/// is the one `chap-devops update` compares against and the one it moves - and
/// the duplicate that made the answer ambiguous does not survive the
/// write.
#[test]
fn set_env_chap_tag_follows_the_line_compose_reads() {
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join(ENV_FILE);

    // Two active lines: compose runs the second, so the first is not what
    // "the recorded tag" means.
    std::fs::write(
        &env,
        "CHAP_IMAGE_TAG=v1.0.0\nPOSTGRES_DB=chap_core\nCHAP_IMAGE_TAG=v2.3.0\n",
    )
    .unwrap();
    assert_eq!(
        set_env_chap_tag(dir.path(), "v2.3.0", "v2.3.1").unwrap(),
        EnvTag::Updated
    );
    assert_eq!(read(&env), "CHAP_IMAGE_TAG=v2.3.1\nPOSTGRES_DB=chap_core\n");

    // And the other way round: the last line is the operator's own value,
    // whatever an earlier line says, so nothing is touched.
    std::fs::write(&env, "CHAP_IMAGE_TAG=v2.3.0\nCHAP_IMAGE_TAG=v1.0.0\n").unwrap();
    assert_eq!(
        set_env_chap_tag(dir.path(), "v2.3.0", "v2.3.1").unwrap(),
        EnvTag::Foreign("v1.0.0".to_string())
    );
    assert_eq!(read(&env), "CHAP_IMAGE_TAG=v2.3.0\nCHAP_IMAGE_TAG=v1.0.0\n");

    // The shapes compose accepts are the shapes this reads, and an
    // `export ` prefix is the operator's to keep.
    std::fs::write(&env, "export CHAP_IMAGE_TAG=\"v2.3.0\"\n").unwrap();
    assert_eq!(
        set_env_chap_tag(dir.path(), "v2.3.0", "v2.3.1").unwrap(),
        EnvTag::Updated
    );
    assert_eq!(read(&env), "export CHAP_IMAGE_TAG=v2.3.1\n");
}

#[test]
fn set_env_chap_tag_keeps_its_hands_off_the_operators_choices() {
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join(ENV_FILE);

    // The generated placeholder: commented, so the deployment follows the
    // compose default and uncommenting it is the operator's call.
    std::fs::write(&env, "# CHAP_IMAGE_TAG=latest\n").unwrap();
    assert_eq!(
        set_env_chap_tag(dir.path(), "latest", "v2.3.1").unwrap(),
        EnvTag::Commented
    );
    assert_eq!(read(&env), "# CHAP_IMAGE_TAG=latest\n");

    // An active line with someone else's value.
    std::fs::write(&env, "CHAP_IMAGE_TAG=v1.0.0\n").unwrap();
    assert_eq!(
        set_env_chap_tag(dir.path(), "v2.3.0", "v2.3.1").unwrap(),
        EnvTag::Foreign("v1.0.0".to_string())
    );
    assert_eq!(read(&env), "CHAP_IMAGE_TAG=v1.0.0\n");

    // Never mentioned, and no file at all.
    std::fs::write(&env, "POSTGRES_DB=chap_core\n").unwrap();
    assert_eq!(
        set_env_chap_tag(dir.path(), "v2.3.0", "v2.3.1").unwrap(),
        EnvTag::Absent
    );
    assert_eq!(read(&env), "POSTGRES_DB=chap_core\n");
    assert_eq!(
        set_env_chap_tag(&dir.path().join("elsewhere"), "a", "b").unwrap(),
        EnvTag::NoFile
    );
}

/// `chap-devops update --chap-tag` writes through the same rule: the one active
/// line moves, in either direction, and only when it still says what the
/// project recorded.
#[test]
fn set_env_chap_tag_moves_a_release_to_a_moving_tag_and_back() {
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join(ENV_FILE);
    std::fs::write(&env, "CHAP_IMAGE_TAG=v2.3.1\nPOSTGRES_DB=chap_core\n").unwrap();

    assert_eq!(
        set_env_chap_tag(dir.path(), "v2.3.1", "dev").unwrap(),
        EnvTag::Updated
    );
    assert_eq!(read(&env), "CHAP_IMAGE_TAG=dev\nPOSTGRES_DB=chap_core\n");

    assert_eq!(
        set_env_chap_tag(dir.path(), "dev", "v2.3.1").unwrap(),
        EnvTag::Updated
    );
    assert_eq!(read(&env), "CHAP_IMAGE_TAG=v2.3.1\nPOSTGRES_DB=chap_core\n");

    // And a line the operator pinned themselves is still theirs: the
    // switch is recorded, the file is not touched, and the caller warns.
    std::fs::write(&env, "CHAP_IMAGE_TAG=sha-fa880a1\n").unwrap();
    assert_eq!(
        set_env_chap_tag(dir.path(), "v2.3.1", "dev").unwrap(),
        EnvTag::Foreign("sha-fa880a1".to_string())
    );
    assert_eq!(read(&env), "CHAP_IMAGE_TAG=sha-fa880a1\n");
}

#[test]
fn overlay_name_shape() {
    assert!(is_overlay_name("compose.chapkit-ewars-model.yml"));
    assert!(!is_overlay_name(MARKETPLACE_COMPOSE));
    assert!(!is_overlay_name(BASE_COMPOSE));
    assert!(!is_overlay_name("compose.yaml"));
    assert!(!is_overlay_name("notes.yml"));
}

/// A service id chap-devops keeps for itself that reached the state by hand is
/// refused at render time, before `compose.chap-devops.yml` is written over.
#[test]
fn sync_refuses_a_model_on_a_reserved_service_id() {
    let (dir, mut project, registry) = project_with(&["chapkit_ewars_model"]);
    let chap_devops_before = read(&dir.path().join("compose.chap-devops.yml"));
    let model = project.state.models.get_mut("chapkit_ewars_model").unwrap();
    model.service_id = "chap-devops".to_string();
    model.compose_file = "compose.chap-devops.yml".to_string();
    let err = sync(&mut project, &registry, false).unwrap_err();
    assert!(
        format!("{err:#}").contains("chap-devops uses for its own services"),
        "{err:#}"
    );
    assert_eq!(
        read(&dir.path().join("compose.chap-devops.yml")),
        chap_devops_before
    );
}
