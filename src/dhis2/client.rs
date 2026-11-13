//! The HTTP client: one DHIS2 instance, its requests, and what its errors say.

use super::{
    ADMIN_PASSWORD_ENV_VAR, ADMIN_USERNAME_ENV_VAR, API_TOKEN_ENV_VAR, AuthKind, Credentials,
    ME_PATH, PASSWORD_ENV_VAR, PING_PATH, SYSTEM_INFO_PATH, TOKEN_ENV_VAR, USER_AGENT,
};
use crate::api::Answer;
use crate::error::{ChapError, Result};
use std::time::{Duration, Instant};

/// One DHIS2 instance, ready to be asked.
#[derive(Debug, Clone)]
pub struct Dhis2 {
    base: String,
    pub(super) credentials: Credentials,
    timeout: Duration,
    /// Whether this is the deployment's own `dhis2` container, whose logs
    /// `chap-devops logs dhis2` shows, rather than an external instance.
    deployed: bool,
}

impl Dhis2 {
    /// A client for `base`, authenticating as `credentials`.
    pub fn new(base: &str, credentials: Credentials, timeout: Duration) -> Dhis2 {
        Dhis2 {
            base: base.trim_end_matches('/').to_string(),
            credentials,
            timeout,
            deployed: true,
        }
    }

    /// The same client, for a DHIS2 this deployment did not start.
    pub fn external(self) -> Dhis2 {
        Dhis2 {
            deployed: false,
            ..self
        }
    }

    /// The same instance and user, with a different request timeout.
    pub fn with_timeout(&self, timeout: Duration) -> Dhis2 {
        Dhis2 {
            timeout,
            ..self.clone()
        }
    }

    /// The base URL, without its trailing slash.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// The credential requests carry, for the line that says what it is.
    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    /// The full URL `path` is asked at.
    pub fn url(&self, path: &str) -> String {
        if path.starts_with('/') {
            format!("{}{path}", self.base)
        } else {
            format!("{}/{path}", self.base)
        }
    }

    /// `method path` with the credentials and an optional JSON body.
    pub fn send(&self, method: &str, path: &str, body: Option<&str>) -> Result<Answer> {
        self.request(method, path, body, true)
    }

    /// `GET path` without credentials, for [`PING_PATH`].
    ///
    /// The one route DHIS2 answers to anybody, and therefore the only honest
    /// probe for "is the API layer up": a 401 from it would mean something in
    /// front of DHIS2 is asking as well.
    pub fn send_open(&self, path: &str) -> Result<Answer> {
        self.request("GET", path, None, false)
    }

