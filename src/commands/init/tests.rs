use super::components::*;
use super::dropped::*;
use super::env::*;
use super::ports::*;
use super::summary::*;
use super::*;
use crate::chapcore;
use crate::cli::ComponentPortArg;
use crate::components::Components;
use crate::project::{AuthState, cached_compose_file};
use crate::registry::load_embedded;

/// A deployment without chap-core can take models too, and is told how
/// they run there before it enables one.
#[test]
fn a_deployment_without_chap_core_is_told_its_models_run_on_their_own() {
    let mut components = Components::default();
    assert_eq!(no_models_line(&components), Some((NO_MODELS, None)));

    components.set_enabled(Component::ChapCore, false);
    let (line, hint) = no_models_line(&components).expect("a models-only deployment gets the line");
    assert!(line.contains("`chap-devops models enable ID`"), "{line}");
    let hint = hint.expect("and the hint about how they run");
    assert!(hint.contains("published host port"), "{hint}");

    // A DHIS2 or an OCS on its own was not about models.
    components.set_enabled(Component::Dhis2, true);
    assert_eq!(no_models_line(&components), None);
}

#[test]
fn models_all_enables_every_model_but_the_starters() {
    let registry = load_embedded().unwrap();
    let all = parse_models("all", &registry).unwrap();
    let expected = registry.models.iter().filter(|m| !m.is_starter()).count();
    assert_eq!(all.enable.len(), expected);
    assert!(all.enable.len() > 1);
    assert!(all.enable.iter().any(|r| r.id == "chapkit_ewars_model"));
}

#[test]
fn models_none_enables_nothing() {
    let registry = load_embedded().unwrap();
    assert!(parse_models("none", &registry).unwrap().enable.is_empty());
    assert!(parse_models("  ", &registry).unwrap().enable.is_empty());
}

/// There is no default model: `default` is an id like any other, and the
/// marketplace lists no model by that name.
#[test]
fn models_has_no_default_value() {
    let registry = load_embedded().unwrap();
    let err = parse_models("default", &registry).expect_err("no model is named default");
    assert!(matches!(
        err.downcast_ref::<ChapError>(),
        Some(ChapError::UnknownModel(id)) if id == "default"
    ));
}

#[test]
fn models_takes_a_comma_list_of_ids_or_service_ids() {
    let registry = load_embedded().unwrap();
    let sel = parse_models("chapkit_ewars_model, auto-arima-chapkit", &registry).unwrap();
    let ids: Vec<&str> = sel.enable.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["chapkit_ewars_model", "auto-arima-chapkit"]);
}

#[test]
fn an_unknown_model_is_named_in_the_error() {
    let registry = load_embedded().unwrap();
    let err = parse_models("chapkit_ewars_model,nope", &registry).expect_err("unknown model");
    assert!(matches!(
        err.downcast_ref::<ChapError>(),
        Some(ChapError::UnknownModel(id)) if id == "nope"
    ));
}

/// `parse_components` from the two list flags alone, which is what most of
/// these cases are about.
fn components_of(with: Option<&str>, without: Option<&str>) -> Result<Components> {
    parse_components(&ComponentFlags {
        with,
        without,
        ..ComponentFlags::default()
    })
}

#[test]
fn only_names_the_whole_component_set() {
    let only = |spec: &str| {
        parse_components(&ComponentFlags {
            only: Some(spec),
            ..ComponentFlags::default()
        })
    };
    let ocs = only("ocs,s3").unwrap();
    assert!(!ocs.chap_core.enabled && ocs.ocs.enabled && ocs.s3.enabled);
    assert!(!ocs.dhis2.enabled);
    assert_eq!(ocs.label(), "ocs, s3");

    let dhis2 = only("dhis2").unwrap();
    assert_eq!(dhis2.enabled(), vec![Component::Dhis2]);

    // chap-core is on only when it is named.
    let core = only("chap-core,ocs").unwrap();
    assert!(core.chap_core.enabled && core.ocs.enabled);

    assert!(only("none").unwrap().enabled().is_empty());
    let empty = only(" , ").unwrap_err().to_string();
    assert!(empty.contains("`--only none`"), "{empty}");
    assert!(only("nope").is_err());
}

