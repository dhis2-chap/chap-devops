use super::data::{Kind, build};
use super::view::{App, Confirm, Entry, draw};
use crate::docker::{Labeled, Usage};
use crate::project::{EnabledModel, Project, ProjectState};
use crate::tui::theme::Theme;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn container(project: &str, service: &str, role: &str, state: &str, status: &str) -> Labeled {
    let mut labels = BTreeMap::from([
        (
            "com.docker.compose.project".to_string(),
            project.to_string(),
        ),
        (
            "com.docker.compose.service".to_string(),
            service.to_string(),
        ),
        (
            "com.docker.compose.project.working_dir".to_string(),
            format!("/deploy/{project}"),
        ),
        (
            "io.github.dhis2-chap.chap-devops.role".to_string(),
            role.to_string(),
        ),
    ]);
    if role == "model" {
        labels.insert(
            "io.github.dhis2-chap.chap-devops.model".to_string(),
            service.replace('-', "_"),
        );
    }
    Labeled {
        id: format!("id-{service}"),
        name: format!("{project}-{service}-1"),
        state: state.to_string(),
        status: status.to_string(),
        labels,
    }
}

fn model(service: &str, port: Option<u16>) -> EnabledModel {
    EnabledModel {
        reads_port: false,
        service_id: service.to_string(),
        image: format!("ghcr.io/chap-models/{service}"),
        image_tag: "sha-1".to_string(),
        version: "1.0.0".to_string(),
        channel: None,
        host_port: port,
        bind: None,
        data_dir: "/app/data".to_string(),
        user: "1000:1000".to_string(),
        user_from: Default::default(),
        platform: None,
        compose_file: format!("compose.{service}.yml"),
    }
}

/// A loader that knows one project, `demo`, with chap-core and one model.
fn loader(dir: &Path) -> Option<Project> {
    (dir == Path::new("/deploy/demo")).then(|| {
        let mut state = ProjectState {
            compose_project: "demo".to_string(),
            api_port: 8700,
            ..ProjectState::default()
        };
        state.models.insert(
            "chapkit_ewars_model".to_string(),
            model("chapkit-ewars-model", Some(5001)),
        );
        state.models.insert(
            "auto_arima_chapkit".to_string(),
            model("auto-arima-chapkit", None),
        );
        Project {
            dir: PathBuf::from("/deploy/demo"),
            state,
        }
    })
}

#[test]
fn containers_group_into_deployments_with_chap_core_first_and_urls_filled_in() {
    let containers = [
        container(
            "demo",
            "chapkit-ewars-model",
            "model",
            "running",
            "Up 1 minute",
        ),
        container(
            "demo",
            "chap",
            "chap-core",
            "running",
            "Up 2 minutes (healthy)",
        ),
        container(
            "demo",
            "chapkit-ewars-model-init",
            "model",
            "exited",
            "Exited (0)",
        ),
        container(
            "other",
            "dhis2",
            "dhis2",
            "exited",
            "Exited (1) 1 minute ago",
        ),
    ];
    let usage = BTreeMap::from([(
        "demo-chap-1".to_string(),
        Usage {
            cpu: "3.10%".to_string(),
            memory: "300MiB / 8GiB".to_string(),
        },
    )]);
    let nodes = build(&containers, &usage, &[], &loader);

    assert_eq!(nodes.len(), 2);
    let demo = &nodes[0];
    assert_eq!(demo.name, "demo");
    assert_eq!(demo.kind, Kind::Init);
    let services: Vec<&str> = demo.services.iter().map(|s| s.service.as_str()).collect();
    // The finished init container is left out.
    assert_eq!(services, ["chap", "chapkit-ewars-model"]);
    assert_eq!(demo.services[0].health.as_deref(), Some("healthy"));
    assert_eq!(demo.services[0].cpu.as_deref(), Some("3.10%"));
    assert_eq!(
        demo.services[0].url.as_deref(),
        Some("http://localhost:8700")
    );
    assert_eq!(
        demo.services[1].url.as_deref(),
        Some("http://localhost:5001")
    );
    assert_eq!(demo.running(), 2);
    // A deployment whose directory cannot be read still shows.
    assert_eq!(nodes[1].name, "other");
    assert_eq!(nodes[1].services[0].url, None);
}

