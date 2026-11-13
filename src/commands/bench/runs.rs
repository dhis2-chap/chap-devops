//! The backtests of `chap-devops bench --write`, as the Modeling App's
//! evaluation and benchmark pages start them: one `create-backtests`
//! request for each run, with every model under one specification. Each
//! backtest job queues the baselines its specification does not have yet
//! (`run_baseline` jobs); the run waits for those too, because the benchmark
//! page is only complete when they end.
//!
//! The run times are the jobs' own, from `start_time` and `end_time` in
//! `/v1/jobs`, not the polls: a poll every 3 s cannot time a job of 2 s.

use super::write::{Backtest, POLL};
use crate::api::Api;
use crate::commands::Ctx;
use crate::commands::chap_jobs::{job_status, post};
use crate::commands::modeltest::first_line;
use crate::jobs::{Job, Outcome};
use serde_json::Value;
use std::collections::BTreeSet;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The job type of a baseline run.
pub const BASELINE_JOB: &str = "run_baseline";

/// Seconds since the Unix epoch, now.
fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default()
}

/// The ids of the baseline jobs chap-core lists now, so that the run can
/// tell its own from the ones before it.
pub fn baseline_jobs(api: &Api) -> BTreeSet<String> {
    list(api, &format!("/v1/jobs?type={BASELINE_JOB}"))
        .into_iter()
        .map(|j| j.id)
        .collect()
}

fn list(api: &Api, path: &str) -> Vec<Job> {
    api.get_json(path)
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// Start `runs` runs of `models` (id and name) on `dataset`, one
/// `create-backtests` request each, and give a row for each job.
pub fn start(
    api: &Api,
    models: &[(i64, String)],
    dataset: i64,
    stamp: &str,
    runs: u32,
) -> Vec<Backtest> {
    let mut rows = Vec::new();
    for number in 1..=runs {
        let body = serde_json::json!({
            "name": format!("bench-{stamp}-{number}"),
            "modelIds": models.iter().map(|(_, name)| name.as_str()).collect::<Vec<_>>(),
            "datasetId": dataset,
        });
        let sent = Instant::now();
        let answer = post(api, "/v1/analytics/create-backtests", &body);
        let request_seconds = sent.elapsed().as_secs_f64();
        let answered_at = unix_now();
        match answer {
            Ok(answer) => {
                let jobs = answer
                    .get("jobs")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if jobs.is_empty() {
                    rows.push(Backtest {
                        number,
                        model: models
                            .iter()
                            .map(|(_, n)| n.as_str())
                            .collect::<Vec<_>>()
                            .join(","),
                        status: "not started".to_string(),
                        request_seconds: Some(request_seconds),
                        problem: Some("chap-core gave no job ids".to_string()),
                        ..Backtest::default()
                    });
                }
                for job in jobs {
                    let model_id = job.get("configuredModelId").and_then(Value::as_i64);
                    let model = models
                        .iter()
                        .find(|(id, _)| Some(*id) == model_id)
                        .map(|(_, name)| name.clone())
                        .unwrap_or_else(|| model_id.map(|id| id.to_string()).unwrap_or_default());
                    let id = job.get("jobId").and_then(Value::as_str).map(str::to_string);
                    rows.push(Backtest {
                        number,
                        model,
                        status: "PENDING".to_string(),
                        problem: id.is_none().then(|| "chap-core gave no job id".to_string()),
                        job: id,
                        request_seconds: Some(request_seconds),
                        answered_at: Some(answered_at),
                        ..Backtest::default()
                    });
                }
            }
            Err(e) => rows.push(Backtest {
                number,
                model: models
                    .iter()
                    .map(|(_, n)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                status: "not started".to_string(),
                request_seconds: Some(request_seconds),
                problem: Some(first_line(&e.to_string())),
                ..Backtest::default()
            }),
        }
    }
    rows
}

/// Poll every job until it ends, adding the baseline jobs that were not in
/// `before` as they appear. Returns whether Ctrl-C stopped the wait.
pub fn wait(
    ctx: &Ctx,
    api: &Api,
    rows: &mut Vec<Backtest>,
    before: &BTreeSet<String>,
    until: Instant,
) -> bool {
    let started_at = unix_now();
    loop {
        for job in list(api, &format!("/v1/jobs?type={BASELINE_JOB}")) {
            let ours = !before.contains(&job.id)
                && !rows
                    .iter()
                    .any(|r| r.job.as_deref() == Some(job.id.as_str()));
            if ours {
                rows.push(Backtest {
                    number: 0,
                    model: job
                        .name
                        .strip_prefix("baseline/")
                        .unwrap_or(&job.name)
                        .to_string(),
                    baseline: true,
                    status: job.status.clone(),
                    job: Some(job.id.clone()),
                    answered_at: Some(started_at),
                    ..Backtest::default()
                });
            }
        }
        let mut running = 0;
        for row in rows.iter_mut().filter(|r| r.job.is_some() && !r.ended) {
            let job = row.job.clone().unwrap_or_default();
            match job_status(api, &job) {
                Ok(status) => {
                    row.ended = !Outcome::of(&status).is_running();
                    row.status = status;
                    if !row.ended {
                        running += 1;
                    }
                }
                Err(e) => {
                    ctx.out
                        .verbose(&format!("job {job}: {}", first_line(&e.to_string())));
                    running += 1;
                }
            }
        }
        if running == 0 {
            return false;
        }
        if crate::interrupt::requested() {
            return true;
        }
        if Instant::now() + POLL >= until {
            return false;
        }
        ctx.out.verbose(&format!("{running} jobs still running"));
        std::thread::sleep(POLL);
    }
}

/// Fill in each job's run time and its time in the queue, from the job list.
pub fn times(api: &Api, rows: &mut [Backtest]) {
    let ids: Vec<String> = rows
        .iter()
        .filter_map(|r| r.job.as_deref())
        .map(|id| format!("ids={}", crate::api::encode(id)))
        .collect();
    if ids.is_empty() {
        return;
    }
    let jobs = list(api, &format!("/v1/jobs?{}", ids.join("&")));
    for row in rows.iter_mut() {
        let Some(job) = jobs
            .iter()
            .find(|j| Some(j.id.as_str()) == row.job.as_deref())
        else {
            continue;
        };
        let (start, end) = job_times(job);
        if let (Some(start), Some(end)) = (start, end) {
            row.seconds = Some((end - start).max(0.0));
        }
        if let (Some(start), Some(answered)) = (start, row.answered_at)
            && !row.baseline
        {
            row.queued_seconds = Some((start - answered).max(0.0));
        }
    }
}

/// A job's start and end, in seconds since the Unix epoch.
pub fn job_times(job: &Job) -> (Option<f64>, Option<f64>) {
    let at = |t: &Option<String>| t.as_deref().and_then(crate::status::parse_rfc3339_precise);
    (at(&job.start_time), at(&job.end_time))
}
