//! A stand-in DHIS2 and App Hub, and the sandboxes the dhis2 tests run in.

use crate::common::*;
#[cfg(unix)]
use assert_cmd::Command;
#[cfg(unix)]
use serde_json::Value as Json;
#[cfg(unix)]
use std::net::Ipv4Addr;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(unix)]
use tempfile::TempDir;

/// A DHIS2 whose API layer is alive.
pub(crate) fn dhis2_lookalike() -> u16 {
    routed(Some("/api/ping"), "pong")
}

/// A DHIS2 that is falsely healthy: it serves, and it has no API.
#[cfg(unix)]
pub(crate) fn dhis2_without_an_api() -> u16 {
    routed(None, "Not Found")
}

/// A stand-in for the two endpoints `chap-devops status` decides `up` by, on a port
/// of its own.
///
/// `up` means chap-core answered *as* chap-core: `/health` has to be JSON with
/// a `status` field and `/v2/services` has to parse as `{count, services}`. So
/// a report that gets as far as its closing line - which is where the hints
/// under it live - needs both, and no test can start a real chap-core. The port
/// comes back so the deployment can be created with `--api-port` pointing here.
#[cfg(unix)]
pub(crate) fn chap_core_lookalike() -> u16 {
    use std::io::{BufRead, BufReader, Write};

    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a free port");
    let port = listener.local_addr().expect("a local address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let mut request = String::new();
            // The request has to be read before the answer, or the client sees
            // a reset instead of the response.
            if BufReader::new(&stream).read_line(&mut request).is_err() {
                continue;
            }
            let path = request.split_whitespace().nth(1).unwrap_or_default();
            let body = match path.split('?').next().unwrap_or_default() {
                "/health" => r#"{"status":"success","message":"healthy"}"#,
                "/v2/services" => r#"{"count":0,"services":[]}"#,
                _ => "",
            };
            let (code, reason) = match body.is_empty() {
                true => (404, "Not Found"),
                false => (200, "OK"),
            };
            let response = format!(
                "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let mut out = &stream;
            let _ = out.write_all(response.as_bytes());
            let _ = out.flush();
        }
    });
    port
}

/// A `docker` on PATH that reports one service of this deployment as running,
/// and knows nothing else.
///
/// `chap-devops status` asks docker which containers are up, and that answer is what
/// decides whether a component is asked anything at all. No test may start a
/// real DHIS2 - it wants several gigabytes and many minutes - so the container
/// half of the answer comes from here and the HTTP half from a stand-in on the
/// port, which is the whole of what the row is made of.
#[cfg(unix)]
pub(crate) fn docker_running(service: &str) -> (TempDir, PathBuf) {
    docker_running_all(&[service])
}

/// [`docker_running`] for several services at once, one running container
/// each.
#[cfg(unix)]
pub(crate) fn docker_running_all(services: &[&str]) -> (TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("a directory for the fake docker");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("a bin directory");
    let lines: String = services
        .iter()
        .map(|service| {
            format!("{{\"Service\":\"{service}\",\"State\":\"running\",\"Health\":\"healthy\"}}\\n")
        })
        .collect();
    let script = format!(
        "#!/bin/sh\n\
         case \"$*\" in\n\
         *' ps -a --format json'*) \
           printf '{lines}'; \
           exit 0;;\n\
         esac\n\
         exit 1\n",
    );
    let docker = bin.join("docker");
    std::fs::write(&docker, script).expect("the fake docker");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755))
            .expect("an executable fake docker");
    }
    (temp, bin)
}

/// A deployment whose only component besides chap-core is a DHIS2 published on
/// `port`, which is where a stand-in is already listening.
pub(crate) fn dhis2_sandbox(port: u16) -> (Sandbox, PathBuf) {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "none", "--api-port", &free_port().to_string()])
        .assert()
        .success();
    sandbox
        .components(&["enable", "dhis2", "--port", &port.to_string()])
        .assert()
        .success();
    (sandbox, dir)
}

