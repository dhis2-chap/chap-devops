//! The backtest level: through chap-core, the way the Modeling App goes.

use super::configured::{self, Choice};
use super::{first_line, period_type, service_info};
use crate::api::Api;
use crate::cli::ModelsTestArgs;
use crate::commands::Ctx;
use crate::error::Result;
use crate::jobs;
use crate::modeltest::{self, Level, Run, Verdict};
use crate::output::Message;
use crate::project::EnabledModel;
use std::time::{Duration, Instant};

mod baselines;

/// How often the dataset job is asked whether it has finished.
const DATASET_POLL: Duration = Duration::from_secs(3);

/// How often the backtest job is, which is the slower of the two.
const BACKTEST_POLL: Duration = Duration::from_secs(5);

/// Build a dataset from the model's own sample data, backtest over it, and
/// report the scores.
pub(super) fn backtest_level(
    ctx: &Ctx,
    api: &Api,
    id: &str,
    enabled: &EnabledModel,
    args: &ModelsTestArgs,
    timeout: Duration,
) -> Run {
    let mut run = Run::new(id, &enabled.service_id, Level::Backtest);
    let until = Instant::now() + timeout;

    let Some(info) = service_info(ctx, api, &enabled.service_id) else {
        return run.end(
            Verdict::Skip,
            "chap-core has no registration for it",
            Some("run `chap-devops status` to see why".to_string()),
        );
    };
    let period = period_type(Some(&info));

    // A registration is the model calling chap-core; a backtest is chap-core
    // calling the model. When that way back is broken, every later step fails
    // with a message about something else, so it is asked first.
    let proxied = crate::status::proxied_health_path(&enabled.service_id);
    let unreachable = match api.send("GET", &proxied, None) {
        Ok(answer) if answer.status >= 500 => Some(answer.status_line()),
        Ok(_) => None,
        Err(err) => Some(err.to_string()),
    };
    if let Some(answer) = unreachable {
        return run.end(
            Verdict::Skip,
            format!("chap-core cannot reach it ({answer} from {proxied})"),
            Some("run `chap-devops status`, which names the address and the fix".to_string()),
        );
    }

    // Before anything is built: a service chap-core has nothing configured
    // for cannot be backtested, and finding that out after a dataset has been
    // imported would be a dataset created and deleted for nothing.
    let version = info
        .get("version")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let of = match &version {
        Some(version) => format!("{} {version}", enabled.service_id),
        None => enabled.service_id.clone(),
    };
    let wanted = args.config.as_deref();
    let (model_id, covariates) =
        match configured::choose(ctx, api, &enabled.service_id, version.as_deref(), wanted) {
            Choice::Chosen {
                model_id,
                covariates,
                used,
            } => {
                run.configured_model = Some(used);
                (model_id, covariates)
            }
            Choice::NoneLive => {
                return run.end(
                Verdict::Skip,
                format!("chap-core has no configured model for {of}"),
                Some(format!(
                    "it is registered, and chap-core has nothing to run it with; run `chap-devops \
                         models configs sync {id}`, then `chap-devops models test {id} --backtest`"
                )),
            );
            }
            Choice::NoVariant(names) => {
                return run.end(
                    Verdict::Skip,
                    format!(
                        "chap-core has no configured model {} for {of}; it has {}",
                        wanted.unwrap_or_default(),
                        names.join(", ")
                    ),
                    Some(format!(
                        "run `chap-devops models configs list {id}` to see its configured models"
                    )),
                );
            }
            Choice::Unlisted => {
                return run.end(
                    Verdict::Skip,
                    format!(
                        "chap-core did not list its configured models, so chap-devops cannot find \
                         --config {}",
                        wanted.unwrap_or_default()
                    ),
                    Some("run `chap-devops status` to see why".to_string()),
                );
            }
        };

    let sample = match sample_data(
        api,
        &enabled.service_id,
        &period,
        covariates.len(),
        args.seed,
    ) {
        Ok(Some(sample)) => sample,
        Ok(None) => {
            return run.end(
                Verdict::Skip,
                format!(
                    "the image's chapkit predates the sample-data route \
                     (needs chapkit {}, this one reports {})",
                    modeltest::SAMPLE_DATA_CHAPKIT,
                    chapkit_version(Some(&info))
                ),
                Some(format!(
                    "update the model with `chap-devops update`, or test it on its own \
                     with `chap-devops models test {id}`"
                )),
            );
        }
        Err(err) => {
            return run.end(
                Verdict::Skip,
                "its sample data could not be fetched",
                Some(first_line(&err.to_string())),
            );
        }
    };

    let mut sample = sample;
    let frame = match sample.get_mut("data") {
        Some(frame) => frame,
        None => {
            return run.end(
                Verdict::Skip,
                "its sample data carries no frame",
                Some("run `chap-devops models test --backtest -v` to see the answer".to_string()),
            );
        }
    };
    for (from, to) in modeltest::fill_covariates(frame, &covariates) {
        ctx.out.verbose(&format!(
            "{}: the sample has no `{to}`, which the configured model asks for; `{from}` stands in",
            enabled.service_id
        ));
    }
    let (observations, locations) = match modeltest::observations(frame) {
        Ok(parts) => parts,
        Err(err) => {
            return run.end(
                Verdict::Skip,
                "its sample data is not a frame chap-devops understands",
                Some(first_line(&err.to_string())),
            );
        }
    };
    let geojson = modeltest::feature_collection(sample.get("geo"), &locations);
    let name = format!(
        "chap-devops-test-{}-{}",
        enabled.service_id,
        crate::backup::stamp(crate::backup::now())
    );
    ctx.out.verbose(&format!(
        "{}: {} observations over {} org units as `{name}`",
        enabled.service_id,
        observations.len(),
        locations.len()
    ));

    // --- the dataset ---
    let body = serde_json::json!({
        "name": name,
        "geojson": geojson,
        "providedData": observations,
        "dataToBeFetched": [],
    });
    let made = match post(api, "/v1/analytics/make-dataset", &body) {
        Ok(made) => made,
        Err(err) => {
            return run.end(
                Verdict::Skip,
                "chap-core would not take the dataset",
                Some(first_line(&err.to_string())),
            );
        }
    };
    if made.get("importedCount").and_then(|c| c.as_i64()) == Some(0) {
        return run.end(
            Verdict::Fail,
            "chap-core imported no org units from the sample data",
            Some("run `chap-devops models test --backtest -v` to see what was sent".to_string()),
        );
    }
    let Some(dataset_job) = made.get("id").and_then(|id| id.as_str()) else {
        return run.end(
            Verdict::Skip,
            "chap-core answered make-dataset without a job id",
            Some("run `chap-devops models test --backtest -v` to see the answer".to_string()),
        );
    };
    run.job_id = Some(dataset_job.to_string());
    match wait(ctx, api, dataset_job, DATASET_POLL, until) {
        Ok(Some(status)) if jobs::Outcome::of(&status) == jobs::Outcome::Done => {}
        Ok(Some(_)) => {
            let (why, next) = job_failure(api, dataset_job);
            return run.end(Verdict::Fail, format!("dataset: {why}"), Some(next));
        }
        Ok(None) => {
            return run.end(
                Verdict::Skip,
                format!(
                    "its dataset was still building after {}",
                    modeltest::took(timeout.as_secs())
                ),
                Some(format!(
                    "run `chap-devops jobs show {dataset_job}` for where it got to"
                )),
            );
        }
        Err(err) => {
            return run.end(
                Verdict::Skip,
                "chap-core stopped answering",
                Some(first_line(&err.to_string())),
            );
        }
    }
    let Some(dataset) = database_result(api, dataset_job) else {
        return run.end(
            Verdict::Skip,
            "chap-core built the dataset without saying which row it is",
            Some(format!("run `chap-devops jobs show {dataset_job}`")),
        );
    };
    ctx.out
        .verbose(&format!("{}: dataset {dataset}", enabled.service_id));

    // --- the backtest ---
    let body = serde_json::json!({
        "name": name,
        "modelId": model_id,
        "datasetId": dataset,
        "nPeriods": modeltest::BACKTEST_PERIODS,
        "nSplits": modeltest::BACKTEST_SPLITS,
        "stride": modeltest::BACKTEST_STRIDE,
    });
    let started = match post(api, "/v1/analytics/create-backtest", &body) {
        Ok(started) => started,
        Err(err) => {
            let cleanup = drop_rows(ctx, api, None, Some(dataset), dataset_job, args.keep);
            return run.with_cleanup(cleanup).end(
                Verdict::Skip,
                "chap-core would not start the backtest",
                Some(first_line(&err.to_string())),
            );
        }
    };
    let Some(backtest_job) = started
        .get("id")
        .and_then(|id| id.as_str())
        .map(str::to_string)
    else {
        let cleanup = drop_rows(ctx, api, None, Some(dataset), dataset_job, args.keep);
        return run.with_cleanup(cleanup).end(
            Verdict::Skip,
            "chap-core answered create-backtest without a job id",
            Some("run `chap-devops models test --backtest -v` to see the answer".to_string()),
        );
    };
    run.job_id = Some(backtest_job.clone());

    match wait(ctx, api, &backtest_job, BACKTEST_POLL, until) {
        Ok(Some(status)) if jobs::Outcome::of(&status) == jobs::Outcome::Done => {}
        Ok(Some(_)) => {
            let (why, next) = job_failure(api, &backtest_job);
            let cleanup = drop_rows(ctx, api, None, Some(dataset), dataset_job, args.keep);
            return run
                .with_cleanup(cleanup)
                .end(Verdict::Fail, why, Some(next));
        }
        Ok(None) => {
            // Nothing is deleted: the job is still running, and its dataset
            // is what it is reading.
            return run.end(
                Verdict::Skip,
                format!(
                    "it was still backtesting after {}",
                    modeltest::took(timeout.as_secs())
                ),
                Some(format!(
                    "run `chap-devops jobs show {backtest_job}` for where it got to; \
                     dataset {dataset} was left behind"
                )),
            );
        }
        Err(err) => {
            // The dataset this run uploaded stays behind as it does on the
            // deadline, and is named the same way so it can be removed.
            return run.end(
                Verdict::Skip,
                "chap-core stopped answering",
                Some(format!(
                    "{}; dataset {dataset} was left behind, and `chap-devops api DELETE \
                     /v1/crud/datasets/{dataset}` removes it once chap-core answers",
                    first_line(&err.to_string())
                )),
            );
        }
    }

    let backtest = database_result(api, &backtest_job);
    run.backtest_id = backtest;
    let metrics = backtest.and_then(|row| {
        api.get_json(&format!("/v1/crud/backtests/{row}"))
            .ok()?
            .get("aggregateMetrics")
            .cloned()
    });
    let summary = match &metrics {
        Some(metrics) => modeltest::metrics_cell(metrics),
        None => "it finished, but chap-core reported no scores".to_string(),
    };
    run.metrics = metrics;
    let cleanup = drop_rows(ctx, api, backtest, Some(dataset), dataset_job, args.keep);
    run.with_cleanup(cleanup).end(Verdict::Pass, summary, None)
}