#[test]
fn the_with_and_without_lists_shape_the_component_set() {
    let plain = components_of(None, None).unwrap();
    assert_eq!(plain, Components::default());
    assert!(plain.chap_core.enabled && !plain.ocs.enabled);

    let both = components_of(Some("ocs,s3"), None).unwrap();
    assert!(both.chap_core.enabled && both.ocs.enabled && both.s3.enabled);
    assert_eq!(both.label(), "chap-core, ocs, s3");
    // Whitespace and case are the shell's, not part of the name.
    assert_eq!(components_of(Some(" OCS , s3 "), None).unwrap(), both);

    let standalone = components_of(Some("ocs"), Some("chap-core")).unwrap();
    assert!(!standalone.chap_core.enabled && standalone.ocs.enabled);
    assert_eq!(standalone.label(), "ocs");

    // An unknown name is a typo to report before anything is written.
    let err = components_of(Some("ocs,nope"), None).expect_err("unknown component");
    assert!(matches!(
        err.downcast_ref::<ChapError>(),
        Some(ChapError::UnknownComponent(name)) if name == "nope"
    ));

    // And a name on both lists is a contradiction, not a silent winner.
    let err = components_of(Some("ocs"), Some("ocs")).expect_err("on and off at once");
    assert!(
        err.to_string().contains("both --with and --without"),
        "{err}"
    );
}

#[test]
fn the_ocs_base_url_needs_the_component_and_has_to_be_absolute() {
    let base_url = |with, url| {
        parse_components(&ComponentFlags {
            with,
            ocs_base_url: Some(url),
            ..ComponentFlags::default()
        })
    };
    let proxied = base_url(Some("ocs"), "https://ocs.example.org/").unwrap();
    assert_eq!(
        proxied.ocs.base_url.as_deref(),
        Some("https://ocs.example.org"),
        "the trailing slash goes: OCS appends a path to this"
    );

    // An empty value is no value, not a refusal.
    assert_eq!(base_url(Some("ocs"), "  ").unwrap().ocs.base_url, None);

    let err = base_url(None, "https://ocs.example.org").expect_err("no component to set it on");
    assert!(err.to_string().contains("--with ocs"), "{err}");

    let err = base_url(Some("ocs"), "ocs.example.org").expect_err("no scheme");
    assert!(err.to_string().contains("absolute URL"), "{err}");
}

/// A port for a component this deployment is not getting is the same mistake
/// as a base URL for one: it has to be a refusal, because a flag that
/// quietly did nothing leaves the operator waiting for OCS on a port no file
/// mentions.
#[test]
fn the_component_ports_and_the_read_only_switch_need_their_component() {
    let with_ocs = |flags: ComponentFlags| {
        parse_components(&ComponentFlags {
            with: Some("ocs,s3"),
            ..flags
        })
    };

    let moved = with_ocs(ComponentFlags {
        ocs_port: Some(ComponentPortArg(Some(18010))),
        s3_port: Some(ComponentPortArg(Some(18011))),
        ..ComponentFlags::default()
    })
    .unwrap();
    assert_eq!(moved.ocs.port, Some(18010));
    assert_eq!(moved.s3.port, Some(18011));

    // `none` is how a component is reached inside the deployment alone, and
    // it has to survive the flag as well as the record.
    let internal = with_ocs(ComponentFlags {
        ocs_port: Some(ComponentPortArg(None)),
        ..ComponentFlags::default()
    })
    .unwrap();
    assert_eq!(internal.ocs.port, None);
    assert_eq!(internal.ocs_reach(), "internal");

    let read_only = with_ocs(ComponentFlags {
        ocs_read_only: true,
        ..ComponentFlags::default()
    })
    .unwrap();
    assert!(read_only.ocs.read_only);
    // Left alone without the flag: the file says what it says, and this is
    // only a record of it.
    assert!(!with_ocs(ComponentFlags::default()).unwrap().ocs.read_only);

    for (flags, wanted) in [
        (
            ComponentFlags {
                ocs_port: Some(ComponentPortArg(Some(18010))),
                ..ComponentFlags::default()
            },
            "--ocs-port needs the ocs component; add `--with ocs`",
        ),
        (
            ComponentFlags {
                with: Some("ocs"),
                s3_port: Some(ComponentPortArg(Some(18011))),
                ..ComponentFlags::default()
            },
            "--s3-port needs the s3 component; add `--with s3`",
        ),
        (
            ComponentFlags {
                ocs_read_only: true,
                ..ComponentFlags::default()
            },
            "--ocs-read-only needs the ocs component; add `--with ocs`",
        ),
        (
            ComponentFlags {
                dhis2_port: Some(ComponentPortArg(Some(18080))),
                ..ComponentFlags::default()
            },
            "--dhis2-port needs the dhis2 component; add `--with dhis2`",
        ),
        (
            ComponentFlags {
                dhis2_seed: Some("none"),
                ..ComponentFlags::default()
            },
            "--dhis2-seed needs the dhis2 component; add `--with dhis2`",
        ),
        (
            ComponentFlags {
                dhis2_seed_password: Some("district"),
                ..ComponentFlags::default()
            },
            "--dhis2-seed-password needs the dhis2 component; add `--with dhis2`",
        ),
    ] {
        let err = parse_components(&flags).expect_err("no component to set it on");
        assert_eq!(err.to_string(), wanted);
    }
}

