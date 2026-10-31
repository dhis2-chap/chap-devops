use super::*;
use crate::configs;
use crate::project::{EnabledModel, ProjectState};
use crate::registry::{Registry, load_embedded};
use crate::tui::form::{draft_of, form_for};
use serde_json::json;

const EWARS: &str = "chapkit_ewars_model";

fn registry() -> Registry {
    load_embedded().expect("embedded snapshot parses")
}

/// A deployment with chap-core that runs the EWARS model.
fn state(registry: &Registry, chap_core: bool) -> ProjectState {
    let model = registry.get(EWARS).expect("vendored model");
    let mut state = ProjectState::default();
    state.components.chap_core.enabled = chap_core;
    state.models.insert(
        model.id.clone(),
        EnabledModel {
            previous: None,
            reads_port: false,
            service_id: model.service_id.clone(),
            image: model.source.image.clone(),
            image_tag: "sha-1".to_string(),
            version: "1.0.0".to_string(),
            channel: None,
            host_port: None,
            bind: None,
            data_dir: "/work/data".to_string(),
            user: "chapkit:chapkit".to_string(),
            user_from: Default::default(),
            platform: None,
            compose_file: format!("compose.{}.yml", model.service_id),
        },
    );
    state
}

/// Put the cursor on the EWARS row.
fn on_ewars(app: &mut App) {
    app.filter = EWARS.to_string();
    app.refilter();
    assert_eq!(app.selected_model().map(|m| m.id.as_str()), Some(EWARS));
}

fn template() -> Template {
    configs::parse_template(&json!({
        "id": 30,
        "name": "chapkit-ewars-model",
        "requiredCovariates": ["population"],
        "allowFreeAdditionalContinuousCovariates": true,
        "userOptions": {
            "n_lags": {"type": "integer", "default": 3, "description": "Lags of the target."}
        }
    }))
    .expect("a template")
}

fn ready() -> Load {
    let rows = json!([
        {"id": 7, "name": "chapkit-ewars-model:monthly_climate",
         "additionalContinuousCovariates": ["rainfall"], "userOptionValues": {"n_lags": 6}}
    ]);
    Load::Ready {
        template: Some(template()),
        configs: configs::configs_of(&rows, "chapkit-ewars-model")
            .into_iter()
            .map(|config| (config, "marketplace"))
            .collect(),
    }
}

/// The page open on EWARS, with chap-core's answer in.
fn opened(app: &mut App) {
    on_ewars(app);
    app.reduce(Action::Configs);
    assert_eq!(app.mode, Mode::Configs);
    assert_eq!(
        app.take_effect(),
        Some(Effect::LoadConfigs {
            model: EWARS.to_string(),
            service: "chapkit-ewars-model".to_string(),
        })
    );
    app.configs_loaded(ready());
}

#[test]
fn m_on_a_model_that_is_not_enabled_says_what_to_do() {
    let registry = registry();
    let mut state = state(&registry, true);
    state.models.clear();
    let mut app = App::new(&registry, &state);
    on_ewars(&mut app);
    app.reduce(Action::Configs);
    assert_eq!(app.mode, Mode::Browse);
    assert_eq!(app.message.as_deref(), Some(CONFIGS_NEED_ENABLED));
    assert_eq!(app.take_effect(), None);
}

#[test]
fn m_without_chap_core_says_so() {
    let registry = registry();
    let mut app = App::new(&registry, &state(&registry, false));
    on_ewars(&mut app);
    app.reduce(Action::Configs);
    assert_eq!(app.message.as_deref(), Some(CONFIGS_NEED_CHAP_CORE));
}

#[test]
fn the_form_writes_nothing_until_the_user_says_yes() {
    let registry = registry();
    let mut app = App::new(&registry, &state(&registry, true));
    opened(&mut app);

    app.reduce(Action::ConfigAdd);
    assert_eq!(app.mode, Mode::ConfigForm);
    let form = app
        .configs
        .as_ref()
        .and_then(|v| v.form.clone())
        .expect("a form");
    let labels: Vec<&str> = form.fields.iter().map(|f| f.label.as_str()).collect();
    assert_eq!(labels, ["name", "n_lags", "covariates"]);
    assert_eq!(form.fields[1].default, "3");
    assert_eq!(form.fields[1].kind, "integer");

    // A name in use is refused on the form.
    for c in "monthly_climate".chars() {
        app.reduce(Action::FormChar(c));
    }
    app.reduce(Action::FormSubmit);
    assert_eq!(app.mode, Mode::ConfigForm);
    let error = app
        .configs
        .as_ref()
        .and_then(|v| v.form.as_ref()?.error.clone());
    assert!(error.expect("a refusal").contains("already"));

    for _ in 0.."climate".len() {
        app.reduce(Action::FormBackspace);
    }
    for c in "weekly".chars() {
        app.reduce(Action::FormChar(c));
    }
    app.reduce(Action::Down);
    app.reduce(Action::FormChar('5'));
    app.reduce(Action::Down);
    for c in "rainfall".chars() {
        app.reduce(Action::FormChar(c));
    }
    app.reduce(Action::FormSubmit);
    assert_eq!(app.mode, Mode::ConfigConfirm);
    assert_eq!(app.take_effect(), None);

    // No goes back to the form, with what was typed.
    app.reduce(Action::ConfirmNo);
    assert_eq!(app.mode, Mode::ConfigForm);
    app.reduce(Action::FormSubmit);
    app.reduce(Action::ConfirmYes);
    assert_eq!(app.mode, Mode::Configs);
    let Some(Effect::CreateConfig { model, draft, .. }) = app.take_effect() else {
        panic!("a create");
    };
    assert_eq!(model, EWARS);
    assert_eq!(draft.variant, "monthly_weekly");
    assert_eq!(draft.values["n_lags"], json!(5));
    assert_eq!(draft.covariates, ["rainfall"]);

    app.configs_done("created".to_string(), ready());
    assert_eq!(app.message.as_deref(), Some("created"));
}