/// The chapkit release the service says it was built with.
fn chapkit_version(info: Option<&serde_json::Value>) -> String {
    info.and_then(|info| info.get("chapkit_version"))
        .and_then(|value| value.as_str())
        .unwrap_or("nothing")
        .to_string()
}

/// `GET .../$generate-sample-data` through chap-core's proxy.
///
/// `Ok(None)` for a 404, which is how a model built before chapkit
/// [`modeltest::SAMPLE_DATA_CHAPKIT`] says it has no such route.
///
/// `covariates` is how many the configured model asks for: at least that many
/// `feature_N` columns are generated, so each one the sample lacks has a spare
/// to be renamed from (see [`modeltest::fill_covariates`]).
fn sample_data(
    api: &Api,
    service_id: &str,
    period: &str,
    covariates: usize,
    seed: Option<i64>,
) -> Result<Option<serde_json::Value>> {
    // `%24` rather than a bare `$`: the proxy forwards the raw sub-path, and
    // the encoded form is the spelling every hop agrees on.
    //
    // `include_geo=true` whatever the service declares. chap-core's dataset
    // always carries a geometry, so a model that says it needs none would
    // otherwise be handed features with no geometry in them - and a model
    // that builds a neighbour graph fails on an empty polygon where it would
    // have been perfectly happy with none at all. Real polygons are never
    // worse: a model that ignores geometry ignores these too.
    let mut path = format!(
        "/v2/services/{}/run/api/v1/ml/%24generate-sample-data?kind=train&num_locations={}&num_periods={}&period_type={period}&include_geo=true&num_features={}",
        crate::api::encode(service_id),
        modeltest::SAMPLE_LOCATIONS,
        modeltest::SAMPLE_PERIODS,
        covariates.max(modeltest::SAMPLE_FEATURES),
    );
    if let Some(seed) = seed {
        path.push_str(&format!("&seed={seed}"));
    }
    let answer = api.send("GET", &path, None)?;
    if answer.status == 404 {
        return Ok(None);
    }
    if !answer.is_success() {
        return Err(api.status_error(&path, &answer));
    }
    answer
        .json()
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("{} did not answer with JSON", api.url(&path)))
}

