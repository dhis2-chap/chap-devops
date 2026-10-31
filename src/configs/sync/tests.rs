use super::*;
use crate::registry::load_embedded;
use crate::registry::model::Configuration;

/// A marketplace entry from the snapshot built into the binary.
fn entry(id: &str) -> Model {
    load_embedded()
        .expect("the embedded snapshot parses")
        .models
        .into_iter()
        .find(|model| model.id == id)
        .expect("the entry is in the snapshot")
}

fn row(name: &str, archived: bool, version: Option<&str>) -> ConfiguredModel {
    ConfiguredModel {
        id: 1,
        name: name.to_string(),
        archived,
        covariates: Vec::new(),
        version: version.map(str::to_string),
        health: None,
    }
}

#[test]
fn every_configuration_of_the_entry_is_one_request() {
    let ewars = entry("chapkit_ewars_model");
    let made = requests(&Source::Marketplace(Box::new(ewars)), 7);
    let names: Vec<&str> = made.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "monthly_climate",
            "monthly_population_only",
            "monthly_region_seasonal"
        ]
    );
    let climate = &made[0];
    assert_eq!(climate.model_template_id, 7);
    // The covariate list is a field of its own, and the horizon is gone.
    assert_eq!(
        climate.additional_continuous_covariates.as_deref(),
        Some(["rainfall".to_string(), "mean_temperature".to_string()].as_slice())
    );
    for key in RESERVED_CONFIG_KEYS {
        assert!(!climate.user_option_values.contains_key(*key), "{key}");
    }
    assert_eq!(
        climate.user_option_values["n_lags"],
        serde_json::json!([3, 3])
    );
    assert_eq!(climate.user_option_values["region_seasonal"], false);
    // An empty list in the config is kept: it is a choice, not a default.
    assert_eq!(
        made[1].additional_continuous_covariates.as_deref(),
        Some([].as_slice())
    );
}

#[test]
fn a_configuration_without_covariates_gets_the_entry_defaults() {
    let mut model = entry("chapkit_simple_multistep_model");
    model.configurations.clear();
    model.configurations.insert(
        "plain".to_string(),
        Configuration {
            description: None,
            config: serde_json::json!({"prediction_periods": 3, "n_samples": 10}),
        },
    );
    let made = requests(&Source::Marketplace(Box::new(model)), 3);
    assert_eq!(made.len(), 1);
    assert_eq!(
        made[0].additional_continuous_covariates.as_deref(),
        Some(
            [
                "rainfall".to_string(),
                "mean_temperature".to_string(),
                "mean_relative_humidity".to_string()
            ]
            .as_slice()
        )
    );
    assert_eq!(
        serde_json::Value::Object(made[0].user_option_values.clone()),
        serde_json::json!({"n_samples": 10})
    );
}

#[test]
fn an_entry_without_configurations_gets_one_default() {
    let mut model = entry("chapkit_ewars_model");
    model.configurations.clear();
    let made = requests(&Source::Marketplace(Box::new(model)), 5);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].name, DEFAULT_CONFIGURATION);
    assert!(made[0].user_option_values.is_empty());
    assert_eq!(
        made[0].additional_continuous_covariates.as_deref(),
        Some(["rainfall".to_string(), "mean_temperature".to_string()].as_slice())
    );
}

#[test]
fn a_custom_model_gets_one_default_without_covariates() {
    let made = requests(&Source::Custom, 9);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].name, DEFAULT_CONFIGURATION);
    let body = serde_json::to_value(&made[0]).expect("JSON");
    assert_eq!(
        body,
        serde_json::json!({"name": "default", "model_template_id": 9, "user_option_values": {}})
    );
}

#[test]
fn a_model_with_a_live_configured_model_is_configured() {
    let service = "chapkit-ewars-model";
    // The bare name and a named configuration both count.
    assert!(is_configured(&[row(service, false, None)], service, None));
    assert!(is_configured(
        &[row(
            "chapkit-ewars-model:monthly_climate",
            false,
            Some("1.0.0")
        )],
        service,
        Some("1.0.0")
    ));
    // Nothing at all, or only another model.
    assert!(!is_configured(&[], service, None));
    assert!(!is_configured(
        &[row("chapkit-ewars-model-two", false, None)],
        service,
        None
    ));
}

