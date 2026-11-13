//! `chap-devops bench` and `chap-devops bench dataset`: load on chap-core,
//! and the synthetic data for it.

use crate::synthetic::{Format, PeriodType, Spec};
use clap::{Args, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Time the Modeling App's pages, or run backtests under load
#[derive(Debug, Clone, Args)]
#[command(args_conflicts_with_subcommands = true, disable_help_subcommand = true)]
pub struct BenchArgs {
    #[command(subcommand)]
    pub command: Option<BenchSub>,

    /// Backtest id to time; the newest backtest when not given
    #[arg(value_name = "BACKTEST", conflicts_with = "write")]
    pub backtest: Option<i64>,

    /// Pages to time, comma-separated; all pages when not given
    #[arg(
        long,
        value_name = "PAGE",
        value_enum,
        value_delimiter = ',',
        conflicts_with = "write"
    )]
    pub page: Vec<BenchPage>,

    /// Prediction id for the prediction page; the newest when not given
    #[arg(long, value_name = "ID", conflicts_with = "write")]
    pub prediction: Option<i64>,

    /// Run once for each API worker count, comma-separated, and compare
    #[arg(long, value_name = "N,N", value_delimiter = ',', value_parser = clap::value_parser!(u32).range(1..=64))]
    pub workers: Vec<u32>,

    /// How many times to send each request
    #[arg(long, value_name = "N", default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=1000))]
    pub repeat: u32,

    /// How many requests to have open at the same time
    #[arg(long, value_name = "N", default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=64))]
    pub parallel: u32,

    /// Base URL of the chap-core API
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// Request timeout in seconds
    #[arg(long, value_name = "SECONDS", default_value_t = 120)]
    pub timeout: u64,

    /// Import a dataset and run backtests on it, instead of timing reads
    #[arg(long, help_heading = "Write options")]
    pub write: bool,

    /// Configured models to backtest, by name or id, comma-separated
    #[arg(
        long,
        value_name = "MODEL",
        requires = "write",
        help_heading = "Write options"
    )]
    pub model: Option<String>,

    /// How many runs of the models to start at the same time
    #[arg(long, value_name = "N", default_value_t = 4, value_parser = clap::value_parser!(u32).range(1..=64), help_heading = "Write options")]
    pub backtests: u32,

    /// Backtest this dataset id instead of a new synthetic one
    #[arg(
        long,
        value_name = "ID",
        requires = "write",
        help_heading = "Write options"
    )]
    pub dataset: Option<i64>,

    /// Seconds to wait for the jobs to finish
    #[arg(long, value_name = "SECONDS", default_value_t = 3600, value_parser = clap::value_parser!(u64).range(1..), help_heading = "Write options")]
    pub wait: u64,

    /// Also make a prediction from the first backtest that passed
    #[arg(
        long,
        requires = "write",
        conflicts_with = "dataset",
        help_heading = "Write options"
    )]
    pub predict: bool,

    /// Keep the dataset and the backtests that the run made
    #[arg(long, requires = "write", help_heading = "Write options")]
    pub keep: bool,

    #[command(flatten)]
    pub synthetic: SyntheticArgs,
}

/// A Modeling App page that `chap-devops bench` times.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BenchPage {
    // One evaluation, as it opens
    Evaluation,
    // The comparison of an evaluation, with 10 org units
    Compare,
    // The custom plots of an evaluation, after a person picks a plot
    Plots,
    // The benchmark of the backtest specification
    Benchmark,
    // The dataset of the backtest, with every observation
    Dataset,
    // One prediction, with the cases of all its org units
    Prediction,
}

impl BenchPage {
    pub const ALL: [BenchPage; 6] = [
        BenchPage::Evaluation,
        BenchPage::Compare,
        BenchPage::Plots,
        BenchPage::Benchmark,
        BenchPage::Dataset,
        BenchPage::Prediction,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BenchPage::Evaluation => "evaluation",
            BenchPage::Compare => "compare",
            BenchPage::Plots => "plots",
            BenchPage::Benchmark => "benchmark",
            BenchPage::Dataset => "dataset",
            BenchPage::Prediction => "prediction",
        }
    }
}

/// The shape of a synthetic dataset.
#[derive(Debug, Clone, Args)]
#[command(next_help_heading = "Dataset options")]
pub struct SyntheticArgs {
    /// Number of locations (org units)
    #[arg(long, value_name = "N", default_value_t = 18, value_parser = clap::value_parser!(u64).range(1..=100_000))]
    pub locations: u64,

    /// Number of periods for each location
    #[arg(long, value_name = "N", default_value_t = 104, value_parser = clap::value_parser!(u64).range(2..=10_000))]
    pub periods: u64,

    /// Length of one period
    #[arg(long, value_name = "TYPE", value_enum, default_value_t = PeriodType::Week)]
    pub period_type: PeriodType,

    /// Typical number of cases in one location and period
    #[arg(long, value_name = "N", default_value_t = 100, value_parser = clap::value_parser!(u64).range(0..=1_000_000_000))]
    pub cases: u64,

    /// Number of covariates: rainfall, mean_temperature, population, then more
    #[arg(long, value_name = "N", default_value_t = 3, value_parser = clap::value_parser!(u64).range(0..=100))]
    pub covariates: u64,

    /// How to send the observations: rows as the Modeling App, or columns
    #[arg(long, value_name = "FORMAT", value_enum, default_value_t = Format::Rows)]
    pub format: Format,

    /// Seed for the random values; the same seed gives the same dataset
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub seed: u64,
}

impl SyntheticArgs {
    pub fn spec(&self) -> Spec {
        Spec {
            locations: self.locations as usize,
            periods: self.periods as usize,
            period_type: self.period_type,
            cases: self.cases,
            covariates: self.covariates as usize,
            seed: self.seed,
        }
    }
}

#[derive(Debug, Clone, Subcommand)]
pub enum BenchSub {
    /// Write a synthetic dataset as CSV and GeoJSON, or import it into chap-core
    Dataset(DatasetArgs),
}

/// Write a synthetic dataset as CSV and GeoJSON, or import it into chap-core
#[derive(Debug, Clone, Args)]
pub struct DatasetArgs {
    /// Directory for the CSV and GeoJSON files; `.` when there is no --import
    #[arg(long, value_name = "DIR")]
    pub out: Option<PathBuf>,

    /// Import the dataset into chap-core and wait until it is stored
    #[arg(long = "import")]
    pub import: bool,

    /// Name of the dataset and of its files; made from the options if not given
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    /// Base URL of the chap-core API to import into
    #[arg(long, value_name = "URL", requires = "import")]
    pub url: Option<String>,

    /// Seconds to wait for chap-core to store the dataset
    #[arg(long, value_name = "SECONDS", default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..))]
    pub timeout: u64,

    #[command(flatten)]
    pub synthetic: SyntheticArgs,
}
