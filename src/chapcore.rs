//! chap-core itself: which release its images are tagged with, and the compose
//! file every tag publishes.
//!
//! [`crate::registry`] is the equivalent for the *model* marketplace. Here the
//! questions are narrower: the GitHub release API answers "what is the newest
//! version", and `compose.ghcr.yml` at a tag is the upstream source of truth
//! for the base stack, so a deployment can be pinned to a release and still
//! carry exactly the compose file that release shipped.

use crate::error::{ChapError, Result};
use serde::Deserialize;
use std::time::Duration;

/// GitHub repository the chap-core images and the compose file come from.
pub const REPO: &str = "dhis2-chap/chap-core";

/// Public raw.githubusercontent.com, unless `CHAP_DEVOPS_GITHUB_RAW` points
/// somewhere else. Both are test hooks and nothing on the command line moves
/// them; see `docs/development.md`.
pub const DEFAULT_RAW: &str = "https://raw.githubusercontent.com";

/// The standalone compose file published with every chap-core tag.
pub const COMPOSE_FILE: &str = "compose.ghcr.yml";

/// The moving image tag `chap-devops init` resolves to the release it points at.
pub const LATEST_TAG: &str = "latest";

/// The moving tags chap-core publishes, newest build first. `latest` follows
/// the newest release; the other two follow their branches.
pub const MOVING_TAGS: &[&str] = &["dev", "master", LATEST_TAG];

/// How many releases `chap-devops update --list-tags` lists.
pub const LIST_LIMIT: usize = 10;

/// How many releases are read to find the newest version.
const LATEST_WINDOW: usize = 30;

/// `User-Agent` sent with every request, matching the registry fetch.
const USER_AGENT: &str = concat!("chap-devops/", env!("CARGO_PKG_VERSION"));

/// Base of the GitHub REST API this run talks to.
pub fn api_base() -> String {
    crate::github::api_base()
}

/// Base of the raw file host this run talks to.
pub fn raw_base() -> String {
    crate::github::base("CHAP_DEVOPS_GITHUB_RAW", DEFAULT_RAW)
}

/// The newest `limit` releases, newest first. Unauthenticated requests are
/// allowed, at 60 an hour per address; [`crate::github`] sends a token where
/// this run has one.
pub fn releases_url(limit: usize) -> String {
    format!("{}/repos/{REPO}/releases?per_page={limit}", api_base())
}

/// One release by its tag: 200 when that release exists, 404 when it does not.
pub fn release_url(tag: &str) -> String {
    format!("{}/repos/{REPO}/releases/tags/{tag}", api_base())
}

/// The newest commit of one branch, which is what a moving tag is built from.
pub fn branch_url(branch: &str) -> String {
    format!(
        "{}/repos/{REPO}/commits?sha={branch}&per_page=1",
        api_base()
    )
}

/// Raw URL of [`COMPOSE_FILE`] at `tag`.
pub fn compose_url(tag: &str) -> String {
    format!("{}/{REPO}/{tag}/{COMPOSE_FILE}", raw_base())
}

/// The tag of the newest chap-core release, e.g. `v2.3.1`.
///
/// Read from the release list and not from `releases/latest`: the repository
/// also publishes its Helm chart as releases named `chap-X.Y.Z`, GitHub can
/// mark one of those as the latest, and no image has that tag.
pub fn latest_release(timeout: Duration) -> Result<String> {
    let url = releases_url(LATEST_WINDOW);
    let body = crate::github::get_ok(&url, timeout)?;
    parse_latest(&body).map_err(|e| anyhow::anyhow!("reading the release list at {url}: {e}"))
}

/// One chap-core release: the tag it names and the day it was published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The tag, e.g. `v2.3.1`.
    pub tag: String,
    /// `YYYY-MM-DD`, or empty when the payload carried no date.
    pub published: String,
}

/// The newest chap-core releases, newest first.
///
/// Only the tags that are versions ([`release_version`]) come back: the
/// repository also tags other things, and a tag that is not a version is not
/// somewhere `chap-devops update --chap-tag` can move a deployment to as a release.
pub fn releases(limit: usize, timeout: Duration) -> Result<Vec<Release>> {
    // Asked for wider than the list that is printed: the drafts, the
    // prereleases and the tags that are not versions are dropped here, not by
    // GitHub, so a page of exactly `limit` entries could come back short.
    let url = releases_url((limit * 3).min(100));
    let body = crate::github::get_ok(&url, timeout)?;
    let mut list = parse_releases(&body)
        .map_err(|e| anyhow::anyhow!("reading the release list at {url}: {e}"))?;
    list.truncate(limit);
    Ok(list)
}

/// Whether chap-core has released `tag`.
///
/// `Ok(false)` is GitHub saying there is no such release; an error is this
/// run not having been able to ask, which is a different answer and the
/// caller treats it as one.
pub fn release_exists(tag: &str, timeout: Duration) -> Result<bool> {
    let url = release_url(tag);
    match crate::github::get_ok(&url, timeout) {
        Ok(_) => Ok(true),
        Err(err) => match err.downcast_ref::<ChapError>() {
            Some(ChapError::Http { status: 404, .. }) => Ok(false),
            _ => Err(err),
        },
    }
}

