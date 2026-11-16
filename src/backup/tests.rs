use super::files::{is_compose_file, is_contained_relative};
use super::pg_restore::{PgRestoreError, pg_restore_error_is_ignorable, pg_restore_ignored_count};
use super::volumes::{ComponentVolume, volume_create_args, volume_read_args, volume_write_args};
use super::*;

/// The model restore used to clear `dir/*` alone, so a `.stale` written
/// after the backup survived it. Run for real, with the `sh` and `tar` the
/// script is written for.
#[cfg(unix)]
#[test]
fn the_refill_script_empties_hidden_files_too() {
    use std::process::{Command, Stdio};

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("data");
    let source = temp.path().join("source");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&source).unwrap();
    for name in ["old", ".stale", "..odd"] {
        std::fs::write(dir.join(name), "after the backup").unwrap();
    }
    std::fs::create_dir(dir.join(".hidden-dir")).unwrap();
    std::fs::write(source.join("restored"), "from the backup").unwrap();

    let tar = Command::new("tar")
        .arg("-C")
        .arg(&source)
        .args(["-cf", "-", "."])
        .output()
        .expect("tar");
    assert!(tar.status.success());
    let mut sh = Command::new("sh")
        .arg("-c")
        .arg(refill_script(&dir.display().to_string()))
        .stdin(Stdio::piped())
        .spawn()
        .expect("sh");
    use std::io::Write;
    sh.stdin.take().unwrap().write_all(&tar.stdout).unwrap();
    assert!(sh.wait().unwrap().success());

    let mut left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, vec!["restored".to_string()]);
}

/// A restore into another deployment keeps that deployment's database
/// volume, and the volume only opens with the password it was created
/// with - so those lines of `.env` stay this deployment's.
#[test]
fn a_restore_keeps_the_credentials_of_the_volume_it_keeps() {
    let current = "POSTGRES_USER=chap\nPOSTGRES_PASSWORD=ours\nCHAP_API_TOKEN=ours-token\n";
    let archived = "POSTGRES_USER=chap\nPOSTGRES_PASSWORD=theirs\nPOSTGRES_DB=elsewhere\nCHAP_API_TOKEN=theirs-token\n";
    let (body, moved) = keep_credentials(archived, current, CHAP_DB_CREDENTIALS);
    assert_eq!(moved, vec!["POSTGRES_PASSWORD", "POSTGRES_DB"]);
    assert_eq!(
        env_value(&body, "POSTGRES_PASSWORD").as_deref(),
        Some("ours")
    );
    // Not set here, so compose's default - the one this volume was made
    // with - applies again.
    assert_eq!(env_value(&body, "POSTGRES_DB"), None);
    // Everything else is the archive's to restore.
    assert_eq!(
        env_value(&body, "CHAP_API_TOKEN").as_deref(),
        Some("theirs-token")
    );

    // The same deployment's own backup changes nothing.
    let (same, moved) = keep_credentials(current, current, CHAP_DB_CREDENTIALS);
    assert!(moved.is_empty());
    assert_eq!(same, current);
}

fn manifest() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        created_by: "chap-devops 0.1.0".into(),
        created_at: "2026-09-23T07:10:00Z".into(),
        project: "e2e".into(),
        chap_image_tag: Some("latest".into()),
        files: vec![".env".into(), ".chap-devops/models.yaml".into()],
        database: Some(ManifestDatabase {
            path: DB_MEMBER.into(),
            user: "chap".into(),
            name: "chap_core".into(),
            server_version: Some("17.6".into()),
            size_bytes: 2048,
        }),
        models: vec![ManifestModel {
            id: "chapkit_ewars_model".into(),
            service_id: "chapkit-ewars-model".into(),
            version: "1.0.0".into(),
            image_tag: "sha-fa880a1".into(),
            host_port: Some(5001),
            data_dir: "/app/data".into(),
            user: "chapkit:chapkit".into(),
            volume: "ck_chapkit_ewars_model_data".into(),
            path: Some(model_member("chapkit-ewars-model")),
            size_bytes: 40960,
            skipped: None,
            failed: false,
            quiesce: Some("paused for 1.4 s".into()),
        }],
        components: vec![ManifestComponent {
            name: "ocs".into(),
            service: "ocs".into(),
            volume: "ocs_data".into(),
            data_dir: "/app/data".into(),
            path: Some(component_member("ocs")),
            size_bytes: 4096,
            skipped: None,
            failed: false,
            quiesce: None,
        }],
    }
}

#[test]
fn a_manifest_round_trips_through_yaml() {
    let body = render_manifest(&manifest()).unwrap();
    assert!(body.starts_with("# chap-devops backup manifest"));
    assert!(body.contains("schema_version: 1"));
    assert!(body.contains("service_id: chapkit-ewars-model"));
    assert!(body.contains("quiesce: paused for 1.4 s"));
    assert!(body.contains("path: components/ocs.tar"));
    let back = parse_manifest(&body, Path::new("/tmp/a.tar.gz")).unwrap();
    assert_eq!(back, manifest());
    assert_eq!(back.captured_models().count(), 1);
    assert_eq!(back.captured_components().count(), 1);
    assert_eq!(back.content_bytes(), 2048 + 40960 + 4096);
}

#[test]
fn a_manifest_without_the_optional_parts_still_parses() {
    let body = "schema_version: 1\n\
                    created_by: chap-devops 0.1.0\n\
                    created_at: 2026-09-23T07:10:00Z\n\
                    project: e2e\n\
                    chap_image_tag: latest\n";
    let parsed = parse_manifest(body, Path::new("/tmp/a.tar.gz")).unwrap();
    assert!(parsed.files.is_empty());
    assert!(parsed.database.is_none());
    assert!(parsed.models.is_empty());
    assert!(parsed.components.is_empty());
    assert_eq!(parsed.content_bytes(), 0);
}

