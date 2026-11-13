//! `chap-devops bench --write`: import a dataset, start N backtests of one
//! model on it at the same time, wait for them, and check what each one
//! stored. A job that fails, does not finish, or stores the wrong number of
//! forecasts is a failure, so a run with more workers can be compared with
//! a run with one. See `docs/bench.md`.

use super::measure::human_ms;
use crate::api::Api;
use crate::cli::BenchArgs;
use crate::commands::Ctx;
use crate::commands::chap_jobs::{Target, database_result, drop_rows, job_failure};
use crate::commands::dataset;
use crate::commands::modeltest::first_line;
use crate::error::{ChapError, Result};
use crate::jobs::Outcome;
use crate::output::{Level, Message, Report};
use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, Instant};

/// How often the running jobs are asked whether they have finished.
pub(super) const POLL: Duration = Duration::from_secs(3);

/// The exit code when a backtest failed.
const EXIT_FAILED: i32 = 1;

/// One backtest of the run: a job of a `create-backtests` request, or a
/// baseline job that one of them queued.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Backtest {
    /// The run that started it; `0` for a baseline.
    pub number: u32,
    /// The configured model, or the name of the baseline.
    pub model: String,
    pub baseline: bool,
    pub job: Option<String>,
    /// The last status chap-core gave, or `not started`.
    pub status: String,
    /// How long the `create-backtests` request of its run took.
    pub request_seconds: Option<f64>,
    /// From the answer to the request to the start of the job.
    pub queued_seconds: Option<f64>,
    /// How long the job ran, from its start and end time in `/v1/jobs`.
    pub seconds: Option<f64>,
    /// When the request was answered, in seconds since the Unix epoch.
    #[serde(skip)]
    pub answered_at: Option<f64>,
    /// Whether a poll saw the job end.
    #[serde(skip)]
    pub ended: bool,
    pub backtest_id: Option<i64>,
    pub forecasts: Option<usize>,
    pub expected_forecasts: Option<usize>,
    pub metrics: Option<usize>,
    /// Why it failed; `None` when it passed.
    pub problem: Option<String>,
}

impl Backtest {
    pub fn passed(&self) -> bool {
        self.problem.is_none()
    }
}

/// What the backtest row says it covers: one forecast for each org unit,
/// split and forecast period.
pub fn expected_forecasts(row: &Value) -> Option<usize> {
    let units = row.get("orgUnits")?.as_array()?.len();
    let splits = row.get("splitPeriods")?.as_array()?.len();
    let periods = row.get("nPeriods")?.as_u64()? as usize;
    Some(units * splits * periods)
}

/// The problem with a finished backtest, if there is one.
pub fn check(forecasts: Option<usize>, expected: Option<usize>, metrics: usize) -> Option<String> {
    match (forecasts, expected) {
        (None, _) => Some("chap-core did not give its forecasts".to_string()),
        (Some(0), _) => Some("it stored no forecasts".to_string()),
        (Some(got), Some(want)) if got != want => Some(format!(
            "it stored {got} forecasts, and its org units, splits and periods make {want}"
        )),
        _ if metrics == 0 => Some("it stored no aggregate metrics".to_string()),
        _ => None,
    }
}

