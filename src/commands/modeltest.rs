//! `chap-devops models test` — make a model do the work, and say whether it could.
//!
//! `chap-devops status` and `chap-devops doctor` can both be fully green while a model
//! cannot produce a single prediction: registration is a heartbeat, and a
//! heartbeat says the service is alive, not that its runtime works. This
//! command is the check neither of them can make, at two levels.
//!
//! The model level runs `chapkit test` inside the model's own container.
//! Every chapkit-built image ships it: it reads the service's own config
//! schema, generates data that matches the covariates, period type and
//! geometry the service declares, then validates, trains and predicts. It
//! needs no chap-core at all, which is what makes it the level to reach for
//! when chap-core is the thing that is broken.
//!
//! The backtest level goes the whole way round: chapkit's sample data comes
//! out through chap-core's proxy, goes back in as a dataset, and a two-split
//! backtest is run over it. That is the path the Modeling App takes, so it is
//! the one that proves a model is usable rather than merely runnable.
//!
//! Both levels clean up after themselves, and `--keep` says what was kept.
//! The rules for parsing, transposing and rendering are in
//! [`crate::modeltest`]; this module holds the requests and the `exec`.

mod backtest;
mod cleanup;
mod configured;
mod model;

use crate::api::Api;
use crate::cli::ModelsTestArgs;
use crate::commands::Ctx;
use crate::docker;
use crate::error::{ChapError, Result};
use crate::modeltest::{self, Level, Run, Verdict};
use crate::output::{self, Out, Report};
use crate::project::{EnabledModel, Project};
use backtest::backtest_level;
use model::model_level;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// A model service's own address inside its container.
///
/// Not the published host port and not the compose DNS name: `chapkit test`
/// and the `curl` that cleans up both run inside the container, and this is
/// the one address that is right whether or not the model publishes a port.
const SERVICE_URL: &str = "http://127.0.0.1:8000";

/// Where a model's own API answers inside its container: [`SERVICE_URL`],
/// except for a model registered with a chap-core elsewhere, which listens on
/// its host port in there too (see `docs/components.md`, "A chap-core
/// elsewhere").
fn service_url(project: &Project, service_id: &str) -> String {
    if project.state.components.chap_core_external.is_none() {
        return SERVICE_URL.to_string();
    }
    project
        .state
        .models
        .values()
        .find(|m| m.service_id == service_id)
        .and_then(|m| m.host_port)
        .map(|port| format!("http://127.0.0.1:{port}"))
        .unwrap_or_else(|| SERVICE_URL.to_string())
}

/// How long one request to chap-core may take.
///
/// Longer than [`crate::api::DEFAULT_TIMEOUT`]: `make-dataset` is a few
/// thousand observations going over the wire, and `$generate-sample-data`
/// makes the model service compute a frame before it answers.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// What the process exits with when a model failed.
///
/// Everything there was to say is on the screen already, so there is no
/// `error:` line on top of it - the same reasoning as `chap-devops doctor`.
const EXIT_FAILED: i32 = 1;

