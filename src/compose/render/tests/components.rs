//! The component compose files: OCS, the object store and DHIS2, with their config scaffolds.

use super::*;

fn ocs_spec() -> OcsSpec {
    OcsSpec {
        host_port: Some(crate::components::OCS_DEFAULT_PORT),
        image_tag: crate::components::OCS_DEFAULT_TAG.to_string(),
        base_url: None,
        s3: false,
        plugins: false,
        group: None,
    }
}

fn s3_spec() -> S3Spec {
    S3Spec {
        host_port: None,
        image_tag: crate::components::S3_DEFAULT_TAG.to_string(),
        group: None,
    }
}

#[test]
fn ocs_publishes_its_port_and_mounts_the_scaffolded_config() {
    let text = render_ocs(&ocs_spec());
    assert_no_tokens(&text);
    assert_eq!(text.lines().next(), Some(GENERATED_HEADER));
    assert_eq!(
        text.lines().nth(1),
        Some("# ocs: Open Climate Service (https://github.com/dhis2/open-climate-service)")
    );

    let doc = parse(&text);
    let svc = service(&doc, "ocs");
    assert_eq!(
        svc["image"].as_str(),
        Some("ghcr.io/dhis2/open-climate-service:${OCS_IMAGE_TAG:-main}")
    );
    // Multi-arch upstream, so no platform pin and no `depends_on` on chap.
    assert!(svc.get("platform").is_none());
    assert!(svc.get("depends_on").is_none());
    assert!(svc.get("networks").is_none());
    // The image ships its own HEALTHCHECK; a second one here would only
    // be a copy to keep in step.
    assert!(svc.get("healthcheck").is_none());
    assert_eq!(svc["restart"].as_str(), Some("unless-stopped"));
    assert_eq!(svc["init"].as_bool(), Some(true));
    assert_eq!(
        svc["expose"],
        Value::Sequence(vec![Value::String("9000".into())])
    );
    assert_eq!(
        svc["ports"],
        Value::Sequence(vec![Value::String("8790:9000".into())])
    );
    // Quoted: an unquoted number is not a string, which is what compose
    // wants of an environment value.
    assert_eq!(svc["environment"]["PORT"].as_str(), Some("9000"));
    assert_eq!(
        svc["environment"]["CLIMATE_SERVICE_CONFIG"].as_str(),
        Some("/app/climate-service.yaml")
    );
    assert!(svc["environment"].get("ROOT_PATH").is_none());
    assert_eq!(
        svc["volumes"][0].as_str(),
        Some("./ocs/climate-service.yaml:/app/climate-service.yaml:ro")
    );
    assert_eq!(svc["volumes"][1]["source"].as_str(), Some(OCS_VOLUME));
    assert_eq!(svc["volumes"][1]["target"].as_str(), Some("/app/data"));
    assert_eq!(
        svc["volumes"].as_sequence().unwrap().len(),
        2,
        "no plugin mount without an ocs/plugins/ to mount"
    );
    assert!(doc["volumes"].get(OCS_VOLUME).is_some());
    // One service: OCS needs no init container of its own.
    assert_eq!(doc["services"].as_mapping().unwrap().len(), 1);
}

/// The five dataset credentials are passed whatever the deployment has:
/// OCS reads each as `os.getenv(...) or <the file>`, so empty is absent.
#[test]
fn ocs_always_gets_the_dataset_credential_variables_as_empty_defaults() {
    let text = render_ocs(&ocs_spec());
    assert_no_tokens(&text);
    let env = &parse(&text)["services"]["ocs"]["environment"];
    for var in crate::components::OCS_DATA_SOURCE_ENV_VARS {
        assert_eq!(
            env[*var].as_str(),
            Some(format!("${{{var}:-}}").as_str()),
            "{var}"
        );
    }
    assert!(text.contains("# Dataset credentials, empty unless .env sets them"));
}

/// The base URL is a `${VAR:-default}` like the image tag: the recorded
/// value is the default, and `.env` can still move it without a sync.
#[test]
fn the_base_url_renders_as_a_default_that_env_can_override() {
    let without = render_ocs(&ocs_spec());
    assert_eq!(
        parse(&without)["services"]["ocs"]["environment"]["CLIMATE_SERVICE_BASE_URL"].as_str(),
        Some("${CLIMATE_SERVICE_BASE_URL:-}"),
        "unset, which OCS reads as `compose the links from the request`"
    );

    let with = render_ocs(&OcsSpec {
        base_url: Some("https://ocs.example.org".to_string()),
        ..ocs_spec()
    });
    assert_no_tokens(&with);
    assert_eq!(
        parse(&with)["services"]["ocs"]["environment"]["CLIMATE_SERVICE_BASE_URL"].as_str(),
        Some("${CLIMATE_SERVICE_BASE_URL:-https://ocs.example.org}")
    );
}

/// The reverse-proxy shape: `expose` and no `ports`, so nothing on the
/// host is bound and the proxy is the only way in.
#[test]
fn ocs_publishes_nothing_when_it_has_no_host_port() {
    let text = render_ocs(&OcsSpec {
        host_port: None,
        base_url: Some("https://ocs.example.org".to_string()),
        ..ocs_spec()
    });
    assert_no_tokens(&text);
    let doc = parse(&text);
    let svc = service(&doc, "ocs");
    assert_eq!(
        svc["expose"],
        Value::Sequence(vec![Value::String("9000".into())])
    );
    assert!(svc.get("ports").is_none(), "{text}");
    // The container port never moves: it is what chap-core and the proxy
    // both reach.
    assert_eq!(svc["environment"]["PORT"].as_str(), Some("9000"));
}