/// `--dhis2-tag` picks the version, and the default seed follows it: a
/// minor line with no known dump starts empty.
#[test]
fn the_dhis2_tag_picks_the_version_and_the_seed_follows_it() {
    let with = |tag: &str| {
        parse_components(&ComponentFlags {
            with: Some("dhis2"),
            dhis2_tag: Some(tag),
            ..ComponentFlags::default()
        })
    };
    let seeded = with("2.42.6").unwrap();
    assert_eq!(seeded.dhis2.image_tag, "2.42.6");
    assert!(seeded.dhis2_seed_source().unwrap().contains("2.42"));
    assert!(with("2.41").unwrap().dhis2_seed_is_unknown());
    let new = with("2.43").unwrap();
    assert_eq!(new.dhis2.image_tag, "2.43");
    assert!(new.dhis2_seed_is_unknown());
    assert!(with(" ").is_err());

    let dev = parse_components(&ComponentFlags {
        with: Some("dhis2"),
        dhis2_image: Some("dhis2/core-dev"),
        dhis2_tag: Some("master"),
        ..ComponentFlags::default()
    })
    .unwrap();
    assert_eq!(dev.dhis2.image, "dhis2/core-dev");
    assert_eq!(dev.dhis2.image_tag, "master");
    let both = parse_components(&ComponentFlags {
        with: Some("dhis2"),
        dhis2_image: Some("dhis2/core-dev:master"),
        ..ComponentFlags::default()
    })
    .unwrap_err()
    .to_string();
    assert!(both.contains("--dhis2-tag master"), "{both}");
    let err = parse_components(&ComponentFlags {
        dhis2_tag: Some("2.42"),
        ..ComponentFlags::default()
    })
    .unwrap_err();
    assert!(err.to_string().contains("--with dhis2"), "{err}");
}

/// The DHIS2 flags, and the one contradiction between a flag and a global
/// one: a seed that has to be downloaded on a deployment told not to reach
/// out.
#[test]
fn the_dhis2_flags_record_the_port_and_the_seed() {
    use crate::components::{DHIS2_DEFAULT_PORT, Dhis2Seed};
    let with_dhis2 = |flags: ComponentFlags| {
        parse_components(&ComponentFlags {
            with: Some("dhis2"),
            ..flags
        })
    };

    // The defaults: published on 8780, seeded from the table.
    let plain = with_dhis2(ComponentFlags::default()).unwrap();
    assert_eq!(plain.dhis2.port, Some(DHIS2_DEFAULT_PORT));
    assert_eq!(plain.dhis2.seed, Dhis2Seed::Default);
    assert_eq!(plain.dhis2.image_tag, crate::components::DHIS2_DEFAULT_TAG);
    assert_eq!(plain.dhis2_reach(), "http://localhost:8780");

    let moved = with_dhis2(ComponentFlags {
        dhis2_port: Some(ComponentPortArg(Some(18080))),
        dhis2_seed: Some("none"),
        ..ComponentFlags::default()
    })
    .unwrap();
    assert_eq!(moved.dhis2.port, Some(18080));
    assert_eq!(moved.dhis2.seed, Dhis2Seed::None);
    assert_eq!(moved.dhis2_seed_source(), None);

    // The reverse-proxy shape, and a dump of the operator's own.
    let internal = with_dhis2(ComponentFlags {
        dhis2_port: Some(ComponentPortArg(None)),
        dhis2_seed: Some("dumps/laos.sql.gz"),
        ..ComponentFlags::default()
    })
    .unwrap();
    assert_eq!(internal.dhis2.port, None);
    assert_eq!(internal.dhis2_reach(), "internal");
    assert_eq!(internal.dhis2_seed_source(), Some("dumps/laos.sql.gz"));

    // A URL under --offline asks for the opposite of what --offline asks
    // for, and the refusal names both ways out on the line.
    let err = parse_components(&ComponentFlags {
        with: Some("dhis2"),
        dhis2_seed: Some("https://databases.dhis2.org/climate/laos/2.42/laos.sql.gz"),
        offline: true,
        ..ComponentFlags::default()
    })
    .expect_err("--offline and a download");
    assert!(err.to_string().contains("--offline"), "{err}");
    assert!(err.to_string().contains("`--dhis2-seed none`"), "{err}");
    assert!(err.to_string().contains("a path to a dump"), "{err}");

    // A path is fine offline: nothing is downloaded.
    let offline_path = parse_components(&ComponentFlags {
        with: Some("dhis2"),
        dhis2_seed: Some("dumps/laos.sql.gz"),
        offline: true,
        ..ComponentFlags::default()
    })
    .unwrap();
    assert_eq!(
        offline_path.dhis2.seed,
        Dhis2Seed::From("dumps/laos.sql.gz".to_string())
    );
    // And so is the built-in default: the table is compiled in, so nothing
    // has to be looked up to record it.
    assert!(
        parse_components(&ComponentFlags {
            with: Some("dhis2"),
            offline: true,
            ..ComponentFlags::default()
        })
        .is_ok()
    );
}

