//! `chap-devops jobs` — what chap-core has been asked to compute, and how it went.
//!
//! chap-core runs everything slow on a Celery worker: building a dataset,
//! backtesting a model, making a prediction. The Modeling App starts them and
//! watches them, and until now a deployment with no browser in front of it had
//! no way to ask what it was doing. These five verbs are that way.
//!
//! The one thing the API makes awkward, and that this module exists to hide:
//! a job description carries a status and nothing else, so a `FAILURE` says
//! only that something went wrong. The message is in
//! `GET /v1/jobs/{id}/logs`, mixed into the model's own output, which is why
//! `chap-devops jobs logs` is the command the failure line points at and why it
//! digs the last error line out of the `--- stderr ---` section for a job that
//! failed.
//!
//! Everything printed here goes through [`crate::jobs`], which holds no
//! requests at all; this module holds the requests and nothing else.

use crate::api::Api;
use crate::cli::{JobsCancelArgs, JobsDeleteArgs, JobsListArgs, JobsLogsArgs, JobsShowArgs};
use crate::commands::Ctx;
use crate::error::Result;
use crate::jobs::{self, Job, Matched};
use crate::output::{self, Out};
use crate::project::Project;
use std::time::Duration;

/// `chap-devops jobs list`, and the bare `chap-devops jobs`.
pub fn list(ctx: &Ctx, args: &JobsListArgs) -> Result<()> {
    let (_project, api) = connect(ctx)?;
    let mut query: Vec<String> = args
        .status
        .iter()
        .map(|status| crate::api::query_pair("status", status.trim()))
        .collect();
    if let Some(kind) = args.kind.as_deref() {
        query.push(crate::api::query_pair("type", kind.trim()));
    }
    let (jobs, raw) = fetch(&api, &crate::api::with_query(jobs::JOBS_PATH, &query))?;

    // The order is the table's; the limit is applied after it, so `--limit 5`
    // is the five newest and not five arbitrary ones.
    let keep = args.limit.unwrap_or(jobs.len()).min(jobs.len());
    let jobs: Vec<Job> = jobs.into_iter().take(keep).collect();
    let raw: Vec<serde_json::Value> = raw.into_iter().take(keep).collect();

    let now = crate::backup::now();
    if !ctx.out.json {
        print!("{}", human_list(&jobs, now, &ctx.out));
    }
    ctx.out.report(&raw, |lines| say_list(&jobs, lines))
}

/// The table, with a blank line under it; nothing for an empty list.
fn human_list(jobs: &[Job], now: u64, out: &Out) -> String {
    if jobs.is_empty() {
        return String::new();
    }
    let mut text = out.table(jobs::HEADERS, &jobs::rows(jobs, now));
    text.push('\n');
    text
}

/// The line the table adds up to, and what to do about a failure.
fn say_list(jobs: &[Job], lines: &mut output::Report) {
    if jobs.is_empty() {
        lines.info(jobs::NO_JOBS);
        return;
    }
    lines.info(jobs::summary(jobs));
    if let Some(step) = jobs::failure_hint(jobs) {
        lines.info(step);
    }
}

/// `chap-devops jobs show ID`.
pub fn show(ctx: &Ctx, args: &JobsShowArgs) -> Result<()> {
    let (_project, api) = connect(ctx)?;
    let (id, jobs) = find(ctx, &api, &args.id)?;
    let job = jobs
        .into_iter()
        .find(|j| j.id == id)
        .unwrap_or_else(|| Job {
            id: id.clone(),
            ..Job::default()
        });

    // The list is a snapshot and the job may have moved on since; the status
    // endpoint is the current answer, and it is one request.
    let status = current_status(&api, &id)?.unwrap_or_else(|| job.status.clone());
    // Where the result landed, which is the number every `/v1/crud/` path
    // wants next. Only a finished job has one, and asking about a running job
    // is a 400 rather than an answer.
    let database_result = database_result(&api, &id, &status);

    let value = serde_json::json!({
        "id": job.id,
        "type": job.kind,
        "name": job.name,
        "status": status,
        "start_time": job.start_time,
        "end_time": job.end_time,
        "duration_seconds": job.duration().map(|d| d.as_secs()),
        "result": job.result,
        "prediction_setup_id": job.prediction_setup_id,
        "database_result_id": database_result,
    });
    let now = crate::backup::now();
    if !ctx.out.json {
        print!(
            "{}",
            human_show(&job, &status, database_result, now, &ctx.out)
        );
    }
    ctx.out.report(&value, |lines| {
        next_step(&job, &status, database_result, lines)
    })
}