/// The mount only appears when the directory is there: compose refuses to
/// start a service whose bind source does not exist.
#[test]
fn the_plugin_directory_is_mounted_only_when_the_project_has_one() {
    let text = render_ocs(&OcsSpec {
        plugins: true,
        ..ocs_spec()
    });
    assert_no_tokens(&text);
    let doc = parse(&text);
    let volumes = doc["services"]["ocs"]["volumes"].as_sequence().unwrap();
    assert_eq!(
        volumes[1].as_str(),
        Some("./ocs/plugins:/app/plugins:ro"),
        "{text}"
    );
    assert_eq!(volumes.len(), 3, "the config, the plugins, the data volume");
    assert_eq!(volumes[2]["source"].as_str(), Some(OCS_VOLUME));
}

#[test]
fn ocs_gets_the_s3_variables_only_when_the_store_is_enabled() {
    let without = render_ocs(&ocs_spec());
    assert!(!without.contains("S3_ENDPOINT"));
    // Dropping the block must not leave a blank line behind.
    assert!(!without.contains("\n\n    volumes:"));

    let with = render_ocs(&OcsSpec {
        s3: true,
        ..ocs_spec()
    });
    assert_no_tokens(&with);
    assert!(
        with.contains("# Forward-looking: OCS does not read these yet."),
        "{with}"
    );
    let env = &parse(&with)["services"]["ocs"]["environment"];
    assert_eq!(env["S3_ENDPOINT"].as_str(), Some("http://s3:9000"));
    assert_eq!(env["S3_ACCESS_KEY"].as_str(), Some("${S3_ACCESS_KEY:-}"));
    assert_eq!(env["S3_SECRET_KEY"].as_str(), Some("${S3_SECRET_KEY:-}"));
    assert_eq!(env["S3_BUCKET"].as_str(), Some("ocs"));
}

#[test]
fn a_custom_ocs_port_moves_only_the_host_side() {
    let text = render_ocs(&OcsSpec {
        host_port: Some(9010),
        ..ocs_spec()
    });
    assert_eq!(
        parse(&text)["services"]["ocs"]["ports"],
        Value::Sequence(vec![Value::String("9010:9000".into())])
    );
    // The container port is what chap-core reaches, and it never moves.
    assert_eq!(
        parse(&text)["services"]["ocs"]["environment"]["PORT"].as_str(),
        Some("9000")
    );
}

#[test]
fn the_store_is_internal_until_a_port_is_asked_for() {
    let internal = render_s3(&s3_spec());
    assert_no_tokens(&internal);
    let doc = parse(&internal);
    let svc = service(&doc, "s3");
    assert_eq!(
        svc["expose"],
        Value::Sequence(vec![Value::String("9000".into())])
    );
    assert!(svc.get("ports").is_none(), "nothing is published");

    let published = render_s3(&S3Spec {
        host_port: Some(9002),
        ..s3_spec()
    });
    assert_no_tokens(&published);
    let svc = &parse(&published)["services"]["s3"];
    assert_eq!(
        svc["ports"],
        Value::Sequence(vec![Value::String("9002:9000".into())])
    );
    assert_eq!(
        svc["expose"],
        Value::Sequence(vec![Value::String("9000".into())])
    );
}

#[test]
fn the_store_carries_its_credentials_its_health_check_and_a_bucket_init() {
    let text = render_s3(&s3_spec());
    assert_eq!(text.lines().next(), Some(GENERATED_HEADER));
    assert_eq!(
        text.lines().nth(1),
        Some("# s3: RustFS, an S3-compatible object store (https://github.com/rustfs/rustfs)")
    );
    let doc = parse(&text);
    let svc = service(&doc, "s3");
    assert_eq!(
        svc["image"].as_str(),
        Some("rustfs/rustfs:${S3_IMAGE_TAG:-latest}")
    );
    assert!(svc.get("platform").is_none(), "multi-arch upstream");
    assert_eq!(svc["restart"].as_str(), Some("unless-stopped"));
    assert_eq!(svc["init"].as_bool(), Some(true));
    // The image's own variable names, substituted from .env.
    let env = &svc["environment"];
    assert_eq!(
        env["RUSTFS_ACCESS_KEY"].as_str(),
        Some("${S3_ACCESS_KEY:-}")
    );
    assert_eq!(
        env["RUSTFS_SECRET_KEY"].as_str(),
        Some("${S3_SECRET_KEY:-}")
    );
    assert_eq!(env["RUSTFS_VOLUMES"].as_str(), Some("/data"));
    assert_eq!(
        svc["healthcheck"]["test"][1].as_str(),
        Some("curl"),
        "the image ships curl, which is what signs the bucket request too"
    );
    assert_eq!(svc["volumes"][0]["source"].as_str(), Some(S3_VOLUME));
    assert!(doc["volumes"].get(S3_VOLUME).is_some());

    // The one-shot creates the bucket with the same image, so nothing else
    // has to be pulled; `$$` survives for the container's own shell.
    let init = service(&doc, "s3-init");
    assert_eq!(init["restart"].as_str(), Some("no"));
    assert_eq!(
        init["depends_on"]["s3"]["condition"].as_str(),
        Some("service_healthy")
    );
    let command = init["command"][0].as_str().unwrap();
    assert!(
        command.contains("--aws-sigv4 \"aws:amz:us-east-1:s3\""),
        "{command}"
    );
    assert!(
        command.contains("-X PUT \"http://s3:9000/ocs\""),
        "{command}"
    );
    assert!(
        text.contains("$${S3_ACCESS_KEY}"),
        "a literal $ for compose"
    );
    assert_eq!(doc["services"].as_mapping().unwrap().len(), 2);
}

