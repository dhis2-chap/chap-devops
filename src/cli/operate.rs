//! `chap-devops backup`, `status`, `open`, `jobs`, `api`, `doctor` and `auth`:
//! looking after a running deployment.

use clap::{Args, Subcommand};
use std::path::PathBuf;

/// Create or restore a backup of this deployment
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct BackupArgs {
    #[command(subcommand)]
    pub command: BackupSub,
}

#[derive(Debug, Clone, Subcommand)]
pub enum BackupSub {
    /// Write a tar.gz of the database, the data and the files
    Create(BackupCreateArgs),

    /// Put a deployment back from a backup archive
    Restore(RestoreArgs),
}

/// Write a tar.gz of the database, the data and the files
#[derive(Debug, Clone, Args)]
pub struct BackupCreateArgs {
    /// Where to write the archive: a file, or a directory
    #[arg(long, value_name = "PATH")]
    pub out: Option<PathBuf>,

    /// Do not put the chap-core database in the archive
    #[arg(long)]
    pub no_db: bool,

    /// Do not put the model data volumes in the archive
    #[arg(long)]
    pub no_models: bool,

    /// Do not put the component data volumes in the archive
    #[arg(long)]
    pub no_components: bool,
}

/// Put a deployment back from a backup archive
#[derive(Debug, Clone, Args)]
pub struct RestoreArgs {
    /// The tar.gz written by chap-devops backup create
    #[arg(value_name = "ARCHIVE")]
    pub archive: PathBuf,

    /// Skip the confirmation
    #[arg(long)]
    pub yes: bool,

    /// Restore only the deployment files; no Docker needed
    #[arg(long, conflicts_with_all = ["db_only", "no_models", "no_components", "no_start"])]
    pub files_only: bool,

    /// Restore only the chap-core database
    #[arg(long, conflicts_with_all = ["no_models", "no_components"])]
    pub db_only: bool,

    /// Leave the model data volumes as they are
    #[arg(long)]
    pub no_models: bool,

    /// Leave the component data volumes as they are
    #[arg(long)]
    pub no_components: bool,

    /// Use the compose project name from the archive
    #[arg(long, conflicts_with = "db_only")]
    pub adopt_identity: bool,

    /// Do not start the deployment at the end
    #[arg(long)]
    pub no_start: bool,
}

/// Show chap-core health and which models registered
#[derive(Debug, Clone, Args)]
pub struct StatusArgs {
    /// Base URL of the chap-core API
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// Request timeout in seconds
    #[arg(long, value_name = "SECONDS", default_value_t = 5)]
    pub timeout: u64,
}

/// Open a component's web interface in a browser
#[derive(Debug, Clone, Args)]
pub struct OpenArgs {
    // Optional, and `arg_required_else_help` is not what this wants: that is
    // the convention for a group with no default subcommand, and `open` is a
    // verb. With nothing named it lists what this deployment has and what each
    // one opens, which answers the question a bare `chap-devops open` is asking.
    /// Component to open (chap-core, ocs, dhis2), or an enabled model's id
    pub name: Option<String>,

    /// Print the address instead of opening a browser
    #[arg(long)]
    pub no_browser: bool,
}

/// List the backtests and predictions chap-core has run
///
/// `args_conflicts_with_subcommands`: the filters below are `list`'s, repeated
/// here so `chap-devops jobs --status FAILURE` works without the verb. Only one side
/// may carry them, so `chap-devops jobs --limit 5 show ID` is a usage error rather
/// than a flag that silently does nothing.
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct JobsArgs {
    /// Absent is `list`: the bare command is the one that answers "what has
    /// this deployment been doing", and that is the question people type.
    #[command(subcommand)]
    pub command: Option<JobsCmd>,

    #[command(flatten)]
    pub list: JobsListArgs,
}

#[derive(Debug, Clone, Subcommand)]
pub enum JobsCmd {
    /// List the jobs chap-core knows about, newest first
    List(JobsListArgs),

    /// Show everything chap-core records about one job
    Show(JobsShowArgs),

    /// Print one job's log, which is where a failure says why
    Logs(JobsLogsArgs),

    /// Ask chap-core to stop a job that is still running
    Cancel(JobsCancelArgs),

