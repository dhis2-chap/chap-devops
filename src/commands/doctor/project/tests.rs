use super::super::test_support::*;
use super::*;

#[test]
fn the_file_list_is_what_init_writes() {
    let files = project_files(&Components::default());
    assert_eq!(files.len(), 7);
    for name in [
        ".chap-devops/project.yaml",
        ".chap-devops/models.yaml",
        ".chap-devops/components.yaml",
        ".env",
        "compose.yml",
        "compose.chap-devops.yml",
        "compose.marketplace.yml",
    ] {
        assert!(files.contains(&name.to_string()), "{name} is missing");
    }

    // A component adds its own compose file, between the override and the
    // umbrella; chap-core off takes the base stack out of the list.
    let mut components = Components::default();
    components.set_enabled(crate::components::Component::Ocs, true);
    assert_eq!(
        project_files(&components),
        vec![
            ".chap-devops/project.yaml",
            ".chap-devops/models.yaml",
            ".chap-devops/components.yaml",
            ".env",
            "compose.yml",
            "compose.chap-devops.yml",
            "compose.ocs.yml",
            "compose.marketplace.yml",
        ]
    );
    components.set_enabled(crate::components::Component::ChapCore, false);
    assert_eq!(
        project_files(&components),
        vec![
            ".chap-devops/project.yaml",
            ".chap-devops/models.yaml",
            ".chap-devops/components.yaml",
            ".env",
            "compose.ocs.yml",
            "compose.marketplace.yml",
        ]
    );

    let (status, detail, fix) = files_verdict(6, &[], &[]);
    assert_eq!(status, Status::Ok);
    assert_eq!(detail, "all 6 present");
    assert_eq!(fix, None);

    let (status, detail, fix) =
        files_verdict(6, &[".env".to_string(), "compose.yml".to_string()], &[]);
    assert_eq!(status, Status::Fail);
    assert_eq!(detail, "missing .env, compose.yml");
    let fix = fix.unwrap();
    assert!(fix.contains("chap-devops sync") && fix.contains("chap-devops init --force"));
}

/// The count is of the files this check looks for, and the line says so
/// rather than reading as "nothing is missing from this deployment": a
/// component's own directory is not in the list, and `dhis2/dhis.conf` is a
/// file DHIS2 will not start without.
#[test]
fn the_files_line_names_the_component_directories_it_did_not_count() {
    assert_eq!(files_verdict(8, &[], &[]).1, "all 8 present");
    assert_eq!(
        files_verdict(8, &[], &["dhis2"]).1,
        "all 8 present; dhis2/ is on the `components` line"
    );
    assert_eq!(
        files_verdict(9, &[], &["ocs", "dhis2"]).1,
        "all 9 present; ocs/ and dhis2/ are on the `components` line"
    );
    assert_eq!(
        files_verdict(9, &[], &["ocs", "s3", "dhis2"]).1,
        "all 9 present; ocs/, s3/ and dhis2/ are on the `components` line"
    );
    // The fail has no room for it: the fix is the one thing to do.
    assert_eq!(
        files_verdict(8, &["compose.dhis2.yml".to_string()], &["dhis2"]).1,
        "missing compose.dhis2.yml"
    );
}

/// Which directories the line names: the enabled components' own, taken
/// from [`Component::dir`] so a component added later is named without this
/// check learning it.
#[test]
fn the_files_check_points_at_the_directories_of_the_enabled_components() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut components = Components::default();
    components.set_enabled(Component::Dhis2, true);
    for name in project_files(&components) {
        let path = root.join(&name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "written\n").unwrap();
    }

    // Every mandatory file is there and the line still says where the
    // DHIS2 config is judged, which is the whole of what was untrue before.
    let check = files_check(root, &components);
    assert_eq!(check.status, Status::Ok);
    assert_eq!(
        check.detail, "all 8 present; dhis2/ is on the `components` line",
        "{check:?}"
    );

    // Whether the operator's file is there changes nothing on this line -
    // the `components` line is where that is judged - and the tail is what
    // sends the reader to it. It was absent for the assertion above.
    std::fs::create_dir_all(root.join(DHIS2_DIR)).unwrap();
    std::fs::write(root.join(DHIS2_DIR).join(DHIS2_CONFIG_FILE), "x\n").unwrap();
    assert_eq!(files_check(root, &components).detail, check.detail);

    // A deployment with no such component gets no tail.
    let plain = Components::default();
    for name in project_files(&plain) {
        let path = root.join(&name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "written\n").unwrap();
    }
    assert_eq!(files_check(root, &plain).detail, "all 7 present");
}