#[test]
fn the_ocs_config_scaffold_says_when_it_is_the_example() {
    let example = render_ocs_config(&OcsConfigSpec::default());
    assert_no_tokens(&example);
    assert!(example.contains(OCS_EXAMPLE_MARKER), "{example}");
    let doc = parse(&example);
    assert_eq!(doc["id"].as_str(), Some("laos-climate-service"));
    assert_eq!(doc["name"].as_str(), Some("Laos Climate Service"));
    assert_eq!(doc["extent"]["name"].as_str(), Some("Laos"));
    assert_eq!(doc["extent"]["country_code"].as_str(), Some("LAO"));
    assert_eq!(doc["extent"]["bbox"][0].as_f64(), Some(100.0));
    assert_eq!(doc["extent"]["bbox"][3].as_f64(), Some(22.5));
    assert_eq!(doc["data_dir"].as_str(), Some("/app/data"));
    // `components enable ocs --read-only` and the plugin mount edit the
    // file later, so the header names the two keys chap-devops changes.
    assert_eq!(
        example.lines().next(),
        Some(
            "# Written once by chap-devops when the ocs component was enabled; chap-devops changes only \
             the read_only and plugins_dir keys."
        )
    );
}

#[test]
fn the_ocs_config_scaffold_fills_the_values_it_is_given() {
    let spec = crate::compose::spec::OcsConfigRequest {
        name: Some("Malawi".into()),
        country_code: Some("mwi".into()),
        bbox: Some("32.6,-17.2,35.9,-9.3".into()),
    }
    .into_spec();
    let text = render_ocs_config(&spec);
    assert_no_tokens(&text);
    assert!(
        !text.contains(OCS_EXAMPLE_MARKER),
        "these are the operator's values"
    );
    let doc = parse(&text);
    assert_eq!(doc["id"].as_str(), Some("malawi-climate-service"));
    assert_eq!(doc["name"].as_str(), Some("Malawi Climate Service"));
    assert_eq!(doc["extent"]["name"].as_str(), Some("Malawi"));
    assert_eq!(doc["extent"]["country_code"].as_str(), Some("MWI"));
    assert_eq!(doc["extent"]["bbox"][1].as_f64(), Some(-17.2));
    assert_eq!(doc["data_dir"].as_str(), Some("/app/data"));
}

/// A seeded deployment publishing the container port on the host: what
/// `init --with dhis2` would ask for.
fn dhis2_spec() -> Dhis2Spec {
    Dhis2Spec {
        host_port: Some(DHIS2_CONTAINER_PORT),
        image_tag: crate::components::DHIS2_DEFAULT_TAG.to_string(),
        image: crate::components::DHIS2_IMAGE.to_string(),
        seed: Some(Dhis2SeedSource::Url(DHIS2_DEFAULT_SEED_URL.to_string())),
        seed_password: None,
        host_gateway: false,
        group: None,
    }
}

/// The same without a seed: an empty DHIS2 that migrates itself.
fn unseeded_dhis2_spec() -> Dhis2Spec {
    Dhis2Spec {
        seed: None,
        ..dhis2_spec()
    }
}

#[test]
fn a_seeded_dhis2_renders_four_services_and_three_volumes() {
    let text = render_dhis2(&dhis2_spec());
    assert_no_tokens(&text);
    assert_eq!(text.lines().next(), Some(GENERATED_HEADER));
    assert!(
        text.lines().nth(1).unwrap().starts_with("# dhis2: DHIS2, "),
        "{text}"
    );
    assert!(text.ends_with('\n'));

    let doc = parse(&text);
    let services = doc["services"].as_mapping().unwrap();
    assert_eq!(services.len(), 4, "{text}");
    for name in ["dhis2", "dhis2-db", "dhis2-dump", "dhis2-prep"] {
        assert!(services.contains_key(name), "no {name}: {text}");
    }
    // Everything but the web service is `dhis2-<something>`, so a container
    // name says which component owns it.
    for name in services.keys() {
        let name = name.as_str().unwrap();
        assert!(
            name == "dhis2" || name.starts_with("dhis2-"),
            "{name} breaks the naming convention"
        );
    }

    let volumes = doc["volumes"].as_mapping().unwrap();
    assert_eq!(volumes.len(), 3, "{text}");
    for volume in [DHIS2_HOME_VOLUME, DHIS2_DB_VOLUME, DHIS2_DUMP_VOLUME] {
        assert!(volumes.contains_key(volume), "{volume} is not declared");
    }

    // Rendering is a pure function of the spec, which is what lets
    // `chap-devops sync` compare the file byte for byte with what is on disk.
    assert_eq!(render_dhis2(&dhis2_spec()), text);
}

#[test]
fn dhis2_without_a_seed_drops_the_one_shot_the_volume_and_the_wait() {
    let text = render_dhis2(&unseeded_dhis2_spec());
    assert_no_tokens(&text);
    assert!(!text.contains("dhis2-dump"), "{text}");
    assert!(!text.contains("docker-entrypoint-initdb.d"), "{text}");
    assert!(!text.contains(DHIS2_DUMP_VOLUME), "{text}");
    assert!(!text.contains(DHIS2_DEFAULT_SEED_URL), "{text}");

    let doc = parse(&text);
    let services = doc["services"].as_mapping().unwrap();
    assert_eq!(services.len(), 3, "{text}");
    let volumes = doc["volumes"].as_mapping().unwrap();
    assert_eq!(volumes.len(), 2, "{text}");
    // The database waits for nothing once there is no dump to wait for.
    assert!(service(&doc, "dhis2-db").get("depends_on").is_none());
    // And dropping four blocks leaves no blank line and no dangling comment
    // behind.
    assert!(!text.contains("\n\n\n"), "{text}");
    assert!(
        text.ends_with(&format!("  {DHIS2_HOME_VOLUME}: {{}}\n")),
        "{text}"
    );
    assert_eq!(render_dhis2(&unseeded_dhis2_spec()), text);
}

