//! `.chap-devops/` — the directory that records what a deployment is meant to be.
//!
//! Three YAML files: `project.yaml` holds the project-wide settings,
//! `models.yaml` the enabled model set and `components.yaml` the enabled
//! components. They are intent; the compose files at the project root are
//! artifacts rendered from them by `chap-devops sync`.

mod lock;
mod manual;
mod naming;
mod store;

pub use lock::{LOCK_FILE, StateLock};
pub use manual::{ManualModel, ManualModels};
pub use naming::new_compose_project_name;

use crate::components::Components;
use crate::compose::UserSource;
use crate::registry::Channel;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::path::PathBuf;

/// Directory that marks a project, at the root of a project directory.
pub const CHAP_DEVOPS_DIR: &str = ".chap-devops";
/// Project-wide settings, inside [`CHAP_DEVOPS_DIR`].
pub const PROJECT_FILE: &str = "project.yaml";
/// The enabled model set, inside [`CHAP_DEVOPS_DIR`].
pub const MODELS_FILE: &str = "models.yaml";
/// Models added with `chap-devops models add`, inside [`CHAP_DEVOPS_DIR`].
///
/// The marketplace's answer for a model it does not list: the definition the
/// catalogue would have carried, recorded per deployment. It is a definition,
/// not an enablement - [`MODELS_FILE`] still says which models are on.
pub const MANUAL_MODELS_FILE: &str = "models-manual.yaml";
/// Base compose file: chap-core, worker, valkey, postgres.
pub const BASE_COMPOSE: &str = "compose.yml";
/// chap-devops-owned overrides that sit on top of [`BASE_COMPOSE`]: the API's host
/// port, and anything else this CLI decides about the base stack.
///
/// It is a separate `-f` entry rather than an `include:` because a file listed
/// in `include:` cannot override a service the main file defines.
pub const CHAP_DEVOPS_COMPOSE: &str = "compose.chap-devops.yml";
/// Umbrella file that `include:`s one overlay per enabled model.
pub const MARKETPLACE_COMPOSE: &str = "compose.marketplace.yml";
/// Environment file docker compose picks up automatically.
pub const ENV_FILE: &str = ".env";
/// The compose file a chap-core checkout carries, which `compose.yml` is
/// rendered from for a deployment built from one.
pub const CHECKOUT_COMPOSE: &str = "compose.ghcr.yml";
/// The `.env` variable the chap-core images read their tag from.
pub const CHAP_TAG_ENV_VAR: &str = "CHAP_IMAGE_TAG";
/// The `.env` variable [`CHAP_DEVOPS_COMPOSE`] reads the API's host port from.
pub const API_PORT_ENV_VAR: &str = "CHAP_API_PORT";
/// The `.env` variable [`CHAP_DEVOPS_COMPOSE`] reads the number of API worker
/// processes from, passed to gunicorn as `WEB_CONCURRENCY`.
pub const API_WORKERS_ENV_VAR: &str = "CHAP_API_WORKERS";
/// The `.env` variable that gives chap-core's API a path prefix, for an
/// instance served behind a reverse proxy. Never written by `init`.
pub const ROOT_PATH_ENV_VAR: &str = "CHAP_ROOT_PATH";

/// `project.yaml` schema version written by this CLI.
pub const SCHEMA_VERSION: u32 = 1;
/// Host port range model overlays are allocated from.
pub const DEFAULT_PORT_RANGE: (u16, u16) = (5001, 5999);
/// Host port chap-core's API is published on unless `--api-port` says
/// otherwise.
///
/// Not the container's 8000: that, 8080 and 9000 are the ports every other
/// development server defaults to, so chap-devops's services start one block away
/// from them (8700 chap-core, 8780 DHIS2, 8790 OCS).
pub const DEFAULT_API_PORT: u16 = 8700;

/// The `-f` list a project written by this CLI has.
pub fn default_compose_files() -> Vec<String> {
    compose_files_for(&Components::default())
}

/// The `-f` list a project with these components has.
///
/// The base stack and the chap-devops-owned override only appear when chap-core is
/// one of the components; a deployment that is only OCS has neither. Component
/// files sit between the override and the marketplace umbrella, so a component
/// can add to the base stack and still be added to by a model overlay.
pub fn compose_files_for(components: &Components) -> Vec<String> {
    let mut files = Vec::new();
    if components.chap_core.enabled {
        files.push(BASE_COMPOSE.to_string());
        files.push(CHAP_DEVOPS_COMPOSE.to_string());
    }
    files.extend(components.compose_files());
    files.push(MARKETPLACE_COMPOSE.to_string());
    files
}