/// The new ports have to reach the warning `init` already prints, which they
/// do by being recorded on the component: `wanted_claims` reads the set, not
/// the flags.
#[test]
fn a_moved_component_port_is_the_one_that_gets_probed() {
    let components = parse_components(&ComponentFlags {
        with: Some("ocs,s3,dhis2"),
        ocs_port: Some(ComponentPortArg(Some(18010))),
        s3_port: Some(ComponentPortArg(Some(18011))),
        dhis2_port: Some(ComponentPortArg(Some(18012))),
        ..ComponentFlags::default()
    })
    .unwrap();

    let ports: Vec<(String, u16)> = wanted_claims(&components, 18000)
        .into_iter()
        .map(|claim| (claim.service, claim.port))
        .collect();
    assert_eq!(
        ports,
        vec![
            (API_SERVICE.to_string(), 18000),
            ("ocs".to_string(), 18010),
            ("s3".to_string(), 18011),
            ("dhis2".to_string(), 18012),
        ]
    );

    let warned = warn_about_ports(&components, 18000, &|port| port == 18010, &[], &[]);
    assert_eq!(warned.busy.len(), 1);
    assert!(warned.lines[0].contains("(needed by ocs)"), "{warned:?}");

    // `--ocs-port none` publishes nothing, so there is nothing to probe.
    let internal = parse_components(&ComponentFlags {
        with: Some("ocs"),
        ocs_port: Some(ComponentPortArg(None)),
        ..ComponentFlags::default()
    })
    .unwrap();
    assert_eq!(
        wanted_claims(&internal, 18000).len(),
        1,
        "the API port only"
    );
}

/// A `--force` re-init resets the component set from the flags, the way it
/// resets the model set from `--models`. The default for everything but
/// chap-core is off, so the components go without the run mentioning them -
/// which is exactly why each one has to be said out loud.
#[test]
fn a_re_init_says_which_components_it_is_taking_away() {
    let mut previous = Components::default();
    previous.set_enabled(Component::Ocs, true);
    previous.set_enabled(Component::S3, true);

    let both = dropped_components(&previous, &Components::default(), &[]);
    assert_eq!(both, vec![Component::Ocs, Component::S3]);

    // One the operator asked to leave out needs no warning: they typed it.
    assert_eq!(
        dropped_components(&previous, &Components::default(), &[Component::Ocs]),
        vec![Component::S3]
    );
    // And nothing is dropped when the flags bring them back.
    assert!(dropped_components(&previous, &previous, &[]).is_empty());

    // chap-core has no compose file of its own to take away, and can only go
    // off because `--without chap-core` said so.
    let mut core_off = previous.clone();
    core_off.set_enabled(Component::ChapCore, false);
    assert_eq!(dropped_components(&previous, &core_off, &[]), Vec::new());

    let line = component_dropped(Component::Ocs);
    assert!(line.contains("compose.ocs.yml is removed"), "{line}");
    assert!(line.contains("`--with ocs`"), "{line}");
    assert!(!line.contains('\n'), "one line: {line}");
}

