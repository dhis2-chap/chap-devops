//! `models-manual.yaml`: the models a deployment defines itself.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

impl ManualModel {
    /// What `chap-devops models add` was given, or its equivalent: the repository
    /// when it came from one, else the pinned image reference.
    pub fn source(&self) -> String {
        match &self.repository {
            Some(repository) => repository.clone(),
            None if self.tag.starts_with('@') => format!("{}{}", self.image, self.tag),
            None => format!("{}:{}", self.image, self.tag),
        }
    }
}

/// The manual model definitions of a deployment, keyed by id.
pub type ManualModels = BTreeMap<String, ManualModel>;

/// One model added with `chap-devops models add`, as recorded in
/// `models-manual.yaml`.
///
/// Everything a marketplace file would have said about it, and nothing about
/// whether it is enabled: that stays in `models.yaml`, so a manually added
/// model can be disabled and enabled again like any other.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManualModel {
    /// Compose service name and DNS name. It has to match the id the service
    /// registers with chap-core under, or `chap-devops status` sees an unmanaged
    /// service next to a model that never arrived.
    pub service_id: String,
    pub display_name: String,
    /// The GitHub repository it was added from, when it was added from one.
    #[serde(default)]
    pub repository: Option<String>,
    /// Tagless image reference, lowercase.
    pub image: String,
    /// The pin: a `sha-<short commit>` tag, or `@sha256:...` for a digest.
    pub tag: String,
    /// The commit the tag was built from, where it is known.
    #[serde(default)]
    pub commit: Option<String>,
    /// The branch `chap-devops update` follows, or `None` for a pinned entry.
    #[serde(default)]
    pub follow: Option<String>,
    /// The data directory the image writes to, as `models add` resolved it.
    ///
    /// The marketplace's equivalent is [`crate::compose::overrides`], which
    /// is a table of images this CLI ships with and cannot grow an entry for
    /// a model it has never seen. Recording it here is what makes `models
    /// disable` followed by `models enable` bring the model back as it was,
    /// rather than on the chapkit defaults.
    #[serde(default)]
    pub data_dir: Option<String>,
    /// The `user:group` the container runs as, likewise.
    #[serde(default)]
    pub user: Option<String>,
    /// Whether the image is published for amd64 only, which is what the
    /// synthesised entry reports as the R-INLA runtime.
    #[serde(default)]
    pub runtime_amd64: bool,
    /// Whether the image reads its port from `PORT`, read off its command
    /// when it was added. See [`crate::compose::resolve::reads_port_env`].
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reads_port: bool,
    /// `YYYY-MM-DD`, the day it was added.
    pub added: String,
}

impl ManualModel {
    /// The marketplace entry this definition stands in for.
    ///
    /// One version, which both channels point at, so every path that resolves
    /// a model - `enable`, `sync`, `update`, the browser - reaches the
    /// recorded pin without a special case. The status is gray and the
    /// summary says where it came from, because nothing here was reviewed by
    /// the marketplace.
    pub fn to_model(&self, id: &str) -> crate::registry::Model {
        use crate::registry::model::{
            Attribution, Channels, Compatibility, Covariates, Kind, Source, Version, VersionStatus,
        };
        let origin = self
            .repository
            .clone()
            .unwrap_or_else(|| crate::compose::image_ref(&self.image, &self.tag));
        crate::registry::Model {
            schema_version: 2,
            id: id.to_string(),
            service_id: self.service_id.clone(),
            display_name: self.display_name.clone(),
            kind: Kind::Model,
            assessed_status: crate::registry::AssessedStatus::Gray,
            summary: format!("added manually from {origin}"),
            source: Source {
                repository: origin,
                image: self.image.clone(),
                runtime_image: match self.runtime_amd64 {
                    true => crate::registry::model::R_INLA_RUNTIME.to_string(),
                    false => String::new(),
                },
            },
            attribution: Attribution {
                author: String::new(),
                organization: None,
                contact: None,
                citation: None,
            },
            maintainers: Vec::new(),
            compatibility: Compatibility {
                period_types: Vec::new(),
                min_prediction_periods: 0,
                max_prediction_periods: 0,
                requires_geo: false,
            },
            covariates: Covariates {
                required: Vec::new(),
                defaults: Vec::new(),
                allow_free_additional: false,
            },
            channels: Channels {
                stable: self.tag.clone(),
                latest: self.tag.clone(),
            },
            versions: vec![Version {
                version: self.tag.clone(),
                commit: self.commit.clone().unwrap_or_default(),
                image_tag: self.tag.clone(),
                image_digest: None,
                chapkit: String::new(),
                status: VersionStatus::Unstable,
                verified_by: Vec::new(),
                changelog: None,
                notes: None,
            }],
            configurations: BTreeMap::new(),
            role: None,
            manual: true,
        }
    }

    /// The image reference this entry pins.
    pub fn image_ref(&self) -> String {
        crate::compose::image_ref(&self.image, &self.tag)
    }
}
