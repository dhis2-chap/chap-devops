//! What `chap-devops models add` accepts, and what it derives from it.
//!
//! Two forms, and only two: a GitHub repository URL, which follows the
//! default branch, and an image reference on ghcr, which pins exactly. A bare
//! image name (`chapkit_ghr_model`) is refused rather than guessed at, and so
//! is a registry this CLI cannot read anonymously.

use crate::error::Result;

/// The container registry the Chap models are published to, and the only one
/// `models add` reads.
pub const GHCR_HOST: &str = "ghcr.io";
/// Host of the repository URLs `models add` accepts.
pub const GITHUB_HOST: &str = "github.com";

/// A GitHub repository: the owner and the repository name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub owner: String,
    pub repo: String,
}

impl Repo {
    /// `<owner>/<repo>`, the path both the API and ghcr use.
    pub fn path(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    /// The repository's web URL, as recorded in `models-manual.yaml`.
    pub fn url(&self) -> String {
        format!("https://{GITHUB_HOST}/{}", self.path())
    }

    /// Where the repository's publish workflow pushes its images.
    ///
    /// Every model repository under `chap-models` publishes to
    /// `ghcr.io/<owner>/<repo>`, which is what the marketplace entries point
    /// at too. Image names are lowercase, whatever the repository is called.
    pub fn image(&self) -> String {
        format!("{GHCR_HOST}/{}", self.path().to_lowercase())
    }
}

/// An image reference split into the part that never moves and the pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Tagless reference, lowercase: `ghcr.io/chap-models/chapkit_ghr_model`.
    pub image: String,
    /// `sha-b1d6c31`, or `@sha256:<64 hex>` for a digest.
    pub tag: String,
}

impl Image {
    /// The reference as docker would be handed it.
    pub fn reference(&self) -> String {
        crate::compose::image_ref(&self.image, &self.tag)
    }
}

/// What the caller named on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A repository: the newest commit of its default branch that has a
    /// published `sha-` build.
    Repo(Repo),
    /// An image reference, pinned exactly as given.
    Image(Image),
    /// An image in the local docker image store only, such as one just
    /// built with `docker build -t my-model:dev .`: no registry to ask and
    /// nothing to pull.
    Local(Image),
}

impl Source {
    /// Parse a repository URL or an image reference.
    pub fn parse(text: &str) -> Result<Source> {
        let text = text.trim();
        if text.is_empty() {
            return Err(anyhow::anyhow!(
                "name a GitHub repository URL, an image reference such as `quay.io/org/model:v1`, or a local image such as `my-model:dev`"
            ));
        }
        if is_github(text) {
            return parse_repo(text).map(Source::Repo);
        }
        if crate::compose::is_local_image(text) {
            return parse_local(text).map(Source::Local);
        }
        parse_image(text).map(Source::Image)
    }

    /// The tagless image this source publishes to.
    pub fn image(&self) -> String {
        match self {
            Source::Repo(repo) => repo.image(),
            Source::Image(image) | Source::Local(image) => image.image.clone(),
        }
    }

    /// The repository URL, for a source that names one.
    pub fn repository(&self) -> Option<String> {
        match self {
            Source::Repo(repo) => Some(repo.url()),
            Source::Image(_) | Source::Local(_) => None,
        }
    }

    /// The id this source is recorded under unless `--id` says otherwise.
    ///
    /// The repository name (or the last path segment of the image), folded to
    /// lowercase with everything outside `[a-z0-9]` written as `_` - the shape
    /// every marketplace id already has.
    pub fn default_id(&self) -> String {
        let last = match self {
            Source::Repo(repo) => repo.repo.clone(),
            Source::Image(image) | Source::Local(image) => image
                .image
                .rsplit('/')
                .next()
                .unwrap_or(&image.image)
                .to_string(),
        };
        snake(&last)
    }

    /// How the entry's summary names where it came from.
    pub fn describe(&self) -> String {
        match self {
            Source::Repo(repo) => repo.url(),
            Source::Image(image) | Source::Local(image) => image.reference(),
        }
    }
}

/// Whether this looks like a GitHub repository rather than an image.
fn is_github(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.starts_with("git@github.com:")
        || lower.starts_with(GITHUB_HOST)
        || lower.starts_with("www.github.com")
        || lower.contains("://github.com/")
        || lower.contains("://www.github.com/")
}

/// `https://github.com/<owner>/<repo>` and the spellings around it.
///
/// A URL copied out of a browser carries more than the two segments
/// (`/tree/main`, `/blob/...`); everything after the repository is dropped,
/// because the repository is the whole of what is being named.
fn parse_repo(text: &str) -> Result<Repo> {
    let rest = text
        .strip_prefix("git@github.com:")
        .map(str::to_string)
        .unwrap_or_else(|| {
            let without_scheme = text
                .split_once("://")
                .map(|(_, rest)| rest)
                .unwrap_or(text)
                .trim_start_matches("www.");
            without_scheme
                .strip_prefix(GITHUB_HOST)
                .unwrap_or(without_scheme)
                .trim_start_matches('/')
                .to_string()
        });
    let rest = rest.trim_end_matches('/');
    let mut parts = rest.split('/').filter(|p| !p.is_empty());
    let owner = parts.next().unwrap_or_default().to_string();
    let repo = parts
        .next()
        .unwrap_or_default()
        .trim_end_matches(".git")
        .to_string();
    if owner.is_empty() || repo.is_empty() {
        return Err(anyhow::anyhow!(
            "`{text}` names no repository; the form is https://github.com/<owner>/<repo>"
        ));
    }
    Ok(Repo { owner, repo })
}

