//! `chap-devops restart --api-workers N`: set how many API worker processes
//! chap-core runs.
//!
//! The number is `CHAP_API_WORKERS` in `.env`, which the `chap` service gets
//! as `WEB_CONCURRENCY`. Setting it writes that one line and recreates only
//! the `chap` service: the worker, the database and the models keep
//! running. The report gives the processes that run, counted with
//! `docker compose top`, not the setting.

use crate::api::Api;
use crate::commands::Ctx;
use crate::error::{ChapError, Result};
use crate::project::{API_WORKERS_ENV_VAR, BASE_COMPOSE, ENV_FILE, Project};
use std::time::{Duration, Instant};

/// How long `chap` may take to answer again after it was recreated.
const READY_TIMEOUT: Duration = Duration::from_secs(180);

/// `chap-devops restart --api-workers N`.
pub fn run(ctx: &Ctx, count: u32) -> Result<()> {
    let project = ctx.project()?;
    crate::components::require_chap_core(
        &project.state.components,
        "`chap-devops restart --api-workers`",
    )?;
    let running = set_workers(ctx, &project, count)?;
    let value = serde_json::json!({"api_workers": count, "running": running});
    ctx.out.report(&value, |lines| {
        lines.info(format!(
            "chap-core runs {running} API worker{}; set {API_WORKERS_ENV_VAR}={count} in `{ENV_FILE}` \
             and recreated only `chap`",
            plural(running)
        ));
        if running != count as usize {
            lines.warning(format!(
                "chap-core runs {running} API workers, not {count}; `chap-devops logs chap` shows why"
            ));
        }
    })
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// The API worker processes that run now, or `None` when chap-core does
/// not run or docker cannot say.
pub fn running(project: &Project) -> Option<usize> {
    let args = ["top", "chap"].map(str::to_string);
    match crate::docker::compose_output(project, &args) {
        Ok((0, text, _)) => crate::commands::bench::server::workers(&text, "chap"),
        _ => None,
    }
}

/// Set the API workers to `count`, recreate `chap`, wait until it answers,
/// and give the worker processes that run then.
pub fn set_workers(ctx: &Ctx, project: &Project, count: u32) -> Result<usize> {
    if count == 0 {
        return Err(ChapError::Usage("give `--api-workers` a number of 1 or more".into()).into());
    }
    // More than one worker needs a chap-core that maps WEB_CONCURRENCY in
    // its own compose file (newer than v2.7.0), as `chap-devops doctor`
    // checks.
    let base = std::fs::read_to_string(project.dir.join(BASE_COMPOSE)).ok();
    let line = format!("{API_WORKERS_ENV_VAR}={count}\n");
    let (status, _, next) =
        crate::commands::doctor::api_workers_verdict(Some(&line), base.as_deref());
    if status != crate::commands::doctor::Status::Ok {
        return Err(anyhow::anyhow!(
            "this chap-core is v2.7.0 or older, and its API workers would share database connections; {}",
            next.unwrap_or_default()
        ));
    }

    let path = project.dir.join(ENV_FILE);
    let body = std::fs::read_to_string(&path).map_err(|e| {
        anyhow::anyhow!(
            "could not read `{}` ({e}); `--api-workers` writes {API_WORKERS_ENV_VAR} there",
            path.display()
        )
    })?;
    let mut lines: Vec<String> = body.lines().map(str::to_string).collect();
    if crate::dotenv::set(&mut lines, API_WORKERS_ENV_VAR, &count.to_string())
        == crate::dotenv::Wrote::Absent
    {
        lines.push(format!("{API_WORKERS_ENV_VAR}={count}"));
    }
    crate::dotenv::write(&path, &crate::dotenv::join(&lines))?;

    crate::output::notice(&format!("recreating `chap` with {count} API workers"));
    let args = ["up", "-d", "--no-deps", "--force-recreate", "chap"].map(str::to_string);
    match crate::docker::compose_output(project, &args) {
        Ok((0, _, _)) => {}
        Ok((_, _, err)) => {
            return Err(anyhow::anyhow!(
                "docker compose could not recreate `chap`: {}; run `chap-devops up`",
                err.lines().last().unwrap_or("no reason given")
            ));
        }
        Err(e) => return Err(e),
    }

    let api = Api::new(
        &project.api_url(),
        crate::api::token_for(Some(&project.dir)),
        Duration::from_secs(5),
    );
    let started = Instant::now();
    ctx.out
        .verbose(&format!("waiting for chap-core at {}", api.base()));
    loop {
        if matches!(api.send("GET", "/health", None), Ok(a) if a.is_success()) {
            break;
        }
        if started.elapsed() >= READY_TIMEOUT {
            return Err(anyhow::anyhow!(
                "chap-core did not answer {} s after `chap` was recreated; `chap-devops logs chap` shows why",
                READY_TIMEOUT.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    running(project).ok_or_else(|| {
        anyhow::anyhow!("chap-core answers, but docker did not say how many API workers run; run `chap-devops doctor`")
    })
}
