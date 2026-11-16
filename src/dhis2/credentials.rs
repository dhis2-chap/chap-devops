//! Which credential a request to DHIS2 carries, and where it was found.

use super::{
    ADMIN_PASSWORD_ENV_VAR, ADMIN_USERNAME_ENV_VAR, API_TOKEN_ENV_VAR, DEFAULT_PASSWORD,
    DEFAULT_USERNAME, PASSWORD_ENV_VAR, TOKEN_ENV_VAR, USERNAME_ENV_VAR, base64,
};
use crate::error::Result;
use serde::Serialize;
use std::path::Path;

/// Where the credential came from, which is all a report may say about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialSource {
    /// An active line in this deployment's `.env`: [`API_TOKEN_ENV_VAR`], or
    /// [`ADMIN_PASSWORD_ENV_VAR`] for the user `.env` names.
    EnvFile,
    /// [`TOKEN_ENV_VAR`] or [`PASSWORD_ENV_VAR`] in the environment this
    /// command ran in.
    Environment,
    /// `DHIS2_SEED_PASSWORD` in `.env`: the restore of the seed
    /// gave every user that password, on a DHIS2 chap-devops deployed.
    SeedPassword,
    /// None of these, so [`DEFAULT_PASSWORD`] - which is what a seeded dump
    /// and an empty database both give, on a DHIS2 chap-devops deployed and nowhere
    /// else.
    Default,
}

/// Which `Authorization` scheme a request goes out with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthKind {
    /// `Basic`, a username and its password.
    Basic,
    /// `ApiToken`, a DHIS2 personal access token.
    Token,
}

/// The phrase a report names a credential by: what it is and where it was
/// found, never what it is worth.
pub fn describe_credential(kind: AuthKind, source: CredentialSource) -> String {
    match (kind, source) {
        (AuthKind::Basic, CredentialSource::EnvFile) => "password from `.env`".to_string(),
        (AuthKind::Basic, CredentialSource::Environment) => {
            format!("password from {PASSWORD_ENV_VAR}")
        }
        (AuthKind::Basic, CredentialSource::SeedPassword) => {
            "the seed password from `DHIS2_SEED_PASSWORD` in `.env`".to_string()
        }
        (AuthKind::Basic, CredentialSource::Default) => "the DHIS2 default password".to_string(),
        (AuthKind::Token, CredentialSource::EnvFile) => "API token from `.env`".to_string(),
        (AuthKind::Token, _) => format!("API token from {TOKEN_ENV_VAR}"),
    }
}

/// The secret half of [`Credentials`].
#[derive(Clone)]
enum Secret {
    Basic { user: String, password: String },
    Token(String),
}

/// A DHIS2 user and password, or a personal access token, and where it was
/// found.
///
/// `Debug` is written by hand: a derived one would put the password into every
/// `-d` line and every panic message that carries this value.
#[derive(Clone)]
pub struct Credentials {
    secret: Secret,
    pub source: CredentialSource,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("Credentials");
        match &self.secret {
            Secret::Basic { user, .. } => debug.field("user", user).field("password", &"<hidden>"),
            Secret::Token(_) => debug.field("token", &"<hidden>"),
        };
        debug.field("source", &self.source).finish()
    }
}

impl Credentials {
    /// A username and password, however they were arrived at.
    pub fn new(user: &str, password: &str, source: CredentialSource) -> Credentials {
        Credentials {
            secret: Secret::Basic {
                user: user.to_string(),
                password: password.to_string(),
            },
            source,
        }
    }

    /// A personal access token.
    pub fn token(token: &str, source: CredentialSource) -> Credentials {
        Credentials {
            secret: Secret::Token(token.to_string()),
            source,
        }
    }

    /// Which scheme this is.
    pub fn kind(&self) -> AuthKind {
        match self.secret {
            Secret::Basic { .. } => AuthKind::Basic,
            Secret::Token(_) => AuthKind::Token,
        }
    }

    /// The username, for a password. A token carries none that chap-devops can
    /// read; DHIS2 says whose it is when asked ([`ME_PATH`]).
    ///
    /// [`ME_PATH`]: super::ME_PATH
    pub fn user(&self) -> Option<&str> {
        match &self.secret {
            Secret::Basic { user, .. } => Some(user),
            Secret::Token(_) => None,
        }
    }