pub(super) fn run(ctx: &Ctx, args: &BenchArgs, target: &Target) -> Result<()> {
    let api = Api::new(
        &target.base,
        target.token.clone(),
        Duration::from_secs(args.timeout),
    );
    let models = model_ids(&api, args.model.as_deref())?;
    let model = models
        .iter()
        .map(|(_, name)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let stamp = crate::backup::stamp(crate::backup::now());
    let wait = Duration::from_secs(args.wait);
    let started = Instant::now();

    let mut recorder = super::recorder(ctx, args, target.local);
    let mut phase = |name: &str| {
        if let Some(r) = recorder.as_mut() {
            r.phase(name);
        }
    };

    // --- the dataset ---
    phase("import");
    let spec = args.synthetic.spec();
    let mut synthetic = None;
    let mut import_took = None;
    let (dataset, dataset_job, made) = match args.dataset {
        Some(id) => {
            api.get_json(&format!("/v1/crud/datasets/{id}")).map_err(|e| {
                anyhow::anyhow!(
                    "chap-core has no dataset {id} ({}); leave out --dataset to import a new one",
                    first_line(&e.to_string())
                )
            })?;
            (id, None, false)
        }
        None => {
            let data = crate::synthetic::generate(&spec);
            let name = format!("{}-bench-{stamp}", spec.name());
            let imported = dataset::import(ctx, &api, &data, &name, args.synthetic.format, wait)?;
            ctx.out.verbose(&format!(
                "dataset {} from {}",
                imported.dataset,
                dataset::describe(&spec, data.rows())
            ));
            import_took = Some((imported.request_seconds, imported.took()));
            synthetic = Some(data);
            (imported.dataset, Some(imported.job), true)
        }
    };
    let imported_in = started.elapsed().as_secs_f64();

    // --- the backtests, all started before any is polled ---
    phase("backtests");
    crate::interrupt::install();
    crate::output::notice(&format!(
        "starting {} runs of {model} on dataset {dataset}",
        args.backtests
    ));
    let before = super::runs::baseline_jobs(&api);
    let mut runs = super::runs::start(&api, &models, dataset, &stamp, args.backtests);

    // --- the wait, for the baselines too ---
    let stopped = super::runs::wait(ctx, &api, &mut runs, &before, Instant::now() + wait);
    super::runs::times(&api, &mut runs);

    // --- what each one stored ---
    for run in runs.iter_mut() {
        inspect(&api, run, wait);
    }
    let still_running = runs.iter().any(|r| r.job.is_some() && !r.ended);

    // --- the prediction, from the first backtest that passed ---
    if args.predict {
        phase("prediction");
    }
    let first_passed = runs
        .iter()
        .find(|r| r.passed() && !r.baseline)
        .and_then(|r| r.backtest_id);
    let prediction = match (&synthetic, first_passed) {
        (Some(data), Some(backtest)) if args.predict && !stopped => Some(super::predict::make(
            ctx,
            &api,
            data,
            backtest,
            &format!("bench-{stamp}"),
            Instant::now() + wait,
        )),
        _ => None,
    };
    let phases = recorder.map(super::resources::Recorder::finish);
    let prediction_running = prediction
        .as_ref()
        .is_some_and(|p| p.job.is_some() && p.seconds.is_none());
    let mut cleanup: Vec<Message> = match (&prediction, still_running || prediction_running) {
        (Some(p), false) => super::predict::drop_rows(ctx, &api, p, args.keep),
        _ => Vec::new(),
    };

    // --- the cleanup, unless a job still uses the rows ---
    let made_backtests: Vec<i64> = runs.iter().filter_map(|r| r.backtest_id).collect();
    cleanup.extend(match (still_running || prediction_running, &dataset_job) {
        (true, _) => vec![Message::warning(format!(
            "kept dataset {dataset} and the backtests, because some jobs still run; \
             `chap-devops jobs` lists them"
        ))],
        (false, Some(job)) => drop_rows(ctx, &api, &made_backtests, Some(dataset), job, args.keep),
        (false, None) => drop_rows(ctx, &api, &made_backtests, None, "", args.keep),
    });

    let failed = runs.iter().filter(|r| !r.passed()).count()
        + prediction.iter().filter(|p| p.problem.is_some()).count();
    if !ctx.out.json {
        print!("{}", table(ctx, &runs));
    }
    let server = super::server::describe(ctx, &api, target.local);
    let value = serde_json::json!({
        "url": api.base(),
        "server": server,
        "models": models.iter().map(|(_, name)| name).collect::<Vec<_>>(),
        "dataset": dataset,
        "dataset_imported": made,
        "import_seconds": made.then_some(imported_in),
        "import_request_seconds": import_took.as_ref().map(|t| t.0),
        "backtest_request_seconds": runs.iter().filter_map(|r| r.request_seconds).fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s)))),
        "backtests": runs,
        "prediction": prediction,
        "resources": phases,
        "failed": failed,
        "seconds": started.elapsed().as_secs_f64(),
    });
    ctx.out.report(&value, |lines| {
        closing(&runs, &model, dataset, started.elapsed(), stopped, lines);
        lines.info(super::server::line(&server));
        for line in phases
            .as_deref()
            .map(super::resources::lines)
            .unwrap_or_default()
        {
            lines.info(line);
        }
        if args.resources && !target.local {
            lines.warning(super::RESOURCES_NEED_A_DEPLOYMENT);
        }
        match &prediction {
            Some(p) => {
                let message = super::predict::line(p, args.keep);
                match message.level {
                    Level::Warning => lines.warning(message.text),
                    _ => lines.info(message.text),
                };
            }
            None if args.predict && args.dataset.is_none() && !stopped => {
                lines.warning("made no prediction, because no backtest passed");
            }
            None => {}
        }
        if cleanup.is_empty() && !still_running {
            lines.info(deleted_line(made.then_some(dataset), &made_backtests));
        }
        for message in &cleanup {
            match message.level {
                Level::Warning => lines.warning(message.text.as_str()),
                _ => lines.info(message.text.as_str()),
            };
        }
        if let Some((_, took)) = &import_took {
            lines.hint(format!("the import took {took}"));
        }
        if target.local && failed > 0 {
            lines.hint("`chap-devops logs worker` shows what the worker did");
        }
    })?;
    if failed > 0 {
        std::process::exit(EXIT_FAILED);
    }
    Ok(())
}

