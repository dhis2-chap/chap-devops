//! Serde types for the Chap Marketplace schema (schema_version 2).
//!
//! These mirror the YAML in <https://github.com/dhis2-chap/chap-marketplace>
//! verbatim. Unknown fields are tolerated on purpose: the marketplace may add
//! fields before the CLI learns about them, and an old CLI must keep working.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `registry.yaml` — the marketplace index.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistryIndex {
    pub schema_version: u32,
    pub marketplace: Marketplace,
    pub review_policy: ReviewPolicy,
    /// Relative paths of the model files, e.g. `models/chapkit_ewars_model.yaml`.
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Marketplace {
    pub name: String,
    pub description: String,
    pub repository: String,
    #[serde(default)]
    pub documentation: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReviewPolicy {
    pub required_approvals: u32,
    #[serde(default)]
    pub note: Option<String>,
}

/// `models/<id>.yaml` — one marketplace entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Model {
    pub schema_version: u32,
    /// Marketplace identifier, e.g. `chapkit_ewars_model`.
    pub id: String,
    /// Compose service name and DNS name, e.g. `chapkit-ewars-model`.
    pub service_id: String,
    pub display_name: String,
    pub kind: Kind,
    pub assessed_status: AssessedStatus,
    pub summary: String,
    pub source: Source,
    pub attribution: Attribution,
    #[serde(default)]
    pub maintainers: Vec<String>,
    pub compatibility: Compatibility,
    pub covariates: Covariates,
    pub channels: Channels,
    pub versions: Vec<Version>,
    #[serde(default)]
    pub configurations: BTreeMap<String, Configuration>,
    /// What chap-core compares with the model: `comparison` flags a model
    /// that is compared against on request. chap-core v2.5.0 and later stores
    /// it on the template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Whether this entry is one the deployment defines itself
    /// ([`crate::project::ManualModel`]) rather than one the marketplace
    /// lists.
    ///
    /// Never read from a model file: a catalogue that started publishing a
    /// `manual:` key must not be able to make one of its own entries claim it
    /// is local. It is written to `--json`, which is where a caller reads it.
    #[serde(default, skip_deserializing)]
    pub manual: bool,
}

/// Whether an entry is deployable or only scaffolding to copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Model,
    Starter,
}

/// The author's own chapkit assessment, not a marketplace verdict.
///
/// The scale is chapkit's (`src/chapkit/api/service_builder.py`), mirrored in
/// chap-core's `docs/external_models/model_metadata.md`. It runs from "do not
/// use this" to "this has been validated"; what each colour means is
/// [`AssessedStatus::describe`], because a bare colour word tells a reader
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AssessedStatus {
    Green,
    Yellow,
    Orange,
    Red,
    Gray,
}

