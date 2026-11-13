//! The `chap-devops` command tree.
//!
//! Doc comments on the types and fields are the `--help` text, and every one of
//! them is a single clause: asking for help is not asking for the manual, so the
//! explanations live in the book (`docs/`) and the reference chapter is
//! generated from these one-liners. A subcommand's own help comes from its
//! variant in the enum, never from the argument struct the variant names. This
//! module and `main.rs` are complete: the command modules only read the arg
//! structs.

mod bench;
mod chap;
mod dhis2;
mod lifecycle;
mod models;
mod operate;
mod project;
mod run;

pub use bench::*;
pub use chap::*;
pub use dhis2::*;
pub use lifecycle::*;
pub use models::*;
pub use operate::*;
pub use project::*;
pub use run::*;

use crate::compose::PortRequest;
use crate::registry::DEFAULT_REGISTRY_URL;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// The value of `--port`: a number, or `auto` for the lowest free one.
///
/// Parsed here rather than in the command so a typo is a clap error next to
/// the flag, not a failure halfway through a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortArg(pub PortRequest);

impl std::str::FromStr for PortArg {
    type Err = String;

    fn from_str(text: &str) -> std::result::Result<PortArg, String> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("auto") {
            return Ok(PortArg(PortRequest::Auto));
        }
        match text.parse::<u16>() {
            Ok(0) => Err("0 is not a host port; pass a number or `auto`".to_string()),
            Ok(port) => Ok(PortArg(PortRequest::Fixed(port))),
            Err(_) => Err(format!("`{text}` is neither a port number nor `auto`")),
        }
    }
}

/// The value of `components enable --port`: a number, or `none` for a component
/// that is only reached inside the compose network.
///
/// `none` rather than a second subcommand, because `enable` is already how a
/// component's settings change and a port that can be moved should be
/// removable the same way. `Option<ComponentPortArg>` then has three states -
/// flag absent, `--port none`, `--port 9010` - which is exactly the three
/// things an operator can mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentPortArg(pub Option<u16>);

impl std::str::FromStr for ComponentPortArg {
    type Err = String;

    fn from_str(text: &str) -> std::result::Result<ComponentPortArg, String> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("none") {
            return Ok(ComponentPortArg(None));
        }
        match text.parse::<u16>() {
            Ok(0) => Err("0 is not a host port; pass a number or `none`".to_string()),
            Ok(port) => Ok(ComponentPortArg(Some(port))),
            Err(_) => Err(format!("`{text}` is neither a port number nor `none`")),
        }
    }
}

/// The one-liner both `chap-devops -h` and `chap-devops --help` open with. There is no
/// `long_about`: the two spellings of help say the same thing.
const ABOUT: &str = "deploy Chap (climate-informed disease forecasting) and its services";

/// The last line of `chap-devops --help`. Anything longer than a clause belongs in
/// the book, so the help points at it instead of repeating it.
///
/// A line, and nothing more: the blank line the help ends on is written where
/// the help is printed (`print_and_exit` in `main`), because clap `trim_end`s
/// the rendered help and appends one newline of its own, so a newline added
/// here or to `after_help` would be thrown away.
pub const DOCS_LINE: &str = "Docs: https://dhis2-chap.github.io/chap-devops/";

/// The book itself, for the places that open it rather than print it. A test
/// keeps it and [`DOCS_LINE`] pointing at the same place.
pub const DOCS_URL: &str = "https://dhis2-chap.github.io/chap-devops/";

/// The heading the global options are listed under, in their own block after
/// each command's own options.
const GLOBAL: &str = "Global options";

/// The heading of `--registry-url`, `--offline` and `--cache-dir`. They are
/// global, so `chap-devops --offline init` parses, but [`command`] hides them from
/// the help of every command that does not read them.
const REGISTRY: &str = "Registry options";

/// The ids of the registry options, as clap names them.
const REGISTRY_OPTIONS: &[&str] = &["registry_url", "offline", "cache_dir"];

