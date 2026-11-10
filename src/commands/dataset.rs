//! `chap-devops bench dataset`: a synthetic dataset as files for `chap
//! eval`, or imported into chap-core for `bench --write`. See
//! `docs/bench.md`.

use crate::api::Api;
use crate::cli::DatasetArgs;
use crate::commands::Ctx;
use crate::commands::chap_jobs::{self, database_result, job_failure, post, wait};
use crate::error::{ChapError, Result};
use crate::jobs;
use crate::synthetic::{self, Dataset, Spec};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often the import job is asked whether it has finished.
const IMPORT_POLL: Duration = Duration::from_secs(2);

pub fn generate(ctx: &Ctx, args: &DatasetArgs) -> Result<()> {
    let spec = args.synthetic.spec();
    let name = args.name.clone().unwrap_or_else(|| spec.name());
    check_name(&name)?;
    let out = match (&args.out, args.import) {
        (Some(dir), _) => Some(dir.clone()),
        (None, false) => Some(PathBuf::from(".")),
        (None, true) => None,
    };
    // The address first: a wrong `--url` or a missing deployment is said
    // before a large dataset is made.
    let target = match args.import {
        true => Some(chap_jobs::chap_core(
            ctx,
            args.url.as_deref(),
            "`chap-devops bench dataset --import`",
        )?),
        false => None,
    };

    let data = synthetic::generate(&spec);
    let files = match &out {
        Some(dir) => Some(write_files(dir, &name, &data)?),
        None => None,
    };
    let imported = match &target {
        Some(target) => {
            let api = Api::new(
                &target.base,
                target.token.clone(),
                Duration::from_secs(args.timeout),
            );
            Some(import(
                ctx,
                &api,
                &data,
                &name,
                Duration::from_secs(args.timeout),
            )?)
        }
        None => None,
    };

    let value = serde_json::json!({
        "name": name,
        "locations": spec.locations,
        "periods": spec.periods,
        "period_type": format!("{:?}", spec.period_type).to_lowercase(),
        "covariates": synthetic::covariate_names(spec.covariates),
        "seed": spec.seed,
        "rows": data.rows(),
        "values": spec.values(),
        "files": files.as_ref().map(|(csv, geo)| [csv, geo]),
        "dataset_id": imported.as_ref().map(|i| i.dataset),
        "job_id": imported.as_ref().map(|i| i.job.clone()),
    });
    ctx.out.report(&value, |lines| {
        let what = describe(&spec, data.rows());
        if let Some((csv, geo)) = &files {
            lines.info(format!(
                "wrote {what} to `{}` and `{}`",
                csv.display(),
                geo.display()
            ));
        }
        match &imported {
            Some(imported) => {
                if files.is_none() {
                    lines.info(format!("made {what}"));
                }
                lines.info(format!(
                    "chap-core stored it as dataset {} (`{name}`) in {:.1} s",
                    imported.dataset, imported.seconds
                ));
                lines.hint(format!(
                    "`chap-devops bench --write --dataset {} --model MODEL` backtests it",
                    imported.dataset
                ));
            }
            None => {
                if let Some((csv, _)) = &files {
                    lines.hint(format!(
                        "`chap-devops chap eval --model-name MODEL --dataset-csv {} --output-file eval.nc` \
                         evaluates a model on it",
                        csv.display()
                    ));
                }
            }
        }
    })
}

/// `1,872 rows (18 locations, 104 weekly periods, 3 covariates)`.
pub(crate) fn describe(spec: &Spec, rows: usize) -> String {
    let kind = match spec.period_type {
        synthetic::PeriodType::Week => "weekly",
        synthetic::PeriodType::Month => "monthly",
    };
    format!(
        "{rows} rows ({} locations, {} {kind} periods, {} covariates)",
        spec.locations, spec.periods, spec.covariates
    )
}

/// A name that is also a file name: no directory in it.
fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(ChapError::Usage(format!(
            "`{name}` cannot be a file name; give --name a name without `/`, `\\` or a leading `.`"
        ))
        .into());
    }
    Ok(())
}

/// Write `<dir>/<name>.csv` and `<dir>/<name>.geojson`, the pair `chap eval`
/// reads.
fn write_files(dir: &Path, name: &str, data: &Dataset) -> Result<(PathBuf, PathBuf)> {
    std::fs::create_dir_all(dir)
        .map_err(|e| anyhow::anyhow!("could not create `{}`: {e}", dir.display()))?;
    let csv = dir.join(format!("{name}.csv"));
    let geo = dir.join(format!("{name}.geojson"));
    std::fs::write(&csv, synthetic::csv(data))
        .map_err(|e| anyhow::anyhow!("could not write `{}`: {e}", csv.display()))?;
    let text = serde_json::to_string(&synthetic::geojson(data))?;
    std::fs::write(&geo, text)
        .map_err(|e| anyhow::anyhow!("could not write `{}`: {e}", geo.display()))?;
    Ok((csv, geo))
}

/// What an import made.
pub(crate) struct Imported {
    pub dataset: i64,
    pub job: String,
    pub seconds: f64,
}

/// Send `data` to `make-dataset` as `name`, and wait until chap-core has
/// stored it.
pub(crate) fn import(
    ctx: &Ctx,
    api: &Api,
    data: &Dataset,
    name: &str,
    timeout: Duration,
) -> Result<Imported> {
    let started = Instant::now();
    let until = started + timeout;
    crate::output::notice(&format!(
        "importing {} rows into chap-core at {}",
        data.rows(),
        api.base()
    ));
    let body = synthetic::make_dataset_body(data, name);
    let answer = post(api, "/v1/analytics/make-dataset", &body)?;
    if answer.get("importedCount").and_then(|c| c.as_i64()) == Some(0) {
        let why = answer["rejected"][0]["reason"]
            .as_str()
            .unwrap_or("no reason given")
            .to_string();
        return Err(anyhow::anyhow!(
            "chap-core imported no location of `{name}` ({why}); run it again with -d to see the answer"
        ));
    }
    let job = answer
        .get("id")
        .and_then(|id| id.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "chap-core answered make-dataset without a job id; run it again with -d"
            )
        })?
        .to_string();
    match wait(ctx, api, &job, IMPORT_POLL, until)? {
        Some(status) if jobs::Outcome::of(&status) == jobs::Outcome::Done => {}
        Some(_) => {
            let (why, next) = job_failure(api, &job);
            return Err(anyhow::anyhow!(
                "chap-core could not store `{name}`: {why}; {next}"
            ));
        }
        None => {
            return Err(anyhow::anyhow!(
                "chap-core was still storing `{name}` after {} s; run `chap-devops jobs show {job}` \
                 to see where it got to, or give a longer --timeout",
                timeout.as_secs()
            ));
        }
    }
    let dataset = database_result(api, &job).ok_or_else(|| {
        anyhow::anyhow!(
            "chap-core stored `{name}` without saying which dataset it is; run `chap-devops jobs show {job}`"
        )
    })?;
    Ok(Imported {
        dataset,
        job,
        seconds: started.elapsed().as_secs_f64(),
    })
}