/// What the DHIS2 stand-in holds and what it has been asked.
///
/// A real DHIS2 cannot be started in a test - it wants several gigabytes and
/// many minutes - so the instance is this: one thread, the handful of endpoints
/// `chap-devops dhis2` talks to, and a record of every request, which is what lets a
/// test assert that the route was *repointed* rather than created and that
/// nothing ever sent `lastYears`.
#[cfg(unix)]
#[derive(Default)]
pub(crate) struct Dhis2State {
    /// `METHOD path` for every request it answered, in order.
    pub(crate) asked: Vec<String>,
    /// The `Authorization` header of every authenticated request.
    pub(crate) auth: Vec<String>,
    /// The `chap` route row, as DHIS2 would store it.
    pub(crate) route: Option<Json>,
    /// The apps `GET /api/apps` lists.
    pub(crate) apps: Vec<Json>,
    /// Whether an analytics run has been asked for.
    pub(crate) analytics_started: bool,
    /// A run that is already going when the command arrives.
    pub(crate) analytics_running: bool,
    /// A run that DHIS2 accepted and has not started: it is in the job list
    /// and not yet in the notifier.
    pub(crate) analytics_queued: bool,
    /// A `lastAnalyticsTableSuccess` that came with the seed dump: the setting
    /// is a database row, so a restored deployment inherits it while its
    /// notifier - which lives in the process - stays empty.
    pub(crate) inherited_analytics: bool,
    /// The epoch as the last analytics success, which is what DHIS2 2.42.6
    /// sends on an empty database that has never run analytics.
    pub(crate) epoch_analytics: bool,
    /// Answer every authenticated request with a 401.
    pub(crate) unauthorized: bool,
    /// Refuse every route write the way `route.remote_servers_allowed` does.
    pub(crate) route_not_permitted: bool,
    /// Answer every request proxied through the route with a 502, the way
    /// DHIS2 does when chap-core is down behind a route that is right.
    pub(crate) proxy_fails: bool,
    /// How many requests through the route answer 500 before it works, the
    /// way a real DHIS2 can fail the first request after a repoint.
    pub(crate) proxy_failures: u32,
    /// Answer every request proxied through the route with this status and
    /// body: an empty 503 is what the allowlist does at proxy time, a 500 is
    /// a route target that DHIS2 cannot connect to.
    pub(crate) proxy_answer: Option<(u16, &'static str)>,
    /// chap-core's API token: when set, anything proxied past `/health`
    /// answers 401 unless the route's `auth` sends `Bearer` and this token.
    pub(crate) chap_token: Option<String>,
    /// The port the stand-in listens on, for the download URL GitHub lists.
    pub(crate) port: u16,
    /// Which of [`STAND_IN_BUILDS`] the GitHub `latest` release of
    /// chap-frontend holds; `None` is no such release.
    pub(crate) unstable: Option<usize>,
    /// A digest for the release to list in place of the right one.
    pub(crate) unstable_digest: Option<&'static str>,
    /// The body of every `POST /api/apps`, the upload of an unstable build.
    pub(crate) uploads: Vec<String>,
}

/// The unstable builds the GitHub stand-in can serve: the commit, the zip and
/// its digest.
#[cfg(unix)]
pub(crate) const STAND_IN_BUILDS: &[(&str, &str, &str)] = &[
    (
        "0c30f8de28515c933aeb5916a3d94b8478f843de",
        "PK fake modeling app build one",
        "sha256:46f3cb8716812ae404bdb97b947bbda52dc0afcb29a50597a00a6eb1fa078d61",
    ),
    (
        "9a1b2c3d4e5f60718293a4b5c6d7e8f901234567",
        "PK fake modeling app build two",
        "sha256:63eed785e8f5c028c84b033cc519f6b861f75f614510d228aa33f711365c44d4",
    ),
];

/// A stand-in for one DHIS2 instance and for the App Hub beside it.
#[cfg(unix)]
pub(crate) struct Dhis2StandIn {
    pub(crate) port: u16,
    pub(crate) state: std::sync::Arc<std::sync::Mutex<Dhis2State>>,
}

#[cfg(unix)]
impl Dhis2StandIn {
    /// A stand-in on a port of its own, with nothing configured.
    pub(crate) fn new() -> Dhis2StandIn {
        Dhis2StandIn::with(Dhis2State::default())
    }

