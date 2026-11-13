use super::*;
use crate::configs::sync::ModelOutcome;
use crate::project::{EnabledModel, PreviousPin};

fn enabled(previous: bool) -> EnabledModel {
    EnabledModel {
        previous: previous.then(|| PreviousPin {
            version: "1.0.0".to_string(),
            image_tag: "sha-1111111".to_string(),
            image_digest: None,
            user: "chapkit:chapkit".to_string(),
            user_from: Default::default(),
            reads_port: false,
            multi_arch: false,
        }),
        reads_port: false,
        service_id: "m".to_string(),
        image: "ghcr.io/chap-models/m".to_string(),
        image_tag: "sha-2222222".to_string(),
        image_digest: None,
        version: "1.1.0".to_string(),
        channel: None,
        host_port: None,
        bind: None,
        data_dir: "/app/data".to_string(),
        user: "chapkit:chapkit".to_string(),
        user_from: Default::default(),
        platform: None,
        multi_arch: false,
        compose_file: "compose.m.yml".to_string(),
    }
}

fn outcome(id: &str, outcome: Outcome) -> ModelOutcome {
    ModelOutcome {
        id: id.to_string(),
        service_id: id.replace('_', "-"),
        outcome,
    }
}

/// A configured model forgets its previous pin, a model whose new version
/// chap-core retired goes back to it, and the others keep it.
#[test]
fn the_outcome_of_the_configure_step_settles_the_previous_pin() {
    let mut project = Project {
        dir: std::path::PathBuf::from("/srv/chap"),
        state: Default::default(),
    };
    for id in ["created", "already", "retired", "not_retired", "fresh"] {
        project
            .state
            .models
            .insert(id.to_string(), enabled(id != "fresh"));
    }
    let failed = |undo| Outcome::Failed {
        error: "HTTP 500".to_string(),
        undo,
    };
    let configured = Folded {
        models: vec![
            outcome(
                "created",
                Outcome::Created {
                    configured: vec!["default".to_string()],
                },
            ),
            outcome("already", Outcome::AlreadyConfigured),
            outcome("retired", failed(Some(Undo::Retired { template_id: 7 }))),
            outcome(
                "not_retired",
                failed(Some(Undo::NotRetired {
                    template_id: 8,
                    error: "HTTP 500".to_string(),
                })),
            ),
            outcome("fresh", Outcome::AlreadyConfigured),
        ],
        error: None,
    };
    assert_eq!(
        decide(&project, &configured),
        [
            Settle::Forget("created".to_string()),
            Settle::Forget("already".to_string()),
            Settle::Restore("retired".to_string()),
        ]
    );
}
