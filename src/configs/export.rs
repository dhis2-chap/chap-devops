//! Configured models in the marketplace format, out and in.
//!
//! A marketplace entry lists its configurations as `configurations: <name>:
//! {description, config}`, where `config` is the flat object chapkit takes
//! on `POST /api/v1/configs`: the user options, `additional_continuous_
//! covariates`, and `prediction_periods`, which the marketplace requires
//! (`models/README.md` in dhis2-chap/chap-marketplace). chap-core does not
//! store `prediction_periods` with a configured model, so an export takes it
//! from somewhere else and says where.

use super::{Config, Draft, Template, options};
use crate::registry::Model;
use crate::registry::model::Configuration;
use serde::Serialize;
use serde_json::{Map, Value as Json};
use std::collections::BTreeMap;

/// The key of the horizon in a marketplace `config`.
pub const PERIODS_KEY: &str = "prediction_periods";

/// The key of the covariate list in a marketplace `config`.
pub const COVARIATES_KEY: &str = "additional_continuous_covariates";

/// chapkit's own default horizon (`BaseConfig.prediction_periods`).
pub const CHAPKIT_PERIODS: i64 = 3;

/// Where the `prediction_periods` of one exported configuration came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PeriodsFrom {
    /// The marketplace configuration of the same name.
    Marketplace,
    /// The default in the service's own config schema.
    Service,
    /// chapkit's `BaseConfig` default, because the service did not say.
    Chapkit,
}

impl PeriodsFrom {
    /// The words a hint uses for it.
    pub fn words(self) -> &'static str {
        match self {
            PeriodsFrom::Marketplace => "the marketplace configuration of the same name",
            PeriodsFrom::Service => "the default in the service's config schema",
            PeriodsFrom::Chapkit => "chapkit's default, because the service gave none",
        }
    }
}

/// The `configurations` block of one model.
///
/// `entry` is its marketplace entry, when it has one, and `service_periods`
/// the default horizon the service declares. A configured model without a
/// marketplace twin gets a description that names where it came from, for
/// the reader to replace.
pub fn configurations(
    configs: &[Config],
    model: &str,
    entry: Option<&Model>,
    service_periods: Option<i64>,
) -> (
    BTreeMap<String, Configuration>,
    BTreeMap<String, PeriodsFrom>,
) {
    let mut out = BTreeMap::new();
    let mut from = BTreeMap::new();
    for config in configs {
        let twin = entry.and_then(|entry| entry.configurations.get(&config.variant));
        let (periods, source) = match twin.and_then(|t| t.config.get(PERIODS_KEY)).cloned() {
            Some(periods) => (periods, PeriodsFrom::Marketplace),
            None => match service_periods {
                Some(periods) => (Json::from(periods), PeriodsFrom::Service),
                None => (Json::from(CHAPKIT_PERIODS), PeriodsFrom::Chapkit),
            },
        };
        let mut object = Map::new();
        object.insert(PERIODS_KEY.to_string(), periods);
        for (key, value) in &config.values {
            object.insert(key.clone(), value.clone());
        }
        object.insert(
            COVARIATES_KEY.to_string(),
            Json::from(config.covariates.clone()),
        );
        let description = twin
            .and_then(|t| t.description.clone())
            .unwrap_or_else(|| format!("The configured model {} of {model}.", config.variant));
        out.insert(
            config.variant.clone(),
            Configuration {
                description: Some(description),
                config: Json::Object(object),
            },
        );
        from.insert(config.variant.clone(), source);
    }
    (out, from)
}

/// The block as YAML, ready to paste under a marketplace entry.
pub fn to_yaml(configurations: &BTreeMap<String, Configuration>) -> String {
    #[derive(Serialize)]
    struct Document<'a> {
        configurations: &'a BTreeMap<String, Configuration>,
    }
    serde_yaml_ng::to_string(&Document { configurations }).unwrap_or_default()
}

/// The configurations in a file: an exported block, or a whole marketplace
/// entry, which has the same `configurations` key.
pub fn from_yaml(text: &str) -> Result<BTreeMap<String, Configuration>, String> {
    #[derive(serde::Deserialize)]
    struct Document {
        configurations: Option<BTreeMap<String, Configuration>>,
    }
    let document: Document = serde_yaml_ng::from_str(text)
        .map_err(|err| format!("it is not YAML of the marketplace format: {err}"))?;
    match document.configurations {
        Some(configurations) if !configurations.is_empty() => Ok(configurations),
        _ => Err("it has no `configurations:` block".to_string()),
    }
}

/// The configured model one configuration of a file stands for.
///
/// The same split as [`super::sync::requests`]: the covariate list is a field
/// of its own, and the reserved keys are left out.
pub fn draft_of(name: &str, configuration: &Configuration) -> Result<Draft, String> {
    let Some(object) = configuration.config.as_object() else {
        return Err(format!("the `config` of {name} is not a mapping"));
    };
    let mut values = object.clone();
    let covariates = match values.remove(COVARIATES_KEY) {
        None | Some(Json::Null) => Vec::new(),
        Some(Json::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_string))
            .collect::<Option<Vec<String>>>()
            .ok_or_else(|| format!("the covariates of {name} are not a list of names"))?,
        Some(_) => return Err(format!("the covariates of {name} are not a list of names")),
    };
    for key in super::sync::RESERVED_CONFIG_KEYS {
        values.remove(*key);
    }
    Ok(Draft {
        variant: name.to_string(),
        values,
        covariates,
    })
}

/// Why the values of `draft` do not fit the options of `template`, if they
/// do not.
pub fn check_values(draft: &Draft, template: &Template, model: &str) -> Result<(), String> {
    for (key, value) in &draft.values {
        let Some(option) = template.options.iter().find(|option| &option.key == key) else {
            return Err(format!(
                "{}: `{key}` is not an option of {model}; its options are {}",
                draft.variant,
                options::options_list(&template.options)
            ));
        };
        if !options::fits(option, value) {
            return Err(format!(
                "{}: `{}` is not a value of `{key}`, which takes {}",
                draft.variant,
                options::text_of(value),
                option.kind.phrase()
            ));
        }
    }
    Ok(())
}

/// The default horizon in a chapkit config schema
/// (`GET /api/v1/configs/$schema`).
pub fn schema_periods(schema: &Json) -> Option<i64> {
    schema
        .get("properties")?
        .get(PERIODS_KEY)?
        .get("default")?
        .as_i64()
}

#[cfg(test)]
mod tests;
