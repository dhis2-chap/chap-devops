use super::*;
use crate::project::EnabledModel;
use crate::registry::Channel;

fn model(port: Option<u16>) -> EnabledModel {
    EnabledModel {
        previous: None,
        reads_port: false,
        service_id: "chapkit-ewars-model".into(),
        image: "ghcr.io/chap-models/chapkit_ewars_model".into(),
        image_tag: "sha-fa880a1".into(),
        version: "1.0.0".into(),
        channel: Some(Channel::Stable),
        host_port: port,
        bind: None,
        data_dir: "/app/data".into(),
        user: "chapkit:chapkit".into(),
        user_from: Default::default(),
        platform: Some("linux/amd64".into()),
        compose_file: "compose.chapkit-ewars-model.yml".into(),
    }
}

/// The lines of [`summary`], as the tests read them.
fn human(report: &ApplyReport, notes: &[String]) -> String {
    let mut lines = Report::default();
    summary(report, notes, &mut lines);
    lines.text()
}

#[test]
fn a_report_lists_what_changed_and_what_to_do_next() {
    let report = ApplyReport {
        enabled: vec![("chapkit_ewars_model".to_string(), model(Some(5001)))],
        updated: vec![("auto_arima_chapkit".to_string(), model(Some(5002)))],
        disabled: vec!["chapkit_simple_multistep_model".to_string()],
        warnings: vec!["templates are not for real forecasts".to_string()],
        ..ApplyReport::default()
    };
    let text = human(&report, &[]);
    assert!(text.starts_with("warning: templates are not for real forecasts\n"));
    assert!(text.contains("enabled chapkit_ewars_model v1.0.0 on http://localhost:5001\n"));
    assert!(text.contains("updated auto_arima_chapkit v1.0.0 on http://localhost:5002\n"));
    assert!(text.contains("disabled chapkit_simple_multistep_model\n"));
    assert!(text.ends_with("run `chap-devops up` to apply\n"));
}

#[test]
fn an_empty_report_says_so() {
    assert_eq!(human(&ApplyReport::default(), &[]), "no changes\n");
}

/// The seam the browser exists for: keys in, files on disk out. Everything
/// except the terminal itself is exercised here.
#[test]
fn what_the_browser_selects_applies_to_a_real_project() {
    use crate::project::Project;
    use crate::registry::load_embedded;
    use crate::tui::app::{App, Outcome};
    use crate::tui::keys::action_for;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let dir = tempfile::tempdir().expect("temp dir");
    let registry = load_embedded().expect("embedded snapshot parses");
    let mut project = Project {
        dir: dir.path().to_path_buf(),
        state: Default::default(),
    };

    let mut app = App::new(&registry, &project.state);
    let press = |code| KeyEvent::new(code, KeyModifiers::NONE);

    // Move down one, enable that model, then save.
    assert!(
        app.reduce(action_for(app.mode, &press(KeyCode::Char('j'))))
            .is_none()
    );
    assert!(
        app.reduce(action_for(app.mode, &press(KeyCode::Char(' '))))
            .is_none()
    );
    assert_eq!(
        app.reduce(action_for(app.mode, &press(KeyCode::Char('s')))),
        Some(Outcome::Save)
    );

    let selection = app.selection();
    assert_eq!(selection.enable.len(), 1);
    let report = crate::compose::apply::apply_with(
        &mut project,
        &registry,
        &selection,
        &|_| false,
        &crate::compose::resolve::from_table,
    )
    .expect("the browser's selection is applicable");

    assert_eq!(report.enabled.len(), 1);
    let (id, model) = &report.enabled[0];
    assert_eq!(id, &selection.enable[0].id);
    assert!(dir.path().join(&model.compose_file).is_file());
    assert!(dir.path().join(".chap-devops/models.yaml").is_file());
    assert!(dir.path().join("compose.marketplace.yml").is_file());
    assert!(human(&report, &[]).contains("run `chap-devops up`"));
}

/// The same seam for the second page: Tab, space, save - and a component
/// compose file on disk.
#[test]
fn what_the_browser_selects_on_the_components_page_applies_too() {
    use crate::components::{Component, OCS_COMPOSE};
    use crate::project::Project;
    use crate::registry::load_embedded;
    use crate::tui::app::{App, Outcome};
    use crate::tui::keys::action_for;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let dir = tempfile::tempdir().expect("temp dir");
    let registry = load_embedded().expect("embedded snapshot parses");
    let mut project = Project {
        dir: dir.path().to_path_buf(),
        state: Default::default(),
    };

    let mut app = App::new(&registry, &project.state);
    let press = |code| KeyEvent::new(code, KeyModifiers::NONE);

    // Tab to the components page, down to ocs, space, save.
    app.reduce(action_for(app.mode, &press(KeyCode::Tab)));
    app.reduce(action_for(app.mode, &press(KeyCode::Char('j'))));
    assert_eq!(app.selected_component(), Component::Ocs);
    app.reduce(action_for(app.mode, &press(KeyCode::Char(' '))));
    assert_eq!(
        app.reduce(action_for(app.mode, &press(KeyCode::Char('s')))),
        Some(Outcome::Save)
    );

    let selection = app.selection();
    assert!(selection.enable.is_empty() && selection.disable.is_empty());
    assert!(!selection.is_empty(), "the component set is the change");
    let report = crate::compose::apply::apply_with(
        &mut project,
        &registry,
        &selection,
        &|_| false,
        &crate::compose::resolve::from_table,
    )
    .expect("the browser's component set is applicable");

    assert_eq!(
        report.components_enabled,
        vec![("ocs".to_string(), Some(8790))]
    );
    assert!(dir.path().join(OCS_COMPOSE).is_file());
    assert!(dir.path().join(".chap-devops/components.yaml").is_file());
    let text = human(&report, &["stopped nothing".to_string()]);
    assert!(
        text.contains("enabled the ocs component on http://localhost:8790\n"),
        "{text}"
    );
    assert!(text.contains("\nstopped nothing\n"), "{text}");
    assert!(text.ends_with("run `chap-devops up` to apply\n"), "{text}");
}

/// A component that was switched off is reported as off, with the notes
/// from stopping its containers under it.
#[test]
fn the_report_names_the_components_that_were_switched_off() {
    let report = ApplyReport {
        components_disabled: vec!["ocs".to_string()],
        removed: vec![std::path::PathBuf::from("compose.ocs.yml")],
        ..ApplyReport::default()
    };
    let text = human(
        &report,
        &[
            "stopped ocs".to_string(),
            "kept the volume ocs_data".to_string(),
        ],
    );
    assert!(text.contains("disabled the ocs component\n"), "{text}");
    assert!(text.contains("\nstopped ocs\n"), "{text}");
    assert!(text.contains("\nkept the volume ocs_data\n"), "{text}");
    assert!(text.ends_with("run `chap-devops up` to apply\n"), "{text}");
}
