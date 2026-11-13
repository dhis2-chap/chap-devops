use super::images::{
    image_config_with, parse_command, parse_container_builds, parse_image_config,
    parse_repo_digest, parse_uid_gid, short_digest,
};
use super::plain::strip_ansi;
use super::ps::{containers, parse_ps_json};
use super::query::{parse_config_hashes, parse_service_images};
use super::version::parse_version;
use super::volumes::parse_du_kilobytes;
use super::*;
use std::collections::BTreeSet;

#[test]
fn a_containers_creation_time_is_read_with_its_offset() {
    let at = |created_at: &str| Container {
        created_at: created_at.to_string(),
        ..Container::default()
    };
    assert_eq!(at("1970-01-01 01:00:10 +0100 CET").created_unix(), Some(10));
    assert_eq!(at("1970-01-01 00:00:10 +0000 UTC").created_unix(), Some(10));
    assert_eq!(at("").created_unix(), None);
}

#[test]
fn a_container_is_young_for_its_first_two_minutes() {
    let at = |status: &str| Container {
        status: status.to_string(),
        ..Container::default()
    };
    for young in [
        "Up Less than a second",
        "Up 1 second",
        "Up 45 seconds",
        "Up 12 seconds (health: starting)",
        "Up About a minute",
    ] {
        assert!(at(young).is_young(), "{young}");
    }
    for old in [
        "Up 2 minutes",
        "Up About an hour",
        "Up 3 hours (healthy)",
        "Exited (0) 5 seconds ago",
        "",
    ] {
        assert!(!at(old).is_young(), "{old}");
    }
}

/// `du -sk` counts kilobytes, and the report counts bytes.
#[test]
fn a_du_total_is_read_as_kilobytes_and_reported_as_bytes() {
    // What busybox prints for a mounted volume, tab and all.
    assert_eq!(parse_du_kilobytes("217088\t/v\n"), Some(217_088 * 1024));
    assert_eq!(
        parse_du_kilobytes("212992      /app/data"),
        Some(212_992 * 1024)
    );
    assert_eq!(parse_du_kilobytes("0\t/v"), Some(0));
    // The total is the last line, so a `du` that complained about a
    // directory it could not read on the way still answers. The
    // complaint's own line carries no number to be mistaken for one.
    assert_eq!(
        parse_du_kilobytes("du: /v/lost+found: Permission denied\n4\t/v\n"),
        Some(4096)
    );
    assert_eq!(
        parse_du_kilobytes("4\t/v/downloads\n217088\t/v\n"),
        Some(217_088 * 1024),
        "the total is the last line, not the first"
    );
    // Nothing to read: a `du` that printed nothing, or something else.
    for text in ["", "\n \n", "du: /v: No such file or directory", "-1\t/v"] {
        assert_eq!(parse_du_kilobytes(text), None, "{text:?}");
    }
}

#[test]
fn the_uid_probe_reads_the_two_numbers_the_image_printed() {
    assert_eq!(parse_uid_gid("10001\n10001\n"), Some((10001, 10001)));
    assert_eq!(parse_uid_gid("  1000 \n 1000 \n"), Some((1000, 1000)));
    assert_eq!(parse_uid_gid("0\n0"), Some((0, 0)));
    // A busybox that says `id: unknown user app` on stderr prints
    // nothing here, and an image with no `id` at all prints nothing
    // either: both are "the image could not be asked".
    for text in ["", "\n\n", "10001", "nope\nnope"] {
        assert_eq!(parse_uid_gid(text), None, "{text:?}");
    }
}
use crate::project::{MARKETPLACE_COMPOSE, ProjectState};
use std::path::PathBuf;

fn project(dir: &str) -> Project {
    Project {
        dir: PathBuf::from(dir),
        state: ProjectState::default(),
    }
}

