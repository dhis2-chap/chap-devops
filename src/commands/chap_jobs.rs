//! The chap-core jobs a command starts and waits for: post the request,
//! poll the job, read what it wrote or why it failed, and delete the rows
//! it made. `models test --backtest` and `bench --write` use them.

use crate::api::Api;
use crate::commands::Ctx;
use crate::commands::modeltest::first_line;
use crate::error::Result;
use crate::jobs;
use crate::modeltest;
use crate::output::Message;
use std::time::{Duration, Instant};

mod baselines;

/// `POST path` with a JSON body, where a non-2xx is an error.
pub(crate) fn post(api: &Api, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    let text = serde_json::to_string(body)?;
    let answer = api.send_json("POST", path, Some(&text))?;
    if !answer.is_success() {
        return Err(api.status_error(path, &answer));
    }
    answer
        .json()
        .ok_or_else(|| anyhow::anyhow!("{} did not answer with JSON", api.url(path)))
}

/// Poll one job until it is no longer running.
///
/// `Ok(None)` when the deadline passed first, which leaves the job running:
/// chap-core is doing the work and stopping it is the operator's call, so the
/// caller names the job rather than cancelling it.
pub(crate) fn wait(
    ctx: &Ctx,
    api: &Api,
    job: &str,
    every: Duration,
    until: Instant,
) -> Result<Option<String>> {
    loop {
        let status = job_status(api, job)?;
        if !jobs::Outcome::of(&status).is_running() {
            return Ok(Some(status));
        }
        if Instant::now() + every >= until {
            return Ok(None);
        }
        ctx.out.verbose(&format!("job {job} {status}"));
        std::thread::sleep(every);
    }
}

/// The status from `GET /v1/jobs/{id}`.
pub(crate) fn job_status(api: &Api, job: &str) -> Result<String> {
    let path = format!("{}/{}", jobs::JOBS_PATH, crate::api::encode(job));
    let answer = api.send("GET", &path, None)?;
    if !answer.is_success() {
        return Err(api.status_error(&path, &answer));
    }
    Ok(jobs::status_in(&answer.text()))
}

/// The row a finished job wrote, from `GET /v1/jobs/{id}/database_result`.
pub(crate) fn database_result(api: &Api, job: &str) -> Option<i64> {
    let path = format!(
        "{}/{}/database_result",
        jobs::JOBS_PATH,
        crate::api::encode(job)
    );
    let answer = api.send("GET", &path, None).ok()?;
    if !answer.is_success() {
        return None;
    }
    answer.json()?.get("id")?.as_i64()
}

/// Why a failed job failed, and what to run about it.
///
/// The job description carries a status and nothing else, so the reason is in
/// the log; the rule for picking the line out of it is the one
/// `chap-devops jobs logs` already uses.
pub(crate) fn job_failure(api: &Api, job: &str) -> (String, String) {
    let next = format!("run `chap-devops jobs logs {job}`");
    let path = format!("{}/{}/logs", jobs::JOBS_PATH, crate::api::encode(job));
    let text = match api.send("GET", &path, None) {
        Ok(answer) if answer.is_success() => match answer.json() {
            Some(serde_json::Value::String(text)) => text,
            _ => answer.text().into_owned(),
        },
        _ => String::new(),
    };
    // The model's own error stream first, then chap-core's record of it: a
    // job that failed before the model was reached has no stderr section at
    // all, and the traceback is then the whole of the answer.
    match jobs::stderr_hint(&text).or_else(|| modeltest::log_hint(&text)) {
        Some(hint) => (hint, next),
        None => (
            "the job failed and its log says nothing more".to_string(),
            next,
        ),
    }
}

/// Delete the rows this run created, and nothing else. Returns what was kept
/// or left behind, for the closing lines.
///
/// The rows go in the order chap-core accepts: the backtests of the
/// baselines and of the model first, then the dataset they point at. Each
/// backtest is one request: chap-core deletes the forecasts of a backtest
/// one row at a time, and a batch of large backtests runs past the API
/// worker's timeout (`BUGS.md`).
pub(crate) fn drop_rows(
    ctx: &Ctx,
    api: &Api,
    made: &[i64],
    dataset: Option<i64>,
    dataset_job: &str,
    keep: bool,
) -> Vec<Message> {
    let mut backtests = dataset
        .map(|dataset| baselines::rows(ctx, api, dataset, dataset_job))
        .unwrap_or_default();
    backtests.extend_from_slice(made);
    backtests.sort_unstable();
    backtests.dedup();
    let mut lines = Vec::new();
    if keep {
        if let Some(line) = kept_line(&backtests, dataset) {
            lines.push(Message::info(line));
        }
        return lines;
    }
    for path in row_paths(&backtests, dataset) {
        let command = format!("`chap-devops api DELETE {path}`");
        match api.send("DELETE", &path, None) {
            Ok(answer) if answer.is_success() => ctx.out.verbose(&format!("deleted {path}")),
            Ok(answer) => lines.push(Message::warning(format!(
                "could not delete {path} ({}); remove it with {command}",
                answer.status_line()
            ))),
            Err(err) => lines.push(Message::warning(format!(
                "could not delete {path} ({}); remove it with {command}",
                first_line(&err.to_string())
            ))),
        }
    }
    lines
}

/// What to delete, in order: each backtest, then the dataset.
fn row_paths(backtests: &[i64], dataset: Option<i64>) -> Vec<String> {
    backtests
        .iter()
        .map(|id| format!("/v1/crud/backtests/{id}"))
        .chain(dataset.map(|id| format!("/v1/crud/datasets/{id}")))
        .collect()
}

/// The line for `--keep`: what was kept, and the commands that remove it.
fn kept_line(backtests: &[i64], dataset: Option<i64>) -> Option<String> {
    let ids: Vec<String> = backtests.iter().map(i64::to_string).collect();
    let dataset_command = |d: i64| format!("`chap-devops api DELETE /v1/crud/datasets/{d}`");
    Some(match (ids.as_slice(), dataset) {
        ([], None) => return None,
        ([], Some(d)) => format!("kept dataset {d}; remove it with {}", dataset_command(d)),
        ([one], None) => format!(
            "kept backtest {one}; remove it with `chap-devops api DELETE /v1/crud/backtests/{one}`"
        ),
        ([one], Some(d)) => format!(
            "kept backtest {one} and dataset {d}; remove them with \
             `chap-devops api DELETE /v1/crud/backtests/{one}` then {}",
            dataset_command(d)
        ),
        (several, None) => format!(
            "kept backtests {}; remove each with `chap-devops api DELETE /v1/crud/backtests/ID`",
            and_list(several)
        ),
        (several, Some(d)) => format!(
            "kept backtests {} and dataset {d}; remove each backtest with \
             `chap-devops api DELETE /v1/crud/backtests/ID`, then the dataset with {}",
            and_list(several),
            dataset_command(d)
        ),
    })
}

/// `a`, `a and b`, `a, b and c`.
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests;