#[test]
fn a_newer_schema_is_refused_rather_than_half_understood() {
    let body = "schema_version: 99\n\
                    created_by: chap-devops 9.9.9\n\
                    created_at: 2026-09-23T07:10:00Z\n\
                    project: e2e\n\
                    chap_image_tag: latest\n";
    let err = parse_manifest(body, Path::new("/tmp/a.tar.gz")).unwrap_err();
    assert!(err.to_string().contains("newer than this chap-devops"));

    let err = parse_manifest("not: [a manifest", Path::new("/tmp/a.tar.gz")).unwrap_err();
    assert!(
        err.to_string()
            .contains("not a chap-devops backup manifest")
    );
}

#[test]
fn archive_names_carry_the_project_and_the_stamp() {
    assert_eq!(
        archive_name("e2e", "20260923-071000"),
        "chap-devops-backup-e2e-20260923-071000.tar.gz"
    );
    assert_eq!(
        archive_name("Chap prod (eu)", "20260923-071000"),
        "chap-devops-backup-Chap-prod-eu-20260923-071000.tar.gz"
    );
    assert_eq!(
        archive_name("", "20260923-071000"),
        "chap-devops-backup-20260923-071000.tar.gz"
    );
}

#[test]
fn out_is_a_file_a_directory_or_the_working_directory() {
    let cwd = Path::new("/home/me");
    let name = "chap-devops-backup-e2e-20260923-071000.tar.gz";

    assert_eq!(
        resolve_out_path(None, false, cwd, name),
        PathBuf::from("/home/me").join(name)
    );
    assert_eq!(
        resolve_out_path(Some(Path::new("/backups")), true, cwd, name),
        PathBuf::from("/backups").join(name)
    );
    assert_eq!(
        resolve_out_path(Some(Path::new("/backups/today.tar.gz")), false, cwd, name),
        PathBuf::from("/backups/today.tar.gz")
    );
    // A trailing separator says "directory" even for one that does not exist.
    assert_eq!(
        resolve_out_path(Some(Path::new("nightly/")), false, cwd, name),
        PathBuf::from("nightly").join(name)
    );
}

#[test]
fn a_second_backup_in_the_same_second_gets_a_name_of_its_own() {
    let first = PathBuf::from("/backups/chap-devops-backup-e2e-20260923-071000.tar.gz");
    assert_eq!(unused_path(first.clone(), |_| false), first);

    let taken = [
        first.clone(),
        PathBuf::from("/backups/chap-devops-backup-e2e-20260923-071000-2.tar.gz"),
    ];
    assert_eq!(
        unused_path(first, |p| taken.iter().any(|t| t == p)),
        PathBuf::from("/backups/chap-devops-backup-e2e-20260923-071000-3.tar.gz")
    );
}

#[test]
fn an_archive_is_written_to_a_hidden_sibling_first() {
    assert_eq!(
        temp_archive_path(Path::new("/backups/nightly.tar.gz")),
        PathBuf::from("/backups/.nightly.tar.gz.tmp")
    );
    // Same directory, so the rename that follows stays on one filesystem.
    let tmp = temp_archive_path(Path::new("/backups/nightly.tar.gz"));
    assert_eq!(tmp.parent(), Some(Path::new("/backups")));
}

#[test]
fn utc_conversion_matches_known_instants() {
    assert_eq!(timestamp(0), "1970-01-01T00:00:00Z");
    assert_eq!(stamp(0), "19700101-000000");
    // 2026-09-23T07:10:00Z
    assert_eq!(timestamp(1_790_147_400), "2026-09-23T07:10:00Z");
    assert_eq!(stamp(1_790_147_400), "20260923-071000");
    // A leap day, and the last second of a year.
    assert_eq!(timestamp(1_709_164_800), "2024-02-29T00:00:00Z");
    assert_eq!(timestamp(1_735_689_599), "2024-12-31T23:59:59Z");
    assert!(now() > 1_700_000_000, "the clock is past 2023");
}

#[test]
fn sizes_read_like_du() {
    assert_eq!(human_size(0), "0 B");
    assert_eq!(human_size(912), "912 B");
    assert_eq!(human_size(1024), "1.0 KB");
    assert_eq!(human_size(4400), "4.3 KB");
    assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0 GB");
}

#[test]
fn env_values_come_from_active_lines_only() {
    let body = "# POSTGRES_USER=commented\n\
                    POSTGRES_USER=chapuser\n\
                    export POSTGRES_DB=\"chapdb\"\n\
                    POSTGRES_PASSWORD='p a s s'\n\
                    JUNK\n";
    assert_eq!(
        env_value(body, "POSTGRES_USER").as_deref(),
        Some("chapuser")
    );
    assert_eq!(env_value(body, "POSTGRES_DB").as_deref(), Some("chapdb"));
    assert_eq!(
        env_value(body, "POSTGRES_PASSWORD").as_deref(),
        Some("p a s s")
    );
    assert_eq!(env_value(body, "NOPE"), None);
    assert_eq!(
        postgres_credentials(body),
        ("chapuser".to_string(), "chapdb".to_string())
    );
    // A later line wins, the way compose resolves duplicates.
    assert_eq!(
        env_value("POSTGRES_DB=a\nPOSTGRES_DB=b\n", "POSTGRES_DB").as_deref(),
        Some("b")
    );
    // Nothing to read: chap-core's own defaults.
    assert_eq!(
        postgres_credentials(""),
        ("chap".to_string(), "chap_core".to_string())
    );
}

#[test]
fn the_old_env_is_kept_only_when_it_differs() {
    assert!(keep_env_copy(Some(b"a=1"), Some(b"a=2")));
    assert!(!keep_env_copy(Some(b"a=1"), Some(b"a=1")));
    assert!(!keep_env_copy(Some(b"a=1"), None));
    assert!(!keep_env_copy(None, Some(b"a=1")));
    assert!(!keep_env_copy(None, None));
}

