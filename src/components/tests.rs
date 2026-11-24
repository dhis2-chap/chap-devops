use super::*;

#[test]
fn an_empty_document_is_chap_core_and_nothing_else() {
    let components: Components = serde_yaml_ng::from_str("{}").unwrap();
    assert_eq!(components, Components::default());
    assert!(components.chap_core.enabled);
    assert!(!components.ocs.enabled);
    assert!(!components.s3.enabled);
    assert_eq!(components.label(), "chap-core");
    assert!(components.compose_files().is_empty());
}

/// A loopback chap-core is this machine to the models, which reach it over
/// the host gateway; any other host is left as it is.
#[test]
fn an_external_chap_core_is_reached_from_containers_over_the_gateway() {
    let at = |url: &str| external_chap_core(url).unwrap().url_from_containers();
    assert_eq!(
        at("http://localhost:8000"),
        "http://host.docker.internal:8000"
    );
    assert_eq!(
        at("http://127.0.0.1:8000/"),
        "http://host.docker.internal:8000"
    );
    assert_eq!(at("http://localhost"), "http://host.docker.internal");
    assert_eq!(at("https://chap.example.org"), "https://chap.example.org");
    // A host that merely starts with "localhost" is not the loopback.
    assert_eq!(
        at("http://localhost.example.org"),
        "http://localhost.example.org"
    );
    assert!(external_chap_core("localhost:8000").is_err());
    assert_eq!(
        external_chap_core("http://x:1").unwrap().models_host,
        "localhost"
    );

    // Turning this deployment's own chap-core on forgets the other one.
    let mut components = Components {
        chap_core_external: Some(external_chap_core("http://localhost:8000").unwrap()),
        ..Components::default()
    };
    components.set_enabled(Component::ChapCore, false);
    assert!(components.has_chap_core_api());
    components.set_enabled(Component::ChapCore, true);
    assert!(components.chap_core_external.is_none());
}

#[test]
fn the_settings_round_trip_with_their_defaults() {
    let components = Components {
        chap_core: ChapCoreComponent { enabled: true },
        ocs: OcsComponent {
            enabled: true,
            port: Some(9010),
            base_url: None,
            read_only: false,
            image_tag: "main".into(),
        },
        s3: S3Component {
            enabled: true,
            port: None,
        },
        dhis2: Dhis2Component {
            enabled: true,
            port: Some(18080),
            image_tag: "2.41.7".into(),
            image: "dhis2/core-dev".into(),
            seed: Dhis2Seed::From("dumps/laos.sql.gz".into()),
            seed_password: true,
            modeling_app: ModelingApp::Unstable,
            modeling_app_build: None,
            connected_at: None,
        },
        dhis2_external: None,
        chap_core_external: None,
    };
    let text = serde_yaml_ng::to_string(&components).unwrap();
    assert!(text.contains("chap-core:\n"), "{text}");
    assert!(text.contains("  port: 9010\n"), "{text}");
    assert!(text.contains("  port: null\n"), "{text}");
    assert!(text.contains("  base_url: null\n"), "{text}");
    assert!(text.contains("  read_only: false\n"), "{text}");
    // The seed is one string, whichever of the three it is, so the block
    // reads as something an operator would type.
    assert!(text.contains("  seed: dumps/laos.sql.gz\n"), "{text}");
    assert!(text.contains("  seed_password: true\n"), "{text}");
    // The one record in the file, and it is written whichever way round it
    // is, so a reader can see that nothing has connected this deployment.
    assert!(text.contains("  connected_at: null\n"), "{text}");
    assert_eq!(
        serde_yaml_ng::from_str::<Components>(&text).unwrap(),
        components
    );

    assert_eq!(components.label(), "chap-core, ocs, s3, dhis2");
    assert_eq!(
        components.compose_files(),
        vec!["compose.ocs.yml", "compose.s3.yml", "compose.dhis2.yml"]
    );
    assert_eq!(components.port_of(Component::Dhis2), Some(18080));
    assert_eq!(components.dhis2_reach(), "http://localhost:18080");
    assert_eq!(components.port_of(Component::Ocs), Some(9010));
    assert_eq!(components.port_of(Component::S3), None);
    assert_eq!(
        components.ocs_url().as_deref(),
        Some("http://localhost:9010")
    );
    assert_eq!(components.ocs_reach(), "http://localhost:9010");
}

