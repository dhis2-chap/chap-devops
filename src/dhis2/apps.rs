//! The apps: what the App Hub publishes, which version fits, and what the
//! instance already has.

use super::{
    APP_HUB_INSTALL_PATH, APP_HUB_VAR, DEFAULT_APP_HUB, USER_AGENT, compare_parts,
    compare_versions, dhis_version_parts,
};
use crate::error::{ChapError, Result};
use serde::Deserialize;
use std::time::Duration;

/// One app as the App Hub describes it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubApp {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub versions: Vec<HubVersion>,
}

/// One published version of it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubVersion {
    /// The id `POST /api/appHub/{id}` installs.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub min_dhis_version: Option<String>,
    #[serde(default)]
    pub max_dhis_version: Option<String>,
}

/// One app as the instance lists it under `GET /api/apps`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct InstalledApp {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub version: String,
}

/// Base of the App Hub this run reads, for the tests to move.
pub fn app_hub_base() -> String {
    crate::github::base(APP_HUB_VAR, DEFAULT_APP_HUB)
}

/// `GET {app_hub}/{id}`: what the App Hub publishes for one app.
///
/// Its own request rather than one through DHIS2: the version id has to be
/// resolved before the instance can be told to install it, and this is the only
/// place that number exists. It is also why the step needs the network twice -
/// once from here for the id, once from DHIS2 for the app itself.
pub fn fetch_hub_app(base: &str, id: &str, timeout: Duration) -> Result<HubApp> {
    let url = format!("{}/{id}", base.trim_end_matches('/'));
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .user_agent(USER_AGENT)
            .http_status_as_error(false)
            .build(),
    );
    crate::output::verbose(&format!("GET {url}"));
    let mut response = agent
        .get(&url)
        .header("Accept", crate::api::JSON)
        .call()
        .map_err(|e| {
            anyhow::anyhow!(
                "the DHIS2 App Hub at {url} could not be reached: {e}; install both apps from \
                 DHIS2's own App Management page instead, or fix this machine's reach to \
                 apps.dhis2.org"
            )
        })?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| anyhow::anyhow!("reading the answer from {url}: {e}"))?;
    crate::output::debug(&format!("  {}", crate::output::trace_body(&body)));
    if !(200..300).contains(&status) {
        return Err(ChapError::Http {
            url: url.clone(),
            status,
        }
        .into());
    }
    serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("the App Hub's answer for {id} is not an app listing: {e}"))
}

/// The newest version of an app this DHIS2 can run.
///
/// `dhis_version` is the instance's own, and a version whose `minDhisVersion`
/// is newer than it is skipped: DHIS2 installs it happily and the app then
/// fails in the browser, which is a worse outcome than installing the newest
/// one that fits. An empty `dhis_version` - an instance that did not say -
/// takes the newest published version, because refusing on a missing field
/// would refuse everything.
pub fn pick_version<'a>(versions: &'a [HubVersion], dhis_version: &str) -> Option<&'a HubVersion> {
    let instance = dhis_version_parts(dhis_version);
    versions
        .iter()
        .filter(|version| !version.id.trim().is_empty())
        .filter(|version| instance.is_empty() || fits(version, &instance))
        .max_by(|a, b| compare_versions(&a.version, &b.version))
}

/// The one published version `wanted`, for `--modeling-app 7.5.0`, or why it
/// cannot be installed on a DHIS2 of `dhis_version`.
pub fn exact_version<'a>(
    versions: &'a [HubVersion],
    wanted: &str,
    dhis_version: &str,
) -> std::result::Result<&'a HubVersion, String> {
    let Some(version) = versions.iter().find(|version| {
        !version.id.trim().is_empty()
            && compare_versions(&version.version, wanted) == std::cmp::Ordering::Equal
    }) else {
        let mut published: Vec<&str> = versions.iter().map(|v| v.version.as_str()).collect();
        published.sort_by(|a, b| compare_versions(b, a));
        published.truncate(5);
        return Err(format!(
            "the App Hub has no version {wanted}; the newest are {}",
            published.join(", ")
        ));
    };
    let instance = dhis_version_parts(dhis_version);
    if !instance.is_empty() && !fits(version, &instance) {
        return Err(format!(
            "version {wanted} needs DHIS2 {} or newer, and this DHIS2 is {dhis_version}",
            version.min_dhis_version.as_deref().unwrap_or("?")
        ));
    }
    Ok(version)
}