#[test]
fn compose_args_lists_every_file_absolutely_and_in_order() {
    let dir = "/tmp/chapx";
    let p = project(dir);
    // Each `-f` is the project directory joined with the file name, in
    // the separator of whatever platform the CLI runs on.
    let file = |name: &str| PathBuf::from(dir).join(name).to_string_lossy().into_owned();
    assert_eq!(
        compose_args(&p),
        vec![
            "compose".to_string(),
            "-f".to_string(),
            file("compose.yml"),
            // The chap-devops-owned overrides sit between the base file and the
            // umbrella: later files win, and this one overrides chap.
            "-f".to_string(),
            file("compose.chap-devops.yml"),
            "-f".to_string(),
            file("compose.marketplace.yml"),
        ]
    );
}

#[test]
fn compose_args_follows_the_state_file_list() {
    let mut p = project("/tmp/chapx");
    p.state.compose_files = vec![
        "compose.yml".to_string(),
        MARKETPLACE_COMPOSE.to_string(),
        "compose.override.yml".to_string(),
    ];
    let args = compose_args(&p);
    assert_eq!(args.iter().filter(|a| *a == "-f").count(), 3);
    assert!(args.last().unwrap().ends_with("compose.override.yml"));
}

#[test]
fn compose_args_of_a_project_without_files_is_just_compose() {
    let mut p = project("/tmp/chapx");
    p.state.compose_files.clear();
    assert_eq!(compose_args(&p), vec!["compose".to_string()]);
}

#[test]
fn ps_json_is_read_in_both_shapes_compose_prints() {
    // Compose 2.21+: one object per line.
    let lines = concat!(
        r#"{"Name":"x-chap-1","Service":"chap","State":"running","Health":"healthy"}"#,
        "\n",
        r#"{"Name":"x-ewars-1","Service":"chapkit-ewars-model","State":"running"}"#,
        "\n",
        r#"{"Name":"x-init-1","Service":"chapkit-ewars-model-init","State":"exited"}"#,
        "\n"
    );
    let running = parse_ps_json(lines);
    assert_eq!(
        running,
        BTreeSet::from(["chap".to_string(), "chapkit-ewars-model".to_string()]),
        "an exited container is not running"
    );

    // Older compose: a single array.
    let array =
        r#"[{"Service":"chap","State":"running"},{"Service":"worker","State":"restarting"}]"#;
    assert_eq!(parse_ps_json(array), BTreeSet::from(["chap".to_string()]));

    // A state compose did not report still counts: `ps` without -a lists
    // what is up.
    assert_eq!(
        parse_ps_json(r#"{"Service":"chap"}"#),
        BTreeSet::from(["chap".to_string()])
    );
}

#[test]
fn unreadable_ps_output_is_an_empty_set_not_a_panic() {
    for text in [
        "",
        "   \n\n",
        "no containers",
        "<html>",
        r#"{"Name":"x-chap-1"}"#,
        "[]",
        "{}",
    ] {
        assert!(parse_ps_json(text).is_empty(), "{text:?}");
    }
}

/// Two containers, as `ps -a --format json` prints them.
const PS_A: &str = concat!(
    r#"{"ID":"aaa","Name":"x-chap-1","Service":"chap","State":"running","#,
    r#""CreatedAt":"2026-09-23 12:58:52 +0200 CEST"}"#,
    "\n",
    r#"{"ID":"bbb","Name":"x-init-1","Service":"ewars-init","State":"exited","#,
    r#""CreatedAt":"2026-09-23 12:58:52 +0200 CEST"}"#,
    "\n"
);

#[test]
fn containers_are_read_with_the_fields_the_wrappers_use() {
    let found = containers(PS_A);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].service, "chap");
    assert_eq!(found[0].name, "x-chap-1");
    assert_eq!(found[0].id, "aaa");
    assert_eq!(found[0].created_at, "2026-09-23 12:58:52 +0200 CEST");
    assert!(found[0].is_running());
    assert!(!found[1].is_running(), "an exited container is not running");

    assert_eq!(running_of(&found), BTreeSet::from(["chap".to_string()]));
    assert_eq!(service_names(&found), vec!["chap", "ewars-init"]);

    // Nothing usable in, nothing out.
    for text in ["", "   ", "no containers", "<html>", "[]", r#"{"ID":"a"}"#] {
        assert!(containers(text).is_empty(), "{text:?}");
        assert!(service_names(&containers(text)).is_empty(), "{text:?}");
    }
}