#[test]
fn d_archives_the_selected_one_after_the_question() {
    let registry = registry();
    let mut app = App::new(&registry, &state(&registry, true));
    opened(&mut app);
    app.reduce(Action::ConfigArchive);
    assert_eq!(app.mode, Mode::ConfigConfirm);
    app.reduce(Action::FilterCancel);
    assert_eq!(app.mode, Mode::Configs);
    app.reduce(Action::ConfigArchive);
    app.reduce(Action::ConfirmYes);
    assert_eq!(
        app.take_effect(),
        Some(Effect::ArchiveConfig {
            model: EWARS.to_string(),
            service: "chapkit-ewars-model".to_string(),
            id: 7,
            variant: "monthly_climate".to_string(),
        })
    );
}

#[test]
fn esc_goes_back_to_the_list_and_a_failure_offers_nothing_to_add() {
    let registry = registry();
    let mut app = App::new(&registry, &state(&registry, true));
    opened(&mut app);
    app.configs_loaded(Load::Failed("down".to_string()));
    app.reduce(Action::ConfigAdd);
    assert_eq!(app.mode, Mode::Configs);
    app.reduce(Action::ConfigReload);
    assert!(matches!(
        app.take_effect(),
        Some(Effect::LoadConfigs { .. })
    ));
    app.reduce(Action::FilterCancel);
    assert_eq!(app.mode, Mode::Browse);
    assert!(app.configs.is_none());
}

#[test]
fn an_option_at_its_default_is_left_out_of_the_draft() {
    let mut form = form_for(&template());
    form.fields[0].input = "w".to_string();
    form.fields[1].input = "3".to_string();
    let draft = draft_of(&form, &template(), &[], EWARS).expect("a draft");
    assert!(draft.values.is_empty());
    form.fields[1].input = "x".to_string();
    assert!(draft_of(&form, &template(), &[], EWARS).is_err());
}

#[test]
fn e_edits_the_selected_one_and_refuses_no_change() {
    let registry = registry();
    let mut app = App::new(&registry, &state(&registry, true));
    opened(&mut app);
    app.reduce(Action::ConfigEdit);
    assert_eq!(app.mode, Mode::ConfigForm);
    let form = app
        .configs
        .as_ref()
        .and_then(|v| v.form.clone())
        .expect("a form");
    assert_eq!(form.fixed_name.as_deref(), Some("monthly_climate"));
    assert_eq!(form.fields[0].input, "6");

    // Unchanged: the form says so and stays.
    app.reduce(Action::FormSubmit);
    assert_eq!(app.mode, Mode::ConfigForm);
    let error = app
        .configs
        .as_ref()
        .and_then(|v| v.form.as_ref()?.error.clone());
    assert!(
        error
            .expect("a refusal")
            .contains("has these values already")
    );

    app.reduce(Action::FormBackspace);
    app.reduce(Action::FormChar('8'));
    app.reduce(Action::FormSubmit);
    assert_eq!(app.mode, Mode::ConfigConfirm);
    app.reduce(Action::ConfirmYes);
    let Some(Effect::UpdateConfig { old, draft, .. }) = app.take_effect() else {
        panic!("an update");
    };
    assert_eq!(old, 7);
    assert_eq!(draft.variant, "monthly_climate");
    assert_eq!(draft.values["n_lags"], json!(8));
}

#[test]
fn esc_in_the_form_writes_nothing_and_says_so() {
    let registry = registry();
    let mut app = App::new(&registry, &state(&registry, true));
    opened(&mut app);
    app.reduce(Action::ConfigAdd);
    app.reduce(Action::FilterCancel);
    assert_eq!(app.mode, Mode::Configs);
    assert_eq!(app.message.as_deref(), Some(crate::tui::form::LEFT));
    assert_eq!(app.take_effect(), None);
}