#[test]
fn the_env_check_reads_the_two_things_that_bite_later() {
    // Nothing to read is not a fault: `project-files` already said so.
    assert_eq!(env_verdict(None).0, Status::Skip);

    let good = "POSTGRES_PASSWORD=0123456789abcdef\n\
                    CHAP_API_TOKEN=sekret\n\
                    # CHAP_IMAGE_TAG=latest\n";
    let (status, detail, fix) = env_verdict(Some(good));
    assert_eq!(status, Status::Ok);
    assert_eq!(
        detail,
        "auth on, POSTGRES_PASSWORD set, CHAP_IMAGE_TAG present"
    );
    assert_eq!(fix, None);

    // Authentication off is reported, not complained about.
    let open = "POSTGRES_PASSWORD=0123456789abcdef\n# CHAP_API_TOKEN=\nCHAP_IMAGE_TAG=v2.3.1\n";
    let (status, detail, _) = env_verdict(Some(open));
    assert_eq!(status, Status::Ok);
    assert!(detail.starts_with("auth off, "), "{detail}");

    // The default password is the one that has to be moved off.
    let (status, detail, fix) =
        env_verdict(Some("POSTGRES_PASSWORD=chap\n# CHAP_IMAGE_TAG=latest\n"));
    assert_eq!(status, Status::Warn);
    assert!(detail.contains("the default `chap`"), "{detail}");
    assert!(fix.unwrap().contains("ALTER USER"));

    // An unset one resolves to the same default through compose.
    let (status, detail, _) = env_verdict(Some("# CHAP_IMAGE_TAG=latest\n"));
    assert_eq!(status, Status::Warn);
    assert!(detail.contains("unset"), "{detail}");

    // And a file the pin comments have been cut out of.
    let (status, detail, fix) = env_verdict(Some("POSTGRES_PASSWORD=0123456789abcdef\n"));
    assert_eq!(status, Status::Warn);
    assert!(detail.contains("no CHAP_IMAGE_TAG line"), "{detail}");
    assert!(fix.unwrap().contains("chap-devops sync"));
}

#[test]
fn the_project_line_warns_when_the_name_is_empty() {
    let (status, detail, fix) = project_name_verdict(Some("demo-1ab2c3"), &[]);
    assert_eq!(status, Status::Ok);
    assert_eq!(
        detail,
        "demo-1ab2c3 (recorded in .chap-devops/project.yaml)"
    );
    assert_eq!(fix, None);

    // An empty name is a hand edit: compose falls back to the directory.
    let (status, detail, fix) = project_name_verdict(None, &[]);
    assert_eq!(status, Status::Warn);
    assert!(detail.contains("`compose_project` is empty"), "{detail}");
    let fix = fix.unwrap();
    assert!(fix.contains("chap-devops sync"), "{fix}");
}

/// Two directories with one name, as `--adopt-identity` on the same machine
/// leaves them, share every container and volume.
#[test]
fn the_project_line_warns_when_another_directory_has_the_same_name() {
    let other = PathBuf::from("/srv/chapx");
    let (status, detail, fix) = project_name_verdict(Some("chapx-31ab52"), &[other]);
    assert_eq!(status, Status::Warn);
    assert_eq!(
        detail,
        "chapx-31ab52, and /srv/chapx records the same name, so these directories use the same \
         containers and volumes"
    );
    assert_eq!(
        fix.as_deref(),
        Some("use only one of these directories, or remove /srv/chapx")
    );
}

/// When `.chap-devops/project.yaml` was created, in these tests.
const CREATED: u64 = 1_790_147_400;