/// The reverse-proxy shape: no host port, and a base URL in its place.
#[test]
fn an_unpublished_instance_reads_as_internal_and_names_its_proxy() {
    let mut components = Components::default();
    components.ocs.enabled = true;
    components.ocs.port = None;
    assert_eq!(components.ocs_url(), None);
    assert_eq!(components.port_of(Component::Ocs), None);
    assert_eq!(components.ocs_reach(), "internal");

    components.ocs.base_url = Some("https://ocs.example.org".to_string());
    assert_eq!(
        components.ocs_reach(),
        "internal (proxy: https://ocs.example.org)"
    );

    // A published port is the address, whatever the base URL says: that is
    // where this machine reaches it.
    components.ocs.port = Some(9000);
    assert_eq!(components.ocs_reach(), "http://localhost:9000");
}

/// `port: null` is how a deployment records "no host port", and it has to
/// survive a round trip: the field defaults to 9000, so a null that read
/// back as the default would republish the port on the next sync.
#[test]
fn a_null_port_survives_the_round_trip_and_a_missing_one_is_the_default() {
    let explicit: Components =
        serde_yaml_ng::from_str("ocs:\n  enabled: true\n  port: null\n").unwrap();
    assert_eq!(explicit.ocs.port, None);
    let text = serde_yaml_ng::to_string(&explicit).unwrap();
    assert_eq!(
        serde_yaml_ng::from_str::<Components>(&text)
            .unwrap()
            .ocs
            .port,
        None
    );

    let omitted: Components = serde_yaml_ng::from_str("ocs:\n  enabled: true\n").unwrap();
    assert_eq!(omitted.ocs.port, Some(OCS_DEFAULT_PORT));
}

/// The same invariant on the DHIS2 block: the port defaults to 8080, so a
/// recorded `null` that read back as the default would republish the port on
/// the next sync - and put a DHIS2 login page on the host of a deployment
/// that deliberately keeps it behind a proxy.
#[test]
fn a_null_dhis2_port_survives_the_round_trip_and_a_missing_one_is_the_default() {
    let explicit: Components =
        serde_yaml_ng::from_str("dhis2:\n  enabled: true\n  port: null\n").unwrap();
    assert_eq!(explicit.dhis2.port, None);
    assert_eq!(explicit.dhis2_reach(), "internal");
    let text = serde_yaml_ng::to_string(&explicit).unwrap();
    assert_eq!(
        serde_yaml_ng::from_str::<Components>(&text)
            .unwrap()
            .dhis2
            .port,
        None
    );

    let omitted: Components = serde_yaml_ng::from_str("dhis2:\n  enabled: true\n").unwrap();
    assert_eq!(omitted.dhis2.port, Some(DHIS2_DEFAULT_PORT));
}

/// A `components.yaml` without a `dhis2` block, and one whose block sets
/// only some fields: both load, and neither turns DHIS2 on by itself.
#[test]
fn a_components_file_without_dhis2_loads_with_the_block_off() {
    let without: Components =
        serde_yaml_ng::from_str("chap-core:\n  enabled: true\nocs:\n  enabled: true\n").unwrap();
    assert!(!without.dhis2.enabled);
    assert_eq!(without.dhis2, Dhis2Component::default());
    assert_eq!(without.label(), "chap-core, ocs");
    assert!(!without.compose_files().contains(&DHIS2_COMPOSE.to_string()));

    // And a block with nothing but `enabled` keeps every default under it,
    // the seed among them.
    let partial: Components = serde_yaml_ng::from_str("dhis2:\n  enabled: true\n").unwrap();
    assert_eq!(partial.dhis2.image_tag, DHIS2_DEFAULT_TAG);
    assert_eq!(partial.dhis2.seed, Dhis2Seed::Default);
    assert_eq!(partial.compose_files(), vec![DHIS2_COMPOSE]);
}

/// The three seeds, in the spelling `components.yaml` holds and
/// `--dhis2-seed` takes.
#[test]
fn a_seed_is_one_string_and_reads_back_as_what_was_written() {
    assert_eq!(Dhis2Seed::parse("default"), Dhis2Seed::Default);
    assert_eq!(Dhis2Seed::parse("  DEFAULT "), Dhis2Seed::Default);
    assert_eq!(Dhis2Seed::parse(""), Dhis2Seed::Default, "no value is none");
    assert_eq!(Dhis2Seed::parse("none"), Dhis2Seed::None);
    assert_eq!(Dhis2Seed::parse("None"), Dhis2Seed::None);
    let url = "https://databases.dhis2.org/climate/laos/2.42/laos.sql.gz";
    assert_eq!(Dhis2Seed::parse(url), Dhis2Seed::From(url.to_string()));
    assert_eq!(
        Dhis2Seed::parse(" dumps/mine.sql.gz "),
        Dhis2Seed::From("dumps/mine.sql.gz".to_string()),
        "the shell's whitespace is not part of the path"
    );

    // Only a URL is a download, which is the one shape --offline refuses.
    assert!(Dhis2Seed::parse(url).is_url());
    assert!(Dhis2Seed::parse("http://example.org/d.sql.gz").is_url());
    assert!(!Dhis2Seed::parse("dumps/mine.sql.gz").is_url());
    assert!(
        !Dhis2Seed::Default.is_url(),
        "the table decides, not the flag"
    );
    assert!(!Dhis2Seed::None.is_url());

    // A `seed:` with nothing after it is null in YAML, and "no value" is
    // what the default means - not a type error on a hand-edited file.
    let bare: Components = serde_yaml_ng::from_str("dhis2:\n  enabled: true\n  seed:\n").unwrap();
    assert_eq!(bare.dhis2.seed, Dhis2Seed::Default);

    // Round trip through YAML as one scalar each.
    for seed in [
        Dhis2Seed::Default,
        Dhis2Seed::None,
        Dhis2Seed::From(url.to_string()),
    ] {
        let text = serde_yaml_ng::to_string(&seed).unwrap();
        assert_eq!(text.trim(), seed.as_str(), "{text}");
        assert_eq!(serde_yaml_ng::from_str::<Dhis2Seed>(&text).unwrap(), seed);
    }
}

