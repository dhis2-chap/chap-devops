use super::*;
use crate::registry::{Channel, Registry, VersionSelector, load_embedded};

fn registry() -> Registry {
    load_embedded().unwrap()
}

fn spec_for(r: &Registry, id: &str, data_dir: Option<&str>, user: Option<&str>) -> OverlaySpec {
    let m = r.get(id).unwrap();
    let v = m
        .resolve(&VersionSelector::Channel(Channel::Stable))
        .unwrap();
    OverlaySpec::from_model(m, v, Some(5001), data_dir, user)
}

#[test]
fn from_model_copies_the_marketplace_entry() {
    let r = registry();
    let spec = spec_for(&r, "chapkit_ewars_model", None, None);
    // The pin comes from the snapshot, so a refresh moves it here too.
    let stable = r
        .get("chapkit_ewars_model")
        .unwrap()
        .resolve(&VersionSelector::Channel(Channel::Stable))
        .unwrap();
    assert_eq!(spec.id, "chapkit_ewars_model");
    assert_eq!(spec.service_id, "chapkit-ewars-model");
    assert_eq!(spec.version, stable.version);
    assert_eq!(spec.image, "ghcr.io/chap-models/chapkit_ewars_model");
    assert_eq!(spec.image_tag, stable.image_tag);
    assert_eq!(spec.tag_env_var, "CHAPKIT_EWARS_MODEL_IMAGE_TAG");
    assert_eq!(spec.volume_name, "ck_chapkit_ewars_model_data");
    assert_eq!(spec.host_port, Some(5001));
    assert!(!spec.registration_key);
}

#[test]
fn from_model_pins_every_overlay_to_amd64() {
    let r = registry();
    for id in ["chapkit_ewars_model", "chapkit_simple_multistep_model"] {
        assert_eq!(
            spec_for(&r, id, None, None).platform.as_deref(),
            Some("linux/amd64"),
            "{id}"
        );
    }
}

#[test]
fn data_dir_and_user_prefer_the_explicit_value_then_the_table() {
    let r = registry();
    // The table: ewars writes to /app/data as the account its image
    // declares.
    let table = spec_for(&r, "chapkit_ewars_model", None, None);
    assert_eq!(table.data_dir, "/app/data");
    assert_eq!(table.user, "chapkit");

    // And Auto-ARIMA runs as root, which is what the overlay has to
    // leave alone.
    let root = spec_for(&r, "auto_arima_chapkit", None, None);
    assert_eq!(root.data_dir, "/work/data");
    assert_eq!(root.user, "root");

    // The flags win over the table.
    let explicit = spec_for(
        &r,
        "chapkit_ewars_model",
        Some("/srv/data"),
        Some("1000:1000"),
    );
    assert_eq!(explicit.data_dir, "/srv/data");
    assert_eq!(explicit.user, "1000:1000");

    // Nothing known about the image at all: the chapkit defaults. Every
    // marketplace id has a table entry, so this is a model the catalogue
    // grew after this binary was built.
    let unlisted = r.get("chapkit_ewars_model").unwrap();
    let v = unlisted
        .resolve(&VersionSelector::Channel(Channel::Stable))
        .unwrap();
    let mut unknown = unlisted.clone();
    unknown.id = "grown_since_this_build".to_string();
    let fallback = OverlaySpec::from_model(&unknown, v, None, None, None);
    assert_eq!(fallback.data_dir, overrides::DEFAULT_DATA_DIR);
    assert_eq!(fallback.user, overrides::DEFAULT_USER);
}

#[test]
fn from_enabled_replays_the_recorded_state() {
    let r = registry();
    let m = r.get("chapkit_ewars_model").unwrap();
    let enabled = EnabledModel {
        previous: None,
        reads_port: false,
        service_id: "chapkit-ewars-model".into(),
        image: "ghcr.io/chap-models/chapkit_ewars_model".into(),
        image_tag: "sha-0000000".into(),
        image_digest: None,
        version: "0.9.0".into(),
        channel: None,
        host_port: Some(5007),
        bind: None,
        data_dir: "/srv/data".into(),
        user: "1000:1000".into(),
        user_from: Default::default(),
        platform: None,
        multi_arch: false,
        compose_file: "compose.chapkit-ewars-model.yml".into(),
    };
    let spec = OverlaySpec::from_enabled("chapkit_ewars_model", &enabled, m);
    assert_eq!(spec.host_port, Some(5007));

    // A model that publishes nothing replays as such.
    let internal = EnabledModel {
        previous: None,
        host_port: None,
        ..enabled.clone()
    };
    let spec = OverlaySpec::from_enabled("chapkit_ewars_model", &internal, m);
    assert_eq!(spec.host_port, None);
    let spec = OverlaySpec::from_enabled_without_registry("gone", &internal);
    assert_eq!(spec.host_port, None);
    let spec = OverlaySpec::from_enabled("chapkit_ewars_model", &enabled, m);
    // Recorded values win; the marketplace only supplies the repository.
    assert_eq!(spec.image_tag, "sha-0000000");
    assert_eq!(spec.version, "0.9.0");
    assert_eq!(spec.host_port, Some(5007));
    assert_eq!(spec.data_dir, "/srv/data");
    assert_eq!(spec.user, "1000:1000");
    assert_eq!(spec.platform, None);
    assert_eq!(spec.repository, m.source.repository);
    assert_eq!(spec.tag_env_var, "CHAPKIT_EWARS_MODEL_IMAGE_TAG");
}

