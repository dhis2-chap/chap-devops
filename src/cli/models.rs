//! `chap-devops models`, `chap-devops ui` and `chap-devops registry`: the marketplace and what
//! this deployment runs from it.

use super::PortArg;
use crate::registry::Channel;
use clap::{Args, Subcommand};

/// Browse and manage marketplace models
#[derive(Debug, Args)]
pub struct ModelsArgs {
    #[command(subcommand)]
    pub command: ModelsCmd,
}

#[derive(Debug, Subcommand)]
pub enum ModelsCmd {
    /// List marketplace models
    List(ModelsListArgs),

    /// Search the marketplace by id, name or summary
    Search(ModelsSearchArgs),

    /// Show everything known about one model
    Info(ModelsInfoArgs),

    /// Make a model train and predict, and say whether it could
    Test(ModelsTestArgs),

    /// List, add, archive and sync the configured models of each model
    Configs(ConfigsArgs),

    /// Create a model project from a template
    New(ModelsNewArgs),

    /// Add a model the marketplace does not list
    Add(ModelsAddArgs),

    /// Remove a model that was added with models add
    Remove(ModelsRemoveArgs),

    /// Enable a model and write its compose overlay
    Enable(ModelsEnableArgs),

    /// Disable a model and remove its compose overlay
    Disable(ModelsDisableArgs),

    /// Publish a host port for an enabled model
    Expose(ModelsExposeArgs),

    /// Remove the host port of an enabled model
    Unexpose(ModelsUnexposeArgs),
}

/// List marketplace models
#[derive(Debug, Clone, Args)]
pub struct ModelsListArgs {
    /// Include every entry, starters as well as models
    #[arg(long)]
    pub all: bool,

    /// List only starters
    #[arg(long)]
    pub starters: bool,

    /// List only the models enabled in this deployment
    #[arg(long)]
    pub enabled: bool,
}

/// Search the marketplace by id, name or summary
#[derive(Debug, Clone, Args)]
pub struct ModelsSearchArgs {
    /// Text to look for; matching is case-insensitive
    #[arg(value_name = "QUERY")]
    pub query: String,
}

/// Show everything known about one model
#[derive(Debug, Clone, Args)]
pub struct ModelsInfoArgs {
    /// Marketplace id or service id
    #[arg(value_name = "ID")]
    pub id: String,
}

/// Make a model train and predict, and say whether it could
#[derive(Debug, Clone, Args)]
pub struct ModelsTestArgs {
    /// Marketplace ids or service ids of the models to test
    #[arg(value_name = "ID", conflicts_with = "all")]
    pub ids: Vec<String>,

    /// Test every model this deployment has enabled
    #[arg(long)]
    pub all: bool,

    /// Run a backtest through chap-core instead of the model's own test
    #[arg(long)]
    pub backtest: bool,

    /// Variant name of the configured model to backtest, as in `models configs`
    #[arg(long, value_name = "NAME", requires = "backtest")]
    pub config: Option<String>,

    /// Seed for the generated data, so a run can be repeated
    #[arg(long, value_name = "N")]
    pub seed: Option<i64>,

    /// Time limit for the test of one model, in seconds
    #[arg(long, value_name = "SECONDS")]
    pub timeout: Option<u64>,

    /// Configs that `chapkit test` creates for each model
    #[arg(long, value_name = "N", conflicts_with = "backtest", value_parser = clap::value_parser!(u32).range(1..))]
    pub configs: Option<u32>,

    /// Trainings for each config
    #[arg(long, value_name = "N", conflicts_with = "backtest", value_parser = clap::value_parser!(u32).range(1..))]
    pub trainings: Option<u32>,

    /// Predictions for each trained model
    #[arg(long, value_name = "N", conflicts_with = "backtest", value_parser = clap::value_parser!(u32).range(1..))]
    pub predictions: Option<u32>,

    /// Rows of training data (locations times periods)
    #[arg(long, value_name = "N", conflicts_with = "backtest", value_parser = clap::value_parser!(u32).range(1..))]
    pub rows: Option<u32>,

    /// Rows of the history and of the future data for each prediction
    #[arg(long, value_name = "N", conflicts_with = "backtest", value_parser = clap::value_parser!(u32).range(1..))]
    pub predict_rows: Option<u32>,

    /// Keep what the test created instead of deleting it
    #[arg(long)]
    pub keep: bool,
}