/// The published dump is looked up, never built from the tag: a URL
/// assembled from a version would 404 on the first start.
#[test]
fn the_seed_dump_comes_from_the_table_and_an_unlisted_minor_has_none() {
    assert_eq!(dhis2_minor("2.42"), "2.42");
    assert_eq!(dhis2_minor("2.42.1"), "2.42");
    assert_eq!(dhis2_minor("2.42.1.1"), "2.42");
    assert_eq!(dhis2_minor(" 2.41.7 "), "2.41");
    assert_eq!(dhis2_minor("latest"), "latest", "a tag with no dots at all");

    assert_eq!(
        dhis2_seed_dump(DHIS2_DEFAULT_TAG),
        Some(crate::compose::render::DHIS2_DEFAULT_SEED_URL),
        "the minor chap-devops pins by default has a dump"
    );
    assert_eq!(dhis2_seed_dump("2.42.3"), dhis2_seed_dump("2.42"));
    // The 2.41 dump is no longer published, so 2.41 starts empty.
    assert_eq!(dhis2_seed_dump("2.41"), None);
    // Nothing is guessed for a line the table does not list.
    assert_eq!(dhis2_seed_dump("2.40"), None);
    assert_eq!(dhis2_seed_dump("latest"), None);

    // No two entries claim one minor, or the lookup would have two answers.
    let mut lines: Vec<&str> = DHIS2_SEED_DUMPS.iter().map(|(line, _)| *line).collect();
    let count = lines.len();
    lines.sort_unstable();
    lines.dedup();
    assert_eq!(lines.len(), count, "one minor, one dump");
    for (line, url) in DHIS2_SEED_DUMPS {
        assert_eq!(dhis2_minor(line), *line, "{line} is a minor line");
        assert!(url.starts_with("https://"), "{url}");
    }
}

/// What the first `chap-devops up` will restore, which is the seed setting and the
/// pinned minor together.
#[test]
fn the_resolved_seed_follows_the_setting_and_then_the_pin() {
    let mut components = Components::default();
    components.dhis2.enabled = true;
    assert_eq!(
        components.dhis2_seed_source(),
        dhis2_seed_dump(DHIS2_DEFAULT_TAG)
    );

    assert!(!components.dhis2_seed_is_unknown());

    // A pin the table does not know leaves the database empty, which is the
    // same answer `none` gives - and `dhis2_unknown_seed` is what tells the
    // two apart on the screen, which is what this flag is for.
    components.dhis2.image_tag = "2.40".to_string();
    assert_eq!(components.dhis2_seed_source(), None);
    assert!(components.dhis2_seed_is_unknown());

    components.dhis2.seed = Dhis2Seed::From("dumps/mine.sql.gz".to_string());
    assert_eq!(components.dhis2_seed_source(), Some("dumps/mine.sql.gz"));
    components.dhis2.image_tag = DHIS2_DEFAULT_TAG.to_string();
    assert_eq!(
        components.dhis2_seed_source(),
        Some("dumps/mine.sql.gz"),
        "an explicit dump outranks the table"
    );

    components.dhis2.seed = Dhis2Seed::None;
    assert_eq!(components.dhis2_seed_source(), None);
    assert!(
        !components.dhis2_seed_is_unknown(),
        "an empty database that was asked for is not one chap-devops has no dump for"
    );
}

#[test]
fn a_partial_block_keeps_the_defaults_of_the_fields_it_omits() {
    let components: Components = serde_yaml_ng::from_str("ocs:\n  enabled: true\n").unwrap();
    assert!(components.ocs.enabled);
    assert_eq!(components.ocs.port, Some(OCS_DEFAULT_PORT));
    assert_eq!(components.ocs.image_tag, OCS_DEFAULT_TAG);
    assert_eq!(components.ocs.base_url, None);
    assert!(!components.ocs.read_only);
    assert!(components.chap_core.enabled, "still on when unmentioned");
}