    pub(crate) fn with(state: Dhis2State) -> Dhis2StandIn {
        use std::io::{BufRead, BufReader, Read, Write};

        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a free port");
        let port = listener.local_addr().expect("a local address").port();
        let state = std::sync::Arc::new(std::sync::Mutex::new(Dhis2State { port, ..state }));
        let shared = std::sync::Arc::clone(&state);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                // The headers have to be read before the answer, and the body
                // with them, or the client sees a reset instead of a response.
                let mut length = 0usize;
                let mut authorization = String::new();
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 {
                        break;
                    }
                    if header.trim().is_empty() {
                        break;
                    }
                    let lower = header.to_ascii_lowercase();
                    if let Some(value) = lower.strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                    if let Some((name, value)) = header.split_once(':')
                        && name.trim().eq_ignore_ascii_case("authorization")
                    {
                        authorization = value.trim().to_string();
                    }
                }
                let mut body = vec![0u8; length];
                if length > 0 && reader.read_exact(&mut body).is_err() {
                    continue;
                }
                let body = String::from_utf8_lossy(&body).to_string();

                let (code, answer) = {
                    let mut state = shared.lock().expect("the stand-in outlives its panics");
                    state.asked.push(format!("{method} {path}"));
                    if !authorization.is_empty() {
                        state.auth.push(authorization);
                    }
                    dhis2_answer(&mut state, &method, &path, &body)
                };
                let reason = match code {
                    200 => "OK",
                    201 => "Created",
                    401 => "Unauthorized",
                    409 => "Conflict",
                    500 => "Internal Server Error",
                    502 => "Bad Gateway",
                    _ => "Not Found",
                };
                let response = format!(
                    "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                let mut out = &stream;
                let _ = out.write_all(response.as_bytes());
                let _ = out.flush();
            }
        });
        Dhis2StandIn { port, state }
    }

    /// Every request it answered, in order.
    pub(crate) fn asked(&self) -> Vec<String> {
        self.state.lock().expect("the request log").asked.clone()
    }

    /// Whether it was asked this exact `METHOD path`.
    pub(crate) fn was_asked(&self, request: &str) -> bool {
        self.asked().iter().any(|seen| seen == request)
    }

    /// The `chap` route it holds now.
    pub(crate) fn route(&self) -> Option<Json> {
        self.state.lock().expect("the route").route.clone()
    }

    /// The `Authorization` header of the first authenticated request.
    pub(crate) fn authorization(&self) -> String {
        self.state
            .lock()
            .expect("the headers")
            .auth
            .first()
            .cloned()
            .unwrap_or_default()
    }
}

/// The version ids the App Hub stand-in publishes, and what each one installs.
///
/// The names are the App Hub's own, which are not the names chap-devops prints: it
/// publishes the Modeling App as `Modeling`, and a real instance lists it under
/// that name again.
#[cfg(unix)]
pub(crate) const STAND_IN_APPS: &[(&str, &str, &str, &str)] = &[
    (
        "a29851f9-82a7-4ecd-8b2c-58e0f220bc75",
        "mv-7.1.0",
        "Modeling",
        "7.1.0",
    ),
    (
        "effb986c-a3c7-485e-a2f6-5e54ff9df7c3",
        "cv-1.16.2",
        "DHIS2 Climate App",
        "1.16.2",
    ),
];

/// One published version, in the shape the App Hub actually sends.
///
/// **`maxDhisVersion` is an empty string and not an absent field.** Every one
/// of the 32 published versions of the Modeling App carries it that way, and a
/// stub tidier than that hid the bug where no app could ever be installed: the
/// blank was read as a maximum, 2.42.6 compared above it, and every version was
/// filtered out. The rest of the fields are here for the same reason - this is
/// what comes off the wire, not a convenient subset of it.
#[cfg(unix)]
pub(crate) fn stand_in_version(id: &str, version: &str, min: &str) -> Json {
    serde_json::json!({
        "created": 1789983819971i64,
        "demoUrl": "",
        "changeSummary": "",
        "downloadUrl": format!("https://apps.dhis2.org/api/v1/apps/download/dhis2/{version}.zip"),
        "id": id,
        "lastUpdated": 1789983819971i64,
        "maxDhisVersion": "",
        "minDhisVersion": min,
        "version": version,
        "channel": "stable",
    })
}

/// What a live DHIS2 2.42.6 answers a route write its allowlist refuses, as it
/// comes off the wire.
///
/// `errorCode` is in it because it is in the real answer, and because it is the
/// trap: `E1004` is DHIS2's `API query cannot be performed`, the code every
/// plain `ConflictException` carries - a malformed URL and a response timeout
/// out of range answer with it too. The message is what identifies this one.
#[cfg(unix)]
pub(crate) const ROUTE_NOT_PERMITTED_BODY: &str = r#"{"httpStatus":"Conflict","httpStatusCode":409,"status":"ERROR","message":"Route URL is not permitted","errorCode":"E1004"}"#;