#[test]
fn the_file_list_takes_env_chap_devops_and_the_root_compose_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let write = |rel: &str, body: &str| {
        let path = join_relative(root, rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };
    write(".env", "POSTGRES_USER=chap\n");
    write(".chap-devops/project.yaml", "schema_version: 1\n");
    write(".chap-devops/models.yaml", "{}\n");
    write(
        ".chap-devops/compose.chap-core.v2.3.1.yml",
        "services: {}\n",
    );
    write(".chap-devops/.models.yaml.tmp", "half written");
    // The locks of running commands: the state lock and `chap-devops run`'s.
    write(".chap-devops/lock", "");
    write(".chap-devops/up-chapkit-ewars-model.lock", "");
    write(".chap-devops/tmp/backup-1/manifest.yaml", "staged");
    write("ocs/climate-service.yaml", "sources: []\n");
    write("ocs/extra/regions.csv", "id,name\n");
    write("dhis2/dhis.conf", "connection.dialect = ...\n");
    write("compose.yml", "services: {}\n");
    write("compose.marketplace.yml", "include: []\n");
    write("compose.chapkit-ewars-model.yml", "services: {}\n");
    write("compose.override.yml", "services: {}\n");
    write("notes.md", "not a compose file");
    write("compose.yaml.bak", "not a compose file either");
    write("sub/compose.yml", "someone else's stack");

    assert_eq!(
        project_files(root),
        vec![
            ".env",
            ".chap-devops/compose.chap-core.v2.3.1.yml",
            ".chap-devops/models.yaml",
            ".chap-devops/project.yaml",
            // The component directories are the operator's own files, so
            // they are in the archive like .chap-devops/ is, in Component::ALL
            // order: ocs/ before dhis2/.
            "ocs/climate-service.yaml",
            "ocs/extra/regions.csv",
            "dhis2/dhis.conf",
            "compose.chapkit-ewars-model.yml",
            "compose.marketplace.yml",
            "compose.override.yml",
            "compose.yml",
        ]
    );
}

/// Every component that owns a directory has it in the archive, asked of
/// [`crate::components::Component::dir`] rather than of a list written out
/// here.
///
/// This is the test the bug got past: `ocs/` was collected by name, so
/// `dhis2/dhis.conf` - the one file DHIS2 will not start without, written
/// once and never rewritten - was in no archive ever taken, and nothing
/// said so. Written this way a fifth component with a directory fails here
/// until `project_files` collects it, instead of needing a test of its own
/// that somebody has to remember to add.
#[test]
fn the_file_list_takes_the_directory_of_every_component_that_owns_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let write = |rel: &str| {
        let path = join_relative(root, rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "the operator's own\n").unwrap();
    };
    write(".chap-devops/project.yaml");
    write("compose.yml");

    let owned: Vec<&str> = crate::components::Component::ALL
        .iter()
        .filter_map(|component| component.dir())
        .collect();
    assert!(!owned.is_empty(), "some component keeps files of its own");
    for owned_dir in &owned {
        write(&format!("{owned_dir}/config.marker"));
        // Nested, like ocs/plugins/: a directory is collected recursively,
        // so whatever an operator puts beside the config file comes too.
        write(&format!("{owned_dir}/plugins/nested.marker"));
    }

    let files = project_files(root);
    for owned_dir in &owned {
        let config = format!("{owned_dir}/config.marker");
        for rel in [config.clone(), format!("{owned_dir}/plugins/nested.marker")] {
            assert!(files.contains(&rel), "{rel} is in the archive: {files:?}");
        }
        // And in the group between `.chap-devops/**` and the compose files, which
        // is the order a restore's own tests read the list in.
        let at = |name: &str| files.iter().position(|f| f == name);
        assert!(at(".chap-devops/project.yaml") < at(&config));
        assert!(at(&config) < at("compose.yml"));
    }
}

#[test]
fn the_file_list_of_an_empty_directory_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(project_files(dir.path()).is_empty());
    assert!(is_compose_file("compose.yml"));
    assert!(is_compose_file("compose.marketplace.yml"));
    assert!(!is_compose_file("compose.yaml"));
    assert!(!is_compose_file("docker-compose.yml"));
}

/// What `pg_restore --clean --if-exists` prints on a restore that lost
/// nothing: drops of objects a fresh database never had, and the count it
/// finished with.
const IGNORABLE_STDERR: &str = "pg_restore: error: could not execute query: ERROR:  schema \"public\" does not exist\n\
         \x20   Command was: DROP SCHEMA public;\n\
         pg_restore: error: could not execute query: ERROR:  extension \"postgis\" already exists\n\
         pg_restore: warning: errors ignored on restore: 2\n";

/// A restore that never connected. Exit 1 as well, and the reason even
/// carries "does not exist" - which is why the ignored count has to be
/// there before exit 1 is read as success.
const CONNECTION_STDERR: &str = "pg_restore: error: connection to server at \"postgres\" (172.18.0.3), port 5432 failed: \
         FATAL:  role \"nosuchrole\" does not exist\n";