/// One container, spelled out so a test can vary a single field.
fn container(service: &str, id: &str, created: &str) -> Container {
    Container {
        service: service.to_string(),
        name: format!("x-{service}-1"),
        id: id.to_string(),
        created_at: created.to_string(),
        state: "running".to_string(),
        ..Container::default()
    }
}

#[test]
fn the_diff_splits_recreated_services_from_untouched_ones() {
    let before = vec![
        container("chap", "aaa", "t1"),
        container("postgres", "ccc", "t1"),
    ];
    let after = vec![
        // Same container: compose left it alone.
        container("postgres", "ccc", "t1"),
        // Same service, new container: recreated.
        container("chap", "ddd", "t2"),
        // Not there before at all: started.
        container("worker", "eee", "t2"),
    ];
    let (started, unchanged) = diff_containers(&before, &after);
    assert_eq!(started, vec!["chap", "worker"]);
    assert_eq!(unchanged, vec!["postgres"]);

    // A first start: everything is new.
    let (started, unchanged) = diff_containers(&[], &after);
    assert_eq!(started, vec!["postgres", "chap", "worker"]);
    assert!(unchanged.is_empty());

    // A no-op `up`: nothing moved.
    let (started, unchanged) = diff_containers(&after, &after);
    assert!(started.is_empty());
    assert_eq!(unchanged, vec!["postgres", "chap", "worker"]);

    // A container recreated with the same id (never seen in practice, but
    // the creation time still says it is a different one).
    let (started, _) = diff_containers(
        &[container("chap", "aaa", "t1")],
        &[container("chap", "aaa", "t2")],
    );
    assert_eq!(started, vec!["chap"]);
}

#[test]
fn health_is_read_from_the_same_ps_output() {
    let lines = concat!(
        r#"{"Service":"postgres","State":"running","Health":"starting"}"#,
        "\n",
        r#"{"Service":"chap","State":"running","Health":"healthy"}"#,
        "\n",
        r#"{"Service":"redis","State":"exited","Health":"healthy"}"#,
        "\n"
    );
    assert_eq!(ps_entries(lines).len(), 3);
    assert!(service_is_healthy(lines, "chap"));
    assert!(!service_is_healthy(lines, "postgres"), "still starting");
    assert!(!service_is_healthy(lines, "redis"), "not running");
    assert!(!service_is_healthy(lines, "worker"), "not there at all");

    // No healthcheck at all: running is as good as it gets.
    assert!(service_is_healthy(
        r#"{"Service":"postgres","State":"running"}"#,
        "postgres"
    ));
    // The array shape older compose prints.
    assert!(service_is_healthy(
        r#"[{"Service":"postgres","State":"running","Health":"healthy"}]"#,
        "postgres"
    ));
    for text in ["", "not json", "[]"] {
        assert!(!service_is_healthy(text, "postgres"), "{text:?}");
        assert!(ps_entries(text).is_empty(), "{text:?}");
    }
}

#[test]
fn version_parsing_handles_the_shapes_compose_prints() {
    assert_eq!(parse_version("v5.5.1").unwrap(), (5, 5, 1));
    assert_eq!(parse_version("2.24.0\n").unwrap(), (2, 24, 0));
    assert_eq!(parse_version("  v2.20.0  ").unwrap(), (2, 20, 0));
    assert_eq!(parse_version("2.24.0-desktop.1").unwrap(), (2, 24, 0));
    assert_eq!(parse_version("2.24").unwrap(), (2, 24, 0));
    assert_eq!(parse_version("3").unwrap(), (3, 0, 0));
}

#[test]
fn version_parsing_rejects_nonsense() {
    for bad in ["", "unknown", "vx.y.z", "Docker Compose"] {
        assert!(parse_version(bad).is_err(), "{bad} should not parse");
    }
}