#[test]
fn a_known_deployment_lists_its_models_that_have_no_container() {
    let nodes = build(
        &[],
        &BTreeMap::new(),
        &[PathBuf::from("/deploy/demo")],
        &loader,
    );
    assert_eq!(nodes.len(), 1);
    let states: Vec<(&str, &str)> = nodes[0]
        .services
        .iter()
        .map(|s| (s.service.as_str(), s.state.as_str()))
        .collect();
    assert_eq!(
        states,
        [
            ("auto-arima-chapkit", "not running"),
            ("chapkit-ewars-model", "not running")
        ]
    );
    // No host port, but chap-core's proxy reaches it.
    assert!(
        nodes[0].services[0]
            .url
            .as_deref()
            .unwrap()
            .contains("/v2/services/auto-arima-chapkit/run/"),
        "{:?}",
        nodes[0].services[0].url
    );
}

fn screen(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(110, 12)).expect("a test backend");
    terminal
        .draw(|frame| draw(frame, app, &Theme::monochrome()))
        .expect("draw");
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_tree_draws_each_deployment_with_its_services_under_it_and_folds() {
    let containers = [
        container("demo", "chap", "chap-core", "running", "Up (healthy)"),
        container("demo", "chapkit-ewars-model", "model", "running", "Up"),
    ];
    let mut app = App {
        interval: 2,
        ..App::default()
    };
    app.replace(
        build(&containers, &BTreeMap::new(), &[], &loader),
        "12:00:00".to_string(),
    );
    let text = screen(&app);
    assert!(text.contains("1 deployment  2/2 running"), "{text}");
    assert!(text.contains("▾ demo  init"), "{text}");
    assert!(text.contains("├─ chap"), "{text}");
    assert!(text.contains("└─ chapkit-ewars-model"), "{text}");
    assert!(text.contains("running (healthy)"), "{text}");

    app.toggle();
    assert_eq!(app.entries(), [Entry::Node(0)]);
    let text = screen(&app);
    assert!(text.contains("▸ demo"), "{text}");
    assert!(!text.contains("chapkit-ewars-model"), "{text}");
}

#[test]
fn a_refresh_keeps_the_cursor_on_the_same_service() {
    let first = [
        container("demo", "chap", "chap-core", "running", "Up"),
        container("demo", "chapkit-ewars-model", "model", "running", "Up"),
    ];
    let mut app = App::default();
    app.replace(build(&first, &BTreeMap::new(), &[], &loader), String::new());
    app.down();
    app.down();
    assert_eq!(
        app.selected_service().map(|(_, s)| s.service.as_str()),
        Some("chapkit-ewars-model")
    );
    // A new service sorts in ahead of it.
    let second = [
        container("demo", "chap", "chap-core", "running", "Up"),
        container("demo", "dhis2", "dhis2", "running", "Up"),
        container("demo", "chapkit-ewars-model", "model", "running", "Up"),
    ];
    app.replace(
        build(&second, &BTreeMap::new(), &[], &loader),
        String::new(),
    );
    assert_eq!(
        app.selected_service().map(|(_, s)| s.service.as_str()),
        Some("chapkit-ewars-model")
    );
}

/// A stop takes the model out of the deployment, and the prompt says so: an
/// operator who only means to restart it must not find it gone after `up`.
#[test]
fn the_stop_prompt_says_the_model_leaves_the_deployment() {
    let app = App {
        confirm: Some(Confirm {
            dir: std::path::PathBuf::from("/srv/chap"),
            id: "chapkit_ewars_model".to_string(),
            deployment: "chap".to_string(),
        }),
        ..App::default()
    };
    let text = screen(&app);
    assert!(
        text.contains("stop chapkit_ewars_model and take it out of chap (its data stays)?"),
        "{text}"
    );
}