/// `<registry host>/<repository>:<tag>` or the same with `@sha256:<digest>`,
/// such as `ghcr.io/<owner>/<repo>:<tag>` or `quay.io/<org>/<repo>:<tag>`.
/// Only a reference with a registry host gets here; one without is local.
fn parse_image(text: &str) -> Result<Image> {
    if let Some((image, digest)) = text.split_once('@') {
        let digest = digest.trim();
        if !crate::compose::is_digest(digest) {
            return Err(anyhow::anyhow!(
                "`{digest}` is not a digest; the form is @sha256:<64 hex characters>"
            ));
        }
        return Ok(Image {
            image: image.trim_end_matches(':').to_lowercase(),
            tag: format!("@{digest}"),
        });
    }

    // A colon inside the host's port (`host:5000/x`) is not a tag, so only a
    // colon after the last `/` counts.
    let last_slash = text.rfind('/').map(|i| i + 1).unwrap_or(0);
    match text[last_slash..].find(':') {
        Some(i) => {
            let (image, tag) = text.split_at(last_slash + i);
            let tag = &tag[1..];
            if tag.is_empty() {
                return Err(anyhow::anyhow!("`{text}` ends in an empty tag"));
            }
            Ok(Image {
                image: image.to_lowercase(),
                tag: tag.to_string(),
            })
        }
        None => Err(anyhow::anyhow!(
            "`{text}` names no tag, so there is nothing to pin; \
             add `:<tag>` or `@sha256:<digest>`"
        )),
    }
}

/// `my-model:dev`, `org/my-model:dev` or the same `@sha256:...`: an image
/// with no registry host, which only the local image store can have.
fn parse_local(text: &str) -> Result<Image> {
    if let Some((image, digest)) = text.split_once('@') {
        if !crate::compose::is_digest(digest.trim()) {
            return Err(anyhow::anyhow!(
                "`{digest}` is not a digest; the form is @sha256:<64 hex characters>"
            ));
        }
        return Ok(Image {
            image: image.to_lowercase(),
            tag: format!("@{}", digest.trim()),
        });
    }
    match text.rsplit_once(':') {
        Some((image, tag)) if !tag.is_empty() && !tag.contains('/') => Ok(Image {
            image: image.to_lowercase(),
            tag: tag.to_string(),
        }),
        _ => Err(anyhow::anyhow!(
            "`{text}` names no tag, so there is nothing to pin; build it with a tag \
             (`docker build -t {text}:dev .`) and add `{text}:dev`"
        )),
    }
}

/// `GHRmodel-chapkit` -> `ghrmodel_chapkit`: the shape of a marketplace id.
pub fn snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// The compose service name an id gets unless `--service-id` says otherwise:
/// the id with its underscores written as hyphens, as the marketplace does.
pub fn default_service_id(id: &str) -> String {
    id.replace('_', "-")
}

/// Whether `id` is usable as a key in `.chap-devops/models-manual.yaml`.
///
/// The same alphabet the marketplace uses, because the id names a volume
/// (`ck_<id>_data`) and an environment variable (`<ID>_IMAGE_TAG`).
pub fn check_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(anyhow::anyhow!("the id is empty"));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(anyhow::anyhow!(
            "`{id}` is not a usable id; use lowercase letters, digits and underscores"
        ));
    }
    if BASELINE_IDS.contains(&id) {
        return Err(anyhow::anyhow!(
            "`{id}` is the name of a model built into chap-core, which refuses a service of \
             that name; pass `--id <another>`"
        ));
    }
    Ok(())
}

/// The models built into chap-core v2.5.0 and later. chap-core refuses to
/// store a template from a service with one of these ids.
pub const BASELINE_IDS: &[&str] = &["persistence", "global_median", "seasonal_median"];

/// Names a model's service id must not take: the services chap-core and the
/// components run as, which a model of that name would be merged into, and
/// the stems of the compose files chap-devops writes besides the overlays
/// (`compose.chap-devops.yml`, `compose.marketplace.yml`), which an overlay of that
/// name would be written over. A test holds this to the templates.
pub const RESERVED_SERVICE_IDS: &[&str] = &[
    "chap",
    "worker",
    "redis",
    "postgres",
    "ocs",
    "s3",
    "s3-init",
    "dhis2",
    "dhis2-db",
    "dhis2-prep",
    "chap-devops",
    "marketplace",
];

/// The error for a service id chap-devops keeps for itself, or `Ok`.
pub fn check_not_reserved(service_id: &str) -> Result<()> {
    if RESERVED_SERVICE_IDS.contains(&service_id) {
        return Err(anyhow::anyhow!(
            "`{service_id}` is a name chap-devops uses for its own services and files; pass \
             `--service-id <another>`, or edit `service_id:` in `.chap-devops/models-manual.yaml`"
        ));
    }
    Ok(())
}

/// Whether `service_id` is usable as a compose service and DNS name.
pub fn check_service_id(service_id: &str) -> Result<()> {
    if service_id.is_empty() {
        return Err(anyhow::anyhow!("the service id is empty"));
    }
    check_not_reserved(service_id)?;
    let first = service_id.chars().next().unwrap_or('-');
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(anyhow::anyhow!(
            "`{service_id}` is not a usable service id; it must start with a letter or a digit"
        ));
    }
    if !service_id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(anyhow::anyhow!(
            "`{service_id}` is not a usable service id; use lowercase letters, digits and hyphens"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