/// The commands whose help lists registry options, and which ones.
///
/// Every other command parses them (they are global) and its help leaves them
/// out. The root lists them all, because the root is where they are usually
/// typed. A group is here only when every subcommand it has reads the
/// registry. `self update` and the two `dhis2` commands use `--offline` (and
/// `self update` the cache directory) for things other than the registry.
const REGISTRY_USERS: &[(&str, &[&str])] = &[
    ("init", REGISTRY_OPTIONS),
    ("run", REGISTRY_OPTIONS),
    ("stop", REGISTRY_OPTIONS),
    ("models", REGISTRY_OPTIONS),
    ("models list", REGISTRY_OPTIONS),
    ("models search", REGISTRY_OPTIONS),
    ("models info", REGISTRY_OPTIONS),
    ("models test", REGISTRY_OPTIONS),
    ("models configs list", REGISTRY_OPTIONS),
    ("models configs add", REGISTRY_OPTIONS),
    ("models configs update", REGISTRY_OPTIONS),
    ("models configs archive", REGISTRY_OPTIONS),
    ("models configs export", REGISTRY_OPTIONS),
    ("models configs sync", REGISTRY_OPTIONS),
    ("models add", REGISTRY_OPTIONS),
    ("models remove", REGISTRY_OPTIONS),
    ("models enable", REGISTRY_OPTIONS),
    ("models disable", REGISTRY_OPTIONS),
    ("models expose", REGISTRY_OPTIONS),
    ("models unexpose", REGISTRY_OPTIONS),
    ("components enable", REGISTRY_OPTIONS),
    ("components disable", REGISTRY_OPTIONS),
    ("ui", REGISTRY_OPTIONS),
    ("registry", REGISTRY_OPTIONS),
    ("registry update", REGISTRY_OPTIONS),
    ("registry show", REGISTRY_OPTIONS),
    ("sync", REGISTRY_OPTIONS),
    ("update", REGISTRY_OPTIONS),
    ("up", REGISTRY_OPTIONS),
    ("backup restore", REGISTRY_OPTIONS),
    ("chap", REGISTRY_OPTIONS),
    ("doctor", REGISTRY_OPTIONS),
    ("auth enable", REGISTRY_OPTIONS),
    ("auth disable", REGISTRY_OPTIONS),
    ("auth rotate", REGISTRY_OPTIONS),
    ("dhis2 apps", &["offline"]),
    ("dhis2 connect", REGISTRY_OPTIONS),
    ("self update", &["offline", "cache_dir"]),
];

/// The registry options a command's help lists, by its path below `chap-devops`
/// (`"models list"`).
pub fn registry_options_of(path: &str) -> &'static [&'static str] {
    REGISTRY_USERS
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, ids)| *ids)
        .unwrap_or(&[])
}

/// The command tree that parses and prints help: [`Cli::command`] with the
/// registry options hidden where they do not apply.
///
/// Clap copies a global option into each subcommand when it builds the tree,
/// unless the subcommand already has an option with that id. So every
/// subcommand gets its own copies first, in the order clap would use, with
/// the registry options hidden where [`REGISTRY_USERS`] does not list them.
/// The copies stay global, so an option still parses on every command, before
/// or after its name.
pub fn command() -> clap::Command {
    use clap::CommandFactory;
    let root = Cli::command();
    let globals: Vec<clap::Arg> = root
        .get_arguments()
        .filter(|arg| arg.is_global_set())
        .cloned()
        .collect();
    with_global_copies(root, "", &globals)
}

/// Give each subcommand below `command` its own copy of the global options.
fn with_global_copies(
    mut command: clap::Command,
    prefix: &str,
    globals: &[clap::Arg],
) -> clap::Command {
    let names: Vec<String> = command
        .get_subcommands()
        .map(|sub| sub.get_name().to_string())
        .collect();
    for name in names {
        let path = format!("{prefix}{name}");
        command = command.mut_subcommand(&name, |mut sub| {
            let shown = registry_options_of(&path);
            for arg in globals {
                let id = arg.get_id().as_str();
                let hide = REGISTRY_OPTIONS.contains(&id) && !shown.contains(&id);
                sub = sub.arg(arg.clone().hide(hide));
            }
            with_global_copies(sub, &format!("{path} "), globals)
        });
    }
    command
}

/// Deploy Chap (climate-informed disease forecasting) and its services.
#[derive(Debug, Parser)]
// `bin_name` as well as `name`: without it clap takes the usage line from
// argv[0], so the same help reads `chap-devops.exe` on Windows and `chap-devops`
// everywhere else - and `docs/reference.md` is generated from that help.
#[command(name = "chap-devops", bin_name = "chap-devops", version, about = ABOUT, after_help = DOCS_LINE)]
pub struct Cli {
    /// Print machine-readable JSON instead of human output
    #[arg(long, global = true, help_heading = GLOBAL)]
    pub json: bool,

    /// Never colour the output (NO_COLOR does the same)
    #[arg(long, global = true, help_heading = GLOBAL)]
    pub no_color: bool,

    /// Show the hints; -vv also shows the commands and requests that run
    #[arg(short, long, global = true, action = clap::ArgAction::Count, help_heading = GLOBAL)]
    pub verbose: u8,

    /// -vv plus the raw responses
    #[arg(short, long, global = true, help_heading = GLOBAL)]
    pub debug: bool,

