//! `--modeling-app`: the stable App Hub version, a pinned one, or the
//! unstable build of chap-frontend's `main` from GitHub.

use super::*;

/// What `.chap-devops/components.yaml` says now.
#[cfg(unix)]
fn components(dir: &std::path::Path) -> String {
    std::fs::read_to_string(dir.join(".chap-devops/components.yaml"))
        .expect("init wrote the components")
}

/// The unstable build is downloaded from the GitHub release, checked and
/// uploaded to `POST /api/apps`. The state records it, so a second run
/// installs nothing, a new build of `main` is installed again, and `stable`
/// goes back to the App Hub and clears the record.
#[cfg(unix)]
#[test]
fn the_unstable_build_comes_from_github_and_is_recorded() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        unstable: Some(0),
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);
    let hub = Some(stand_in.port);

    dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        hub,
        &["apps", "--modeling-app", "unstable"],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains(
        "installed Modeling App unstable 0c30f8d",
    ))
    .stdout(predicates::str::contains(
        "installed DHIS2 Climate App 1.16.2",
    ));
    {
        let state = stand_in.state.lock().unwrap();
        assert_eq!(state.uploads.len(), 1);
        assert!(
            state.uploads[0].contains("filename=\"modeling-app-latest.zip\""),
            "{}",
            state.uploads[0]
        );
        assert!(state.uploads[0].contains("PK fake modeling app build one"));
        assert!(
            !state
                .asked
                .iter()
                .any(|seen| seen.starts_with("POST /api/appHub/mv-")),
            "{:?}",
            state.asked
        );
    }
    let written = components(&dir);
    assert!(written.contains("modeling_app: unstable"), "{written}");
    assert!(
        written.contains("commit: 0c30f8de28515c933aeb5916a3d94b8478f843de"),
        "{written}"
    );

    // The setting is followed without the flag, and the same build is not
    // uploaded again.
    dhis2_chap(&sandbox, &dir, &bin, hub, &["apps"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Modeling App unstable 0c30f8d is already installed",
        ));
    assert_eq!(stand_in.state.lock().unwrap().uploads.len(), 1);

    // A new build of `main` is installed again.
    stand_in.state.lock().unwrap().unstable = Some(1);
    dhis2_chap(&sandbox, &dir, &bin, hub, &["apps"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "moved Modeling App from unstable 0c30f8d to unstable 9a1b2c3",
        ));
    assert_eq!(stand_in.state.lock().unwrap().uploads.len(), 2);

    // `show` names the build, which DHIS2 lists as the release before it.
    dhis2_chap(&sandbox, &dir, &bin, None, &["show"])
        .assert()
        .success()
        .stdout(predicates::str::contains("(unstable 9a1b2c3)"));

    // Back to stable: the App Hub version replaces it, though the version
    // number is the same, and the record goes.
    dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        hub,
        &["apps", "--modeling-app", "stable"],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains(
        "moved Modeling App from unstable 9a1b2c3 to 7.1.0",
    ));
    assert!(stand_in.was_asked("POST /api/appHub/mv-7.1.0"));
    let written = components(&dir);
    assert!(!written.contains("modeling_app"), "{written}");
}

/// A pinned version comes from the App Hub. One it does not have, or one this
/// DHIS2 cannot run, is refused with the reason, and the setting is not kept.
#[cfg(unix)]
#[test]
fn a_pinned_version_must_exist_and_fit_this_dhis2() {
    let stand_in = Dhis2StandIn::new();
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);
    let hub = Some(stand_in.port);

    dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        hub,
        &["apps", "--modeling-app", "6.0.0"],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains(
        "the App Hub has no version 6.0.0; the newest are 7.1.0, 0.9.0",
    ));
    dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        hub,
        &["apps", "--modeling-app", "0.9.0"],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains(
        "version 0.9.0 needs DHIS2 2.43 or newer, and this DHIS2 is 2.42.6",
    ));
    assert!(!components(&dir).contains("modeling_app"));

    dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        hub,
        &["apps", "--modeling-app", "v7.1.0"],
    )
    .assert()
    .success()
    .stdout(predicates::str::contains("installed Modeling App 7.1.0"));
    assert!(components(&dir).contains("modeling_app: 7.1.0"));
}

/// A zip whose digest is not the one GitHub lists is never uploaded.
#[cfg(unix)]
#[test]
fn an_unstable_build_with_the_wrong_digest_is_refused() {
    let stand_in = Dhis2StandIn::with(Dhis2State {
        unstable: Some(0),
        unstable_digest: Some(
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        ),
        ..Dhis2State::default()
    });
    let (sandbox, dir, _temp, bin) = dhis2_connected(&stand_in);
    dhis2_chap(
        &sandbox,
        &dir,
        &bin,
        Some(stand_in.port),
        &["apps", "--modeling-app", "unstable"],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains("not the sha256:0000"));
    assert!(stand_in.state.lock().unwrap().uploads.is_empty());
    assert!(!components(&dir).contains("modeling_app"));
}

/// `init --modeling-app` records the choice for the deployment's own DHIS2,
/// and needs that DHIS2.
#[test]
fn init_records_the_modeling_app_and_needs_dhis2() {
    let sandbox = Sandbox::new();
    sandbox
        .init(&["--modeling-app", "unstable"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "--modeling-app needs the dhis2 component",
        ));
    sandbox
        .init(&["--modeling-app", "latest"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("is not a Modeling App build"));

    let sandbox = Sandbox::new();
    sandbox
        .init(&["--only", "dhis2", "--modeling-app", "unstable"])
        .assert()
        .success();
    let written = std::fs::read_to_string(sandbox.project().join(".chap-devops/components.yaml"))
        .expect("init wrote the components");
    assert!(written.contains("modeling_app: unstable"), "{written}");
}
