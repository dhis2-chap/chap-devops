//! Which requests `chap-devops bench` sends: the ones the Modeling App sends
//! to chap-core when a person opens a page, built from what chap-core says
//! about the backtest and the prediction.
//!
//! The pages and their requests follow the Modeling App
//! (`chap-frontend/apps/modeling-app`). Each page is a list of steps: the
//! requests in one step go out at the same time, and a step starts when the
//! step before it has its answers. Requests to DHIS2 itself (org unit names,
//! system settings) are left out: they do not reach chap-core.

use crate::cli::BenchPage;
use serde_json::Value;

/// The quantiles the Modeling App asks for.
const QUANTILES: &str = "quantiles=0.1&quantiles=0.25&quantiles=0.5&quantiles=0.75&quantiles=0.9";

/// How many org units the compare page selects when it opens.
pub const PAGE_ORG_UNITS: usize = 10;

/// One request to time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub page: BenchPage,
    /// The step of the page: requests of one step go out together.
    pub step: u8,
    /// What the table calls it.
    pub label: String,
    pub method: &'static str,
    /// Path and query string.
    pub path: String,
    /// A JSON body, for a POST.
    pub body: Option<String>,
}

/// What the requests of the pages need to know, from the questions that are
/// not timed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// chap-core's version as `(major, minor, patch)`. `None` when
    /// `/system/info` did not say: then every request is sent, as to the
    /// newest chap-core.
    pub version: Option<(u64, u64, u64)>,
    pub backtest: Option<Backtest>,
    pub specification: Option<Specification>,
    pub prediction: Option<Prediction>,
    /// Each plot with the subplot body its facet coordinates give.
    pub plots: Vec<(String, Option<String>)>,
}

/// What `GET /v1/crud/backtests/{id}/info` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backtest {
    pub id: i64,
    pub dataset_id: Option<i64>,
    pub specification_id: Option<i64>,
    pub org_units: Vec<String>,
    /// Whether it has aggregate metrics, which decides the metrics request
    /// of a chap-core before 2.6.
    pub has_metrics: bool,
}

/// What `GET /v1/crud/backtest-specifications/{id}` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Specification {
    pub id: i64,
    /// Whether it carries `metrics` itself (chap-core 2.6 and later).
    pub has_metrics: bool,
    /// The first backtest of the specification.
    pub first_backtest: Option<i64>,
    /// The backtests of the baselines that are done.
    pub baselines_done: Vec<i64>,
}

/// What `GET /v1/crud/predictions/{id}` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prediction {
    pub id: i64,
    pub dataset_id: Option<i64>,
    /// The org units of the prediction, else those of its dataset, as the
    /// prediction page picks them.
    pub org_units: Vec<String>,
}

/// Why there is no newest backtest: the list is empty. chap-core leaves the
/// backtests of its built-in baselines out of the list, as the Modeling App
/// does, so they can still be there and be timed by id.
pub fn no_backtest(base: &str) -> String {
    format!(
        "chap-core at {base} lists no backtest to time; it does not list the backtests of the \
         built-in baselines (seasonal_median, global_median, persistence, \
         seasonal_shift_persistence), so give the id of one of those with `chap-devops bench ID`, \
         or run an evaluation of another model first"
    )
}

/// The highest id in a list answer such as `GET /v1/crud/backtests`.
pub fn newest_id(list: &Value) -> Option<i64> {
    list.as_array()?
        .iter()
        .filter_map(|b| b.get("id")?.as_i64())
        .max()
}

/// `(major, minor, patch)` from `chap_core_version` in `/system/info`:
/// `2.7.1`, `v2.7.1` and `2.7.1.dev3` all give `(2, 7, 1)`.
pub fn version(info: &Value) -> Option<(u64, u64, u64)> {
    let text = info.get("chap_core_version")?.as_str()?;
    let mut parts = text.trim().trim_start_matches('v').split('.').map(|p| {
        p.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u64>()
            .ok()
    });
    Some((
        parts.next()??,
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
    ))
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|u| u.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn int(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn nested_id(value: &Value, key: &str) -> Option<i64> {
    value
        .get(key)
        .and_then(|d| d.get("id"))
        .and_then(Value::as_i64)
}

fn not_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(_) => true,
    }
}

