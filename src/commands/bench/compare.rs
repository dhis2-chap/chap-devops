//! `chap-devops bench --workers 1,4`: the same measurement once for each
//! API worker count, side by side. The child runs and the table are shared
//! with `--sizes` (`sizes.rs`).
//!
//! Each count is set with the code of `restart --api-workers`, which
//! recreates only the `chap` service. Then the same `bench` command runs as
//! a child process with `--json`, so a run here is exactly a run by hand.
//! At the end the deployment gets back the count it had.

use crate::cli::BenchArgs;
use crate::commands::Ctx;
use crate::error::{ChapError, Result};
use serde_json::Value;

use super::measure::human_ms;

/// The options a series sets for each of its runs, with a value.
const SERIES_VALUES: &[&str] = &[
    "--workers",
    "--sizes",
    "--locations",
    "--periods",
    "--period-type",
];

/// The options a series sets for each of its runs, without a value.
const SERIES_FLAGS: &[&str] = &["--fresh-database", "--json"];

/// The arguments of this process without the options of the series, and
/// with `--json`.
pub fn child_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if SERIES_VALUES.contains(&arg.as_str()) {
            skip = true;
            continue;
        }
        let with_value = SERIES_VALUES
            .iter()
            .any(|o| arg.starts_with(&format!("{o}=")));
        if with_value || SERIES_FLAGS.contains(&arg.as_str()) {
            continue;
        }
        out.push(arg.clone());
    }
    out.push("--json".to_string());
    out
}

/// Run this `bench` again with `extra`, and give its `--json` result.
pub fn child(extra: &[String]) -> std::result::Result<Value, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("could not find this chap-devops binary to run again: {e}"))?;
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut args = child_args(&argv);
    args.extend_from_slice(extra);
    let out = std::process::Command::new(&exe)
        .args(&args)
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("could not run `chap-devops bench` again: {e}"))?;
    serde_json::from_slice::<Value>(&out.stdout).map_err(|_| {
        format!(
            "it gave no result (exit code {})",
            out.status.code().unwrap_or(-1)
        )
    })
}

pub fn run(ctx: &Ctx, args: &BenchArgs, local: bool) -> Result<()> {
    if !local {
        return Err(ChapError::Usage(
            "--workers changes the API workers of this deployment's chap-core, so it cannot be used \
             with --url; leave out --url, or --workers"
                .into(),
        )
        .into());
    }
    let project = ctx.project()?;
    for &count in &args.workers {
        super::super::workers::check(&project, count)?;
    }
    let before = super::super::workers::configured(&project);

    let mut runs: Vec<(u32, usize, Value)> = Vec::new();
    let mut failed = None;
    for &count in &args.workers {
        let running = match super::super::workers::set_workers(ctx, &project, count) {
            Ok(n) => n,
            Err(e) => {
                failed = Some(e);
                break;
            }
        };
        crate::output::notice(&format!(
            "timing with {running} API worker{}",
            super::super::workers::plural(running)
        ));
        match child(&[]) {
            Ok(value) => runs.push((count, running, value)),
            Err(why) => {
                failed = Some(anyhow::anyhow!(
                    "the run with {count} API workers failed: {why}; run it alone with \
                     `chap-devops restart --api-workers {count}` and `chap-devops bench`"
                ));
                break;
            }
        }
        if crate::interrupt::requested() {
            break;
        }
    }
    let restored = super::super::workers::set_workers(ctx, &project, before);

    if !ctx.out.json && !runs.is_empty() {
        let headers: Vec<String> = runs.iter().map(|(c, ..)| format!("{c} WORKERS")).collect();
        let values: Vec<Value> = runs.iter().map(|(.., v)| v.clone()).collect();
        print!("{}", table(ctx, &headers, &values));
    }
    let value = serde_json::json!({
        "runs": runs.iter().map(|(count, running, value)| serde_json::json!({
            "api_workers": count, "running": running, "result": value,
        })).collect::<Vec<_>>(),
        "restored_api_workers": before,
    });
    ctx.out.report(&value, |lines| {
        let counts: Vec<String> = runs.iter().map(|(c, ..)| c.to_string()).collect();
        lines.info(format!(
            "ran the same bench with {} of {} API worker counts ({}); set back to {before}",
            runs.len(),
            args.workers.len(),
            counts.join(", ")
        ));
        if let Some(e) = &failed {
            lines.warning(e.to_string());
        }
        if let Err(e) = &restored {
            lines.warning(format!(
                "could not set the API workers back to {before}: {e}; run `chap-devops restart --api-workers {before}`"
            ));
        }
    })?;
    if failed.is_some() {
        std::process::exit(1);
    }
    Ok(())
}

/// One row for each number the runs share, one column for each run.
pub fn rows(runs: &[Value]) -> Vec<(String, Vec<Option<f64>>)> {
    let mut rows: Vec<(String, Vec<Option<f64>>)> = Vec::new();
    let mut add = |label: String, at: usize, ms: Option<f64>| match rows
        .iter_mut()
        .find(|(l, _)| *l == label)
    {
        Some((_, cells)) => cells[at] = ms,
        None => {
            let mut cells = vec![None; runs.len()];
            cells[at] = ms;
            rows.push((label, cells));
        }
    };
    for (at, value) in runs.iter().enumerate() {
        for page in value["pages"].as_array().into_iter().flatten() {
            let name = page["page"].as_str().unwrap_or_default();
            add(format!("{name} page"), at, page["ms"].as_f64());
        }
        for route in value["routes"].as_array().into_iter().flatten() {
            let label = format!(
                "{} {} p50",
                route["page"].as_str().unwrap_or_default(),
                route["label"].as_str().unwrap_or_default()
            );
            add(label, at, route["p50_ms"].as_f64());
        }
        let secs = |key: &str| value[key].as_f64().map(|s| s * 1000.0);
        if value.get("backtests").is_some() {
            add("import".to_string(), at, secs("import_seconds"));
            add(
                "import request".to_string(),
                at,
                secs("import_request_seconds"),
            );
            add(
                "backtest request".to_string(),
                at,
                secs("backtest_request_seconds"),
            );
            let slowest = value["backtests"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| b["seconds"].as_f64())
                .fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s))));
            add("slowest job".to_string(), at, slowest.map(|s| s * 1000.0));
        }
        add("whole run".to_string(), at, secs("seconds"));
    }
    rows
}

/// The rows of [`rows`] under `headers`, one for each run.
pub fn table(ctx: &Ctx, headers: &[String], runs: &[Value]) -> String {
    let mut header = vec!["MEASURE".to_string()];
    header.extend(headers.iter().cloned());
    let header: Vec<&str> = header.iter().map(String::as_str).collect();
    let body: Vec<Vec<String>> = rows(runs)
        .into_iter()
        .map(|(label, cells)| {
            let mut row = vec![label];
            row.extend(
                cells
                    .into_iter()
                    .map(|ms| ms.map(human_ms).unwrap_or_else(|| "-".to_string())),
            );
            row
        })
        .collect();
    ctx.out.table(&header, &body)
}
