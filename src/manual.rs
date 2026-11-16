//! Models that are not in the marketplace: resolving what `chap-devops models add`
//! was given into an entry a deployment can run.
//!
//! The marketplace answers three questions about a model: which image, which
//! tag, and how the container has to be run. For a repository that is not in
//! the catalogue, those answers have to come from the repository and the
//! image themselves - GitHub for the commits, ghcr for the published tags and
//! the image config, and docker only where neither can say. What comes out is
//! recorded in `.chap-devops/models-manual.yaml` and synthesised back into a
//! marketplace-shaped entry ([`crate::project::ManualModel::to_model`]), so
//! every command downstream treats it like any other model.

pub mod ghcr;
pub mod github;
mod image;
pub mod location;
mod published;
pub mod source;

#[cfg(test)]
use image::data_dir_under;
pub(crate) use image::image_data_dir;
pub use image::{resolve_user, resolve_user_with};
pub use published::{BranchBuilds, branch_builds, commit_day, default_branch_of, newest_published};

use crate::compose::overrides;
use crate::error::Result;
use crate::registry::VersionSelector;
use image::image_config;
use published::pin;
use source::Source;
use std::time::Duration;

/// Default network timeout for the lookups `models add` makes.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// `User-Agent` sent with every request, matching the registry fetch.
const USER_AGENT: &str = concat!("chap-devops/", env!("CARGO_PKG_VERSION"));

/// Where the lookups go, and whether they may go anywhere at all.
///
/// The two URLs are overridable so the end-to-end tests can stand a local
/// server in for GitHub and ghcr; see `docs/development.md`. Nothing on the
/// command line moves them, because an operator who wants a different
/// registry wants a different image reference.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub github_api: String,
    pub ghcr_url: String,
    /// Where every registry other than ghcr answers, for the tests;
    /// `None` is each registry's own host.
    pub oci_url: Option<String>,
    /// `--offline`: the network is not to be touched.
    pub offline: bool,
    /// Whether the local docker daemon may be asked about an image at all:
    /// `docker image inspect` for a config the registry would not give up,
    /// and the `id -u` probe that pulls and runs one. Off in the tests, whose
    /// answers must not depend on what this machine has pulled.
    pub docker_probe: bool,
    pub timeout: Duration,
}