/// The compose file of a component that is not coming back has to go with
/// it. Nothing else would ever remove it: the new `project.yaml` lists it in
/// neither `compose_files` nor `rendered_files`, so `chap-devops sync` does not see
/// it as its own any more.
#[test]
fn the_compose_file_of_a_dropped_component_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let mut previous = Components::default();
    previous.set_enabled(Component::Ocs, true);
    previous.set_enabled(Component::S3, true);
    let write = |file: &str| {
        std::fs::write(dir.path().join(file), "services: {}\n").unwrap();
        dir.path().join(file)
    };
    let ocs = write(crate::components::OCS_COMPOSE);
    let s3 = write(crate::components::S3_COMPOSE);

    // The store stays, so only OCS's file goes.
    let mut kept = Components::default();
    kept.set_enabled(Component::S3, true);
    assert_eq!(
        remove_dropped_component_files(dir.path(), &previous, &kept),
        vec![ocs.clone()]
    );
    assert!(!ocs.is_file() && s3.is_file());

    // A file that is already gone is not reported twice, and one the new set
    // keeps is left for the sync to re-render.
    assert!(
        remove_dropped_component_files(dir.path(), &previous, &kept).is_empty(),
        "nothing left to remove"
    );
    assert_eq!(
        remove_dropped_component_files(dir.path(), &previous, &Components::default()),
        vec![s3.clone()]
    );
    assert!(!s3.is_file());
}

/// Which of the previous project's models the new selection brings back, by
/// either of the two names one can be asked for. It decides both which
/// overlays are reported as removed and which containers are stopped, so the
/// two can never disagree.
#[test]
fn a_model_is_kept_by_either_of_its_two_names() {
    let model = crate::project::EnabledModel {
        previous: None,
        service_id: "chapkit-ewars-model".to_string(),
        ..model_record()
    };
    let selection = |id: &str| Selection {
        enable: vec![EnableRequest::new(id)],
        ..Selection::default()
    };
    assert!(kept_by(
        &selection("chapkit_ewars_model"),
        "chapkit_ewars_model",
        &model
    ));
    assert!(kept_by(
        &selection("chapkit-ewars-model"),
        "chapkit_ewars_model",
        &model
    ));
    assert!(!kept_by(
        &selection("auto-arima-chapkit"),
        "chapkit_ewars_model",
        &model
    ));
    assert!(!kept_by(
        &Selection::default(),
        "chapkit_ewars_model",
        &model
    ));
}

/// An enabled-model record with nothing in it that these tests care about.
fn model_record() -> crate::project::EnabledModel {
    crate::project::EnabledModel {
        previous: None,
        reads_port: false,
        service_id: "x".to_string(),
        image: "ghcr.io/chap-models/x".into(),
        image_tag: "sha-1111111".into(),
        version: "1.0.0".into(),
        channel: None,
        host_port: None,
        bind: None,
        data_dir: "/app/data".into(),
        user: "chapkit:chapkit".into(),
        user_from: Default::default(),
        platform: None,
        compose_file: "compose.x.yml".into(),
    }
}

#[test]
fn a_component_port_is_probed_alongside_the_api_port() {
    let mut components = Components::default();
    components.ocs.enabled = true;
    components.ocs.port = Some(9000);

    assert!(
        warn_about_ports(&components, 8000, &|_| false, &[], &[])
            .busy
            .is_empty()
    );
    let busy = warn_about_ports(&components, 8000, &|port| port == 9000, &[], &[]).busy;
    assert_eq!(busy.len(), 1);
    assert_eq!(busy[0].service, "ocs");
    assert_eq!(busy[0].port, 9000);

    // With chap-core off the API port is not one of this deployment's.
    components.chap_core.enabled = false;
    assert!(
        warn_about_ports(&components, 8000, &|port| port == 8000, &[], &[])
            .busy
            .is_empty(),
        "no chap-core, no API port"
    );
}

#[test]
fn resolve_dir_drops_cur_dir_components() {
    let cwd = std::env::current_dir().unwrap();
    assert_eq!(resolve_dir(Path::new(".")).unwrap(), cwd);
    assert_eq!(
        resolve_dir(Path::new("./chapx")).unwrap(),
        cwd.join("chapx")
    );
    assert_eq!(
        resolve_dir(Path::new("./a/./b")).unwrap(),
        cwd.join("a").join("b")
    );
    // An absolute path is taken as it stands. It is spelled with the
    // platform's own separators and prefix, which is what `current_dir`
    // hands back, so the test says nothing about `/tmp` on a machine
    // where that is not a path at all.
    let absolute = cwd.join("chapx");
    assert!(absolute.is_absolute());
    assert_eq!(resolve_dir(&absolute).unwrap(), absolute);
}

