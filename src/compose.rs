//! Generating the compose files that make up a deployment.

pub mod apply;
pub mod overrides;
pub mod ports;
pub mod render;
pub mod resolve;
pub mod spec;
pub mod sync;

pub use apply::{ApplyReport, EnableRequest, PortRequest, Selection, apply};
pub use render::render_env;
pub use resolve::UserSource;
pub use sync::{SyncReport, sync};

/// Compose service name of the OCS component.
pub const OCS_SERVICE: &str = "ocs";
/// Compose service name of the object store component.
pub const S3_SERVICE: &str = "s3";
/// Compose service name of the DHIS2 component: the web service, which is the
/// one that publishes a host port. Its database and its two one-shots are
/// `dhis2-<something>` and belong to the same component by that name alone.
pub const DHIS2_SERVICE: &str = "dhis2";

/// Platform pinned for services built on the R-INLA runtime.
pub const AMD64_PLATFORM: &str = "linux/amd64";

/// Compose service name of chap-core itself, in upstream's `compose.ghcr.yml`
/// and therefore in every rendered `compose.yml`.
pub const API_SERVICE: &str = "chap";

/// Overlay file name for a model service: `compose.<service_id>.yml`.
pub fn overlay_filename(service_id: &str) -> String {
    format!("compose.{service_id}.yml")
}

/// Environment variable that overrides a model's image tag:
/// the uppercased marketplace id with non-alphanumerics folded to `_`,
/// suffixed with `_IMAGE_TAG`.
pub fn tag_env_var(id: &str) -> String {
    let mut out = String::with_capacity(id.len() + 10);
    for c in id.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push('_');
        }
    }
    out.push_str("_IMAGE_TAG");
    out
}

/// The separator between a tagless image and the pin that follows it.
///
/// A tag is joined with `:`; a digest already carries its own `@`, so it is
/// appended with nothing in between. Everything that builds an image
/// reference goes through here, because a `ghcr.io/x:@sha256:...` is not a
/// reference docker will pull.
pub fn tag_separator(tag: &str) -> &'static str {
    if tag.starts_with('@') { "" } else { ":" }
}

/// A fully qualified image reference: `image:tag`, or `image@sha256:...` when
/// the pin is a digest.
pub fn image_ref(image: &str, tag: &str) -> String {
    format!("{image}{}{tag}", tag_separator(tag))
}

/// The pin a model runs at: `tag@sha256:...` when the marketplace lists a
/// digest for the version, the tag alone otherwise.
///
/// Docker pulls `image:tag@digest` by the digest, so a tag that was moved on
/// the registry cannot change what runs. The tag stays in the reference for
/// the reader. A tag that is itself a digest (`@sha256:...`, a manual pin)
/// takes no second one.
pub fn pinned_tag(tag: &str, digest: Option<&str>) -> String {
    match digest {
        Some(digest) if !tag.starts_with('@') => format!("{tag}@{digest}"),
        _ => tag.to_string(),
    }
}

/// Whether `text` is a `sha256:<64 hex>` digest.
pub fn is_digest(text: &str) -> bool {
    match text.split_once(':') {
        Some(("sha256", hex)) => hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()),
        _ => false,
    }
}

/// Whether an image reference names no registry, so only the local image
/// store can have it: `my-model:dev`, `org/my-model:dev`. Docker reads the
/// first path segment as a registry host when it has a `.` or a `:` or is
/// `localhost`, and as Docker Hub otherwise; a model image is never on Docker
/// Hub, so a reference without a host is a local one.
pub fn is_local_image(reference: &str) -> bool {
    let name = reference.split('@').next().unwrap_or(reference);
    match name.split_once('/') {
        None => true,
        Some((first, _)) => !(first.contains('.') || first.contains(':') || first == "localhost"),
    }
}

/// Named volume holding a model's data directory: `ck_<id>_data`.
pub fn volume_name(id: &str) -> String {
    format!("ck_{id}_data")
}

/// The marketplace id inside a `ck_<id>_data` volume name, or `None` for a
/// name that is not one.
///
/// The inverse of [`volume_name`], and the only way back: a volume left
/// behind by a disabled model is a name and nothing else, so `chap-devops doctor`
/// reads the id out of it to ask whether that model is still enabled. The
/// name is the bare one, with the compose project prefix already stripped.
pub fn volume_model_id(volume: &str) -> Option<&str> {
    let id = volume.strip_prefix("ck_")?.strip_suffix("_data")?;
    (!id.is_empty()).then_some(id)
}

#[cfg(test)]
mod tests;
