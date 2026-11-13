use super::*;
use crate::components::Component;
use crate::project::EnabledModel;

fn container(service: &str, id: &str) -> docker::Container {
    docker::Container {
        service: service.to_string(),
        name: format!("chapx-{service}-1"),
        id: id.to_string(),
        created_at: "t1".to_string(),
        state: "running".to_string(),
        ..docker::Container::default()
    }
}

/// A deployment with chap-core and one enabled model, `m`.
fn project() -> Project {
    let mut project = Project {
        dir: std::path::PathBuf::from("/srv/chap"),
        state: Default::default(),
    };
    project.state.models.insert(
        "m".to_string(),
        EnabledModel {
            previous: None,
            reads_port: false,
            service_id: "m-service".to_string(),
            image: "ghcr.io/chap-models/m".to_string(),
            image_tag: "sha-1111111".to_string(),
            image_digest: None,
            version: "1.0.0".to_string(),
            channel: None,
            host_port: None,
            bind: None,
            data_dir: "/app/data".to_string(),
            user: "root".to_string(),
            user_from: Default::default(),
            platform: None,
            multi_arch: false,
            compose_file: "compose.m-service.yml".to_string(),
        },
    );
    project
}

/// A plain `up` that started a model says that the configured models are
/// still to make.
#[test]
fn a_started_model_needs_its_configured_models() {
    let project = project();
    assert!(project.state.components.has_chap_core_api());
    let after = [container("chap", "a"), container("m-service", "b")];
    assert_eq!(configure_next(&[], &after, &project), Some(CONFIGURE_NEXT));
    // Recreated is started too: a new image can register a new version.
    let before = [container("chap", "a"), container("m-service", "old")];
    assert_eq!(
        configure_next(&before, &after, &project),
        Some(CONFIGURE_NEXT)
    );
}

/// No line when no model started, and none without chap-core.
#[test]
fn no_line_when_nothing_needs_configuring() {
    let mut project = project();
    let after = [container("chap", "a"), container("m-service", "b")];
    assert_eq!(configure_next(&after, &after, &project), None);

    let only_chap = [container("chap", "a")];
    assert_eq!(configure_next(&[], &only_chap, &project), None);

    project
        .state
        .components
        .set_enabled(Component::ChapCore, false);
    assert_eq!(configure_next(&[], &after, &project), None);
}