#[test]
fn dhis2_publishes_a_port_only_when_one_was_asked_for() {
    // The reverse-proxy shape: exposed on the compose network, nothing bound
    // on the host.
    let internal = render_dhis2(&Dhis2Spec {
        host_port: None,
        ..dhis2_spec()
    });
    assert_no_tokens(&internal);
    let svc = &parse(&internal)["services"]["dhis2"];
    assert_eq!(
        svc["expose"],
        Value::Sequence(vec![Value::String("8080".into())])
    );
    assert!(svc.get("ports").is_none(), "{internal}");
    // Dropping the ports block must not leave a blank line behind.
    assert!(internal.contains("\n    environment:\n"));
    assert!(!internal.contains("\n\n    environment:"));

    // With a port, both keys: `expose` still documents the container port.
    let published = render_dhis2(&Dhis2Spec {
        host_port: Some(8081),
        ..dhis2_spec()
    });
    assert_no_tokens(&published);
    let svc = &parse(&published)["services"]["dhis2"];
    assert_eq!(
        svc["expose"],
        Value::Sequence(vec![Value::String("8080".into())])
    );
    assert_eq!(
        svc["ports"],
        Value::Sequence(vec![Value::String("8081:8080".into())])
    );
}

/// The container port never moves: 8080 is the one connector the image's
/// `server.xml` declares, and the context path is the root.
#[test]
fn dhis2_pins_only_the_database_image_platform_and_pulls_the_tag_env_can_move() {
    let text = render_dhis2(&dhis2_spec());
    let doc = parse(&text);
    // The PostGIS image is amd64-only, in every service that runs it; the
    // others are multi-arch. An arm64 host fails its pull without the pin.
    for name in ["dhis2-db", "dhis2-prep"] {
        assert_eq!(service(&doc, name)["image"].as_str(), Some(DHIS2_DB_IMAGE));
        assert_eq!(
            service(&doc, name)["platform"].as_str(),
            Some("linux/amd64"),
            "{name} does not pin the platform"
        );
    }
    for name in ["dhis2", "dhis2-dump"] {
        assert!(
            service(&doc, name).get("platform").is_none(),
            "{name} pins a platform"
        );
    }
    let svc = service(&doc, "dhis2");
    assert_eq!(
        svc["image"].as_str(),
        Some("dhis2/core:${DHIS2_IMAGE_TAG:-2.42}")
    );
    assert_eq!(svc["restart"].as_str(), Some("unless-stopped"));
    assert_eq!(svc["init"].as_bool(), Some(true));
    // The recorded tag is the variable's default, so the file stands on its
    // own and `.env` still moves the pin without a sync.
    let pinned = render_dhis2(&Dhis2Spec {
        image_tag: "2.43.1.0".to_string(),
        ..dhis2_spec()
    });
    assert!(
        pinned.contains("    image: dhis2/core:${DHIS2_IMAGE_TAG:-2.43.1.0}\n"),
        "{pinned}"
    );
}

/// The config is a file mount inside the volume that covers its directory.
/// Docker orders mounts by destination depth, so `/opt/dhis2` is mounted
/// first and `/opt/dhis2/dhis.conf` lands on top of it; verified against
/// docker itself, not assumed.
#[test]
fn dhis2_mounts_the_config_read_only_inside_the_volume_that_covers_it() {
    let text = render_dhis2(&dhis2_spec());
    let svc = &parse(&text)["services"]["dhis2"];
    let volumes = svc["volumes"].as_sequence().unwrap();
    assert_eq!(volumes.len(), 2, "{text}");
    assert_eq!(
        volumes[0].as_str(),
        Some("./dhis2/dhis.conf:/opt/dhis2/dhis.conf:ro")
    );
    assert_eq!(volumes[1]["source"].as_str(), Some(DHIS2_HOME_VOLUME));
    assert_eq!(volumes[1]["target"].as_str(), Some("/opt/dhis2"));
}

#[test]
fn dhis2_sizes_the_heap_through_java_tool_options() {
    let text = render_dhis2(&dhis2_spec());
    let env = &parse(&text)["services"]["dhis2"]["environment"];
    // JAVA_OPTS is silently ignored by the distroless image variant, which
    // then takes a quarter of the machine's memory as its heap.
    assert!(env.get("JAVA_OPTS").is_none(), "{text}");
    assert_eq!(
        env["JAVA_TOOL_OPTIONS"].as_str(),
        Some("${DHIS2_JAVA_TOOL_OPTIONS:--Xms2g -Xmx4g -XX:+UseG1GC}")
    );
    // And the five variables dhis.conf substitutes are all there, the
    // encryption password included, so no placeholder is left standing as a
    // literal string.
    for var in [
        "DHIS2_DB_HOST",
        "DHIS2_DB_NAME",
        "DHIS2_DB_USER",
        "DHIS2_DB_PASSWORD",
        "DHIS2_ENCRYPTION_PASSWORD",
    ] {
        assert!(env.get(var).is_some(), "{var} is not passed: {text}");
    }
    assert_eq!(
        env["DHIS2_ENCRYPTION_PASSWORD"].as_str(),
        Some("${DHIS2_ENCRYPTION_PASSWORD:-}")
    );
}

#[test]
fn dhis2_waits_for_the_database_and_for_the_prep_one_shot() {
    let doc = parse(&render_dhis2(&dhis2_spec()));
    let depends = &service(&doc, "dhis2")["depends_on"];
    assert_eq!(
        depends["dhis2-db"]["condition"].as_str(),
        Some("service_healthy")
    );
    assert_eq!(
        depends["dhis2-prep"]["condition"].as_str(),
        Some("service_completed_successfully")
    );
    // /api/ping is the one route that answers unauthenticated, and the boot
    // budget is long because a cold start under emulation is minutes.
    let health = &service(&doc, "dhis2")["healthcheck"];
    assert_eq!(
        health["test"],
        Value::Sequence(vec![
            "CMD".into(),
            "curl".into(),
            "-fsS".into(),
            "http://localhost:8080/api/ping".into(),
        ])
    );
    assert_eq!(health["start_period"].as_str(), Some("240s"));
    assert_eq!(health["retries"].as_i64(), Some(60));
}