/// What the cleanup deleted, when it deleted all of it.
pub fn deleted_line(dataset: Option<i64>, backtests: &[i64]) -> String {
    let mut ids = backtests.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let backtests = match ids.len() {
        0 => None,
        1 => Some("1 backtest".to_string()),
        n => Some(format!("{n} backtests")),
    };
    match (dataset, backtests) {
        (Some(d), Some(b)) => {
            format!("deleted dataset {d} and the {b} the run made; --keep keeps them")
        }
        (Some(d), None) => format!("deleted dataset {d}, which the run made; --keep keeps it"),
        (None, Some(b)) => format!("deleted the {b} the run made; --keep keeps them"),
        (None, None) => "the run made no rows to delete".to_string(),
    }
}

/// The closing lines: how many passed, and why each other one failed.
fn closing(
    runs: &[Backtest],
    model: &str,
    dataset: i64,
    took: Duration,
    stopped: bool,
    lines: &mut Report,
) {
    let own: Vec<&Backtest> = runs.iter().filter(|r| !r.baseline).collect();
    let passed = own.iter().filter(|r| r.passed()).count();
    lines.info(format!(
        "{passed} of {} backtests of {model} on dataset {dataset} passed, in {}",
        own.len(),
        human_ms(took.as_secs_f64() * 1000.0)
    ));
    let baselines: Vec<&Backtest> = runs.iter().filter(|r| r.baseline).collect();
    if !baselines.is_empty() {
        lines.info(format!(
            "{} of {} baseline backtests passed",
            baselines.iter().filter(|r| r.passed()).count(),
            baselines.len()
        ));
    }
    if stopped {
        lines.warning("stopped by Ctrl-C before every backtest finished");
    }
    for run in runs.iter().filter(|r| !r.passed()) {
        let job = run
            .job
            .as_deref()
            .map(|job| format!(" (job {job})"))
            .unwrap_or_default();
        let which = match run.baseline {
            true => format!("baseline {}", run.model),
            false => format!("backtest {} of {}", run.number, run.model),
        };
        lines.warning(format!(
            "{which}{job}: {}",
            run.problem.as_deref().unwrap_or_default()
        ));
    }
}