impl Default for Endpoints {
    fn default() -> Endpoints {
        Endpoints {
            github_api: github::DEFAULT_API.to_string(),
            ghcr_url: ghcr::DEFAULT_URL.to_string(),
            oci_url: None,
            offline: false,
            docker_probe: true,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl Endpoints {
    /// The endpoints this run uses: the public ones, unless the test hooks
    /// say otherwise.
    pub fn from_env(offline: bool) -> Endpoints {
        Endpoints {
            github_api: crate::github::api_base(),
            ghcr_url: std::env::var("CHAP_DEVOPS_GHCR_URL")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| ghcr::DEFAULT_URL.to_string()),
            oci_url: std::env::var("CHAP_DEVOPS_OCI_URL")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            offline,
            docker_probe: !matches!(
                std::env::var("CHAP_DEVOPS_NO_DOCKER_PROBE").as_deref(),
                Ok("1") | Ok("true")
            ),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

/// What `chap-devops models add` was asked for, before anything was looked up.
#[derive(Debug, Clone, Default)]
pub struct AddRequest {
    /// The repository URL or image reference, as typed.
    pub source: String,
    pub id: Option<String>,
    pub service_id: Option<String>,
    pub display_name: Option<String>,
    pub data_dir: Option<String>,
    pub user: Option<String>,
    /// `--runtime-amd64`, for an image whose index this CLI cannot read.
    pub runtime_amd64: bool,
}

/// Where one resolved value came from, so the report can say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A flag on the command line.
    Flag,
    /// The image's own config.
    Image,
    /// A `docker run` that asked the image itself.
    Probe,
    /// The chapkit default, because nothing else said.
    Default,
}

impl Origin {
    /// How the origin reads in the report.
    pub fn label(self) -> &'static str {
        match self {
            Origin::Flag => "given on the command line",
            Origin::Image => "from the image config",
            Origin::Probe => "from a docker probe",
            Origin::Default => "the chapkit default",
        }
    }
}

/// The pull the uid probe needs first: `reference -> did it work`.
pub type PullFn<'a> = &'a dyn Fn(&str) -> bool;
/// The uid probe itself: `(reference, account name) -> (uid, gid)`.
pub type ProbeFn<'a> = &'a dyn Fn(&str, &str) -> Option<(u32, u32)>;

/// Everything `models add` needs to write the entry, and nothing written yet.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub source: Source,
    pub id: String,
    pub service_id: String,
    pub display_name: String,
    /// Tagless image reference.
    pub image: String,
    /// The pin: a `sha-` tag, or `@sha256:...` for a digest.
    pub tag: String,
    /// The commit the tag was built from, where it is known.
    pub commit: Option<String>,
    /// The branch this entry follows, or `None` for a pinned one.
    pub follow: Option<String>,
    pub data_dir: String,
    pub data_dir_from: Origin,
    pub user: String,
    pub user_from: Origin,
    pub runtime_amd64: bool,
    /// Whether the image reads its port from `PORT`.
    pub reads_port: bool,
    /// What the resolution had to guess at, in the order it came up.
    pub notes: Vec<String>,
}

impl Resolved {
    /// The image reference this entry pins.
    pub fn image_ref(&self) -> String {
        crate::compose::image_ref(&self.image, &self.tag)
    }

    /// How the version is asked for when the entry is enabled: a channel for
    /// an entry that follows a branch, an exact pin for one that does not.
    ///
    /// Both resolve to the same single version of the synthesised model; the
    /// difference is what `.chap-devops/models.yaml` records, and therefore what
    /// `chap-devops update` sees.
    pub fn selector(&self) -> VersionSelector {
        match self.follow {
            Some(_) => VersionSelector::Channel(crate::registry::Channel::Latest),
            None => VersionSelector::Exact(self.tag.clone()),
        }
    }
}

/// What an add request is called, before anything is looked up.
///
/// Separate from [`resolve`] because these are the answers a collision is
/// decided on, and asking GitHub, ghcr and docker for an entry whose id is
/// already taken would mean pulling an image to be told to pick another name.
#[derive(Debug, Clone)]
pub struct Names {
    pub source: Source,
    pub id: String,
    pub service_id: String,
    pub display_name: String,
}

/// The id, service name and display name this request would be recorded
/// under, with nothing looked up.
pub fn names(req: &AddRequest) -> Result<Names> {
    let source = Source::parse(&req.source)?;
    let id = match &req.id {
        Some(id) => id.trim().to_string(),
        None => source.default_id(),
    };
    source::check_id(&id)?;
    let service_id = match &req.service_id {
        Some(service_id) => service_id.trim().to_string(),
        None => source::default_service_id(&id),
    };
    source::check_service_id(&service_id)?;
    if let Some(dir) = &req.data_dir {
        crate::compose::apply::check_data_dir(dir.trim())?;
    }
    if let Some(user) = &req.user {
        crate::compose::apply::check_user(user.trim())?;
    }
    let display_name = match &req.display_name {
        Some(name) => name.trim().to_string(),
        None => default_display_name(&source),
    };
    Ok(Names {
        source,
        id,
        service_id,
        display_name,
    })
}

/// Resolve an add request, writing nothing.
///
/// The order is the order the answers depend on each other: the source, then
/// the tag it pins, then what the image at that tag says about how it runs.
pub fn resolve(req: &AddRequest, endpoints: &Endpoints) -> Result<Resolved> {
    let Names {
        source,
        id,
        service_id,
        display_name,
    } = names(req)?;

    let image = source.image();
    let mut notes = Vec::new();
    let (tag, commit, follow) = pin(&source, endpoints)?;
    let reference = crate::compose::image_ref(&image, &tag);

    let config = image_config(&source, &reference, endpoints, &mut notes)?;
    let image_dir = match (&req.data_dir, &config) {
        (None, Some(config)) => image_data_dir(&config.working_dir, &reference, &mut notes),
        _ => None,
    };
    let data_dir = match (&req.data_dir, image_dir) {
        (Some(dir), _) => (dir.trim().to_string(), Origin::Flag),
        (None, Some(dir)) => (dir, Origin::Image),
        (None, None) => (overrides::DEFAULT_DATA_DIR.to_string(), Origin::Default),
    };
    let declared = config.as_ref().map(|c| c.user.as_str()).unwrap_or("");
    let (user, user_from) = match &source {
        // Already in the local store, and pulling it would fail: there is no
        // registry behind it.
        Source::Local(_) => resolve_user_with(
            req.user.as_deref(),
            declared,
            &reference,
            endpoints,
            &mut notes,
            &|_| true,
            &crate::docker::uid_gid_in_image,
        ),
        _ => resolve_user(
            req.user.as_deref(),
            declared,
            &reference,
            endpoints,
            &mut notes,
        ),
    };

    Ok(Resolved {
        id,
        service_id,
        display_name,
        image,
        tag,
        commit,
        follow,
        data_dir: data_dir.0,
        data_dir_from: data_dir.1,
        user,
        user_from,
        reads_port: config
            .as_ref()
            .is_some_and(|c| crate::compose::resolve::reads_port_env(&c.command)),
        runtime_amd64: req.runtime_amd64 || config.map(|c| c.amd64_only).unwrap_or(false),
        notes,
        source,
    })
}

/// The name a manually added model is shown under unless `--name` says
/// otherwise: the repository name, or the last segment of the image path.
fn default_display_name(source: &Source) -> String {
    match source {
        Source::Repo(repo) => repo.repo.clone(),
        Source::Image(image) | Source::Local(image) => image
            .image
            .rsplit('/')
            .next()
            .unwrap_or(&image.image)
            .to_string(),
    }
}

/// GET `url`, returning the status and the body.
///
/// Modelled on [`crate::chapcore`]'s fetch - one global deadline covering
/// connect, send and receive - with one difference: a non-2xx status is a
/// value rather than an error, because a 404 from ghcr is the answer to "is
/// this tag published" and not a failure.
pub(crate) fn get(url: &str, timeout: Duration, headers: &[(&str, &str)]) -> Result<(u16, String)> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .user_agent(USER_AGENT)
            .build(),
    );
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let started = std::time::Instant::now();
    let mut response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(status)) => {
            crate::output::verbose(&format!(
                "GET {url} -> {status} in {}ms",
                started.elapsed().as_millis()
            ));
            return Ok((status, String::new()));
        }
        Err(err) => {
            crate::output::verbose(&format!(
                "GET {url} -> failed in {}ms",
                started.elapsed().as_millis()
            ));
            return Err(anyhow::anyhow!("fetching {url}: {err}"));
        }
    };
    let status = response.status().as_u16();
    crate::output::verbose(&format!(
        "GET {url} -> {status} in {}ms",
        started.elapsed().as_millis()
    ));
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| anyhow::anyhow!("reading the response body from {url}: {e}"))?;
    crate::output::debug(&format!("  {}", crate::output::trace_body(&body)));
    Ok((status, body))
}

/// `GET url` for its status and its `WWW-Authenticate` header, which is how
/// a registry says where its pull tokens come from.
pub(crate) fn challenge(url: &str, timeout: Duration) -> Result<(u16, Option<String>)> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .user_agent(USER_AGENT)
            .http_status_as_error(false)
            .build(),
    );
    let response = agent
        .get(url)
        .call()
        .map_err(|err| anyhow::anyhow!("fetching {url}: {err}"))?;
    let status = response.status().as_u16();
    crate::output::verbose(&format!("GET {url} -> {status}"));
    let challenge = response
        .headers()
        .get("www-authenticate")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    Ok((status, challenge))
}

#[cfg(test)]
mod tests;
