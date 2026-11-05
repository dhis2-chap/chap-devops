use super::*;

#[test]
fn a_reference_without_a_registry_host_is_local() {
    assert!(is_local_image("my-model:dev"));
    assert!(is_local_image("me/my-model:dev"));
    assert!(is_local_image("my-model@sha256:abc"));
    assert!(!is_local_image("ghcr.io/chap-models/x:sha-1"));
    assert!(!is_local_image("localhost/x:dev"));
    assert!(!is_local_image("registry:5000/x:dev"));
}

#[test]
fn overlay_filename_uses_the_service_id() {
    assert_eq!(
        overlay_filename("chapkit-ewars-model"),
        "compose.chapkit-ewars-model.yml"
    );
    assert_eq!(
        overlay_filename("auto-arima-chapkit"),
        "compose.auto-arima-chapkit.yml"
    );
}

#[test]
fn tag_env_var_uppercases_and_folds_separators() {
    assert_eq!(
        tag_env_var("chapkit_ewars_model"),
        "CHAPKIT_EWARS_MODEL_IMAGE_TAG"
    );
    assert_eq!(
        tag_env_var("chapkit-ewars-model"),
        "CHAPKIT_EWARS_MODEL_IMAGE_TAG"
    );
    assert_eq!(
        tag_env_var("auto_arima_chapkit"),
        "AUTO_ARIMA_CHAPKIT_IMAGE_TAG"
    );
    assert_eq!(tag_env_var("a.b c"), "A_B_C_IMAGE_TAG");
    // The chap-core deployment test matches ^\$\{[A-Z_]+_IMAGE_TAG:-...\}$.
    assert!(
        tag_env_var("chapkit_minimalist_example_py")
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_')
    );
}

#[test]
fn an_image_reference_joins_a_tag_with_a_colon_and_a_digest_with_nothing() {
    assert_eq!(
        image_ref("ghcr.io/chap-models/m", "sha-b1d6c31"),
        "ghcr.io/chap-models/m:sha-b1d6c31"
    );
    let digest = format!("@sha256:{}", "a".repeat(64));
    assert_eq!(
        image_ref("ghcr.io/chap-models/m", &digest),
        format!("ghcr.io/chap-models/m@sha256:{}", "a".repeat(64))
    );
    assert_eq!(tag_separator("sha-b1d6c31"), ":");
    assert_eq!(tag_separator(&digest), "");
}

#[test]
fn volume_name_is_prefixed_and_suffixed() {
    assert_eq!(
        volume_name("chapkit_ewars_model"),
        "ck_chapkit_ewars_model_data"
    );
    assert_eq!(
        volume_name("auto_arima_chapkit"),
        "ck_auto_arima_chapkit_data"
    );
}

#[test]
fn a_model_volume_name_reads_back_as_the_id_that_made_it() {
    for id in ["chapkit_ewars_model", "auto_arima_chapkit", "a-b"] {
        assert_eq!(volume_model_id(&volume_name(id)), Some(id));
    }
    // Every other volume of a deployment belongs to someone else.
    assert_eq!(volume_model_id("chap-db"), None);
    assert_eq!(volume_model_id("ocs_data"), None);
    assert_eq!(volume_model_id("ck__data"), None);
    assert_eq!(volume_model_id("ck_ewars"), None);
}

#[test]
fn a_digest_is_pinned_after_the_tag() {
    let digest = format!("sha256:{}", "a".repeat(64));
    assert_eq!(
        pinned_tag("sha-b1d6c31", Some(&digest)),
        format!("sha-b1d6c31@{digest}")
    );
    assert_eq!(pinned_tag("sha-b1d6c31", None), "sha-b1d6c31");
    // A tag that is a digest already takes no second one.
    assert_eq!(
        pinned_tag(&format!("@{digest}"), Some(&digest)),
        format!("@{digest}")
    );
    assert_eq!(
        image_ref(
            "ghcr.io/chap-models/m",
            &pinned_tag("sha-b1d6c31", Some(&digest))
        ),
        format!("ghcr.io/chap-models/m:sha-b1d6c31@{digest}")
    );
}

#[test]
fn a_digest_is_sha256_and_64_hex_characters() {
    assert!(is_digest(&format!("sha256:{}", "a".repeat(64))));
    assert!(!is_digest(&format!("sha256:{}", "a".repeat(63))));
    assert!(!is_digest(&format!("sha256:{}", "g".repeat(64))));
    assert!(!is_digest(&format!("sha512:{}", "a".repeat(64))));
    assert!(!is_digest("sha-b1d6c31"));
}
