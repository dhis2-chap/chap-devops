//! The values the compose templates are filled with.
//!
//! The types are frozen here; the constructors are owned by agent B.

use crate::components::Components;
use crate::compose::{AMD64_PLATFORM, overrides, tag_env_var, volume_name};
use crate::project::EnabledModel;
use crate::registry::{Model, Version};

/// Values for the base `compose.yml`.
#[derive(Debug, Clone)]
pub struct BaseSpec {
    /// chap-core's own `compose.ghcr.yml`, when the project records a tag it
    /// was downloaded from; `None` renders the copy compiled into the binary.
    pub upstream: Option<UpstreamCompose>,
}

/// chap-core's `compose.ghcr.yml` at one tag, as fetched.
///
/// The body is used verbatim - upstream is the source of truth for the base
/// stack, and the recorded SHA-256 only means something if nothing rewrites
/// it. `chap-devops` adds two header lines in front and nothing else.
#[derive(Debug, Clone)]
pub struct UpstreamCompose {
    pub tag: String,
    pub body: String,
}

/// The services chap-core's `compose.ghcr.yml` defines, for a base file
/// nothing could read the names out of.
pub const UPSTREAM_SERVICES: [&str; 4] = ["chap", "worker", "redis", "postgres"];

/// Values for `compose.chap-devops.yml`.
#[derive(Debug, Clone)]
pub struct ChapDevopsOverlaySpec {
    /// Rendered as the `${CHAP_API_PORT:-...}` default.
    pub api_port: u16,
    /// The top-level `name:`; see [`crate::compose::render::project_name_block`].
    pub project_name: Option<String>,
    /// A chap-core checkout chap and the worker are built from.
    pub checkout: Option<String>,
    /// The platform the `chap` image of a checkout is built for. The worker
    /// stays `linux/amd64`: its base image has no other platform.
    pub checkout_platform: Option<String>,
    /// Every service the base `compose.yml` defines, in its order: each one
    /// gets the chap-core labels, and none that is not there may be named,
    /// since compose would read it as a service with no image.
    pub services: Vec<String>,
    /// The `chap-devops run` group the deployment is.
    pub group: Option<String>,
}

impl ChapDevopsOverlaySpec {
    /// The spec for a base file with upstream's services and nothing else set.
    pub fn new(api_port: u16) -> ChapDevopsOverlaySpec {
        ChapDevopsOverlaySpec {
            api_port,
            project_name: None,
            checkout: None,
            checkout_platform: None,
            services: UPSTREAM_SERVICES.iter().map(|s| s.to_string()).collect(),
            group: None,
        }
    }
}

/// The docker platform of this machine: `linux/arm64` on Apple Silicon and
/// ARM servers, else `linux/amd64`.
///
/// A `chap` image built from a checkout uses it, so that the API runs
/// without emulation, as it does on a server. Its base image has both
/// platforms.
pub fn native_platform() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" | "arm64" => "linux/arm64",
        _ => "linux/amd64",
    }
}

/// The service names a compose file defines, in its order, or `None` for a
/// file that does not parse as one.
pub fn compose_services(text: &str) -> Option<Vec<String>> {
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(text).ok()?;
    let services = doc.get("services")?.as_mapping()?;
    Some(
        services
            .keys()
            .filter_map(|key| key.as_str().map(str::to_string))
            .collect(),
    )
}

/// Values for the generated `.env`.
#[derive(Debug, Clone)]
pub struct EnvSpec {
    pub postgres_user: String,
    pub postgres_password: String,
    pub postgres_db: String,
    /// `Some` when the chap image is pinned to something other than `latest`.
    pub chap_image_tag: Option<String>,
    /// Host port chap-core's API is published on. Written as an active line
    /// even at the default, so the one port the stack publishes is visible
    /// where an operator would look for it.
    pub api_port: u16,
    /// API token to write as an active `CHAP_API_TOKEN` line; `None` leaves
    /// the commented placeholder, which is chap-core's "no authentication".
    pub api_token: Option<String>,
    /// The same for `SERVICEKIT_REGISTRATION_KEY`. It travels with the API
    /// token: a protected API rejects an unauthenticated registration, so a
    /// deployment that has one needs the other.
    pub registration_key: Option<String>,
    /// `(<ID>_IMAGE_TAG, tag)` pairs written as commented-out pins.
    pub model_tag_pins: Vec<(String, String)>,
}