/// `chap-devops models test [ID..] [--all]`.
pub fn run(ctx: &Ctx, args: &ModelsTestArgs) -> Result<()> {
    let project = ctx.project()?;
    // The model level runs `chapkit test` in the model's own container and
    // asks chap-core only for two best-effort details, so a model running on
    // its own can be tested; the backtest is chap-core's to run.
    if args.backtest {
        crate::components::require_chap_core(
            &project.state.components,
            "`chap-devops models test --backtest`",
        )?;
    }
    let token = crate::api::token_for(Some(&project.dir));
    let api = Api::new(&project.api_url(), token, REQUEST_TIMEOUT);
    let targets = targets(ctx, &project, &api, args)?;
    let level = if args.backtest {
        Level::Backtest
    } else {
        Level::Model
    };
    let timeout = Duration::from_secs(args.timeout.unwrap_or(match level {
        Level::Model => modeltest::MODEL_TIMEOUT,
        Level::Backtest => modeltest::BACKTEST_TIMEOUT,
    }));

    ctx.out
        .verbose(&format!("asking chap-core at {}", api.base()));

    let names: Vec<String> = targets
        .iter()
        .map(|(_, enabled)| enabled.service_id.clone())
        .collect();
    let width = modeltest::name_width(&names);
    if !ctx.out.json {
        println!("{}", ctx.out.dim(&modeltest::header(targets.len(), level)));
    }

    // chap-core 2.4 and later makes no configured model from a registration,
    // and a backtest names one; the step gives each enabled model its own
    // first. A model registered from outside is not this deployment's to
    // configure.
    let configured = match level {
        Level::Backtest => {
            let mine: Vec<crate::configs::sync::Target> = targets
                .iter()
                .filter(|(id, _)| project.state.models.contains_key(id))
                .map(|(id, enabled)| crate::configs::sync::Target {
                    id: id.clone(),
                    service_id: enabled.service_id.clone(),
                    managed: true,
                })
                .collect();
            (!mine.is_empty()).then(|| match super::configs::sync::step(ctx, &project, &mine) {
                Ok(models) => crate::configs::sync::Folded {
                    models,
                    error: None,
                },
                Err(err) => crate::configs::sync::Folded {
                    models: Vec::new(),
                    error: Some(first_line(&err.to_string())),
                },
            })
        }
        Level::Model => None,
    };

    // One `docker compose ps` for the whole run: a container that was not up
    // when the command started is not one this run can test.
    let running = if level == Level::Model {
        docker::running_services(&project)
    } else {
        BTreeSet::new()
    };

    let mut runs: Vec<Run> = Vec::with_capacity(targets.len());
    for (id, enabled) in &targets {
        let started = Instant::now();
        let mut run = match level {
            Level::Model => model_level(ctx, &project, &api, id, enabled, args, timeout, &running),
            Level::Backtest => backtest_level(ctx, &api, id, enabled, args, timeout),
        };
        run.seconds = started.elapsed().as_secs();
        if !ctx.out.json {
            print_run(&ctx.out, &run, width);
        }
        runs.push(run);
    }

    if !ctx.out.json {
        println!();
    }
    let failed = modeltest::any_failed(&runs) || modeltest::none_tested(&runs);
    let value = serde_json::json!({
        "ok": !failed,
        "models": runs,
        "configured": configured,
    });
    ctx.out.report(&value, |lines| {
        super::configs::sync::fold(&configured, false, lines);
        closing(&runs, lines);
    })?;
    if failed {
        std::process::exit(EXIT_FAILED);
    }
    Ok(())
}

/// The line the run ends on: how many passed. The way to see the full output
/// of a failure is already under its row, so here it is a hint. A run that
/// tested no model at all says so as a warning, because it proved nothing.
fn closing(runs: &[Run], lines: &mut Report) {
    for message in runs.iter().flat_map(|run| &run.cleanup) {
        match message.level {
            output::Level::Info => lines.info(message.text.as_str()),
            output::Level::Hint => lines.hint(message.text.as_str()),
            output::Level::Warning => lines.warning(message.text.as_str()),
        };
    }
    let (count, more) = output::split_hint(&modeltest::closing(runs));
    lines.info(count);
    if let Some(more) = more {
        lines.hint(more);
    }
    if modeltest::none_tested(runs) {
        lines.warning(modeltest::NONE_TESTED);
    }
}

/// One row, and the way out under it when there is one.
fn print_run(out: &Out, run: &Run, width: usize) {
    let verdict = match run.verdict {
        Verdict::Pass => out.ok(run.verdict.label()),
        Verdict::Fail => out.bad(run.verdict.label()),
        Verdict::Skip => out.warn(run.verdict.label()),
    };
    println!("{}", modeltest::row(run, &verdict, width));
    // Which configuration the scores are of, so that they are never read as
    // the scores of another one.
    if let Some(used) = &run.configured_model {
        println!("  {}", used.line());
    }
    if let Some(detail) = &run.detail {
        println!("  {detail}");
    }
}