    /// The `Authorization` header value, which is the only thing that reads
    /// the secret.
    pub fn header(&self) -> String {
        match &self.secret {
            Secret::Basic { user, password } => {
                format!("Basic {}", base64(format!("{user}:{password}").as_bytes()))
            }
            Secret::Token(token) => format!("ApiToken {token}"),
        }
    }

    /// What a trace line says went out, in place of the header.
    pub(super) fn traced(&self) -> String {
        match &self.secret {
            Secret::Basic { user, .. } => format!("Basic <{user} and its password>"),
            Secret::Token(_) => "ApiToken <the token>".to_string(),
        }
    }

    /// The phrase a report names this by.
    pub fn describe(&self) -> String {
        describe_credential(self.kind(), self.source)
    }
}

/// Everything [`credentials_of`] decides between, handed in so the rule can be
/// tested without an environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct CredentialInputs<'a> {
    /// The deployment's `.env`, as text.
    pub env_body: &'a str,
    /// `--user NAME`.
    pub user: Option<&'a str>,
    /// [`USERNAME_ENV_VAR`].
    pub env_username: Option<&'a str>,
    /// [`PASSWORD_ENV_VAR`].
    pub env_password: Option<&'a str>,
    /// [`TOKEN_ENV_VAR`].
    pub env_token: Option<&'a str>,
    /// Whether chap-devops deployed this DHIS2, which is the only case in which
    /// [`DEFAULT_PASSWORD`] is known to be anybody's password.
    pub deployed: bool,
    /// `DHIS2_SEED_PASSWORD` in `.env` of a DHIS2 chap-devops deployed from a
    /// seed: the password of every user, so it serves any `--user`.
    pub seed_password: Option<&'a str>,
}

/// The credentials for this deployment's DHIS2, read from `.env` and the
/// environment this command runs in.
pub fn credentials_for(
    project_dir: &Path,
    user: Option<&str>,
    deployed: bool,
    seeded_with_password: bool,
) -> Result<Credentials> {
    let body =
        std::fs::read_to_string(project_dir.join(crate::project::ENV_FILE)).unwrap_or_default();
    let seed_password = seeded_with_password
        .then(|| crate::dotenv::non_empty(&body, crate::components::DHIS2_SEED_PASSWORD_ENV_VAR))
        .flatten();
    let var = |name: &str| std::env::var(name).ok();
    let (env_username, env_password, env_token) = (
        var(USERNAME_ENV_VAR),
        var(PASSWORD_ENV_VAR),
        var(TOKEN_ENV_VAR),
    );
    credentials_of(&CredentialInputs {
        env_body: &body,
        user,
        env_username: env_username.as_deref(),
        env_password: env_password.as_deref(),
        env_token: env_token.as_deref(),
        deployed,
        seed_password: seed_password.as_deref(),
    })
}

