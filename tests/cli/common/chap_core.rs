//! A stand-in chap-core: its REST API, its jobs and a model test run.

use super::*;
#[cfg(unix)]
use assert_cmd::Command;
use serde_json::Value as Json;
use std::net::Ipv4Addr;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use tempfile::TempDir;

/// The three jobs the stand-in chap-core reports, in the order it sends them:
/// chap-core answers newest first already, and `chap-devops jobs` is what decides
/// the order the table is in.
pub(crate) const DONE_ID: &str = "11111111-aaaa-4aaa-8aaa-000000000001";

pub(crate) const FAILED_ID: &str = "22222222-bbbb-4bbb-8bbb-000000000002";

pub(crate) const RUNNING_ID: &str = "33333333-cccc-4ccc-8ccc-000000000003";

/// A job log shaped like the real one: chap-core's own lines, then the model's
/// output, then the `--- stderr ---` section where the reason is.
pub(crate) const JOB_LOG: &str = "\
2026-09-24 16:28:19,440 [INFO] chap_status: Starting backtest for model '15'
Fitting INLA model...

--- stderr ---
Loading required package: Matrix
Error in predict_chap() : the inla program crashed
In addition: Warning messages:
1: In poly2nb(polygons, queen = FALSE) :
  some observations have no neighbours
Execution halted
";

/// The `/v1/jobs` payload, in chap-core's snake_case with its unquoted
/// timestamps.
pub(crate) fn job_list() -> String {
    format!(
        r#"[
  {{"id":"{RUNNING_ID}","type":"create_prediction","name":"eval-live",
    "status":"STARTED","start_time":"2026-09-24T16:34:12.911182",
    "end_time":null,"result":null,"prediction_setup_id":1}},
  {{"id":"{FAILED_ID}","type":"create_backtest","name":"eval-bym",
    "status":"FAILURE","start_time":"2026-09-24T16:28:19.437116",
    "end_time":"2026-09-24T16:28:29.686103","result":null,
    "prediction_setup_id":null}},
  {{"id":"{DONE_ID}","type":"create_backtest","name":"eval-ewars",
    "status":"SUCCESS","start_time":"2026-09-24T16:25:17.007503",
    "end_time":"2026-09-24T16:25:58.351174","result":"3",
    "prediction_setup_id":null}}
]"#
    )
}

/// A stand-in for chap-core's job and CRUD endpoints, on the port it returns.
///
/// Answers the paths `chap-devops jobs` and `chap-devops api` reach, and nothing else:
/// the point is the shapes - a bare JSON string for a status and for a log, a
/// `detail` object for a 404, a `message` for a cancel - because those are
/// what the commands render. The thread lives as long as the test process.
///
/// The server binds before anything is written, and the port is the one the
/// kernel gave that bind: a port picked first and bound later is free for a
/// parallel test to take in between. `chap-devops init --api-port` with it only
/// notes that the port is busy.
pub(crate) fn chap_core_server() -> u16 {
    serve_chap_core(false, false, false)
}

/// [`chap_core_server`] in the shape of chap-core 2.4: the same services
/// are registered, and nothing has configured models until a client posts
/// them. `POST /v1/crud/model-templates/from-service` stores a template, and
/// `POST /v1/crud/configured-models` adds a row to the listing. The old
/// chapkit model's template is a revision conflict, as a 409.
pub(crate) fn fresh_chap_core_server() -> u16 {
    serve_chap_core(false, false, true)
}

/// [`fresh_chap_core_server`] that refuses the configured model `name`
/// once, with a 422, the way chap-core refuses a configuration it cannot
/// store.
pub(crate) fn refusing_chap_core_server(name: &str) -> u16 {
    serve(Recorded {
        fresh: true,
        refuse_config: Some(name.to_string()),
        ..Recorded::default()
    })
}

/// The paths chap-core answers without a token, copied from `OPEN_PATHS` in
/// its `rest_api/auth.py` rather than from chap-devops, so the stand-in does not
/// agree with chap-devops by construction.
const CHAP_CORE_OPEN_PATHS: &[&str] = &["/health", "/health/ready", "/system/info"];

/// The version the stand-in chap-core reports on `/system/info`, which no
/// pin in the tests uses, so a report that shows it read the running build.
pub(crate) const CHAP_CORE_VERSION: &str = "1.9.3";

/// The revision the stand-in chap-core reports on `/system/info`.
pub(crate) const CHAP_CORE_REVISION: &str = "0a1b2c3";