/// The supported minimum is the release `!override` arrived in, not the
/// older one `include:` did: every `compose.chap-devops.yml` this CLI renders
/// uses the tag, so 2.24.3 cannot run what `chap-devops sync` writes.
#[test]
fn min_version_ordering_is_what_the_warning_uses() {
    assert_eq!(MIN_COMPOSE_VERSION, (2, 24, 4));
    assert!(parse_version("2.19.1").unwrap() < MIN_COMPOSE_VERSION);
    assert!(parse_version("2.20.0").unwrap() < MIN_COMPOSE_VERSION);
    assert!(parse_version("2.24.3").unwrap() < MIN_COMPOSE_VERSION);
    assert!(parse_version("2.24.4").unwrap() >= MIN_COMPOSE_VERSION);
    assert!(parse_version("2.24.4-desktop.1").unwrap() >= MIN_COMPOSE_VERSION);
    assert!(parse_version("v5.5.1").unwrap() >= MIN_COMPOSE_VERSION);
    // `include:` is the older requirement, and still a real one.
    assert!(INCLUDE_COMPOSE_VERSION < MIN_COMPOSE_VERSION);
    assert!(parse_version("2.19.1").unwrap() < INCLUDE_COMPOSE_VERSION);
}

#[test]
fn missing_docker_keeps_the_shell_not_found_code() {
    let err = spawn_error(&std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "no such file",
    ));
    assert!(err.to_string().contains("not found on PATH"));
    match err.downcast_ref::<ChapError>() {
        Some(ChapError::DockerFailed(code)) => assert_eq!(*code, DOCKER_NOT_FOUND),
        other => panic!("wrong error: {other:?}"),
    }
}

#[test]
fn a_refused_query_keeps_dockers_first_words() {
    let failure = QueryFailure::refused(
        "ps -a --format json",
        1,
        "\n  error during connect: open //./pipe/dockerDesktopLinuxEngine: \
             the system cannot find the file specified.\nrun `docker context ls`\n",
    );
    assert!(failure.ran());
    assert_eq!(
        failure.to_string(),
        "`docker compose ps -a --format json` exited with status 1: error during connect: \
             open //./pipe/dockerDesktopLinuxEngine: the system cannot find the file specified."
    );
}

#[test]
fn a_query_refused_without_a_word_is_still_the_status() {
    let failure = QueryFailure::refused("ps", 1, "  \n\n");
    assert_eq!(
        failure.to_string(),
        "`docker compose ps` exited with status 1"
    );
}

#[test]
fn a_docker_that_is_not_there_never_answered() {
    let failure = QueryFailure::unusable(
        "ps",
        &std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
    );
    assert!(!failure.ran());
    assert!(
        failure
            .to_string()
            .starts_with("`docker` could not be run: "),
        "{failure}"
    );
}

#[test]
fn other_spawn_failures_are_plain_errors() {
    let err = spawn_error(&std::io::Error::other("boom"));
    assert!(err.downcast_ref::<ChapError>().is_none());
    assert!(err.to_string().contains("could not run"));
}

#[test]
fn ps_carries_the_image_reference_the_container_was_made_from() {
    let text = r#"{"Service":"chap","Name":"x-chap-1","ID":"ca78","CreatedAt":"now","State":"running","Image":"ghcr.io/dhis2-chap/chap:v2.3.1"}"#;
    let found = containers(text);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].image, "ghcr.io/dhis2-chap/chap:v2.3.1");
    // A `ps` from a compose that does not print it is not an error.
    assert_eq!(containers(r#"{"Service":"chap"}"#)[0].image, "");
}

#[test]
fn the_service_images_come_from_the_merged_configuration() {
    let text = r#"{
          "name": "chapx",
          "services": {
            "chap":   {"image": "ghcr.io/dhis2-chap/chap:v2.3.1"},
            "worker": {"image": "ghcr.io/dhis2-chap/chap:v2.3.1"},
            "local":  {"build": {"context": "."}},
            "built":  {"build": {"context": "."}, "image": "x-chap:checkout",
                       "pull_policy": "build"}
          }
        }"#;
    let images = parse_service_images(text);
    assert_eq!(images.len(), 2, "a built service pins no image");
    assert!(!images.contains_key("built"), "{images:?}");
    assert_eq!(
        images.get("chap").map(String::as_str),
        Some("ghcr.io/dhis2-chap/chap:v2.3.1")
    );
    assert_eq!(images.get("worker"), images.get("chap"));
    assert!(parse_service_images("not json").is_empty());
    assert!(parse_service_images("{}").is_empty());
}

