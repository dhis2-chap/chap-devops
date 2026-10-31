use super::*;

fn render(build: impl FnOnce(&mut Report)) -> String {
    let mut lines = Report::default();
    build(&mut lines);
    lines.text()
}
use crate::project::{EnabledModel, ProjectState};
use std::collections::BTreeMap;

fn project_with_ewars(host_port: Option<u16>) -> Project {
    let model = EnabledModel {
        previous: None,
        reads_port: false,
        service_id: "chapkit-ewars-model".into(),
        image: "ghcr.io/chap-models/chapkit_ewars_model".into(),
        image_tag: "sha-fa880a1".into(),
        version: "1.0.0".into(),
        channel: Some(Channel::Stable),
        host_port,
        bind: None,
        data_dir: "/app/data".into(),
        user: "chapkit:chapkit".into(),
        user_from: Default::default(),
        platform: Some("linux/amd64".into()),
        compose_file: "compose.chapkit-ewars-model.yml".into(),
    };
    Project {
        dir: std::path::PathBuf::from("/tmp/chapx"),
        state: ProjectState {
            models: BTreeMap::from([("chapkit_ewars_model".to_string(), model)]),
            ..ProjectState::default()
        },
    }
}

/// What `--purge` calls a model that `.chap-devops/models.yaml` no longer holds,
/// and therefore which volume it removes.
#[test]
fn a_purge_names_the_volume_by_the_marketplace_id() {
    let registry = crate::registry::load_embedded().expect("the embedded snapshot parses");
    for typed in ["chapkit_ewars_model", "chapkit-ewars-model"] {
        let listed = registry.get(typed).map(|model| model.id.as_str());
        assert_eq!(purge_id(listed, typed), "chapkit_ewars_model");
        assert_eq!(
            crate::compose::volume_name(&purge_id(listed, typed)),
            "ck_chapkit_ewars_model_data"
        );
    }

    // An entry the marketplace no longer lists is only the name that was
    // typed: a service id folds back to the id that named the volume, and
    // a marketplace id is already it.
    assert_eq!(purge_id(None, "left-the-market"), "left_the_market");
    assert_eq!(purge_id(None, "left_the_market"), "left_the_market");
}

#[test]
fn enabled_id_accepts_both_identifiers() {
    let project = project_with_ewars(None);
    assert_eq!(
        enabled_id(&project, "chapkit_ewars_model").as_deref(),
        Some("chapkit_ewars_model")
    );
    assert_eq!(
        enabled_id(&project, "chapkit-ewars-model").as_deref(),
        Some("chapkit_ewars_model")
    );
    assert_eq!(enabled_id(&project, "auto_arima_chapkit"), None);
}

/// An `enabled` report for the one model `project_with_ewars` holds.
fn enabled_report(project: &Project) -> ApplyReport {
    ApplyReport {
        enabled: vec![(
            "chapkit_ewars_model".to_string(),
            project.state.models["chapkit_ewars_model"].clone(),
        )],
        written: vec![project.dir.join("compose.chapkit-ewars-model.yml")],
        ..ApplyReport::default()
    }
}

/// An enable that asked for what the project already has says so, and has
/// no apply step.
#[test]
fn an_unchanged_enable_says_nothing_changed() {
    let project = project_with_ewars(None);
    let report = ApplyReport {
        unchanged: vec![(
            "chapkit_ewars_model".to_string(),
            project.state.models["chapkit_ewars_model"].clone(),
        )],
        ..ApplyReport::default()
    };
    let text = render(|lines| summary(&report, &[], &project, lines));
    assert!(
        text.starts_with(
            "chapkit_ewars_model v1.0.0 is already enabled at \
             http://localhost:8700/v2/services/chapkit-ewars-model/run/; nothing changed\n"
        ),
        "{text}"
    );
    assert!(!text.contains("chap-devops up"), "{text}");
}