#[test]
fn chap_core_can_be_turned_off_explicitly() {
    let components: Components =
        serde_yaml_ng::from_str("chap-core:\n  enabled: false\nocs:\n  enabled: true\n").unwrap();
    assert!(!components.chap_core.enabled);
    assert_eq!(components.label(), "ocs");
    assert_eq!(components.enabled(), vec![Component::Ocs]);
}

#[test]
fn names_parse_in_the_spellings_people_type() {
    assert_eq!(Component::from_name("ocs").unwrap(), Component::Ocs);
    assert_eq!(Component::from_name(" OCS ").unwrap(), Component::Ocs);
    assert_eq!(
        Component::from_name("chap_core").unwrap(),
        Component::ChapCore
    );
    assert_eq!(
        Component::from_name("chap-core").unwrap(),
        Component::ChapCore
    );
    assert_eq!(Component::from_name("s3").unwrap(), Component::S3);
    assert_eq!(Component::from_name("dhis2").unwrap(), Component::Dhis2);
    assert_eq!(Component::from_name(" DHIS2 ").unwrap(), Component::Dhis2);
    let err = Component::from_name("nope").expect_err("not a component");
    assert!(matches!(
        err.downcast_ref::<ChapError>(),
        Some(ChapError::UnknownComponent(name)) if name == "nope"
    ));
}

#[test]
fn a_disabled_component_publishes_nothing() {
    let components = Components::default();
    assert_eq!(components.port_of(Component::Ocs), None);
    assert_eq!(components.port_of(Component::S3), None);
}

/// The port field keeps its value while the component is off - that is how
/// a re-enable puts the instance back where it was - so every reader of it
/// has to filter on `enabled`. An address for a component nothing is going
/// to start is a lie a caller would print.
#[test]
fn a_disabled_ocs_has_no_address_although_its_port_is_remembered() {
    let mut components = Components::default();
    components.ocs.port = Some(9010);
    assert!(!components.ocs.enabled);
    assert_eq!(components.ocs_url(), None);
    assert_eq!(components.ocs_reach(), "internal");

    components.ocs.enabled = true;
    assert_eq!(
        components.ocs_url().as_deref(),
        Some("http://localhost:9010")
    );
}

/// The compose file a component contributes is also the file a `disable` or
/// a re-init has to delete, so the two answers come from one place.
#[test]
fn the_component_compose_files_are_the_enabled_ones_own_files() {
    assert_eq!(Component::ChapCore.compose_file(), None);
    assert_eq!(Component::Ocs.compose_file(), Some(OCS_COMPOSE));
    assert_eq!(Component::S3.compose_file(), Some(S3_COMPOSE));

    let mut components = Components::default();
    assert!(components.compose_files().is_empty());
    components.set_enabled(Component::S3, true);
    assert_eq!(components.compose_files(), vec![S3_COMPOSE]);
    components.set_enabled(Component::Ocs, true);
    assert_eq!(
        components.compose_files(),
        vec![OCS_COMPOSE, S3_COMPOSE],
        "in the order they belong in the -f list"
    );
}

/// The volumes a component keeps are what a `disable` names and `--purge`
/// removes, so they have to be the component's own: chap-core's are
/// upstream's, declared in a file this CLI does not write, and an empty
/// slice is how that is said.
#[test]
fn a_component_names_the_volumes_it_keeps_and_chap_cores_are_upstreams() {
    assert_eq!(Component::Ocs.volumes(), ["ocs_data"].as_slice());
    assert_eq!(Component::S3.volumes(), ["s3_data"].as_slice());
    assert!(Component::ChapCore.volumes().is_empty());
    // Every volume `compose.dhis2.yml` declares, the download cache
    // included: this is the list the doctor decides leftovers by, so one left
    // out would be reported as a leftover on a deployment that has it and
    // `--purge` would leave it behind.
    assert_eq!(
        Component::Dhis2.volumes(),
        ["dhis2_home", "dhis2_db", "dhis2_dump"].as_slice()
    );

    // No two components may claim one volume: the doctor decides whether a
    // volume is a leftover by asking every component whether it is one of
    // theirs, and two answers would be two verdicts.
    let mut all: Vec<&str> = Component::ALL
        .iter()
        .flat_map(|c| c.volumes().iter().copied())
        .collect();
    let count = all.len();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), count, "one volume, one component");
}

