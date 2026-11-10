use super::*;
use crate::manual::Names;
use crate::manual::source::Source;
use crate::project::ProjectState;

fn names_of(id: &str, service_id: &str) -> Names {
    Names {
        source: Source::parse("https://github.com/chap-models/chapkit_ghr_model").unwrap(),
        id: id.to_string(),
        service_id: service_id.to_string(),
        display_name: "chapkit_ghr_model".into(),
    }
}

fn resolved(id: &str, service_id: &str) -> Resolved {
    Resolved {
        reads_port: false,
        source: Source::parse("https://github.com/chap-models/chapkit_ghr_model").unwrap(),
        id: id.to_string(),
        service_id: service_id.to_string(),
        display_name: "chapkit_ghr_model".into(),
        image: "ghcr.io/chap-models/chapkit_ghr_model".into(),
        tag: "sha-b1d6c31".into(),
        commit: Some("b1d6c31f4b2f0d8a0f4c6e2a5e9d3c7b8a1f0e2d".into()),
        follow: Some("main".into()),
        data_dir: "/work/data".into(),
        data_dir_from: Origin::Image,
        user: "10001:10001".into(),
        user_from: Origin::Probe,
        runtime_amd64: true,
        notes: Vec::new(),
    }
}

fn project_with(manual: &[(&str, &str)]) -> Project {
    let mut state = ProjectState::default();
    for (id, service_id) in manual {
        state.manual.insert(
            id.to_string(),
            ManualModel {
                reads_port: false,
                service_id: service_id.to_string(),
                display_name: id.to_string(),
                repository: Some(format!("https://github.com/someone/{id}")),
                image: format!("ghcr.io/someone/{id}"),
                tag: "sha-0000000".into(),
                commit: None,
                follow: None,
                data_dir: None,
                user: None,
                runtime_amd64: false,
                added: "2026-09-24".into(),
            },
        );
    }
    Project {
        dir: std::path::PathBuf::from("/tmp/chapx"),
        state,
    }
}

#[test]
fn an_id_the_marketplace_lists_is_a_model_to_enable() {
    let marketplace = crate::registry::load_embedded().unwrap();
    let project = project_with(&[]);
    let err = check_free(
        &project,
        &marketplace,
        &names_of("chapkit_ewars_model", "x-service"),
    )
    .expect_err("the marketplace lists it");
    assert!(err.to_string().contains("models enable"), "{err}");

    let err = check_free(
        &project,
        &marketplace,
        &names_of("something_else", "chapkit-ewars-model"),
    )
    .expect_err("the service name is taken");
    assert!(err.to_string().contains("--service-id"), "{err}");
    // The image may well be that marketplace model, registering under the
    // id it was built with, and then enabling it is the answer.
    assert!(
        err.to_string()
            .contains("`chap-devops models enable chapkit_ewars_model` runs it"),
        "{err}"
    );
}

#[test]
fn an_id_this_deployment_added_is_removed_before_it_is_added_again() {
    let marketplace = crate::registry::load_embedded().unwrap();
    // An id the marketplace does not list, so the only thing in the way
    // is what this deployment added for itself.
    let project = project_with(&[(
        "chapkit_example_manual_model",
        "chapkit-example-manual-model",
    )]);

    let err = check_free(
        &project,
        &marketplace,
        &names_of("chapkit_example_manual_model", "chapkit-example-manual-2"),
    )
    .expect_err("already added");
    assert!(err.to_string().contains("models remove"), "{err}");

    let err = check_free(
        &project,
        &marketplace,
        &names_of("other_model", "chapkit-example-manual-model"),
    )
    .expect_err("the service name is taken");
    assert!(err.to_string().contains("--service-id"), "{err}");

    // A free pair goes through.
    check_free(
        &project,
        &marketplace,
        &names_of("other_model", "other-model"),
    )
    .unwrap();
}

#[test]
fn a_manual_model_answers_to_its_id_and_its_service_name() {
    let project = project_with(&[("chapkit_ghr_model", "chapkit-ghr-model")]);
    assert_eq!(
        manual_id(&project, "chapkit_ghr_model").as_deref(),
        Some("chapkit_ghr_model")
    );
    assert_eq!(
        manual_id(&project, "chapkit-ghr-model").as_deref(),
        Some("chapkit_ghr_model")
    );
    assert_eq!(manual_id(&project, "chapkit_ewars_model"), None);
}

#[test]
fn the_added_block_says_where_every_value_came_from() {
    let render = |resolved: &Resolved| {
        let mut lines = Report::default();
        added_block(resolved, &mut lines);
        lines.text()
    };
    let text = render(&resolved("chapkit_ghr_model", "chapkit-ghr-model"));
    assert!(text.starts_with("added chapkit_ghr_model (chapkit-ghr-model)\n"));
    for line in [
        "hint: source: https://github.com/chap-models/chapkit_ghr_model\n",
        "hint: image: ghcr.io/chap-models/chapkit_ghr_model:sha-b1d6c31\n",
        "hint: pin: sha-b1d6c31 (commit b1d6c31)\n",
        "hint: follows: main (`chap-devops update` moves the pin)\n",
        "hint: data dir: /work/data (from the image config)\n",
        "hint: user: 10001:10001 (from a docker probe)\n",
    ] {
        assert!(text.contains(line), "{line} in {text}");
    }

    // A pinned entry says so where the branch would have been.
    let pinned = Resolved {
        follow: None,
        commit: None,
        ..resolved("chapkit_ghr_model", "chapkit-ghr-model")
    };
    let text = render(&pinned);
    assert!(text.contains("hint: follows: nothing (pinned)\n"), "{text}");
    assert!(text.contains("hint: pin: sha-b1d6c31\n"), "{text}");
}

#[test]
fn the_date_is_the_day_in_utc() {
    let date = today();
    assert_eq!(date.len(), 10, "{date}");
    assert_eq!(date.matches('-').count(), 2, "{date}");
    assert_eq!(short("b1d6c31f4b2f0d8a"), "b1d6c31");
}

#[test]
fn a_source_added_before_is_found_again_under_its_id() {
    let mut project = project_with(&[("thing_2", "thing-2")]);
    let url = "https://github.com/someone/thing_2";
    assert_eq!(added_as(&project, url, None).as_deref(), Some("thing_2"));
    assert_eq!(
        added_as(&project, &format!("{url}.git/"), Some("auto")).as_deref(),
        Some("thing_2")
    );
    // `--id` naming another entry asks for a copy beside it.
    assert_eq!(added_as(&project, url, Some("thing_3")), None);
    assert_eq!(
        added_as(&project, url, Some("thing_2")).as_deref(),
        Some("thing_2")
    );

    // An image is the same entry only at the same tag, and only an entry
    // that was added from the image rather than from a repository.
    let image = "ghcr.io/someone/thing_2:sha-0000000";
    assert_eq!(added_as(&project, image, None), None);
    project.state.manual.get_mut("thing_2").unwrap().repository = None;
    assert_eq!(added_as(&project, image, None).as_deref(), Some("thing_2"));
    assert_eq!(
        added_as(&project, "ghcr.io/someone/thing_2:sha-1111111", None),
        None
    );
    assert_eq!(added_as(&project, "chapkit_ewars_model", None), None);
}
