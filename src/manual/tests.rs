use super::published::pick_published;
use super::*;

fn endpoints() -> Endpoints {
    Endpoints {
        docker_probe: false,
        ..Endpoints::default()
    }
}

#[test]
fn the_endpoints_default_to_the_public_ones() {
    let e = Endpoints::default();
    assert_eq!(e.github_api, "https://api.github.com");
    assert_eq!(e.ghcr_url, "https://ghcr.io");
    assert!(!e.offline && e.docker_probe);
}

#[test]
fn the_newest_commit_with_a_published_build_wins() {
    let shas: Vec<String> = ["b1d6c31aaaa", "60b16a2bbbb", "81ab16bcccc"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    // Every build published: the head commit.
    let (sha, tag) = pick_published(&shas, &|_| Ok(true)).unwrap().unwrap();
    assert_eq!(sha, "b1d6c31aaaa");
    assert_eq!(tag, "sha-b1d6c31");

    // The head changed nothing the workflow builds, so the pin lands on
    // the newest tag there is.
    let (sha, tag) = pick_published(&shas, &|tag| Ok(tag == "sha-60b16a2"))
        .unwrap()
        .unwrap();
    assert_eq!(sha, "60b16a2bbbb");
    assert_eq!(tag, "sha-60b16a2");

    // Nothing published at all.
    assert!(pick_published(&shas, &|_| Ok(false)).unwrap().is_none());
    // And a registry that would not answer is not "nothing published".
    assert!(pick_published(&shas, &|_| Err(anyhow::anyhow!("boom"))).is_err());
}

#[test]
fn the_data_directory_follows_the_working_directory() {
    assert_eq!(data_dir_under("/work"), "/work/data");
    assert_eq!(data_dir_under("/app/"), "/app/data");
    assert_eq!(data_dir_under(""), overrides::DEFAULT_DATA_DIR);
    assert_eq!(data_dir_under("/"), overrides::DEFAULT_DATA_DIR);
}

/// Everything `resolve_user` can be told, and what it decides.
#[test]
fn the_user_comes_from_the_flag_then_the_image_then_a_probe() {
    let mut notes = Vec::new();
    let never = |_: &str, _: &str| None;
    let pulled = |_: &str| true;

    // The flag wins over everything.
    assert_eq!(
        resolve_user_with(
            Some("1500:1600"),
            "app",
            "ghcr.io/x/y:t",
            &endpoints(),
            &mut notes,
            &pulled,
            &never
        ),
        ("1500:1600".to_string(), Origin::Flag)
    );
    // An image that declares no user gets the chapkit default.
    assert_eq!(
        resolve_user_with(
            None,
            "",
            "ghcr.io/x/y:t",
            &endpoints(),
            &mut notes,
            &pulled,
            &never
        ),
        (overrides::DEFAULT_USER.to_string(), Origin::Default)
    );
    // A numeric user, and a name the chapkit images create, are both
    // usable as they are.
    for declared in ["10001:10001", "chapkit", "chap:chap"] {
        assert_eq!(
            resolve_user_with(
                None,
                declared,
                "ghcr.io/x/y:t",
                &endpoints(),
                &mut notes,
                &pulled,
                &never
            ),
            (declared.to_string(), Origin::Image),
            "{declared}"
        );
    }
    assert!(notes.is_empty(), "{notes:?}");

    // A name nobody knows: the image itself is asked.
    let probe = |_: &str, name: &str| (name == "app").then_some((10001, 10001));
    assert_eq!(
        resolve_user_with(
            None,
            "app",
            "ghcr.io/x/y:t",
            &Endpoints::default(),
            &mut notes,
            &pulled,
            &probe
        ),
        ("10001:10001".to_string(), Origin::Probe)
    );
    assert!(notes.is_empty(), "{notes:?}");

    // And when the probe cannot run, the name is kept and the caller is
    // told what the init container will do instead.
    assert_eq!(
        resolve_user_with(
            None,
            "app",
            "ghcr.io/x/y:t",
            &endpoints(),
            &mut notes,
            &pulled,
            &never
        ),
        ("app".to_string(), Origin::Image)
    );
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("--user"), "{}", notes[0]);
    assert!(notes[0].contains("1000:1000"), "{}", notes[0]);
}