/// chap-core's `SystemInfoResponse`, with every field it sends.
fn system_info(protected: bool) -> String {
    format!(
        r#"{{"chap_core_version":"{CHAP_CORE_VERSION}","python_version":"3.13.7",
  "server_date":"2026-10-04T09:00:00.000000+00:00","server_time_zone_id":"Etc/UTC",
  "revision":"{CHAP_CORE_REVISION}","auth_required":{protected}}}"#
    )
}

/// [`chap_core_server`] behind a token, the way chap-core is when
/// `CHAP_API_TOKEN` is set: a request without a Bearer header is answered
/// 401, except on the paths chap-core leaves open (`OPEN_PATHS` in its
/// `rest_api/auth.py`).
pub(crate) fn protected_chap_core_server() -> u16 {
    serve_chap_core(true, false, false)
}

/// [`chap_core_server`] that cannot reach its models: every proxied
/// `/health` is a 502.
pub(crate) fn unreachable_models_chap_core_server() -> u16 {
    serve_chap_core(false, true, false)
}

fn serve_chap_core(protected: bool, unreachable: bool, fresh: bool) -> u16 {
    serve(Recorded {
        unreachable,
        protected,
        fresh,
        ..Recorded::default()
    })
}

/// Serve the stand-in, starting from `recorded`.
fn serve(recorded: Recorded) -> u16 {
    use std::io::Write;

    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a free port");
    let port = listener.local_addr().expect("a local address").port();
    std::thread::spawn(move || {
        // What this server was asked to do, for the tests that check the
        // cleanup: one server per port, so the record is this thread's own.
        let mut recorded = recorded;
        let protected = recorded.protected;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // The request has to be read before the answer, or the client
            // sees a reset instead of the response.
            let request = read_request(&mut stream);
            let mut head = request.lines().next().unwrap_or("").split_whitespace();
            let method = head.next().unwrap_or("GET").to_string();
            let path = head.next().unwrap_or("/").to_string();
            let authed = request.to_lowercase().contains("authorization: bearer ");
            let body = request
                .split_once("\r\n\r\n")
                .map(|(_, rest)| rest.to_string())
                .unwrap_or_default();

            let route = path
                .split_once('?')
                .map_or(path.as_str(), |(route, _)| route);
            let open = CHAP_CORE_OPEN_PATHS.contains(&route);
            let (status, content_type, payload) = match protected && !authed && !open {
                true => (
                    401,
                    "application/json",
                    r#"{"detail":"Missing or invalid API token"}"#.to_string(),
                ),
                false => chap_core_route(&method, &path, authed, &body, &mut recorded),
            };
            let reason = match status {
                200 => "OK",
                400 => "Bad Request",
                401 => "Unauthorized",
                409 => "Conflict",
                422 => "Unprocessable Entity",
                502 => "Bad Gateway",
                _ => "Not Found",
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

/// The three models the `models test` stand-in knows, and what each one is
/// there to cover: a model that backtests cleanly, one whose prediction
/// crashes, and one whose chapkit predates `$generate-sample-data`.
pub(crate) const PASSING_MODEL: &str = "chapkit-ewars-model";

pub(crate) const FAILING_MODEL: &str = "chapkit-rwanda-malaria-bym-model";

pub(crate) const OLD_CHAPKIT_MODEL: &str = "auto-arima-chapkit";

/// The dataset and backtest rows the stand-in says the jobs wrote, which are
/// the two rows the cleanup has to delete and nothing else.
pub(crate) const TEST_DATASET: i64 = 41;

pub(crate) const TEST_BACKTEST: i64 = 7;

/// The config and the artifact `chapkit test` leaves in a model service's own
/// database, which the model level has to find and delete.
pub(crate) const TEST_CONFIG: &str = "01M3A4TSB2YHRPFBEFCT4TMX4A";

pub(crate) const TEST_ARTIFACT: &str = "01M3A4TSB9WZ8V5XN0A4F4Y1M6";

/// `GET /v1/crud/backtests/7`, cut to the field `models test` reads.
pub(crate) const BACKTEST_ROW: &str = r#"{"id":7,"datasetId":41,"modelId":"chapkit-ewars-model",
  "aggregateMetrics":{"ratio_above_truth":0.45,"crps":20.04,"mae":28.96,"rmse":34.88,
  "mape":12.98,"coverage_10_90":0.6}}"#;

/// The configured model a backtest of the passing model has to name.
pub(crate) const PASSING_CONFIGURED: i64 = 20;

/// What chap-core has configured, as `(id, name, archived)`.
///
/// The passing model is in the state that makes the id necessary: it has
/// re-registered with a new version, so the row it first got - the one named
/// plainly after the service - is archived, and what is live are the configs
/// chap-core synced out of the service itself, named `<service>:<config>`.
/// One of those is what a previous `chapkit test` left behind, and picking
/// that one would be backtesting a test's leftovers.
pub(crate) fn configured_models() -> Vec<(i64, String, bool)> {
    vec![
        (14, PASSING_MODEL.to_string(), true),
        (
            19,
            format!("{PASSING_MODEL}:test_config_{TEST_CONFIG}"),
            false,
        ),
        (
            PASSING_CONFIGURED,
            format!("{PASSING_MODEL}:{PASSING_MODEL}_1790267117082761"),
            false,
        ),
        // The other two are as a service looks the day it registers.
        (15, FAILING_MODEL.to_string(), false),
        (16, OLD_CHAPKIT_MODEL.to_string(), false),
    ]
}

/// `GET /v1/crud/configured-models`, with the fields chap-core carries.
pub(crate) fn configured_models_payload() -> String {
    let rows: Vec<Json> = configured_models()
        .into_iter()
        .map(|(id, name, archived)| {
            // The passing model's config asks for a covariate the sample
            // data does not carry, as the simple multistep model's does.
            let covariates: &[&str] = if id == PASSING_CONFIGURED {
                &["rainfall", "mean_temperature", "mean_relative_humidity"]
            } else {
                &[]
            };
            serde_json::json!({
                "id": id,
                "name": name,
                "version": "1.0.1",
                "archived": archived,
                "usesChapkit": true,
                "sourceDigest": "f9a1c0d",
                "additionalContinuousCovariates": covariates,
            })
        })
        .collect();
    serde_json::to_string(&rows).expect("JSON")
}

/// The service a `create-backtest` body is about, from either spelling of
/// `modelId`: the integer key of a configured model, or a service id.
pub(crate) fn backtest_service(model_id: &Json, recorded: &Recorded) -> String {
    if let Some(name) = model_id.as_str() {
        return name.to_string();
    }
    let wanted = model_id.as_i64().unwrap_or_default();
    let created = recorded.created.iter().map(|row| {
        (
            row["id"].as_i64().unwrap_or_default(),
            row["name"].as_str().unwrap_or_default().to_string(),
            false,
        )
    });
    configured_models()
        .into_iter()
        .chain(created)
        .find(|(id, ..)| *id == wanted)
        .map(|(_, name, _)| match name.split_once(':') {
            Some((service, _)) => service.to_string(),
            None => name,
        })
        .unwrap_or_default()
}

/// A service registered from outside the deployment, the way a model run
/// from its checkout is.
pub(crate) const HOST_RUN_MODEL: &str = "my-host-model";

/// `GET /v2/services`: the three stand-in models and one from outside.
fn service_list() -> String {
    let entries: Vec<String> = [
        PASSING_MODEL,
        FAILING_MODEL,
        OLD_CHAPKIT_MODEL,
        HOST_RUN_MODEL,
    ]
    .iter()
    .map(|id| {
        // The old chapkit is built without a revision, which is why the 2.4
        // stand-in refuses its template. The others report one, as a model
        // built with one does.
        let revision = match *id == OLD_CHAPKIT_MODEL {
            true => r#","git_revision":null"#,
            false => r#","git_revision":"0123456789abcdef""#,
        };
        format!(
            r#"{{"id":"{id}","url":"http://{id}:8000","info":{{"id":"{id}","version":"1.0.1"{revision}}},
                   "last_ping_at":"2026-09-24T16:00:00Z","expires_at":"2026-09-24T16:05:00Z"}}"#
        )
    })
    .collect();
    format!(
        r#"{{"count":{},"services":[{}]}}"#,
        entries.len(),
        entries.join(",")
    )
}

/// `GET /v2/services/...` and everything under its proxy.
pub(crate) fn services_route(
    method: &str,
    route: &str,
    recorded: &mut Recorded,
) -> Option<(u16, &'static str, String)> {
    let json = "application/json";
    let missing = || Some((404, json, r#"{"detail":"Not Found"}"#.to_string()));
    let rest = route.strip_prefix("/v2/services/")?;
    if method != "GET" {
        return missing();
    }
    let (service, tail) = match rest.split_once('/') {
        Some(parts) => parts,
        None => (rest, ""),
    };
    if ![
        PASSING_MODEL,
        FAILING_MODEL,
        OLD_CHAPKIT_MODEL,
        HOST_RUN_MODEL,
    ]
    .contains(&service)
    {
        return Some((
            404,
            json,
            format!(r#"{{"detail":"Service '{service}' not found"}}"#),
        ));
    }
    if tail == "run/health" && recorded.unreachable {
        return Some((502, json, r#"{"detail":"Bad Gateway"}"#.to_string()));
    }
    if tail.is_empty() {
        // The `info` block a chapkit service registers with, cut to the
        // fields `models test` reads off it. The version is the one of every
        // configured model in the listing.
        let chapkit = if service == OLD_CHAPKIT_MODEL {
            "1.0.0"
        } else {
            "2.0.0"
        };
        let geo = service == FAILING_MODEL;
        return Some((
            200,
            json,
            format!(
                r#"{{"id":"{service}","url":"http://{service}:8000","info":{{"id":"{service}",
                   "version":"1.0.1","period_type":"monthly","required_covariates":["population"],
                   "requires_geo":{geo},"chapkit_version":"{chapkit}"}}}}"#
            ),
        ));
    }
    if tail.contains("generate-sample-data") {
        recorded.sampled.push(service.to_string());
        if service == OLD_CHAPKIT_MODEL {
            return missing();
        }
        // The passing model stands in for a chapkit that answered without a
        // `geo` at all, which is what the null-geometry fallback is for.
        return Some((200, json, sample_payload(service == FAILING_MODEL)));
    }
    // The model level lists these before the run and again after it, and
    // deletes the difference; the second listing is therefore the one that
    // has to carry what `chapkit test` left behind.
    if tail.ends_with("api/v1/configs") {
        recorded.config_lists += 1;
        let body = if recorded.config_lists == 2 {
            format!(
                r#"[{{"id":"{TEST_CONFIG}","created_at":"2026-09-24T16:43:57",
                   "name":"test_config_{TEST_CONFIG}","data":{{}}}}]"#
            )
        } else {
            "[]".to_string()
        };
        return Some((200, json, body));
    }
    if tail.ends_with("api/v1/artifacts") {
        recorded.artifact_lists += 1;
        // Gone again on the third listing: chapkit cascades a config delete
        // to the artifact trees linked to it.
        let body = if recorded.artifact_lists == 2 {
            format!(r#"[{{"id":"{TEST_ARTIFACT}","data":{{"type":"ml_training_workspace"}}}}]"#)
        } else {
            "[]".to_string()
        };
        return Some((200, json, body));
    }
    missing()
}