    /// `GET path` as JSON, where anything but a 2xx JSON body is an error.
    pub fn get_json(&self, path: &str) -> Result<serde_json::Value> {
        let answer = self.send("GET", path, None)?;
        if !answer.is_success() {
            return Err(self.status_error(path, &answer));
        }
        answer.json().ok_or_else(|| {
            anyhow::anyhow!(
                "{} answered {} with {}, which is not JSON",
                self.url(path),
                answer.status_line(),
                answer.body_description()
            )
        })
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        authenticated: bool,
    ) -> Result<Answer> {
        let url = self.url(path);
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(self.timeout))
                .user_agent(USER_AGENT)
                // A status code is an answer: DHIS2 says what it did not like
                // in the body of the 409 or the 401, and dropping it would be
                // dropping the message.
                .http_status_as_error(false)
                // Followed, with the credentials kept only for the same host
                // on the same or a more secure scheme. See `crate::api`.
                .redirect_auth_headers(ureq::config::RedirectAuthHeaders::SameHost)
                .build(),
        );

        let mut request = ureq::http::Request::builder().method(method).uri(&url);
        crate::output::verbose(&format!("{method} {url}"));
        if authenticated {
            request = request.header("Authorization", self.credentials.header());
            // Never the value, and never the password or the token anywhere
            // near a trace line: they end up in bug reports.
            crate::output::verbose(&format!("  Authorization: {}", self.credentials.traced()));
        }
        // DHIS2 answers HTML to a request that does not ask for JSON.
        request = request.header("Accept", crate::api::JSON);
        if body.is_some() {
            request = request.header("Content-Type", crate::api::JSON);
        }
        if let Some(body) = body {
            crate::output::debug(&format!("  {}", crate::output::trace_body(body)));
        }

        let request = request
            .body(body.unwrap_or_default().as_bytes().to_vec())
            .map_err(|e| ChapError::Usage(format!("{method} {url} is not a valid request: {e}")))?;

        let started = Instant::now();
        let mut response = agent.run(request).map_err(|e| {
            crate::output::verbose(&format!(
                "{method} {url} -> failed in {}ms",
                started.elapsed().as_millis()
            ));
            ChapError::Dhis2Unreachable {
                url: self.base.clone(),
                reason: e.to_string(),
                next: match self.deployed {
                    true => "run `chap-devops status` to see whether its container is up",
                    false => {
                        "check the URL with `chap-devops dhis2 use`, and that this machine can reach it"
                    }
                },
            }
        })?;
        let status = response.status();
        crate::output::verbose(&format!(
            "{method} {url} -> {} in {}ms",
            status.as_u16(),
            started.elapsed().as_millis()
        ));
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = response
            .body_mut()
            .with_config()
            .limit(crate::api::MAX_BODY)
            .read_to_vec()
            .map_err(|e| anyhow::anyhow!("reading the response body from {url}: {e}"))?;
        crate::output::debug(&format!(
            "  {}",
            crate::output::trace_body(&String::from_utf8_lossy(&body))
        ));
        Ok(Answer {
            status: status.as_u16(),
            reason: status.canonical_reason().unwrap_or_default().to_string(),
            content_type,
            wire_bytes: body.len(),
            transport: Default::default(),
            body,
        })
    }

    /// The error for a non-2xx answer to a request that had to work, with the
    /// way out for the two statuses that have one.
    pub fn status_error(&self, path: &str, answer: &Answer) -> anyhow::Error {
        if let Some(text) = self.refusal(answer) {
            return anyhow::anyhow!(text);
        }
        let said = said(answer);
        let where_ = self.url(path);
        if said.is_empty() {
            anyhow::anyhow!("{where_} answered {}", answer.status_line())
        } else {
            anyhow::anyhow!("{where_} answered {}: {said}", answer.status_line())
        }
    }

    /// The sentence a 401 or a 403 deserves, naming the file to change.
    ///
    /// The two are different problems: a 401 is the wrong password or a token
    /// DHIS2 no longer honours, a 403 is the right credential on a user DHIS2
    /// will not let do this. Neither ever prints the secret.
    pub fn refusal(&self, answer: &Answer) -> Option<String> {
        let who = self.who();
        match (answer.status, self.credentials.kind()) {
            (401, AuthKind::Basic) => Some(format!(
                "DHIS2 at {} did not accept the password for {who} ({}); set \
                 `{ADMIN_USERNAME_ENV_VAR}` and `{ADMIN_PASSWORD_ENV_VAR}` in `.env`, or export \
                 `{PASSWORD_ENV_VAR}`",
                self.base,
                self.credentials.describe()
            )),
            (401, AuthKind::Token) => Some(format!(
                "DHIS2 at {} did not accept the API token ({}): it is expired, revoked, or not \
                 allowed from this address; set a current one as `{API_TOKEN_ENV_VAR}` in \
                 `.env`, or export `{TOKEN_ENV_VAR}`",
                self.base,
                self.credentials.describe()
            )),
            // A token that is not well formed is not a 401: DHIS2 checks the
            // checksum in it first and answers 400 `Checksum validation
            // failed`. The way out is the same as for a revoked token.
            (400, AuthKind::Token) if is_malformed_token(&said(answer)) => Some(format!(
                "DHIS2 at {} did not accept the API token ({}): {}; set a current one as \
                 `{API_TOKEN_ENV_VAR}` in `.env`, or export `{TOKEN_ENV_VAR}`",
                self.base,
                self.credentials.describe(),
                said(answer)
            )),
            (403, _) => Some(format!(
                "DHIS2 refused the request as {who} (HTTP 403{}): that user is authenticated but \
                 not allowed to do this, and a DHIS2 superuser is; name another with `--user NAME`",
                match said(answer) {
                    reason if reason.is_empty() => String::new(),
                    reason => format!(", \"{reason}\""),
                }
            )),
            _ => None,
        }
    }

    /// Wait until [`PING_PATH`] answers, then report what DHIS2 says it is.
    ///
    /// Two questions rather than one, because the container being up says
    /// neither: Tomcat serves pages while every `/api/*` request 404s, which is
    /// what [`PING_PATH`] settles, and a password that is wrong only shows on a
    /// request that carries one, which is what [`SYSTEM_INFO_PATH`] settles.
    ///
    /// `waiting` is called once, with the deadline, the first time the API has
    /// not answered: a command that is about to sit here for twenty minutes has
    /// to say so, and one that finds DHIS2 ready says nothing at all.
    pub fn wait_for_api(
        &self,
        wait: Duration,
        interval: Duration,
        waiting: &mut impl FnMut(Duration),
    ) -> Result<Ready> {
        let started = Instant::now();
        let until = started + wait;
        let mut said = false;
        loop {
            match self.send_open(PING_PATH) {
                Ok(answer) if answer.is_success() => break,
                Ok(answer) => crate::output::verbose(&format!(
                    "{} answered {}",
                    self.url(PING_PATH),
                    answer.status_line()
                )),
                Err(err) => crate::output::verbose(&format!("{err}")),
            }
            if !said {
                said = true;
                waiting(wait);
            }
            if Instant::now() + interval >= until {
                return Err(anyhow::anyhow!(
                    "DHIS2 at {} did not answer {PING_PATH} within {}; {}, and `--wait SECONDS` \
                     waits longer",
                    self.base,
                    crate::output::human_age(wait),
                    match self.deployed {
                        true => "`chap-devops logs dhis2` is where the migration shows",
                        false => "check that the URL recorded by `chap-devops dhis2 use` is right",
                    }
                ));
            }
            std::thread::sleep(interval);
        }
        let answer = self.send("GET", SYSTEM_INFO_PATH, None)?;
        if !answer.is_success() {
            return Err(self.status_error(SYSTEM_INFO_PATH, &answer));
        }
        let info = answer.json().unwrap_or(serde_json::Value::Null);
        Ok(Ready {
            version: text_at(&info, "version"),
            last_analytics: super::recorded_success_of(&info, crate::backup::now()),
            user: self.whoami(),
            waited: started.elapsed(),
        })
    }

    /// Who the requests are sent as: the username for a password, and for a
    /// token whatever [`ME_PATH`] says, since the token itself does not tell.
    ///
    /// A token whose user cannot be read is not a failure - the request that
    /// proved the token works has already been answered - so it is named for
    /// what it is instead.
    fn whoami(&self) -> String {
        if let Some(user) = self.credentials.user() {
            return user.to_string();
        }
        match self.get_json(ME_PATH) {
            Ok(me) if !text_at(&me, "username").is_empty() => text_at(&me, "username"),
            Ok(_) => "the token's user".to_string(),
            Err(why) => {
                crate::output::verbose(&format!(
                    "{ME_PATH} could not be read ({why}), so the token's user is not named"
                ));
                "the token's user".to_string()
            }
        }
    }
}