/// The day `branch` was last committed to, `YYYY-MM-DD`.
///
/// Best effort and never fatal: it decorates one column of
/// `chap-devops update --list-tags`, and a branch GitHub will not talk about simply
/// has no date to print.
pub fn branch_updated(branch: &str, timeout: Duration) -> Option<String> {
    let body = crate::github::get_ok(&branch_url(branch), timeout).ok()?;
    parse_commit_date(&body)
}

/// `[0].commit.committer.date` of a commit list, as a day.
pub fn parse_commit_date(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let commit = value.as_array()?.first()?.get("commit")?;
    for who in ["committer", "author"] {
        if let Some(date) = commit
            .get(who)
            .and_then(|w| w.get("date"))
            .and_then(|d| d.as_str())
            && let Some(day) = day_of(date)
        {
            return Some(day);
        }
    }
    None
}

/// The `YYYY-MM-DD` of an RFC 3339 timestamp, or `None` when it is not one.
pub(crate) fn day_of(timestamp: &str) -> Option<String> {
    let day = timestamp.trim().split(['T', 't', ' ']).next()?;
    let mut fields = day.split('-');
    let ok = fields
        .next()
        .is_some_and(|y| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()))
        && fields.next().is_some_and(|m| m.len() == 2)
        && fields.next().is_some_and(|d| d.len() == 2)
        && fields.next().is_none();
    ok.then(|| day.to_string())
}

/// Download chap-core's `compose.ghcr.yml` at `tag`.
///
/// The text is validated before it is returned ([`validate_compose`]), so a
/// caller that gets an `Ok` has something it can write as `compose.yml`.
pub fn fetch_compose(tag: &str, timeout: Duration) -> Result<String> {
    let url = compose_url(tag);
    let body = get(&url, timeout)?;
    validate_compose(&body).map_err(|e| anyhow::anyhow!("{url}: {e}"))?;
    Ok(body)
}

/// The highest version in a release-list payload.
pub fn parse_latest(body: &str) -> Result<String> {
    parse_releases(body)?
        .into_iter()
        .max_by_key(|release| release_version(&release.tag))
        .map(|release| release.tag)
        .ok_or_else(|| anyhow::anyhow!("the release list has no version tag"))
}

/// The releases of a release-list payload, newest first.
///
/// Drafts and prereleases are left out - they are not what the `latest` image
/// tag follows - and so is every tag that is not a version, because a
/// deployment moved to one of those would be pinned to something `update`
/// could never compare against anything.
pub fn parse_releases(body: &str) -> Result<Vec<Release>> {
    #[derive(Debug, Default, Deserialize)]
    struct Entry {
        #[serde(default)]
        tag_name: String,
        #[serde(default)]
        published_at: String,
        #[serde(default)]
        draft: bool,
        #[serde(default)]
        prerelease: bool,
    }
    let entries: Vec<Entry> =
        serde_json::from_str(body).map_err(|e| anyhow::anyhow!("invalid release JSON: {e}"))?;
    Ok(entries
        .into_iter()
        .filter(|entry| !entry.draft && !entry.prerelease)
        .filter(|entry| release_version(entry.tag_name.trim()).is_some())
        .map(|entry| Release {
            tag: entry.tag_name.trim().to_string(),
            published: day_of(&entry.published_at).unwrap_or_default(),
        })
        .collect())
}

/// What a chap-core tag is, as far as moving a deployment's pin goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagKind {
    /// `vX.Y.Z`: a point on the version line, and what `update` moves forward.
    Release,
    /// `latest`, `master`, `dev`: the same name, a different image tomorrow.
    Moving,
    /// A `sha-` build or anything else: an exact pin nothing here moves.
    Other,
}

impl TagKind {
    /// How the kind reads in a table cell.
    pub fn label(self) -> &'static str {
        match self {
            TagKind::Release => "release",
            TagKind::Moving => "moving",
            TagKind::Other => "exact",
        }
    }
}

/// Which of the three `tag` is.
pub fn tag_kind(tag: &str) -> TagKind {
    if is_moving_tag(tag) {
        return TagKind::Moving;
    }
    if release_version(tag).is_some() {
        return TagKind::Release;
    }
    TagKind::Other
}

/// Whether moving the pin from `from` to `to` goes backwards, which is the
/// direction that can run an older schema against a newer database.
///
/// Two cases, and only two: an older release than the one that is pinned, and
/// a release after a moving tag, because `dev` and `master` are ahead of
/// every release and `latest` is the newest of them. Everything else - a
/// newer release, a release to a moving tag, one moving tag to another, and
/// anything involving a `sha-` build that is not on the version line at all -
/// is not something this can call backwards, so it does not.
pub fn is_backwards(from: &str, to: &str) -> bool {
    match (tag_kind(from), tag_kind(to)) {
        (TagKind::Release, TagKind::Release) => is_newer(from, to),
        (TagKind::Moving, TagKind::Release) => true,
        _ => false,
    }
}

