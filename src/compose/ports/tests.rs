use super::*;
use crate::project::DEFAULT_PORT_RANGE;

/// Nothing is listening on anything.
fn all_free(_: u16) -> bool {
    false
}

#[test]
fn allocate_hands_out_the_lowest_free_port() {
    let mut alloc = PortAllocator::new((5001, 5999), [5001, 5002, 5004]);
    assert_eq!(alloc.allocate(&all_free).unwrap(), 5003);
    assert_eq!(alloc.allocate(&all_free).unwrap(), 5005);
    assert_eq!(alloc.allocate(&all_free).unwrap(), 5006);
    assert!(alloc.used().contains(&5003));
    assert_eq!(alloc.range(), (5001, 5999));
}

#[test]
fn allocate_skips_a_port_something_is_listening_on() {
    // 5001 is free in every compose file and still unusable.
    let busy = |port: u16| (5001..=5002).contains(&port);
    let mut alloc = PortAllocator::new((5001, 5999), []);
    assert_eq!(alloc.allocate(&busy).unwrap(), 5003);
    assert!(
        !alloc.is_used(5001),
        "a port the host holds is not ours to record"
    );
}

#[test]
fn allocate_runs_out_at_the_top_of_the_range() {
    let mut alloc = PortAllocator::new((5001, 5002), []);
    assert_eq!(alloc.allocate(&all_free).unwrap(), 5001);
    assert_eq!(alloc.allocate(&all_free).unwrap(), 5002);
    let err = alloc
        .allocate(&all_free)
        .expect_err("the range is exhausted");
    assert!(err.to_string().contains("5001-5002"));

    // A range that is entirely busy on the host runs out the same way.
    let mut alloc = PortAllocator::new((5001, 5002), []);
    let err = alloc
        .allocate(&|_| true)
        .expect_err("the whole range is listening");
    assert!(err.to_string().contains("this machine"));
}

#[test]
fn claim_rejects_a_taken_port_and_one_outside_the_range() {
    let mut alloc = PortAllocator::new(DEFAULT_PORT_RANGE, [5002]);
    alloc.claim(5005, &all_free).unwrap();
    assert!(alloc.is_used(5005));

    let err = alloc.claim(5002, &all_free).expect_err("5002 is taken");
    assert!(matches!(
        err.downcast_ref::<ChapError>(),
        Some(ChapError::PortInUse {
            port: 5002,
            holder: PortHolder::ComposeFile
        })
    ));
    let err = alloc.claim(5005, &all_free).expect_err("just claimed");
    assert!(matches!(
        err.downcast_ref::<ChapError>(),
        Some(ChapError::PortInUse {
            port: 5005,
            holder: PortHolder::ComposeFile
        })
    ));

    for out in [80, 5000, 6000] {
        let err = alloc.claim(out, &all_free).expect_err("outside the range");
        assert!(
            matches!(err.downcast_ref::<ChapError>(), Some(ChapError::PortOutOfRange { port, .. }) if *port == out),
            "port {out}"
        );
        // The message names the range and every way out of it.
        let text = err.to_string();
        assert!(text.contains("range 5001-5999"), "{text}");
        assert!(text.contains("`--port auto`"), "{text}");
        assert!(
            text.contains("`port_range` in `.chap-devops/project.yaml`"),
            "{text}"
        );
    }
}

#[test]
fn claim_says_when_the_host_is_the_one_holding_the_port() {
    let mut alloc = PortAllocator::new(DEFAULT_PORT_RANGE, []);
    let err = alloc
        .claim(5010, &|port| port == 5010)
        .expect_err("something is listening on 5010");
    assert!(matches!(
        err.downcast_ref::<ChapError>(),
        Some(ChapError::PortInUse {
            port: 5010,
            holder: PortHolder::Host
        })
    ));
    assert!(
        err.to_string().contains("something is listening"),
        "{err:#}"
    );
    assert!(!alloc.is_used(5010), "a rejected claim records nothing");
}

#[test]
fn reserve_ignores_the_range() {
    // A port a project recorded before its range was narrowed is still
    // taken, and re-recording it must not fail over the range.
    let mut alloc = PortAllocator::new((5001, 5999), []);
    alloc.reserve(9090);
    assert!(alloc.is_used(9090));
    alloc.reserve(5001);
    assert_eq!(alloc.allocate(&all_free).unwrap(), 5002);
}

#[test]
fn short_form_entries_yield_the_host_side() {
    assert_eq!(host_port_of_short_form("5002:8000"), Some(5002));
    assert_eq!(host_port_of_short_form("5002:8000/tcp"), Some(5002));
    assert_eq!(host_port_of_short_form("127.0.0.1:5003:8000"), Some(5003));
    assert_eq!(host_port_of_short_form("[::1]:5004:8000"), Some(5004));
    assert_eq!(host_port_of_short_form("[::1]:5004:8000/tcp"), Some(5004));
    assert_eq!(host_port_of_short_form("0.0.0.0:5004:8000/udp"), Some(5004));
    assert_eq!(host_port_of_short_form("8000"), None);
    assert_eq!(host_port_of_short_form("5000-5010:8000"), None);
}

