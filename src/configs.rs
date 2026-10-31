//! chap-core's configured models of the models this deployment runs:
//! `chap-devops models configs`, the configurations page of `chap-devops ui`, and the
//! step other commands run ([`sync`]).
//!
//! A configured model is a model template with a name, values for its user
//! options and a list of additional covariates (chap-core's
//! `ConfiguredModelDB`). chap-core names it after the template:
//! `<template>:<name>`, or the bare template name for the name `default`
//! (`add_configured_model` in `database/database.py`). The template of a
//! chapkit model is named after its service id. The Modeling App calls the
//! name the variant name, and shows `<display name> [<variant>]`.
//!
//! chap-core never deletes a configured model: `DELETE
//! /v1/crud/configured-models/{id}` sets `archived`, and the row stays for
//! the backtests and predictions that use it. The Modeling App shows it as
//! Archived. A configured model posted again with the same name and the same
//! values is shown again.

pub mod export;
pub mod options;
pub mod sync;

use crate::api::Api;
use crate::error::Result;
use options::UserOption;
use serde::Serialize;
use serde_json::{Map, Value as Json};

pub use sync::CONFIGURED_MODELS_PATH;

/// The live model templates. A read that also makes a template for each
/// registered service that has none (chap-core's discovery), so `chap-devops
/// status` does not ask it.
pub const TEMPLATES_PATH: &str = "/v1/crud/model-templates";

/// The prefix of a configured model that `chapkit test` or `chap-devops models
/// test` left.
pub const TEST_PREFIX: &str = "test_config_";

/// One model template, cut to what a configuration needs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub version: Option<String>,
    pub display_name: Option<String>,
    pub options: Vec<UserOption>,
    /// The covariates every run of it gets.
    pub required_covariates: Vec<String>,
    /// Whether a configuration may add covariates of its own.
    pub free_covariates: bool,
    /// The covariates a configuration may name when it may not add its own:
    /// the marketplace entry's `covariates.defaults`. chap-core does not
    /// know them, so the caller fills them in.
    pub allowed_covariates: Vec<String>,
}

/// A template from chap-core's `ModelTemplateRead`, in either spelling.
pub fn parse_template(value: &Json) -> Option<Template> {
    let field = |camel: &str, snake: &str| value.get(camel).or_else(|| value.get(snake));
    Some(Template {
        id: value.get("id")?.as_i64()?,
        name: value.get("name")?.as_str()?.to_string(),
        version: value
            .get("version")
            .and_then(Json::as_str)
            .map(str::to_string),
        display_name: field("displayName", "display_name")
            .and_then(Json::as_str)
            .map(str::to_string),
        options: field("userOptions", "user_options")
            .map(options::parse_options)
            .unwrap_or_default(),
        required_covariates: field("requiredCovariates", "required_covariates")
            .and_then(Json::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|n| n.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        free_covariates: field(
            "allowFreeAdditionalContinuousCovariates",
            "allow_free_additional_continuous_covariates",
        )
        .and_then(Json::as_bool)
        .unwrap_or(false),
        allowed_covariates: Vec::new(),
    })
}

/// One configured model of a template.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Config {
    pub id: i64,
    /// chap-core's name: `<template>:<variant>`, or the template name.
    pub name: String,
    /// The name it was given, which the Modeling App shows in brackets:
    /// `default` for a configured model named after its template alone.
    pub variant: String,
    pub archived: bool,
    pub version: Option<String>,
    pub covariates: Vec<String>,
    pub values: Map<String, Json>,
}

impl Config {
    /// Whether a test left it.
    pub fn is_test(&self) -> bool {
        self.variant.starts_with(TEST_PREFIX)
    }
}

/// The configured models of `template` in a listing, by id.
pub fn configs_of(listed: &Json, template: &str) -> Vec<Config> {
    let prefix = format!("{template}:");
    let mut configs: Vec<Config> = listed
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let name = row.get("name")?.as_str()?;
            let variant = match name.strip_prefix(&prefix) {
                Some(variant) => variant.to_string(),
                None if name == template => sync::DEFAULT_CONFIGURATION.to_string(),
                None => return None,
            };
            Some(Config {
                id: row.get("id")?.as_i64()?,
                name: name.to_string(),
                variant,
                archived: row.get("archived").and_then(Json::as_bool).unwrap_or(false),
                version: row
                    .get("version")
                    .and_then(Json::as_str)
                    .map(str::to_string),
                covariates: row
                    .get("additionalContinuousCovariates")
                    .and_then(Json::as_array)
                    .map(|names| {
                        names
                            .iter()
                            .filter_map(|n| n.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                values: row
                    .get("userOptionValues")
                    .and_then(Json::as_object)
                    .cloned()
                    .unwrap_or_default(),
            })
        })
        .collect();
    configs.sort_by_key(|config| config.id);
    configs
}

/// Where a configured model comes from, as the SOURCE column says it.
pub fn source_label(config: &Config, source: Option<&sync::Source>) -> &'static str {
    if config.is_test() {
        return "models test";
    }
    let made_by_sync = source.is_some_and(|source| {
        sync::requests(source, 0)
            .iter()
            .any(|request| request.name == config.variant)
    });
    match (made_by_sync, source) {
        (true, Some(sync::Source::Marketplace(_))) => "marketplace",
        (true, _) => "service default",
        (false, _) => "by hand",
    }
}

