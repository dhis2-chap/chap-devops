//! The platform a model overlay pins: linux/amd64, unless the registry also
//! lists a linux/arm64 image for the tag; then the host's own.

use crate::common::*;
use std::path::Path;

const PYTHON_RUNTIME: &str = "ghcr.io/dhis2-chap/chapkit-py";

/// The `platform:` of the model service, or `None` when the overlay has none.
fn platform(dir: &Path) -> Option<String> {
    let overlay = yaml(&dir.join("compose.chapkit-ewars-model.yml"));
    overlay["services"]["chapkit-ewars-model"]
        .get("platform")
        .and_then(|p| p.as_str())
        .map(str::to_string)
}

/// Enable the served model; the sandbox is returned, as it owns the files.
fn enable(hub: Hub) -> (Sandbox, std::path::PathBuf) {
    let (sandbox, dir, port) = added_sandbox(hub.publishing(MARKETPLACE_TAG));
    sandbox
        .online(port)
        .args(["models", "enable", "chapkit_ewars_model"])
        .assert()
        .success();
    (sandbox, dir)
}

/// The platform of this host, as docker names it.
fn host() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "linux/arm64",
        _ => "linux/amd64",
    }
}

/// An image with an arm64 build runs as the host's own platform. The line is
/// there either way: without it, compose starts a local copy pulled for
/// amd64 alone.
#[test]
fn a_multi_arch_image_runs_on_the_host_architecture() {
    let (_sandbox, dir) = enable(Hub::new().on_runtime(PYTHON_RUNTIME).multi_arch());
    assert_eq!(platform(&dir).as_deref(), Some(host()));
    let model = &state(&dir)["models"]["chapkit_ewars_model"];
    assert_eq!(model["multi_arch"], true, "{model}");
}

/// An amd64-only image keeps the pin, which an arm64 host needs to pull it.
#[test]
fn an_amd64_only_image_keeps_the_pin() {
    let (_sandbox, dir) = enable(Hub::new().on_runtime(PYTHON_RUNTIME));
    assert_eq!(platform(&dir).as_deref(), Some("linux/amd64"));
    assert!(state(&dir)["models"]["chapkit_ewars_model"]["multi_arch"].is_null());
}

/// An R-INLA model keeps the pin whatever the index lists: INLA has no arm64
/// build.
#[test]
fn an_r_inla_model_keeps_the_pin() {
    let (_sandbox, dir) = enable(Hub::new().multi_arch());
    assert_eq!(platform(&dir).as_deref(), Some("linux/amd64"));
}

/// Without the registry nothing says the image has an arm64 build, so the
/// pin stays.
#[test]
fn offline_keeps_the_pin() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&[
            "--models",
            "chapkit_simple_multistep_model",
            "--api-port",
            &free_port().to_string(),
        ])
        .assert()
        .success();
    let overlay = yaml(&dir.join("compose.chapkit-simple-multistep-model.yml"));
    assert_eq!(
        overlay["services"]["chapkit-simple-multistep-model"]["platform"].as_str(),
        Some("linux/amd64")
    );
}