/// Values for `compose.ocs.yml`.
#[derive(Debug, Clone)]
pub struct OcsSpec {
    /// Host port OCS is published on, or `None` for an instance that is only
    /// `expose`d on the compose network - the reverse-proxy shape.
    pub host_port: Option<u16>,
    /// The tag the `${OCS_IMAGE_TAG:-...}` default carries.
    pub image_tag: String,
    /// The `${CLIMATE_SERVICE_BASE_URL:-...}` default: the public origin, or
    /// nothing, which OCS reads as "compose the links from the request".
    pub base_url: Option<String>,
    /// Whether the `s3` component is on, which is what decides if the service
    /// gets the (forward-looking) `S3_*` variables.
    pub s3: bool,
    /// Whether `ocs/plugins/` is there to mount. A filesystem fact rather than
    /// a recorded setting: the directory is the whole declaration.
    pub plugins: bool,
    /// The `chap-devops run` group the deployment is, which its labels name; see
    /// [`crate::compose::render::labels_block`].
    pub group: Option<String>,
}

impl OcsSpec {
    /// The spec a project's components describe, given whether the project has
    /// a plugin directory.
    pub fn from_components(components: &Components, plugins: bool) -> OcsSpec {
        OcsSpec {
            host_port: components.ocs.port,
            image_tag: components.ocs.image_tag.clone(),
            base_url: components.ocs.base_url.clone(),
            s3: components.s3.enabled,
            plugins,
            group: None,
        }
    }
}

/// Values for `compose.s3.yml`.
#[derive(Debug, Clone)]
pub struct S3Spec {
    /// Host port the object store is published on, or `None` for the default:
    /// reachable only inside the compose network, which is where OCS is.
    pub host_port: Option<u16>,
    pub image_tag: String,
    /// The `chap-devops run` group the deployment is, which its labels name; see
    /// [`crate::compose::render::labels_block`].
    pub group: Option<String>,
}

impl S3Spec {
    /// The spec a project's components describe.
    pub fn from_components(components: &Components) -> S3Spec {
        S3Spec {
            host_port: components.s3.port,
            image_tag: crate::components::S3_DEFAULT_TAG.to_string(),
            group: None,
        }
    }
}

/// Where the seed dump comes from, which is the one thing about it the compose
/// file has to render differently.
///
/// A URL is downloaded by the one-shot; a file is bind-mounted into it. Both end
/// up in the same `DHIS2_DB_DUMP_URL`, and the script decides by testing whether
/// the value names a file, so there is one code path in the container and one
/// variable `.env` can move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dhis2SeedSource {
    /// An `http://` or `https://` URL.
    Url(String),
    /// A path, absolute or relative to the deployment directory.
    File(String),
}

impl Dhis2SeedSource {
    /// Which of the two a written value is: the scheme is the whole of the rule,
    /// so nothing has to touch the filesystem to decide, and a path that is not
    /// there yet still renders (the operator may be about to copy the dump in).
    pub fn of(source: &str) -> Dhis2SeedSource {
        if source.starts_with("http://") || source.starts_with("https://") {
            Dhis2SeedSource::Url(source.to_string())
        } else {
            Dhis2SeedSource::File(source.to_string())
        }
    }
}

