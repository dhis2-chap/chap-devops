use super::*;
use crate::registry::embedded;

fn parse_all() -> Vec<Model> {
    embedded::files()
        .iter()
        .filter(|(name, _)| *name != "registry.yaml")
        .map(|(name, body)| {
            serde_yaml_ng::from_str::<Model>(body)
                .unwrap_or_else(|e| panic!("{name} failed to parse: {e}"))
        })
        .collect()
}

fn model(id: &str) -> Model {
    parse_all()
        .into_iter()
        .find(|m| m.id == id)
        .unwrap_or_else(|| panic!("no vendored model {id}"))
}

#[test]
fn vendored_index_parses() {
    let (name, body) = embedded::files()[0];
    assert_eq!(name, "registry.yaml");
    let index: RegistryIndex = serde_yaml_ng::from_str(body).expect("registry.yaml parses");
    assert_eq!(index.schema_version, 2);
    assert_eq!(index.models.len(), 7);
    assert!(index.models.iter().all(|p| p.starts_with("models/")));
    assert!(!index.marketplace.documentation.is_empty());
    assert_eq!(index.review_policy.required_approvals, 3);
}

/// The order every listing of the catalogue is in, pinned here because
/// the browser, `models list` and `models search` all read it from one
/// place.
#[test]
fn the_catalogue_is_ordered_by_maturity_then_by_name() {
    let ranks: Vec<u8> = [
        AssessedStatus::Green,
        AssessedStatus::Yellow,
        AssessedStatus::Orange,
        AssessedStatus::Red,
        AssessedStatus::Gray,
    ]
    .iter()
    .map(AssessedStatus::rank)
    .collect();
    assert_eq!(ranks, vec![0, 1, 2, 3, 4], "most mature first");

    let mut models = parse_all();
    models.sort_by_key(Model::order_key);
    let listed: Vec<&str> = models
        .iter()
        .filter(|m| !m.is_starter())
        .map(|m| m.display_name.as_str())
        .collect();
    assert_eq!(
        listed,
        vec![
            "CHAP-EWARS",
            "Simple Multistep",
            "Auto-ARIMA",
            "GHRmodel",
            "Rwanda Malaria BYM",
        ]
    );
    // Scaffolding is not a forecasting model, so it comes after every
    // one of them however its own author assessed it.
    assert!(
        models.iter().rev().take(2).all(Model::is_starter),
        "the starters are last"
    );
}

#[test]
fn every_vendored_model_parses() {
    let models = parse_all();
    assert_eq!(models.len(), 7);
    for m in &models {
        assert_eq!(m.schema_version, 2);
        assert!(!m.versions.is_empty(), "{} has no versions", m.id);
        assert!(!m.configurations.is_empty(), "{} has no configs", m.id);
        // Every channel pointer must resolve.
        assert!(m.version(&m.channels.stable).is_some(), "{}", m.id);
        assert!(m.version(&m.channels.latest).is_some(), "{}", m.id);
    }
    assert_eq!(models.iter().filter(|m| m.is_starter()).count(), 2);
}

#[test]
fn needs_amd64_tracks_the_r_inla_runtime() {
    for id in [
        "chapkit_ewars_model",
        "chapkit_rwanda_malaria_bym_model",
        "chapkit_ghr_model",
        "chapkit_minimalist_example_r",
    ] {
        assert!(model(id).needs_amd64(), "{id} should need amd64");
    }
    for id in [
        "auto_arima_chapkit",
        "chapkit_simple_multistep_model",
        "chapkit_minimalist_example_py",
    ] {
        assert!(!model(id).needs_amd64(), "{id} should not need amd64");
    }
}

#[test]
fn runtime_base_strips_only_a_real_tag() {
    assert_eq!(runtime_base(R_INLA_RUNTIME), R_INLA_RUNTIME);
    assert_eq!(
        runtime_base("ghcr.io/dhis2-chap/chapkit-r-inla:2.0.0"),
        R_INLA_RUNTIME
    );
    assert_eq!(runtime_base("localhost:5000/img"), "localhost:5000/img");
}

#[test]
fn resolve_stable_yields_a_verified_version() {
    let m = model("chapkit_ewars_model");
    let v = m
        .resolve(&VersionSelector::Channel(Channel::Stable))
        .unwrap();
    assert_eq!(v.version, m.channels.stable);
    assert_eq!(v.status, VersionStatus::Verified);
    assert_eq!(
        m.image_ref(v),
        format!("ghcr.io/chap-models/chapkit_ewars_model:{}", v.image_tag)
    );
}

#[test]
fn resolve_exact_and_unknown() {
    let m = model("auto_arima_chapkit");
    // The pin the snapshot carries, asked for by its exact number rather
    // than through a channel.
    let pinned = m.channels.stable.clone();
    let v = m
        .resolve(&VersionSelector::Exact(pinned.clone()))
        .expect("the pinned version exists");
    assert_eq!(v.version, pinned);
    assert_eq!(v.image_tag, m.version(&pinned).unwrap().image_tag);

    let err = m
        .resolve(&VersionSelector::Exact("9.9.9".into()))
        .expect_err("unknown version errors");
    match err.downcast_ref::<crate::error::ChapError>() {
        Some(crate::error::ChapError::UnknownVersion { id, version }) => {
            assert_eq!(id, "auto_arima_chapkit");
            assert_eq!(version, "9.9.9");
        }
        other => panic!("wrong error: {other:?}"),
    }
}

