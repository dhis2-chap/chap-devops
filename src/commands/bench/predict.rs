//! `chap-devops bench --write --predict`: make one prediction from a backtest
//! of the run, as the Modeling App does, so that the prediction page has
//! something to show.
//!
//! The Modeling App makes a prediction through a prediction setup: a setup
//! on a backtest (`POST /v1/crud/prediction-setups`), then a run of it with
//! the observations inline, one object each
//! (`POST /v1/crud/prediction-setups/{id}/run`). chap-core stores them as a
//! new dataset that the prediction points at.

use super::write::POLL;
use crate::api::Api;
use crate::commands::Ctx;
use crate::commands::chap_jobs::{database_result, job_failure, job_status, post, post_text};
use crate::commands::modeltest::first_line;
use crate::jobs::Outcome;
use crate::output::Message;
use crate::synthetic::Dataset;
use serde::Serialize;
use serde_json::Value;
use std::time::Instant;

/// The prediction of the run, and the rows it made.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Prediction {
    pub backtest_id: i64,
    pub setup_id: Option<i64>,
    pub job: Option<String>,
    /// The last status chap-core gave, or `not started`.
    pub status: String,
    /// From the run request to the poll that saw the job end.
    pub seconds: Option<f64>,
    pub prediction_id: Option<i64>,
    /// The dataset chap-core made from the observations of the run.
    pub dataset_id: Option<i64>,
    pub entries: Option<usize>,
    pub expected_entries: Option<usize>,
    /// Why it failed; `None` when it passed.
    pub problem: Option<String>,
}

/// One entry for each org unit and forecast period, from the prediction row.
pub fn expected_entries(row: &Value) -> Option<usize> {
    let units = row.get("orgUnits")?.as_array()?.len();
    let periods = row.get("nPeriods")?.as_u64()? as usize;
    Some(units * periods)
}

/// Make the prediction from `backtest`, and wait until `until` for it.
pub fn make(
    ctx: &Ctx,
    api: &Api,
    data: &Dataset,
    backtest: i64,
    name: &str,
    until: Instant,
) -> Prediction {
    let mut made = Prediction {
        backtest_id: backtest,
        status: "not started".to_string(),
        ..Prediction::default()
    };
    let setup = serde_json::json!({"backtestId": backtest, "name": name});
    let setup = match post(api, "/v1/crud/prediction-setups", &setup) {
        Ok(answer) => answer.get("id").and_then(Value::as_i64),
        Err(e) => {
            made.problem = Some(format!(
                "chap-core did not make the prediction setup: {}",
                first_line(&e.to_string())
            ));
            return made;
        }
    };
    let Some(setup) = setup else {
        made.problem = Some("chap-core gave no prediction setup id".to_string());
        return made;
    };
    made.setup_id = Some(setup);

    crate::output::notice(&format!(
        "making a prediction from backtest {backtest} with {} observations",
        data.rows() * data.columns.len()
    ));
    let started = Instant::now();
    let path = format!("/v1/crud/prediction-setups/{setup}/run");
    let body = crate::synthetic::prediction_run_body(data, name);
    let job = match post_text(api, &path, &body) {
        Ok(answer) => answer.get("id").and_then(Value::as_str).map(str::to_string),
        Err(e) => {
            made.problem = Some(first_line(&e.to_string()));
            return made;
        }
    };
    let Some(job) = job else {
        made.problem = Some("chap-core gave no job id for the prediction".to_string());
        return made;
    };
    made.job = Some(job.clone());
    made.status = "PENDING".to_string();

    loop {
        match job_status(api, &job) {
            Ok(status) => {
                let running = Outcome::of(&status).is_running();
                made.status = status;
                if !running {
                    made.seconds = Some(started.elapsed().as_secs_f64());
                    break;
                }
            }
            Err(e) => ctx
                .out
                .verbose(&format!("job {job}: {}", first_line(&e.to_string()))),
        }
        if crate::interrupt::requested() || Instant::now() + POLL >= until {
            made.problem = Some(format!(
                "still {}; `chap-devops jobs show {job}` shows where it is",
                made.status
            ));
            return made;
        }
        std::thread::sleep(POLL);
    }

    if Outcome::of(&made.status) != Outcome::Done {
        let (why, next) = job_failure(api, &job);
        made.problem = Some(format!("{why}; {next}"));
        return made;
    }
    let Some(id) = database_result(api, &job) else {
        made.problem = Some("it finished without saying which prediction it wrote".to_string());
        return made;
    };
    made.prediction_id = Some(id);
    let row = api.get_json(&format!("/v1/crud/predictions/{id}")).ok();
    made.dataset_id = row.as_ref().and_then(|r| {
        r.get("datasetId")
            .and_then(Value::as_i64)
            .or_else(|| r.get("dataset")?.get("id")?.as_i64())
    });
    made.expected_entries = row.as_ref().and_then(expected_entries);
    made.entries = api
        .get_json(&format!(
            "/v1/analytics/prediction-entry/{id}?quantiles=0.5"
        ))
        .ok()
        .and_then(|entries| entries.as_array().map(Vec::len));
    made.problem = check(made.entries, made.expected_entries);
    made
}