/// One job as a block of fields.
fn human_show(
    job: &Job,
    status: &str,
    database_result: Option<i64>,
    now: u64,
    out: &Out,
) -> String {
    let started = match (job.start_time.as_deref(), job.started_at()) {
        (Some(stamp), Some(at)) => format!(
            "{stamp} ({})",
            crate::output::ago(Duration::from_secs(now.saturating_sub(at)))
        ),
        (Some(stamp), None) => stamp.to_string(),
        (None, _) => String::new(),
    };
    let rows = vec![
        ("Job", out.value(&job.id)),
        ("Type", job.kind.clone()),
        ("Name", job.name.clone()),
        ("Status", status_cell(out, status)),
        ("Started", started),
        ("Ended", job.end_time.clone().unwrap_or_default()),
        (
            "Duration",
            job.duration().map(jobs::took_text).unwrap_or_default(),
        ),
        ("Result", job.result.clone().unwrap_or_default()),
        (
            "Prediction setup",
            job.prediction_setup_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
        ),
        (
            "Database result",
            database_result.map(|id| id.to_string()).unwrap_or_default(),
        ),
    ];
    crate::output::fields_with(0, &rows, &|label| out.key(label))
}

/// What to do with this job now. Only a failure needs a step; the other
/// statuses get the optional commands as hints.
fn next_step(job: &Job, status: &str, database_result: Option<i64>, lines: &mut output::Report) {
    let id = &job.id;
    match jobs::Outcome::of(status) {
        jobs::Outcome::Failed => lines.info(format!("run `chap-devops jobs logs {id}` to see why")),
        jobs::Outcome::Running => lines.hint(format!(
            "`chap-devops jobs` shows when it finishes, and `chap-devops jobs cancel {id}` stops it"
        )),
        jobs::Outcome::Cancelled => {
            lines.hint(format!("`chap-devops jobs logs {id}` shows how far it got"))
        }
        jobs::Outcome::Done => match database_result {
            // The collection the row is in follows the job type, so the line
            // is a command that works rather than one to adapt.
            Some(row) => match collection_of(&job.kind) {
                Some(collection) => lines.hint(format!(
                    "the result is row {row} in chap-core's database; \
                     `chap-devops api GET /v1/crud/{collection}/{row}` reads it"
                )),
                None => lines.hint(format!("the result is row {row} in chap-core's database")),
            },
            None => lines.hint(format!("`chap-devops jobs logs {id}` shows what it did")),
        },
    };
}

/// The `/v1/crud/` collection a job of this type writes its result into.
///
/// `None` for a job type this CLI has not heard of: chap-core is free to add
/// one, and guessing at a path would be worse than saying only the row.
fn collection_of(kind: &str) -> Option<&'static str> {
    match kind.trim() {
        "create_dataset" => Some("datasets"),
        "create_backtest" | "create_backtest_from_data" | "run_baseline" => Some("backtests"),
        "create_prediction" => Some("predictions"),
        _ => None,
    }
}

/// The STATUS field, coloured the way the rest of the CLI colours a verdict.
fn status_cell(out: &Out, status: &str) -> String {
    match jobs::Outcome::of(status) {
        jobs::Outcome::Done => out.ok(status),
        jobs::Outcome::Failed => out.bad(status),
        jobs::Outcome::Cancelled => out.warn(status),
        jobs::Outcome::Running => out.value(status),
    }
}

