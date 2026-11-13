//! A stand-in for GitHub, its raw file host, ghcr and the marketplace index.

use super::*;
use std::net::Ipv4Addr;
use std::path::PathBuf;

/// The newest commit on the example repository's default branch.
pub(crate) const NEW_SHA: &str = "b1d6c31f4b2f0d8a0f4c6e2a5e9d3c7b8a1f0e2d";

/// The commit before it, and the only one this hub publishes by default.
pub(crate) const OLD_SHA: &str = "1eb8cf1a2b3c4d5e6f708192a3b4c5d6e7f80910";

/// The tags those two commits are built as.
pub(crate) const NEW_TAG: &str = "sha-b1d6c31";

pub(crate) const OLD_TAG: &str = "sha-1eb8cf1";

/// The commit the served marketplace entry pins, older than either of them.
pub(crate) const MARKETPLACE_SHA: &str = "fa880a1";

/// The day every commit this hub knows about was made. The `registry pin`
/// checks read these, and place a pin the branch listing does not carry by
/// asking for its commit by name.
pub(crate) const COMMIT_DAYS: &[(&str, &str)] = &[
    (NEW_SHA, "2026-09-23T08:15:00Z"),
    (OLD_SHA, "2026-09-08T11:00:00Z"),
    (MARKETPLACE_SHA, "2026-09-01T09:00:00Z"),
];

/// The committer date this hub gives one commit, matched on the short SHA so
/// a full one and the seven characters a pin carries find the same day.
pub(crate) fn commit_date(sha: &str) -> Option<&'static str> {
    let short = |sha: &str| sha.chars().take(7).collect::<String>();
    COMMIT_DAYS
        .iter()
        .find(|(known, _)| short(known) == short(sha))
        .map(|(_, date)| *date)
}

/// Every request the hub answered: the path, and the `Authorization` header
/// it carried, where it carried one.
pub(crate) type Seen = std::sync::Arc<std::sync::Mutex<Vec<(String, Option<String>)>>>;

/// The Unix second the hub's `/rate_limit` says the window resets at, so the
/// clock time on the checklist is the same on every run.
pub(crate) const RATE_RESET: u64 = 1_758_805_440;

/// A local stand-in for GitHub, ghcr and the marketplace, routed by path.
///
/// One listener answers every request one `chap-devops models add` makes: the
/// repository's default branch, its commits, a ghcr pull token, the OCI index
/// of a tag, the amd64 manifest inside it, the config blob that manifest
/// points at, and the registry index `--registry-url` names. Nothing in these
/// tests touches the real network.
#[derive(Clone)]
pub(crate) struct Hub {
    /// Commits on the default branch, newest first.
    pub(crate) commits: Vec<String>,
    /// The `sha-` tags the registry has an image for.
    pub(crate) published: Vec<String>,
    /// What the image's config says it runs as.
    pub(crate) user: String,
    /// Its `WorkingDir`, which is what the data directory is derived from.
    pub(crate) working_dir: String,
    /// chap-core's releases, newest first, as `(tag, published_at)`. They
    /// answer `releases/latest` (the first one, as GitHub can mark a Helm chart
    /// release as the latest), `releases`, `releases/tags/<tag>` and the
    /// `compose.ghcr.yml` every one of them publishes.
    pub(crate) releases: Vec<(String, String)>,
    /// The commit the served marketplace entry pins, which is what the
    /// `registry pin` checks compare against the branch.
    pub(crate) pinned: String,
    /// The `image_digest` the served marketplace entry lists, if any.
    pub(crate) digest: Option<String>,
    /// Whether ghcr refuses an anonymous pull token, which is what it answers
    /// for a repository that publishes no public image.
    pub(crate) denied: bool,
    /// chap-core tags ghcr has no `chap-core` and `chap-worker` image for.
    /// Every release and moving tag has both unless it is named here.
    pub(crate) unbuilt: Vec<String>,
    /// chap-core tags other than releases and moving tags that ghcr has both
    /// images for.
    pub(crate) built: Vec<String>,
    /// Whether the model's index lists a `linux/arm64` image next to the
    /// amd64 one.
    pub(crate) arm64: bool,
    /// The `runtime_image` of the served marketplace entry.
    pub(crate) runtime: String,
}