/// Which compose services a component owns, which is what a `disable` stops
/// before the definitions go away.
#[test]
fn a_component_owns_its_own_service_and_the_siblings_beside_it() {
    assert!(Component::Ocs.owns_service("ocs"));
    assert!(Component::Ocs.owns_service("ocs-init"));
    assert!(Component::S3.owns_service("s3") && Component::S3.owns_service("s3-init"));
    assert!(!Component::Ocs.owns_service("s3") && !Component::S3.owns_service("ocs"));

    // The hyphen is the whole of the sibling rule: a name that merely starts
    // with a component's is some other service, and a name the component's
    // own is a prefix of is not the component's either.
    assert!(!Component::S3.owns_service("s3x"));
    assert!(!Component::Ocs.owns_service("ocsx"));
    assert!(!Component::Ocs.owns_service("oc"));

    // chap-core's services share no prefix with the component - `chap-core`
    // is not a service at all - so they can only be the ones no other
    // component claims. Which is also what leaves a model service where it
    // has always been, chap-core's, while `disable` refuses to take
    // chap-core away with one enabled.
    assert!(Component::ChapCore.owns_service("chap"));
    assert!(Component::ChapCore.owns_service("chap-worker"));
    assert!(Component::ChapCore.owns_service("postgres"));
    assert!(Component::ChapCore.owns_service("chapkit-ewars-model"));
    assert!(Component::ChapCore.owns_service("chapkit-ewars-model-init"));
    assert!(!Component::ChapCore.owns_service("ocs"));
    assert!(!Component::ChapCore.owns_service("ocs-init"));
    assert!(!Component::ChapCore.owns_service("s3-init"));

    // And the invariant the whole rule exists for: exactly one component
    // owns any given service, whatever the component is called and however
    // many services it brings. A fourth component's `<name>-db` and
    // `<name>-dump` are its own on the same terms `ocs-init` is, which a
    // hand-kept list of the names that are not chap-core's would not give
    // them - chap-core would claim them and a `disable chap-core` would
    // stop them.
    for component in Component::ALL {
        for suffix in ["", "-init", "-db", "-dump"] {
            let service = format!("{}{suffix}", component.name());
            let owners: Vec<&str> = Component::ALL
                .iter()
                .filter(|c| c.owns_service(&service))
                .map(|c| c.name())
                .collect();
            assert_eq!(owners, [component.name()], "who owns {service}");
        }
    }
}

/// Every note `ocs` and `s3` print names the command that acts on it,
/// which is the project's message rule, and none of them overstates the S3
/// contract OCS does not have yet.
#[test]
fn the_component_notes_name_the_command_that_answers_them() {
    assert!(S3_WITHOUT_OCS_NOTE.contains("`chap-devops components enable ocs`"));
    assert!(S3_LEAVES_OCS_NOTE.contains("`chap-devops components enable s3`"));
    assert!(
        S3_LEAVES_OCS_NOTE.contains("does not read them yet"),
        "the S3_* block is forward-looking, and the note says so"
    );
    assert!(OCS_DATA_SOURCE_NOTE.contains("`chap-devops auth show`"));
    // The two ERA5-Land accounts are not alternatives, and the note must not
    // read as though picking one were enough.
    assert!(
        OCS_DATA_SOURCE_NOTE
            .contains("ERA5-Land needs one or both of ECMWF_DATASTORES_* and EDH_API_KEY"),
        "{OCS_DATA_SOURCE_NOTE}"
    );
    assert!(
        !OCS_DATA_SOURCE_NOTE.contains("needs one of"),
        "{OCS_DATA_SOURCE_NOTE}"
    );
    for note in [
        S3_SOON_NOTE,
        S3_WITHOUT_OCS_NOTE,
        S3_LEAVES_OCS_NOTE,
        OCS_DATA_SOURCE_NOTE,
    ] {
        assert!(!note.contains('\n'), "one line each: {note}");
    }
}