#[test]
fn a_database_volume_older_than_the_deployment_is_a_warning() {
    let prefix = "demo-1ab2c3_";
    let db = "demo-1ab2c3_chap-db";

    // Nothing started yet: nothing to be suspicious of.
    let (status, detail, _) = volume_verdict(prefix, db, &[], Some(CREATED), &[], None);
    assert_eq!(status, Status::Ok);
    assert!(detail.contains("no demo-1ab2c3_* volume yet"), "{detail}");

    // Volumes this deployment made itself.
    let mine = volumes(&[(db, CREATED + 60), ("demo-1ab2c3_logs", CREATED + 60)]);
    let (status, detail, fix) = volume_verdict(prefix, db, &mine, Some(CREATED), &[], None);
    assert_eq!(status, Status::Ok);
    assert_eq!(detail, "2 volumes named demo-1ab2c3_*");
    assert_eq!(fix, None);

    // With a size for the OCS data volume, the line says how much data a
    // `down --volumes` would destroy rather than only how many volumes.
    let (status, detail, _) = volume_verdict(
        prefix,
        db,
        &mine,
        Some(CREATED),
        &[],
        Some(("demo-1ab2c3_ocs_data", 217_088 * 1024)),
    );
    assert_eq!(status, Status::Ok);
    assert_eq!(
        detail,
        "2 volumes named demo-1ab2c3_*; demo-1ab2c3_ocs_data holds 212.0 MB"
    );

    // A database volume that predates the directory it belongs to came
    // from somewhere else, and still holds that deployment's password.
    let inherited = volumes(&[(db, CREATED - 86_400), ("demo-1ab2c3_logs", CREATED + 60)]);
    let (status, detail, fix) = volume_verdict(prefix, db, &inherited, Some(CREATED), &[], None);
    assert_eq!(status, Status::Warn);
    assert!(
        detail.starts_with(
            "the database volume demo-1ab2c3_chap-db predates this deployment; \
                 if chap-core cannot log in, it belongs to an earlier deployment with \
                 the same name"
        ),
        "{detail}"
    );
    assert!(fix.unwrap().contains("chap-devops down --volumes"));

    // Neither time is guaranteed: a filesystem that records no creation
    // time, and a docker that did not say, each cost the comparison only.
    assert_eq!(
        volume_verdict(prefix, db, &inherited, None, &[], None).0,
        Status::Ok
    );
    let undated = vec![(db.to_string(), None)];
    assert_eq!(
        volume_verdict(prefix, db, &undated, Some(CREATED), &[], None).0,
        Status::Ok
    );
    // And an old volume that is not the database is not this warning.
    let other = volumes(&[("demo-1ab2c3_logs", CREATED - 86_400)]);
    assert_eq!(
        volume_verdict(prefix, db, &other, Some(CREATED), &[], None).0,
        Status::Ok
    );
}

/// The volume of a model or a component this deployment no longer enables
/// is data nothing will ever mount again, and nothing else names it: the
/// overlay that declared it is gone, so `down --volumes` cannot reach it
/// either.
#[test]
fn a_volume_of_something_no_longer_enabled_is_a_warning_naming_it() {
    let prefix = "demo-1ab2c3_";
    let db = "demo-1ab2c3_chap-db";
    let ewars = "demo-1ab2c3_ck_chapkit_ewars_model_data";

    let held = volumes(&[(db, CREATED + 60), (ewars, CREATED + 60)]);
    let (status, detail, fix) = volume_verdict(
        prefix,
        db,
        &held,
        Some(CREATED),
        &[ewars.to_string(), "demo-1ab2c3_ocs_data".to_string()],
        None,
    );
    assert_eq!(status, Status::Warn);
    assert_eq!(
        detail,
        "leftover volumes from disabled models or components: \
             demo-1ab2c3_ck_chapkit_ewars_model_data, demo-1ab2c3_ocs_data"
    );
    // The OCS size stays off this line even when it was measured.
    let (_, detail, _) = volume_verdict(
        prefix,
        db,
        &volumes(&[(ewars, CREATED + 60)]),
        Some(CREATED),
        &[ewars.to_string()],
        Some(("demo-1ab2c3_ocs_data", 40 * 1024)),
    );
    assert!(!detail.contains("holds"), "{detail}");
    let fix = fix.expect("a leftover volume has something to do about it");
    assert!(
        fix.contains("chap-devops models disable <id> --purge"),
        "{fix}"
    );
    assert!(
        fix.contains("chap-devops components disable <name> --purge"),
        "{fix}"
    );
    assert!(fix.contains("docker volume rm <name>"), "{fix}");
    // `down --volumes` is the one answer that does not work here.
    assert!(!fix.contains("--volumes"), "{fix}");

    // A database volume older than the deployment is the worse of the two
    // findings, and the one the line reports.
    let inherited = volumes(&[(db, CREATED - 86_400), (ewars, CREATED + 60)]);
    let (status, detail, _) = volume_verdict(
        prefix,
        db,
        &inherited,
        Some(CREATED),
        &[ewars.to_string()],
        None,
    );
    assert_eq!(status, Status::Warn);
    assert!(detail.starts_with("the database volume"), "{detail}");

    // A leftover OCS volume is measured too: it is data nothing will
    // mount again, and its size is what decides whether to keep it.
    let (_, detail, _) = volume_verdict(
        prefix,
        db,
        &held,
        Some(CREATED),
        &["demo-1ab2c3_ocs_data".to_string()],
        Some(("demo-1ab2c3_ocs_data", 3 * 1024 * 1024 * 1024)),
    );
    assert_eq!(
        detail,
        "leftover volumes from disabled models or components: \
             demo-1ab2c3_ocs_data; demo-1ab2c3_ocs_data holds 3.0 GB"
    );
}