/// [`DEFAULT_API_PORT`], for `serde(default)` on a `project.yaml` that does
/// not set the field.
fn default_api_port() -> u16 {
    DEFAULT_API_PORT
}

/// Which file decided the host port chap-core's API is published on.
///
/// Two files can, and they do not always agree: `chap-devops init --api-port`
/// records one in `.chap-devops/project.yaml` and writes the same number into
/// `.env`, but `.env` belongs to the operator afterwards and compose reads it
/// last. So the answer has to come with its source, or a report naming a port
/// the recorded state does not mention looks like a bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiPortSource {
    /// An active `CHAP_API_PORT=` line in `.env`, which is what compose reads.
    Env,
    /// `api_port` in `.chap-devops/project.yaml`, because `.env` sets no usable one.
    Project,
}

impl ApiPortSource {
    /// How the source reads in a sentence: which file the number came from.
    pub fn label(self) -> &'static str {
        match self {
            ApiPortSource::Env => "from .env",
            ApiPortSource::Project => "from .chap-devops/project.yaml",
        }
    }
}

/// File name of the cached copy of chap-core's `compose.ghcr.yml` at `tag`,
/// inside [`CHAP_DEVOPS_DIR`].
///
/// The tag is part of the name, so switching tags never overwrites the copy
/// the running deployment was rendered from. Anything a tag may contain that a
/// file name should not (a slash, most of all) is folded to `_`.
pub fn cached_compose_file(tag: &str) -> String {
    let mut safe = String::with_capacity(tag.len());
    for ch in tag.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
            safe.push(ch);
        } else {
            safe.push('_');
        }
    }
    format!("compose.chap-core.{safe}.yml")
}
/// Where the base `compose.yml` is rendered from.
///
/// `compose.yml` is an artifact like the overlays: [`crate::compose::sync()`]
/// re-renders it, and this says from what. Old `project.yaml` files that
/// predate the field load as [`ComposeSource::Embedded`], which is what they
/// were written from.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComposeSource {
    /// The copy of chap-core's `compose.ghcr.yml` compiled into this binary.
    #[default]
    Embedded,
    /// chap-core's own `compose.ghcr.yml` at a tag, downloaded by `chap-devops init`
    /// or `chap-devops update` and kept in `.chap-devops/` so `sync` can replay it
    /// offline.
    Fetched {
        url: String,
        tag: String,
        /// SHA-256 of the cached copy as it was downloaded, so a later edit of
        /// it is noticed rather than silently rendered.
        sha256: String,
    },
    /// A chap-core checkout on this machine: `compose.yml` is rendered from
    /// its own `compose.ghcr.yml`, re-read on every sync, and the chap and
    /// worker images are built from it (`chap-devops init --source`).
    Checkout { path: String },
}

impl ComposeSource {
    /// The cached copy's file name inside [`CHAP_DEVOPS_DIR`], for a fetched source.
    pub fn cached_file(&self) -> Option<String> {
        match self {
            ComposeSource::Embedded | ComposeSource::Checkout { .. } => None,
            ComposeSource::Fetched { tag, .. } => Some(cached_compose_file(tag)),
        }
    }

    /// One-line description for human output.
    pub fn describe(&self) -> String {
        match self {
            ComposeSource::Embedded => "the compose.ghcr.yml built into this binary".to_string(),
            ComposeSource::Fetched { tag, .. } => format!("chap-core compose.ghcr.yml at {tag}"),
            ComposeSource::Checkout { path } => format!("the chap-core checkout at {path}"),
        }
    }
}

/// Which of chap-core's two shared secrets this deployment uses.
///
/// Booleans only, and deliberately so: the values themselves live in `.env`,
/// which is the file compose reads and the one nobody should copy around.
/// `.chap-devops/` records the intent, so `project.yaml` can be committed, backed up
/// and pasted into a bug report without leaking a credential. A
/// `project.yaml` without this field loads as both `false`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthState {
    /// `CHAP_API_TOKEN` is set, so every request needs
    /// `Authorization: Bearer <token>` outside [`crate::auth::OPEN_PATHS`].
    #[serde(default)]
    pub api_token: bool,
    /// `SERVICEKIT_REGISTRATION_KEY` is set, so each model overlay carries the
    /// line that hands the key to its service.
    #[serde(default)]
    pub registration_key: bool,
}

impl AuthState {
    /// Whether anything at all is protected.
    pub fn is_on(&self) -> bool {
        self.api_token || self.registration_key
    }
}