#[test]
fn archived_rows_test_leftovers_and_old_versions_do_not_count() {
    let service = "chapkit-ewars-model";
    assert!(!is_configured(&[row(service, true, None)], service, None));
    assert!(!is_configured(
        &[row(
            "chapkit-ewars-model:test_config_01M3A4TSB2YHRPFBEFCT4TMX4A",
            false,
            None
        )],
        service,
        None
    ));
    // An update registers a new version, and the old one's rows do not run it.
    assert!(!is_configured(
        &[row(service, false, Some("1.0.0"))],
        service,
        Some("1.1.0")
    ));
    // Without a version on either side the name decides.
    assert!(is_configured(
        &[row(service, false, None)],
        service,
        Some("1.1.0")
    ));
    assert!(is_configured(
        &[row(service, false, Some("1.0.0"))],
        service,
        Some("")
    ));
}

#[test]
fn the_command_says_one_line_for_each_configured_model() {
    let outcomes = vec![
        ModelOutcome {
            id: "chapkit_ewars_model".to_string(),
            service_id: "chapkit-ewars-model".to_string(),
            outcome: Outcome::Created {
                configured: vec![
                    "monthly_climate".to_string(),
                    "monthly_region_seasonal".to_string(),
                ],
            },
        },
        ModelOutcome {
            id: "auto_arima_chapkit".to_string(),
            service_id: "auto-arima-chapkit".to_string(),
            outcome: Outcome::NotRegistered,
        },
    ];
    let mut lines = Report::default();
    command_lines(&outcomes, &mut lines);
    let texts: Vec<&str> = lines.messages().iter().map(|m| m.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "chapkit_ewars_model: created configured model monthly_climate",
            "chapkit_ewars_model: created configured model monthly_region_seasonal",
            "auto_arima_chapkit: not registered with chap-core; run `chap-devops models configs sync` \
             again once `chap-devops status` shows it registered",
        ]
    );

    // Folded into another command: one line, and nothing for the rest.
    let mut lines = Report::default();
    folded_lines(&outcomes, false, &mut lines);
    let texts: Vec<&str> = lines.messages().iter().map(|m| m.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "created configured models in chap-core for chapkit_ewars_model (monthly_climate, \
          monthly_region_seasonal)"
        ]
    );
}

fn service(revision: Option<Option<&str>>) -> crate::status::RegisteredService {
    crate::status::RegisteredService {
        id: "minimalist-norev".to_string(),
        url: "http://host.docker.internal:9090".to_string(),
        display_name: "minimalist-norev".to_string(),
        version: "1.0.1".to_string(),
        last_ping_at: String::new(),
        expires_at: String::new(),
        git_revision: revision.map(|r| r.map(str::to_string)),
    }
}

/// chapkit 2.1 sends no `git_revision` key when it has no revision. The
/// refusal of chap-core 2.4 says so, and so does the key that chap-core
/// sent for another service.
#[test]
fn a_registration_without_the_revision_key_is_a_model_that_reports_none() {
    let refusal = "http://localhost:8700/v1/crud/model-templates/from-service answered HTTP \
                   409 Conflict: Model template 'minimalist-norev' version '1.0.1' is stored \
                   from revision None, but its source now reports revision None";
    let other = "http://localhost:8700/v1/crud/model-templates/from-service answered HTTP 500";
    assert!(is_no_revision(&service(None), false, refusal));
    assert!(is_no_revision(&service(None), true, other));
    assert!(is_no_revision(&service(Some(None)), false, other));
    assert!(!is_no_revision(&service(None), false, other));
    assert!(!is_no_revision(&service(Some(Some("abc"))), true, other));
}

