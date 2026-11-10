//! The backtests of chap-core's built-in baselines on the dataset of a run.
//!
//! From chap-core v2.5.0, a backtest also queues one `run_baseline` job for
//! each built-in model, and each one writes a backtest on the same dataset.
//! chap-core leaves these out of every backtest listing, but they hold the
//! dataset: it refuses to delete a dataset that a backtest points at. The
//! job's `result` is the id of its backtest, so the jobs are where to find
//! them.

use crate::api::Api;
use crate::commands::Ctx;
use crate::jobs::{self, Job};
use std::time::{Duration, Instant};

/// The job type of a baseline run.
const BASELINE_JOB: &str = "run_baseline";

/// How long the cleanup waits for the baselines of the run to finish.
const BASELINE_WAIT: Duration = Duration::from_secs(60);

/// How often it asks while it waits.
const BASELINE_POLL: Duration = Duration::from_secs(2);

/// The baseline jobs of a run: the ones that started at or after `since`,
/// or that are still queued. Whether any of them still runs, and the
/// backtest ids of the ones that finished.
pub(super) fn of_run(list: &[Job], since: u64) -> (bool, Vec<i64>) {
    let mine: Vec<&Job> = list
        .iter()
        .filter(|job| job.kind == BASELINE_JOB)
        .filter(|job| job.started_at().is_none_or(|start| start >= since))
        .collect();
    let running = mine.iter().any(|job| job.outcome().is_running());
    let rows = mine
        .iter()
        .filter(|job| job.outcome() == jobs::Outcome::Done)
        .filter_map(|job| job.result.as_deref()?.trim().parse().ok())
        .collect();
    (running, rows)
}

/// The backtests of the baselines on `dataset`, after the baseline jobs
/// that started with the run have finished, or [`BASELINE_WAIT`] is over.
/// `dataset_job` is the job that built the dataset: its start is the start
/// of the run, on chap-core's clock. Before v2.6, `GET /v1/jobs/{id}` gives
/// a status only, so the start comes from the list.
pub(super) fn rows(ctx: &Ctx, api: &Api, dataset: i64, dataset_job: &str) -> Vec<i64> {
    let list = || {
        api.get_json(jobs::JOBS_PATH)
            .ok()
            .and_then(|value| serde_json::from_value::<Vec<Job>>(value).ok())
    };
    let Some(since) = list()
        .and_then(|jobs| jobs.into_iter().find(|job| job.id == dataset_job))
        .and_then(|job| job.started_at())
    else {
        return Vec::new();
    };
    let deadline = Instant::now() + BASELINE_WAIT;
    let found = loop {
        let Some(jobs) = list() else {
            return Vec::new();
        };
        let (running, found) = of_run(&jobs, since);
        if !running || Instant::now() >= deadline {
            break found;
        }
        ctx.out
            .verbose("waiting for the baselines of chap-core to finish");
        std::thread::sleep(BASELINE_POLL);
    };
    found
        .into_iter()
        .filter(|row| {
            api.get_json(&format!("/v1/crud/backtests/{row}"))
                .ok()
                .and_then(|backtest| backtest.get("datasetId")?.as_i64())
                == Some(dataset)
        })
        .collect()
}

#[cfg(test)]
mod tests;