#[test]
fn the_database_is_postgis_and_answers_over_tcp_once_the_restore_is_done() {
    let doc = parse(&render_dhis2(&dhis2_spec()));
    let svc = service(&doc, "dhis2-db");
    assert_eq!(svc["image"].as_str(), Some(DHIS2_DB_IMAGE));
    assert_eq!(svc["restart"].as_str(), Some("unless-stopped"));
    let env = &svc["environment"];
    assert_eq!(env["POSTGRES_DB"].as_str(), Some("${DHIS2_DB_NAME:-dhis}"));
    assert_eq!(
        env["POSTGRES_USER"].as_str(),
        Some("${DHIS2_DB_USER:-dhis}")
    );
    assert_eq!(
        env["POSTGRES_PASSWORD"].as_str(),
        Some("${DHIS2_DB_PASSWORD:-dhis}")
    );
    // -h makes it a TCP probe. Over the unix socket the entrypoint's own
    // temporary server would answer while the dump was still restoring, and
    // DHIS2 would start against half a database.
    let test = svc["healthcheck"]["test"][1].as_str().unwrap();
    assert!(test.starts_with("pg_isready -h 127.0.0.1 "), "{test}");
    assert!(test.contains("${POSTGRES_USER}"), "{test}");
    // A seeded database is healthy only with the mark of a complete restore.
    assert!(
        test.ends_with(&format!("| grep -qx '{DHIS2_SEED_MARK}'")),
        "{test}"
    );

    // The data directory is a named volume, and the dump volume is mounted
    // where the postgres entrypoint looks for init scripts.
    let volumes = svc["volumes"].as_sequence().unwrap();
    assert_eq!(volumes[0]["source"].as_str(), Some(DHIS2_DB_VOLUME));
    assert_eq!(
        volumes[0]["target"].as_str(),
        Some("/var/lib/postgresql/data")
    );
    assert_eq!(volumes[1]["source"].as_str(), Some(DHIS2_DUMP_VOLUME));
    assert_eq!(
        volumes[1]["target"].as_str(),
        Some("/docker-entrypoint-initdb.d/")
    );
    // And it waits for the dump to be prepared before it starts restoring.
    assert_eq!(
        svc["depends_on"]["dhis2-dump"]["condition"].as_str(),
        Some("service_completed_successfully")
    );
}

#[test]
fn the_dump_one_shot_keeps_the_download_and_writes_the_one_pass_restore() {
    let text = render_dhis2(&dhis2_spec());
    let doc = parse(&text);
    let svc = service(&doc, "dhis2-dump");
    assert_eq!(svc["image"].as_str(), Some(DHIS2_DUMP_IMAGE));
    assert_eq!(svc["restart"].as_str(), Some("no"));
    assert_eq!(
        svc["environment"]["DHIS2_DB_DUMP_URL"].as_str(),
        Some(format!("${{DHIS2_DB_DUMP_URL:-{DHIS2_DEFAULT_SEED_URL}}}").as_str())
    );
    assert_eq!(
        svc["volumes"][0]["source"].as_str(),
        Some(DHIS2_DUMP_VOLUME)
    );
    assert_eq!(svc["volumes"][0]["target"].as_str(), Some("/opt/dump"));

    let script = svc["command"][0].as_str().unwrap();
    // A download that stopped is never kept: the move comes last.
    assert!(
        script.contains(&format!("mv raw.part {DHIS2_SEED_FILE}")),
        "{script}"
    );
    assert!(
        !script.contains("gzip -t"),
        "no extra pass over the dump: {script}"
    );
    // The rewrite and the restore are one pass, through sed into psql.
    assert!(
        script.contains(&format!(
            "| sed -E -f /docker-entrypoint-initdb.d/{DHIS2_REWRITE_SCRIPT}"
        )),
        "{script}"
    );
    assert!(script.contains("| psql -v ON_ERROR_STOP=1"), "{script}");
    // The entrypoint loads *.sql.gz by itself; the seed must not be one.
    assert!(!DHIS2_SEED_FILE.ends_with(".sql.gz"));
    assert!(DHIS2_RESTORE_SCRIPT.ends_with(".sh"));
    // A dump an earlier chap-devops prepared would be loaded twice.
    assert!(script.contains("rm -f dump.sql.gz out.part"), "{script}");
    // A dump from a newer pg_dump loads into the PostgreSQL 16 of the image.
    for line in [
        "/^\\\\restrict /d",
        "/^\\\\unrestrict /d",
        "/^SET transaction_timeout /d",
        // The roles of the source server do not exist here.
        "/^ALTER [^;]* OWNER TO /d",
        "/^(GRANT|REVOKE) /d",
        "/^ALTER DEFAULT PRIVILEGES /d",
    ] {
        assert!(script.contains(line), "{line} in {script}");
    }
    // Published dumps are `pg_dump --clean` without `--if-exists`, and the
    // entrypoint runs init scripts under ON_ERROR_STOP=1.
    assert!(
        script.contains("s/^ALTER TABLE (IF EXISTS )?(ONLY )?/ALTER TABLE IF EXISTS \\2/"),
        "{script}"
    );
    assert!(script.contains("/^DROP EXTENSION /d"), "{script}");
    assert!(script.contains("/^DROP SCHEMA /d"), "{script}");
    assert!(
        script.contains("s/^CREATE SCHEMA (IF NOT EXISTS )?/CREATE SCHEMA IF NOT EXISTS /"),
        "{script}"
    );
    // An already prepared dump is left alone, so a restart re-downloads
    // nothing.
    assert!(
        script.contains(&format!("if [ -f {DHIS2_SEED_FILE} ]; then")),
        "{script}"
    );
    // `$$` in the file is one `$` for the container's shell; a single one
    // would have been expanded away by compose.
    assert!(text.contains("$${DHIS2_DB_DUMP_URL}"), "{text}");
    assert!(
        text.contains(&format!("$$(wc -c < {DHIS2_SEED_FILE})")),
        "{text}"
    );
    assert!(text.contains("--username \"$$POSTGRES_USER\""), "{text}");
}