#[test]
fn the_config_hashes_are_one_service_per_line() {
    let text = "chap 8b08690bc131\nworker 2aaae2717d12\n";
    let hashes = parse_config_hashes(text);
    assert_eq!(hashes.get("chap").map(String::as_str), Some("8b08690bc131"));
    assert_eq!(
        hashes.get("worker").map(String::as_str),
        Some("2aaae2717d12")
    );
    // Anything that is not a pair is not a hash.
    assert!(parse_config_hashes("chap\n\n   \n").is_empty());
}

#[test]
fn an_inspected_container_yields_its_image_and_its_config_hash() {
    let text = "abc123def456\tsha256:aaa\t{\"com.docker.compose.config-hash\":\"h1\",\
                    \"com.docker.compose.project\":\"chapx\"}\n\
                    fff000\tsha256:bbb\t{}\n";
    let found = parse_container_builds(text);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].0, "abc123def456");
    assert_eq!(found[0].1.image_id, "sha256:aaa");
    assert_eq!(found[0].1.config_hash, "h1");
    // No label, no hash: unknown, not empty-and-therefore-changed.
    assert_eq!(found[1].1.config_hash, "");
    // Labels that are not JSON at all cost the hash and nothing else.
    let odd = parse_container_builds("abc\tsha256:ccc\tnot json\n");
    assert_eq!(odd[0].1.image_id, "sha256:ccc");
    assert_eq!(odd[0].1.config_hash, "");
    assert!(parse_container_builds("").is_empty());
}