/// Whether `text` is usable as the base `compose.yml`.
///
/// Three things matter: it has to parse as YAML, it has to define the `chap`
/// service, and that service's image has to keep reading `CHAP_IMAGE_TAG`, or
/// the tag this CLI records in `.chap-devops/project.yaml` and `.env` would pin
/// nothing at all.
pub fn validate_compose(text: &str) -> Result<()> {
    let doc: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(text).map_err(|e| anyhow::anyhow!("not valid YAML: {e}"))?;
    let chap = doc
        .get("services")
        .and_then(|services| services.get("chap"))
        .ok_or_else(|| anyhow::anyhow!("no `services.chap` in the document"))?;
    let image = chap
        .get("image")
        .and_then(|image| image.as_str())
        .ok_or_else(|| anyhow::anyhow!("`services.chap` has no image"))?;
    if !image.contains("${CHAP_IMAGE_TAG") {
        return Err(anyhow::anyhow!(
            "the chap image is `{image}`, which does not read CHAP_IMAGE_TAG"
        ));
    }
    Ok(())
}

/// A tag that can point at a different image tomorrow.
pub fn is_moving_tag(tag: &str) -> bool {
    matches!(
        tag,
        "latest" | "master" | "main" | "dev" | "edge" | "nightly"
    )
}

/// `(major, minor, patch)` of a release tag such as `v2.3.1`.
///
/// A leading `v` is optional and anything from the first `-` or `+` on (a
/// prerelease or build suffix) is ignored; anything that is not three numeric
/// components is not a release tag at all, which is how a moving tag or a
/// `sha-` build is told apart from a version.
pub fn release_version(tag: &str) -> Option<(u64, u64, u64)> {
    let core = tag.strip_prefix('v').unwrap_or(tag);
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let version = (next()?, next()?, next()?);
    match parts.next() {
        None => Some(version),
        Some(_) => None,
    }
}

/// Whether `candidate` is a newer release than `current`.
///
/// Both have to be release tags: a moving tag is never "older" than a release,
/// because it is not a point on the same line.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (release_version(candidate), release_version(current)) {
        (Some(new), Some(old)) => new > old,
        _ => false,
    }
}

/// SHA-256 of `bytes`, as lowercase hex.
///
/// Written out here rather than added as a dependency: it is one function, it
/// is only ever used to notice that a cached file changed since it was
/// fetched, and the six release targets (one of them a static musl build) keep
/// the dependency set they have.
pub fn sha256_hex(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut hash: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // The padded message: the bytes, a 1 bit, zeroes up to 56 mod 64, then the
    // original length in bits as a big-endian u64.
    let mut message = Vec::with_capacity(bytes.len() + 72);
    message.extend_from_slice(bytes);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(bytes.len() as u64 * 8).to_be_bytes());

    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (word, quad) in w.iter_mut().zip(block.chunks(4)) {
            *word = u32::from_be_bytes([quad[0], quad[1], quad[2], quad[3]]);
        }
        for i in 16..64 {
            let a = w[i - 15];
            let b = w[i - 2];
            let s0 = a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3);
            let s1 = b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut v = hash;
        for (k, word) in K.iter().zip(w.iter()) {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(*k)
                .wrapping_add(*word);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (slot, value) in hash.iter_mut().zip(v.iter()) {
            *slot = slot.wrapping_add(*value);
        }
    }

    hash.iter().map(|word| format!("{word:08x}")).collect()
}

/// GET `url` as text, with the same timeout semantics as the registry fetch:
/// one global deadline covering connect, send and receive.
///
/// The raw file host only: every REST call goes through [`crate::github`], so
/// that all of them carry the same headers and the same token. Raw files need
/// neither - they count against no limit - and a credential sent to a file
/// host is a credential sent somewhere it was not needed.
fn get(url: &str, timeout: Duration) -> Result<String> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .user_agent(USER_AGENT)
            .build(),
    );
    let request = agent.get(url);
    let started = std::time::Instant::now();
    let mut response = request.call().map_err(|e| {
        crate::output::verbose(&format!(
            "GET {url} -> failed in {}ms",
            started.elapsed().as_millis()
        ));
        map_error(url, e)
    })?;
    crate::output::verbose(&format!(
        "GET {url} -> {} in {}ms",
        response.status().as_u16(),
        started.elapsed().as_millis()
    ));
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| anyhow::anyhow!("reading the response body from {url}: {e}"))?;
    crate::output::debug(&format!("  {}", crate::output::trace_body(&body)));
    Ok(body)
}

/// [`get`] for the rest of the crate: one raw file, such as a model's
/// `MLproject`, with no token sent.
pub(crate) fn get_raw(url: &str, timeout: Duration) -> Result<String> {
    get(url, timeout)
}

/// A non-2xx response is a [`ChapError::Http`] (the rate limit and a missing
/// tag both arrive that way); everything else is a transport failure that
/// keeps the URL as context.
fn map_error(url: &str, err: ureq::Error) -> anyhow::Error {
    match err {
        ureq::Error::StatusCode(status) => ChapError::Http {
            url: url.to_string(),
            status,
        }
        .into(),
        other => anyhow::anyhow!("fetching {url}: {other}"),
    }
}

#[cfg(test)]
mod tests;