#[test]
fn an_existing_env_is_kept_whatever_force_says() {
    // --force is not an input at all: the only thing that replaces a file
    // that is already there is --fresh-env.
    assert_eq!(env_action(true, false, false), EnvAction::Kept);
    assert_eq!(env_action(true, true, false), EnvAction::Written);
}

#[test]
fn a_missing_env_is_written() {
    assert_eq!(env_action(false, false, false), EnvAction::Written);
    assert_eq!(env_action(false, true, false), EnvAction::Written);
}

#[test]
fn no_env_skips_the_file_either_way() {
    assert_eq!(env_action(false, false, true), EnvAction::Skipped);
    assert_eq!(env_action(true, false, true), EnvAction::Skipped);
}

#[test]
fn the_env_action_serializes_lowercase() {
    let json = |a: EnvAction| serde_json::to_string(&a).unwrap();
    assert_eq!(json(EnvAction::Written), "\"written\"");
    assert_eq!(json(EnvAction::Kept), "\"kept\"");
    assert_eq!(json(EnvAction::Skipped), "\"skipped\"");
}

/// A [`Ctx`] that never touches the network.
fn offline_ctx() -> Ctx {
    Ctx {
        out: crate::output::Out::default(),
        project_dir: PathBuf::from("."),
        registry: crate::registry::RegistryOptions {
            offline: true,
            ..crate::registry::RegistryOptions::default()
        },
        cli_version: "0.1.0",
    }
}

#[test]
fn offline_keeps_the_tag_as_given_and_the_embedded_compose_file() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = offline_ctx();

    let latest = resolve_chap_core(&ctx, dir.path(), "latest");
    assert_eq!(latest.tag, "latest");
    assert_eq!(latest.source, ComposeSource::Embedded);
    assert!(latest.cached.is_none());

    // An explicit release tag is used verbatim; only the compose file
    // falls back, because downloading it needs the network.
    let pinned = resolve_chap_core(&ctx, dir.path(), "v2.3.1");
    assert_eq!(pinned.tag, "v2.3.1");
    assert_eq!(pinned.source, ComposeSource::Embedded);
    assert!(pinned.cached.is_none());
}

#[test]
fn a_cached_compose_file_is_reused_instead_of_downloaded() {
    let dir = tempfile::tempdir().unwrap();
    let chap_devops = dir.path().join(CHAP_DEVOPS_DIR);
    std::fs::create_dir_all(&chap_devops).unwrap();
    let body = "services:\n  chap:\n    image: ghcr.io/x:${CHAP_IMAGE_TAG:-latest}\n";
    std::fs::write(chap_devops.join(cached_compose_file("v2.3.1")), body).unwrap();

    // Offline, and still a fetched source: the copy is right there.
    let found = resolve_chap_core(&offline_ctx(), dir.path(), "v2.3.1");
    assert_eq!(found.tag, "v2.3.1");
    assert!(
        found.cached.is_none(),
        "nothing to write, it is already there"
    );
    assert_eq!(
        found.source,
        ComposeSource::Fetched {
            url: chapcore::compose_url("v2.3.1"),
            tag: "v2.3.1".to_string(),
            sha256: chapcore::sha256_hex(body.as_bytes()),
        }
    );

    // A cached file that is not a usable compose document is ignored.
    std::fs::write(
        chap_devops.join(cached_compose_file("v2.2.0")),
        "nonsense: [",
    )
    .unwrap();
    let ignored = resolve_chap_core(&offline_ctx(), dir.path(), "v2.2.0");
    assert_eq!(ignored.source, ComposeSource::Embedded);
}

#[test]
fn an_active_api_port_line_is_read_out_of_an_env_file() {
    let body = "POSTGRES_DB=chap_core\nCHAP_API_PORT=8123\n# CHAP_IMAGE_TAG=latest\n";
    assert_eq!(env_api_port(body), Some(8123));
    // A commented placeholder is not a setting, and neither is nonsense.
    assert_eq!(env_api_port("# CHAP_API_PORT=8123\n"), None);
    assert_eq!(env_api_port("CHAP_API_PORT=\nCHAP_API_PORT=nope\n"), None);
    assert_eq!(env_api_port("POSTGRES_DB=chap_core\n"), None);
    // The first usable value wins, the way compose reads the file.
    assert_eq!(
        env_api_port("CHAP_API_PORT=8010\nCHAP_API_PORT=8020\n"),
        Some(8010)
    );
}

