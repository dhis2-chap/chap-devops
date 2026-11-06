//! Which requests `chap-devops bench` sends: the ones the Modeling App makes
//! when a person opens one evaluation, built from what chap-core says about
//! that backtest.

use serde_json::Value;

/// The quantiles the Modeling App asks `evaluation-entry` for.
const QUANTILES: &str = "quantiles=0.1&quantiles=0.25&quantiles=0.5&quantiles=0.75&quantiles=0.9";

/// How many org units the Modeling App asks for at most on one chart page.
pub const PAGE_ORG_UNITS: usize = 10;

/// One request to time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// What the table calls it.
    pub label: String,
    pub method: &'static str,
    /// Path and query string.
    pub path: String,
    /// A JSON body, for a POST.
    pub body: Option<String>,
}

impl Route {
    fn get(label: impl Into<String>, path: impl Into<String>) -> Route {
        Route {
            label: label.into(),
            method: "GET",
            path: path.into(),
            body: None,
        }
    }
}

/// What the requests of one backtest need to know about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub id: i64,
    pub dataset_id: Option<i64>,
    pub specification_id: Option<i64>,
    pub org_units: Vec<String>,
}

/// The highest backtest id in a `GET /v1/crud/backtests` answer.
pub fn newest_backtest(list: &Value) -> Option<i64> {
    list.as_array()?
        .iter()
        .filter_map(|b| b.get("id")?.as_i64())
        .max()
}

/// The target from `GET /v1/crud/backtests/{id}/info`.
pub fn target(id: i64, info: &Value) -> Target {
    let int = |key: &str| info.get(key).and_then(Value::as_i64);
    Target {
        id,
        dataset_id: int("datasetId").or_else(|| {
            info.get("dataset")
                .and_then(|d| d.get("id"))
                .and_then(Value::as_i64)
        }),
        specification_id: int("specificationId"),
        org_units: info
            .get("orgUnits")
            .and_then(Value::as_array)
            .map(|units| {
                units
                    .iter()
                    .filter_map(|u| u.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
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

/// The requests for one backtest. `plots` holds each plot id with the
/// subplot body its facet coordinates gave, if they gave one.
pub fn routes(target: &Target, plots: &[(String, Option<String>)]) -> Vec<Route> {
    let id = target.id;
    let page: Vec<&str> = target
        .org_units
        .iter()
        .take(PAGE_ORG_UNITS)
        .map(String::as_str)
        .collect();
    let units = page
        .iter()
        .map(|u| format!("orgUnits={u}"))
        .collect::<Vec<_>>()
        .join("&");

    let mut routes = vec![
        Route::get("backtests", "/v1/crud/backtests"),
        Route::get("configured-models", "/v1/crud/configured-models"),
        Route::get("datasets", "/v1/crud/datasets"),
        Route::get("jobs", "/v1/jobs"),
        Route::get("backtest info", format!("/v1/crud/backtests/{id}/info")),
    ];
    if let Some(spec) = target.specification_id {
        routes.push(Route::get(
            "backtest-specification",
            format!(
                "/v1/crud/backtest-specifications/{spec}?includeComparisonModels=true&includeArchivedModels=false"
            ),
        ));
    }
    routes.push(Route::get(
        "evaluation-entry adm0",
        format!("/v1/analytics/evaluation-entry?backtestId={id}&{QUANTILES}&orgUnits=adm0"),
    ));
    routes.push(Route::get(
        "actualCases adm0",
        format!("/v1/analytics/actualCases/{id}?orgUnits=adm0&isDatasetId=false"),
    ));
    if !page.is_empty() {
        routes.push(Route::get(
            format!("evaluation-entry {} org units", page.len()),
            format!("/v1/analytics/evaluation-entry?backtestId={id}&{QUANTILES}&{units}"),
        ));
        routes.push(Route::get(
            format!("actualCases {} org units", page.len()),
            format!("/v1/analytics/actualCases/{id}?{units}&isDatasetId=false"),
        ));
    }
    routes.push(Route::get(
        "compatible-backtests",
        format!("/v1/analytics/compatible-backtests/{id}"),
    ));
    if let Some(dataset) = target.dataset_id {
        routes.push(Route::get(
            "dataset",
            format!("/v1/crud/datasets/{dataset}"),
        ));
    }
    routes.push(Route::get(
        "metrics",
        format!("/v1/visualization/metrics/{id}"),
    ));
    for (plot, body) in plots {
        routes.push(Route::get(
            format!("{plot} facet-coords"),
            format!("/v1/visualization/backtest-plots/{plot}/{id}/facet-coords"),
        ));
        if let Some(body) = body {
            routes.push(Route {
                label: format!("{plot} subplot"),
                method: "POST",
                path: format!("/v1/visualization/backtest-plots/{plot}/{id}/subplot"),
                body: Some(body.clone()),
            });
        }
    }
    routes
}
