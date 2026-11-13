//! `chap-devops sync`, `update`, `up`, `down`, `logs`, `restart` and `chap-devops docker`:
//! rendering the compose files and running them.

use clap::{Args, Subcommand};

/// Render the compose files from .chap-devops/
#[derive(Debug, Clone, Args)]
pub struct SyncArgs {
    /// Write nothing; exit non-zero if anything would change
    #[arg(long)]
    pub check: bool,
}

/// Move the pins to what upstream publishes now, and pull
#[derive(Debug, Clone, Args)]
pub struct UpdateArgs {
    /// Show what would change: no pull, and nothing written
    #[arg(long)]
    pub dry_run: bool,

    /// Pin a moving chap-core tag to the newest release
    #[arg(long)]
    pub pin_chap_core: bool,

    /// Move chap-core to this tag: vX.Y.Z, latest, master or dev
    #[arg(long, value_name = "TAG", conflicts_with = "pin_chap_core")]
    pub chap_tag: Option<String>,

    /// List the chap-core tags you can move to, newest first
    #[arg(long, conflicts_with_all = ["chap_tag", "pin_chap_core"])]
    pub list_tags: bool,

    /// Answer yes to the confirmation a backwards move asks for
    #[arg(long)]
    pub yes: bool,
}

/// Sync, then start Chap (docker compose up)
#[derive(Debug, Clone, Args)]
pub struct UpArgs {
    /// Run in the foreground and stream all logs (Ctrl-C stops Chap)
    #[arg(short = 'a', long, visible_alias = "foreground")]
    pub attach: bool,

    /// Pull every image first (docker compose up --pull always)
    #[arg(long)]
    pub pull: bool,

    /// Do not check the host ports first
    #[arg(long)]
    pub no_preflight: bool,

    /// First stop the other chap-devops deployments that hold these ports
    #[arg(long, conflicts_with = "no_preflight")]
    pub replace: bool,

    /// Return only once chap-core, the models, OCS and DHIS2 answer
    #[arg(long, conflicts_with = "attach")]
    pub wait: bool,

    /// How long --wait waits before it fails, in seconds
    #[arg(long, value_name = "SECONDS", default_value_t = 300, requires = "wait")]
    pub timeout: u64,

    /// Extra arguments passed through to docker compose up
    #[arg(
        value_name = "EXTRA",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub extra: Vec<String>,
}

/// Stop Chap (docker compose down)
#[derive(Debug, Clone, Args)]
pub struct DownArgs {
    // Long spelling only: `-v` is the global `--verbose` flag, and a `down`
    // that kept or dropped the data depending on which of the two clap
    // resolved it to would be the worst of both.
    /// Also remove this deployment's volumes, destroying its data
    #[arg(long)]
    pub volumes: bool,

    /// Skip the confirmation
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Extra arguments passed through to docker compose down
    #[arg(
        value_name = "EXTRA",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub extra: Vec<String>,
}

/// Show container logs (docker compose logs)
#[derive(Debug, Clone, Args)]
pub struct LogsArgs {
    /// Show new output when it arrives
    #[arg(short = 'f', long)]
    pub follow: bool,

    /// Show only the last N lines of each service's log
    #[arg(long, value_name = "N")]
    pub tail: Option<usize>,

    /// Services to show logs for; all of them when omitted
    #[arg(value_name = "SERVICE")]
    pub services: Vec<String>,
}

/// Recreate the services whose image or configuration changed
#[derive(Debug, Clone, Args)]
pub struct RestartArgs {
    /// Recreate the named services even when nothing changed
    #[arg(long)]
    pub all: bool,

    /// Set CHAP_API_WORKERS to N and recreate only the chap service
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=64), conflicts_with_all = ["all", "services"])]
    pub api_workers: Option<u32>,

    /// Services to restart; the whole deployment when omitted
    #[arg(value_name = "SERVICE")]
    pub services: Vec<String>,
}

/// Use Docker directly: containers, images, compose
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct DockerArgs {
    #[command(subcommand)]
    pub command: DockerSub,
}

#[derive(Debug, Clone, Subcommand)]
pub enum DockerSub {
    /// List the containers this deployment is running
    Ps(PsArgs),

    /// Download the pinned images into the local Docker daemon
    Pull(PullArgs),

    /// Run a command inside one of the running containers
    Exec(ExecArgs),

    /// Run any docker compose command against this deployment
    Run(RunArgs),

    /// Print every compose file merged into one document
    Config(ConfigArgs),
}

/// List the containers this deployment is running
#[derive(Debug, Clone, Args)]
pub struct PsArgs {
    /// Extra arguments passed through to docker compose ps
    #[arg(
        value_name = "EXTRA",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub extra: Vec<String>,
}

/// Download the pinned images into the local Docker daemon
#[derive(Debug, Clone, Args)]
pub struct PullArgs {}

/// Run a command inside one of the running containers
#[derive(Debug, Clone, Args)]
pub struct ExecArgs {
    /// Service to run the command in
    #[arg(value_name = "SERVICE")]
    pub service: String,

    /// Command to run; a shell (sh) when omitted
    #[arg(
        value_name = "CMD",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub cmd: Vec<String>,
}

/// Run any docker compose command against this deployment
#[derive(Debug, Clone, Args)]
pub struct RunArgs {
    /// Arguments passed to docker compose after the -f list
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub args: Vec<String>,
}

/// Print every compose file merged into one document
#[derive(Debug, Clone, Args)]
pub struct ConfigArgs {
    /// Extra arguments passed through to docker compose config
    #[arg(
        value_name = "EXTRA",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub extra: Vec<String>,
}

/// The docker compose wrappers, as one value for `commands::docker::run`.
///
/// Flat on purpose: `up`, `down` and `logs` are top-level commands while the
/// rest live under `chap-devops docker`, but all of them build one argument list and
/// run down the same path.
#[derive(Debug, Clone)]
pub enum DockerCmd {
    Up(UpArgs),
    Down(DownArgs),
    Logs(LogsArgs),
    Restart(RestartArgs),
    Ps(PsArgs),
    Pull(PullArgs),
    Exec(ExecArgs),
    Run(RunArgs),
    Config(ConfigArgs),
}

impl From<&DockerSub> for DockerCmd {
    fn from(sub: &DockerSub) -> DockerCmd {
        match sub {
            DockerSub::Ps(args) => DockerCmd::Ps(args.clone()),
            DockerSub::Pull(args) => DockerCmd::Pull(args.clone()),
            DockerSub::Exec(args) => DockerCmd::Exec(args.clone()),
            DockerSub::Run(args) => DockerCmd::Run(args.clone()),
            DockerSub::Config(args) => DockerCmd::Config(args.clone()),
        }
    }
}