#[test]
fn an_image_reports_the_digest_it_was_pulled_at() {
    let text = r#"["ghcr.io/dhis2-chap/chap-core@sha256:7f3a1c2e9b4d5a6b7c8d9e0f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d"]"#;
    assert_eq!(
        parse_repo_digest(text).as_deref(),
        Some("sha256:7f3a1c2e9b4d5a6b7c8d9e0f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d")
    );
    // An image built here rather than pulled carries no digest at all.
    assert_eq!(parse_repo_digest("[]"), None);
    assert_eq!(parse_repo_digest("null"), None);
    assert_eq!(parse_repo_digest(""), None);
    assert_eq!(parse_repo_digest(r#"["ghcr.io/x/y:dev"]"#), None);
}

#[test]
fn a_digest_is_shortened_to_the_characters_a_person_compares() {
    assert_eq!(
        short_digest("sha256:7f3a1c2e9b4d5a6b7c8d9e0f1a2b3c4d"),
        "7f3a1c2e9b4d"
    );
    assert_eq!(short_digest("  sha256:abc  "), "abc");
    assert_eq!(short_digest("7f3a1c2e9b4d5a6b"), "7f3a1c2e9b4d");
    assert_eq!(short_digest(""), "");
}

/// One `docker image inspect` line, the way the real format prints it.
fn config_line(user: &str, working_dir: &str, entrypoint: &str, cmd: &str) -> String {
    format!("{user}\t{working_dir}\t{entrypoint}\t{cmd}\n")
}

/// The whole point of asking for four fields: an empty config is an
/// answer about a platform this machine does not hold, not an image that
/// runs as root.
#[test]
fn an_empty_image_config_is_unknown_rather_than_root() {
    // What Docker on the containerd image store prints for a multi-arch
    // image whose amd64 variant is the only one pulled, asked about
    // arm64.
    assert_eq!(
        parse_image_config(&config_line("", "", "null", "null")),
        None
    );
    assert_eq!(parse_image_config("\t\t\t\n"), None);
    assert_eq!(parse_image_config(&config_line("", "", "[]", "[]")), None);

    // The same image asked about the platform it does hold.
    assert_eq!(
        parse_image_config(&config_line(
            "chap",
            "/app",
            "[\"/usr/bin/tini\",\"--\"]",
            "[\"python\",\"-m\",\"m\"]"
        )),
        Some(("chap".to_string(), "/app".to_string()))
    );

    // Root by omission is a real config: it has a working directory, or a
    // command, or both.
    assert_eq!(
        parse_image_config(&config_line("", "/work", "null", "null")),
        Some((String::new(), "/work".to_string()))
    );
    assert_eq!(
        parse_image_config(&config_line("", "", "null", "[\"/bin/sh\"]")),
        Some((String::new(), String::new()))
    );

    // A line with nothing on it at all is no answer either way.
    assert_eq!(parse_image_config(""), None);
    assert_eq!(parse_image_config("chap\n"), None, "no tab, no answer");
}

/// The platform is pinned, because the host's own architecture is the
/// wrong question for an amd64-only image - and a docker too old for the
/// flag is asked again without it.
#[test]
fn the_image_config_is_asked_for_amd64_and_retried_without_the_flag() {
    let runs = std::cell::RefCell::new(Vec::new());
    let answer = |args: &[String]| {
        runs.borrow_mut().push(args.to_vec());
        Some(DockerRun {
            ok: true,
            code: Some(0),
            stdout: config_line("chap", "/app", "null", "null"),
            stderr: String::new(),
        })
    };
    assert_eq!(
        image_config_with("img:tag", &answer),
        Some(("chap".to_string(), "/app".to_string()))
    );
    let asked = runs.borrow().clone();
    assert_eq!(asked.len(), 1, "one run when the flag is understood");
    assert!(
        asked[0].windows(2).any(|pair| pair
            == [
                "--platform".to_string(),
                crate::compose::AMD64_PLATFORM.to_string()
            ]),
        "{asked:?}"
    );
    assert_eq!(asked[0].last().map(String::as_str), Some("img:tag"));

    // A CLI older than `--platform` for `image inspect` (before Docker
    // 25) refuses the whole command; the answer without the flag is
    // still better than none.
    let runs = std::cell::RefCell::new(Vec::new());
    let old = |args: &[String]| {
        runs.borrow_mut().push(args.to_vec());
        let knows = !args.iter().any(|a| a == "--platform");
        Some(DockerRun {
            ok: knows,
            code: Some(if knows { 0 } else { 125 }),
            stdout: if knows {
                config_line("chap", "/app", "null", "null")
            } else {
                String::new()
            },
            stderr: if knows {
                String::new()
            } else {
                "unknown flag: --platform\nSee 'docker image inspect --help'.\n".to_string()
            },
        })
    };
    assert_eq!(
        image_config_with("img:tag", &old),
        Some(("chap".to_string(), "/app".to_string()))
    );
    let asked = runs.borrow().clone();
    assert_eq!(asked.len(), 2, "the pinned ask, then the plain one");
    assert!(!asked[1].iter().any(|a| a == "--platform"), "{asked:?}");
    assert_eq!(asked[1].last().map(String::as_str), Some("img:tag"));

    // A daemon whose API is older than the flag is the same case: the
    // refusal is about the flag, not the image.
    let runs = std::cell::RefCell::new(Vec::new());
    let old_api = |args: &[String]| {
        runs.borrow_mut().push(args.to_vec());
        let knows = !args.iter().any(|a| a == "--platform");
        Some(DockerRun {
            ok: knows,
            code: Some(if knows { 0 } else { 1 }),
            stdout: if knows {
                config_line("chap", "/app", "null", "null")
            } else {
                String::new()
            },
            stderr: if knows {
                String::new()
            } else {
                "\"--platform\" requires API version 1.49, but the Docker daemon API \
                     version is 1.43\n"
                    .to_string()
            },
        })
    };
    assert_eq!(
        image_config_with("img:tag", &old_api),
        Some(("chap".to_string(), "/app".to_string()))
    );
    assert_eq!(runs.borrow().len(), 2);

    // A refusal about the image itself is a real answer: no second ask,
    // and the caller hears "we do not have it".
    let runs = std::cell::RefCell::new(Vec::new());
    let missing = |args: &[String]| {
        runs.borrow_mut().push(args.to_vec());
        Some(DockerRun {
            ok: false,
            code: Some(1),
            stdout: String::new(),
            stderr: "Error response from daemon: image with reference img:tag was found \
                         but does not provide the specified platform (linux/amd64)\n"
                .to_string(),
        })
    };
    assert_eq!(image_config_with("img:tag", &missing), None);
    assert_eq!(runs.borrow().len(), 1);

    // And no docker at all is one `None`, not a retry loop.
    let runs = std::cell::RefCell::new(0usize);
    let no_docker = |_: &[String]| {
        *runs.borrow_mut() += 1;
        None
    };
    assert_eq!(image_config_with("img:tag", &no_docker), None);
    assert_eq!(*runs.borrow(), 1);
}

#[test]
fn strip_ansi_removes_colour_and_cursor_escapes() {
    let line = b"\x1b[32mINFO\x1b[0m model \x1b[1;31mready\x1b[0m\n";
    assert_eq!(strip_ansi(line), b"INFO model ready\n");
    assert_eq!(strip_ansi(b"\x1b[2K\x1b[1Gdone"), b"done");
}

#[test]
fn strip_ansi_removes_osc_and_two_byte_escapes() {
    assert_eq!(strip_ansi(b"\x1b]0;title\x07text"), b"text");
    assert_eq!(strip_ansi(b"\x1b]8;;http://x\x1b\\link"), b"link");
    assert_eq!(strip_ansi(b"a\x1b(Bb"), b"ab");
}

#[test]
fn strip_ansi_keeps_plain_text_and_drops_a_cut_off_escape() {
    assert_eq!(strip_ansi(b"plain text\n"), b"plain text\n");
    assert_eq!(strip_ansi("ål ✓\n".as_bytes()), "ål ✓\n".as_bytes());
    assert_eq!(strip_ansi(b"end\x1b[3"), b"end");
    assert_eq!(strip_ansi(b"end\x1b"), b"end");
}

#[test]
fn stats_lines_are_read_by_container_name() {
    let text = r#"{"Name":"default-ab12cd-chapkit-ewars-model-1","CPUPerc":"1.50%","MemUsage":"181.2MiB / 7.654GiB"}
not json
{"Name":"other-1","CPUPerc":"0.00%"}"#;
    let usage = super::stats::parse_stats(text);
    assert_eq!(usage.len(), 2);
    assert_eq!(
        usage["default-ab12cd-chapkit-ewars-model-1"],
        Usage {
            cpu: "1.50%".to_string(),
            memory: "181.2MiB / 7.654GiB".to_string(),
        }
    );
    assert_eq!(usage["other-1"].memory, "");
}

#[test]
fn a_label_list_keeps_commas_inside_values() {
    let labels = super::labels::parse_label_list(
        "com.docker.compose.project=demo-ab12cd,com.docker.compose.project.config_files=/d/compose.yml,/d/compose.chap-devops.yml,io.github.dhis2-chap.chap-devops.role=chap-core",
    );
    assert_eq!(labels["com.docker.compose.project"], "demo-ab12cd");
    assert_eq!(
        labels["com.docker.compose.project.config_files"],
        "/d/compose.yml,/d/compose.chap-devops.yml"
    );
    assert_eq!(labels["io.github.dhis2-chap.chap-devops.role"], "chap-core");
}

#[test]
fn docker_desktop_port_labels_are_keys_of_their_own() {
    let labels = super::labels::parse_label_list(
        "com.docker.compose.project.config_files=/d/compose.yml,/d/compose.chap-devops.yml,io.github.dhis2-chap.chap-devops.role=model,desktop.docker.io/ports.scheme=v2,desktop.docker.io/ports/8000/tcp=127.0.0.1:5001",
    );
    assert_eq!(labels["io.github.dhis2-chap.chap-devops.role"], "model");
    assert_eq!(labels["desktop.docker.io/ports.scheme"], "v2");
    assert_eq!(labels["desktop.docker.io/ports/8000/tcp"], "127.0.0.1:5001");
    assert_eq!(
        labels["com.docker.compose.project.config_files"],
        "/d/compose.yml,/d/compose.chap-devops.yml"
    );
}

#[test]
fn labeled_containers_say_their_health() {
    let text = r#"{"ID":"abc","Names":"demo-chap-1","State":"running","Status":"Up 5 minutes (healthy)","Labels":"io.github.dhis2-chap.chap-devops.role=chap-core"}
{"ID":"def","Names":"demo-m-1","State":"exited","Status":"Exited (1) 2 minutes ago","Labels":"io.github.dhis2-chap.chap-devops.role=model,io.github.dhis2-chap.chap-devops.model=m"}"#;
    let rows = super::labels::parse_labeled(text);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].health(), Some("healthy"));
    assert_eq!(rows[1].health(), None);
    assert_eq!(
        rows[1].label("io.github.dhis2-chap.chap-devops.model"),
        Some("m")
    );
}

