//! Marketplace versions pinned by image digest.

use crate::common::*;

fn digest() -> String {
    format!("sha256:{}", "7".repeat(64))
}

/// A version with an `image_digest` runs at `tag@digest`: the overlay puts
/// the digest in the default of the tag variable, `models.yaml` records it,
/// the commented pin in `.env` carries it, and the image config is read by
/// the digest.
#[test]
fn a_listed_digest_is_pinned_with_the_tag() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--api-port", &free_port().to_string()])
        .assert()
        .success();
    let (port, seen) = Hub::new()
        .publishing(MARKETPLACE_TAG)
        .listing_digest(&digest())
        .start_seen();
    sandbox
        .online(port)
        .args(["models", "enable", "chapkit_ewars_model"])
        .assert()
        .success();

    let model = &state(&dir)["models"]["chapkit_ewars_model"];
    assert_eq!(model["image_tag"], MARKETPLACE_TAG);
    assert_eq!(model["image_digest"], digest().as_str());

    let overlay = read(&dir.join("compose.chapkit-ewars-model.yml"));
    let line = format!(
        "    image: ghcr.io/chap-models/chapkit_ewars_model:\
         ${{CHAPKIT_EWARS_MODEL_IMAGE_TAG:-{MARKETPLACE_TAG}@{}}}\n",
        digest()
    );
    assert!(overlay.contains(&line), "{overlay}");

    let env = read(&dir.join(".env"));
    assert!(
        env.contains(&format!(
            "# CHAPKIT_EWARS_MODEL_IMAGE_TAG={MARKETPLACE_TAG}@{}\n",
            digest()
        )),
        "{env}"
    );

    let paths: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|(p, _)| p.clone())
        .collect();
    assert!(
        paths
            .iter()
            .any(|p| p.ends_with(&format!("/manifests/{}", digest()))),
        "{paths:?}"
    );
    assert!(
        !paths
            .iter()
            .any(|p| p.ends_with(&format!("/manifests/{MARKETPLACE_TAG}"))),
        "{paths:?}"
    );

    sandbox
        .models(&["info", "chapkit_ewars_model"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "ghcr.io/chap-models/chapkit_ewars_model:{MARKETPLACE_TAG}@{}",
            digest()
        )));
}

/// A digest that is not `sha256:<64 hex>` stops the enable and names the
/// version, so nothing is pinned to a reference docker cannot pull.
#[test]
fn a_malformed_digest_is_refused() {
    let (sandbox, dir, port) = added_sandbox(
        Hub::new()
            .publishing(MARKETPLACE_TAG)
            .listing_digest("sha256:short"),
    );
    sandbox
        .online(port)
        .args(["models", "enable", "chapkit_ewars_model"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "version 1.0.0 of `chapkit_ewars_model` lists the image digest sha256:short",
        ));
    assert!(!dir.join("compose.chapkit-ewars-model.yml").exists());
}