#[test]
fn a_pg_restore_verdict_reads_the_stderr_as_well_as_the_code() {
    // A clean run, whatever it printed.
    assert_eq!(pg_restore_outcome(0, ""), PgRestore::Ok);
    assert_eq!(pg_restore_outcome(0, IGNORABLE_STDERR), PgRestore::Ok);

    // Exit 1 with nothing but the complaints --clean --if-exists cannot
    // avoid, and the count that says it carried on: restored.
    assert_eq!(
        pg_restore_outcome(1, IGNORABLE_STDERR),
        PgRestore::Warnings,
        "the drops of objects a fresh database never had are not a failure"
    );

    // Exit 1 because it never connected: a failure, not a warning.
    assert_eq!(pg_restore_outcome(1, CONNECTION_STDERR), PgRestore::Failed);
    // Exit 1 with an error that is nobody's --if-exists: a failure too,
    // even though it finished and counted.
    let real = "pg_restore: error: could not execute query: ERROR:  out of shared memory\n\
                    pg_restore: warning: errors ignored on restore: 1\n";
    assert_eq!(pg_restore_outcome(1, real), PgRestore::Failed);
    // Exit 1 and silent: still a failure; success has a count.
    assert_eq!(pg_restore_outcome(1, ""), PgRestore::Failed);

    // Anything above 1 is pg_restore refusing outright.
    assert_eq!(pg_restore_outcome(2, IGNORABLE_STDERR), PgRestore::Failed);
    assert_eq!(pg_restore_outcome(127, ""), PgRestore::Failed);
}

/// What PostgreSQL 17's `pg_restore --clean --if-exists --no-owner` prints
/// when a table's rows have no table to go into: measured, with the table's
/// definition left out of the restore list. It exits 1 and counts the error
/// as ignored, and the error says `does not exist` like a harmless drop.
const LOST_ROWS_STDERR: &str = "pg_restore: error: could not execute query: ERROR:  relation \"public.jobs\" does not exist\n\
         Command was: COPY public.jobs (id, name) FROM stdin;\n\
         pg_restore: warning: errors ignored on restore: 1\n";

#[test]
fn rows_that_were_never_loaded_fail_the_restore() {
    assert_eq!(pg_restore_outcome(1, LOST_ROWS_STDERR), PgRestore::Failed);
    let errors = pg_restore_error_entries(LOST_ROWS_STDERR);
    assert!(errors[0].loads_data(), "{errors:?}");
    assert!(!errors[0].is_harmless());

    // pg_restore's own wording for a COPY that broke off part-way.
    let broken = "pg_restore: error: COPY failed for table \"jobs\": ERROR:  relation \"jobs\" does not exist\n\
                  pg_restore: warning: errors ignored on restore: 1\n";
    assert_eq!(pg_restore_outcome(1, broken), PgRestore::Failed);

    // The harmless drops alongside it do not rescue it.
    let mixed = format!("{IGNORABLE_STDERR}{LOST_ROWS_STDERR}");
    assert_eq!(pg_restore_outcome(1, &mixed), PgRestore::Failed);
}

#[test]
fn the_ignored_count_and_the_error_lines_are_read_off_the_stderr() {
    assert_eq!(pg_restore_ignored_count(IGNORABLE_STDERR), Some(2));
    assert_eq!(pg_restore_ignored_count(CONNECTION_STDERR), None);
    assert_eq!(
        pg_restore_ignored_count("errors ignored on restore: x"),
        None
    );

    let errors = pg_restore_error_entries(IGNORABLE_STDERR);
    assert_eq!(
        errors.len(),
        2,
        "the `Command was:` line is not an error of its own"
    );
    assert_eq!(errors[0].command.as_deref(), Some("DROP SCHEMA public;"));
    assert_eq!(errors[1].command, None);
    assert!(errors.iter().all(PgRestoreError::is_harmless));
    assert!(pg_restore_error_is_ignorable(
        "role \"chap\" must be owner of table x"
    ));
    assert!(!pg_restore_error_is_ignorable("out of shared memory"));
}

#[test]
fn warnings_and_tails_quote_what_postgres_said() {
    let warnings = pg_restore_warnings(
        "pg_restore: warning: errors ignored on restore: 2\n\
             \n\
             pg_restore: error: could not execute query: DROP SCHEMA public\n",
    );
    assert_eq!(
        warnings,
        vec![
            "warning: errors ignored on restore: 2",
            "error: could not execute query: DROP SCHEMA public",
        ]
    );
    assert!(pg_restore_warnings("   \n\n").is_empty());

    // The reason a restore failed is at the end, not at the start.
    assert_eq!(
        tail_lines("one\n\ntwo\nthree\n", 2),
        vec!["two".to_string(), "three".to_string()]
    );
    assert_eq!(tail_lines("only\n", 5), vec!["only".to_string()]);
    assert!(tail_lines("\n \n", 3).is_empty());
}

#[test]
fn a_restore_keeps_the_destinations_identity_unless_it_is_told_not_to() {
    // The normal case: restoring an archive into another deployment leaves
    // that deployment's containers and volumes where they are.
    assert_eq!(
        restored_compose_project("chapy-ab12cd", "chapx-9f01bc", false),
        "chapy-ab12cd"
    );
    // --adopt-identity is the takeover, for a deployment moving machines.
    assert_eq!(
        restored_compose_project("chapy-ab12cd", "chapx-9f01bc", true),
        "chapx-9f01bc"
    );
    // A deployment that never recorded a name keeps deriving one from its
    // directory, which is what the empty string means.
    assert_eq!(restored_compose_project("", "chapx-9f01bc", false), "");
    assert_eq!(
        restored_compose_project(" chapy-ab12cd ", "", false),
        "chapy-ab12cd"
    );

    let body = "schema_version: 1\ncompose_project: chapx-9f01bc\napi_port: 8000\n";
    assert_eq!(
        archived_compose_project(body).as_deref(),
        Some("chapx-9f01bc")
    );
    assert_eq!(archived_compose_project("compose_project: ''\n"), None);
    assert_eq!(archived_compose_project("schema_version: 1\n"), None);
    assert_eq!(archived_compose_project("not: [yaml"), None);
}