/// `POST path` with a JSON body, where a non-2xx is an error.
fn post(api: &Api, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
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
fn wait(
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
fn job_status(api: &Api, job: &str) -> Result<String> {
    let path = format!("{}/{}", jobs::JOBS_PATH, crate::api::encode(job));
    let answer = api.send("GET", &path, None)?;
    if !answer.is_success() {
        return Err(api.status_error(&path, &answer));
    }
    Ok(jobs::status_in(&answer.text()))
}

/// The row a finished job wrote, from `GET /v1/jobs/{id}/database_result`.
fn database_result(api: &Api, job: &str) -> Option<i64> {
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
fn job_failure(api: &Api, job: &str) -> (String, String) {
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
/// baselines and of the model first, in one request when there are several,
/// then the dataset they point at.
fn drop_rows(
    ctx: &Ctx,
    api: &Api,
    backtest: Option<i64>,
    dataset: Option<i64>,
    dataset_job: &str,
    keep: bool,
) -> Vec<Message> {
    let mut backtests = dataset
        .map(|dataset| baselines::rows(ctx, api, dataset, dataset_job))
        .unwrap_or_default();
    backtests.extend(backtest);
    backtests.sort_unstable();
    let rows = row_paths(&backtests, dataset);
    let mut lines = Vec::new();
    if keep {
        if !rows.is_empty() {
            let named: Vec<&str> = rows.iter().map(|(named, _)| named.as_str()).collect();
            let commands: Vec<String> = rows
                .iter()
                .map(|(_, path)| format!("`chap-devops api DELETE {}`", shell_word(path)))
                .collect();
            lines.push(Message::info(format!(
                "kept {}; remove them with {}",
                named.join(" and "),
                commands.join(" then ")
            )));
        }
        return lines;
    }
    for (_, path) in rows {
        let command = format!("`chap-devops api DELETE {}`", shell_word(&path));
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

/// What to delete, in order, as `(what it is, the API path)`: the backtests,
/// in one request when there are several, then the dataset.
fn row_paths(backtests: &[i64], dataset: Option<i64>) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    match backtests {
        [] => {}
        [one] => rows.push((
            format!("backtest {one}"),
            format!("/v1/crud/backtests/{one}"),
        )),
        several => {
            let ids: Vec<String> = several.iter().map(i64::to_string).collect();
            rows.push((
                format!("backtests {}", and_list(&ids)),
                format!("/v1/crud/backtests?ids={}", ids.join(",")),
            ));
        }
    }
    if let Some(dataset) = dataset {
        rows.push((
            format!("dataset {dataset}"),
            format!("/v1/crud/datasets/{dataset}"),
        ));
    }
    rows
}

/// A path as one shell word: a query string has `?`, which a shell expands.
fn shell_word(path: &str) -> String {
    match path.contains('?') {
        true => format!("'{path}'"),
        false => path.to_string(),
    }
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