/// What `$generate-sample-data?kind=train` answers with.
///
/// The shape rather than the size: a column-oriented frame, and - for a model
/// that needs geometry - a `geo` whose features carry their location id in
/// `properties` and nothing at the top level, which is exactly the thing the
/// CLI has to fix before chap-core will match an org unit to one.
pub(crate) fn sample_payload(with_geo: bool) -> String {
    let mut columns = vec![
        "time_period",
        "location",
        "disease_cases",
        "population",
        "rainfall",
        "mean_temperature",
        "feature_0",
    ];
    if with_geo {
        columns.push("relative_humidity");
    }
    let mut rows: Vec<Json> = Vec::new();
    for period in ["2020-01", "2020-02", "2020-03"] {
        for (at, location) in ["location_0", "location_1"].iter().enumerate() {
            let mut row = vec![Json::from(period), Json::from(*location)];
            for n in 0..columns.len() - 2 {
                row.push(Json::from((at + n + 1) as f64 * 1.5));
            }
            rows.push(Json::Array(row));
        }
    }
    let mut payload = serde_json::json!({"data": {"columns": columns, "data": rows}});
    if with_geo {
        payload["geo"] = serde_json::json!({
            "type": "FeatureCollection",
            "features": [
                {"type": "Feature", "geometry": {"type": "Point", "coordinates": [0.0, 0.0]},
                 "properties": {"id": "location_0"}},
                {"type": "Feature", "geometry": {"type": "Point", "coordinates": [1.0, 1.0]},
                 "properties": {"id": "location_1"}},
            ],
        });
    }
    payload.to_string()
}