#[test]
fn a_free_api_port_warns_about_nothing() {
    let quiet = warn_about_ports(&Components::default(), 8000, &|_| false, &[], &[]);
    assert!(quiet.busy.is_empty() && quiet.claimed.is_empty() && quiet.lines.is_empty());
}

#[test]
fn a_busy_api_port_is_a_warning_not_a_refusal() {
    // 8000 and 8001 are taken, 8002 is not: init still writes the
    // directory and says which port to use instead.
    let warned = warn_about_ports(
        &Components::default(),
        8000,
        &|port| (8000..=8001).contains(&port),
        &[],
        &[],
    );
    assert_eq!(warned.busy.len(), 1);
    assert_eq!(warned.lines.len(), 1);
    assert!(
        warned.lines[0].contains("CHAP_API_PORT=8002"),
        "{:?}",
        warned.lines
    );
    // Even with nothing free above it, the warning goes out.
    let warned = warn_about_ports(&Components::default(), 8000, &|_| true, &[], &[]);
    assert_eq!(warned.busy.len(), 1);
    assert!(
        warned.lines[0].contains("CHAP_API_PORT=<free>"),
        "{:?}",
        warned.lines
    );
}

#[test]
fn without_the_flag_there_are_no_secrets_at_all() {
    assert!(resolve_secrets(None).unwrap().is_none());
}

#[test]
fn a_bare_api_token_flag_generates_both_secrets() {
    let secrets = resolve_secrets(Some(&None)).unwrap().expect("both secrets");
    assert_eq!(secrets.api_token.len(), 64);
    assert_eq!(secrets.registration_key.len(), 64);
    assert_ne!(
        secrets.api_token, secrets.registration_key,
        "two independent secrets"
    );
    for secret in [&secrets.api_token, &secrets.registration_key] {
        assert!(
            secret
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
            "{secret}"
        );
    }
}

#[test]
fn an_explicit_api_token_is_used_verbatim_and_still_gets_a_key() {
    let given = "mysecrettoken-that-is-long-enough-32ch";
    let secrets = resolve_secrets(Some(&Some(given.to_string())))
        .unwrap()
        .expect("both secrets");
    assert_eq!(secrets.api_token, given);
    assert_eq!(secrets.registration_key.len(), 64);

    // Surrounding whitespace is the shell's, not part of the secret.
    let secrets = resolve_secrets(Some(&Some(format!("  {given}  "))))
        .unwrap()
        .unwrap();
    assert_eq!(secrets.api_token, given);

    // A short token is accepted (chap-core does too) and warned about.
    let secrets = resolve_secrets(Some(&Some("short".to_string())))
        .unwrap()
        .unwrap();
    assert_eq!(secrets.api_token, "short");

    // An empty value cannot be a token, so one is generated instead of
    // silently leaving the API open.
    let secrets = resolve_secrets(Some(&Some("   ".to_string())))
        .unwrap()
        .unwrap();
    assert_eq!(secrets.api_token.len(), 64);
}

#[test]
fn the_recorded_auth_state_follows_the_env_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(ENV_FILE);

    // --no-env: no file, and nothing is protected.
    assert_eq!(env_auth(EnvAction::Skipped, &path), AuthState::default());
    // A file that is not there yet reads the same way.
    assert_eq!(env_auth(EnvAction::Written, &path), AuthState::default());

    std::fs::write(&path, "# CHAP_API_TOKEN=\nPOSTGRES_DB=chap_core\n").unwrap();
    assert_eq!(env_auth(EnvAction::Written, &path), AuthState::default());

    std::fs::write(&path, "CHAP_API_TOKEN=t\nSERVICEKIT_REGISTRATION_KEY=k\n").unwrap();
    assert_eq!(
        env_auth(EnvAction::Kept, &path),
        AuthState {
            api_token: true,
            registration_key: true
        },
        "a kept .env keeps the deployment authenticated"
    );
    // ...but --no-env never looks at a file it was told not to write.
    assert_eq!(env_auth(EnvAction::Skipped, &path), AuthState::default());
}

#[test]
fn the_generated_password_is_url_safe_hex() {
    let password = random_password().unwrap();
    assert_eq!(password.len(), 32);
    assert!(
        password
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    );
    assert_ne!(password, random_password().unwrap());
}