/// The chap-core releases the hub publishes by default: two of them, so a
/// switch can go forwards and backwards between releases as well as to a
/// moving tag.
pub(crate) const CHAP_RELEASES: &[(&str, &str)] = &[
    ("v2.3.1", "2026-09-21T10:00:25Z"),
    ("v2.3.0", "2026-09-11T09:20:41Z"),
];

/// The day the hub's `dev` branch was last committed to.
pub(crate) const DEV_COMMITTED: &str = "2026-09-24T08:15:00Z";

/// chap-core's `compose.ghcr.yml` as the hub publishes it at one ref.
///
/// The ref is written into the document, so a test can tell which one
/// `compose.yml` was rendered from; everything else is the little the CLI
/// requires of it, the `${CHAP_IMAGE_TAG}` pin included.
pub(crate) fn chap_compose(reference: &str) -> String {
    format!(
        "services:\n  \
         chap:\n    \
         image: ghcr.io/dhis2-chap/chap-core:${{CHAP_IMAGE_TAG:-latest}}\n    \
         environment:\n      \
         CHAP_DEVOPS_TEST_REF: {reference}\n  \
         worker:\n    \
         image: ghcr.io/dhis2-chap/chap-core:${{CHAP_IMAGE_TAG:-latest}}\n"
    )
}

/// The digest of the amd64 manifest inside the index.
pub(crate) const AMD64_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

/// The digest of the config blob that manifest points at.
pub(crate) const CONFIG_DIGEST: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";

impl Hub {
    /// The example repository as it stands: two commits on `main`, only the
    /// older of them published, and an image that runs as 10001:10001 out of
    /// `/work`.
    ///
    /// Publishing only the older commit is the case that matters: the pin has
    /// to land on the newest build there *is*, not on the newest commit.
    pub(crate) fn new() -> Hub {
        Hub {
            commits: vec![NEW_SHA.to_string(), OLD_SHA.to_string()],
            published: vec![OLD_TAG.to_string()],
            user: "10001:10001".to_string(),
            working_dir: "/work".to_string(),
            releases: CHAP_RELEASES
                .iter()
                .map(|(tag, at)| (tag.to_string(), at.to_string()))
                .collect(),
            pinned: MARKETPLACE_SHA.to_string(),
            digest: None,
            denied: false,
            unbuilt: Vec::new(),
            built: Vec::new(),
            arm64: false,
            runtime: R_INLA_RUNTIME.to_string(),
        }
    }

    /// The same, with the served marketplace entry built on `runtime`.
    pub(crate) fn on_runtime(self, runtime: &str) -> Hub {
        Hub {
            runtime: runtime.to_string(),
            ..self
        }
    }

    /// The same, with the model's image published for linux/arm64 too.
    pub(crate) fn multi_arch(self) -> Hub {
        Hub {
            arm64: true,
            ..self
        }
    }

    /// The same hub once the newer commit has been published too, which is
    /// what `chap-devops update` is meant to notice.
    pub(crate) fn advanced(self) -> Hub {
        Hub {
            published: vec![NEW_TAG.to_string(), OLD_TAG.to_string()],
            ..self
        }
    }

    /// The same, for an image that runs as an account name nothing here can
    /// turn into numbers.
    pub(crate) fn running_as(self, user: &str) -> Hub {
        Hub {
            user: user.to_string(),
            ..self
        }
    }

    /// The same hub, also publishing the tag the served marketplace entry
    /// pins - which is what `models enable` reads the user from.
    pub(crate) fn publishing(self, tag: &str) -> Hub {
        let mut published = self.published.clone();
        published.push(tag.to_string());
        Hub { published, ..self }
    }

    /// The same, with the marketplace entry pinning `commit` rather than the
    /// one it carries by default.
    pub(crate) fn pinning(self, commit: &str) -> Hub {
        Hub {
            pinned: commit.to_string(),
            ..self
        }
    }

    /// The same, with the marketplace entry listing `digest` as the
    /// `image_digest` of its version.
    pub(crate) fn listing_digest(self, digest: &str) -> Hub {
        Hub {
            digest: Some(digest.to_string()),
            ..self
        }
    }

