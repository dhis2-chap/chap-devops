//! The unstable Modeling App: the latest build of chap-frontend's `main`.
//!
//! chap-frontend publishes it as the GitHub pre-release `latest`, with one
//! asset of a fixed name, and moves that release on every push to `main`.
//! The App Hub does not have it, so DHIS2 cannot fetch it itself: chap-devops
//! downloads the zip, checks it against the digest GitHub lists, and uploads it
//! to `POST /api/apps`.

use super::USER_AGENT;
use crate::components::ModelingAppBuild;
use crate::error::Result;
use serde::Deserialize;
use std::time::Duration;

/// The repository that builds the Modeling App.
pub const FRONTEND_REPO: &str = "dhis2-chap/chap-frontend";
/// The moving tag of the unstable pre-release.
pub const UNSTABLE_TAG: &str = "latest";
/// The one asset of that pre-release.
pub const UNSTABLE_ASSET: &str = "modeling-app-latest.zip";

/// A zip larger than this is not a Modeling App build. The builds are about
/// 11 MB.
const MAX_ZIP_BYTES: u64 = 100 * 1024 * 1024;

/// One unstable build, as the GitHub release describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnstableBuild {
    /// What the state records about it.
    pub build: ModelingAppBuild,
    /// Where the zip is downloaded from.
    pub url: String,
}

#[derive(Deserialize)]
struct Release {
    #[serde(default)]
    target_commitish: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    updated_at: String,
}

/// `GET /repos/dhis2-chap/chap-frontend/releases/tags/latest`.
pub fn unstable_release_url() -> String {
    format!(
        "{}/repos/{FRONTEND_REPO}/releases/tags/{UNSTABLE_TAG}",
        crate::github::api_base()
    )
}

/// The unstable build GitHub lists now.
pub fn fetch_unstable(timeout: Duration) -> Result<UnstableBuild> {
    let url = unstable_release_url();
    let answer = crate::github::get(&url, timeout)?;
    if answer.status == 404 {
        return Err(anyhow::anyhow!(
            "{FRONTEND_REPO} has no `{UNSTABLE_TAG}` release, so there is no unstable Modeling \
             App to install; use `--modeling-app stable`"
        ));
    }
    if !answer.ok() {
        return Err(answer.error(&url));
    }
    parse_unstable_release(&answer.body)
}

/// The build in a `releases/tags/latest` body.
///
/// The commit comes from the release notes, `Unstable build of <sha>`:
/// chap-frontend moves the tag on every push, but `target_commitish` keeps
/// the commit the release was first created for. The digest identifies the
/// build; a release without one is identified by the time of its upload.
pub fn parse_unstable_release(body: &str) -> Result<UnstableBuild> {
    let release: Release = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("the `{UNSTABLE_TAG}` release of {FRONTEND_REPO}: {e}"))?;
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == UNSTABLE_ASSET)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "the `{UNSTABLE_TAG}` release of {FRONTEND_REPO} has no {UNSTABLE_ASSET}; use \
                 `--modeling-app stable`"
            )
        })?;
    let commit = release
        .body
        .as_deref()
        .and_then(commit_in_notes)
        .unwrap_or(release.target_commitish);
    let digest = asset
        .digest
        .filter(|digest| digest.starts_with("sha256:"))
        .unwrap_or_else(|| format!("uploaded:{}", asset.updated_at));
    Ok(UnstableBuild {
        build: ModelingAppBuild { commit, digest },
        url: asset.browser_download_url,
    })
}

/// The commit in `Unstable build of <sha>. ...`.
fn commit_in_notes(notes: &str) -> Option<String> {
    let rest = notes.split("Unstable build of ").nth(1)?;
    let sha: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
    (sha.len() >= 7).then_some(sha)
}

/// Download the zip, and refuse it when its digest is not the one GitHub lists.
pub fn download_unstable(build: &UnstableBuild, timeout: Duration) -> Result<Vec<u8>> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .user_agent(USER_AGENT)
            .build(),
    );
    crate::output::verbose(&format!("GET {}", build.url));
    let bytes = agent
        .get(&build.url)
        .call()
        .map_err(|e| anyhow::anyhow!("downloading {}: {e}", build.url))?
        .body_mut()
        .with_config()
        .limit(MAX_ZIP_BYTES)
        .read_to_vec()
        .map_err(|e| anyhow::anyhow!("downloading {}: {e}", build.url))?;
    check_digest(&bytes, &build.build.digest)?;
    Ok(bytes)
}

/// Whether `bytes` have the `sha256:` digest. A build identified by its
/// upload time has no digest to check.
pub fn check_digest(bytes: &[u8], digest: &str) -> Result<()> {
    let Some(want) = digest.strip_prefix("sha256:") else {
        return Ok(());
    };
    let got = crate::chapcore::sha256_hex(bytes);
    if !got.eq_ignore_ascii_case(want) {
        return Err(anyhow::anyhow!(
            "the downloaded {UNSTABLE_ASSET} has the digest sha256:{got}, not the {digest} that \
             GitHub lists; run the command again"
        ));
    }
    Ok(())
}

/// A `multipart/form-data` body with the zip as its one `file` part, the way
/// DHIS2's `POST /api/apps` takes an app, and the content type that names its
/// boundary.
///
/// The boundary comes from the zip's own digest, so it does not change between
/// runs and a test can predict it.
pub fn app_upload(zip: &[u8]) -> (Vec<u8>, String) {
    let hash = crate::chapcore::sha256_hex(zip);
    let boundary = format!("chap-devops-{}", &hash[..32]);
    let mut body = Vec::with_capacity(zip.len() + 256);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"{UNSTABLE_ASSET}\"\r\nContent-Type: application/zip\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(zip);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (body, format!("multipart/form-data; boundary={boundary}"))
}