/// The one route DHIS2 answers unauthenticated, and the shape of everything
/// else `chap-devops dhis2` asks for.
#[cfg(unix)]
pub(crate) fn dhis2_answer(
    state: &mut Dhis2State,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, String) {
    if path == "/api/ping" {
        return (200, "pong".to_string());
    }
    // The App Hub is not DHIS2 and is never authenticated.
    if let Some(id) = path.strip_prefix("/apphub/") {
        return match STAND_IN_APPS.iter().find(|(app, ..)| *app == id) {
            Some((app, version_id, name, version)) => (
                200,
                serde_json::json!({
                    "appType": "APP",
                    "status": "APPROVED",
                    "id": app,
                    "name": name,
                    "coreApp": false,
                    "versions": [
                        stand_in_version(version_id, version, "2.40"),
                        // An older one that wants a newer DHIS2 than the
                        // instance, so the pick is a pick and not the only row.
                        stand_in_version(&format!("{version_id}-old"), "0.9.0", "2.43"),
                    ],
                })
                .to_string(),
            ),
            None => (404, "{}".to_string()),
        };
    }
    // GitHub, for the unstable Modeling App: never authenticated either.
    if path == "/github/repos/dhis2-chap/chap-frontend/releases/tags/latest" {
        let Some(index) = state.unstable else {
            return (404, serde_json::json!({"message": "Not Found"}).to_string());
        };
        let (commit, _, digest) = STAND_IN_BUILDS[index];
        return (
            200,
            serde_json::json!({
                "tag_name": "latest",
                "prerelease": true,
                // What the real release keeps: the commit it was created for,
                // not the one the tag points at now.
                "target_commitish": "1111111111111111111111111111111111111111",
                "body": format!("Unstable build of {commit}. Use a versioned release for production."),
                "assets": [{
                    "name": "modeling-app-latest.zip",
                    "browser_download_url": format!("http://127.0.0.1:{}/github/download/{index}", state.port),
                    "digest": state.unstable_digest.unwrap_or(digest),
                    "updated_at": "2026-11-19T10:26:50Z",
                }],
            })
            .to_string(),
        );
    }
    if let Some(index) = path.strip_prefix("/github/download/") {
        return match index
            .parse::<usize>()
            .ok()
            .and_then(|i| STAND_IN_BUILDS.get(i))
        {
            Some((_, zip, _)) => (200, zip.to_string()),
            None => (404, "{}".to_string()),
        };
    }
    if state.unauthorized {
        return (
            401,
            serde_json::json!({"httpStatus": "Unauthorized", "message": "Unauthorized"})
                .to_string(),
        );
    }
    if path.starts_with("/api/me") {
        return (200, serde_json::json!({"username": "ops"}).to_string());
    }
    if path == "/api/system/info" {
        let mut info = serde_json::json!({"version": "2.42.6", "serverDate": server_date_now()});
        if state.epoch_analytics {
            info["lastAnalyticsTableSuccess"] = serde_json::json!("1970-01-01T00:00:00.000");
        }
        if state.inherited_analytics {
            info["lastAnalyticsTableSuccess"] = serde_json::json!("2026-06-16T07:51:00.093");
        }
        if state.analytics_started {
            info["lastAnalyticsTableSuccess"] = serde_json::json!("2026-09-25T10:01:00.000");
        }
        return (200, info.to_string());
    }
    if method == "POST" && path == "/api/apps" {
        // An unstable build carries the version of the release before it.
        state.uploads.push(body.to_string());
        state
            .apps
            .retain(|app| app["name"] != serde_json::json!("Modeling"));
        state
            .apps
            .push(serde_json::json!({"name": "Modeling", "version": "7.1.0"}));
        return (201, "{}".to_string());
    }
    if path == "/api/apps" {
        return (
            200,
            serde_json::Value::Array(state.apps.clone()).to_string(),
        );
    }
    if let Some(version_id) = path.strip_prefix("/api/appHub/") {
        return match STAND_IN_APPS
            .iter()
            .find(|(_, published, ..)| *published == version_id)
        {
            Some((_, _, name, version)) => {
                state
                    .apps
                    .retain(|app| app["name"] != serde_json::json!(name));
                state
                    .apps
                    .push(serde_json::json!({"name": name, "version": version}));
                (201, "{}".to_string())
            }
            None => (
                404,
                serde_json::json!({"message": "no such version"}).to_string(),
            ),
        };
    }
    if path.starts_with("/api/routes/chap/run/") {
        if state.proxy_failures > 0 {
            state.proxy_failures -= 1;
            return (
                500,
                serde_json::json!({"message": "Connection prematurely closed BEFORE response"})
                    .to_string(),
            );
        }
        if let Some((status, body)) = state.proxy_answer {
            return (status, body.to_string());
        }
        // The proxy answers only when the route points at this deployment's
        // chap-core, which is what makes the verification worth making.
        // Either this deployment's compose alias, or the chap-core URL the
        // external-DHIS2 tests record.
        let url = state.route.as_ref().map(|route| route["url"].clone());
        let ours = !state.proxy_fails
            && (url == Some(serde_json::json!("http://chap:8000/**"))
                || url == Some(serde_json::json!(EXTERNAL_CHAP_URL_TARGET)));
        // `/health` is open; everything else wants chap-core's token, which
        // only the route's `auth` headers can carry.
        let sent = state
            .route
            .as_ref()
            .and_then(|route| route["auth"]["headers"]["Authorization"].as_str())
            .map(str::to_string);
        let refused = !path.ends_with("/health")
            && state
                .chap_token
                .as_ref()
                .is_some_and(|token| sent != Some(format!("Bearer {token}")));
        if ours && refused {
            return (
                401,
                serde_json::json!({"detail": "Missing or invalid API token"}).to_string(),
            );
        }
        return match ours {
            true if !path.ends_with("/health") => (
                200,
                serde_json::json!({"count": 0, "services": []}).to_string(),
            ),
            true => (
                200,
                serde_json::json!({"status": "success", "message": "healthy"}).to_string(),
            ),
            false => (
                502,
                serde_json::json!({"message": "could not reach the route target"}).to_string(),
            ),
        };
    }
    // What the allowlist does to a route write, before either verb handles it.
    if state.route_not_permitted
        && ((method == "POST" && path == "/api/routes")
            || (method == "PUT" && path.starts_with("/api/routes/")))
    {
        return (409, ROUTE_NOT_PERMITTED_BODY.to_string());
    }
    if method == "POST" && path == "/api/routes" {
        let mut route: Json = serde_json::from_str(body).unwrap_or(serde_json::json!({}));
        route["id"] = serde_json::json!("route-1");
        state.route = Some(route);
        return (201, serde_json::json!({"status": "OK"}).to_string());
    }
    if method == "PUT" && path.starts_with("/api/routes/") {
        let mut route: Json = serde_json::from_str(body).unwrap_or(serde_json::json!({}));
        route["id"] = serde_json::json!("route-1");
        state.route = Some(route);
        return (200, serde_json::json!({"status": "OK"}).to_string());
    }
    if method == "GET" && path.starts_with("/api/routes") {
        // DHIS2 lists a route's auth type and keeps the headers to itself.
        let routes: Vec<Json> = state
            .route
            .clone()
            .map(|mut route| {
                if let Some(kind) = route["auth"].get("type").cloned() {
                    route["auth"] = serde_json::json!({"type": kind});
                }
                route
            })
            .into_iter()
            .collect();
        return (200, serde_json::json!({"routes": routes}).to_string());
    }
    if method == "POST" && path.starts_with("/api/resourceTables/analytics") {
        state.analytics_started = true;
        return (
            200,
            serde_json::json!({"response": {"id": "job-1", "jobType": "ANALYTICS_TABLE"}})
                .to_string(),
        );
    }
    if path.starts_with("/api/jobConfigurations?") {
        let mut jobs = Vec::new();
        if state.analytics_queued {
            jobs.push(serde_json::json!({
                "id": "job-q", "jobStatus": "SCHEDULED", "schedulingType": "ONCE_ASAP",
                "enabled": true,
            }));
        }
        return (
            200,
            serde_json::json!({"jobConfigurations": jobs}).to_string(),
        );
    }
    if path == "/api/system/tasks/ANALYTICS_TABLE" {
        let mut tasks = serde_json::json!({});
        if state.analytics_running {
            tasks["job-0"] = serde_json::json!([
                {"time": "2026-09-25T09:59:00.000", "level": "INFO", "message": "analytics_2023",
                 "completed": false},
            ]);
        }
        if state.analytics_started {
            tasks["job-1"] = dhis2_finished_job();
        }
        return (200, tasks.to_string());
    }
    if let Some(job) = path.strip_prefix("/api/system/tasks/ANALYTICS_TABLE/") {
        // A run that was already going finishes on the first poll, so the test
        // asserts the adoption rather than the sleeping.
        return match job {
            "job-0" | "job-1" | "job-q" => (200, dhis2_finished_job().to_string()),
            _ => (200, "[]".to_string()),
        };
    }
    (
        404,
        serde_json::json!({"message": "no such endpoint"}).to_string(),
    )
}

