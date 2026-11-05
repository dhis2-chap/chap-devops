//! Which account a marketplace model's service runs as, read off the image.
//!
//! A model image declares the account it runs as in `config.User`, and forcing
//! a different one is how a model ends up unable to execute its own binaries:
//! the Rwanda BYM model shipped root-owned, mode 744 INLA binaries up to
//! 0.1.1, so a hardened `user: chapkit:chapkit` turned every prediction into
//! `inla.run: Permission denied`. So the image is asked, in the order the
//! answers can be trusted: the flag, the registry, the local daemon, and only
//! then the table compiled into this binary.
//!
//! What comes out is recorded on the [`crate::project::EnabledModel`], which
//! is what keeps `chap-devops sync` offline and deterministic: the lookup happens
//! once, when a model is enabled or moved to a new tag, and every render after
//! that reads the answer out of `.chap-devops/models.yaml`.

use crate::compose::overrides;
use crate::error::Result;
use crate::manual::{Endpoints, Origin, ProbeFn, PullFn, ghcr, resolve_user_with};
use serde::{Deserialize, Serialize};

/// Where the user recorded for a model came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UserSource {
    /// `--user` on the command line.
    Flag,
    /// `config.User` of the image, read from the registry.
    ImageConfig,
    /// The local docker daemon: `docker image inspect`, or `id` inside the
    /// image for an account name nothing else could resolve.
    DockerProbe,
    /// The table in [`crate::compose::overrides`], because nothing could be
    /// asked. It is also what an entry without `user_from` reads as.
    #[default]
    Table,
}

impl UserSource {
    /// How the source reads in `models info`, the browser and `chap-devops update`.
    pub fn label(self) -> &'static str {
        match self {
            UserSource::Flag => "--user",
            UserSource::ImageConfig => "image config",
            UserSource::DockerProbe => "docker probe",
            UserSource::Table => "table",
        }
    }
}

/// One model whose data directory and user are to be resolved.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    /// Marketplace id, for the notes.
    pub id: &'a str,
    /// Tagless image reference.
    pub image: &'a str,
    /// The tag the enable pins, which is the image that will actually run.
    pub image_tag: &'a str,
    /// The digest pinned with the tag, which the lookups use when there is one.
    pub image_digest: Option<&'a str>,
    pub data_dir_flag: Option<&'a str>,
    pub user_flag: Option<&'a str>,
}

impl Request<'_> {
    /// The reference the lookups are made against.
    fn reference(&self) -> String {
        crate::compose::image_ref(
            self.image,
            &crate::compose::pinned_tag(self.image_tag, self.image_digest),
        )
    }

    /// What the registry is asked for: the digest when there is one, as that
    /// is what runs; the tag otherwise.
    fn registry_reference(&self) -> &str {
        self.image_digest.unwrap_or(self.image_tag)
    }

    /// The repository inside its registry, such as `<owner>/<name>`.
    fn repository(&self) -> String {
        crate::manual::location::Location::of(self.image, &Endpoints::default()).repository
    }
}

/// What the resolution decided, and what it had to guess at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub data_dir: String,
    /// The `user:` the overlay runs the service as: `root`, a numeric
    /// `uid:gid`, or the account name when nothing could turn it into numbers.
    pub user: String,
    pub user_from: UserSource,
    /// Whether the image reads its port from `PORT`, which decides the port
    /// mapping for a chap-core elsewhere. See [`reads_port_env`].
    pub reads_port: bool,
    pub notes: Vec<String>,
}

/// Whether an image's command reads the port from `PORT`, rather than fixing
/// it.
///
/// A chapkit image either starts uvicorn with `--port 8000` itself, or runs a
/// module that reads `PORT`. The first listens on 8000 whatever it is told,
/// so the second is the only one a different port can be handed to. A
/// command that names `--port` fixes it; any other command that is known
/// reads `PORT`; an unknown command is taken as the common case, 8000.
pub fn reads_port_env(command: &[String]) -> bool {
    let fixes_port = command.iter().any(|part| {
        part.split_whitespace()
            .any(|word| word == "--port" || word.starts_with("--port="))
    });
    !command.is_empty() && !fixes_port
}

/// How a caller of [`crate::compose::apply::apply_with`] resolves one model.
pub type ResolveFn<'a> = &'a dyn Fn(&Request) -> Resolution;

/// Reading `config.User` and `config.WorkingDir` off the registry:
/// `(repository, tag) -> config`, with `None` for a tag it does not publish.
pub type RegistryFn<'a> = &'a dyn Fn(&str, &str) -> Result<Option<ghcr::ImageConfig>>;
/// The same two fields from the local daemon: `reference -> (user,
/// working_dir)`, with `None` when this machine's image store cannot answer
/// for the `linux/amd64` variant the overlay runs - which is not the same as
/// "it runs as root", and is why an empty config falls through to the table.
pub type LocalFn<'a> = &'a dyn Fn(&str) -> Option<(String, String)>;
/// The command of a local image, `reference -> Entrypoint + Cmd`.
pub type CommandFn<'a> = &'a dyn Fn(&str) -> Option<Vec<String>>;

