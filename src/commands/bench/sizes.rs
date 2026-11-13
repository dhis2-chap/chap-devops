//! `chap-devops bench --write --sizes 500x520w,3000x120m`: the same run once
//! for each dataset size, side by side.
//!
//! Each size runs as a child `bench` with `--json`, with the locations,
//! periods and period type of that size. With `--fresh-database`, chap-core's
//! database is emptied before each size: `chap` and `worker` stop, the
//! database is dropped and made again, and chap-core makes its tables and
//! configured models again when it starts.

use super::compare::{child, table};
use crate::api::Api;
use crate::cli::{BenchArgs, Size};
use crate::commands::Ctx;
use crate::error::{ChapError, Result};
use crate::project::Project;
use serde_json::Value;
use std::time::{Duration, Instant};

/// How long chap-core may take to answer after the database was emptied.
const READY_TIMEOUT: Duration = Duration::from_secs(180);

/// The options of a child run for `size`.
pub fn size_args(size: &Size) -> Vec<String> {
    let kind = match size.period_type {
        crate::synthetic::PeriodType::Week => "week",
        crate::synthetic::PeriodType::Month => "month",
    };
    [
        "--locations".to_string(),
        size.locations.to_string(),
        "--periods".to_string(),
        size.periods.to_string(),
        "--period-type".to_string(),
        kind.to_string(),
    ]
    .to_vec()
}

pub fn run(ctx: &Ctx, args: &BenchArgs, local: bool) -> Result<()> {
    let project = match (args.fresh_database, local) {
        (false, _) => None,
        (true, true) => Some(ctx.project()?),
        (true, false) => {
            return Err(ChapError::Usage(
                "--fresh-database empties the database of this deployment's chap-core, so it \
                 cannot be used with --url; leave out --url, or --fresh-database"
                    .into(),
            )
            .into());
        }
    };
    let mut runs: Vec<(Size, Value)> = Vec::new();
    let mut failed = None;
    for size in &args.sizes {
        if let Some(project) = &project
            && let Err(e) = fresh_database(ctx, project)
        {
            failed = Some(format!("could not empty the database before {size}: {e}"));
            break;
        }
        crate::output::notice(&format!("running size {size}"));
        match child(&size_args(size)) {
            Ok(value) => runs.push((*size, value)),
            Err(why) => {
                failed = Some(format!(
                    "the run of size {size} failed: {why}; run it alone with `chap-devops bench --write` \
                     and the same options"
                ));
                break;
            }
        }
        if crate::interrupt::requested() {
            break;
        }
    }

    if !ctx.out.json && !runs.is_empty() {
        let headers: Vec<String> = runs.iter().map(|(s, _)| s.to_string()).collect();
        let values: Vec<Value> = runs.iter().map(|(_, v)| v.clone()).collect();
        print!("{}", table(ctx, &headers, &values));
    }
    let value = serde_json::json!({
        "fresh_database": args.fresh_database,
        "runs": runs.iter().map(|(size, value)| serde_json::json!({
            "size": size.to_string(),
            "locations": size.locations,
            "periods": size.periods,
            "result": value,
        })).collect::<Vec<_>>(),
    });
    ctx.out.report(&value, |lines| {
        let done: Vec<String> = runs.iter().map(|(s, _)| s.to_string()).collect();
        lines.info(format!(
            "ran {} of {} sizes ({}){}",
            runs.len(),
            args.sizes.len(),
            done.join(", "),
            if args.fresh_database {
                ", each on an empty database"
            } else {
                ""
            }
        ));
        if let Some(why) = &failed {
            lines.warning(why.as_str());
        }
    })?;
    if failed.is_some() {
        std::process::exit(1);
    }
    Ok(())
}

/// Empty chap-core's database: stop `chap` and `worker`, drop the database
/// and make it again, start them, and wait until chap-core answers.
fn fresh_database(ctx: &Ctx, project: &Project) -> Result<()> {
    crate::output::notice("emptying chap-core's database");
    let compose = |args: &[&str]| -> Result<()> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        match crate::docker::compose_output(project, &args)? {
            (0, _, _) => Ok(()),
            (_, _, err) => Err(anyhow::anyhow!(
                "`docker compose {}` failed: {}",
                args.join(" "),
                err.lines().last().unwrap_or("no reason given")
            )),
        }
    };
    compose(&["stop", "chap", "worker"])?;
    compose(&[
        "exec",
        "-T",
        "postgres",
        "sh",
        "-c",
        "psql -U \"$POSTGRES_USER\" -d postgres -v ON_ERROR_STOP=1 \
         -c \"DROP DATABASE \\\"$POSTGRES_DB\\\" WITH (FORCE)\" \
         -c \"CREATE DATABASE \\\"$POSTGRES_DB\\\"\"",
    ])?;
    compose(&["start", "chap", "worker"])?;
    let api = Api::new(
        &project.api_url(),
        crate::api::token_for(Some(&project.dir)),
        Duration::from_secs(5),
    );
    let started = Instant::now();
    ctx.out.verbose("waiting for chap-core to answer");
    loop {
        if matches!(api.send("GET", "/health", None), Ok(a) if a.is_success()) {
            return Ok(());
        }
        if started.elapsed() >= READY_TIMEOUT {
            return Err(anyhow::anyhow!(
                "chap-core did not answer {} s after its database was emptied; \
                 `chap-devops logs chap` shows why",
                READY_TIMEOUT.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