/// The notifications of an analytics run that has finished.
#[cfg(unix)]
pub(crate) fn dhis2_finished_job() -> Json {
    serde_json::json!([
        {"time": "2026-09-25T10:00:00.000", "level": "INFO", "message": "started",
         "completed": false},
        {"time": "2026-09-25T10:01:00.000", "level": "INFO",
         "message": "Analytics tables updated", "completed": true},
    ])
}

/// The `chap` route a climate demo dump ships: right code, right authority, not
/// disabled, and pointed at somebody else's Chap.
#[cfg(unix)]
pub(crate) fn external_chap_route() -> Json {
    serde_json::json!({
        "id": "route-demo",
        "name": "chap",
        "code": "chap",
        "url": "http://158.39.75.126/stable/**",
        "disabled": false,
        "authorities": ["F_CHAP_MODELING_APP"],
    })
}

/// `chap-devops -C <project> dhis2 ...` against a stand-in, with the fake docker on
/// PATH so the container check finds `dhis2` running.
///
/// With no `hub` the run is `--offline`, which is exactly what `route`,
/// `analytics` and `show` have to work under; a hub port is the App Hub
/// stand-in, and then the run is not offline because installing an app cannot be.
#[cfg(unix)]
pub(crate) fn dhis2_chap(
    sandbox: &Sandbox,
    dir: &Path,
    bin: &Path,
    hub: Option<u16>,
    args: &[&str],
) -> Command {
    let mut cmd = Command::cargo_bin("chap-devops").expect("the chap-devops binary is built");
    cmd.env("CHAP_DEVOPS_CACHE_DIR", sandbox.cache.path())
        .env("CHAP_DEVOPS_DATA_DIR", sandbox.cache.path().join("data"))
        .env("CHAP_DEVOPS_NO_UPDATE_CHECK", "1")
        .env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1")
        // A password or a token in the developer's own shell would otherwise
        // decide which source these tests report.
        .env_remove("CHAP_DEVOPS_DHIS2_PASSWORD")
        .env_remove("CHAP_DEVOPS_DHIS2_USERNAME")
        .env_remove("CHAP_DEVOPS_DHIS2_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .env(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .current_dir(dir);
    match hub {
        Some(port) => {
            cmd.env(
                "CHAP_DEVOPS_APP_HUB",
                format!("http://127.0.0.1:{port}/apphub"),
            )
            .env(
                "CHAP_DEVOPS_GITHUB_API",
                format!("http://127.0.0.1:{port}/github"),
            );
        }
        None => {
            cmd.arg("--offline");
        }
    }
    cmd.arg("-C").arg(dir).arg("dhis2").args(args);
    cmd
}

/// A deployment with a DHIS2 published where the stand-in is listening, plus
/// the fake docker that says its container is up.
#[cfg(unix)]
pub(crate) fn dhis2_connected(stand_in: &Dhis2StandIn) -> (Sandbox, PathBuf, TempDir, PathBuf) {
    let (sandbox, dir) = dhis2_sandbox(stand_in.port);
    let (temp, bin) = docker_running("dhis2");
    (sandbox, dir, temp, bin)
}

/// The chap-core URL the external-DHIS2 tests record, as DHIS2 would reach it.
pub(crate) const EXTERNAL_CHAP_URL: &str = "https://chap.example.org";

/// The route target that URL makes.
#[cfg(unix)]
pub(crate) const EXTERNAL_CHAP_URL_TARGET: &str = "https://chap.example.org/**";

/// `serverDate` as DHIS2 2.42.6 in a container writes it: the clock of a UTC
/// server, with no zone.
#[cfg(unix)]
fn server_date_now() -> String {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|age| age.as_secs())
        .unwrap_or_default() as i64;
    // Howard Hinnant's `civil_from_days`.
    let z = unix.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let rest = unix.rem_euclid(86_400);
    format!(
        "{year}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}