#[test]
fn the_command_is_the_entrypoint_then_the_cmd() {
    assert_eq!(
        parse_command("[\"/usr/bin/tini\",\"--\"]\t[\"python\",\"-m\",\"m\"]\n"),
        Some(
            ["/usr/bin/tini", "--", "python", "-m", "m"]
                .map(str::to_string)
                .to_vec()
        )
    );
    assert_eq!(
        parse_command("null\t[\"uvicorn\",\"main:app\"]"),
        Some(["uvicorn", "main:app"].map(str::to_string).to_vec())
    );
    assert_eq!(parse_command("null\tnull"), Some(Vec::new()));
    assert_eq!(parse_command("not json\tnull"), None);
}

/// No progress lines into a log; a terminal and `-vv` keep them.
#[test]
fn compose_prints_no_progress_without_a_terminal() {
    assert!(quiet_progress(false, false));
    assert!(
        !quiet_progress(true, false),
        "a terminal keeps the progress"
    );
    assert!(!quiet_progress(false, true), "-vv keeps the lines");
}

/// `Publishers` lists a binding per address family; a port of 0 is a
/// container port that is not published on the host.
#[test]
fn the_published_host_ports_are_read_once_each() {
    let text = concat!(
        r#"{"ID":"a","Service":"m","State":"running","Publishers":["#,
        r#"{"URL":"0.0.0.0","TargetPort":8000,"PublishedPort":5001,"Protocol":"tcp"},"#,
        r#"{"URL":"::","TargetPort":8000,"PublishedPort":5001,"Protocol":"tcp"}]}"#,
        "\n",
        r#"{"ID":"b","Service":"n","State":"running","Publishers":["#,
        r#"{"URL":"","TargetPort":8000,"PublishedPort":0,"Protocol":"tcp"}]}"#,
        "\n",
        r#"{"ID":"c","Service":"o","State":"exited"}"#,
        "\n"
    );
    let found = containers(text);
    assert_eq!(found[0].published, vec![5001]);
    assert!(found[1].published.is_empty());
    assert!(found[2].published.is_empty());
}

#[test]
fn docker_info_gives_the_cpus_and_the_memory_of_the_engine() {
    use super::stats::parse_engine_resources;
    assert_eq!(
        parse_engine_resources("12 33598132224\n"),
        Some((12, 33598132224))
    );
    assert_eq!(parse_engine_resources(""), None);
    assert_eq!(parse_engine_resources("x 1"), None);
}