/// A dump that is a file is bind-mounted and copied; a URL is downloaded.
/// One variable either way, and the script's `[ -f ]` on it is what decides,
/// so both shapes get the same verification and the same rewriting.
#[test]
fn a_file_seed_is_mounted_and_copied_where_a_url_is_downloaded() {
    let text = render_dhis2(&Dhis2Spec {
        seed: Some(Dhis2SeedSource::File("dumps/laos.sql.gz".to_string())),
        ..dhis2_spec()
    });
    assert_no_tokens(&text);
    let doc = parse(&text);
    let svc = service(&doc, "dhis2-dump");
    assert_eq!(
        svc["environment"]["DHIS2_DB_DUMP_URL"].as_str(),
        Some(
            format!(
                "${{{}:-{DHIS2_SEED_MOUNT}}}",
                crate::components::DHIS2_SEED_ENV_VAR
            )
            .as_str()
        ),
        "the value names the mount, not the host path: {text}"
    );
    let volumes = svc["volumes"].as_sequence().unwrap();
    assert_eq!(volumes[0]["source"].as_str(), Some(DHIS2_DUMP_VOLUME));
    assert_eq!(
        volumes[1].as_str(),
        Some(format!("./dumps/laos.sql.gz:{DHIS2_SEED_MOUNT}:ro").as_str()),
        "a relative path is spelled with ./, as compose documents it"
    );
    // The branch that reads it, and the download that is still there for a URL.
    let script = svc["command"][0].as_str().unwrap();
    assert!(
        script.contains("if [ -f \"$${DHIS2_DB_DUMP_URL}\" ]; then"),
        "{script}"
    );
    assert!(
        script.contains("cp \"$${DHIS2_DB_DUMP_URL}\" raw.part"),
        "{script}"
    );
    assert!(
        script.contains("wget -O raw.part \"$${DHIS2_DB_DUMP_URL}\""),
        "{script}"
    );
    // Everything after the fetch is shared, so an operator's own dump is
    // restored exactly as a published one is.
    assert!(
        script.contains(&format!("mv raw.part {DHIS2_SEED_FILE}")),
        "{script}"
    );

    // An absolute path is passed through, and a URL adds no mount at all.
    let absolute = render_dhis2(&Dhis2Spec {
        seed: Some(Dhis2SeedSource::File("/srv/dumps/laos.sql.gz".to_string())),
        ..dhis2_spec()
    });
    assert!(
        absolute.contains(&format!(
            "      - /srv/dumps/laos.sql.gz:{DHIS2_SEED_MOUNT}:ro\n"
        )),
        "{absolute}"
    );
    // A Windows path with a drive is absolute too, on any host, and comes
    // out with the forward slashes compose reads on every platform.
    let drive = render_dhis2(&Dhis2Spec {
        seed: Some(Dhis2SeedSource::File(r"C:\dumps\laos.sql.gz".to_string())),
        ..dhis2_spec()
    });
    assert!(
        drive.contains(&format!(
            "      - C:/dumps/laos.sql.gz:{DHIS2_SEED_MOUNT}:ro\n"
        )),
        "{drive}"
    );
    let url = render_dhis2(&dhis2_spec());
    assert!(!url.contains(DHIS2_SEED_MOUNT), "{url}");
    assert_eq!(
        parse(&url)["services"]["dhis2-dump"]["volumes"]
            .as_sequence()
            .unwrap()
            .len(),
        1,
        "the dump volume and nothing else"
    );
    // Dropping the mount leaves no blank line in the volume list.
    assert!(
        url.contains("        target: /opt/dump\n    entrypoint:"),
        "{url}"
    );

    // The scheme is the whole of the rule, and nothing touches the disk to
    // apply it: a dump the operator has not copied in yet still renders.
    assert_eq!(
        Dhis2SeedSource::of(DHIS2_DEFAULT_SEED_URL),
        Dhis2SeedSource::Url(DHIS2_DEFAULT_SEED_URL.to_string())
    );
    assert_eq!(
        Dhis2SeedSource::of("dumps/x.sql.gz"),
        Dhis2SeedSource::File("dumps/x.sql.gz".to_string())
    );
}

/// The `.env` block names the variables the rendered file actually reads, so
/// the constants and the template cannot drift apart: a pin an operator
/// uncomments has to be one compose substitutes.
#[test]
fn the_dhis2_env_variables_are_the_ones_the_compose_file_reads() {
    let text = render_dhis2(&dhis2_spec());
    for var in [
        crate::components::DHIS2_TAG_ENV_VAR,
        crate::components::DHIS2_DB_PASSWORD_ENV_VAR,
        crate::components::DHIS2_ENCRYPTION_PASSWORD_ENV_VAR,
        crate::components::DHIS2_SEED_ENV_VAR,
        crate::components::DHIS2_JAVA_ENV_VAR,
    ] {
        assert!(
            text.contains(&format!("${{{var}:-")),
            "{var} is not a default the file reads: {text}"
        );
    }
    // And the value `.env` offers as the starting point is the one the file
    // already defaults to, so uncommenting the line changes nothing.
    assert!(
        text.contains(&format!(
            "${{{}:-{}}}",
            crate::components::DHIS2_JAVA_ENV_VAR,
            crate::components::DHIS2_DEFAULT_JAVA_OPTIONS
        )),
        "{text}"
    );
}