/// A change to the state that rendered no compose file differently, such as
/// a new channel that points at the same version, has nothing for `up`.
#[test]
fn an_update_that_wrote_no_compose_file_has_no_apply_step() {
    let project = project_with_ewars(None);
    let report = ApplyReport {
        updated: vec![(
            "chapkit_ewars_model".to_string(),
            project.state.models["chapkit_ewars_model"].clone(),
        )],
        ..ApplyReport::default()
    };
    let text = render(|lines| summary(&report, &[], &project, lines));
    assert!(
        text.starts_with("updated chapkit_ewars_model v1.0.0"),
        "{text}"
    );
    assert!(!text.contains("run `chap-devops up` to apply"), "{text}");
}

#[test]
fn the_summary_ends_with_the_next_step() {
    let project = project_with_ewars(Some(5001));
    let text = render(|lines| summary(&enabled_report(&project), &[], &project, lines));
    assert!(text.contains("enabled chapkit_ewars_model v1.0.0 on http://localhost:5001"));
    assert!(text.ends_with("run `chap-devops up` to apply\n"));
}

#[test]
fn a_model_with_no_host_port_is_summarised_with_the_proxy_url() {
    let project = project_with_ewars(None);
    let text = render(|lines| summary(&enabled_report(&project), &[], &project, lines));
    assert!(
        text.contains(
            "enabled chapkit_ewars_model v1.0.0 at \
                 http://localhost:8700/v2/services/chapkit-ewars-model/run/"
        ),
        "{text}"
    );
}

/// A manually added model's version is its image tag; printing `v` in
/// front of it would claim a version number it does not have.
#[test]
fn a_manual_models_line_names_its_tag_rather_than_a_version() {
    let mut project = project_with_ewars(None);
    let entry = project
        .state
        .models
        .get_mut("chapkit_ewars_model")
        .expect("just built");
    entry.version = "sha-b1d6c31".into();
    entry.image_tag = "sha-b1d6c31".into();
    let text = render(|lines| summary(&enabled_report(&project), &[], &project, lines));
    assert!(
        text.contains("enabled chapkit_ewars_model sha-b1d6c31 at "),
        "{text}"
    );
    assert!(!text.contains("vsha-"), "{text}");
}

#[test]
fn a_disable_summary_names_the_removed_file() {
    let project = project_with_ewars(Some(5001));
    let report = ApplyReport {
        disabled: vec!["chapkit_ewars_model".to_string()],
        removed: vec![project.dir.join("compose.chapkit-ewars-model.yml")],
        ..ApplyReport::default()
    };
    let text = render(|lines| summary(&report, &[], &project, lines));
    assert!(text.contains("disabled chapkit_ewars_model"));
    assert!(text.contains("hint: removed compose.chapkit-ewars-model.yml"));
    // Nothing is left for `up` to apply once a model is taken away.
    assert!(text.ends_with("hint: `chap-devops status` shows what runs now\n"));
}

#[test]
fn a_port_change_says_what_happened_and_what_to_do_next() {
    let project = project_with_ewars(None);
    let exposed = PortChange {
        id: "chapkit_ewars_model".into(),
        service_id: "chapkit-ewars-model".into(),
        host_port: Some(5001),
        previous: None,
        url: "http://localhost:5001".into(),
        written: vec![project.dir.join("compose.chapkit-ewars-model.yml")],
        applied: false,
        running: false,
    };
    let text = render(|lines| port_summary(&exposed, &project, &[], lines));
    assert!(text.starts_with("exposed chapkit-ewars-model on http://localhost:5001\n"));
    assert!(text.contains("hint: wrote compose.chapkit-ewars-model.yml\n"));
    assert!(text.ends_with("run `chap-devops up` to apply\n"));
    assert!(!text.contains("no change"));

    let internal = PortChange {
        host_port: None,
        previous: Some(5001),
        url: project.proxy_url("chapkit-ewars-model"),
        written: Vec::new(),
        ..exposed
    };
    let text = render(|lines| port_summary(&internal, &project, &["careful".to_string()], lines));
    assert!(text.starts_with(
        "unexposed chapkit-ewars-model; it stays registered with chap-core and \
             reachable at http://localhost:8700/v2/services/chapkit-ewars-model/run/\n"
    ));
    assert!(text.contains("warning: careful\n"));

    // Asking for what is already there is a no-op worth saying out loud.
    let again = PortChange {
        host_port: None,
        previous: None,
        ..internal
    };
    let text = render(|lines| port_summary(&again, &project, &[], lines));
    assert!(
        text.starts_with("chapkit-ewars-model was already unexposed; nothing changed"),
        "{text}"
    );
    assert!(!text.contains("run `chap-devops up` to apply"), "{text}");
}

