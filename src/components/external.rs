//! A DHIS2 or a chap-core this deployment did not start, as `components.yaml` records them.

use super::*;

/// A DHIS2 this deployment did not start, which `chap-devops dhis2` talks to in
/// place of the `dhis2` component. Recorded by `chap-devops dhis2 use`.
///
/// The shape of most real deployments: Chap beside a DHIS2 that already runs
/// somewhere else. Nothing is rendered from it - there is no container - and
/// it cannot be recorded while the `dhis2` component is on, because the two
/// would each be "this deployment's DHIS2".
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalDhis2 {
    /// Where DHIS2 answers, as this machine reaches it: the origin, plus the
    /// context path of an instance that is served under one.
    pub url: String,
    /// Where chap-core answers as DHIS2 reaches it, which is what the `chap`
    /// route points at. Not the compose network's `http://chap:8000`, which
    /// only a container of this deployment can resolve.
    pub chap_url: String,
    /// When `chap-devops dhis2 connect` last got as far as a verified route, which
    /// is all it sets here. The same record, with the same caveats, as
    /// [`Dhis2Component::connected_at`].
    #[serde(default)]
    pub connected_at: Option<String>,
    /// The same setting as [`Dhis2Component::modeling_app`], for this DHIS2.
    #[serde(default, skip_serializing_if = "ModelingApp::is_stable")]
    pub modeling_app: ModelingApp,
    /// The same record as [`Dhis2Component::modeling_app_build`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modeling_app_build: Option<ModelingAppBuild>,
}

/// A chap-core this deployment did not start, which its model services
/// register with and the chap-core commands talk to. Recorded by
/// `chap-devops init --chap-core-url` or `chap-devops components enable chap-core --url`.
///
/// The shape of chap-core development: chap-core runs from its own checkout
/// on this machine, and chap-devops runs the model services around it. It cannot be
/// recorded while the `chap-core` component is on, because the two would each
/// be "this deployment's chap-core".
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalChapCore {
    /// Where chap-core answers, as this machine reaches it.
    pub url: String,
    /// The host a model service registers itself under, which is what
    /// chap-core then calls it at, on the model's published host port.
    /// `localhost` for a chap-core running on this machine.
    #[serde(default = "default_models_host")]
    pub models_host: String,
}

fn default_models_host() -> String {
    "localhost".to_string()
}

impl ExternalChapCore {
    /// The URL a container of this deployment reaches chap-core at: a
    /// loopback host names the container itself in there, so it becomes the
    /// host gateway every overlay maps.
    pub fn url_from_containers(&self) -> String {
        for loopback in ["localhost", "127.0.0.1", "[::1]"] {
            for scheme in ["http://", "https://"] {
                let prefix = format!("{scheme}{loopback}");
                if let Some(rest) = self.url.strip_prefix(&prefix)
                    && (rest.is_empty() || rest.starts_with(':') || rest.starts_with('/'))
                {
                    return format!("{scheme}{HOST_GATEWAY}{rest}");
                }
            }
        }
        self.url.clone()
    }
}

impl ExternalChapCore {
    /// The port of a chap-core on this machine: `Some` only for a loopback
    /// URL, which is the one case where a container of this machine can be
    /// what answers it.
    pub fn loopback_port(&self) -> Option<u16> {
        let (scheme, rest) = self.url.split_once("://")?;
        let authority = rest.split('/').next()?;
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !port.contains(']') => (host, port.parse().ok()?),
            _ => (authority, if scheme == "https" { 443 } else { 80 }),
        };
        ["localhost", "127.0.0.1", "[::1]"]
            .contains(&host)
            .then_some(port)
    }
}

/// Switch `external` to calling the models back at the host gateway when the
/// chap-core it names is a container on this machine, and say so.
///
/// Models register as `<models_host>:<port>` and chap-core calls them there.
/// `localhost` is right for a chap-core running as a process here; for one in
/// a container (`make restart` in chap-core's checkout starts it that way) it
/// is the container itself, and every call back fails. `publisher` names the
/// container publishing a host port, `None` when there is none or no docker
/// to ask - which leaves `localhost`, and `chap-devops status` to catch it later.
pub fn detect_models_host(
    external: &mut ExternalChapCore,
    publisher: &dyn Fn(u16) -> Option<String>,
) -> Option<String> {
    if external.models_host != default_models_host() {
        return None;
    }
    let port = external.loopback_port()?;
    let container = publisher(port)?;
    external.models_host = HOST_GATEWAY.to_string();
    Some(format!(
        "chap-core at {} is the container `{container}`, so it calls the models back at \
         {HOST_GATEWAY}; if it is a process on this machine instead, run `chap-devops components \
         enable chap-core --url {} --models-host localhost`",
        external.url, external.url
    ))
}

/// An [`ExternalChapCore`] for `url`, refused unless it is an absolute
/// `http(s)` URL: the models append a path to it to register.
pub fn external_chap_core(url: &str) -> Result<ExternalChapCore> {
    let url = url.trim().trim_end_matches('/');
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(anyhow::anyhow!(
            "the chap-core URL has to be absolute, such as `http://localhost:8000`, \
             so `{url}` will not do: the models register at <URL>/v2/services/$register"
        ));
    }
    Ok(ExternalChapCore {
        url: url.to_string(),
        models_host: default_models_host(),
    })
}