/// Every volume a backup archives has to be read from where the compose file
/// mounts it, so the `data_dir`s in that table come from this file rather
/// than from memory.
#[test]
fn the_archived_dhis2_volumes_are_mounted_where_the_backup_table_says() {
    let doc = parse(&render_dhis2(&dhis2_spec()));
    let target = |service: &str, volume: &str| -> String {
        doc["services"][service]["volumes"]
            .as_sequence()
            .unwrap()
            .iter()
            .find(|mount| mount.get("source").and_then(|s| s.as_str()) == Some(volume))
            .unwrap_or_else(|| panic!("{service} mounts {volume}"))["target"]
            .as_str()
            .unwrap()
            .to_string()
    };
    for part in crate::backup::COMPONENT_VOLUMES
        .iter()
        .filter(|part| part.name == "dhis2")
    {
        assert_eq!(
            target(part.service, part.volume),
            part.data_dir,
            "{} is mounted elsewhere",
            part.volume
        );
    }
}

#[test]
fn the_prep_one_shot_analyzes_and_clears_a_job_left_running() {
    let doc = parse(&render_dhis2(&dhis2_spec()));
    let svc = service(&doc, "dhis2-prep");
    // The same image as the database, so nothing extra is pulled.
    assert_eq!(svc["image"].as_str(), Some(DHIS2_DB_IMAGE));
    assert_eq!(svc["restart"].as_str(), Some("no"));
    assert_eq!(svc["entrypoint"], Value::Sequence(vec!["psql".into()]));
    let command: Vec<&str> = svc["command"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    // ON_ERROR_STOP=0: a database DHIS2 has not migrated yet has no
    // jobconfiguration table, and that has to be a no-op.
    assert_eq!(command[0..2], ["-v", "ON_ERROR_STOP=0"]);
    assert!(
        command.contains(
            &"UPDATE jobconfiguration SET jobstatus='SCHEDULED' WHERE jobstatus='RUNNING';"
        ),
        "{command:?}"
    );
    assert!(command.contains(&"ANALYZE;"), "{command:?}");
    assert_eq!(
        svc["depends_on"]["dhis2-db"]["condition"].as_str(),
        Some("service_healthy")
    );
}

#[test]
fn the_dhis2_config_scaffold_keeps_the_placeholders_dhis2_substitutes() {
    let text = render_dhis2_config(&Dhis2ConfigSpec::default());
    assert_no_tokens(&text);
    assert_eq!(
        text.lines().next(),
        Some(
            "# Written once by chap-devops when the dhis2 component was enabled; \
                 chap-devops never rewrites it."
        )
    );
    assert!(text.ends_with('\n'));

    // The three mandatory keys and the placeholders DHIS2 replaces from its
    // own environment: filling them here would be filling them twice.
    assert!(
        text.contains("\nconnection.url = jdbc:postgresql://${DHIS2_DB_HOST}/${DHIS2_DB_NAME}\n"),
        "{text}"
    );
    assert!(
        text.contains("\nconnection.username = ${DHIS2_DB_USER}\n"),
        "{text}"
    );
    assert!(
        text.contains("\nconnection.password = ${DHIS2_DB_PASSWORD}\n"),
        "{text}"
    );
    // Not required to boot, but unset DHIS2 encrypts with a password
    // compiled into its own source, and under 24 characters it refuses.
    assert!(
        text.contains("\nencryption.password = ${DHIS2_ENCRYPTION_PASSWORD}\n"),
        "{text}"
    );
    // The driver class already defaults correctly, so it is there as a
    // comment rather than as a value to keep in step.
    assert!(text.contains("\n# connection.driver_class = "), "{text}");
    assert!(text.contains("\nconnection.dialect = "), "{text}");

    // The one value the spec decides: every http and https target, so a
    // chap-core anywhere works without editing the file.
    assert!(
        text.contains("\nroute.remote_servers_allowed = http://*,https://*\n"),
        "{text}"
    );
    assert_eq!(render_dhis2_config(&Dhis2ConfigSpec::default()), text);
}

#[test]
fn the_route_allowlist_is_whatever_the_caller_narrowed_it_to() {
    let text = render_dhis2_config(&Dhis2ConfigSpec {
        route_allowed: "https://chap.example.org".to_string(),
    });
    assert_no_tokens(&text);
    assert!(
        text.contains("\nroute.remote_servers_allowed = https://chap.example.org\n"),
        "{text}"
    );
    assert!(!text.contains("http://chap:8000"));
    // The reason the value is not just left at DHIS2's default, in the file
    // the operator reads, and the one rule it has to obey.
    assert!(text.contains("may carry a path"), "{text}");
}

/// A seed password writes the init script that resets every user, and its
/// absence removes a script an earlier setting left in the volume.
#[test]
fn a_seed_password_resets_every_user_after_the_dump() {
    let text = render_dhis2(&Dhis2Spec {
        seed_password: Some("dis$trict".to_string()),
        ..dhis2_spec()
    });
    assert_no_tokens(&text);
    let doc = parse(&text);
    let svc = service(&doc, "dhis2-dump");
    assert_eq!(
        svc["environment"]["DHIS2_SEED_PASSWORD"].as_str(),
        Some("${DHIS2_SEED_PASSWORD:-dis$$trict}"),
        "a `$` is doubled for compose: {text}"
    );
    let script = svc["command"][0].as_str().unwrap();
    assert!(
        script.contains(&format!("cat > {DHIS2_PASSWORD_SCRIPT} <<EOF")),
        "{script}"
    );
    assert!(
        script.contains("UPDATE userinfo SET password = h.hash, disabled = false"),
        "{script}"
    );
    // A password alone does not log in a user with two-factor login, and an
    // expired account does not log in at all. Each only where the column is.
    assert!(
        script.contains("UPDATE userinfo SET twofactortype = 'NOT_ENABLED', secret = NULL;"),
        "{script}"
    );
    assert!(
        script.contains("UPDATE userinfo SET accountexpiry = NULL;"),
        "{script}"
    );
    // One hash for all the users, made by PostgreSQL.
    assert!(
        script.contains(
            "(SELECT :chap_devops_pgcrypto_schema.crypt('$$password', \
             :chap_devops_pgcrypto_schema.gen_salt('bf', 10)) AS hash)"
        ),
        "{script}"
    );
    // pgcrypto goes in a schema of its own: the dumps carry its functions in
    // `public` without the extension.
    assert!(
        script.contains("CREATE EXTENSION pgcrypto SCHEMA chap_devops_pgcrypto;"),
        "{script}"
    );
    assert!(
        script.contains("DROP SCHEMA chap_devops_pgcrypto CASCADE;"),
        "{script}"
    );
    // Before the check for a prepared dump, so a cached dump still gets it.
    assert!(
        script.find(DHIS2_PASSWORD_SCRIPT) < script.find(&format!("if [ -f {DHIS2_SEED_FILE} ]")),
        "{script}"
    );
    // The postgres entrypoint runs the files in name order.
    assert!(DHIS2_PASSWORD_SCRIPT > DHIS2_RESTORE_SCRIPT);

    let plain = render_dhis2(&dhis2_spec());
    let doc = parse(&plain);
    let svc = service(&doc, "dhis2-dump");
    assert!(
        svc["environment"]["DHIS2_SEED_PASSWORD"].is_null(),
        "{plain}"
    );
    let script = svc["command"][0].as_str().unwrap();
    assert!(
        script.contains(&format!("rm -f {DHIS2_PASSWORD_SCRIPT}")),
        "{script}"
    );
    assert!(!script.contains("UPDATE userinfo"), "{script}");
}

/// The restore leaves a mark as its last init script, and only a seeded
/// database is asked for it: an empty one has no restore to finish.
#[test]
fn a_seeded_database_is_healthy_only_after_the_whole_restore() {
    let text = render_dhis2(&dhis2_spec());
    let doc = parse(&text);
    let script = service(&doc, "dhis2-dump")["command"][0]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        script.contains(&format!(
            "IS '{DHIS2_SEED_MARK}';\" > {DHIS2_SEED_MARK_SCRIPT}"
        )),
        "{script}"
    );
    // It runs last: after the dump and after the password script.
    assert!(DHIS2_SEED_MARK_SCRIPT > DHIS2_PASSWORD_SCRIPT);
    assert!(DHIS2_SEED_MARK_SCRIPT > DHIS2_RESTORE_SCRIPT);

    let plain = render_dhis2(&unseeded_dhis2_spec());
    let doc = parse(&plain);
    let test = &service(&doc, "dhis2-db")["healthcheck"]["test"];
    assert_eq!(
        test[1].as_str(),
        Some("pg_isready -h 127.0.0.1 -U $${POSTGRES_USER} -d $${POSTGRES_DB}"),
        "{plain}"
    );
}