/// List, add, archive and sync the configured models of each model
///
/// `args_conflicts_with_subcommands`: the bare command is `list`, so its
/// options are repeated here, as `chap-devops jobs` does.
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ConfigsArgs {
    #[command(subcommand)]
    pub command: Option<ConfigsCmd>,

    #[command(flatten)]
    pub list: ConfigsListArgs,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigsCmd {
    /// List the configured models chap-core has of each enabled model
    List(ConfigsListArgs),

    /// Add a configured model of a model to chap-core
    Add(ConfigsAddArgs),

    /// Change the values of a configured model of a model
    Update(ConfigsUpdateArgs),

    /// Archive a configured model of a model
    Archive(ConfigsArchiveArgs),

    /// Write the configured models of a model in the marketplace format
    Export(ConfigsExportArgs),

    /// Create the configured models that chap-core needs to run each model
    Sync(ConfigsSyncArgs),
}

/// List the configured models chap-core has of each enabled model
#[derive(Debug, Clone, Default, Args)]
pub struct ConfigsListArgs {
    /// Marketplace id or service id; all enabled models when none is given
    #[arg(value_name = "ID")]
    pub id: Option<String>,

    /// Show the archived configured models too
    #[arg(long)]
    pub all: bool,
}

/// Add a configured model of a model to chap-core
#[derive(Debug, Clone, Args)]
pub struct ConfigsAddArgs {
    /// Marketplace id or service id
    #[arg(value_name = "ID")]
    pub id: String,

    /// Variant name, which the Modeling App shows in brackets
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    /// Value of one user option; repeat for more than one
    #[arg(long, value_name = "KEY=VALUE")]
    pub set: Vec<String>,

    /// Additional covariates, separated by commas
    #[arg(long, value_name = "A,B")]
    pub covariates: Option<String>,

    /// Add every configuration in a file of the marketplace format
    #[arg(long, value_name = "FILE", conflicts_with_all = ["name", "set", "covariates"])]
    pub from: Option<std::path::PathBuf>,
}

/// Change the values of a configured model of a model
#[derive(Debug, Clone, Args)]
pub struct ConfigsUpdateArgs {
    /// Marketplace id or service id
    #[arg(value_name = "ID")]
    pub id: String,

    /// Variant name of the configured model
    #[arg(value_name = "NAME")]
    pub name: String,

    /// New value of one user option; repeat for more than one
    #[arg(long, value_name = "KEY=VALUE")]
    pub set: Vec<String>,

    /// Remove the value of one option, so the model default applies
    #[arg(long, value_name = "KEY")]
    pub unset: Vec<String>,

    /// Additional covariates, separated by commas, in place of the old ones
    #[arg(long, value_name = "A,B")]
    pub covariates: Option<String>,
}

/// Archive a configured model of a model
#[derive(Debug, Clone, Args)]
pub struct ConfigsArchiveArgs {
    /// Marketplace id or service id
    #[arg(value_name = "ID")]
    pub id: String,

    /// Variant name of the configured model
    #[arg(value_name = "NAME")]
    pub name: String,
}

/// Write the configured models of a model in the marketplace format
#[derive(Debug, Clone, Args)]
pub struct ConfigsExportArgs {
    /// Marketplace id or service id; may be left out with one enabled model
    #[arg(value_name = "ID")]
    pub id: Option<String>,

    /// File to write instead of standard output
    #[arg(long, value_name = "FILE")]
    pub out: Option<std::path::PathBuf>,
}

/// Create the configured models that chap-core needs to run each model
#[derive(Debug, Clone, Args)]
pub struct ConfigsSyncArgs {
    /// Marketplace ids or service ids; all enabled models when none is given
    #[arg(value_name = "ID")]
    pub ids: Vec<String>,
}

/// Add a model the marketplace does not list
#[derive(Debug, Clone, Args)]
pub struct ModelsAddArgs {
    /// GitHub repository URL, image reference, or local image name:tag
    #[arg(value_name = "SOURCE")]
    pub source: String,

    /// Identifier to record the model under, or auto for a free one
    #[arg(long, value_name = "ID|auto")]
    pub id: Option<String>,

    /// Compose service name, which must match the service's own id
    #[arg(long, value_name = "ID")]
    pub service_id: Option<String>,

    /// Name to show in the listings
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    /// Host port to publish the model on, or auto for a free one
    #[arg(long, value_name = "PORT|auto")]
    pub port: Option<PortArg>,

    /// Host address to publish the port on, e.g. 127.0.0.1 for this machine
    #[arg(long, value_name = "ADDR")]
    pub bind: Option<std::net::IpAddr>,

    /// Data directory inside the container
    #[arg(long, value_name = "PATH")]
    pub data_dir: Option<String>,

    /// User the container runs as, as user:group
    #[arg(long, value_name = "USER")]
    pub user: Option<String>,

    /// Record the image as published for amd64 only
    #[arg(long)]
    pub runtime_amd64: bool,

    /// Enable a starter even though it is not a model
    #[arg(long)]
    pub allow_starter: bool,
}

/// Create a model project from a template
#[derive(Debug, Clone, Args)]
pub struct ModelsNewArgs {
    /// Directory to create; its name becomes the model name
    #[arg(value_name = "DIR")]
    pub dir: std::path::PathBuf,

    /// The kind of project and its base image
    #[arg(long, value_name = "TYPE", default_value = "fn-py")]
    pub template: crate::scaffold::Template,

    /// Add $validate hooks to main.py (chapkit service types only)
    #[arg(long)]
    pub with_validation: bool,
}

/// Remove a model that was added with models add
#[derive(Debug, Clone, Args)]
pub struct ModelsRemoveArgs {
    /// Id of a model added with models add
    #[arg(value_name = "ID")]
    pub id: String,

    /// Delete the model's data volume as well
    #[arg(long)]
    pub purge: bool,

    /// Leave the model and its configured models live in chap-core
    #[arg(long)]
    pub keep_in_chap: bool,
}

/// Enable a model and write its compose overlay
#[derive(Debug, Clone, Args)]
pub struct ModelsEnableArgs {
    /// Marketplace ids or service ids, separated by spaces or commas
    #[arg(value_name = "ID", required = true, num_args = 1.., value_delimiter = ',')]
    pub ids: Vec<String>,

    /// Follow a release channel instead of pinning a version
    #[arg(long, value_name = "CHANNEL", conflicts_with = "version")]
    pub channel: Option<Channel>,

    /// Pin an exact version from the model's versions list
    #[arg(long, value_name = "VERSION")]
    pub version: Option<String>,

    /// Host port to publish the model on, or auto for a free one
    #[arg(long, value_name = "PORT|auto")]
    pub port: Option<PortArg>,

    /// Host address to publish the port on, e.g. 127.0.0.1 for this machine
    #[arg(long, value_name = "ADDR")]
    pub bind: Option<std::net::IpAddr>,

    /// Data directory inside the container
    #[arg(long, value_name = "PATH")]
    pub data_dir: Option<String>,

    /// User the container runs as, as user:group
    #[arg(long, value_name = "USER")]
    pub user: Option<String>,

    /// Enable a starter even though it is not a model
    #[arg(long)]
    pub allow_starter: bool,
}

/// Disable a model and remove its compose overlay
#[derive(Debug, Clone, Args)]
pub struct ModelsDisableArgs {
    /// Marketplace id or service id
    #[arg(value_name = "ID")]
    pub id: String,

    /// Delete the model's data volume as well
    #[arg(long)]
    pub purge: bool,

    /// Retire the model in chap-core too, with its configured models
    #[arg(long)]
    pub retire: bool,
}

/// Publish a host port for an enabled model
#[derive(Debug, Clone, Args)]
pub struct ModelsExposeArgs {
    /// Marketplace id or service id
    #[arg(value_name = "ID")]
    pub id: String,

    /// Host port to publish on, or auto for the lowest free one
    #[arg(long, value_name = "PORT|auto")]
    pub port: Option<PortArg>,

    /// Host address to publish the port on, e.g. 127.0.0.1 for this machine
    #[arg(long, value_name = "ADDR")]
    pub bind: Option<std::net::IpAddr>,
}

/// Remove the host port of an enabled model
#[derive(Debug, Clone, Args)]
pub struct ModelsUnexposeArgs {
    /// Marketplace id or service id
    #[arg(value_name = "ID")]
    pub id: String,
}

/// Open the browser: marketplace models and components
#[derive(Debug, Clone, Args)]
pub struct UiArgs {}

/// Inspect and refresh the marketplace registry
#[derive(Debug, Args)]
pub struct RegistryArgs {
    #[command(subcommand)]
    pub command: RegistryCmd,
}

#[derive(Debug, Subcommand)]
pub enum RegistryCmd {
    /// Fetch the registry from the network and refresh the cache
    Update(RegistryUpdateArgs),

    /// Show where the registry was loaded from and what it holds
    Show(RegistryShowArgs),
}

/// Fetch the registry from the network and refresh the cache
#[derive(Debug, Clone, Args)]
pub struct RegistryUpdateArgs {}

/// Show where the registry was loaded from and what it holds
#[derive(Debug, Clone, Args)]
pub struct RegistryShowArgs {}