#[test]
fn a_checkout_revision_is_head_and_says_when_files_differ() {
    let dir = tempfile::tempdir().expect("a temp dir");
    // Not a git checkout: no revision.
    assert_eq!(checkout_revision(dir.path()), None);

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success())
    };
    // Without git on this machine there is nothing more to check.
    if !git(&["init", "-q"]) {
        return;
    }
    std::fs::write(dir.path().join("a"), "1").expect("a file");
    assert!(git(&["add", "a"]));
    assert!(git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.org",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "-m",
        "x"
    ]));
    let clean = checkout_revision(dir.path()).expect("a revision");
    assert_eq!(clean.len(), 40, "{clean}");
    std::fs::write(dir.path().join("a"), "2").expect("a change");
    assert_eq!(
        checkout_revision(dir.path()),
        Some(format!("{clean}-dirty"))
    );
}

/// Every model the embedded marketplace ships passes the overlay check, and
/// a newline or a quote in any value it renders is refused with the model
/// and the field named.
#[test]
fn an_overlay_value_that_could_add_yaml_keys_is_refused() {
    use crate::compose::apply::check_overlay;
    let r = registry();
    for m in &r.models {
        let Ok(v) = m.resolve(&VersionSelector::Channel(Channel::Stable)) else {
            continue;
        };
        let spec = OverlaySpec::from_model(m, v, None, None, None);
        check_overlay(&spec).unwrap_or_else(|e| panic!("{}: {e:#}", m.id));
    }

    let good = spec_for(&r, "chapkit_ewars_model", None, None);
    let inject = "x\n    privileged: true";
    type Set = fn(&mut OverlaySpec, &str);
    let cases: [(&str, Set); 8] = [
        ("version", |s, v| s.version = v.to_string()),
        ("repository", |s, v| s.repository = v.to_string()),
        ("image", |s, v| s.image = v.to_string()),
        ("image tag", |s, v| s.image_tag = v.to_string()),
        ("image digest", |s, v| s.image_digest = Some(v.to_string())),
        ("data directory", |s, v| s.data_dir = format!("/app{v}")),
        ("user", |s, v| s.user = v.to_string()),
        ("service id", |s, v| s.service_id = v.to_string()),
    ];
    for (field, set) in cases {
        let mut spec = good.clone();
        set(&mut spec, inject);
        let err = check_overlay(&spec).expect_err(field).to_string();
        assert!(err.contains(&format!("its {field} ")), "{err}");
        assert!(err.contains("chapkit_ewars_model"), "{err}");
        assert!(err.contains("chap-devops models disable"), "{err}");
    }
    // A tag such as `latest` and a real digest are fine.
    let mut spec = good.clone();
    spec.image_tag = "latest".to_string();
    spec.image_digest = Some(format!("sha256:{}", "a".repeat(64)));
    check_overlay(&spec).unwrap();
    spec.image_tag = format!("@sha256:{}", "b".repeat(64));
    spec.image_digest = None;
    check_overlay(&spec).unwrap();
    spec.image_tag = "@sha256:short".to_string();
    assert!(check_overlay(&spec).is_err());
}

#[test]
fn an_ocs_bbox_is_four_numbers_in_order() {
    assert_eq!(
        parse_bbox("-13.5,6.9, -10.1,10.0").unwrap(),
        "-13.5, 6.9, -10.1, 10.0"
    );
    for (bad, why) in [
        ("1,2,3", "not four numbers"),
        ("a,b,c,d", "not four numbers"),
        ("1,2,3,4,5", "not four numbers"),
        ("10,2,3,4", "not below its maximum"),
        ("1,2,200,4", "outside"),
        ("1,2,NaN,4", "outside"),
    ] {
        let err = parse_bbox(bad).unwrap_err();
        assert!(err.contains(why), "{bad}: {err}");
    }
}

#[test]
fn an_ocs_country_is_three_letters() {
    assert_eq!(parse_country_code("lao").unwrap(), "LAO");
    for bad in ["LA", "LAOS", "L4O", "LA:"] {
        assert!(parse_country_code(bad).is_err(), "{bad}");
    }
}
