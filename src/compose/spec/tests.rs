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