/// Fill in what a backtest ended as and stored.
fn inspect(api: &Api, run: &mut Backtest, wait: Duration) {
    let Some(job) = run.job.clone() else { return };
    if !run.ended {
        run.problem = Some(format!(
            "still {} after {} s; `chap-devops jobs show {job}` shows where it is",
            run.status,
            wait.as_secs()
        ));
        return;
    }
    if Outcome::of(&run.status) != Outcome::Done {
        let (why, next) = job_failure(api, &job);
        run.problem = Some(format!("{why}; {next}"));
        return;
    }
    let Some(id) = database_result(api, &job) else {
        run.problem = Some("it finished without saying which backtest it wrote".to_string());
        return;
    };
    run.backtest_id = Some(id);
    let row = api.get_json(&format!("/v1/crud/backtests/{id}")).ok();
    run.expected_forecasts = row.as_ref().and_then(expected_forecasts);
    let metrics = row
        .as_ref()
        .and_then(|r| r.get("aggregateMetrics")?.as_object().map(|m| m.len()))
        .unwrap_or(0);
    run.metrics = Some(metrics);
    // One entry for each stored forecast at one quantile. `/full` does not
    // carry the forecasts in chap-core v2.7.0.
    run.forecasts = api
        .get_json(&format!(
            "/v1/analytics/evaluation-entry?backtestId={id}&quantiles=0.5"
        ))
        .ok()
        .and_then(|entries| entries.as_array().map(Vec::len));
    run.problem = check(run.forecasts, run.expected_forecasts, metrics);
}

/// The configured models to backtest, a comma-separated list of names or
/// ids, checked against chap-core's list: each as its id and name.
fn model_ids(api: &Api, given: Option<&str>) -> Result<Vec<(i64, String)>> {
    let list = api.get_json("/v1/crud/configured-models")?;
    let models: Vec<(i64, String)> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| !m.get("archived").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|m| {
            let id = m.get("id")?.as_i64()?;
            let name = m.get("name")?.as_str()?.to_string();
            Some((id, name))
        })
        .collect();
    let names = || {
        let mut names: Vec<&str> = models.iter().map(|(_, n)| n.as_str()).collect();
        names.sort_unstable();
        names.join(", ")
    };
    let Some(given) = given else {
        return Err(ChapError::Usage(format!(
            "--write needs --model, one or more of the configured models of chap-core: {}",
            names()
        ))
        .into());
    };
    let mut picked = Vec::new();
    for one in given.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match models
            .iter()
            .find(|(id, name)| id.to_string() == one || name == one)
        {
            Some(model) if !picked.contains(model) => picked.push(model.clone()),
            Some(_) => {}
            None => {
                return Err(ChapError::Usage(format!(
                    "chap-core has no configured model `{one}`; give --model one or more of: {}",
                    names()
                ))
                .into());
            }
        }
    }
    if picked.is_empty() {
        return Err(ChapError::Usage(format!(
            "--write needs --model, one or more of the configured models of chap-core: {}",
            names()
        ))
        .into());
    }
    Ok(picked)
}

fn table(ctx: &Ctx, runs: &[Backtest]) -> String {
    let rows: Vec<Vec<String>> = runs
        .iter()
        .map(|r| {
            let time = |s: Option<f64>| {
                s.map(|s| human_ms(s * 1000.0))
                    .unwrap_or_else(|| "-".to_string())
            };
            vec![
                match r.baseline {
                    true => "-".to_string(),
                    false => r.number.to_string(),
                },
                match r.baseline {
                    true => format!("{} (baseline)", r.model),
                    false => r.model.clone(),
                },
                r.status.clone(),
                time(r.request_seconds),
                time(r.queued_seconds),
                time(r.seconds),
                r.backtest_id.map(|id| id.to_string()).unwrap_or_default(),
                match (r.forecasts, r.expected_forecasts) {
                    (Some(got), Some(want)) => format!("{got}/{want}"),
                    (Some(got), None) => got.to_string(),
                    _ => String::new(),
                },
                r.metrics.map(|m| m.to_string()).unwrap_or_default(),
                match r.passed() {
                    true => "ok".to_string(),
                    false => "failed".to_string(),
                },
            ]
        })
        .collect();
    ctx.out.table(
        &[
            "#",
            "MODEL",
            "STATUS",
            "REQUEST",
            "QUEUED",
            "RUN",
            "BACKTEST",
            "FORECASTS",
            "METRICS",
            "RESULT",
        ],
        &rows,
    )
}