#[test]
fn resolve_rejects_a_yanked_version() {
    let mut m = model("auto_arima_chapkit");
    m.versions[0].status = VersionStatus::Yanked;
    let err = m
        .resolve(&VersionSelector::Channel(Channel::Stable))
        .expect_err("yanked version errors");
    assert!(matches!(
        err.downcast_ref::<crate::error::ChapError>(),
        Some(crate::error::ChapError::YankedVersion { .. })
    ));
}

/// The stable channel takes a verified version only, as `chap-admin install`
/// does; the version itself, named exactly, is still there to take.
#[test]
fn the_stable_channel_refuses_a_version_that_is_not_verified() {
    let mut m = model("auto_arima_chapkit");
    let stable = m.channels.stable.clone();
    let at = m
        .versions
        .iter()
        .position(|v| v.version == stable)
        .expect("the stable version is listed");
    m.versions[at].status = VersionStatus::Unstable;
    let err = m
        .resolve(&VersionSelector::Channel(Channel::Stable))
        .expect_err("an unstable stable version errors");
    assert_eq!(
        err.to_string(),
        format!(
            "the stable version {stable} of `auto_arima_chapkit` is unstable, not verified; \
             pass `--version {stable}` to enable it anyway"
        )
    );
    let exact = m
        .resolve(&VersionSelector::Exact(stable.clone()))
        .expect("an exact version is taken as it is");
    assert_eq!(exact.version, stable);

    // A model added by hand has no review, so its channel is not checked.
    m.manual = true;
    m.resolve(&VersionSelector::Channel(Channel::Stable))
        .expect("a manual model resolves");
}

#[test]
fn configurations_keep_their_json_shape() {
    let m = model("chapkit_ewars_model");
    let cfg = &m.configurations["monthly_climate"].config;
    assert_eq!(cfg["prediction_periods"], serde_json::json!(3));
    assert_eq!(cfg["n_lags"], serde_json::json!([3, 3]));
    assert_eq!(cfg["region_seasonal"], serde_json::json!(false));
}

/// A marketplace version runs only from a `sha-` tag that starts its commit,
/// as `chap-admin` requires; a model the deployment defines itself is not
/// held to it.
#[test]
fn resolve_refuses_an_image_tag_that_is_not_the_commit() {
    let mut m = model("auto_arima_chapkit");
    let stable = m.channels.stable.clone();
    let at = m
        .versions
        .iter()
        .position(|v| v.version == stable)
        .expect("the stable version is listed");
    let commit = m.versions[at].commit.clone();
    assert!(m.versions[at].is_pinned());

    for tag in [
        "latest".to_string(),
        format!("sha-{}", &commit[..6]),
        "sha-0000000".to_string(),
    ] {
        m.versions[at].image_tag = tag.clone();
        let err = m
            .resolve(&VersionSelector::Channel(Channel::Stable))
            .expect_err("an unpinned tag errors");
        assert!(
            matches!(
                err.downcast_ref::<crate::error::ChapError>(),
                Some(crate::error::ChapError::UnpinnedImage { image_tag, .. }) if *image_tag == tag
            ),
            "{err}"
        );
    }

    m.manual = true;
    assert!(
        m.resolve(&VersionSelector::Channel(Channel::Stable))
            .is_ok()
    );
}

/// The `id` is the name of the file, and the `service_id` is the `id` with
/// `-` for `_`.
#[test]
fn the_identity_of_an_entry_matches_its_file() {
    let mut m = model("auto_arima_chapkit");
    assert!(m.identity_matches("models/auto_arima_chapkit.yaml"));
    assert!(!m.identity_matches("models/other_model.yaml"));
    m.service_id = "auto_arima_chapkit".to_string();
    assert!(!m.identity_matches("models/auto_arima_chapkit.yaml"));
}

/// The digest the marketplace lists goes into the image reference, and one
/// that is not `sha256:<64 hex>` stops the version before it is pinned.
#[test]
fn resolve_carries_the_digest_and_refuses_a_malformed_one() {
    let mut m = model("auto_arima_chapkit");
    let stable = m.channels.stable.clone();
    let at = m
        .versions
        .iter()
        .position(|v| v.version == stable)
        .expect("the stable version is listed");
    let digest = format!("sha256:{}", "c".repeat(64));
    m.versions[at].image_digest = Some(digest.clone());
    let v = m
        .resolve(&VersionSelector::Channel(Channel::Stable))
        .expect("a well-formed digest resolves");
    assert_eq!(
        m.image_ref(v),
        format!("{}:{}@{digest}", m.source.image, v.image_tag)
    );

    m.versions[at].image_digest = Some("sha256:nope".to_string());
    let err = m
        .resolve(&VersionSelector::Channel(Channel::Stable))
        .expect_err("a malformed digest errors");
    assert!(
        matches!(
            err.downcast_ref::<crate::error::ChapError>(),
            Some(crate::error::ChapError::InvalidDigest { digest, .. }) if digest == "sha256:nope"
        ),
        "{err}"
    );
}

/// A marketplace file without digests still parses: the field is optional.
#[test]
fn a_version_without_a_digest_parses() {
    let v: Version = serde_yaml_ng::from_str(
        "version: 1.0.0\ncommit: 1111111111111111111111111111111111111111\n\
         image_tag: sha-1111111\nchapkit: \">=2\"\nstatus: verified\n",
    )
    .unwrap();
    assert_eq!(v.image_digest, None);
    assert_eq!(v.pinned_tag(), "sha-1111111");
}