#[test]
fn scan_compose_dir_reads_every_compose_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("compose.yml"),
        concat!(
            "services:\n",
            "  chap:\n",
            "    image: chap\n",
            "    ports:\n",
            "      - \"8000:8000\"\n",
            "  quiet:\n",
            "    image: quiet\n",
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("compose.models.yml"),
        concat!(
            "services:\n",
            "  short:\n",
            "    ports:\n",
            "      - \"5002:8000\"\n",
            "      - \"5003:8000/tcp\"\n",
            "      - \"127.0.0.1:5004:8000\"\n",
            "      - \"8000\"\n",
            "  long:\n",
            "    ports:\n",
            "      - target: 8000\n",
            "        published: \"5005\"\n",
            "        protocol: tcp\n",
            "      - target: 9000\n",
            "        published: 5006\n",
        ),
    )
    .unwrap();
    // Not a compose file: must not be read.
    std::fs::write(
        dir.path().join("notes.yml"),
        "services:\n  x:\n    ports:\n      - \"5111:8000\"\n",
    )
    .unwrap();
    // A compose file that does not parse is skipped, not fatal.
    std::fs::write(dir.path().join("compose.broken.yml"), "services: [\n").unwrap();
    // An empty umbrella parses to null.
    std::fs::write(dir.path().join("compose.marketplace.yml"), "services: {}\n").unwrap();

    let ports = PortAllocator::scan_compose_dir(dir.path()).unwrap();
    assert_eq!(
        ports,
        BTreeSet::from([8000, 5002, 5003, 5004, 5005, 5006]),
        "unexpected scan result"
    );
}

#[test]
fn scan_compose_dir_sees_nothing_in_an_internal_only_overlay() {
    // The golden overlay publishes no host port at all: it only exposes
    // 8000 on the compose network, and its volume init service publishes
    // nothing either.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("compose.chapkit-ewars-model.yml"),
        crate::compose::render::normalize_newlines(include_str!(
            "../../../tests/fixtures/compose.chapkit-ewars-model.yml"
        )),
    )
    .unwrap();
    assert!(
        PortAllocator::scan_compose_dir(dir.path())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn scan_compose_dir_on_a_missing_directory_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let ports = PortAllocator::scan_compose_dir(&dir.path().join("nope")).unwrap();
    assert!(ports.is_empty());
}

#[test]
fn a_compose_override_tag_does_not_hide_its_port_list() {
    // `ports: !override` is the only way to replace a mapping across -f
    // files, and compose.chap-devops.yml uses it; the scanner has to see through
    // the tag rather than skip the file.
    let doc: Value = serde_yaml_ng::from_str(
        "services:\n  chap:\n    ports: !override\n      - \"8010:8000\"\n",
    )
    .unwrap();
    assert_eq!(service_host_ports(&doc), vec![("chap".to_string(), 8010)]);

    // A published port that is a compose variable is no number, so it is
    // left out rather than guessed at.
    let doc: Value = serde_yaml_ng::from_str(
        "services:\n  chap:\n    ports: !override\n      - \"${CHAP_API_PORT:-8000}:8000\"\n",
    )
    .unwrap();
    assert!(service_host_ports(&doc).is_empty());
}

#[test]
fn published_ports_reads_only_the_files_it_is_given() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("compose.yml"),
        "services:\n  chap:\n    ports:\n      - \"8000:8000\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("compose.model.yml"),
        concat!(
            "services:\n",
            "  model:\n",
            "    expose:\n      - \"8000\"\n",
            "    ports:\n      - \"5001:8000\"\n",
            "  model-init:\n",
            "    image: busybox\n",
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("compose.stranger.yml"),
        "services:\n  stranger:\n    ports:\n      - \"5999:8000\"\n",
    )
    .unwrap();

    let files = vec![
        "compose.yml".to_string(),
        "compose.model.yml".to_string(),
        "compose.gone.yml".to_string(),
    ];
    assert_eq!(
        published_ports(dir.path(), &files),
        vec![("chap".to_string(), 8000), ("model".to_string(), 5001)],
        "a missing file is skipped and an unlisted one is never read"
    );
    assert!(published_ports(dir.path(), &[]).is_empty());
}

#[test]
fn allocator_for_seeds_from_the_state_and_the_directory() {
    use crate::project::{EnabledModel, ProjectState};
    use std::collections::BTreeMap;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("compose.mine.yml"),
        "services:\n  mine:\n    ports:\n      - \"5002:8000\"\n",
    )
    .unwrap();
    let model = EnabledModel {
        previous: None,
        reads_port: false,
        service_id: "m".into(),
        image: "ghcr.io/x".into(),
        image_tag: "sha-1111111".into(),
        version: "1.0.0".into(),
        channel: None,
        host_port: Some(5001),
        bind: None,
        data_dir: "/app/data".into(),
        user: "chapkit:chapkit".into(),
        user_from: Default::default(),
        platform: None,
        compose_file: "compose.m.yml".into(),
    };
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState {
            models: BTreeMap::from([("m".to_string(), model)]),
            ..ProjectState::default()
        },
    };

    let mut alloc = allocator_for(&project, &BTreeSet::new()).unwrap();
    assert_eq!(alloc.allocate(&all_free).unwrap(), 5003);

    // Freeing the model's own port makes it available again, which is how
    // a model keeps its port across a rewrite.
    let mut alloc = allocator_for(&project, &BTreeSet::from([5001])).unwrap();
    assert_eq!(alloc.allocate(&all_free).unwrap(), 5001);
}

/// chap-core's API port as `.env` sets it is not a number the compose scan
/// can read, so the allocator reserves it: a model never gets it.
#[test]
fn the_allocator_never_hands_out_the_api_port() {
    use crate::project::ProjectState;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".env"), "CHAP_API_PORT=5001\n").unwrap();
    let project = Project {
        dir: dir.path().to_path_buf(),
        state: ProjectState::default(),
    };
    assert!(project.state.components.chap_core.enabled);
    let mut allocator = allocator_for(&project, &BTreeSet::new()).unwrap();
    assert_eq!(allocator.allocate(&|_| false).unwrap(), 5002);
}
