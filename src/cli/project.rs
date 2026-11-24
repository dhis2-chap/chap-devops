//! `chap-devops init` and `chap-devops components`: what a deployment is made of.

use super::ComponentPortArg;
use crate::project::DEFAULT_API_PORT;
use clap::{Args, Subcommand};
use std::path::PathBuf;

/// Create a deployment directory: compose files, .env and .chap-devops/
#[derive(Debug, Clone, Args)]
pub struct InitArgs {
    /// Directory to create
    #[arg(value_name = "DIR", default_value = ".")]
    pub dir: PathBuf,

    /// Models to enable: none, all, or a list of ids
    #[arg(long, value_name = "SPEC", default_value = "none")]
    pub models: String,

    /// Select the models in the browser instead of with --models
    #[arg(long)]
    pub interactive: bool,

    /// chap-core image tag: latest, master, dev or vX.Y.Z
    #[arg(long, value_name = "TAG", default_value = "latest")]
    pub chap_tag: String,

    /// Overwrite an existing deployment in the target directory
    #[arg(long)]
    pub force: bool,

    /// Host port to publish chap-core's API on
    #[arg(long, value_name = "PORT", default_value_t = DEFAULT_API_PORT)]
    pub api_port: u16,

    /// Lowest host port a model may be published on
    #[arg(long, value_name = "PORT", default_value_t = 5001)]
    pub port_base: u16,

    /// Protect the API with a token (generated when no value is given)
    #[arg(long, value_name = "TOKEN", num_args = 0..=1, conflicts_with = "no_env")]
    pub api_token: Option<Option<String>>,

    /// Do not write a .env file
    #[arg(long)]
    pub no_env: bool,

    /// Regenerate .env with a new database password
    #[arg(long, conflicts_with = "no_env")]
    pub fresh_env: bool,

    /// Build chap-core from the chap-core checkout at PATH
    #[arg(long, value_name = "PATH", conflicts_with = "chap_core_url")]
    pub source: Option<PathBuf>,

    /// Components to add: ocs, s3, dhis2
    #[arg(long = "with", value_name = "LIST")]
    pub with: Option<String>,

    /// Components to omit: chap-core, ocs, s3, dhis2
    #[arg(long = "without", value_name = "LIST")]
    pub without: Option<String>,

    /// Exactly these components and nothing else; `none` for models alone
    #[arg(long = "only", value_name = "LIST", conflicts_with_all = ["with", "without"])]
    pub only: Option<String>,

    /// Take over the chap-admin deployment in DIR, with its database
    #[arg(long, value_name = "DIR")]
    pub adopt: Option<PathBuf>,

    /// Register the models with the chap-core at URL instead of running one
    #[arg(long = "chap-core-url", value_name = "URL")]
    pub chap_core_url: Option<String>,

    /// Public URL OCS is reached at, for a proxied instance
    #[arg(long = "ocs-base-url", value_name = "URL")]
    pub ocs_base_url: Option<String>,

    /// Host port to publish OCS on, or none to keep it internal
    #[arg(long = "ocs-port", value_name = "PORT|none")]
    pub ocs_port: Option<ComponentPortArg>,

    /// Host port to publish the object store on, or none
    #[arg(long = "s3-port", value_name = "PORT|none")]
    pub s3_port: Option<ComponentPortArg>,

    /// Host port to publish DHIS2 on, or none to keep it internal
    #[arg(long = "dhis2-port", value_name = "PORT|none")]
    pub dhis2_port: Option<ComponentPortArg>,

    /// Dump to seed the DHIS2 database from: default, none, URL or path
    #[arg(long = "dhis2-seed", value_name = "SPEC")]
    pub dhis2_seed: Option<String>,

    /// Give every DHIS2 user this password (default: district)
    #[arg(
        long = "dhis2-seed-password",
        value_name = "PW",
        num_args = 0..=1,
        default_missing_value = crate::dhis2::DEFAULT_PASSWORD
    )]
    pub dhis2_seed_password: Option<String>,

    /// Modeling App to install: stable, unstable, or a version such as 7.5.0
    #[arg(
        long = "modeling-app",
        value_name = "WHICH",
        value_parser = crate::components::ModelingApp::parse
    )]
    pub modeling_app: Option<crate::components::ModelingApp>,

    /// DHIS2 version to run: an image tag such as 2.41, 2.42 or 2.43.1
    #[arg(long = "dhis2-tag", value_name = "TAG")]
    pub dhis2_tag: Option<String>,

    /// DHIS2 image repository, e.g. dhis2/core-dev for unreleased versions
    #[arg(long = "dhis2-image", value_name = "REPO")]
    pub dhis2_image: Option<String>,

    /// Refuse ingestion over HTTP on the new OCS instance
    #[arg(long = "ocs-read-only")]
    pub ocs_read_only: bool,

    #[command(flatten)]
    pub ocs: OcsConfigArgs,
}