/// Each DHIS2 note says one thing an operator cannot see for themselves, and
/// names the command or the file that answers it.
#[test]
fn the_dhis2_notes_say_what_the_first_start_does_and_where_to_change_it() {
    let seeded = dhis2_seed_note(Some("https://databases.dhis2.org/x.sql.gz"));
    assert!(
        seeded.contains("https://databases.dhis2.org/x.sql.gz"),
        "{seeded}"
    );
    // The restore is the postgres entrypoint's, so it happens once - which is
    // the half that decides whether changing the seed later does anything.
    assert!(seeded.contains("once"), "{seeded}");
    assert!(
        seeded.contains("`chap-devops components disable dhis2 --purge`"),
        "{seeded}"
    );

    let empty = dhis2_seed_note(None);
    assert!(empty.contains("starts empty"), "{empty}");
    // The way out is a command: the header of `.chap-devops/components.yaml` says
    // not to edit it by hand.
    assert!(
        empty.contains("`chap-devops components enable dhis2 --seed URL`"),
        "{empty}"
    );
    assert!(!empty.contains("components.yaml"), "{empty}");

    let unknown = dhis2_unknown_seed("2.40.1");
    assert!(
        unknown.contains("2.40"),
        "the minor line, not the tag: {unknown}"
    );
    assert!(!unknown.contains("2.40.1"), "{unknown}");
    assert!(
        unknown.contains("`chap-devops components enable dhis2 --seed URL`"),
        "{unknown}"
    );
    assert!(!unknown.contains("components.yaml"), "{unknown}");
    // The seed applies only when the database is created, so a tag moved on
    // a `dhis2_db` that is already there does not empty it.
    assert!(
        unknown.contains("so a new `dhis2_db` starts empty"),
        "{unknown}"
    );
    let master = dhis2_unknown_seed("master");
    assert!(
        master.contains("no DHIS2 demo dump for the image tag `master`"),
        "{master}"
    );

    assert!(
        DHIS2_FIRST_START_NOTE.contains("minutes"),
        "{DHIS2_FIRST_START_NOTE}"
    );
    assert!(
        DHIS2_FIRST_START_NOTE.contains("`chap-devops logs dhis2`"),
        "{DHIS2_FIRST_START_NOTE}"
    );

    // A DHIS2 beside a Chap cannot talk to it until the route exists, so
    // the note that says so names the command that makes it.
    assert!(
        DHIS2_CONNECT_NOTE.contains("`chap-devops dhis2 connect`"),
        "{DHIS2_CONNECT_NOTE}"
    );
    assert!(DHIS2_CONNECT_NOTE.contains("route"), "{DHIS2_CONNECT_NOTE}");
    // Chap is not a DHIS2 product, and nothing here is a "stack".
    assert!(
        !DHIS2_CONNECT_NOTE.contains("stack"),
        "{DHIS2_CONNECT_NOTE}"
    );

    // A downgrade is the one case that has to be stopped before `chap-devops up`,
    // so the line leads with the command that makes it recoverable.
    // A move forward warns that the migration cannot be undone, and does not
    // talk about an older image.
    let forward = dhis2_tag_change_note("2.41", "2.42");
    assert!(forward.contains("from 2.41 to 2.42"), "{forward}");
    assert!(
        forward.contains("migrates the database to 2.42"),
        "{forward}"
    );
    assert!(forward.contains("cannot be undone"), "{forward}");
    assert!(forward.contains("`chap-devops backup create`"), "{forward}");
    assert!(!forward.contains("older image"), "{forward}");
    assert!(!forward.contains('\n'), "{forward}");
    // A tag that is not a version is not judged a move forward.
    assert!(dhis2_tag_change_note("2.42", "master").contains("older image"));

    let moved = dhis2_tag_change_note("2.42", "2.41");
    assert!(moved.contains("from 2.42 to 2.41"), "{moved}");
    assert!(moved.contains("`chap-devops backup create`"), "{moved}");
    assert!(moved.contains("404"), "{moved}");

    for note in [
        seeded,
        empty,
        unknown,
        DHIS2_FIRST_START_NOTE.to_string(),
        moved,
    ] {
        assert!(!note.contains('\n'), "one line each: {note}");
    }
}

/// The record round-trips as one string, and a deployment that has it is
/// not asked to connect again.
#[test]
fn a_recorded_connect_round_trips_and_stops_the_hint() {
    let mut components = Components::default();
    components.set_enabled(Component::Dhis2, true);
    assert!(components.dhis2_needs_connecting());

    components.dhis2.connected_at = Some("2026-09-27T09:12:33Z".to_string());
    assert!(!components.dhis2_needs_connecting());

    let text = serde_yaml_ng::to_string(&components).unwrap();
    assert!(
        text.contains("connected_at: 2026-09-27T09:12:33Z"),
        "{text}"
    );
    assert_eq!(
        serde_yaml_ng::from_str::<Components>(&text).unwrap(),
        components
    );
}

/// Switching the component off forgets the record, wherever the switch was
/// thrown: the CLI, the browser's components page and `init --force` all go
/// through this one method.
#[test]
fn disabling_dhis2_forgets_the_recorded_connect() {
    let mut components = Components::default();
    components.set_enabled(Component::Dhis2, true);
    components.dhis2.connected_at = Some("2026-09-27T09:12:33Z".to_string());

    components.set_enabled(Component::Dhis2, false);
    assert_eq!(components.dhis2.connected_at, None);
    // The settings themselves are not touched: a re-enable keeps the port
    // and the tag it had.
    assert_eq!(components.dhis2.port, Some(DHIS2_DEFAULT_PORT));
    assert_eq!(components.dhis2.image_tag, DHIS2_DEFAULT_TAG);

    // And re-enabling asks for a connect again, because the database the
    // record was true of may be gone and a restored seed dump ships a
    // `chap` route of its own.
    components.set_enabled(Component::Dhis2, true);
    assert!(components.dhis2_needs_connecting());
}