/// Which of a deployment's volumes belong to nothing it still enables.
#[test]
fn the_leftovers_are_the_volumes_of_disabled_models_and_components() {
    let prefix = "demo-1ab2c3_";
    let names: Vec<String> = [
        "demo-1ab2c3_chap-db",
        "demo-1ab2c3_ck_chapkit_ewars_model_data",
        "demo-1ab2c3_ck_auto_arima_chapkit_data",
        "demo-1ab2c3_ocs_data",
        "demo-1ab2c3_s3_data",
        "otherdemo_ck_chapkit_ewars_model_data",
    ]
    .iter()
    .map(|n| n.to_string())
    .collect();

    // One model enabled, no component but chap-core: everything else
    // under this prefix is a leftover, and the other deployment's volume
    // is not this deployment's business.
    let models = vec!["chapkit_ewars_model".to_string()];
    let mut components = Components::default();
    assert_eq!(
        leftover_volumes(prefix, &names, &models, &components),
        [
            "demo-1ab2c3_ck_auto_arima_chapkit_data",
            "demo-1ab2c3_ocs_data",
            "demo-1ab2c3_s3_data"
        ]
    );

    // Turning the components on leaves only the disabled model's.
    components.set_enabled(Component::Ocs, true);
    components.set_enabled(Component::S3, true);
    assert_eq!(
        leftover_volumes(prefix, &names, &models, &components),
        ["demo-1ab2c3_ck_auto_arima_chapkit_data"]
    );

    // And with both models enabled there is nothing left over: the
    // database and chap-core's own volumes are the base stack's.
    let both = vec![
        "chapkit_ewars_model".to_string(),
        "auto_arima_chapkit".to_string(),
    ];
    assert!(leftover_volumes(prefix, &names, &both, &components).is_empty());
}

#[test]
fn a_port_is_only_a_conflict_when_someone_else_holds_it() {
    let nothing = ports::OwnPorts::default();
    let free = |_: u16| false;
    let taken = |_: u16| true;

    let api = claim(API_SERVICE, 8000);
    let check = port_check(&api, &nothing, &free, None, None);
    assert_eq!(check.status, Status::Ok);
    assert_eq!(check.id, "api-port");
    assert_eq!(check.name, "api port");
    assert_eq!(check.detail, "8000 is free");

    let check = port_check(&api, &nothing, &taken, Some(8001), None);
    assert_eq!(check.status, Status::Fail);
    assert_eq!(check.detail, "8000 is in use by something else");
    assert_eq!(
        check.fix.unwrap(),
        ports::busy_line(&api, Some(8001)),
        "the fix is the sentence `chap-devops up` would have failed with"
    );

    // A port this deployment's own container publishes is ours, exactly as
    // the `up` preflight treats it.
    let running = ports::OwnPorts::services(&[API_SERVICE]);
    let check = port_check(&api, &running, &taken, None, None);
    assert_eq!(check.status, Status::Ok);
    assert!(check.detail.contains("this deployment's own container"));

    let model = claim("chapkit-ewars-model", 5001);
    let check = port_check(&model, &nothing, &taken, None, None);
    assert_eq!(check.id, "port-chapkit-ewars-model");
    assert_eq!(check.name, "port chapkit-ewars-model");
    assert!(check.fix.unwrap().contains("chap-devops models unexpose"));
}