/// The way out of a model without a revision depends on where it runs.
/// The refusal is read in the words of each chap-core: a sync that misses it
/// tells the reader to run again, which cannot help.
#[test]
fn the_refusal_of_a_model_without_a_revision_is_known_in_both_wordings() {
    for refusal in [
        "Model template 'm' version '0.1.0' is stored from revision None, but its source now \
         reports revision None",
        "Model template 'm' version '1.0.3': its source reports no git revision, so chap-core \
         cannot check which source it runs.",
    ] {
        assert!(is_no_revision_refusal(refusal), "{refusal}");
        assert!(is_no_revision(&service(None), false, refusal), "{refusal}");
    }
    assert!(!is_no_revision_refusal(
        "Model template 'm' version '0.1.0' is stored from revision abc, but its source now \
         reports revision def"
    ));
}

#[test]
fn a_model_without_a_revision_gets_the_way_out_of_where_it_runs() {
    let outcome = |id: &str, managed: bool| ModelOutcome {
        id: id.to_string(),
        service_id: id.to_string(),
        outcome: Outcome::NoRevision {
            error: "HTTP 409 Conflict".to_string(),
            managed,
        },
    };
    let mut lines = Report::default();
    command_lines(
        &[
            outcome("auto_arima_chapkit", true),
            outcome("minimalist-norev", false),
        ],
        &mut lines,
    );
    let texts: Vec<&str> = lines.messages().iter().map(|m| m.text.as_str()).collect();
    assert!(texts[0].ends_with(NO_REVISION_WAY_OUT), "{texts:?}");
    assert_eq!(
        texts[1],
        "minimalist-norev: could not create its configured models: HTTP 409 Conflict; it \
         reports no git revision, so running this again does not help; set `GIT_REVISION` \
         where it runs, then start it again"
    );
}

#[test]
fn a_failure_says_what_was_done_about_the_template_it_stored() {
    let failed = |undo: Option<Undo>| ModelOutcome {
        id: "chapkit_ewars_model".to_string(),
        service_id: "chapkit-ewars-model".to_string(),
        outcome: Outcome::Failed {
            error: "HTTP 422".to_string(),
            undo,
        },
    };
    let text = |undo: Option<Undo>| {
        let mut lines = Report::default();
        command_lines(&[failed(undo)], &mut lines);
        lines.messages()[0].text.clone()
    };
    let again = "run `chap-devops models configs sync chapkit_ewars_model` again, and \
                 `chap-devops logs chap` says why";

    assert_eq!(
        text(None),
        format!("chapkit_ewars_model: could not create its configured models: HTTP 422; {again}")
    );
    assert_eq!(
        text(Some(Undo::Retired { template_id: 30 })),
        format!(
            "chapkit_ewars_model: could not create its configured models: HTTP 422; the \
             template version it stored is retired again, with the configured models made so \
             far; {again}"
        )
    );
    assert_eq!(
        text(Some(Undo::NotRetired {
            template_id: 30,
            error: "HTTP 500".to_string(),
        })),
        "chapkit_ewars_model: could not create its configured models: HTTP 422; retiring the \
         template version it stored failed too: HTTP 500; run `chap-devops api DELETE \
         '/v1/crud/model-templates/30'`, then `chap-devops models configs sync chapkit_ewars_model`"
    );

    // Folded into another command, the warning is the same.
    let mut lines = Report::default();
    folded_lines(
        &[failed(Some(Undo::Retired { template_id: 30 }))],
        false,
        &mut lines,
    );
    assert!(
        lines.messages()[0].text.contains("is retired again"),
        "{:?}",
        lines.messages()
    );

    // `--json` carries the undo beside the error.
    let json = serde_json::to_value(failed(Some(Undo::Retired { template_id: 30 }))).unwrap();
    assert_eq!(json["state"], "failed");
    assert_eq!(
        json["undo"],
        serde_json::json!({"state": "retired", "template_id": 30})
    );
    let json = serde_json::to_value(failed(None)).unwrap();
    assert!(json.get("undo").is_none());
}