/// A deployment with no chap-core is never asked to connect: the route
/// would point at a service that is not there, and `chap-devops dhis2 connect`
/// refuses on exactly those grounds.
#[test]
fn a_deployment_without_chap_core_is_not_asked_to_connect() {
    let mut components = Components::default();
    components.set_enabled(Component::Dhis2, true);
    components.set_enabled(Component::ChapCore, false);
    assert!(!components.dhis2_needs_connecting());
}

/// The hint says what chap-devops knows rather than what DHIS2 is, names the
/// command, and says when to run it on the one caller that has not seen
/// DHIS2 answer.
#[test]
fn the_connect_hint_claims_only_what_it_read() {
    let answering = dhis2_connect_hint(true);
    let waiting = dhis2_connect_hint(false);
    for line in [&answering, &waiting] {
        assert!(line.contains("`chap-devops dhis2 connect`"), "{line}");
        assert!(line.starts_with("chap-devops has not connected"), "{line}");
        assert!(!line.contains("stack"), "{line}");
        assert!(!line.contains('\n'), "one line each: {line}");
    }
    assert!(!answering.contains("once DHIS2 answers"), "{answering}");
    assert!(waiting.contains("once DHIS2 answers"), "{waiting}");

    for note in [DHIS2_CONNECT_FORGOTTEN, DHIS2_CONNECT_FORGOTTEN_WITH_VOLUME] {
        assert!(note.contains("`chap-devops dhis2 connect`"), "{note}");
        assert!(!note.contains('\n'), "one line each: {note}");
    }
    assert!(
        DHIS2_CONNECT_FORGOTTEN_WITH_VOLUME.contains("`dhis2_db`"),
        "{DHIS2_CONNECT_FORGOTTEN_WITH_VOLUME}"
    );
}

#[test]
fn a_loopback_chap_core_has_a_port_to_look_for_a_container_on() {
    let port = |url: &str| external_chap_core(url).unwrap().loopback_port();
    assert_eq!(port("http://localhost:8000"), Some(8000));
    assert_eq!(port("http://127.0.0.1:8001/"), Some(8001));
    assert_eq!(port("http://localhost"), Some(80));
    assert_eq!(port("https://localhost/api"), Some(443));
    assert_eq!(port("http://chap.example.org:8000"), None);
    assert_eq!(port("http://localhostile:8000"), None);
}

/// Option 10 of the AI page: chap-core from its checkout with
/// `make restart`, which is a container publishing 8000.
#[test]
fn a_chap_core_in_a_container_calls_the_models_back_at_the_gateway() {
    let mut external = external_chap_core("http://localhost:8000").unwrap();
    let note = detect_models_host(&mut external, &|port| {
        (port == 8000).then(|| "chapdev-chap-1".to_string())
    })
    .expect("a note");
    assert_eq!(external.models_host, HOST_GATEWAY);
    assert!(note.contains("the container `chapdev-chap-1`"), "{note}");
    assert!(
        note.contains(
            "`chap-devops components enable chap-core --url http://localhost:8000 --models-host \
             localhost`"
        ),
        "{note}"
    );

    // A process on this machine, or no docker to ask: left as it was.
    let mut process = external_chap_core("http://localhost:8000").unwrap();
    assert!(detect_models_host(&mut process, &|_| None).is_none());
    assert_eq!(process.models_host, "localhost");

    // A chap-core on another machine is nothing a local container says
    // anything about, and a host chosen by hand is kept.
    let mut remote = external_chap_core("http://chap.example.org:8000").unwrap();
    assert!(detect_models_host(&mut remote, &|_| Some("x".into())).is_none());
    let mut chosen = external_chap_core("http://localhost:8000").unwrap();
    chosen.models_host = "10.0.0.5".into();
    assert!(detect_models_host(&mut chosen, &|_| Some("x".into())).is_none());
    assert_eq!(chosen.models_host, "10.0.0.5");
}