/// The in-memory project state: `project.yaml` plus `models.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectState {
    pub schema_version: u32,
    /// e.g. `chap-devops 0.1.0`.
    pub generated_by: String,
    pub chap_image_tag: String,
    /// Where `compose.yml` is rendered from.
    #[serde(default)]
    pub chap_compose_source: ComposeSource,
    /// The compose project name: the prefix every container and every named
    /// volume of this deployment carries.
    ///
    /// Compose derives it from the directory name unless a file says
    /// otherwise, so two deployments in directories both called `demo` share
    /// `demo_chap-db` and every other volume - including one left behind by a
    /// deployment deleted long ago. `chap-devops init` therefore generates
    /// `<slug>-<6 hex>` and `sync` renders it as the top-level `name:` of the
    /// files it owns.
    pub compose_project: String,
    pub registry_url: String,
    /// Host port chap-core's API is published on. Written into `.env` as
    /// `CHAP_API_PORT` and into [`CHAP_DEVOPS_COMPOSE`]; a `project.yaml` without
    /// it loads as [`DEFAULT_API_PORT`].
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    /// Which shared secrets `.env` sets. See [`AuthState`].
    #[serde(default)]
    pub auth: AuthState,
    /// Ordered `-f` list, relative to the project directory.
    pub compose_files: Vec<String>,
    pub port_range: (u16, u16),
    /// The address model host ports are published on when a model names none
    /// of its own: `127.0.0.1` for the deployment `chap-devops run` keeps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_bind: Option<IpAddr>,
    /// The `chap-devops run` group this deployment is, `None` for one `chap-devops init`
    /// wrote. Every container's labels name it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Files at the project root that `chap-devops sync` wrote last time, relative
    /// to the project directory. Only these are ever removed by a later sync.
    #[serde(default)]
    pub rendered_files: Vec<String>,
    /// Whether `chap-devops backup restore --adopt-identity` gave this directory
    /// the compose project name of another deployment. Its volumes then
    /// predate the directory on purpose, and `chap-devops doctor` does not warn
    /// about that.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub adopted_identity: bool,
    /// Enabled models, keyed by marketplace `id`. Lives in `models.yaml`.
    #[serde(skip)]
    pub models: BTreeMap<String, EnabledModel>,
    /// Models this deployment defines itself, keyed by id. Lives in
    /// `models-manual.yaml`; a project that has added none has no such file.
    #[serde(skip)]
    pub manual: ManualModels,
    /// What the deployment is made of. Lives in `components.yaml`; a project
    /// without that file loads as chap-core alone.
    #[serde(skip)]
    pub components: Components,
}

impl Default for ProjectState {
    fn default() -> Self {
        ProjectState {
            schema_version: SCHEMA_VERSION,
            generated_by: format!("chap-devops {}", env!("CARGO_PKG_VERSION")),
            chap_image_tag: "latest".to_string(),
            chap_compose_source: ComposeSource::Embedded,
            compose_project: String::new(),
            registry_url: crate::registry::DEFAULT_REGISTRY_URL.to_string(),
            api_port: DEFAULT_API_PORT,
            auth: AuthState::default(),
            compose_files: default_compose_files(),
            port_range: DEFAULT_PORT_RANGE,
            model_bind: None,
            group: None,
            rendered_files: Vec::new(),
            adopted_identity: false,
            models: BTreeMap::new(),
            manual: ManualModels::new(),
            components: Components::default(),
        }
    }
}