/// The models this run is about, in the order they will be tested.
///
/// Ids are resolved the way `models enable` and `models info` resolve them -
/// a marketplace id or a service id, a manual definition included - and a
/// model that is not enabled here is a mistake rather than a skip: there is
/// no container to test and no registration to go through.
fn targets(
    ctx: &Ctx,
    project: &Project,
    api: &Api,
    args: &ModelsTestArgs,
) -> Result<Vec<(String, EnabledModel)>> {
    if args.all {
        let all: Vec<(String, EnabledModel)> = project
            .state
            .models
            .iter()
            .map(|(id, enabled)| (id.clone(), enabled.clone()))
            .collect();
        if all.is_empty() {
            // A model run from its checkout registers without being enabled
            // here, and is exactly what its developer wants tested.
            let outside = unmanaged_services(project, api);
            return Err(match outside.as_slice() {
                [] => anyhow::anyhow!(
                    "this deployment enables no models; enable one with `chap-devops models enable ID`"
                ),
                [service] => anyhow::anyhow!(
                    "this deployment enables no models; `chap-devops models test {service} --backtest` \
                     tests the one registered from outside it through chap-core"
                ),
                many => anyhow::anyhow!(
                    "this deployment enables no models; `chap-devops models test ID --backtest` tests \
                     one of the {} registered from outside it through chap-core ({})",
                    many.len(),
                    many.join(", ")
                ),
            });
        }
        return Ok(all);
    }
    if args.ids.is_empty() {
        return Err(ChapError::Usage(
            "name a model to test, or pass --all for every model this deployment enables"
                .to_string(),
        )
        .into());
    }

    let registry = super::registry_for(ctx, Some(project))?;
    let outside = unmanaged_services(project, api);
    let mut picked: Vec<(String, EnabledModel)> = Vec::new();
    for given in &args.ids {
        if outside.contains(given) {
            picked.push((given.clone(), unmanaged(given)));
            continue;
        }
        let model = registry
            .get(given)
            .ok_or_else(|| ChapError::UnknownModel(given.clone()))?;
        let enabled = project.state.models.get(&model.id).ok_or_else(|| {
            anyhow::anyhow!(
                "{} is not enabled in this deployment; enable it with `chap-devops models enable {}`",
                model.id,
                model.id
            )
        })?;
        if picked.iter().any(|(id, _)| id == &model.id) {
            continue;
        }
        picked.push((model.id.clone(), enabled.clone()));
    }
    Ok(picked)
}

/// The services chap-core has registered that this deployment does not
/// enable: a model run from its checkout, typically. Empty without chap-core,
/// or when it cannot be asked.
pub(super) fn unmanaged_services(project: &Project, api: &Api) -> Vec<String> {
    if !project.state.components.has_chap_core_api() {
        return Vec::new();
    }
    let Ok(answer) = api.send("GET", "/v2/services", None) else {
        return Vec::new();
    };
    let Ok(services) = crate::status::parse_services(&answer.text()) else {
        return Vec::new();
    };
    services
        .into_iter()
        .map(|s| s.id)
        .filter(|id| !project.state.models.values().any(|m| &m.service_id == id))
        .collect()
}

/// A target for a service this deployment does not run: only its service id
/// means anything, and the empty overlay is what marks it.
fn unmanaged(service_id: &str) -> EnabledModel {
    EnabledModel {
        previous: None,
        reads_port: false,
        service_id: service_id.to_string(),
        image: String::new(),
        image_tag: String::new(),
        image_digest: None,
        version: String::new(),
        channel: None,
        host_port: None,
        bind: None,
        data_dir: String::new(),
        user: String::new(),
        user_from: Default::default(),
        platform: None,
        compose_file: String::new(),
    }
}

/// `GET /v2/services/{id}` — the `info` block the service registered with.
///
/// `None` both for a service chap-core has never seen and for a chap-core
/// that cannot be reached: neither is worth an error of its own here, and the
/// caller says what it cost.
fn service_info(ctx: &Ctx, api: &Api, service_id: &str) -> Option<serde_json::Value> {
    let path = format!("/v2/services/{}", crate::api::encode(service_id));
    match api.send("GET", &path, None) {
        Ok(answer) if answer.is_success() => answer.json()?.get("info").cloned(),
        Ok(answer) => {
            ctx.out.verbose(&format!(
                "{service_id}: {} answered {}",
                api.url(&path),
                answer.status_line()
            ));
            None
        }
        Err(err) => {
            ctx.out
                .verbose(&format!("{service_id}: {}", first_line(&err.to_string())));
            None
        }
    }
}

/// The period type the generated data has to be in.
///
/// A service that declares `any` takes both, and monthly is what chapkit's
/// generator and chap-core's dataset both default to.
fn period_type(info: Option<&serde_json::Value>) -> String {
    match info
        .and_then(|info| info.get("period_type"))
        .and_then(|value| value.as_str())
        .unwrap_or("monthly")
    {
        "weekly" => "weekly".to_string(),
        _ => "monthly".to_string(),
    }
}

/// The first line of a message, for a cell that has room for one.
pub(crate) fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_string()
}
