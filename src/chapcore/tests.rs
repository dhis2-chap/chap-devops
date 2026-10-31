use super::*;

#[test]
fn the_urls_name_the_repository_and_the_tag() {
    assert_eq!(
        compose_url("v2.3.1"),
        "https://raw.githubusercontent.com/dhis2-chap/chap-core/v2.3.1/compose.ghcr.yml"
    );
    assert!(compose_url("master").ends_with("/master/compose.ghcr.yml"));
    assert!(releases_url(1).contains(REPO));
    assert!(releases_url(1).starts_with(crate::github::DEFAULT_API));
    assert_eq!(
        release_url("v2.3.1"),
        "https://api.github.com/repos/dhis2-chap/chap-core/releases/tags/v2.3.1"
    );
    assert!(releases_url(10).ends_with("/releases?per_page=10"));
    assert!(branch_url("dev").ends_with("/commits?sha=dev&per_page=1"));
}

#[test]
fn release_tags_parse_and_everything_else_does_not() {
    assert_eq!(release_version("v2.3.1"), Some((2, 3, 1)));
    assert_eq!(release_version("2.3.1"), Some((2, 3, 1)));
    assert_eq!(release_version("v10.0.0"), Some((10, 0, 0)));
    assert_eq!(release_version("v2.4.0-rc.1"), Some((2, 4, 0)));
    for bad in [
        "latest",
        "master",
        "dev",
        "sha-fa880a1",
        "v2.3",
        "v2.3.1.4",
        "",
    ] {
        assert_eq!(release_version(bad), None, "{bad}");
    }
}

#[test]
fn newer_compares_the_three_components() {
    assert!(is_newer("v2.3.1", "v2.3.0"));
    assert!(is_newer("v2.4.0", "v2.3.9"));
    assert!(is_newer("v3.0.0", "v2.99.99"));
    assert!(is_newer("v2.10.0", "v2.9.0"), "not a string comparison");
    assert!(!is_newer("v2.3.1", "v2.3.1"));
    assert!(!is_newer("v2.3.0", "v2.3.1"));
    // A moving tag is not a point on the version line.
    assert!(!is_newer("v2.3.1", "latest"));
    assert!(!is_newer("latest", "v2.3.1"));
}

#[test]
fn moving_tags_are_the_ones_that_can_change_under_us() {
    for tag in ["latest", "master", "main", "dev"] {
        assert!(is_moving_tag(tag), "{tag}");
    }
    for tag in ["v2.3.1", "sha-fa880a1", "v1.0.0-rc.1"] {
        assert!(!is_moving_tag(tag), "{tag}");
    }
}

#[test]
fn the_vendored_compose_file_validates() {
    let embedded =
        crate::compose::render::render_base(&crate::compose::spec::BaseSpec { upstream: None });
    validate_compose(&embedded).expect("the copy we ship is usable");
}

#[test]
fn a_compose_file_that_would_lose_the_pin_is_rejected() {
    let err = validate_compose("not: [yaml").expect_err("unparseable");
    assert!(err.to_string().contains("not valid YAML"), "{err}");

    let err =
        validate_compose("services:\n  worker:\n    image: x\n").expect_err("no chap service");
    assert!(err.to_string().contains("services.chap"), "{err}");

    let err = validate_compose("services:\n  chap:\n    restart: always\n").expect_err("no image");
    assert!(err.to_string().contains("no image"), "{err}");

    let err = validate_compose("services:\n  chap:\n    image: ghcr.io/x/chap-core:latest\n")
        .expect_err("a hard-coded tag defeats CHAP_IMAGE_TAG");
    assert!(err.to_string().contains("CHAP_IMAGE_TAG"), "{err}");

    validate_compose("services:\n  chap:\n    image: ghcr.io/x:${CHAP_IMAGE_TAG:-latest}\n")
        .expect("the minimal acceptable document");
}

#[test]
fn sha256_matches_the_published_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    // 1_000_000 bytes crosses many blocks and exercises the length field.
    assert_eq!(
        sha256_hex(&vec![b'a'; 1_000_000]),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
    // And the value is stable for something the CLI actually hashes.
    assert_eq!(sha256_hex("services: {}\n".as_bytes()).len(), 64);
}

/// A port nothing listens on covers the transport-error path without a
/// network.
#[test]
fn an_unreachable_host_is_an_error_not_a_hang() {
    let base = crate::test_support::unreachable_base();
    let err = get(&format!("{base}/compose.ghcr.yml"), Duration::from_secs(2))
        .expect_err("nothing is listening there");
    assert!(err.to_string().contains(&base[7..]), "{err}");
}

#[test]
fn a_status_code_becomes_a_typed_http_error() {
    let url = releases_url(1);
    let err = map_error(&url, ureq::Error::StatusCode(403));
    match err.downcast_ref::<ChapError>() {
        Some(ChapError::Http { url: at, status }) => {
            assert_eq!(*at, url);
            assert_eq!(*status, 403);
        }
        other => panic!("wrong error: {other:?}"),
    }
}

/// The list page, trimmed to the fields that decide what is listed.
const RELEASES: &str = r#"[
      {"tag_name":"v2.3.1","published_at":"2026-09-21T10:00:25Z","draft":false,"prerelease":false},
      {"tag_name":"v2.4.0-rc.1","published_at":"2026-09-20T10:00:25Z","draft":false,"prerelease":true},
      {"tag_name":"chap-1.1.1","published_at":"2026-08-26T18:15:28Z","draft":false,"prerelease":false},
      {"tag_name":"v2.3.0","published_at":"2026-09-11T09:20:41Z","draft":false,"prerelease":false},
      {"tag_name":"v2.2.0","draft":true,"prerelease":false}
    ]"#;

