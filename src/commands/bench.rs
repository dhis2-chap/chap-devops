//! `chap-devops bench`: time the chap-core requests the Modeling App sends
//! when a person opens a page, and say which are slow.
//!
//! It first asks chap-core about the backtest and the prediction (their org
//! units, dataset, specification, the plots), untimed. Then it sends each
//! request `--repeat` times, with `--parallel` of them open at once, and
//! reports min, p50, p95 and max per request, and the time each page takes
//! to open. With `--parallel` above one, a slow request that holds up the
//! others shows as a high p95 on requests that are fast alone.

mod measure;
mod plan;
mod write;

use crate::api::Api;
use crate::cli::{BenchArgs, BenchPage};
use crate::commands::Ctx;
use crate::error::Result;
use measure::{RouteResult, Timing, human_bytes, human_ms};
use plan::{Facts, Route};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long the questions before the timing may take.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(60);

/// `chap-devops bench [BACKTEST]`.
pub fn run(ctx: &Ctx, args: &BenchArgs) -> Result<()> {
    if let Some(crate::cli::BenchSub::Dataset(dataset)) = &args.command {
        return super::dataset::generate(ctx, dataset);
    }
    let chap = super::chap_jobs::chap_core(ctx, args.url.as_deref(), "`chap-devops bench`")?;
    if args.write {
        return write::run(ctx, args, &chap);
    }
    let (base, token) = (chap.base.clone(), chap.token.clone());
    let ask = Api::new(&base, token.clone(), DISCOVERY_TIMEOUT);
    ctx.out
        .verbose(&format!("asking chap-core at {}", ask.base()));

    let asked = !args.page.is_empty();
    let pages: Vec<BenchPage> = match asked {
        true => {
            let mut pages = args.page.clone();
            pages.sort();
            pages.dedup();
            pages
        }
        false => BenchPage::ALL.to_vec(),
    };
    let facts = discover(&ask, args, &pages)?;
    let skipped: Vec<(BenchPage, &str)> = pages
        .iter()
        .filter_map(|&p| plan::missing(p, &facts).map(|why| (p, why)))
        .collect();
    let routes = plan::routes(&facts, &pages);
    if routes.is_empty() {
        if facts.backtest.is_none() {
            return Err(anyhow::anyhow!(plan::no_backtest(ask.base())));
        }
        let why: Vec<String> = skipped
            .iter()
            .map(|(p, why)| format!("{} page: {why}", p.name()))
            .collect();
        return Err(anyhow::anyhow!("nothing to time ({})", why.join("; ")));
    }

    let timer = Api::new(&base, token, Duration::from_secs(args.timeout));
    let start = Instant::now();
    let timings = time_all(&timer, &routes, args.repeat, args.parallel);
    let seconds = start.elapsed().as_secs_f64();

    let results: Vec<RouteResult> = routes
        .iter()
        .zip(&timings)
        .map(|(r, t)| RouteResult {
            page: r.page.name().to_string(),
            step: r.step,
            ..measure::summarize(&r.label, r.method, &r.path, t)
        })
        .collect();
    let p50: Vec<f64> = results.iter().map(|r| r.p50_ms).collect();
    let timed: Vec<BenchPage> = pages
        .iter()
        .copied()
        .filter(|p| routes.iter().any(|r| r.page == *p))
        .collect();
    let page_rows: Vec<serde_json::Value> = timed
        .iter()
        .map(|&p| {
            serde_json::json!({
                "page": p.name(),
                "steps": routes.iter().filter(|r| r.page == p).map(|r| r.step).max().unwrap_or(0) + 1,
                "ms": plan::page_ms(&routes, &p50, p),
            })
        })
        .collect();

    if !ctx.out.json {
        print!("{}", table(ctx, &results));
    }
    let backtest = facts.backtest.as_ref().map(|b| b.id);
    let prediction = facts.prediction.as_ref().map(|p| p.id);
    let value = serde_json::json!({
        "url": timer.base(),
        "backtest": backtest,
        "prediction": prediction,
        "repeat": args.repeat,
        "parallel": args.parallel,
        "seconds": seconds,
        "pages": page_rows,
        "skipped": skipped.iter().map(|(p, why)| serde_json::json!({"page": p.name(), "reason": why})).collect::<Vec<_>>(),
        "routes": results,
    });
    let local = chap.local;
    ctx.out.report(&value, |lines| {
        let of: Vec<String> = [
            backtest.map(|id| format!("backtest {id}")),
            prediction.map(|id| format!("prediction {id}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        lines.info(format!(
            "timed {} requests on {} page{} ({}), {}, {} at a time, in {}",
            results.len(),
            timed.len(),
            if timed.len() == 1 { "" } else { "s" },
            of.join(", "),
            measure::times_each(args.repeat),
            args.parallel,
            human_ms(seconds * 1000.0)
        ));
        let opens: Vec<String> = timed
            .iter()
            .map(|&p| format!("{} {}", p.name(), human_ms(plan::page_ms(&routes, &p50, p))))
            .collect();
        lines.info(format!("time to open, from the p50s: {}", opens.join(", ")));
        if let Some(slow) = results
            .iter()
            .filter(|r| r.failed < r.requests)
            .max_by(|a, b| a.p50_ms.total_cmp(&b.p50_ms))
        {
            lines.info(format!(
                "slowest: {} {}, p50 {}",
                slow.page,
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
                "`{} {}` failed {} of {} times: {why}{next}",
                r.page, r.label, r.failed, r.requests
            ));
        }
        for (page, why) in &skipped {
            let line = format!("did not time the {} page: {why}", page.name());
            match asked {
                true => lines.warning(line),
                false => lines.hint(line),
            };
        }
        if args.parallel == 1 {
            lines.hint(
                "`--parallel 8` sends eight at a time, as the Modeling App does when a page opens",
            );
        }
    })
}

/// What the pages need to know, asked untimed.
///
/// A backtest or a prediction that was named and cannot be read is an error.
/// One that was not named and is not there leaves its pages out.
fn discover(api: &Api, args: &BenchArgs, pages: &[BenchPage]) -> Result<Facts> {
    let wants = |page: BenchPage| pages.contains(&page);
    let mut facts = Facts {
        version: api
            .get_json("/system/info")
            .ok()
            .and_then(|info| plan::version(&info)),
        ..Facts::default()
    };
    let backtest_pages = [
        BenchPage::Evaluation,
        BenchPage::Compare,
        BenchPage::Plots,
        BenchPage::Benchmark,
        BenchPage::Dataset,
    ];
    if backtest_pages.iter().any(|&p| wants(p)) {
        let id = match args.backtest {
            Some(id) => Some(id),
            None => plan::newest_id(&api.get_json("/v1/crud/backtests")?),
        };
        if let Some(id) = id {
            let info = api
                .get_json(&format!("/v1/crud/backtests/{id}/info"))
                .map_err(|e| {
                    anyhow::anyhow!(
                        "could not read backtest {id} from chap-core ({e}); run `chap-devops bench` with no id to time the newest one"
                    )
                })?;
            facts.backtest = Some(plan::backtest(id, &info));
        }
    }
    let spec_id = facts.backtest.as_ref().and_then(|b| b.specification_id);
    if let Some(spec) = spec_id
        && [
            BenchPage::Evaluation,
            BenchPage::Compare,
            BenchPage::Benchmark,
        ]
        .iter()
        .any(|&p| wants(p))
    {
        facts.specification = api
            .get_json(&format!(
                "/v1/crud/backtest-specifications/{spec}?includeComparisonModels=true&includeArchivedModels=false"
            ))
            .ok()
            .map(|value| plan::specification(spec, &value));
    }
    if wants(BenchPage::Plots)
        && let Some(id) = facts.backtest.as_ref().map(|b| b.id)
    {
        facts.plots = discover_plots(api, id);
    }
    if wants(BenchPage::Prediction) {
        let id = match args.prediction {
            Some(id) => Some(id),
            None => api
                .get_json("/v1/crud/predictions")
                .ok()
                .and_then(|list| plan::newest_id(&list)),
        };
        if let Some(id) = id {
            let row = api.get_json(&format!("/v1/crud/predictions/{id}"));
            match (row, args.prediction) {
                (Ok(row), _) => facts.prediction = Some(plan::prediction(id, &row)),
                (Err(e), Some(_)) => {
                    return Err(anyhow::anyhow!(
                        "could not read prediction {id} from chap-core ({e}); run `chap-devops bench` without --prediction to time the newest one"
                    ));
                }
                (Err(_), None) => {}
            }
        }
    }
    Ok(facts)
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
            wire_bytes: answer.wire_bytes,
            error: (!answer.is_success())
                .then(|| format!("{}: {}", answer.status_line(), answer.detail())),
        },
        Err(e) => Timing {
            ms,
            status: None,
            bytes: 0,
            wire_bytes: 0,
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
                r.page.clone(),
                r.label.clone(),
                format!("{}", r.requests),
                format!("{}", r.failed),
                human_ms(r.min_ms),
                human_ms(r.p50_ms),
                human_ms(r.p95_ms),
                human_ms(r.max_ms),
                human_bytes(r.bytes),
                human_bytes(r.wire_bytes),
            ]
        })
        .collect();
    ctx.out.table(
        &[
            "PAGE", "REQUEST", "N", "FAILED", "MIN", "P50", "P95", "MAX", "SIZE", "GZIP",
        ],
        &rows,
    )
}

#[cfg(test)]
mod tests;