/// The configured models chap-core lists, as it sent them.
pub fn listing(api: &Api) -> Result<Json> {
    api.get_json(CONFIGURED_MODELS_PATH)
}

/// The live template named `name`, from the template list.
pub fn template_named(api: &Api, name: &str) -> Result<Option<Template>> {
    let listed = api.get_json(TEMPLATES_PATH)?;
    Ok(listed
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(parse_template)
        .find(|template| template.name == name))
}

/// The body of `POST /v1/crud/model-templates/from-service`. chap-core sets
/// the role on every call, so the call sends the role of the marketplace
/// entry, and no key for a model without one: chap-core v2.4.0 knows no
/// role.
pub fn from_service_body(service_id: &str, role: Option<&str>) -> String {
    match role {
        Some(role) => serde_json::json!({ "service_id": service_id, "role": role }),
        None => serde_json::json!({ "service_id": service_id }),
    }
    .to_string()
}

/// The role of a model from its source: the marketplace entry, else none.
pub fn role_of(source: Option<&sync::Source>) -> Option<&str> {
    match source {
        Some(sync::Source::Marketplace(entry)) => entry.role.as_deref(),
        _ => None,
    }
}

/// The template of the version a registered service runs.
///
/// The template list shows a new version only once it has a configured
/// model, so a configuration of the running version starts from the template
/// that `from-service` stores or returns. It is what `chap-admin install`
/// calls, and it changes nothing for a version that is stored already.
/// `managed` says whether this deployment runs the service: its image is
/// built again, while a service from outside is started again where it runs.
pub fn template_of_service(
    api: &Api,
    model: &str,
    service_id: &str,
    role: Option<&str>,
    managed: bool,
) -> std::result::Result<Template, String> {
    let body = from_service_body(service_id, role);
    let answer = api
        .send_json("POST", sync::FROM_SERVICE_PATH, Some(&body))
        .map_err(|err| err.to_string())?;
    match answer.status {
        404 => {
            return Err(format!(
                "{model} is not registered with chap-core; start it with `chap-devops up`, and run \
                 this again when `chap-devops status` shows it registered"
            ));
        }
        409 if sync::is_no_revision_refusal(&answer.detail()) => {
            return Err(format!(
                "chap-core refuses the template of {model}: {}; {}",
                answer.detail(),
                match managed {
                    true => sync::NO_REVISION_WAY_OUT,
                    false => sync::UNMANAGED_NO_REVISION_WAY_OUT,
                }
            ));
        }
        409 => {
            return Err(format!(
                "chap-core refuses the template of {model}: {}",
                answer.detail()
            ));
        }
        _ if !answer.is_success() => {
            return Err(api
                .status_error(sync::FROM_SERVICE_PATH, &answer)
                .to_string());
        }
        _ => {}
    }
    answer
        .json()
        .as_ref()
        .and_then(parse_template)
        .ok_or_else(|| {
            format!(
                "{} answered without a template",
                api.url(sync::FROM_SERVICE_PATH)
            )
        })
}

/// A configured model to create.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Draft {
    pub variant: String,
    pub values: Map<String, Json>,
    pub covariates: Vec<String>,
}

/// The covariates of `--covariates a,b`, without blanks and repeats.
pub fn parse_covariates(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in text.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        if !names.iter().any(|known| known == name) {
            names.push(name.to_string());
        }
    }
    names
}

/// Why `draft` cannot be a configured model of `template`, if it cannot.
///
/// `model` is the id the messages name, and `existing` the configured models
/// the template has now. A name that a configured model has and that is not
/// archived is refused: chap-core would make the new one live and hide the
/// other one without a word.
pub fn check(
    draft: &Draft,
    template: &Template,
    existing: &[Config],
    model: &str,
) -> std::result::Result<(), String> {
    check_variant(&draft.variant)?;
    if existing
        .iter()
        .any(|config| !config.archived && config.variant == draft.variant)
    {
        return Err(format!(
            "{model} has a configured model {} already; give another --name, or archive it \
             first with `chap-devops models configs archive {model} {}`",
            draft.variant, draft.variant
        ));
    }
    for name in &draft.covariates {
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!(
                "`{name}` is not a covariate name; use letters, digits and `_`"
            ));
        }
        if template.required_covariates.contains(name) {
            return Err(format!(
                "`{name}` is a required covariate of {model}, which every run gets; remove it \
                 from --covariates"
            ));
        }
    }
    // Without free covariates, a configuration may only name the defaults
    // of the marketplace entry (`models/README.md` of the marketplace).
    if !template.free_covariates
        && let Some(name) = draft
            .covariates
            .iter()
            .find(|name| !template.allowed_covariates.contains(name))
    {
        return Err(match template.allowed_covariates.is_empty() {
            true => format!("{model} takes no additional covariates; remove --covariates"),
            false => format!(
                "{model} takes only these additional covariates: {}; remove `{name}`",
                template.allowed_covariates.join(", ")
            ),
        });
    }
    Ok(())
}