/// `make-dataset`, `create-backtest`, the CRUD rows and the deletes.
pub(crate) fn analytics_route(
    method: &str,
    route: &str,
    body: &str,
    recorded: &mut Recorded,
) -> Option<(u16, &'static str, String)> {
    let json = "application/json";
    match (method, route) {
        ("POST", "/v1/analytics/make-dataset") => {
            let sent: Json = serde_json::from_str(body).unwrap_or(Json::Null);
            recorded.observations = sent["providedData"].as_array().map(Vec::len).unwrap_or(0);
            let name = sent["name"].as_str().unwrap_or_default().to_string();
            let mut features: Vec<String> = sent["providedData"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|o| o["featureName"].as_str().map(str::to_string))
                .collect();
            features.sort();
            features.dedup();
            recorded.features.push(features);
            // chap-core drops every org unit whose feature has no top-level
            // `id`, so a geojson without them imports nothing at all - which
            // is what makes this the check that the CLI sets them.
            let imported = sent["geojson"]["features"]
                .as_array()
                .map(|features| features.iter().filter(|f| f["id"].is_string()).count())
                .unwrap_or(0);
            let service = dataset_service(&name);
            recorded.datasets.push(name);
            Some((
                200,
                json,
                format!(r#"{{"id":"ds-{service}","importedCount":{imported},"rejected":[]}}"#),
            ))
        }
        ("POST", "/v1/analytics/create-backtest") => {
            let sent: Json = serde_json::from_str(body).unwrap_or(Json::Null);
            recorded.backtests.push(sent.clone());
            let service = backtest_service(&sent["modelId"], recorded);
            Some((200, json, format!(r#"{{"id":"bt-{service}"}}"#)))
        }
        // Not chap-core's: how a test asks this server what it was asked.
        ("GET", "/v1/recorded") => Some((
            200,
            json,
            serde_json::json!({
                "deleted": recorded.deleted,
                "observations": recorded.observations,
                "datasets": recorded.datasets,
                "features": recorded.features,
                "sampled": recorded.sampled,
                "backtests": recorded.backtests,
                "templates": recorded.templates,
                "configured_posts": recorded.configured_posts,
                "retired": recorded.retired,
            })
            .to_string(),
        )),
        ("DELETE", _) if route.starts_with("/v1/crud/") => {
            recorded.deleted.push(route.to_string());
            Some((200, json, r#"{"message":"deleted"}"#.to_string()))
        }
        ("GET", "/v1/crud/configured-models") => Some((200, json, configured_models_payload())),
        ("GET", _) if route == format!("/v1/crud/backtests/{TEST_BACKTEST}") => {
            Some((200, json, BACKTEST_ROW.to_string()))
        }
        _ => None,
    }
}

/// The service a `chap-devops-test-<service>-<stamp>` dataset name is about.
pub(crate) fn dataset_service(name: &str) -> String {
    let rest = name.strip_prefix("chap-devops-test-").unwrap_or(name);
    // The stamp is the last two hyphen-separated parts: `20260924-181500`.
    let parts: Vec<&str> = rest.rsplitn(3, '-').collect();
    parts.get(2).copied().unwrap_or(rest).to_string()
}

/// One whole HTTP request, headers and body.
///
/// A single `read` is enough for the requests `chap-devops jobs` makes and nowhere
/// near enough for `make-dataset`, which is a few thousand observations, so
/// the headers are read first and then exactly as much body as
/// `Content-Length` promises.
pub(crate) fn read_request(stream: &mut std::net::TcpStream) -> String {
    use std::io::Read;
    let mut raw: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        if let Some(at) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&raw[..at]).to_lowercase();
            let want: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(0);
            if raw.len() >= at + 4 + want {
                break;
            }
        }
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => raw.extend_from_slice(&buffer[..read]),
        }
    }
    String::from_utf8_lossy(&raw).into_owned()
}

/// What one stand-in chap-core was asked to do, so a test can check that the
/// cleanup happened and that what went out was the right shape.
#[derive(Debug, Default)]
pub(crate) struct Recorded {
    /// Whether every model's proxied `/health` answers 502, the way chap-core
    /// in a container answers for a model registered as `localhost:<port>`.
    pub(crate) unreachable: bool,
    /// Whether the server wants a token, which `/system/info` reports as
    /// `auth_required`.
    pub(crate) protected: bool,
    /// Every path a `DELETE` reached, in order.
    pub(crate) deleted: Vec<String>,
    /// Observations in the last `make-dataset` body.
    pub(crate) observations: usize,
    /// The `name` of every dataset that was asked for.
    pub(crate) datasets: Vec<String>,
    /// The feature names each of those datasets carried, in the same order.
    pub(crate) features: Vec<Vec<String>>,
    /// The services whose sample data was fetched.
    pub(crate) sampled: Vec<String>,
    /// Every `create-backtest` body, whole.
    pub(crate) backtests: Vec<Json>,
    /// How many times the configs and the artifacts have been listed, so the
    /// second listing can be the one with the test's leftovers in it.
    pub(crate) config_lists: usize,
    pub(crate) artifact_lists: usize,
    /// Whether this is [`fresh_chap_core_server`]: chap-core 2.4, which has
    /// only the configured models a client posted.
    pub(crate) fresh: bool,
    /// The service of every template stored from a service.
    pub(crate) templates: Vec<String>,
    /// Every template retire, with its query.
    pub(crate) retired: Vec<String>,
    /// The name of a configured model the next post of it is refused for.
    pub(crate) refuse_config: Option<String>,
    /// Every `POST /v1/crud/configured-models` body, whole.
    pub(crate) configured_posts: Vec<Json>,
    /// The rows those posts made, as the listing returns them.
    pub(crate) created: Vec<Json>,
}

/// The answer one request gets: `(status, content type, body)`.
pub(crate) fn chap_core_route(
    method: &str,
    path: &str,
    authed: bool,
    body: &str,
    recorded: &mut Recorded,
) -> (u16, &'static str, String) {
    let json = "application/json";
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    let status_of = |id: &str| match id {
        DONE_ID => Some("SUCCESS"),
        FAILED_ID => Some("FAILURE"),
        RUNNING_ID => Some("STARTED"),
        // The backtest of the model whose prediction crashes is the one job
        // `chap-devops models test --backtest` has to report a reason for.
        id if id == format!("bt-{FAILING_MODEL}") => Some("FAILURE"),
        id if id.starts_with("ds-") || id.starts_with("bt-") => Some("SUCCESS"),
        _ => None,
    };
    if route == "/health" && method == "GET" {
        return (
            200,
            json,
            r#"{"status":"success","message":"healthy"}"#.to_string(),
        );
    }
    if route == "/system/info" && method == "GET" {
        return (200, json, system_info(recorded.protected));
    }
    if route == "/v2/services" && method == "GET" {
        return (200, json, service_list());
    }
    if let Some(answer) = services_route(method, route, recorded) {
        return answer;
    }
    if let Some(answer) = configured_route(method, route, query, body, recorded) {
        return answer;
    }
    if let Some(answer) = analytics_route(method, route, body, recorded) {
        return answer;
    }

    if route == "/v1/jobs" && method == "GET" {
        // The `status` query parameter is the one filter the CLI sends.
        let wanted: Vec<&str> = query
            .split('&')
            .filter_map(|pair| pair.strip_prefix("status="))
            .collect();
        if wanted.is_empty() {
            return (200, json, job_list());
        }
        let list: Json = serde_json::from_str(&job_list()).expect("the list is JSON");
        let kept: Vec<Json> = list
            .as_array()
            .expect("a list")
            .iter()
            .filter(|job| {
                let status = job["status"].as_str().unwrap_or_default();
                wanted.iter().any(|w| w.eq_ignore_ascii_case(status))
            })
            .cloned()
            .collect();
        return (200, json, serde_json::to_string(&kept).expect("JSON"));
    }
    if let Some(rest) = route.strip_prefix("/v1/jobs/") {
        let (id, tail) = match rest.split_once('/') {
            Some((id, tail)) => (id, tail),
            None => (rest, ""),
        };
        let Some(status) = status_of(id) else {
            return (404, json, format!(r#"{{"detail":"Job '{id}' not found"}}"#));
        };
        return match (method, tail) {
            // A whole job after chap-core v2.5.1, a bare string up to it. A
            // backtest job answers the first way, every other job the second,
            // so `models test --backtest` reads both.
            ("GET", "") if id.starts_with("bt-") => (
                200,
                json,
                format!(r#"{{"id":"{id}","type":"backtest","status":"{status}"}}"#),
            ),
            ("GET", "") => (200, json, format!(r#""{status}""#)),
            // The log is one JSON string, which is what makes reading it a
            // question of rendering rather than of parsing.
            ("GET", "logs") => (
                200,
                json,
                serde_json::to_string(JOB_LOG).expect("a JSON string"),
            ),
            ("GET", "database_result") if status == "SUCCESS" => {
                let row = if id.starts_with("ds-") {
                    TEST_DATASET
                } else if id.starts_with("bt-") {
                    TEST_BACKTEST
                } else {
                    4
                };
                (200, json, format!(r#"{{"id":{row}}}"#))
            }
            ("GET", "database_result") => {
                (400, json, r#"{"detail":"Job is not finished"}"#.to_string())
            }
            ("POST", "cancel") => (200, json, r#"{"message":"Job cancelled"}"#.to_string()),
            ("DELETE", "") if status == "STARTED" => (
                400,
                json,
                r#"{"detail":"Cannot delete a running job"}"#.to_string(),
            ),
            ("DELETE", "") => (200, json, r#"{"message":"Job deleted"}"#.to_string()),
            _ => (404, json, r#"{"detail":"Not Found"}"#.to_string()),
        };
    }
    // A handful of paths for `chap-devops api` itself: an echo for a body, a
    // non-JSON body, and one that reports whether a token arrived.
    match (method, route) {
        ("POST", "/v1/echo") | ("PUT", "/v1/echo") | ("PATCH", "/v1/echo") => {
            (200, json, format!(r#"{{"received":{body}}}"#))
        }
        ("GET", "/v1/text") => (200, "text/plain", "plain text, not JSON\n".to_string()),
        ("GET", "/v1/whoami") => (200, json, format!(r#"{{"auth":{authed}}}"#)),
        ("GET", "/v1/empty") => (200, json, String::new()),
        _ => (404, json, r#"{"detail":"Not Found"}"#.to_string()),
    }
}

/// What the stand-in was asked to do, read back through `chap-devops api`.
pub(crate) fn recorded(sandbox: &Sandbox, dir: &Path) -> Json {
    json_of(&mut chap_in(
        sandbox,
        dir,
        &["--json", "api", "GET", "/v1/recorded"],
    ))
}

/// `chap-devops <args>` with the fake docker ahead of the real one on PATH.
#[cfg(unix)]
pub(crate) fn chap_with_docker(
    sandbox: &Sandbox,
    cwd: &Path,
    bin: &Path,
    args: &[&str],
) -> Command {
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut cmd = chap_in(sandbox, cwd, args);
    cmd.env("PATH", path);
    cmd
}

/// A `docker` on PATH that answers everything with success and nothing else.
///
/// `chap-devops update` ends in `docker compose pull`, and a test must not pull
/// anything: what it is about is the files the run writes before that, and
/// the closing line it prints after. Every call is logged so a test can check
/// that the pull was asked for at all.
#[cfg(unix)]
pub(crate) fn quiet_docker() -> (TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().expect("a directory for the fake docker");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("a bin directory");
    let log = temp.path().join("calls.log");
    let docker = bin.join("docker");
    std::fs::write(
        &docker,
        format!("#!/bin/sh\necho \"$*\" >> {}\nexit 0\n", log.display()),
    )
    .expect("the fake docker");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755))
            .expect("an executable fake docker");
    }
    (temp, bin, log)
}

/// A deployment pinned to the hub's newest release, and the fake docker the
/// update runs through.
#[cfg(unix)]
pub(crate) fn pinned_sandbox() -> (Sandbox, PathBuf, u16, TempDir, PathBuf) {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    let port = Hub::new().start();
    sandbox
        .online_init(port, &["-v", "--models", "none", "--chap-tag", "v2.3.1"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "hint: chap-core v2.3.1 is from chap-core compose.ghcr.yml at v2.3.1",
        ));
    assert!(read(&dir.join("compose.yml")).contains("CHAP_DEVOPS_TEST_REF: v2.3.1"));
    let (temp, bin, _) = quiet_docker();
    (sandbox, dir, port, temp, bin)
}

/// `chap-devops -C <project> update ...` against the hub, with the fake docker
/// ahead of the real one on PATH.
#[cfg(unix)]
pub(crate) fn online_update(sandbox: &Sandbox, port: u16, bin: &Path, args: &[&str]) -> Command {
    let mut cmd = sandbox.online(port);
    cmd.env(
        "PATH",
        format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    );
    cmd.arg("update").args(args);
    cmd
}