/// Values for `compose.dhis2.yml`.
#[derive(Debug, Clone)]
pub struct Dhis2Spec {
    /// Host port DHIS2's web UI and API are published on, or `None` for an
    /// instance that is only `expose`d on the compose network - the
    /// reverse-proxy shape.
    pub host_port: Option<u16>,
    /// The tag the `${DHIS2_IMAGE_TAG:-...}` default carries.
    pub image_tag: String,
    /// The image repository, `dhis2/core` unless the component names another.
    pub image: String,
    /// The dump the database is seeded from, behind the
    /// `${DHIS2_DB_DUMP_URL:-...}` default. `None` renders no seed one-shot and
    /// no dump volume at all, which is an empty DHIS2 that migrates itself on
    /// first boot.
    pub seed: Option<Dhis2SeedSource>,
    /// The password the restore gives every DHIS2 user. Only with a seed: an
    /// empty database has no users to give it to.
    pub seed_password: Option<String>,
    /// The size of the seed dump in bytes, when it is a file that sync found.
    /// The time the restore gets grows with it.
    pub seed_bytes: Option<u64>,
    /// Map `host.docker.internal` to the host gateway, for a DHIS2 whose route
    /// goes to a chap-core outside the deployment.
    pub host_gateway: bool,
    /// The `chap-devops run` group the deployment is, which its labels name; see
    /// [`crate::compose::render::labels_block`].
    pub group: Option<String>,
}

impl Dhis2Spec {
    /// The spec a project's components describe.
    ///
    /// The seed is resolved here rather than recorded already resolved, because
    /// `Dhis2Seed::Default` means "whatever the pinned minor line publishes" and
    /// the pin can move without the seed setting changing. A minor line with no
    /// dump in [`crate::components::DHIS2_SEED_DUMPS`] resolves to `None`, which
    /// renders an empty database;
    /// [`crate::components::dhis2_unknown_seed`] is the line that says so.
    pub fn from_components(components: &Components) -> Dhis2Spec {
        Dhis2Spec {
            host_port: components.dhis2.port,
            image_tag: components.dhis2.image_tag.clone(),
            image: components.dhis2.image.clone(),
            seed: components.dhis2_seed_source().map(Dhis2SeedSource::of),
            seed_password: components
                .dhis2_seed_source()
                .and(components.dhis2.seed_password.clone()),
            seed_bytes: None,
            host_gateway: components.chap_core_external.is_some(),
            group: None,
        }
    }
}

/// Values for the scaffolded `dhis2/dhis.conf`.
///
/// One field, because every other value in that file is a literal `${...}` DHIS2
/// substitutes from the environment the compose file hands the service. The
/// compose file is where those are decided; this is the one setting that has to
/// be in the file itself.
#[derive(Debug, Clone)]
pub struct Dhis2ConfigSpec {
    /// What `route.remote_servers_allowed` allows: one origin per entry, comma
    /// separated, and no path on any of them - DHIS2 throws on startup if one
    /// carries a path.
    pub route_allowed: String,
}

/// Every `http://` and `https://` target, which is what `dhis.conf` allows.
///
/// DHIS2 42 and later default to `https://*` alone and refuse the `http://`
/// target a chap-core on the compose network, on this machine or on a lab
/// server has, so each of those shapes used to need a `dhis.conf` edit and a
/// restart before `chap-devops dhis2 connect` could write the route. A DHIS2 chap-devops
/// deploys is a development and evaluation instance, where any chap-core the
/// operator points it at has to work without that step. DHIS2 logs a warning
/// about the wildcard on every start; a production DHIS2 is not one chap-devops
/// scaffolds, and its operator narrows the line in their own file.
pub const ROUTE_ALLOWED_ANY: &str = "http://*,https://*";

impl Default for Dhis2ConfigSpec {
    fn default() -> Dhis2ConfigSpec {
        Dhis2ConfigSpec {
            route_allowed: ROUTE_ALLOWED_ANY.to_string(),
        }
    }
}

/// Values for the scaffolded `ocs/climate-service.yaml`.
///
/// Everything but `example` comes from the `--ocs-*` flags; without them the
/// file is the Laos example, the country of the DHIS2 demo database chap-devops
/// seeds, and `example` is what says so - in the file, and to `chap-devops doctor`.
#[derive(Debug, Clone)]
pub struct OcsConfigSpec {
    /// STAC catalog id, derived from the name.
    pub id: String,
    /// Human-readable instance name.
    pub name: String,
    /// Name of the spatial extent: the country or region.
    pub extent_name: String,
    /// `xmin, ymin, xmax, ymax`, already formatted.
    pub bbox: String,
    /// ISO 3166-1 alpha-3 country code.
    pub country_code: String,
    /// Whether these are the example values rather than someone's own.
    pub example: bool,
}