/// `chap-devops jobs logs ID`.
///
/// stdout is the log and nothing else, so it can be piped into `grep` or
/// `less`; which job it is, and the one line that says why it failed, go to
/// stderr.
pub fn logs(ctx: &Ctx, args: &JobsLogsArgs) -> Result<()> {
    let (_project, api) = connect(ctx)?;
    let (id, jobs) = find(ctx, &api, &args.id)?;
    let job = jobs
        .into_iter()
        .find(|j| j.id == id)
        .unwrap_or_else(|| Job {
            id: id.clone(),
            ..Job::default()
        });

    let path = format!("{}/{}/logs", jobs::JOBS_PATH, crate::api::encode(&id));
    let answer = api.send("GET", &path, None)?;
    if !answer.is_success() {
        return Err(api.status_error(&path, &answer));
    }
    // The endpoint answers with a JSON string holding the whole log; anything
    // else is taken as the log itself, so a chap-core that stops quoting it
    // still prints.
    let text = match answer.json() {
        Some(serde_json::Value::String(text)) => text,
        _ => answer.text().into_owned(),
    };
    let shown = match args.tail {
        Some(n) => jobs::tail(&text, n),
        None => text.clone(),
    };

    if !ctx.out.json {
        eprintln!("{}", job.headline());
    }
    let value = serde_json::json!({
        "id": job.id,
        "type": job.kind,
        "name": job.name,
        "status": job.status,
        "logs": shown,
    });
    ctx.out.emit(&value, || shown.clone())?;

    // Only for a job that failed, and only from the section a model's own
    // error stream is in: on a job that worked the last stderr line is
    // usually a package loading itself.
    if !ctx.out.json && job.outcome() == jobs::Outcome::Failed {
        match jobs::stderr_hint(&text) {
            Some(hint) => crate::output::notice(&format!("stderr: {hint}")),
            None => crate::output::notice(
                "this job failed; the traceback above is chap-core's own record of it",
            ),
        }
    }
    Ok(())
}

/// `chap-devops jobs cancel ID`.
pub fn cancel(ctx: &Ctx, args: &JobsCancelArgs) -> Result<()> {
    let (_project, api) = connect(ctx)?;
    let (id, _) = find(ctx, &api, &args.id)?;
    let path = format!("{}/{}/cancel", jobs::JOBS_PATH, crate::api::encode(&id));
    let answer = api.send("POST", &path, None)?;
    if !answer.is_success() {
        return Err(api.status_error(&path, &answer));
    }
    let message = message_of(&answer, "cancelled");
    let value = serde_json::json!({ "id": id, "cancelled": true, "message": message });
    ctx.out.report(&value, |lines| {
        lines
            .info(message.as_str())
            .hint("`chap-devops jobs` shows whether it has stopped");
    })
}

/// `chap-devops jobs delete ID`.
pub fn delete(ctx: &Ctx, args: &JobsDeleteArgs) -> Result<()> {
    let (_project, api) = connect(ctx)?;
    let (id, _) = find(ctx, &api, &args.id)?;
    let path = format!("{}/{}", jobs::JOBS_PATH, crate::api::encode(&id));
    let answer = api.send("DELETE", &path, None)?;
    // chap-core refuses to forget a job it is still working on, and says so
    // with a 400. The verb that does apply is `cancel`, so name it.
    if answer.status == 400 {
        return Err(anyhow::anyhow!(
            "job {id} is still running; cancel it first (`chap-devops jobs cancel {id}`)"
        ));
    }
    if !answer.is_success() {
        return Err(api.status_error(&path, &answer));
    }
    let message = message_of(&answer, "deleted");
    let value = serde_json::json!({ "id": id, "deleted": true, "message": message });
    ctx.out.report(&value, |lines| {
        lines
            .info(message.as_str())
            .hint("`chap-devops jobs` shows the jobs that are left");
    })
}

