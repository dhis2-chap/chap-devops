//! `chap-devops bench`: time the chap-core requests the Modeling App makes
//! when a person opens one evaluation, and say which are slow.
//!
//! It first asks chap-core about the backtest (its org units, its dataset, the
//! plots it has), untimed. Then it sends each request `--repeat` times, with
//! `--parallel` of them open at once, and reports min, p50, p95 and max per
//! request. With `--parallel` above one, a slow request that holds up the
//! others shows as a high p95 on requests that are fast alone.

mod measure;
mod plan;

use crate::api::Api;
use crate::cli::BenchArgs;
use crate::commands::Ctx;
use crate::error::Result;
use crate::project::Project;
use measure::{RouteResult, Timing, human_bytes, human_ms};
use plan::Route;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long the questions before the timing may take.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(60);

/// `chap-devops bench [BACKTEST]`.
pub fn run(ctx: &Ctx, args: &BenchArgs) -> Result<()> {
    let (base, project) = match &args.url {
        Some(url) => (url.trim().to_string(), Project::find(&ctx.project_dir).ok()),
        None => {
            let project = ctx.project()?;
            crate::components::require_chap_core(&project.state.components, "`chap-devops bench`")?;
            (project.api_url(), Some(project))
        }
    };
    let token = crate::api::token_for_url(&base, project.as_ref());
    let ask = Api::new(&base, token.clone(), DISCOVERY_TIMEOUT);
    ctx.out
        .verbose(&format!("asking chap-core at {}", ask.base()));

    let id = match args.backtest {
        Some(id) => id,
        None => plan::newest_backtest(&ask.get_json("/v1/crud/backtests")?).ok_or_else(|| {
            anyhow::anyhow!(
                "chap-core at {} has no backtest to time; run an evaluation in the Modeling App first",
                ask.base()
            )
        })?,
    };
    let info = ask
        .get_json(&format!("/v1/crud/backtests/{id}/info"))
        .map_err(|e| {
            anyhow::anyhow!(
                "could not read backtest {id} from chap-core ({e}); run `chap-devops bench` with no id to time the newest one"
            )
        })?;
    let target = plan::target(id, &info);
    let plots = discover_plots(&ask, id);
    let routes = plan::routes(&target, &plots);

    let timer = Api::new(&base, token, Duration::from_secs(args.timeout));
    let start = Instant::now();
    let timings = time_all(&timer, &routes, args.repeat, args.parallel);
    let seconds = start.elapsed().as_secs_f64();

    let results: Vec<RouteResult> = routes
        .iter()
        .zip(&timings)
        .map(|(r, t)| measure::summarize(&r.label, r.method, &r.path, t))
        .collect();

    if !ctx.out.json {
        print!("{}", table(ctx, &results));
    }
    let value = serde_json::json!({
        "url": timer.base(),
        "backtest": id,
        "repeat": args.repeat,
        "parallel": args.parallel,
        "seconds": seconds,
        "routes": results,
    });
    let local = args.url.is_none();
    ctx.out.report(&value, |lines| {
        lines.info(format!(
            "timed {} requests of backtest {id}, {}, {} at a time, in {}",
            results.len(),
            measure::times_each(args.repeat),
            args.parallel,
            human_ms(seconds * 1000.0)
        ));
        if let Some(slow) = results
            .iter()
            .filter(|r| r.failed < r.requests)
            .max_by(|a, b| a.p50_ms.total_cmp(&b.p50_ms))
        {
            lines.info(format!(
                "slowest: {}, p50 {}",
                slow.label,
                human_ms(slow.p50_ms)
            ));
        }
        for r in results.iter().filter(|r| r.failed > 0) {
            let why = r.error.clone().unwrap_or_default();
            let next = if local {
                "; `chap-devops logs chap` shows why"
            } else {
                ""
            };
            lines.warning(format!(
                "`{}` failed {} of {} times: {why}{next}",
                r.label, r.failed, r.requests
            ));
        }
        if args.parallel == 1 {
            lines.hint(
                "`--parallel 8` sends eight at a time, as the Modeling App does when a page opens",
            );
        }
    })
}

/// Each plot with the subplot body its facet coordinates give. A plot list
/// that cannot be read leaves the plots out: the other requests still tell
/// most of the story.
fn discover_plots(api: &Api, id: i64) -> Vec<(String, Option<String>)> {
    let Ok(list) = api.get_json("/v1/visualization/backtest-plots/") else {
        return Vec::new();
    };
    plan::plot_ids(&list)
        .into_iter()
        .map(|plot| {
            let body = api
                .get_json(&format!(
                    "/v1/visualization/backtest-plots/{plot}/{id}/facet-coords"
                ))
                .ok()
                .and_then(|coords| plan::subplot_body(&coords));
            (plot, body)
        })
        .collect()
}

/// Send every route `repeat` times, `parallel` at a time, and hand back the
/// timings of each route in route order.
fn time_all(api: &Api, routes: &[Route], repeat: u32, parallel: u32) -> Vec<Vec<Timing>> {
    // Round by round, so with one at a time the same request is not sent
    // twice in a row and a cache warmed by the first does not hide the cost.
    let queue: VecDeque<usize> = (0..repeat).flat_map(|_| 0..routes.len()).collect();
    let queue = Arc::new(Mutex::new(queue));
    let done: Arc<Mutex<Vec<Vec<Timing>>>> = Arc::new(Mutex::new(vec![Vec::new(); routes.len()]));
    std::thread::scope(|scope| {
        for _ in 0..parallel {
            let (queue, done) = (queue.clone(), done.clone());
            scope.spawn(move || {
                loop {
                    let next = queue.lock().ok().and_then(|mut q| q.pop_front());
                    let Some(i) = next else { break };
                    let timing = time_one(api, &routes[i]);
                    if let Ok(mut done) = done.lock() {
                        done[i].push(timing);
                    }
                }
            });
        }
    });
    Arc::try_unwrap(done)
        .ok()
        .and_then(|m| m.into_inner().ok())
        .unwrap_or_default()
}

fn time_one(api: &Api, route: &Route) -> Timing {
    let start = Instant::now();
    let answer = api.send(
        route.method,
        &route.path,
        route.body.as_deref().map(str::as_bytes),
    );
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    match answer {
        Ok(answer) => Timing {
            ms,
            status: Some(answer.status),
            bytes: answer.body.len(),
            error: (!answer.is_success())
                .then(|| format!("{}: {}", answer.status_line(), answer.detail())),
        },
        Err(e) => Timing {
            ms,
            status: None,
            bytes: 0,
            error: Some(
                e.to_string()
                    .lines()
                    .next()
                    .unwrap_or("no answer")
                    .to_string(),
            ),
        },
    }
}

fn table(ctx: &Ctx, results: &[RouteResult]) -> String {
    let rows: Vec<Vec<String>> = results
        .iter()
        .map(|r| {
            vec![
                r.label.clone(),
                format!("{}", r.requests),
                format!("{}", r.failed),
                human_ms(r.min_ms),
                human_ms(r.p50_ms),
                human_ms(r.p95_ms),
                human_ms(r.max_ms),
                human_bytes(r.bytes),
            ]
        })
        .collect();
    ctx.out.table(
        &["REQUEST", "N", "FAILED", "MIN", "P50", "P95", "MAX", "SIZE"],
        &rows,
    )
}

#[cfg(test)]
mod tests;