    /// Deployment directory, or any directory inside one
    #[arg(
        short = 'C',
        long,
        global = true,
        value_name = "DIR",
        default_value = ".",
        help_heading = GLOBAL
    )]
    pub project_dir: PathBuf,

    /// URL of the marketplace registry index
    #[arg(long, global = true, value_name = "URL", default_value = DEFAULT_REGISTRY_URL, help_heading = REGISTRY)]
    pub registry_url: String,

    /// Never touch the network; use what is cached or on this machine
    #[arg(long, global = true, help_heading = REGISTRY)]
    pub offline: bool,

    /// Cache directory: the registry snapshot and the self-update downloads
    #[arg(long, global = true, value_name = "DIR", help_heading = REGISTRY)]
    pub cache_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a deployment directory: compose files, .env and .chap-devops/
    Init(Box<InitArgs>),

    /// Show and change what this deployment is made of
    Components(ComponentsArgs),

    /// Browse and manage marketplace models
    Models(ModelsArgs),

    /// Inspect and refresh the marketplace registry
    Registry(RegistryArgs),

    /// Render the compose files from .chap-devops/
    Sync(SyncArgs),

    /// Move the pins to what upstream publishes now, and pull
    Update(UpdateArgs),

    /// Enable or disable API authentication, and show the token
    Auth(AuthArgs),

    /// Let the DHIS2 Modeling App reach this deployment's Chap
    Dhis2(Dhis2Args),

    /// Sync, then start Chap (docker compose up)
    Up(UpArgs),

    /// Stop Chap (docker compose down)
    Down(DownArgs),

    /// Recreate the services whose image or configuration changed
    Restart(RestartArgs),

    /// Show chap-core health and which models registered
    Status(StatusArgs),

    /// Show container logs (docker compose logs)
    Logs(LogsArgs),

    /// Open a component's web interface in a browser
    Open(OpenArgs),

    /// Open the browser: marketplace models and components
    Ui(UiArgs),

    /// Start one model and print where it answers
    Run(ModelRunArgs),

    /// List the models that run, and where each one answers
    Ps(ModelPsArgs),

    /// Stop a model and remove its compose overlay; its data stays
    Stop(ModelStopArgs),

    /// List the backtests and predictions chap-core has run
    Jobs(JobsArgs),

    /// Send one authenticated request to chap-core's API
    Api(ApiArgs),

    /// Time the Modeling App's pages, or run backtests under load
    Bench(BenchArgs),

    /// Run the chap CLI in a container, with no Python or uv installed
    Chap(ChapArgs),

    /// Run the chapkit CLI in the chap-core image, with no Python installed
    Chapkit(ChapkitArgs),

    /// Create or restore a backup of this deployment
    Backup(BackupArgs),

    /// Run a checklist over this machine and this deployment
    Doctor(DoctorArgs),

    /// Use Docker directly: containers, images, compose
    Docker(DockerArgs),

    /// Watch every chap-devops deployment on this machine as a live tree
    Top(TopArgs),

    /// Delete what deployments whose directory is gone left in docker
    Cleanup(CleanupArgs),

    /// Update chap-devops itself, and report what this build is
    #[command(name = "self")]
    SelfCmd(SelfArgs),

    /// Print a shell completion script for chap-devops
    Completions(CompletionsArgs),

    /// Print the whole command tree as Markdown
    #[command(hide = true)]
    DocsMarkdown(DocsMarkdownArgs),
}

/// Update chap-devops itself, and report what this build is
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct SelfArgs {
    #[command(subcommand)]
    pub command: SelfSub,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SelfSub {
    /// Replace this binary with the newest release
    Update(SelfUpdateArgs),

    /// Show what this build is: version, revision, target and path
    Version(SelfVersionArgs),
}

/// Replace this binary with the newest release
#[derive(Debug, Clone, Args)]
pub struct SelfUpdateArgs {
    /// Report what an update would do and change nothing
    #[arg(long)]
    pub check: bool,

    /// Install this release tag instead of the newest one
    #[arg(long, value_name = "TAG")]
    pub version: Option<String>,

    /// Do not ask before replacing the binary
    #[arg(short, long)]
    pub yes: bool,
}

/// Show what this build is: version, revision, target and path
#[derive(Debug, Clone, Args)]
pub struct SelfVersionArgs {}

/// Print a shell completion script for chap-devops
#[derive(Debug, Clone, Args)]
pub struct CompletionsArgs {
    /// Shell to generate for
    #[arg(value_name = "SHELL")]
    pub shell: clap_complete::Shell,
}

/// Print the whole command tree as Markdown.
///
/// Hidden: it documents `chap-devops` rather than doing anything to a deployment,
/// and `make docs-reference` is the only caller. It needs no project and no
/// Docker.
#[derive(Debug, Clone, Args)]
pub struct DocsMarkdownArgs {}

#[cfg(test)]
mod tests;