/// One enabled model, as recorded in `models.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnabledModel {
    /// Compose service name and DNS name.
    pub service_id: String,
    /// Tagless image reference.
    pub image: String,
    /// Image tag of the pinned version, e.g. `sha-fa880a1`.
    pub image_tag: String,
    /// The digest the marketplace lists for the version. The overlay pulls
    /// `image:tag@digest`, so the reviewed image runs even if the tag moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_digest: Option<String>,
    pub version: String,
    /// `Some` when the pin follows a channel, `None` when it is exact.
    pub channel: Option<Channel>,
    /// Host port the service is published on, when someone asked for one.
    ///
    /// `None` - the default - means the overlay only `expose`s port 8000:
    /// chap-core reaches the model over the compose network and a human goes
    /// through chap-core's proxy.
    #[serde(default)]
    pub host_port: Option<u16>,
    /// The host address [`host_port`] is published on: `127.0.0.1` keeps the
    /// model off the network. `None` follows [`ProjectState::model_bind`],
    /// and with neither compose publishes on every address the host has.
    ///
    /// [`host_port`]: EnabledModel::host_port
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind: Option<IpAddr>,
    pub data_dir: String,
    /// What the container runs as: `root`, a numeric `uid:gid`, or an account
    /// name nothing could turn into numbers.
    ///
    /// Resolved from the image when the model was enabled and recorded here,
    /// which is what keeps `chap-devops sync` offline: the overlay is rendered from
    /// this line, not from a lookup.
    pub user: String,
    /// Where [`user`] came from, for `models info`, the browser and
    /// `chap-devops doctor`. An entry without it reads as [`UserSource::Table`].
    ///
    /// [`user`]: EnabledModel::user
    #[serde(default)]
    pub user_from: UserSource,
    /// Whether the image reads its port from `PORT` rather than fixing 8000,
    /// read off the image's command with [`user`]. It decides the port a
    /// model listens on in its container when it registers with a chap-core
    /// elsewhere. See [`crate::compose::resolve::reads_port_env`].
    ///
    /// [`user`]: EnabledModel::user
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reads_port: bool,
    /// `Some("linux/amd64")` for a marketplace model.
    pub platform: Option<String>,
    /// Whether the registry lists a `linux/arm64` image next to the amd64
    /// one, read when the model was enabled or updated. The overlay then pins
    /// the host's own platform in place of [`platform`]; see
    /// [`crate::compose::spec::host_platform`].
    ///
    /// [`platform`]: EnabledModel::platform
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multi_arch: bool,
    /// Overlay file name, relative to the project directory.
    pub compose_file: String,
    /// The pin before `chap-devops update` moved it. It stays until chap-core
    /// has configured the new version, and `chap-devops up` goes back to it
    /// when chap-core refuses the new version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<PreviousPin>,
}

/// The parts of an [`EnabledModel`] that `chap-devops update` changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreviousPin {
    pub version: String,
    pub image_tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_digest: Option<String>,
    pub user: String,
    #[serde(default)]
    pub user_from: UserSource,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reads_port: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multi_arch: bool,
}

/// A project directory plus its parsed state.
#[derive(Debug, Clone)]
pub struct Project {
    pub dir: PathBuf,
    pub state: ProjectState,
}

impl EnabledModel {
    /// The tag with the digest, as the overlay pins it; see
    /// [`crate::compose::pinned_tag`].
    pub fn pinned_tag(&self) -> String {
        crate::compose::pinned_tag(&self.image_tag, self.image_digest.as_deref())
    }

    /// The image reference the overlay pulls when `.env` overrides nothing.
    pub fn image_ref(&self) -> String {
        crate::compose::image_ref(&self.image, &self.pinned_tag())
    }
}

impl Project {
    /// The `.chap-devops/` directory of this project.
    pub fn chap_devops_dir(&self) -> PathBuf {
        self.dir.join(CHAP_DEVOPS_DIR)
    }

    /// Absolute path of the cached `compose.ghcr.yml` this project's
    /// `compose.yml` is rendered from, when it is not the embedded copy.
    pub fn cached_compose_path(&self) -> Option<PathBuf> {
        self.state
            .chap_compose_source
            .cached_file()
            .map(|name| self.chap_devops_dir().join(name))
    }

    /// The compose project name this deployment records.
    ///
    /// `None` when the name is empty, so that no caller removes or names a
    /// volume under the bare prefix `_`. See [`ProjectState::compose_project`].
    pub fn compose_project(&self) -> Option<&str> {
        let name = self.state.compose_project.trim();
        (!name.is_empty()).then_some(name)
    }

    /// The same name as an owned string.
    ///
    /// Answered without asking docker, which is what makes it usable in
    /// `status` and in the volume checks.
    pub fn compose_project_name(&self) -> Option<String> {
        self.compose_project().map(str::to_string)
    }

    /// The prefix compose puts in front of every named volume of this
    /// deployment: `<project>_`.
    pub fn volume_prefix(&self) -> Option<String> {
        self.compose_project_name().map(|name| format!("{name}_"))
    }

    /// The name docker holds one of this deployment's named volumes under:
    /// the compose project name, then the name the compose file declares.
    ///
    /// Answered without asking docker, like [`Project::volume_prefix`] it is
    /// built on, so the commands that remove a volume can name it whether or
    /// not a daemon is reachable. `None` where this directory has no compose
    /// project name at all, and then there is no volume to name either.
    pub fn prefixed_volume(&self, volume: &str) -> Option<String> {
        self.volume_prefix()
            .map(|prefix| format!("{prefix}{volume}"))
    }

    /// Absolute paths of the ordered `-f` list.
    pub fn compose_file_paths(&self) -> Vec<PathBuf> {
        self.state
            .compose_files
            .iter()
            .map(|f| self.dir.join(f))
            .collect()
    }