/// Whether one published version runs on an instance of these parts.
fn fits(version: &HubVersion, instance: &[u64]) -> bool {
    let at_least = bound(version.min_dhis_version.as_deref())
        .map(|min| compare_parts(instance, &min) != std::cmp::Ordering::Less)
        .unwrap_or(true);
    let at_most = bound(version.max_dhis_version.as_deref())
        .map(|max| compare_parts(instance, &max) != std::cmp::Ordering::Greater)
        .unwrap_or(true);
    at_least && at_most
}

/// The version a declared bound names, or `None` where it names none.
///
/// **A blank bound is no bound, on both sides.** The App Hub sends a bound it
/// has not set as an empty string rather than as an absent field: every one of
/// the 32 published versions of the Modeling App carries
/// `"maxDhisVersion": ""`, and `#[serde(default)]` turns that into `Some("")`
/// and not into `None`. Read as a version, `""` has no parts at all, which
/// compares below every instance - so `2.42.6` sat *above* a maximum of
/// nothing, every version was filtered out, and `chap-devops dhis2 apps` reported
/// that the App Hub publishes no version this DHIS2 can run while a plain
/// `POST /api/appHub/{versionId}` installed it fine.
///
/// Missing, empty, whitespace and unreadable are therefore one answer. A bound
/// chap-devops cannot compare against is a bound it cannot honour, and refusing on it
/// would refuse everything - the same reason [`pick_version`] takes the newest
/// version when the instance itself did not say what it is.
fn bound(value: Option<&str>) -> Option<Vec<u64>> {
    let parts = dhis_version_parts(value.unwrap_or_default());
    (!parts.is_empty()).then_some(parts)
}

/// The app of that name the instance already has, if any.
///
/// Matched on the App Hub's name against the instance's `name` and `key`, with
/// the punctuation and the case taken out, because an installed app's key is its
/// name in another spelling (`DHIS2 Climate App` / `dhis2-climate-app`). An
/// exact match first, then one that contains the other, which covers the app
/// that is listed under a longer or shorter name than the App Hub publishes.
///
/// A wrong answer here costs a reinstall and nothing else:
/// `POST /api/appHub/{versionId}` is idempotent, so a miss installs an app that
/// was already there rather than skipping one that was not.
pub fn installed_app(apps: &serde_json::Value, name: &str) -> Option<InstalledApp> {
    let wanted = squash(name);
    let rows: Vec<InstalledApp> = apps
        .as_array()?
        .iter()
        .filter_map(|row| serde_json::from_value::<InstalledApp>(row.clone()).ok())
        .collect();
    rows.iter()
        .find(|app| squash(&app.name) == wanted || squash(&app.key) == wanted)
        .or_else(|| {
            rows.iter()
                .find(|app| near(&squash(&app.name), &wanted) || near(&squash(&app.key), &wanted))
        })
        .cloned()
}

/// A name with everything but its letters and digits removed, lowercased.
fn squash(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether two squashed names are the same app under different spellings.
///
/// Containment, with a floor on the length so that two short names cannot match
/// each other by accident.
fn near(a: &str, b: &str) -> bool {
    a.len() >= 6 && b.len() >= 6 && (a.contains(b) || b.contains(a))
}

/// Where `POST /api/appHub/{versionId}` installs from.
pub fn install_path(version_id: &str) -> String {
    format!(
        "{APP_HUB_INSTALL_PATH}/{}",
        crate::api::encode(version_id.trim())
    )
}

/// Why installing an app cannot work with `--offline`.
pub const OFFLINE_APPS: &str = "installing an app needs the DHIS2 App Hub, twice: chap-devops resolves \
     the version there and DHIS2 downloads the app itself; run the command without `--offline`, \
     or install both apps from DHIS2's own App Management page";