/// What an instance said about itself once it was ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    /// DHIS2's own version string, `2.42.6` and the like. Empty when the
    /// answer carried none.
    pub version: String,
    /// When analytics last succeeded, as DHIS2 records it, in this machine's
    /// zone where the server offset is known. Empty for an instance that has
    /// never run it. See [`super::recorded_success_of`].
    pub last_analytics: String,
    /// Who the requests are sent as. See [`Dhis2::whoami`].
    pub user: String,
    /// How long the wait took, which is the fact a slow first start needs.
    pub waited: Duration,
}

/// A string field of a JSON object, or `""`.
pub(super) fn text_at(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// What DHIS2 said went wrong: its `WebMessage`, else the body.
///
/// Every DHIS2 error carries `message`, and the interesting ones carry
/// `devMessage` or a `response.message` under it as well. Anything that is not
/// JSON is handed back trimmed and cut, because a Tomcat error page in an error
/// line helps nobody.
pub fn said(answer: &Answer) -> String {
    if let Some(value) = answer.json() {
        for key in ["message", "devMessage", "description"] {
            if let Some(text) = value.get(key).and_then(serde_json::Value::as_str)
                && !text.trim().is_empty()
            {
                return text.trim().to_string();
            }
        }
    }
    let text = answer.text();
    let text = text.trim();
    // A body that carries nothing is nothing to say: "answered HTTP 404 Not
    // Found: {}" is the status line plus noise.
    if matches!(text, "{}" | "[]" | "null") {
        return String::new();
    }
    if text.chars().count() > 200 {
        let cut: String = text.chars().take(200).collect();
        format!("{cut}...")
    } else {
        text.to_string()
    }
}

/// Whether a 400 from DHIS2 is about the API token itself rather than about
/// the request: DHIS2 says `Checksum validation failed` for a token with a
/// wrong checksum, which is what a mistyped token has.
fn is_malformed_token(said: &str) -> bool {
    let said = said.to_lowercase();
    said.contains("checksum") || said.contains("token")
}