/// The newest Flyway migration of a dump, compared as numbers, with the
/// table found by its name and the column by its name.
#[test]
fn the_dump_version_is_the_newest_flyway_migration() {
    let dump = "COPY public.datavalue (dataelementid, value) FROM stdin;\n\
                1\t2.99.1\n\
                \\.\n\
                COPY public.flyway_schema_history (installed_rank, version, description) FROM stdin;\n\
                1\t2.42.9\tone\n\
                2\t2.42.54\ttwo\n\
                3\t2.42.10\tthree\n\
                4\t\\N\trepeatable\n\
                \\.\n\
                COPY public.zzz (a) FROM stdin;\n\
                2.50.1\n";
    assert_eq!(
        dump_migration_of(std::io::Cursor::new(dump)).as_deref(),
        Some("2.42.54"),
        "numbers, not text: 54 is newer than 10 and 9"
    );
    assert_eq!(migration_minor("2.42.54").as_deref(), Some("2.42"));
    // Very old DHIS2, and a dump that is not a DHIS2 database.
    let old = "COPY public.schema_version (version_rank, installed_rank, version) FROM stdin;\n\
               1\t1\t2.30.1\n\\.\n";
    assert_eq!(
        dump_migration_of(std::io::Cursor::new(old)).as_deref(),
        Some("2.30.1")
    );
    assert_eq!(dump_migration_of(std::io::Cursor::new("SELECT 1;\n")), None);
}

#[test]
fn the_tag_follows_the_dump_and_never_goes_below_it() {
    assert_eq!(tag_for_dump("2.42", None).unwrap(), "2.42");
    assert_eq!(tag_for_dump("2.42", Some("2.43.1")).unwrap(), "2.43.1");
    assert_eq!(tag_for_dump("2.42", Some("master")).unwrap(), "master");
    let older = tag_for_dump("2.42", Some("2.41")).unwrap_err().to_string();
    assert!(older.contains("use `--dhis2-tag 2.42` or newer"), "{older}");
    let old_dump = tag_for_dump("2.40", None).unwrap_err().to_string();
    assert!(
        old_dump.contains("supports DHIS2 2.41 and newer"),
        "{old_dump}"
    );
}

/// chap-devops supports DHIS2 2.41 and newer, so an older tag is refused on the
/// command line. A tag that is not a version passes.
#[test]
fn a_dhis2_tag_older_than_the_oldest_supported_is_refused() {
    let refused = dhis2_tag_arg("2.40", "--tag").unwrap_err().to_string();
    assert!(
        refused.contains("--tag 2.40 is older than DHIS2 2.41"),
        "{refused}"
    );
    assert!(refused.contains("`--tag 2.41` or newer"), "{refused}");
    assert!(dhis2_tag_arg("2.39.1", "--dhis2-tag").is_err());
    for tag in ["2.41", "2.41.3", "2.42", "2.43.1", "master", "latest"] {
        assert_eq!(dhis2_tag_arg(tag, "--tag").unwrap(), tag);
    }
}

#[test]
fn the_modeling_app_setting_is_stable_unstable_or_a_version() {
    assert_eq!(ModelingApp::parse("stable"), Ok(ModelingApp::Stable));
    assert_eq!(ModelingApp::parse(" unstable "), Ok(ModelingApp::Unstable));
    assert_eq!(
        ModelingApp::parse("v7.5.0"),
        Ok(ModelingApp::Version("7.5.0".into()))
    );
    for bad in ["latest", "7.5", "7.5.0-rc1", "", "7..0"] {
        let err = ModelingApp::parse(bad).unwrap_err();
        assert!(
            err.contains("stable, unstable, or a version"),
            "{bad}: {err}"
        );
    }
    // `stable` is the default and is not written; the others are, and a
    // value that is none of the three does not load.
    let written = serde_yaml_ng::to_string(&Dhis2Component::default()).unwrap();
    assert!(!written.contains("modeling_app"), "{written}");
    let pinned = Dhis2Component {
        modeling_app: ModelingApp::Version("7.5.0".into()),
        ..Dhis2Component::default()
    };
    let written = serde_yaml_ng::to_string(&pinned).unwrap();
    assert!(written.contains("modeling_app: 7.5.0"), "{written}");
    let back: Dhis2Component = serde_yaml_ng::from_str(&written).unwrap();
    assert_eq!(back.modeling_app, pinned.modeling_app);
    assert!(serde_yaml_ng::from_str::<Dhis2Component>("modeling_app: nightly\n").is_err());
}

/// The setting and the record follow whichever DHIS2 `chap-devops dhis2`
/// talks to.
#[test]
fn the_modeling_app_follows_the_dhis2_in_use() {
    let mut components = Components::default();
    *components.modeling_app_mut() = ModelingApp::Unstable;
    assert_eq!(components.dhis2.modeling_app, ModelingApp::Unstable);
    components.dhis2_external = Some(ExternalDhis2::default());
    assert_eq!(*components.modeling_app(), ModelingApp::Stable);
    *components.modeling_app_build_mut() = Some(ModelingAppBuild {
        commit: "0c30f8de28515c933aeb5916a3d94b8478f843de".into(),
        digest: "sha256:aa".into(),
    });
    assert!(components.dhis2.modeling_app_build.is_none());
    assert_eq!(
        components.modeling_app_build().unwrap().short_commit(),
        "0c30f8d"
    );
}