/// The values that go into the scaffolded `ocs/climate-service.yaml`.
///
/// Shared by `init --with ocs` and `chap-devops components enable ocs`, because both
/// write that file the first time the component is turned on.
#[derive(Debug, Clone, Default, Args)]
pub struct OcsConfigArgs {
    /// Country or region the OCS instance covers, e.g. Malawi
    #[arg(long = "ocs-name", value_name = "NAME")]
    pub ocs_name: Option<String>,

    /// ISO 3166-1 alpha-3 country code for the OCS extent
    #[arg(long = "ocs-country", value_name = "CODE", value_parser = crate::compose::spec::parse_country_code)]
    pub ocs_country: Option<String>,

    // `allow_hyphen_values`: a western or southern extent starts with a minus
    // sign, and without it clap reads `--ocs-bbox -13.5,6.9,-10.1,10.0` as a
    // flag it does not know rather than as this flag's value. Only the
    // `--ocs-bbox=...` spelling would work then, which is not the spelling
    // anyone types first. `allow_negative_numbers` is the narrower version of
    // the same setting and does not help: the value is a list, not a number.
    /// OCS extent as xmin,ymin,xmax,ymax in degrees
    #[arg(
        long = "ocs-bbox",
        value_name = "BBOX",
        allow_hyphen_values = true,
        value_parser = crate::compose::spec::parse_bbox
    )]
    pub ocs_bbox: Option<String>,
}

/// Show and change what this deployment is made of
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ComponentsArgs {
    #[command(subcommand)]
    pub command: ComponentsCmd,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ComponentsCmd {
    /// List every component and whether this deployment has it
    #[command(visible_alias = "ls")]
    List(ComponentsListArgs),

    /// Enable a component, or change the settings of an enabled one
    Enable(Box<ComponentsEnableArgs>),

    /// Disable a component and remove its compose file
    Disable(ComponentsDisableArgs),
}

/// List every component and whether this deployment has it
#[derive(Debug, Clone, Args)]
pub struct ComponentsListArgs {}

/// Enable a component, or change the settings of an enabled one
#[derive(Debug, Clone, Args)]
pub struct ComponentsEnableArgs {
    /// Component name: ocs, s3, dhis2 or chap-core
    #[arg(value_name = "NAME")]
    pub name: String,

    /// Host port to publish on, or none to keep it internal
    #[arg(long, value_name = "PORT|none")]
    pub port: Option<ComponentPortArg>,

    /// Public URL OCS is reached at, for a proxied instance
    #[arg(long = "base-url", value_name = "URL")]
    pub base_url: Option<String>,

    /// Refuse ingestion over HTTP on this OCS instance
    #[arg(long = "read-only", conflicts_with = "read_write")]
    pub read_only: bool,

    /// Allow ingestion over HTTP again
    #[arg(long = "read-write")]
    pub read_write: bool,

    /// chap-core only: use the chap-core at URL instead of running one
    #[arg(long = "url", value_name = "URL")]
    pub url: Option<String>,

    /// dhis2 only: the DHIS2 version to run, an image tag such as 2.43
    #[arg(long = "tag", value_name = "TAG")]
    pub tag: Option<String>,

    /// dhis2 only: the image repository, e.g. dhis2/core-dev
    #[arg(long = "image", value_name = "REPO")]
    pub image: Option<String>,

    /// dhis2 only: dump for a new database: default, none, URL or path
    #[arg(long = "seed", value_name = "SPEC")]
    pub seed: Option<String>,

    /// With --url: the host that chap-core calls the models back at
    #[arg(long = "models-host", value_name = "HOST", requires = "url")]
    pub models_host: Option<String>,

    #[command(flatten)]
    pub ocs: OcsConfigArgs,
}

/// Disable a component and remove its compose file
#[derive(Debug, Clone, Args)]
pub struct ComponentsDisableArgs {
    /// Component name: ocs, s3, dhis2 or chap-core
    #[arg(value_name = "NAME")]
    pub name: String,

    /// Delete the component's data volume as well
    #[arg(long)]
    pub purge: bool,
}