fn plan(start: bool, stop: Vec<&str>) -> RestorePlan {
    RestorePlan {
        archive: PathBuf::from("/backups/chap-devops-backup-e2e-20260923-071000.tar.gz"),
        project_dir: PathBuf::from("/srv/e2e"),
        manifest: manifest(),
        files: vec![".env".into(), ".chap-devops/models.yaml".into()],
        database: true,
        models: vec![PlannedModel {
            service_id: "chapkit-ewars-model".into(),
            data_dir: "/app/data".into(),
            volume: "ck_chapkit_ewars_model_data".into(),
        }],
        components: vec![PlannedComponent {
            name: "ocs".into(),
            service: "ocs".into(),
            data_dir: "/app/data".into(),
            volume: "ocs_data".into(),
            dump: false,
        }],
        stop: stop.into_iter().map(str::to_string).collect(),
        start,
        files_only: false,
        compose_project: "e2e-ab12cd".into(),
        archived_compose_project: Some("e2e-ab12cd".into()),
        adopt_identity: false,
        shared_with: Vec::new(),
    }
}

#[test]
fn the_plan_says_what_it_overwrites_and_what_it_stops() {
    let text = plan_text(&plan(true, vec!["chap", "worker"]));
    assert!(text.starts_with("restore /backups/chap-devops-backup-e2e-20260923-071000.tar.gz\n"));
    assert!(text.contains("taken  2026-09-23T07:10:00Z by chap-devops 0.1.0"));
    assert!(text.contains("into   /srv/e2e"));
    assert!(text.contains(
        "files       2 file(s) in the deployment directory: .env, .chap-devops/models.yaml"
    ));
    assert!(text.contains("database    chap_core on postgres, dropped and reloaded"));
    assert!(text.contains("chapkit-ewars-model /app/data emptied and refilled"));
    assert!(text.contains("components  ocs /app/data emptied and refilled (volume ocs_data)"));
    assert!(text.contains("stops first  chap, worker"));
    assert!(
        text.contains("then runs    the port check of `chap-devops up`, then docker compose up -d")
    );
    // The archive was taken from this same deployment, so there is nothing
    // to say about its identity.
    assert!(!text.contains("identity"), "{text}");
    assert!(!plan(true, vec![]).is_empty());
}

#[test]
fn a_deployment_without_chap_core_has_no_chap_core_tag() {
    let mut restore = plan(true, vec![]);
    assert!(plan_text(&restore).contains("from   deployment e2e (chap-core latest)"));
    restore.manifest.chap_image_tag = None;
    let text = plan_text(&restore);
    assert!(
        text.contains("from   deployment e2e (no chap-core)"),
        "{text}"
    );

    let body = render_manifest(&restore.manifest).unwrap();
    assert!(!body.contains("chap_image_tag"), "{body}");
    let back = parse_manifest(&body, Path::new("/tmp/a.tar.gz")).unwrap();
    assert_eq!(back.chap_image_tag, None);
}

#[test]
fn the_plan_says_whose_identity_the_deployment_keeps() {
    let mut restore = plan(true, vec![]);
    restore.archived_compose_project = Some("chapx-9f01bc".into());
    let text = plan_text(&restore);
    assert!(
        text.contains(
            "identity    e2e-ab12cd is kept; the archive's own (chapx-9f01bc) is not adopted"
        ),
        "{text}"
    );

    restore.adopt_identity = true;
    restore.compose_project = "chapx-9f01bc".into();
    let text = plan_text(&restore);
    assert!(
        text.contains("identity    compose project chapx-9f01bc, taken over from the archive"),
        "{text}"
    );

    // Nothing to say when the archive recorded no name at all.
    restore.archived_compose_project = None;
    assert!(!plan_text(&restore).contains("identity"));
}

#[test]
fn the_plan_of_a_stopped_stack_says_there_is_nothing_to_stop() {
    let text = plan_text(&plan(false, vec![]));
    assert!(text.contains("nothing is running, so nothing is stopped first"));
    assert!(text.contains("then leaves  the services as they are (--no-start)"));
}

#[test]
fn a_files_only_plan_claims_nothing_about_the_services() {
    let mut restore = plan(false, vec![]);
    restore.files_only = true;
    let text = plan_text(&restore);
    assert!(
        text.contains("files only (--files-only): no service is stopped or started"),
        "{text}"
    );
    assert!(!text.contains("nothing is running"), "{text}");
    assert!(!text.contains("--no-start"), "{text}");
}

#[test]
fn the_plan_names_a_dhis2_database_dump_as_a_reload() {
    let mut restore = plan(true, vec![]);
    restore.components = vec![PlannedComponent {
        name: "dhis2".into(),
        service: "dhis2-db".into(),
        data_dir: "/var/lib/postgresql/data".into(),
        volume: "dhis2_db".into(),
        dump: true,
    }];
    let text = plan_text(&restore);
    assert!(
        text.contains(
            "components  dhis2-db database dropped and reloaded from the pg_dump (pg_restore -j 4)"
        ),
        "{text}"
    );
}

#[test]
fn a_plan_that_restores_nothing_says_so() {
    let mut plan = plan(true, vec![]);
    plan.files.clear();
    plan.database = false;
    plan.models.clear();
    plan.components.clear();
    assert!(plan.is_empty());
    let text = plan_text(&plan);
    assert!(text.contains("files       nothing"));
    assert!(text.contains("database    nothing"));
    assert!(text.contains("models      nothing"));
    assert!(text.contains("components  nothing"));
}

#[test]
fn the_stage_is_inside_chap_devops_and_cleans_up_after_itself() {
    let dir = tempfile::tempdir().unwrap();
    let chap_devops = dir.path().join(".chap-devops");
    let path = {
        let stage = Stage::new(&chap_devops, "backup").unwrap();
        assert!(stage.dir.starts_with(chap_devops.join(TMP_DIR)));
        let nested = stage.path("files/.chap-devops/models.yaml").unwrap();
        std::fs::write(&nested, "x").unwrap();
        assert!(nested.is_file());
        stage.dir.clone()
    };
    assert!(!path.exists(), "the stage is removed when it is dropped");
    assert!(
        !chap_devops.join(TMP_DIR).exists(),
        "and so is the tmp directory it lived in"
    );
}