/// Every seed empties the values that analytics cannot read as a number, and
/// lists them first, before the passwords and the mark.
#[test]
fn every_seed_empties_the_values_analytics_cannot_read() {
    let text = render_dhis2(&dhis2_spec());
    let doc = parse(&text);
    let script = service(&doc, "dhis2-dump")["command"][0]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        script.contains(&format!("cat > {DHIS2_CLEAN_SCRIPT} <<'EOF'")),
        "{script}"
    );
    // `$$` is one `$` for the shell, and the heredoc is quoted, so the regex
    // reaches psql as it is written.
    assert!(
        script.contains(r"WHERE dv.value ~ '^-?[0-9]+(\.[0-9]+)?$$'"),
        "{script}"
    );
    assert!(
        script.contains("UPDATE datavalue dv SET value = NULL"),
        "{script}"
    );
    assert!(
        script
            .contains("chap-devops: data values that analytics cannot read as a number, emptied:"),
        "{script}"
    );
    // After the dump, before the password script and the mark.
    assert!(DHIS2_CLEAN_SCRIPT > DHIS2_RESTORE_SCRIPT);
    assert!(DHIS2_CLEAN_SCRIPT < DHIS2_PASSWORD_SCRIPT);
    assert!(DHIS2_CLEAN_SCRIPT < DHIS2_SEED_MARK_SCRIPT);

    // No seed, no restore, nothing to clean.
    assert!(!render_dhis2(&unseeded_dhis2_spec()).contains(DHIS2_CLEAN_SCRIPT));
}

/// A downloaded dump is checked for its DHIS2 version before the rewrite; a
/// local one is not, because `chap-devops init` read it already.
#[test]
fn only_a_downloaded_dump_is_checked_for_its_version() {
    let url = render_dhis2(&dhis2_spec());
    assert_no_tokens(&url);
    let doc = parse(&url);
    let svc = service(&doc, "dhis2-dump");
    assert_eq!(
        svc["environment"]["DHIS2_IMAGE_TAG"].as_str(),
        Some(
            format!(
                "${{DHIS2_IMAGE_TAG:-{}}}",
                crate::components::DHIS2_DEFAULT_TAG
            )
            .as_str()
        )
    );
    let script = svc["command"][0].as_str().unwrap();
    assert!(
        script.contains("grep -m1 -A5000 '^COPY public\\.flyway_schema_history '"),
        "{script}"
    );
    assert!(
        script.contains("-lt 2041"),
        "2.41 as the shell compares it: {script}"
    );
    assert!(
        script.find("dump_minor=") < script.find(&format!("mv raw.part {DHIS2_SEED_FILE}")),
        "before the dump is kept: {script}"
    );
    // The way out is a command: the header of `.chap-devops/components.yaml` says
    // not to edit it by hand.
    assert!(
        script.contains(
            "run chap-devops components enable dhis2 --tag $$dump_minor, then chap-devops up"
        ),
        "{script}"
    );
    assert!(!script.contains("components.yaml"), "{script}");

    let file = render_dhis2(&Dhis2Spec {
        seed: Some(Dhis2SeedSource::File("dumps/laos.sql.gz".to_string())),
        ..dhis2_spec()
    });
    assert_no_tokens(&file);
    assert!(!file.contains("dump_minor="), "{file}");
}
