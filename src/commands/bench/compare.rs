//! `chap-devops bench --workers 1,4`: the same measurement once for each
//! API worker count, side by side.
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

/// The arguments of this process without `--workers`, and with `--json`.
pub fn child_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if arg == "--workers" {
            skip = true;
            continue;
        }
        if arg.starts_with("--workers=") || arg == "--json" {
            continue;
        }
        out.push(arg.clone());
    }
    out.push("--json".to_string());
    out
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
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("could not find this chap-devops binary to run again: {e}"))?;
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let child = child_args(&argv);

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
        let out = std::process::Command::new(&exe)
            .args(&child)
            .current_dir(std::env::current_dir().unwrap_or_default())
            .stderr(std::process::Stdio::inherit())
            .output()
            .map_err(|e| anyhow::anyhow!("could not run `chap-devops bench` again: {e}"))?;
        match serde_json::from_slice::<Value>(&out.stdout) {
            Ok(value) => runs.push((count, running, value)),
            Err(_) => {
                failed = Some(anyhow::anyhow!(
                    "the run with {count} API workers gave no result (exit code {}); run it alone \
                     with `chap-devops restart --api-workers {count}` and `chap-devops bench`",
                    out.status.code().unwrap_or(-1)
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
        print!("{}", table(ctx, &runs));
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

/// One row for each number the runs share, one column for each count.
pub fn rows(runs: &[(u32, usize, Value)]) -> Vec<(String, Vec<Option<f64>>)> {
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
    for (at, (_, _, value)) in runs.iter().enumerate() {
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

fn table(ctx: &Ctx, runs: &[(u32, usize, Value)]) -> String {
    let mut header = vec!["MEASURE".to_string()];
    header.extend(runs.iter().map(|(c, ..)| format!("{c} WORKERS")));
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