    /// Remove a finished job from chap-core's list
    Delete(JobsDeleteArgs),
}

/// List the jobs chap-core knows about, newest first
#[derive(Debug, Clone, Default, Args)]
pub struct JobsListArgs {
    /// Only jobs in this status; repeat for more than one
    #[arg(long, value_name = "STATUS")]
    pub status: Vec<String>,

    /// Only jobs of this type, such as create_backtest
    #[arg(long = "type", value_name = "TYPE")]
    pub kind: Option<String>,

    /// Show at most this many jobs
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

/// Show everything chap-core records about one job
#[derive(Debug, Clone, Args)]
pub struct JobsShowArgs {
    /// Job id, or enough of its start to name one job
    #[arg(value_name = "ID")]
    pub id: String,
}

/// Print one job's log, which is where a failure says why
#[derive(Debug, Clone, Args)]
pub struct JobsLogsArgs {
    /// Job id, or enough of its start to name one job
    #[arg(value_name = "ID")]
    pub id: String,

    /// Print only the last N lines of the log
    #[arg(long, value_name = "N")]
    pub tail: Option<usize>,
}

/// Ask chap-core to stop a job that is still running
#[derive(Debug, Clone, Args)]
pub struct JobsCancelArgs {
    /// Job id, or enough of its start to name one job
    #[arg(value_name = "ID")]
    pub id: String,
}

/// Remove a finished job from chap-core's list
#[derive(Debug, Clone, Args)]
pub struct JobsDeleteArgs {
    /// Job id, or enough of its start to name one job
    #[arg(value_name = "ID")]
    pub id: String,
}

/// Send one authenticated request to chap-core's API
#[derive(Debug, Clone, Args)]
pub struct ApiArgs {
    /// GET, POST, PUT, PATCH or DELETE; case does not matter
    #[arg(value_name = "METHOD")]
    pub method: String,

    /// Request path starting with /, query string and all
    #[arg(value_name = "PATH")]
    pub path: String,

    /// JSON body: inline, @file, or - to read stdin
    #[arg(long, value_name = "JSON|@FILE|-", allow_hyphen_values = true)]
    pub data: Option<String>,

    /// Base URL of the chap-core API
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// Print the body exactly as it arrived
    #[arg(long)]
    pub raw: bool,

    /// Request timeout in seconds
    #[arg(long, value_name = "SECONDS", default_value_t = 30)]
    pub timeout: u64,
}

/// Run a checklist over this machine and this deployment
#[derive(Debug, Clone, Args)]
pub struct DoctorArgs {}

/// Delete what deployments whose directory is gone left in docker
#[derive(Debug, Clone, Args)]
pub struct CleanupArgs {
    /// List what would be deleted and change nothing
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Delete without asking first
    #[arg(long, conflicts_with = "dry_run")]
    pub yes: bool,
}

/// Enable or disable API authentication, and show the token
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthSub,
}

#[derive(Debug, Clone, Subcommand)]
pub enum AuthSub {
    /// Say whether the API is protected, and by which token
    Show(AuthShowArgs),

    /// Print the API token alone, for a script to capture
    Token(AuthTokenArgs),

    /// Enable authentication: write both secrets and sync
    Enable(AuthEnableArgs),

    /// Disable authentication; keep both values as comments
    Disable(AuthDisableArgs),

    /// Replace both secrets with new generated ones
    Rotate(AuthRotateArgs),
}

/// Say whether the API is protected, and by which token
#[derive(Debug, Clone, Args)]
pub struct AuthShowArgs {
    /// Print the API token
    #[arg(long)]
    pub reveal: bool,
}

/// Print the API token alone, for a script to capture
#[derive(Debug, Clone, Args)]
pub struct AuthTokenArgs {}

/// Enable authentication: write both secrets and sync
#[derive(Debug, Clone, Args)]
pub struct AuthEnableArgs {
    /// Use this API token instead of generating one
    #[arg(long, value_name = "TOKEN")]
    pub token: Option<String>,
}

/// Disable authentication; keep both values as comments
#[derive(Debug, Clone, Args)]
pub struct AuthDisableArgs {}

/// Replace both secrets with new generated ones
#[derive(Debug, Clone, Args)]
pub struct AuthRotateArgs {}