impl Default for OcsConfigSpec {
    /// Laos, so climate data ingested by an OCS beside the demo DHIS2 covers
    /// the provinces that DHIS2 holds case data for.
    fn default() -> OcsConfigSpec {
        OcsConfigSpec {
            id: "laos-climate-service".to_string(),
            name: "Laos Climate Service".to_string(),
            extent_name: "Laos".to_string(),
            bbox: "100.0, 13.9, 107.7, 22.5".to_string(),
            country_code: "LAO".to_string(),
            example: true,
        }
    }
}

/// Whether one field of a scaffold was given on the command line.
///
/// Kept as three independent options rather than all-or-nothing: someone who
/// only knows the country code still gets it into the file, and the fields
/// they did not give stay the example's, which the note still points at.
#[derive(Debug, Clone, Default)]
pub struct OcsConfigRequest {
    pub name: Option<String>,
    pub country_code: Option<String>,
    pub bbox: Option<String>,
}

impl OcsConfigRequest {
    /// Whether anything at all was asked for.
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.country_code.is_none() && self.bbox.is_none()
    }

    /// The scaffold these flags describe, filling the gaps from the example.
    pub fn into_spec(self) -> OcsConfigSpec {
        let example = self.is_empty();
        let mut spec = OcsConfigSpec {
            example,
            ..OcsConfigSpec::default()
        };
        if let Some(name) = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            spec.extent_name = name.to_string();
            spec.name = format!("{name} Climate Service");
            spec.id = slug(&spec.name);
        }
        if let Some(code) = self
            .country_code
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            spec.country_code = code.to_ascii_uppercase();
        }
        if let Some(bbox) = self.bbox.as_deref() {
            spec.bbox = bbox
                .split(',')
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(", ");
        }
        spec
    }
}

/// `Sierra Leone Climate Service` -> `sierra-leone-climate-service`.
fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Values for one `compose.<service_id>.yml` overlay.
#[derive(Debug, Clone)]
pub struct OverlaySpec {
    pub id: String,
    pub service_id: String,
    pub version: String,
    pub repository: String,
    /// Tagless image reference.
    pub image: String,
    pub image_tag: String,
    /// The digest the overlay pins with the tag; see
    /// [`crate::compose::pinned_tag`].
    pub image_digest: Option<String>,
    /// See [`crate::compose::tag_env_var`].
    pub tag_env_var: String,
    /// Host port to publish 8000 on, or `None` for a service that is only
    /// `expose`d on the compose network - the default.
    pub host_port: Option<u16>,
    /// The host address [`OverlaySpec::host_port`] is published on, or `None`
    /// for every address the host has.
    pub bind: Option<std::net::IpAddr>,
    /// `Some("linux/amd64")` for R-INLA services.
    pub platform: Option<String>,
    pub data_dir: String,
    /// What the service runs as: `root`, a numeric `uid:gid`, or an account
    /// name nothing could turn into numbers. `root` is the one value that
    /// renders no `user:` line - the init container is rendered either way,
    /// chowning to `0:0`; see
    /// [`crate::compose::render::render_overlay`].
    pub user: String,
    /// See [`crate::compose::volume_name`].
    pub volume_name: String,
    /// Whether to emit an active `SERVICEKIT_REGISTRATION_KEY` line.
    pub registration_key: bool,
    /// The deployment has no chap-core: the service registers nowhere and
    /// waits for nothing but its own init container.
    pub standalone: bool,
    /// A chap-core elsewhere to register with, which wins over
    /// [`OverlaySpec::standalone`]: the service registers there and waits for
    /// nothing but its init container.
    pub external_chap_core: Option<ExternalRegistration>,
    /// The `chap-devops run` group the deployment is, which its labels name; see
    /// [`crate::compose::render::labels_block`].
    pub group: Option<String>,
    /// Whether the image reads its port from `PORT`, which is the one case
    /// where a model registered with a chap-core elsewhere listens on its
    /// host port; see [`crate::compose::resolve::reads_port_env`].
    pub reads_port: bool,
}