/// The problem with a finished prediction, if there is one.
pub fn check(entries: Option<usize>, expected: Option<usize>) -> Option<String> {
    match (entries, expected) {
        (None, _) => Some("chap-core did not give the entries of the prediction".to_string()),
        (Some(0), _) => Some("it stored no forecasts".to_string()),
        (Some(got), Some(want)) if got != want => Some(format!(
            "it stored {got} forecasts, and its org units and periods make {want}"
        )),
        _ => None,
    }
}

/// Delete the rows of the prediction, in the order chap-core accepts: the
/// prediction, the dataset of the run, then the setup, which holds the
/// backtest. Returns what was kept or left behind.
pub fn drop_rows(ctx: &Ctx, api: &Api, made: &Prediction, keep: bool) -> Vec<Message> {
    let paths: Vec<String> = [
        made.prediction_id
            .map(|id| format!("/v1/crud/predictions/{id}")),
        made.dataset_id.map(|id| format!("/v1/crud/datasets/{id}")),
        made.setup_id
            .map(|id| format!("/v1/crud/prediction-setups/{id}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    if keep || paths.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for path in paths {
        let command = format!("`chap-devops api DELETE {path}`");
        match api.send("DELETE", &path, None) {
            Ok(answer) if answer.is_success() => ctx.out.verbose(&format!("deleted {path}")),
            Ok(answer) => lines.push(Message::warning(format!(
                "could not delete {path} ({}); remove it with {command}",
                answer.status_line()
            ))),
            Err(e) => lines.push(Message::warning(format!(
                "could not delete {path} ({}); remove it with {command}",
                first_line(&e.to_string())
            ))),
        }
    }
    lines
}

/// The closing line about the prediction.
pub fn line(made: &Prediction, kept: bool) -> Message {
    match (&made.problem, made.prediction_id) {
        (None, Some(id)) => {
            let took = made
                .seconds
                .map(|s| format!(", in {}", super::measure::human_ms(s * 1000.0)))
                .unwrap_or_default();
            let next = match kept {
                true => format!(
                    "; `chap-devops bench --page prediction --prediction {id}` times its page"
                ),
                false => String::new(),
            };
            Message::info(format!(
                "prediction {id} from backtest {} stored {} forecasts{took}{next}",
                made.backtest_id,
                made.entries.unwrap_or_default()
            ))
        }
        (problem, _) => {
            let job = made
                .job
                .as_deref()
                .map(|job| format!(" (job {job})"))
                .unwrap_or_default();
            Message::warning(format!(
                "the prediction from backtest {}{job} failed: {}",
                made.backtest_id,
                problem.as_deref().unwrap_or("it gave no prediction id")
            ))
        }
    }
}