/// The backtest from `GET /v1/crud/backtests/{id}/info`.
pub fn backtest(id: i64, info: &Value) -> Backtest {
    Backtest {
        id,
        dataset_id: int(info, "datasetId").or_else(|| nested_id(info, "dataset")),
        specification_id: int(info, "specificationId"),
        org_units: strings(info.get("orgUnits")),
        has_metrics: not_empty(info.get("aggregateMetrics")),
    }
}

/// The specification from `GET /v1/crud/backtest-specifications/{id}`.
pub fn specification(id: i64, spec: &Value) -> Specification {
    let backtest_id = |b: &Value| int(b, "backtestId").or_else(|| int(b, "id"));
    Specification {
        id,
        has_metrics: spec.get("metrics").is_some_and(|m| !m.is_null()),
        first_backtest: spec
            .get("backtests")
            .and_then(Value::as_array)
            .and_then(|b| b.first())
            .and_then(backtest_id),
        baselines_done: spec
            .get("baselines")
            .and_then(Value::as_array)
            .map(|baselines| {
                baselines
                    .iter()
                    .filter(|b| b.get("status").and_then(Value::as_str) == Some("done"))
                    .filter_map(backtest_id)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// The prediction from `GET /v1/crud/predictions/{id}`.
pub fn prediction(id: i64, row: &Value) -> Prediction {
    let own = strings(row.get("orgUnits"));
    let org_units = if own.is_empty() {
        strings(row.get("dataset").and_then(|d| d.get("orgUnits")))
    } else {
        own
    };
    Prediction {
        id,
        dataset_id: nested_id(row, "dataset").or_else(|| int(row, "datasetId")),
        org_units,
    }
}

/// The plot ids in a `GET /v1/visualization/backtest-plots/` answer.
pub fn plot_ids(list: &Value) -> Vec<String> {
    list.as_array()
        .map(|plots| {
            plots
                .iter()
                .filter_map(|p| p.get("id")?.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The body for one subplot: the first value of each facet dimension, which
/// is the panel the Modeling App draws first. `None` when a dimension has no
/// value to pick.
pub fn subplot_body(coords: &Value) -> Option<String> {
    let dims = coords.as_object()?;
    let mut body = serde_json::Map::new();
    for (dim, values) in dims {
        let first = values.as_array()?.first()?.clone();
        body.insert(dim.clone(), first);
    }
    Some(Value::Object(body).to_string())
}

/// Why a page cannot be timed, or `None` when it can.
pub fn missing(page: BenchPage, facts: &Facts) -> Option<&'static str> {
    let backtest = facts.backtest.as_ref();
    match page {
        BenchPage::Evaluation | BenchPage::Compare | BenchPage::Plots if backtest.is_none() => {
            Some("there is no backtest")
        }
        BenchPage::Benchmark if facts.specification.is_none() => {
            Some("the backtest has no backtest specification")
        }
        BenchPage::Dataset if backtest.and_then(|b| b.dataset_id).is_none() => {
            Some("the backtest names no dataset")
        }
        BenchPage::Prediction => match &facts.prediction {
            None => Some("there is no prediction"),
            Some(p) if p.dataset_id.is_none() => Some("the prediction names no dataset"),
            Some(_) => None,
        },
        _ => None,
    }
}

/// Builds the requests of one page, step by step.
struct Steps {
    page: BenchPage,
    step: u8,
    routes: Vec<Route>,
}

impl Steps {
    fn new(page: BenchPage) -> Steps {
        // Every page waits for `/system/info` first: the Modeling App shows a
        // spinner until chap-core says which version it is.
        let mut steps = Steps {
            page,
            step: 0,
            routes: Vec::new(),
        };
        steps.get("system info", "/system/info");
        steps
    }

    fn next(&mut self) {
        self.step += 1;
    }

    fn get(&mut self, label: impl Into<String>, path: impl Into<String>) {
        self.routes.push(Route {
            page: self.page,
            step: self.step,
            label: label.into(),
            method: "GET",
            path: path.into(),
            body: None,
        });
    }

    fn post(&mut self, label: impl Into<String>, path: impl Into<String>, body: String) {
        self.routes.push(Route {
            page: self.page,
            step: self.step,
            label: label.into(),
            method: "POST",
            path: path.into(),
            body: Some(body),
        });
    }
}

fn units_query(units: &[String]) -> String {
    units
        .iter()
        .map(|u| format!("orgUnits={u}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn spec_path(id: i64, comparison: bool, archived: bool) -> String {
    format!(
        "/v1/crud/backtest-specifications/{id}?includeComparisonModels={comparison}&includeArchivedModels={archived}"
    )
}

/// The requests of `pages`, in page order. A page that [`missing`] rules
/// out is left out.
pub fn routes(facts: &Facts, pages: &[BenchPage]) -> Vec<Route> {
    let at_least = |minor: u64| facts.version.is_none_or(|v| v >= (2, minor, 0));
    let mut all = Vec::new();
    for &page in pages {
        if missing(page, facts).is_some() {
            continue;
        }
        let mut s = Steps::new(page);
        let backtest = facts.backtest.as_ref();
        match page {
            BenchPage::Evaluation => {
                let b = backtest.expect("checked by missing");
                let id = b.id;
                s.next();
                s.get("backtest info", format!("/v1/crud/backtests/{id}/info"));
                s.next();
                s.get("backtest-plots", "/v1/visualization/backtest-plots/");
                s.get("configured-models", "/v1/crud/configured-models");
                if at_least(4) {
                    s.get(
                        "backtest-parameters",
                        "/v1/analytics/backtest-parameters?periodType=month",
                    );
                    s.get("weather-providers", "/v1/analytics/weather-providers");
                }
                if let Some(spec) = b.specification_id {
                    s.get("backtest-specification", spec_path(spec, true, true));
                    if at_least(6) {
                        s.get(
                            "backtest-specification, outbreak",
                            spec_path(spec, false, true),
                        );
                    }
                }
                if at_least(6) {
                    s.get("alert-policies", "/v1/crud/alert-policies");
                    s.get("outbreak-metrics", "/v1/analytics/outbreak-metrics");
                }
                s.next();
                s.get(
                    "evaluation-entry adm0",
                    format!(
                        "/v1/analytics/evaluation-entry?backtestId={id}&{QUANTILES}&orgUnits=adm0"
                    ),
                );
                s.get(
                    "actualCases adm0",
                    format!("/v1/analytics/actualCases/{id}?orgUnits=adm0"),
                );
                let spec_has_metrics = facts.specification.as_ref().is_some_and(|s| s.has_metrics);
                if !spec_has_metrics && b.has_metrics {
                    s.get("metrics", format!("/v1/visualization/metrics/{id}"));
                }
            }
            BenchPage::Compare => {
                let b = backtest.expect("checked by missing");
                let id = b.id;
                s.next();
                s.get("backtests", "/v1/crud/backtests");
                s.next();
                if let Some(spec) = b.specification_id {
                    s.get("backtest-specification", spec_path(spec, false, false));
                }
                s.get(
                    "compatible-backtests",
                    format!("/v1/analytics/compatible-backtests/{id}"),
                );
                let baselines = facts
                    .specification
                    .as_ref()
                    .map(|s| s.baselines_done.clone())
                    .unwrap_or_default();
                if !baselines.is_empty() {
                    s.next();
                    for baseline in baselines {
                        s.get(
                            format!("baseline {baseline} info"),
                            format!("/v1/crud/backtests/{baseline}/info"),
                        );
                    }
                }
                let page: Vec<String> = b.org_units.iter().take(PAGE_ORG_UNITS).cloned().collect();
                if !page.is_empty() {
                    let units = units_query(&page);
                    s.next();
                    s.get(
                        format!("evaluation-entry {} org units", page.len()),
                        format!(
                            "/v1/analytics/evaluation-entry?backtestId={id}&{QUANTILES}&{units}"
                        ),
                    );
                    s.get(
                        format!("actualCases {} org units", page.len()),
                        format!("/v1/analytics/actualCases/{id}?{units}"),
                    );
                }
            }
            BenchPage::Plots => {
                let id = backtest.expect("checked by missing").id;
                s.next();
                s.get("backtest-plots", "/v1/visualization/backtest-plots/");
                s.next();
                for (plot, _) in &facts.plots {
                    s.get(
                        format!("{plot} facet-coords"),
                        format!("/v1/visualization/backtest-plots/{plot}/{id}/facet-coords"),
                    );
                }
                s.next();
                for (plot, body) in &facts.plots {
                    if let Some(body) = body {
                        s.post(
                            format!("{plot} subplot"),
                            format!("/v1/visualization/backtest-plots/{plot}/{id}/subplot"),
                            body.clone(),
                        );
                    }
                }
            }
            BenchPage::Benchmark => {
                let spec = facts.specification.as_ref().expect("checked by missing");
                s.next();
                s.get("backtest-specification", spec_path(spec.id, true, false));
                s.get(
                    "backtest-specification, breadcrumb",
                    spec_path(spec.id, false, false),
                );
                s.get("jobs", "/v1/jobs");
                s.next();
                if at_least(4) {
                    s.get(
                        "backtest-parameters",
                        "/v1/analytics/backtest-parameters?periodType=month",
                    );
                    s.get("weather-providers", "/v1/analytics/weather-providers");
                }
                s.get("configured-models", "/v1/crud/configured-models");
                if !spec.has_metrics
                    && let Some(first) = spec.first_backtest
                {
                    s.get("metrics", format!("/v1/visualization/metrics/{first}"));
                }
            }
            BenchPage::Dataset => {
                let dataset = backtest
                    .and_then(|b| b.dataset_id)
                    .expect("checked by missing");
                s.next();
                s.get("dataset", format!("/v1/crud/datasets/{dataset}"));
            }
            BenchPage::Prediction => {
                let p = facts.prediction.as_ref().expect("checked by missing");
                let dataset = p.dataset_id.expect("checked by missing");
                s.next();
                s.get("prediction", format!("/v1/crud/predictions/{}", p.id));
                s.get("configured-models", "/v1/crud/configured-models");
                s.next();
                s.get(
                    "prediction-entry",
                    format!("/v1/analytics/prediction-entry/{}?{QUANTILES}", p.id),
                );
                s.get(
                    format!("actualCases {} org units", p.org_units.len()),
                    format!(
                        "/v1/analytics/actualCases/{dataset}?{}&isDatasetId=true",
                        units_query(&p.org_units)
                    ),
                );
                if at_least(5) {
                    s.get("alerts", format!("/v1/crud/alerts?predictionId={}", p.id));
                }
                s.get("alert-policies", "/v1/crud/alert-policies");
            }
        }
        all.extend(s.routes);
    }
    all
}

/// How long a page takes to open, from the p50 of its requests: the slowest
/// request of each step, added up over the steps.
pub fn page_ms(routes: &[Route], p50: &[f64], page: BenchPage) -> f64 {
    let mut slowest: std::collections::BTreeMap<u8, f64> = std::collections::BTreeMap::new();
    for (route, ms) in routes.iter().zip(p50) {
        if route.page == page {
            let step = slowest.entry(route.step).or_insert(0.0);
            *step = step.max(*ms);
        }
    }
    slowest.values().sum()
}