/// A port that is already there and already published has no apply step.
#[test]
fn an_applied_port_with_no_change_names_no_apply_step() {
    let project = project_with_ewars(Some(5001));
    let change = PortChange {
        id: "chapkit_ewars_model".into(),
        service_id: "chapkit-ewars-model".into(),
        host_port: Some(5001),
        previous: Some(5001),
        url: "http://localhost:5001".into(),
        written: Vec::new(),
        applied: true,
        running: false,
    };
    let text = render(|lines| port_summary(&change, &project, &[], lines));
    assert_eq!(
        text,
        "chapkit-ewars-model is already exposed on http://localhost:5001; nothing changed\n"
    );
    // A container that does not publish it yet is not a change this command
    // made: `up` is a hint, not a step.
    let pending = PortChange {
        applied: false,
        running: false,
        ..change
    };
    let text = render(|lines| port_summary(&pending, &project, &[], lines));
    assert!(
        text.ends_with(
            "nothing changed\nhint: if its container is not running, `chap-devops up` starts it\n"
        ),
        "{text}"
    );
}

/// A no-op expose on a running container that does not publish the port yet
/// names `chap-devops up` as the step that applies it.
#[test]
fn a_port_not_applied_on_a_running_container_names_chap_devops_up() {
    let project = project_with_ewars(Some(5001));
    let change = PortChange {
        id: "chapkit_ewars_model".into(),
        service_id: "chapkit-ewars-model".into(),
        host_port: Some(5001),
        previous: Some(5001),
        url: "http://localhost:5001".into(),
        written: Vec::new(),
        applied: false,
        running: true,
    };
    let text = render(|lines| port_summary(&change, &project, &[], lines));
    assert_eq!(
        text,
        "chapkit-ewars-model is already exposed on http://localhost:5001; nothing changed\n\
         its running container does not publish this port yet; run `chap-devops up` to apply it\n"
    );
}

fn allocator(_: &BTreeSet<u16>) -> Result<PortAllocator> {
    Ok(PortAllocator::new((18100, 18110), [18100]))
}

/// The model's own container on its port does not make the port busy for it.
#[test]
fn the_own_live_port_is_not_busy_for_the_model() {
    let every_port_is_live = |_: u16| true;
    let own = |port: u16| port == 18105;
    let kept = choose_port(
        PortRequest::Fixed(18105),
        Some(18105),
        &own,
        &allocator,
        &every_port_is_live,
    )
    .expect("the own port is free for the model");
    assert_eq!(kept, Some(18105));

    // `--port auto` keeps it too, and does not move to the next free port.
    let kept = choose_port(PortRequest::Auto, Some(18105), &own, &allocator, &|port| {
        port != 18101
    })
    .expect("the own port is free for the model");
    assert_eq!(kept, Some(18105));

    // A port that something else holds is still refused.
    let held = choose_port(
        PortRequest::Fixed(18106),
        Some(18105),
        &own,
        &allocator,
        &every_port_is_live,
    );
    assert!(held.is_err(), "{held:?}");
}

/// `--port auto` on a model without a port, or one whose port something else
/// holds, takes the first free port.
#[test]
fn auto_moves_only_off_a_port_that_is_not_free() {
    let busy = |port: u16| port == 18101 || port == 18105;
    let no_own = |_: u16| false;
    let first = choose_port(PortRequest::Auto, None, &no_own, &allocator, &busy).unwrap();
    assert_eq!(first, Some(18102));
    let moved = choose_port(PortRequest::Auto, Some(18105), &no_own, &allocator, &busy).unwrap();
    assert_eq!(moved, Some(18102));
    let kept = choose_port(PortRequest::Auto, Some(18107), &no_own, &allocator, &busy).unwrap();
    assert_eq!(kept, Some(18107));
    assert_eq!(
        choose_port(PortRequest::None, Some(18107), &no_own, &allocator, &busy).unwrap(),
        None
    );
}
