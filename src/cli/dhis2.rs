//! `chap-devops dhis2`: connecting DHIS2 and this deployment's Chap.

use clap::{Args, Subcommand};

/// Let the DHIS2 Modeling App reach this deployment's Chap
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct Dhis2Args {
    #[command(subcommand)]
    pub command: Dhis2Sub,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Dhis2Sub {
    /// Say what DHIS2 has: the chap route, analytics and the apps
    Show(Dhis2ShowArgs),

    /// Point DHIS2's chap route at this deployment's chap-core
    Route(Dhis2RouteArgs),

    /// Generate DHIS2's analytics tables and wait for them
    Analytics(Dhis2AnalyticsArgs),

    /// Install the Modeling App and the Climate App in DHIS2
    Apps(Dhis2AppsArgs),

    /// The route, the apps, then analytics; the route only on an external DHIS2
    Connect(Dhis2ConnectArgs),

    /// Use a DHIS2 that runs elsewhere, or show which DHIS2 is used
    Use(Dhis2UseArgs),
}

/// The two things every verb in the group needs.
///
/// Flattened rather than repeated, so the user and the wait are spelled and
/// documented once. There is deliberately no `--password` or `--token`: either
/// would sit in the shell history and in `ps`, and `.env` or the `CHAP_DEVOPS_DHIS2_*`
/// variables are where they belong.
#[derive(Debug, Clone, Args)]
pub struct Dhis2CommonArgs {
    /// DHIS2 user to authenticate as, with a password rather than a token
    #[arg(long, value_name = "NAME")]
    pub user: Option<String>,

    /// Seconds to wait for DHIS2's API; 1200 local, 60 external by default
    #[arg(long, value_name = "SECONDS")]
    pub wait: Option<u64>,
}

/// Say what DHIS2 has: the chap route, analytics and the apps
#[derive(Debug, Clone, Args)]
pub struct Dhis2ShowArgs {
    #[command(flatten)]
    pub common: Dhis2CommonArgs,
}

/// Point DHIS2's chap route at this deployment's chap-core
#[derive(Debug, Clone, Args)]
pub struct Dhis2RouteArgs {
    #[command(flatten)]
    pub common: Dhis2CommonArgs,
}

/// Generate DHIS2's analytics tables and wait for them
#[derive(Debug, Clone, Args)]
pub struct Dhis2AnalyticsArgs {
    #[command(flatten)]
    pub common: Dhis2CommonArgs,

    /// Seconds to wait for the analytics run to finish
    #[arg(long, value_name = "SECONDS", default_value_t = crate::dhis2::DEFAULT_ANALYTICS_TIMEOUT)]
    pub timeout: u64,

    /// Start the run and return without a wait for it to finish
    #[arg(long)]
    pub no_wait: bool,
}

/// Install the Modeling App and the Climate App in DHIS2
#[derive(Debug, Clone, Args)]
pub struct Dhis2AppsArgs {
    #[command(flatten)]
    pub common: Dhis2CommonArgs,

    /// Modeling App to install: stable, unstable, or a version such as 7.5.0
    #[arg(
        long = "modeling-app",
        value_name = "WHICH",
        value_parser = crate::components::ModelingApp::parse
    )]
    pub modeling_app: Option<crate::components::ModelingApp>,
}

/// The route, the apps, then analytics; the route only on an external DHIS2
#[derive(Debug, Clone, Args)]
pub struct Dhis2ConnectArgs {
    #[command(flatten)]
    pub common: Dhis2CommonArgs,

    /// Modeling App to install: stable, unstable, or a version such as 7.5.0
    #[arg(
        long = "modeling-app",
        value_name = "WHICH",
        value_parser = crate::components::ModelingApp::parse
    )]
    pub modeling_app: Option<crate::components::ModelingApp>,

    /// Seconds to wait for the analytics run to finish
    #[arg(long, value_name = "SECONDS", default_value_t = crate::dhis2::DEFAULT_ANALYTICS_TIMEOUT)]
    pub timeout: u64,

    /// Start the run and return without a wait for it to finish
    #[arg(long)]
    pub no_wait: bool,
}

/// Use a DHIS2 that runs elsewhere, or show which DHIS2 is used
#[derive(Debug, Clone, Args)]
pub struct Dhis2UseArgs {
    /// DHIS2's URL, as this machine reaches it
    #[arg(value_name = "URL", conflicts_with = "clear")]
    pub url: Option<String>,

    /// chap-core's URL, as that DHIS2 reaches it; the route points here
    #[arg(long, value_name = "URL", conflicts_with = "clear")]
    pub chap_url: Option<String>,

    /// Forget the external DHIS2 and use the dhis2 component again
    #[arg(long)]
    pub clear: bool,
}