/// [`credentials_for`]'s rule.
///
/// **A password belongs to the user it was set with.** Each source is a pair,
/// and the first pair that has a value wins:
///
/// 1. `.env`: [`API_TOKEN_ENV_VAR`], then [`ADMIN_PASSWORD_ENV_VAR`] for the
///    user [`ADMIN_USERNAME_ENV_VAR`] names (`admin` when it names none);
/// 2. the environment: [`TOKEN_ENV_VAR`], then [`PASSWORD_ENV_VAR`] for
///    [`USERNAME_ENV_VAR`], or for the `.env` user when that is unset;
/// 3. `admin` / [`DEFAULT_PASSWORD`], on a DHIS2 chap-devops deployed only.
///
/// `.env` first, because that is the deployment's own answer and the file every
/// other secret of it lives in - the same order [`crate::api::token_for`] uses
/// for chap-core's token. Within a source the token comes first: it is the
/// credential that can be scoped and revoked, and an operator who wrote one
/// down meant it to be used.
///
/// `user` (`--user NAME`) asks for one user by name, so it is always a
/// password, and only a password that belongs to `NAME` is sent:
/// [`ADMIN_PASSWORD_ENV_VAR`] when `.env` names that user,
/// [`PASSWORD_ENV_VAR`] unless [`USERNAME_ENV_VAR`] gives it to someone else,
/// and the default for `admin` on a deployed DHIS2. Anything else is an error
/// before a request is made, rather than another user's password sent in
/// `NAME`'s name to fail in DHIS2's audit log.
pub fn credentials_of(inputs: &CredentialInputs) -> Result<Credentials> {
    let clean = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let body = inputs.env_body;
    let file_user = crate::dotenv::non_empty(body, ADMIN_USERNAME_ENV_VAR);
    let file_password = crate::dotenv::non_empty(body, ADMIN_PASSWORD_ENV_VAR);
    let file_token = crate::dotenv::non_empty(body, API_TOKEN_ENV_VAR);
    let env_user = clean(inputs.env_username);
    let env_password = clean(inputs.env_password);
    let env_token = clean(inputs.env_token);
    // Only on a DHIS2 chap-devops deployed: a seed password says nothing about any
    // other instance.
    let seed_password = clean(inputs.seed_password).filter(|_| inputs.deployed);
    // Whose each password is.
    let file_owner = file_user
        .clone()
        .unwrap_or_else(|| DEFAULT_USERNAME.to_string());
    let default_allowed = inputs.deployed && file_owner == DEFAULT_USERNAME;

    if let Some(name) = clean(inputs.user) {
        if let Some(password) = file_password.filter(|_| file_owner == name) {
            return Ok(Credentials::new(
                &name,
                &password,
                CredentialSource::EnvFile,
            ));
        }
        if let Some(password) =
            env_password.filter(|_| env_user.as_ref().is_none_or(|u| *u == name))
        {
            return Ok(Credentials::new(
                &name,
                &password,
                CredentialSource::Environment,
            ));
        }
        if let Some(password) = seed_password {
            return Ok(Credentials::new(
                &name,
                &password,
                CredentialSource::SeedPassword,
            ));
        }
        if inputs.deployed && name == DEFAULT_USERNAME {
            return Ok(Credentials::new(
                &name,
                DEFAULT_PASSWORD,
                CredentialSource::Default,
            ));
        }
        return Err(anyhow::anyhow!(
            "chap-devops has no password for DHIS2 user `{name}`: `.env` and `{USERNAME_ENV_VAR}` \
             give theirs to other users; export `{PASSWORD_ENV_VAR}` for this run"
        ));
    }

    if let Some(token) = file_token {
        return Ok(Credentials::token(&token, CredentialSource::EnvFile));
    }
    if let Some(password) = file_password {
        return Ok(Credentials::new(
            &file_owner,
            &password,
            CredentialSource::EnvFile,
        ));
    }
    if let Some(token) = env_token {
        return Ok(Credentials::token(&token, CredentialSource::Environment));
    }
    if let Some(password) = env_password {
        let user = env_user.unwrap_or(file_owner);
        return Ok(Credentials::new(
            &user,
            &password,
            CredentialSource::Environment,
        ));
    }
    if let Some(password) = seed_password {
        return Ok(Credentials::new(
            &file_owner,
            &password,
            CredentialSource::SeedPassword,
        ));
    }
    if default_allowed {
        return Ok(Credentials::new(
            DEFAULT_USERNAME,
            DEFAULT_PASSWORD,
            CredentialSource::Default,
        ));
    }
    Err(anyhow::anyhow!(match inputs.deployed {
        true => format!(
            "chap-devops has no password for DHIS2 user `{file_owner}`: `.env` names the user and sets \
             no `{ADMIN_PASSWORD_ENV_VAR}`; set it there, or export `{PASSWORD_ENV_VAR}`"
        ),
        false => format!(
            "chap-devops has no credentials for this DHIS2, and did not deploy it, so there is no \
             default to try; {}",
            CREDENTIALS_WAY_OUT
        ),
    }))
}

/// The two ways to give chap-devops a DHIS2's credentials, in `.env` where the rest
/// of the deployment's secrets are.
pub const CREDENTIALS_WAY_OUT: &str = "set `DHIS2_ADMIN_USERNAME` and `DHIS2_ADMIN_PASSWORD`, or \
     `DHIS2_API_TOKEN`, in `.env`";
