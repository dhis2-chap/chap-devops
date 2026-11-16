//! The per-component blocks of `components.yaml` and their `serde` defaults.

use super::*;

/// `true`, for the `serde` default of a flag that is on unless said otherwise.
fn enabled_by_default() -> bool {
    true
}

fn default_ocs_port() -> Option<u16> {
    Some(OCS_DEFAULT_PORT)
}

fn default_ocs_tag() -> String {
    OCS_DEFAULT_TAG.to_string()
}

/// chap-core's own block. It has no settings of its own: the API port, the
/// image tag and the model set all live in `project.yaml` and `models.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChapCoreComponent {
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

impl Default for ChapCoreComponent {
    fn default() -> ChapCoreComponent {
        ChapCoreComponent { enabled: true }
    }
}

/// The `ocs` block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OcsComponent {
    #[serde(default)]
    pub enabled: bool,
    /// Host port OCS is published on. OCS has a web interface, so it is
    /// published by default rather than hidden on the compose network; `None`
    /// keeps it on the compose network alone, for a deployment whose way in is
    /// a reverse proxy.
    #[serde(default = "default_ocs_port")]
    pub port: Option<u16>,
    /// The public origin OCS builds absolute links from, for an instance
    /// reached through a proxy rather than on its own port. `None` leaves OCS
    /// to compose them from the request, which behind a proxy names the
    /// internal address.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Whether the instance config turns ingestion over HTTP off.
    ///
    /// A record of what `ocs/climate-service.yaml` says, not the setting
    /// itself: that file is the operator's, and OCS reads it and not this.
    #[serde(default)]
    pub read_only: bool,
    /// The tag the compose file defaults to, and the one `.env` pins.
    #[serde(default = "default_ocs_tag")]
    pub image_tag: String,
}

impl Default for OcsComponent {
    fn default() -> OcsComponent {
        OcsComponent {
            enabled: false,
            port: Some(OCS_DEFAULT_PORT),
            base_url: None,
            read_only: false,
            image_tag: OCS_DEFAULT_TAG.to_string(),
        }
    }
}

/// The `s3` block.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct S3Component {
    #[serde(default)]
    pub enabled: bool,
    /// Host port the object store is published on. `None` — the default —
    /// keeps it inside the compose network, which is where OCS reaches it.
    #[serde(default)]
    pub port: Option<u16>,
}

fn default_dhis2_port() -> Option<u16> {
    Some(DHIS2_DEFAULT_PORT)
}

fn default_dhis2_tag() -> String {
    DHIS2_DEFAULT_TAG.to_string()
}

fn default_dhis2_image() -> String {
    DHIS2_IMAGE.to_string()
}

/// The `dhis2` block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dhis2Component {
    #[serde(default)]
    pub enabled: bool,
    /// Host port DHIS2 is published on. DHIS2 is a web application people log
    /// into, so it is published by default rather than hidden on the compose
    /// network; `None` keeps it inside, for a deployment whose way in is a
    /// reverse proxy.
    #[serde(default = "default_dhis2_port")]
    pub port: Option<u16>,
    /// The tag the compose file defaults to, and the one `.env` pins.
    #[serde(default = "default_dhis2_tag")]
    pub image_tag: String,
    /// The image repository: `dhis2/core` for releases, and another (such as
    /// `dhis2/core-dev` with tag `master`, the next version) for trying one
    /// that is not released yet.
    #[serde(default = "default_dhis2_image")]
    pub image: String,
    /// What the database is restored from the first time it is created.
    #[serde(default)]
    pub seed: Dhis2Seed,
    /// Whether the restore of the seed gives every DHIS2 user one password,
    /// and turns on every account. The password is in `.env` as
    /// [`DHIS2_SEED_PASSWORD_ENV_VAR`](crate::components::DHIS2_SEED_PASSWORD_ENV_VAR),
    /// never here. `false` keeps the passwords of the dump.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub seed_password: bool,
    /// When `chap-devops dhis2 connect` last got as far as a verified route and both
    /// apps here, as a UTC timestamp. `None` for a deployment no such run has
    /// been recorded for, which is every deployment until one happens.
    ///
    /// **A record of a command that ran, not a fact about DHIS2, and never
    /// evidence.** It exists for one purpose: to stop `chap-devops up` and
    /// `chap-devops status` naming [`dhis2_connect_hint`] on a deployment that has
    /// already been through it. Nothing decides anything else by it. The route
    /// can be deleted, repointed or disabled in DHIS2's own interface a minute
    /// later and this timestamp will not move, because nothing here asks - only
    /// `chap-devops dhis2 show` does, and it asks DHIS2 rather than this file.
    ///
    /// So it is cleared wherever the thing it was true of can have gone:
    /// [`Components::set_enabled`] forgets it when the component is switched
    /// off - the seed dump a re-enabled DHIS2 restores ships a `chap` route
    /// pointing at somebody else's Chap, and a stale record would hide exactly
    /// that - and `chap-devops down --volumes` forgets it with `dhis2_db` itself.
    ///
    /// `None` when nothing has recorded a connect.
    #[serde(default)]
    pub connected_at: Option<String>,
}

impl Default for Dhis2Component {
    fn default() -> Dhis2Component {
        Dhis2Component {
            enabled: false,
            port: Some(DHIS2_DEFAULT_PORT),
            image_tag: DHIS2_DEFAULT_TAG.to_string(),
            image: DHIS2_IMAGE.to_string(),
            seed: Dhis2Seed::Default,
            seed_password: false,
            connected_at: None,
        }
    }
}