/// Where a model service registers when chap-core is not in the deployment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalRegistration {
    /// chap-core's base URL as a container reaches it.
    pub register_url: String,
    /// The host chap-core calls the service back at.
    pub models_host: String,
}

impl OverlaySpec {
    /// Build a spec for a freshly enabled model.
    ///
    /// `data_dir` and `user` override the table and the defaults when given,
    /// and [`crate::compose::apply()`] always gives them: it resolves both
    /// against the image itself ([`crate::compose::resolve`]) and records the
    /// answers. The table below it is the last resort, for a caller that
    /// resolved nothing.
    pub fn from_model(
        m: &Model,
        v: &Version,
        host_port: Option<u16>,
        data_dir: Option<&str>,
        user: Option<&str>,
    ) -> OverlaySpec {
        let known = overrides::known_override(&m.id);
        OverlaySpec {
            id: m.id.clone(),
            service_id: m.service_id.clone(),
            version: v.version.clone(),
            repository: m.source.repository.clone(),
            image: m.source.image.clone(),
            image_tag: v.image_tag.clone(),
            image_digest: v.image_digest.clone(),
            tag_env_var: tag_env_var(&m.id),
            host_port,
            bind: None,
            // Every model image the marketplace publishes today is amd64-only
            // (checked with `docker manifest inspect`; only the simple multistep
            // model also ships arm64), and chap-core itself is amd64-only, so
            // the whole stack already runs as linux/amd64. Pinning every
            // overlay makes an arm64 host pull the right variant instead of
            // failing with "no matching manifest".
            platform: Some(AMD64_PLATFORM.to_string()),
            data_dir: data_dir
                .map(str::to_string)
                .or_else(|| known.map(|k| k.data_dir.to_string()))
                .unwrap_or_else(|| overrides::DEFAULT_DATA_DIR.to_string()),
            user: user
                .map(str::to_string)
                .or_else(|| known.map(|k| k.user.to_string()))
                .unwrap_or_else(|| overrides::DEFAULT_USER.to_string()),
            volume_name: volume_name(&m.id),
            // chap-core only enforces a registration key when it has one
            // configured, so the generated overlay leaves the line commented.
            registration_key: false,
            standalone: false,
            external_chap_core: None,
            group: None,
            reads_port: false,
        }
    }

    /// Rebuild a spec from recorded state, as `chap-devops sync` does.
    ///
    /// Everything that affects the running service comes from the recorded
    /// entry; the marketplace only supplies the repository the header names.
    pub fn from_enabled(id: &str, e: &EnabledModel, m: &Model) -> OverlaySpec {
        OverlaySpec {
            repository: m.source.repository.clone(),
            ..OverlaySpec::from_enabled_without_registry(id, e)
        }
    }

    /// [`OverlaySpec::from_enabled`] for a model the registry no longer lists:
    /// the header names the image in place of the repository.
    pub fn from_enabled_without_registry(id: &str, e: &EnabledModel) -> OverlaySpec {
        OverlaySpec {
            id: id.to_string(),
            service_id: e.service_id.clone(),
            version: e.version.clone(),
            repository: e.image.clone(),
            image: e.image.clone(),
            image_tag: e.image_tag.clone(),
            image_digest: e.image_digest.clone(),
            tag_env_var: tag_env_var(id),
            host_port: e.host_port,
            bind: e.bind,
            platform: e.platform.clone(),
            data_dir: e.data_dir.clone(),
            user: e.user.clone(),
            volume_name: volume_name(id),
            registration_key: false,
            standalone: false,
            external_chap_core: None,
            group: None,
            reads_port: e.reads_port,
        }
    }
}

#[cfg(test)]
mod tests;