#[test]
fn a_failed_pull_is_not_a_probe() {
    let mut notes = Vec::new();
    let probe = |_: &str, _: &str| Some((10001, 10001));
    let (user, origin) = resolve_user_with(
        None,
        "app",
        "ghcr.io/x/y:t",
        &Endpoints::default(),
        &mut notes,
        &|_| false,
        &probe,
    );
    assert_eq!((user.as_str(), origin), ("app", Origin::Image));
    assert_eq!(notes.len(), 1);
}

#[test]
fn an_offline_run_refuses_a_repository_and_says_what_to_pass_instead() {
    let offline = Endpoints {
        offline: true,
        ..endpoints()
    };
    let source = Source::parse("https://github.com/chap-models/chapkit_ghr_model").unwrap();
    let err = pin(&source, &offline).expect_err("--offline cannot resolve a branch");
    assert!(err.to_string().contains("--offline"), "{err}");
    assert!(
        err.to_string()
            .contains("ghcr.io/chap-models/chapkit_ghr_model"),
        "{err}"
    );

    // An image reference pins itself, with or without a network.
    let source = Source::parse("ghcr.io/chap-models/chapkit_ghr_model:sha-1eb8cf1").unwrap();
    let (tag, commit, follow) = pin(&source, &offline).unwrap();
    assert_eq!(tag, "sha-1eb8cf1");
    assert_eq!(commit, None);
    assert_eq!(follow, None);
}

#[test]
fn the_selector_follows_a_branch_or_pins_the_tag() {
    let following = Resolved {
        reads_port: false,
        source: Source::parse("https://github.com/chap-models/chapkit_ghr_model").unwrap(),
        id: "chapkit_ghr_model".into(),
        service_id: "chapkit-ghr-model".into(),
        display_name: "chapkit_ghr_model".into(),
        image: "ghcr.io/chap-models/chapkit_ghr_model".into(),
        tag: "sha-b1d6c31".into(),
        commit: Some("b1d6c31".repeat(5)),
        follow: Some("main".into()),
        data_dir: "/work/data".into(),
        data_dir_from: Origin::Image,
        user: "10001:10001".into(),
        user_from: Origin::Probe,
        runtime_amd64: true,
        notes: Vec::new(),
    };
    assert_eq!(
        following.selector(),
        VersionSelector::Channel(crate::registry::Channel::Latest)
    );
    assert_eq!(
        following.image_ref(),
        "ghcr.io/chap-models/chapkit_ghr_model:sha-b1d6c31"
    );

    let pinned = Resolved {
        follow: None,
        ..following
    };
    assert_eq!(
        pinned.selector(),
        VersionSelector::Exact("sha-b1d6c31".into())
    );
}

#[test]
fn the_origins_read_as_sentences() {
    assert_eq!(Origin::Flag.label(), "given on the command line");
    assert_eq!(Origin::Image.label(), "from the image config");
    assert_eq!(Origin::Probe.label(), "from a docker probe");
    assert_eq!(Origin::Default.label(), "the chapkit default");
}

/// An image config can hold any text, unlike a Dockerfile `USER` or
/// `WORKDIR`. A value the overlay cannot hold falls back to the default with
/// a note, and the user never reaches the probe's `sh -c`.
#[test]
fn an_image_user_or_working_directory_with_yaml_in_it_falls_back() {
    let mut notes = Vec::new();
    let probed = std::cell::Cell::new(false);
    let probe = |_: &str, _: &str| {
        probed.set(true);
        Some((1, 1))
    };
    assert_eq!(
        resolve_user_with(
            None,
            "app\n    privileged: true",
            "ghcr.io/x/y:t",
            &endpoints(),
            &mut notes,
            &|_| true,
            &probe
        ),
        (overrides::DEFAULT_USER.to_string(), Origin::Default)
    );
    assert!(!probed.get());
    assert!(notes[0].contains("--user <uid>:<gid>"), "{notes:?}");

    let mut notes = Vec::new();
    assert_eq!(
        image_data_dir("/app\n    privileged: true", "ghcr.io/x/y:t", &mut notes),
        None
    );
    assert!(notes[0].contains("--data-dir <path>"), "{notes:?}");
    assert_eq!(
        image_data_dir("/app", "ghcr.io/x/y:t", &mut notes),
        Some("/app/data".to_string())
    );
    assert_eq!(image_data_dir(" ", "ghcr.io/x/y:t", &mut notes), None);
    assert_eq!(notes.len(), 1, "{notes:?}");
}