impl AssessedStatus {
    /// The colour as the marketplace writes it, which is what `--json` and the
    /// model files use.
    pub fn colour(&self) -> &'static str {
        match self {
            AssessedStatus::Green => "green",
            AssessedStatus::Yellow => "yellow",
            AssessedStatus::Orange => "orange",
            AssessedStatus::Red => "red",
            AssessedStatus::Gray => "gray",
        }
    }

    /// Where this assessment sits on the maturity scale, most mature first.
    ///
    /// Every list of the catalogue is in this order rather than in the order
    /// the marketplace index happens to name its files: a reader looking for
    /// something to run wants what has been validated at the top and what
    /// nobody should run at the bottom.
    pub fn rank(&self) -> u8 {
        match self {
            AssessedStatus::Green => 0,
            AssessedStatus::Yellow => 1,
            AssessedStatus::Orange => 2,
            AssessedStatus::Red => 3,
            AssessedStatus::Gray => 4,
        }
    }

    /// What the colour means, in the width of a table cell.
    pub fn label(&self) -> &'static str {
        match self {
            AssessedStatus::Green => "production",
            AssessedStatus::Yellow => "testing",
            AssessedStatus::Orange => "limited data",
            AssessedStatus::Red => "experimental",
            AssessedStatus::Gray => "not for use",
        }
    }

    /// What the colour means, in full, for the places with room to say it.
    pub fn describe(&self) -> &'static str {
        match self {
            AssessedStatus::Green => "validated and ready for production use",
            AssessedStatus::Yellow => "ready for more rigorous testing on diverse data",
            AssessedStatus::Orange => {
                "shows promise on limited data, needs manual configuration and careful evaluation"
            }
            AssessedStatus::Red => {
                "highly experimental prototype, not validated, only for early experimentation"
            }
            AssessedStatus::Gray => {
                "not intended for use, deprecated or kept for backwards compatibility"
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Source {
    pub repository: String,
    /// Tagless image reference; the tag comes from the selected [`Version`].
    pub image: String,
    /// chapkit base image the service is built on, e.g. `ghcr.io/dhis2-chap/chapkit-r-inla`.
    pub runtime_image: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Attribution {
    pub author: String,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub contact: Option<String>,
    #[serde(default)]
    pub citation: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Compatibility {
    pub period_types: Vec<String>,
    pub min_prediction_periods: u32,
    pub max_prediction_periods: u32,
    #[serde(default)]
    pub requires_geo: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Covariates {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub defaults: Vec<String>,
    #[serde(default)]
    pub allow_free_additional: bool,
}

/// Channel pointers into [`Model::versions`].
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Channels {
    pub stable: String,
    pub latest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Version {
    pub version: String,
    pub commit: String,
    /// Image tag for this pin, e.g. `sha-fa880a1`.
    pub image_tag: String,
    /// chapkit version requirement, e.g. `>=2.0.0,<3`.
    pub chapkit: String,
    pub status: VersionStatus,
    #[serde(default)]
    pub verified_by: Vec<String>,
    #[serde(default)]
    pub changelog: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl Version {
    /// Whether the image tag is `sha-` and at least 7 characters that start
    /// the commit, as `chap-admin` requires. Another tag, such as `latest`,
    /// can move to other code after the version was reviewed.
    pub fn is_pinned(&self) -> bool {
        self.image_tag
            .strip_prefix("sha-")
            .is_some_and(|prefix| prefix.len() >= 7 && self.commit.starts_with(prefix))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionStatus {
    Verified,
    Unstable,
    Deprecated,
    Yanked,
}

/// A verified configuration; `config` is the flat object chapkit accepts as
/// `data` on `POST /api/v1/configs`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Configuration {
    #[serde(default)]
    pub description: Option<String>,
    pub config: serde_json::Value,
}

/// Release channel a model can be pinned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Latest,
}

impl Channel {
    /// Lowercase name, as used on the command line and in `.chap-devops/models.yaml`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Latest => "latest",
        }
    }
}

/// How the caller asked for a version: follow a channel, or pin exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSelector {
    Channel(Channel),
    Exact(String),
}

impl Default for VersionSelector {
    fn default() -> Self {
        VersionSelector::Channel(Channel::Stable)
    }
}

/// The chapkit R-INLA runtime base image; amd64 only.
pub const R_INLA_RUNTIME: &str = "ghcr.io/dhis2-chap/chapkit-r-inla";

impl Model {
    /// Scaffolding rather than a deployable forecasting model.
    pub fn is_starter(&self) -> bool {
        self.kind == Kind::Starter
    }

    /// Look up a version by its `version` string.
    pub fn version(&self, v: &str) -> Option<&Version> {
        self.versions
            .iter()
            .find(|candidate| candidate.version == v)
    }

    /// Resolve a selector to a concrete version.
    ///
    /// Errors with [`crate::error::ChapError::UnknownVersion`] when the version
    /// (or the version a channel points at) is absent, and with
    /// [`crate::error::ChapError::YankedVersion`] when it is yanked. The
    /// stable channel of a marketplace model takes a `verified` version
    /// only, as `chap-admin install` does; an exact `--version` is the way to
    /// take another one.
    pub fn resolve(&self, sel: &VersionSelector) -> crate::error::Result<&Version> {
        let wanted = match sel {
            VersionSelector::Channel(Channel::Stable) => self.channels.stable.clone(),
            VersionSelector::Channel(Channel::Latest) => self.channels.latest.clone(),
            VersionSelector::Exact(v) => v.clone(),
        };
        let found =
            self.version(&wanted)
                .ok_or_else(|| crate::error::ChapError::UnknownVersion {
                    id: self.id.clone(),
                    version: wanted.clone(),
                })?;
        if matches!(sel, VersionSelector::Channel(Channel::Stable))
            && !self.manual
            && !matches!(
                found.status,
                VersionStatus::Verified | VersionStatus::Yanked
            )
        {
            return Err(crate::error::ChapError::UnverifiedStable {
                id: self.id.clone(),
                version: wanted,
                status: format!("{:?}", found.status).to_lowercase(),
            }
            .into());
        }
        if found.status == VersionStatus::Yanked {
            return Err(crate::error::ChapError::YankedVersion {
                id: self.id.clone(),
                version: wanted,
            }
            .into());
        }
        if !self.manual && !found.is_pinned() {
            return Err(crate::error::ChapError::UnpinnedImage {
                id: self.id.clone(),
                version: wanted,
                image_tag: found.image_tag.clone(),
            }
            .into());
        }
        Ok(found)
    }

    /// Whether the entry is the one its file names: the `id` is the name of
    /// the file, and the `service_id` is the `id` with `-` for `_`, as
    /// `chap-admin` requires.
    pub fn identity_matches(&self, path: &str) -> bool {
        let stem = std::path::Path::new(path)
            .file_stem()
            .and_then(|stem| stem.to_str());
        stem == Some(self.id.as_str()) && self.service_id == self.id.replace('_', "-")
    }

    /// Fully qualified image reference for a version.
    pub fn image_ref(&self, v: &Version) -> String {
        crate::compose::image_ref(&self.source.image, &v.image_tag)
    }

    /// Whether the service needs `platform: linux/amd64`, i.e. whether it is
    /// built on the R-INLA runtime.
    /// How the catalogue is listed: starters after models, then by
    /// maturity, then by name, because a tie on maturity is not a reason to
    /// fall back on the order a directory listing happened to have.
    ///
    /// The name is compared without case so `auto_arima` and `Auto-ARIMA`
    /// sort where a reader looks for them.
    pub fn order_key(&self) -> (bool, u8, String) {
        (
            self.is_starter(),
            self.assessed_status.rank(),
            self.display_name.to_lowercase(),
        )
    }

    pub fn needs_amd64(&self) -> bool {
        runtime_base(&self.source.runtime_image) == R_INLA_RUNTIME
    }
}

/// Strip a `:tag` suffix from an image reference, leaving the repository.
///
/// A colon inside the registry host's port (`host:5000/img`) is not a tag, so
/// only a colon after the last `/` counts.
fn runtime_base(image: &str) -> &str {
    let last_slash = image.rfind('/').map(|i| i + 1).unwrap_or(0);
    match image[last_slash..].find(':') {
        Some(i) => &image[..last_slash + i],
        None => image,
    }
}

#[cfg(test)]
mod tests;