#[test]
fn a_seed_password_needs_a_seed_and_a_value() {
    let with_dhis2 = |flags: ComponentFlags| {
        parse_components(&ComponentFlags {
            with: Some("dhis2"),
            ..flags
        })
    };
    let set = with_dhis2(ComponentFlags {
        dhis2_seed_password: Some("district"),
        ..ComponentFlags::default()
    })
    .unwrap();
    assert_eq!(set.dhis2.seed_password.as_deref(), Some("district"));
    assert_eq!(
        with_dhis2(ComponentFlags::default())
            .unwrap()
            .dhis2
            .seed_password,
        None
    );

    let err = with_dhis2(ComponentFlags {
        dhis2_seed: Some("none"),
        dhis2_seed_password: Some("district"),
        ..ComponentFlags::default()
    })
    .unwrap_err();
    assert!(
        err.to_string().contains("`--dhis2-seed none` has no users"),
        "{err}"
    );
    let err = with_dhis2(ComponentFlags {
        dhis2_seed_password: Some(""),
        ..ComponentFlags::default()
    })
    .unwrap_err();
    assert!(err.to_string().contains("needs a password"), "{err}");
}

#[test]
fn a_kept_env_moves_only_the_image_pin_chap_devops_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join(ENV_FILE);
    let read = || std::fs::read_to_string(&env).unwrap();

    // The tag the previous deployment recorded moves to the new one.
    std::fs::write(&env, "POSTGRES_PASSWORD=x\nCHAP_IMAGE_TAG=v2.3.0\n").unwrap();
    assert_eq!(
        move_kept_tag(dir.path(), Some("v2.3.0"), "v2.4.0").unwrap(),
        KeptTag::Moved {
            from: "v2.3.0".into(),
            to: "v2.4.0".into()
        }
    );
    assert_eq!(read(), "POSTGRES_PASSWORD=x\nCHAP_IMAGE_TAG=v2.4.0\n");
    assert_eq!(
        move_kept_tag(dir.path(), Some("v2.3.0"), "v2.4.0").unwrap(),
        KeptTag::Same
    );

    // `checkout` is only ever written by chap-devops, so it moves without a
    // previous deployment to vouch for it.
    std::fs::write(&env, "CHAP_IMAGE_TAG=checkout\n").unwrap();
    assert!(matches!(
        move_kept_tag(dir.path(), None, "latest").unwrap(),
        KeptTag::Moved { .. }
    ));
    assert_eq!(read(), "CHAP_IMAGE_TAG=latest\n");

    // A value of the operator's own is left alone, with the way out.
    std::fs::write(&env, "CHAP_IMAGE_TAG=v1.0.0\n").unwrap();
    let KeptTag::Warning(text) = move_kept_tag(dir.path(), Some("v2.3.0"), "v2.4.0").unwrap()
    else {
        panic!("a warning");
    };
    assert!(text.contains("CHAP_IMAGE_TAG=v1.0.0"), "{text}");
    assert!(text.contains("`CHAP_IMAGE_TAG=v2.4.0`"), "{text}");
    assert_eq!(read(), "CHAP_IMAGE_TAG=v1.0.0\n");
    // A checkout build names its own images, so it does not read the line.
    assert_eq!(
        move_kept_tag(dir.path(), Some("v2.3.0"), "checkout").unwrap(),
        KeptTag::Same
    );

    // The commented placeholder means `latest`, which a release pin is not.
    std::fs::write(&env, "# CHAP_IMAGE_TAG=latest\n").unwrap();
    assert!(matches!(
        move_kept_tag(dir.path(), Some("latest"), "v2.4.0").unwrap(),
        KeptTag::Warning(_)
    ));
    assert_eq!(
        move_kept_tag(dir.path(), Some("v2.4.0"), "latest").unwrap(),
        KeptTag::Same
    );
}

/// `init --force` over a deployment that is up: the port its own chap
/// container publishes is not in the way, and a port it does not hold is.
#[test]
fn a_port_this_deployment_already_holds_is_not_warned_about() {
    let own = vec![crate::ports::PortClaim {
        service: API_SERVICE.to_string(),
        port: 8000,
    }];
    let quiet = warn_about_ports(&Components::default(), 8000, &|_| true, &[], &own);
    assert!(quiet.lines.is_empty(), "{:?}", quiet.lines);
    assert!(quiet.busy.is_empty());

    // Moved to another port, which something else is listening on.
    let warned = warn_about_ports(&Components::default(), 8100, &|_| true, &[], &own);
    assert_eq!(warned.busy.len(), 1, "{:?}", warned.lines);
}