/// Why `variant` cannot be the name of a configured model, if it cannot.
pub fn check_variant(variant: &str) -> std::result::Result<(), String> {
    if variant.is_empty() {
        return Err("the name is empty; give one with --name, such as --name weekly_lags".into());
    }
    if !variant
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(format!(
            "`{variant}` is not a name chap-devops gives a configured model; use letters, digits, \
             `_`, `-` and `.`"
        ));
    }
    if variant.starts_with(TEST_PREFIX) {
        return Err(format!(
            "`{variant}` starts with `{TEST_PREFIX}`, which marks what a test left; give \
             another name"
        ));
    }
    Ok(())
}

/// Create `draft` as a configured model of `template`.
pub fn create(api: &Api, template: &Template, draft: &Draft) -> Result<Json> {
    let body = serde_json::json!({
        "name": draft.variant,
        "model_template_id": template.id,
        "user_option_values": draft.values,
        "additional_continuous_covariates": draft.covariates,
    })
    .to_string();
    let answer = api.send_json("POST", CONFIGURED_MODELS_PATH, Some(&body))?;
    if !answer.is_success() {
        return Err(api.status_error(CONFIGURED_MODELS_PATH, &answer));
    }
    Ok(answer.json().unwrap_or(Json::Null))
}

/// Give the configured model `old` the values of `draft`, under its name.
///
/// chap-core 2.4 has no route that changes a configured model. A post with
/// a name the template has makes the new row the live one, so this posts
/// first and archives `old` after: when the archive fails, the name still
/// has a live row. The error of the archive comes back beside the new row.
/// chap-core gives back `old` itself for the values it had, and then there
/// is nothing to archive.
pub fn replace(
    api: &Api,
    template: &Template,
    old: i64,
    draft: &Draft,
) -> Result<(Json, Result<()>)> {
    let created = create(api, template, draft)?;
    let archived = match created.get("id").and_then(Json::as_i64) == Some(old) {
        true => Ok(()),
        false => archive(api, old),
    };
    Ok((created, archived))
}

/// The warning for an old row that [`replace`] could not archive.
pub fn not_archived(old: i64, err: &anyhow::Error) -> String {
    format!(
        "the new values are live, but the old row {old} was not archived: {err}; archive it \
         with `chap-devops api DELETE {CONFIGURED_MODELS_PATH}/{old}`"
    )
}

/// The line an update reports.
pub fn updated_line(variant: &str, model: &str) -> String {
    format!(
        "updated configured model {variant} of {model}; chap-core keeps the old values as an \
         archived configured model"
    )
}

/// Archive the configured model `id`.
pub fn archive(api: &Api, id: i64) -> Result<()> {
    let path = format!("{CONFIGURED_MODELS_PATH}/{id}");
    let answer = api.send("DELETE", &path, None)?;
    if !answer.is_success() {
        return Err(api.status_error(&path, &answer));
    }
    Ok(())
}

/// The path that retires every version of the template `id`.
pub fn retire_path(id: i64) -> String {
    format!("{TEMPLATES_PATH}/{id}?allVersions=true")
}

/// Retire every version of the template `id` and its configured models,
/// which is what `chap-admin uninstall` does.
///
/// chap-core archives the rows and keeps them for the backtests and
/// predictions that use them. A `from-service` of the same version shows the
/// template again, and [`sync`] posts the marketplace configurations again;
/// a configured model made by hand stays archived.
pub fn retire(api: &Api, id: i64) -> Result<()> {
    let path = retire_path(id);
    let answer = api.send("DELETE", &path, None)?;
    if !answer.is_success() {
        return Err(api.status_error(&path, &answer));
    }
    Ok(())
}

/// Whether `configs sync` leaves the model alone once `archived` is
/// archived: it does when another configured model of the same version is
/// not archived. The same rule as [`sync::is_configured`].
pub fn sync_leaves_alone(existing: &[Config], archived: &Config) -> bool {
    existing.iter().any(|config| {
        config.id != archived.id
            && !config.archived
            && !config.is_test()
            && match (config.version.as_deref(), archived.version.as_deref()) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
    })
}

#[cfg(test)]
mod tests;