    /// The marketplace entry as this hub serves it: the template, repinned.
    pub(crate) fn marketplace_model(&self) -> String {
        let short: String = self.pinned.chars().take(7).collect();
        let digest = match &self.digest {
            Some(digest) => format!("\n    image_digest: {digest}"),
            None => String::new(),
        };
        MARKETPLACE_MODEL
            .replace(R_INLA_RUNTIME, &self.runtime)
            .replace(
                &format!("commit: {MARKETPLACE_SHA}"),
                &format!("commit: {}", self.pinned),
            )
            .replace(
                &format!("image_tag: sha-{MARKETPLACE_SHA}"),
                &format!("image_tag: sha-{short}{digest}"),
            )
    }

    /// `WorkingDir` of the image, which the data directory follows.
    pub(crate) fn working_in(self, working_dir: &str) -> Hub {
        Hub {
            working_dir: working_dir.to_string(),
            ..self
        }
    }

    /// Serve this hub on a port of its own; the thread lives as long as the
    /// test process.
    pub(crate) fn start(self) -> u16 {
        self.start_seen().0
    }

    /// The same, with the log of what was asked for and what credential came
    /// with it, which is the only way a test can see a request header at all.
    pub(crate) fn start_seen(self) -> (u16, Seen) {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a free port");
        let port = listener.local_addr().expect("a local address").port();
        let seen: Seen = Seen::default();
        let log = Seen::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                // The request has to be read before the answer, or the client
                // sees a reset instead of the response.
                let mut buffer = [0u8; 2048];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();
                let authorization = request
                    .lines()
                    .skip(1)
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                    .map(|(_, value)| value.trim().to_string());
                log.lock()
                    .expect("the request log outlives its panics")
                    .push((path.clone(), authorization.clone()));
                let (status, content_type, body) = self.respond(&path, authorization.as_deref());
                let reason = if status == 200 { "OK" } else { "Not Found" };
                // A registry other than ghcr says where its tokens come from
                // in the challenge of a 401 on `/v2/`, which needs the port.
                let (status, reason, challenge) = match path.as_str() {
                    "/v2/" => (
                        401,
                        "Unauthorized",
                        format!(
                            "WWW-Authenticate: Bearer realm=\"http://127.0.0.1:{port}/token\",\
                             service=\"stand-in\"\r\n"
                        ),
                    ),
                    _ => (status, reason, String::new()),
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\n\
                     {challenge}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (port, seen)
    }

    /// The answer one request path gets: `(status, content type, body)`.
    ///
    /// `authorization` is what the request carried, which only `/rate_limit`
    /// reads: GitHub reports one of two hourly limits depending on whether it
    /// was asked by a token or by an address, and the checklist prints which.
    pub(crate) fn respond(
        &self,
        path: &str,
        authorization: Option<&str>,
    ) -> (u16, &'static str, String) {
        let json = "application/json";
        if path.starts_with("/rate_limit") {
            let (limit, remaining) = match authorization {
                Some(_) => (5000, 4990),
                None => (60, 43),
            };
            return (
                200,
                json,
                format!(
                    r#"{{"resources":{{"core":{{"limit":{limit},"remaining":{remaining},"reset":{RATE_RESET},"used":0}}}},"rate":{{"limit":{limit},"remaining":{remaining},"reset":{RATE_RESET}}}}}"#
                ),
            );
        }
        if path.starts_with("/registry.yaml") {
            return (200, "text/plain", REGISTRY_INDEX.to_string());
        }
        // chap-core's own releases and the compose file each ref publishes.
        // Before the model repository's routes, which are the wider match.
        if let Some(answer) = self.chap_core(path) {
            return answer;
        }
        if path.starts_with("/models/chapkit_ewars_model.yaml") {
            return (200, "text/plain", self.marketplace_model());
        }
        if path.starts_with("/token") {
            if self.denied {
                return (
                    403,
                    json,
                    r#"{"errors":[{"code":"DENIED","message":"denied"}]}"#.to_string(),
                );
            }
            return (200, json, r#"{"token":"anonymous"}"#.to_string());
        }
        // One commit by name, which is how a pin the branch listing does not
        // carry is placed. Before the listing, which is the wider match.
        if let Some((_, sha)) = path.split_once("/commits/") {
            let sha = sha.split('?').next().unwrap_or(sha);
            return match commit_date(sha) {
                Some(date) => (200, json, dated_commit(sha, date)),
                None => (404, json, r#"{"message":"Not Found"}"#.to_string()),
            };
        }
        // A repository named `does-not-exist` is what GitHub answers for
        // one that is missing or private: a 404.
        if path.starts_with("/repos/") && path.contains("/does-not-exist") {
            return (404, json, r#"{"message":"Not Found"}"#.to_string());
        }
        if path.starts_with("/repos/") && path.contains("/commits") {
            let entries: Vec<String> = self
                .commits
                .iter()
                .map(|sha| dated_commit(sha, commit_date(sha).unwrap_or_default()))
                .collect();
            return (200, json, format!("[{}]", entries.join(",")));
        }
        if path.starts_with("/repos/") {
            return (
                200,
                json,
                r#"{"name":"chapkit_example_manual_model","default_branch":"main"}"#.to_string(),
            );
        }
        if let Some((_, reference)) = path.split_once("/manifests/") {
            if reference == AMD64_DIGEST {
                return (200, json, manifest());
            }
            // A digest pin asks for the index by digest; a tag pin asks for
            // it by tag, and an unpublished tag is a 404.
            if reference.starts_with("sha256:") || self.published.iter().any(|t| t == reference) {
                return (200, json, model_index(self.arm64));
            }
            return (
                404,
                json,
                r#"{"errors":[{"code":"MANIFEST_UNKNOWN"}]}"#.to_string(),
            );
        }
        if path.contains("/blobs/") {
            return (
                200,
                json,
                format!(
                    r#"{{"architecture":"amd64","os":"linux","config":{{"User":"{}","WorkingDir":"{}"}}}}"#,
                    self.user, self.working_dir
                ),
            );
        }
        (404, json, "{}".to_string())
    }

    /// The chap-core half of the hub: the release feed and the raw
    /// `compose.ghcr.yml` of every ref that publishes one.
    ///
    /// `None` when the path is not one of chap-core's, which is what lets the
    /// model repository keep the routes it had.
    pub(crate) fn chap_core(&self, path: &str) -> Option<(u16, &'static str, String)> {
        const REPO: &str = "dhis2-chap/chap-core";
        let json = "application/json";
        let release = |(tag, at): &(String, String)| {
            format!(
                r#"{{"tag_name":"{tag}","published_at":"{at}","draft":false,"prerelease":false}}"#
            )
        };
        let releases = format!("/repos/{REPO}/releases");

        // ghcr: the API and the worker image of every release and moving tag.
        for image in ["dhis2-chap/chap-core", "dhis2-chap/chap-worker"] {
            if let Some(tag) = path.strip_prefix(&format!("/v2/{image}/manifests/")) {
                let built = !self.unbuilt.iter().any(|t| t == tag)
                    && (["dev", "master", "latest"].contains(&tag)
                        || self.built.iter().any(|t| t == tag)
                        || self.releases.iter().any(|(name, _)| name == tag));
                return Some(match built {
                    true => (200, json, index()),
                    false => (
                        404,
                        json,
                        r#"{"errors":[{"code":"MANIFEST_UNKNOWN"}]}"#.to_string(),
                    ),
                });
            }
        }

        if path.starts_with(&format!("{releases}/latest")) {
            return Some(match self.releases.first() {
                Some(newest) => (200, json, release(newest)),
                None => (404, json, r#"{"message":"Not Found"}"#.to_string()),
            });
        }
        if let Some(tag) = path.strip_prefix(&format!("{releases}/tags/")) {
            let tag = tag.split('?').next().unwrap_or(tag);
            return Some(match self.releases.iter().find(|(name, _)| name == tag) {
                Some(found) => (200, json, release(found)),
                None => (404, json, r#"{"message":"Not Found"}"#.to_string()),
            });
        }
        if path.starts_with(&releases) {
            let entries: Vec<String> = self.releases.iter().map(release).collect();
            return Some((200, json, format!("[{}]", entries.join(","))));
        }
        if path.starts_with(&format!("/repos/{REPO}/commits")) {
            return Some((
                200,
                json,
                format!(
                    r#"[{{"sha":"{NEW_SHA}","commit":{{"committer":{{"date":"{DEV_COMMITTED}"}}}}}}]"#
                ),
            ));
        }
        // The raw host: `/<repo>/<ref>/compose.ghcr.yml`. Every release and
        // the two branches publish one; anything else is a 404, which is what
        // a tag that was never built looks like.
        let reference = path
            .strip_prefix(&format!("/{REPO}/"))?
            .strip_suffix("/compose.ghcr.yml")?;
        let known = reference == "dev"
            || reference == "master"
            || self.releases.iter().any(|(tag, _)| tag == reference);
        Some(match known {
            true => (200, "text/plain", chap_compose(reference)),
            false => (404, "text/plain", "404: Not Found".to_string()),
        })
    }
}

/// One entry of a commit listing, as GitHub writes it.
pub(crate) fn dated_commit(sha: &str, date: &str) -> String {
    format!(r#"{{"sha":"{sha}","commit":{{"message":"x","committer":{{"date":"{date}"}}}}}}"#)
}

/// An OCI index with the attestation entry ghcr adds to every one of them.
pub(crate) fn index() -> String {
    model_index(false)
}

/// [`index`], with a linux/arm64 entry too when `arm64` is set.
pub(crate) fn model_index(arm64: bool) -> String {
    let arm64 = match arm64 {
        true => {
            r#"{"digest":"sha256:4444444444444444444444444444444444444444444444444444444444444444",
            "platform":{"architecture":"arm64","os":"linux"}},"#
        }
        false => "",
    };
    format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[
          {{"digest":"{AMD64_DIGEST}","platform":{{"architecture":"amd64","os":"linux"}}}},
          {arm64}
          {{"digest":"sha256:2222222222222222222222222222222222222222222222222222222222222222",
            "platform":{{"architecture":"unknown","os":"unknown"}}}}]}}"#
    )
}

/// The amd64 manifest inside that index.
pub(crate) fn manifest() -> String {
    format!(r#"{{"schemaVersion":2,"config":{{"digest":"{CONFIG_DIGEST}","size":1}},"layers":[]}}"#)
}

/// A one-model marketplace, so a collision with the catalogue can be told
/// from a collision with something the deployment added itself.
pub(crate) const REGISTRY_INDEX: &str = "\
schema_version: 2
marketplace:
  name: test marketplace
  description: served by the CLI tests
  repository: https://example.test/marketplace
review_policy:
  required_approvals: 3
models:
  - models/chapkit_ewars_model.yaml
";

/// The runtime the served marketplace entry is built on by default.
pub(crate) const R_INLA_RUNTIME: &str = "ghcr.io/dhis2-chap/chapkit-r-inla";

pub(crate) const MARKETPLACE_MODEL: &str = "\
schema_version: 2
id: chapkit_ewars_model
service_id: chapkit-ewars-model
display_name: CHAP-EWARS
kind: model
assessed_status: orange
summary: stands in for the marketplace entry
source:
  repository: https://github.com/chap-models/chapkit_ewars_model
  image: ghcr.io/chap-models/chapkit_ewars_model
  runtime_image: ghcr.io/dhis2-chap/chapkit-r-inla
attribution:
  author: nobody
compatibility:
  period_types:
    - month
  min_prediction_periods: 1
  max_prediction_periods: 6
covariates: {}
channels:
  stable: 1.0.0
  latest: 1.0.0
versions:
  - version: 1.0.0
    commit: fa880a1
    image_tag: sha-fa880a1
    chapkit: '>=2,<3'
    status: verified
configurations: {}
";

/// A deployment with nothing enabled, plus a hub to add against.
pub(crate) fn added_sandbox(hub: Hub) -> (Sandbox, PathBuf, u16) {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--api-port", &free_port().to_string()])
        .assert()
        .success();
    (sandbox, dir, hub.start())
}

/// The tag the served marketplace entry pins, which is the image
/// `models enable` asks about.
pub(crate) const MARKETPLACE_TAG: &str = "sha-fa880a1";