/// `.env` moving the API port is the operator's doing, so the line names
/// the file rather than looking like a number out of nowhere.
#[test]
fn the_api_port_line_names_the_file_that_moved_the_port() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = Project {
        dir: dir.path().to_path_buf(),
        state: crate::project::ProjectState::default(),
    };
    project.state.api_port = 8000;

    // Nothing in .env: the recorded port stands, and there is nothing to
    // explain.
    assert_eq!(api_port_note(&project), None);
    // The line compose reads agrees with the recorded one: still nothing.
    std::fs::write(dir.path().join(ENV_FILE), "CHAP_API_PORT=8000\n").unwrap();
    assert_eq!(api_port_note(&project), None);

    std::fs::write(dir.path().join(ENV_FILE), "CHAP_API_PORT=18000\n").unwrap();
    let note = api_port_note(&project).expect("an override is worth a word");
    assert_eq!(
        note,
        "from .env, over the 8000 recorded in .chap-devops/project.yaml"
    );

    let claim = claim(API_SERVICE, project.effective_api_port());
    let check = port_check(
        &claim,
        &ports::OwnPorts::default(),
        &|_| false,
        None,
        Some(&note),
    );
    assert_eq!(
        check.detail,
        "18000 is free (from .env, over the 8000 recorded in .chap-devops/project.yaml)"
    );
}

/// After a takeover the volumes are older than the directory on purpose, so
/// the comparison that warns about it is not made.
#[test]
fn a_takeover_is_not_compared_with_the_age_of_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(CHAP_DEVOPS_DIR)).unwrap();
    let mut project = Project {
        dir: dir.path().to_path_buf(),
        state: crate::project::ProjectState::default(),
    };
    let created = compared_creation(&project);
    project.state.adopted_identity = true;
    assert_eq!(compared_creation(&project), None);
    // Without a creation time the older database volume is no warning.
    let prefix = "chapx-31ab52_";
    let db = "chapx-31ab52_chap-db";
    let older = volumes(&[(db, created.unwrap_or(CREATED).saturating_sub(86_400))]);
    let (status, ..) = volume_verdict(prefix, db, &older, compared_creation(&project), &[], None);
    assert_eq!(status, Status::Ok);
}

#[test]
fn the_api_workers_check_warns_only_where_chap_core_cannot_take_them() {
    // chap-core's compose file at a version that gives each worker its own
    // database connections, and at one that does not.
    let new =
        "services:\n  chap:\n    environment:\n      WEB_CONCURRENCY: ${CHAP_API_WORKERS:-1}\n";
    let old = "services:\n  chap:\n    environment:\n      REDIS_HOST: redis\n";

    // Not set, or 1, is fine at every version.
    let (status, detail, _) = api_workers_verdict(Some("# CHAP_API_WORKERS=1\n"), Some(old));
    assert_eq!(status, Status::Ok);
    assert_eq!(detail, "1 (CHAP_API_WORKERS not set)");
    assert_eq!(api_workers_verdict(None, Some(old)).0, Status::Ok);
    assert_eq!(
        api_workers_verdict(Some("CHAP_API_WORKERS=1\n"), Some(old)),
        (Status::Ok, "1".to_string(), None)
    );

    // More than 1 is fine where chap-core supports it.
    assert_eq!(
        api_workers_verdict(Some("CHAP_API_WORKERS=4\n"), Some(new)),
        (Status::Ok, "4".to_string(), None)
    );

    // And a warning, with the way out, where it does not.
    let (status, detail, fix) = api_workers_verdict(Some("CHAP_API_WORKERS=4\n"), Some(old));
    assert_eq!(status, Status::Warn);
    assert!(detail.contains("v2.7.0 or older"), "{detail}");
    let fix = fix.unwrap();
    assert!(fix.contains("CHAP_API_WORKERS=1"), "{fix}");
    assert!(
        fix.contains("chap-devops update --chap-tag master"),
        "{fix}"
    );

    // Without a compose file to read, nothing is claimed.
    assert_eq!(
        api_workers_verdict(Some("CHAP_API_WORKERS=4\n"), None).0,
        Status::Ok
    );

    // A value gunicorn cannot use.
    for bad in ["0", "four", "-2"] {
        let (status, detail, fix) =
            api_workers_verdict(Some(&format!("CHAP_API_WORKERS={bad}\n")), Some(new));
        assert_eq!(status, Status::Warn, "{bad}");
        assert!(detail.contains("not a number of 1 or more"), "{detail}");
        assert!(fix.unwrap().contains("chap-devops up"));
    }
}