#[test]
fn a_new_stage_removes_the_stages_that_no_run_holds() {
    let dir = tempfile::tempdir().unwrap();
    let chap_devops = dir.path().join(".chap-devops");
    let tmp = chap_devops.join(TMP_DIR);
    // Left by a run that was killed: no lock file, or one nobody holds.
    for stale in ["backup-1", "backup-2"] {
        std::fs::create_dir_all(tmp.join(stale)).unwrap();
        std::fs::write(tmp.join(stale).join("db.dump"), "x").unwrap();
    }
    std::fs::write(tmp.join("backup-2.lock"), "").unwrap();
    // A restore stage is of another kind and stays.
    std::fs::create_dir_all(tmp.join("restore-3")).unwrap();

    let held = Stage::new(&chap_devops, "backup").unwrap();
    assert!(!tmp.join("backup-1").exists());
    assert!(!tmp.join("backup-2").exists());
    assert!(!tmp.join("backup-2.lock").exists());
    assert!(tmp.join("restore-3").exists());

    // A stage that a live run holds is not removed by the next one.
    let other = tmp.join("backup-999999");
    std::fs::create_dir_all(&other).unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(tmp.join("backup-999999.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    drop(held);
    let _again = Stage::new(&chap_devops, "backup").unwrap();
    assert!(other.exists(), "a held stage stays");
}

#[test]
fn archives_are_written_listed_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let stage = dir.path().join("stage");
    std::fs::create_dir_all(stage.join("files/.chap-devops")).unwrap();
    std::fs::write(stage.join("manifest.yaml"), "schema_version: 1\n").unwrap();
    std::fs::write(stage.join("files/.env"), "POSTGRES_USER=chap\n").unwrap();
    std::fs::write(stage.join("files/.chap-devops/models.yaml"), "{}\n").unwrap();

    let archive = dir.path().join("out.tar.gz");
    tar_create(
        &archive,
        &stage,
        &[MANIFEST_MEMBER.to_string(), FILES_MEMBER.to_string()],
    )
    .unwrap();
    assert!(archive.is_file());

    let members = tar_list(&archive).unwrap();
    assert!(members.contains(&MANIFEST_MEMBER.to_string()));
    assert!(members.iter().any(|m| m == "files/.env"));

    assert_eq!(
        tar_read_member(&archive, MANIFEST_MEMBER).unwrap(),
        "schema_version: 1\n"
    );

    let one = dir.path().join("one.txt");
    tar_extract_member_to(&archive, "files/.env", &one).unwrap();
    assert_eq!(
        std::fs::read_to_string(&one).unwrap(),
        "POSTGRES_USER=chap\n"
    );

    let out = dir.path().join("unpacked");
    tar_extract_into(&archive, &out, &[FILES_MEMBER.to_string()]).unwrap();
    assert!(out.join("files/.chap-devops/models.yaml").is_file());

    let err = tar_read_member(&archive, "db/chap_core.dump").unwrap_err();
    assert!(err.to_string().contains("reading db/chap_core.dump"));
}

#[test]
fn only_the_enabled_components_with_state_are_captured() {
    use crate::components::{Components, OcsComponent, S3Component};

    // chap-core alone: the database and the model volumes are its state,
    // and both are captured in their own right.
    assert!(component_volumes(&Components::default()).is_empty());

    let mut components = Components {
        ocs: OcsComponent {
            enabled: true,
            ..OcsComponent::default()
        },
        ..Components::default()
    };
    assert_eq!(
        component_volumes(&components)
            .iter()
            .map(|p| p.volume)
            .collect::<Vec<_>>(),
        vec!["ocs_data"]
    );

    components.s3 = S3Component {
        enabled: true,
        port: None,
    };
    assert_eq!(
        component_volumes(&components)
            .iter()
            .map(|p| p.name)
            .collect::<Vec<_>>(),
        vec!["ocs", "s3"]
    );

    let parts = component_volumes(&components);
    assert_eq!(parts[0].service, "ocs");
    assert_eq!(parts[0].data_dir, "/app/data");
    assert_eq!(parts[1].data_dir, "/data");

    // A component with two archived volumes contributes two entries under
    // the one name, each with its own member and its own service to hold
    // still. The download cache it also keeps is not one of them.
    components.dhis2.enabled = true;
    let parts = component_volumes(&components);
    assert_eq!(
        parts.iter().map(|p| p.name).collect::<Vec<_>>(),
        vec!["ocs", "s3", "dhis2", "dhis2"]
    );
    assert_eq!(
        parts[2..]
            .iter()
            .map(|p| (p.member, p.service, p.volume, p.data_dir))
            .collect::<Vec<_>>(),
        vec![
            ("dhis2-home", "dhis2", "dhis2_home", "/opt/dhis2"),
            (
                "dhis2-db",
                "dhis2-db",
                "dhis2_db",
                "/var/lib/postgresql/data"
            ),
        ]
    );
    assert!(
        !parts.iter().any(|p| p.volume == "dhis2_dump"),
        "the seed cache is not archived"
    );
    assert_eq!(component_member("dhis2-home"), "components/dhis2-home.tar");
}

/// Every entry of this table names one of its own component's volumes.
///
/// The strict direction, and the only one that holds:
/// [`crate::components::Component::volumes`] is what a `disable` names, what
/// `--purge` removes and what the doctor judges leftovers by, so it has to
/// list every volume the component's compose file declares. This table is
/// what a backup reads, and a volume worth declaring is not always a volume
/// worth archiving - so it is a subset, and an entry naming a volume no
/// component keeps would be an archive member of something nothing declares.
///
/// The one volume that is in the component and not in the table is pinned
/// below with its reason, so it is not "fixed" by adding it: `dhis2_dump` is
/// a download cache the one-shot refills by itself, and putting it in an
/// archive would add the whole dump to every backup for nothing.
#[test]
fn the_table_and_the_component_agree_on_every_volume() {
    use crate::components::Component;

    for part in COMPONENT_VOLUMES {
        let component = Component::from_name(part.name).expect("an entry names a component");
        assert!(
            component.volumes().contains(&part.volume),
            "{} is not one of {}'s volumes",
            part.volume,
            part.name
        );
    }

    // In component order, and within a component in the order that
    // component lists its volumes: the manifest and the archive are read in
    // this order, and a backup holds still one service at a time.
    let mut ordered: Vec<&str> = Vec::new();
    for component in Component::ALL {
        for volume in component.volumes() {
            if COMPONENT_VOLUMES.iter().any(|part| part.volume == *volume) {
                ordered.push(volume);
            }
        }
    }
    let table: Vec<&str> = COMPONENT_VOLUMES.iter().map(|part| part.volume).collect();
    assert_eq!(table, ordered, "one entry per archived volume, in order");

    // The deliberate omission, and the reason it stays one. A dump that is
    // fetched again when it is missing is not state; every other volume a
    // component declares has to be here.
    let cache = crate::compose::render::DHIS2_DUMP_VOLUME;
    assert!(
        Component::Dhis2.volumes().contains(&cache),
        "the component still declares it, so the doctor does not call it a leftover"
    );
    assert!(
        !COMPONENT_VOLUMES.iter().any(|part| part.volume == cache),
        "{cache} is a download cache the dhis2-dump one-shot refetches; archiving it \
             would add the whole dump to every backup for nothing"
    );
    let missing: Vec<&str> = Component::ALL
        .iter()
        .flat_map(|c| c.volumes().iter().copied())
        .filter(|volume| !COMPONENT_VOLUMES.iter().any(|part| part.volume == *volume))
        .collect();
    assert_eq!(
        missing,
        vec![cache],
        "that cache is the only volume a component keeps and a backup leaves out"
    );
}

/// One member per volume, and a component with a single volume uses its own
/// name as the member name.
#[test]
fn every_component_volume_has_an_archive_member_of_its_own() {
    use crate::components::Component;

    let mut members: Vec<&str> = COMPONENT_VOLUMES.iter().map(|part| part.member).collect();
    let count = members.len();
    members.sort_unstable();
    members.dedup();
    assert_eq!(
        members.len(),
        count,
        "two volumes under one member name would be one member in the tar"
    );

    for component in Component::ALL {
        let mine: Vec<&ComponentVolume> = COMPONENT_VOLUMES
            .iter()
            .filter(|part| part.name == component.name())
            .collect();
        if let [only] = mine.as_slice() {
            assert_eq!(
                only.member, only.name,
                "a component with one volume uses its own name as the member name"
            );
        }
        for part in mine {
            assert!(
                part.member == part.name || part.member.starts_with(&format!("{}-", part.name)),
                "{} does not read as one of {}'s members",
                part.member,
                part.name
            );
        }
    }

    assert_eq!(component_member("ocs"), "components/ocs.tar");
    assert_eq!(component_member("s3"), "components/s3.tar");
}

#[test]
fn a_component_volume_is_read_and_written_through_one_busybox_mount() {
    assert_eq!(
        volume_read_args("chapx-ab12cd_ocs_data", "chap-devops-volume-1-0"),
        vec![
            "run",
            "--rm",
            "--name",
            "chap-devops-volume-1-0",
            "-v",
            "chapx-ab12cd_ocs_data:/v",
            BUSYBOX_IMAGE,
            "tar",
            "-C",
            "/v",
            "-cf",
            "-",
            "."
        ]
    );
    let write = volume_write_args("chapx-ab12cd_s3_data", "chap-devops-volume-1-1");
    // Only the writing side keeps stdin open; the reading side would
    // otherwise wait on a terminal that is not there.
    assert_eq!(
        write[..5],
        ["run", "--rm", "--name", "chap-devops-volume-1-1", "-i"]
    );
    assert_eq!(write[5..7], ["-v", "chapx-ab12cd_s3_data:/v"]);
    assert_eq!(write[7], BUSYBOX_IMAGE);
    let script = write.last().expect("the shell script");
    assert!(script.contains("rm -rf /v/* /v/.[!.]* /v/..?*"));
    assert!(script.ends_with("tar -C /v -xf -"));
    // rm's complaints about patterns that match nothing are dropped; tar's
    // are not.
    assert!(!script.contains("tar -C /v -xf - 2>/dev/null"));
}

#[test]
fn copy_file_creates_the_directories_it_needs() {
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("from");
    let to = dir.path().join("to");
    std::fs::create_dir_all(from.join(".chap-devops")).unwrap();
    std::fs::write(from.join(".chap-devops/models.yaml"), "{}\n").unwrap();
    copy_file(&from, &to, ".chap-devops/models.yaml").unwrap();
    assert_eq!(
        std::fs::read_to_string(to.join(".chap-devops/models.yaml")).unwrap(),
        "{}\n"
    );
}

/// A manifest is read out of the archive, so a path in it is only as
/// trustworthy as the file: one that climbs out of the deployment directory
/// is refused before anything is restored, and `copy_file` refuses it too.
#[test]
fn a_manifest_path_outside_the_project_is_refused() {
    assert!(is_contained_relative(".chap-devops/models.yaml"));
    assert!(is_contained_relative("compose.yml"));
    for rel in [
        "",
        "../.bashrc",
        ".chap-devops/../../x",
        "/etc/passwd",
        "a//b",
        "./compose.yml",
        "a\\b",
        "C:x",
    ] {
        assert!(!is_contained_relative(rel), "{rel:?}");
    }

    let manifest: Manifest = serde_yaml_ng::from_str(
        "schema_version: 1\ncreated_by: chap-devops 0.12.2\ncreated_at: 2026-10-01T00:00:00Z\n\
         project: p\nchap_image_tag: v2.3.1\nfiles: [compose.yml, ../../.bashrc]\n",
    )
    .unwrap();
    let err = check_manifest_files(&manifest, Path::new("backup.tar.gz")).unwrap_err();
    assert!(err.to_string().contains("`../../.bashrc`"), "{err}");

    // The member paths, the service id and the data directory of a model are
    // checked too: they become stage paths and a `sh -c` script.
    let head = "schema_version: 1\ncreated_by: chap-devops 0.12.2\ncreated_at: \
                2026-10-01T00:00:00Z\nproject: p\nfiles: [compose.yml]\n";
    let model = |service_id: &str, data_dir: &str, path: &str| {
        format!(
            "models:\n- id: m\n  service_id: {service_id}\n  version: '1'\n  image_tag: t\n  \
             data_dir: {data_dir:?}\n  user: '1:1'\n  volume: v\n  path: {path}\n  \
             size_bytes: 0\n"
        )
    };
    let cases = [
        (model("m", "/data", "models/m.tar"), None),
        (model("m", "/data", "../../x"), Some("the member `../../x`")),
        (
            model("../m", "/data", "models/m.tar"),
            Some("the service id"),
        ),
        (
            model("m", "/data; rm -rf /", "models/m.tar"),
            Some("the data directory"),
        ),
        (
            "components:\n- name: ocs\n  service: ocs\n  volume: v\n  data_dir: /v\n  \
             path: ../escape.tar\n  size_bytes: 0\n"
                .to_string(),
            Some("the member `../escape.tar`"),
        ),
    ];
    for (tail, want) in cases {
        let manifest: Manifest = serde_yaml_ng::from_str(&format!("{head}{tail}")).unwrap();
        let checked = check_manifest_files(&manifest, Path::new("backup.tar.gz"));
        match want {
            None => checked.unwrap(),
            Some(want) => {
                let err = checked.unwrap_err().to_string();
                assert!(err.contains(want), "{err}");
            }
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let stage = Stage::new(&tmp.path().join(".chap-devops"), "t").unwrap();
    assert!(stage.path("../out").is_err());
    assert!(stage.path("components/ocs.tar").is_ok());

    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("to");
    let err = copy_file(dir.path(), &to, "../escaped").unwrap_err();
    assert!(err.to_string().contains("refusing to copy"), "{err}");
    assert!(!dir.path().join("escaped").exists());
}

/// The DHIS2 dump leaves out what DHIS2 makes again, and keeps the metadata
/// whose names start like the analytics tables.
#[test]
fn the_dhis2_dump_leaves_out_what_dhis2_makes_again() {
    let command = dhis2_dump_command();
    for table in ["analytics_*", "aggregated_*", "completeness_*", "_*"] {
        assert!(command.contains(&format!(" -T '{table}'")), "{command}");
    }
    assert!(
        command.contains("--exclude-table-data='audit'"),
        "{command}"
    );
    // `analyticstablehook` is metadata, and `analytics*` would take it.
    assert!(!command.contains("'analytics*'"), "{command}");
    assert!(command.contains("-Fc -Z 1"), "{command}");

    let restore = dhis2_restore_command();
    assert!(restore.contains("--if-exists --force"), "{restore}");
    assert!(
        restore.contains(&format!("-j {DHIS2_RESTORE_JOBS}")),
        "{restore}"
    );
    assert!(restore.contains("--no-owner --no-privileges"), "{restore}");
    assert!(restore.ends_with(DHIS2_RESTORE_PATH), "{restore}");
    assert!(
        dhis2_mark_command().contains(crate::compose::render::DHIS2_SEED_MARK),
        "the restored database is a complete one"
    );
    assert!(is_dhis2_db_dump(DHIS2_DB_DUMP_MEMBER));
    assert!(!is_dhis2_db_dump("components/dhis2-home.tar"));
}

/// A volume the restore makes carries the labels compose gives its own, so
/// compose does not warn that it "was not created by Docker Compose".
#[test]
fn a_volume_the_restore_makes_has_the_compose_labels() {
    assert_eq!(
        volume_create_args("chapv-60913c", "dhis2_home", Some("2.39.1")),
        [
            "volume",
            "create",
            "--label",
            "com.docker.compose.project=chapv-60913c",
            "--label",
            "com.docker.compose.volume=dhis2_home",
            "--label",
            "com.docker.compose.version=2.39.1",
            "chapv-60913c_dhis2_home",
        ]
    );
    assert!(
        !volume_create_args("p", "ocs_data", None)
            .iter()
            .any(|arg| arg.contains("version"))
    );
}

/// The archive and the stage hold `.env` and the database dumps, so other
/// users on the machine cannot read them.
#[cfg(unix)]
#[test]
fn the_archive_and_the_stage_are_for_the_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = tempfile::tempdir().unwrap();
    let chap_devops = dir.path().join(".chap-devops");
    let stage = Stage::new(&chap_devops, "backup").unwrap();
    assert_eq!(mode(&stage.dir), 0o700);

    std::fs::write(stage.dir.join("manifest.yaml"), "schema_version: 1\n").unwrap();
    let archive = dir.path().join("out.tar.gz");
    // An older file of that name is replaced, not reused with its mode.
    std::fs::write(&archive, "old").unwrap();
    std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o644)).unwrap();
    tar_create(&archive, &stage.dir, &[MANIFEST_MEMBER.to_string()]).unwrap();
    assert_eq!(mode(&archive), 0o600);
    assert_eq!(
        tar_read_member(&archive, MANIFEST_MEMBER).unwrap(),
        "schema_version: 1\n"
    );
}