/// The `message` chap-core answered with, or a sentence of our own when it
/// answered with something else.
fn message_of(answer: &crate::api::Answer, verb: &str) -> String {
    answer
        .json()
        .and_then(|value| {
            value
                .get("message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("job {verb}"))
}

/// This deployment and a client for its API.
fn connect(ctx: &Ctx) -> Result<(Project, Api)> {
    let project = ctx.project()?;
    crate::components::require_chap_core(&project.state.components, "`chap-devops jobs`")?;
    let token = crate::api::token_for(Some(&project.dir));
    let api = Api::new(&project.api_url(), token, crate::api::DEFAULT_TIMEOUT);
    ctx.out
        .verbose(&format!("asking chap-core at {}", api.base()));
    Ok((project, api))
}

/// The job list, parsed and in table order, with the wire documents beside it.
fn fetch(api: &Api, path: &str) -> Result<(Vec<Job>, Vec<serde_json::Value>)> {
    let value = api.get_json(path)?;
    let list = value.as_array().ok_or_else(|| {
        anyhow::anyhow!(
            "{} answered with {} rather than a list of jobs",
            api.url(path),
            kind_of(&value)
        )
    })?;
    let jobs: Vec<Job> = list
        .iter()
        .map(|entry| serde_json::from_value(entry.clone()))
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("reading the job list from {}: {e}", api.url(path)))?;
    let order = jobs::order(&jobs);
    Ok((
        order.iter().map(|i| jobs[*i].clone()).collect(),
        order.iter().map(|i| list[*i].clone()).collect(),
    ))
}

/// What a JSON value is, for an error that has to say what came back instead.
fn kind_of(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "a list",
        serde_json::Value::Object(_) => "an object",
    }
}

/// The full id `given` names, and the list it was found in.
///
/// An id is matched exactly first and then as a prefix, so the eight
/// characters the table prints are enough to type. A prefix that came out of a
/// table is announced under `-v`: which job a command acted on is the thing to
/// check when it acted on the wrong one.
fn find(ctx: &Ctx, api: &Api, given: &str) -> Result<(String, Vec<Job>)> {
    let (jobs, _) = fetch(api, jobs::JOBS_PATH)?;
    let matched = jobs::resolve(&jobs, given);
    if let Matched::One(id) = &matched {
        if !id.eq_ignore_ascii_case(given.trim()) {
            ctx.out.verbose(&format!("matched {id}"));
        }
        return Ok((id.clone(), jobs));
    }
    // A job chap-core knows and does not list is not a case the API has, but
    // asking costs one request and is the difference between "not found" and
    // a wrong answer if it ever gains one.
    let given = given.trim().to_string();
    if matches!(matched, Matched::None) && current_status(api, &given)?.is_some() {
        return Ok((given, jobs));
    }
    Err(jobs::no_such_job(&given, &matched))
}

/// The status from `GET /v1/jobs/{id}`.
///
/// `None` for a 404, which is how chap-core says it has no such job.
fn current_status(api: &Api, id: &str) -> Result<Option<String>> {
    let path = format!("{}/{}", jobs::JOBS_PATH, crate::api::encode(id));
    let answer = api.send("GET", &path, None)?;
    if answer.status == 404 {
        return Ok(None);
    }
    if !answer.is_success() {
        return Err(api.status_error(&path, &answer));
    }
    Ok(Some(jobs::status_in(&answer.text())))
}

/// The row a finished job wrote, from `GET /v1/jobs/{id}/database_result`.
///
/// Best effort: the endpoint answers 400 while the job runs and 400 again for
/// one that failed, and neither is something to stop `chap-devops jobs show` over.
fn database_result(api: &Api, id: &str, status: &str) -> Option<i64> {
    if jobs::Outcome::of(status) != jobs::Outcome::Done {
        return None;
    }
    let path = format!(
        "{}/{}/database_result",
        jobs::JOBS_PATH,
        crate::api::encode(id)
    );
    let answer = api.send("GET", &path, None).ok()?;
    if !answer.is_success() {
        return None;
    }
    answer.json()?.get("id")?.as_i64()
}

#[cfg(test)]
mod tests;
