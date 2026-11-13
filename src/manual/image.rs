//! What an image says about how it runs: its config, from the registry or
//! the local daemon, and the user the overlay runs it as.

use super::source::Source;
use super::{Endpoints, Origin, ProbeFn, PullFn, ghcr};
use crate::compose::overrides;
use crate::error::Result;

/// What the image says about itself: over HTTP where the registry can be
/// reached, from the local daemon where it cannot.
///
/// `None` is "nobody could say", which leaves the chapkit defaults in place
/// and puts a note in the report rather than failing: the operator can still
/// pass `--data-dir` and `--user`.
pub(super) fn image_config(
    source: &Source,
    reference: &str,
    endpoints: &Endpoints,
    notes: &mut Vec<String>,
) -> Result<Option<ghcr::ImageConfig>> {
    // A local image has no registry to ask: its config is in the local store
    // or nowhere, and "nowhere" means it was never built.
    if let Source::Local(_) = source {
        return match crate::docker::image_config(reference) {
            Some((user, working_dir)) => Ok(Some(ghcr::ImageConfig {
                user,
                working_dir,
                amd64_only: false,
                arm64: false,
                command: crate::docker::image_command(reference).unwrap_or_default(),
            })),
            None => Err(anyhow::anyhow!(
                "{reference} is not in the local image store for linux/amd64; build it with \
                 `docker build --platform linux/amd64 -t {reference} .` in the model's \
                 checkout, then add it again"
            )),
        };
    }
    if !endpoints.offline {
        match read_config(&source.image(), reference, endpoints) {
            Ok(Some(config)) => return Ok(Some(config)),
            Ok(None) => notes.push(format!(
                "{reference} is not published at {}, so the data directory and user \
                 come from the local image or the chapkit defaults",
                super::location::Location::of(&source.image(), endpoints).base
            )),
            Err(err) => notes.push(format!(
                "could not read the image config from the registry ({err:#}); \
                 falling back to the local image"
            )),
        }
    }

    match crate::docker::image_config(reference) {
        Some((user, working_dir)) => Ok(Some(ghcr::ImageConfig {
            user,
            working_dir,
            amd64_only: false,
            arm64: false,
            command: crate::docker::image_command(reference).unwrap_or_default(),
        })),
        None if endpoints.offline => Err(anyhow::anyhow!(
            "the amd64 variant of {reference} is not in the local image store and this \
             run is --offline; run `docker pull --platform linux/amd64 {reference}` \
             first, or drop --offline"
        )),
        None => {
            notes.push(format!(
                "nothing could say what {reference} runs as; \
                 pass `--data-dir` and `--user` if the defaults are wrong"
            ));
            Ok(None)
        }
    }
}

/// The registry half of [`image_config`], split out so its errors are notes
/// rather than failures.
fn read_config(
    image: &str,
    reference: &str,
    endpoints: &Endpoints,
) -> Result<Option<ghcr::ImageConfig>> {
    // The manifest is addressed by the pin alone: the client already knows
    // the repository.
    let pin = reference
        .rsplit_once('@')
        .map(|(_, digest)| digest.to_string())
        .or_else(|| {
            let last_slash = reference.rfind('/').map(|i| i + 1).unwrap_or(0);
            reference[last_slash..]
                .split_once(':')
                .map(|(_, tag)| tag.to_string())
        })
        .unwrap_or_else(|| "latest".to_string());
    super::location::image_config(image, &pin, endpoints)
}

/// `/work` -> `/work/data`, which is where a chapkit service keeps its SQLite
/// file when it leaves the default `DATABASE_URL` alone.
pub(crate) fn data_dir_under(working_dir: &str) -> String {
    let base = working_dir.trim().trim_end_matches('/');
    if base.is_empty() {
        return overrides::DEFAULT_DATA_DIR.to_string();
    }
    format!("{base}/data")
}

/// The `user:` the overlay runs the service as, and where that came from.
///
/// The flag wins. After that comes whatever the image declares, which is only
/// usable if it can be turned into numbers: the overlay's init container
/// chowns the data volume from busybox, which resolves no account name of its
/// own. A name busybox and this CLI both know nothing about is what the
/// docker probe is for - `id -u` inside the image itself - and if that cannot
/// run either, the name is kept and the caller is told to pass `--user`.
pub fn resolve_user(
    flag: Option<&str>,
    declared: &str,
    reference: &str,
    endpoints: &Endpoints,
    notes: &mut Vec<String>,
) -> (String, Origin) {
    resolve_user_with(
        flag,
        declared,
        reference,
        endpoints,
        notes,
        &crate::docker::pull_image,
        &crate::docker::uid_gid_in_image,
    )
}

/// [`resolve_user`] with the docker calls injected, so the table of cases can
/// be tested without a daemon.
pub fn resolve_user_with(
    flag: Option<&str>,
    declared: &str,
    reference: &str,
    endpoints: &Endpoints,
    notes: &mut Vec<String>,
    pull: PullFn,
    probe: ProbeFn,
) -> (String, Origin) {
    if let Some(user) = flag.map(str::trim).filter(|u| !u.is_empty()) {
        return (user.to_string(), Origin::Flag);
    }
    let declared = declared.trim();
    if declared.is_empty() {
        return (overrides::DEFAULT_USER.to_string(), Origin::Default);
    }
    // Numeric already, or one of the account names the chapkit images create:
    // either way the init container can chown to it.
    if overrides::numeric_user(declared).is_some() {
        return (declared.to_string(), Origin::Image);
    }
    if endpoints.docker_probe {
        crate::output::notice(&format!(
            "asking {reference} what uid `{declared}` is (this pulls the image)"
        ));
        if pull(reference)
            && let Some((uid, gid)) = probe(reference, declared)
        {
            return (format!("{uid}:{gid}"), Origin::Probe);
        }
    }
    notes.push(format!(
        "`{declared}` has no uid this CLI knows and the image could not be asked, \
         so the volume init container will chown to {} - pass `--user <uid>:<gid>` if that is wrong",
        overrides::FALLBACK_UID_GID
    ));
    (declared.to_string(), Origin::Image)
}