#[test]
fn the_release_list_keeps_the_versions_and_drops_the_rest() {
    let list = parse_releases(RELEASES).unwrap();
    assert_eq!(
        list,
        vec![
            Release {
                tag: "v2.3.1".into(),
                published: "2026-09-21".into()
            },
            Release {
                tag: "v2.3.0".into(),
                published: "2026-09-11".into()
            },
        ]
    );
    // A page with nothing usable in it is an empty list, not an error.
    assert!(parse_releases("[]").unwrap().is_empty());
    assert!(parse_releases(r#"{"message":"rate limit"}"#).is_err());
}

#[test]
fn a_commit_page_yields_the_day_of_its_newest_commit() {
    let body = r#"[{"sha":"abc","commit":{"committer":{"date":"2026-09-24T08:15:00Z"}}}]"#;
    assert_eq!(parse_commit_date(body).as_deref(), Some("2026-09-24"));
    // The author's date is the fallback, and a page with neither has none.
    let author = r#"[{"commit":{"author":{"date":"2026-09-23T08:15:00Z"}}}]"#;
    assert_eq!(parse_commit_date(author).as_deref(), Some("2026-09-23"));
    assert_eq!(parse_commit_date("[]"), None);
    assert_eq!(parse_commit_date(r#"[{"commit":{}}]"#), None);
    assert_eq!(day_of("not-a-date"), None);
    assert_eq!(day_of("2026-9-1T00:00:00Z"), None);
}

#[test]
fn a_tag_is_a_release_a_moving_tag_or_an_exact_pin() {
    assert_eq!(tag_kind("v2.3.1"), TagKind::Release);
    assert_eq!(tag_kind("2.3.1"), TagKind::Release);
    for moving in MOVING_TAGS {
        assert_eq!(tag_kind(moving), TagKind::Moving, "{moving}");
    }
    assert_eq!(tag_kind("sha-fa880a1"), TagKind::Other);
    assert_eq!(tag_kind(""), TagKind::Other);
    assert_eq!(TagKind::Release.label(), "release");
    assert_eq!(TagKind::Moving.label(), "moving");
    assert_eq!(TagKind::Other.label(), "exact");
}

#[test]
fn only_an_older_release_counts_as_moving_backwards() {
    // Release to release, by version and not by string.
    assert!(is_backwards("v2.3.1", "v2.3.0"));
    assert!(is_backwards("v2.10.0", "v2.9.0"));
    assert!(!is_backwards("v2.3.0", "v2.3.1"));
    assert!(!is_backwards("v2.3.1", "v2.3.1"));
    // A moving tag is ahead of every release, `latest` included.
    for moving in MOVING_TAGS {
        assert!(is_backwards(moving, "v2.3.1"), "{moving}");
        assert!(!is_backwards("v2.3.1", moving), "{moving}");
    }
    assert!(!is_backwards("dev", "master"));
    // A `sha-` build is not on the line at all, in either direction.
    assert!(!is_backwards("sha-fa880a1", "v2.3.1"));
    assert!(!is_backwards("v2.3.1", "sha-fa880a1"));
    assert!(!is_backwards("dev", "sha-fa880a1"));
}

#[test]
fn the_hosts_follow_the_test_hooks() {
    use crate::github::base_of;
    let hook = Some("http://127.0.0.1:18099/".to_string());
    assert_eq!(base_of(hook, DEFAULT_RAW), "http://127.0.0.1:18099");
    // Unset, empty and whitespace all mean the public host.
    assert_eq!(base_of(None, DEFAULT_RAW), DEFAULT_RAW);
    assert_eq!(base_of(Some(String::new()), DEFAULT_RAW), DEFAULT_RAW);
    assert_eq!(base_of(Some("  ".into()), DEFAULT_RAW), DEFAULT_RAW);
}

/// A Helm chart release can be GitHub's latest, and no image has its tag.
#[test]
fn the_newest_release_is_the_highest_version_and_never_a_chart() {
    let list = r#"[
      {"tag_name":"chap-1.2.0","published_at":"2026-10-29T14:39:27Z","draft":false,"prerelease":false},
      {"tag_name":"v2.4.1","published_at":"2026-10-21T10:00:25Z","draft":false,"prerelease":false},
      {"tag_name":"v2.6.0-rc.1","published_at":"2026-10-20T10:00:25Z","draft":false,"prerelease":true},
      {"tag_name":"v2.5.1","published_at":"2026-10-19T22:38:17Z","draft":false,"prerelease":false}
    ]"#;
    assert_eq!(parse_latest(list).unwrap(), "v2.5.1");
}

#[test]
fn a_release_list_without_a_version_is_an_error() {
    let only_a_chart = r#"[{"tag_name":"chap-1.2.0","published_at":"2026-10-29T14:39:27Z","draft":false,"prerelease":false}]"#;
    assert!(
        parse_latest(only_a_chart)
            .unwrap_err()
            .to_string()
            .contains("no version tag")
    );
    assert!(parse_latest("[]").is_err());
    // The rate-limit body parses as JSON but is not a list.
    assert!(parse_latest(r#"{"message":"API rate limit exceeded"}"#).is_err());
    assert!(parse_latest("<html>nope</html>").is_err());
}