    /// Host ports already claimed inside this deployment: the enabled models
    /// and the enabled components. Anything with no published port claims
    /// nothing.
    pub fn used_ports(&self) -> BTreeSet<u16> {
        let mut ports: BTreeSet<u16> = self
            .state
            .models
            .values()
            .filter_map(|m| m.host_port)
            .collect();
        for component in crate::components::Component::ALL {
            ports.extend(self.state.components.port_of(*component));
        }
        ports
    }

    /// Absolute path of the OCS instance config this project scaffolds.
    pub fn ocs_config_path(&self) -> PathBuf {
        self.dir
            .join(crate::components::OCS_DIR)
            .join(crate::components::OCS_CONFIG_FILE)
    }

    /// Absolute path of the OCS dataset plugin directory.
    ///
    /// Never created by this CLI: the directory existing is how a deployment
    /// says it has plugins, so creating an empty one would turn the mount on
    /// for every project.
    pub fn ocs_plugins_path(&self) -> PathBuf {
        self.dir
            .join(crate::components::OCS_DIR)
            .join(crate::components::OCS_PLUGINS_DIR)
    }

    /// The host port chap-core's API is actually published on, and which file
    /// decided that.
    ///
    /// `.env` wins, because compose reads it last and it is compose that does
    /// the publishing: an operator who wrote `CHAP_API_PORT=18000` there moved
    /// the port, whatever `.chap-devops/project.yaml` still records. Everything in
    /// this CLI that has to reach or reserve the API goes through here, so
    /// `status`, `doctor` and the `up` preflight all talk about the port the
    /// deployment really uses.
    ///
    /// A line that is commented out, empty or not a port is not an answer, and
    /// the recorded value stands.
    pub fn api_port_in_effect(&self) -> (u16, ApiPortSource) {
        match self.env_api_port() {
            Some(port) => (port, ApiPortSource::Env),
            None => (self.state.api_port, ApiPortSource::Project),
        }
    }

    /// [`Project::api_port_in_effect`] without the provenance.
    pub fn effective_api_port(&self) -> u16 {
        self.api_port_in_effect().0
    }

    /// A usable `CHAP_API_PORT` from this project's `.env`.
    ///
    /// Best-effort by design, like every other read of that file: a project
    /// written with `init --no-env` has none to read. Port 0 means "any free
    /// port" to the kernel and nothing at all to compose, so it is not a
    /// valid override either.
    fn env_api_port(&self) -> Option<u16> {
        let body = std::fs::read_to_string(self.dir.join(ENV_FILE)).ok()?;
        crate::dotenv::non_empty(&body, API_PORT_ENV_VAR)?
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
    }

    /// Base URL of chap-core's API on this machine: the one this deployment
    /// runs, or the one elsewhere it names.
    pub fn api_url(&self) -> String {
        match &self.state.components.chap_core_external {
            Some(external) => external.url.trim_end_matches('/').to_string(),
            None => format!("http://localhost:{}", self.effective_api_port()),
        }
    }

    /// The path prefix chap-core mounts its whole API under, from this
    /// project's `.env`, or `""` for the usual deployment that has none.
    ///
    /// `CHAP_ROOT_PATH` is never written by `init`: it is what a deployment
    /// behind a reverse proxy adds by hand, and chap-core passes it to FastAPI
    /// as `root_path`, so every route moves with it - `/docs` included. Read
    /// best-effort like every other line of that file, and normalised to a
    /// leading slash and no trailing one so it can be concatenated.
    ///
    /// Only the addresses a person is sent to use it. `chap-devops status` asks
    /// `/health`, which the container's own healthcheck reaches without the
    /// prefix, so nothing that probes the API is changed by this.
    pub fn root_path(&self) -> String {
        let Ok(body) = std::fs::read_to_string(self.dir.join(ENV_FILE)) else {
            return String::new();
        };
        let Some(value) = crate::dotenv::non_empty(&body, ROOT_PATH_ENV_VAR) else {
            return String::new();
        };
        let path = value
            .trim()
            .trim_end_matches('/')
            .trim_start_matches('/')
            .to_string();
        if path.is_empty() {
            String::new()
        } else {
            format!("/{path}")
        }
    }

    /// Where a person reaches chap-core's API from this machine: the origin plus
    /// whatever [`Project::root_path`] prefixes it with, without a trailing
    /// slash.
    pub fn api_base(&self) -> String {
        format!("{}{}", self.api_url(), self.root_path())
    }

    /// chap-core's read-only proxy to one model service, the way to reach a
    /// model that publishes no host port of its own.
    pub fn proxy_url(&self, service_id: &str) -> String {
        format!("{}/v2/services/{service_id}/run/", self.api_url())
    }
}

#[cfg(test)]
mod tests;