/// Resolve one model against the image it pins.
///
/// The order is the flag, the image config from ghcr, the same config from a
/// locally pulled image, and the built-in table. Nothing here fails: an answer
/// nobody could give is the table plus a note saying so, because a model that
/// cannot be enabled at all is worse than one enabled from the last thing this
/// CLI knows.
pub fn from_image(req: &Request, endpoints: &Endpoints) -> Resolution {
    from_image_with(
        req,
        endpoints,
        &|_, tag| crate::manual::location::image_config(req.image, tag, endpoints),
        &crate::docker::image_config,
        &crate::docker::image_command,
        &crate::docker::pull_image,
        &crate::docker::uid_gid_in_image,
    )
}

/// [`from_image`] with every lookup injected, so the order can be tested
/// without a registry or a daemon.
pub fn from_image_with(
    req: &Request,
    endpoints: &Endpoints,
    registry: RegistryFn,
    local: LocalFn,
    command: CommandFn,
    pull: PullFn,
    probe: ProbeFn,
) -> Resolution {
    let reference = req.reference();
    let mut notes = Vec::new();
    let mut declared: Option<(ghcr::ImageConfig, UserSource)> = None;

    if !endpoints.offline {
        match registry(&req.repository(), req.registry_reference()) {
            Ok(Some(config)) => declared = Some((config, UserSource::ImageConfig)),
            Ok(None) => notes.push(format!(
                "{reference} is not published at {}, so its user comes from the \
                 local image or the built-in table",
                crate::manual::location::Location::of(req.image, endpoints).base
            )),
            Err(err) => notes.push(format!(
                "could not read the image config of {reference} from the registry \
                 ({err:#}); falling back to the local image"
            )),
        }
    }
    if declared.is_none()
        && endpoints.docker_probe
        && let Some((user, working_dir)) = local(&reference)
    {
        declared = Some((
            ghcr::ImageConfig {
                user,
                working_dir,
                amd64_only: false,
                command: command(&reference).unwrap_or_default(),
            },
            UserSource::DockerProbe,
        ));
    }

    let known = overrides::known_override(req.id);
    let data_dir = match (flag(req.data_dir_flag), &declared) {
        (Some(dir), _) => dir,
        (None, Some((config, _))) if !config.working_dir.trim().is_empty() => {
            crate::manual::data_dir_under(&config.working_dir)
        }
        _ => known
            .map(|k| k.data_dir.to_string())
            .unwrap_or_else(|| overrides::DEFAULT_DATA_DIR.to_string()),
    };

    let (user, user_from) = match (flag(req.user_flag), &declared) {
        (Some(user), _) => (user, UserSource::Flag),
        (None, Some((config, from))) => {
            // An image config with no `User` at all is an image that runs as
            // root; saying "nothing declared" here would hand it the chapkit
            // default, which is exactly the bug this module exists for.
            let user = match config.user.trim() {
                "" => "root",
                user => user,
            };
            // The name-to-numbers half is the same one `chap-devops models add`
            // runs, `id -u` inside the image included.
            let (user, origin) =
                resolve_user_with(None, user, &reference, endpoints, &mut notes, pull, probe);
            let from = match origin {
                Origin::Probe => UserSource::DockerProbe,
                _ => *from,
            };
            (user, from)
        }
        (None, None) => {
            notes.push(format!(
                "nothing could say what {reference} runs as, so `{}` was enabled \
                 with the user the built-in table records - re-run `chap-devops models \
                 enable {}` with a network, or pass `--user <uid>:<gid>`",
                req.id, req.id
            ));
            let table = from_table(req);
            (table.user, table.user_from)
        }
    };

    let reads_port = declared
        .as_ref()
        .is_some_and(|(config, _)| reads_port_env(&config.command));
    Resolution {
        data_dir,
        user: normalize(&user),
        user_from,
        reads_port,
        notes,
    }
}

/// The table alone: no registry, no daemon, no notes.
///
/// What a caller that has already decided not to look anything up passes to
/// [`crate::compose::apply::apply_with`] - the unit tests, which must render
/// the same bytes on a machine with the images and on one without.
pub fn from_table(req: &Request) -> Resolution {
    let known = overrides::known_override(req.id);
    let (user, user_from) = match flag(req.user_flag) {
        Some(user) => (user, UserSource::Flag),
        None => (
            known
                .map(|k| k.user.to_string())
                .unwrap_or_else(|| overrides::DEFAULT_USER.to_string()),
            UserSource::Table,
        ),
    };
    Resolution {
        data_dir: flag(req.data_dir_flag)
            .or_else(|| known.map(|k| k.data_dir.to_string()))
            .unwrap_or_else(|| overrides::DEFAULT_DATA_DIR.to_string()),
        user: normalize(&user),
        user_from,
        reads_port: false,
        notes: Vec::new(),
    }
}

/// A flag that was given and is not blank.
fn flag(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// The form a resolved user is recorded and rendered in.
///
/// Root is written as `root`, because that is the one value the overlay reads
/// as "no `user:` line" and `root` says so where `0:0` would have to be
/// decoded. Everything else that resolves becomes the numeric pair, so the
/// `user:` line and the init container's `chown` are the same two
/// numbers. A name nothing could resolve is kept as it is, which is what
/// `chap-devops sync` warns about.
///
/// `chap-devops doctor` compares through here as well: `chapkit` off an image and
/// `1000:1000` out of `models.yaml` are the same answer written two ways.
pub fn normalize(user: &str) -> String {
    if overrides::is_root(user) {
        return "root".to_string();
    }
    overrides::numeric_pair(user).unwrap_or_else(|| user.trim().to_string())
}

#[cfg(test)]
mod tests;
