//! What `chap-devops bench` measured on: the chap-core version, how many
//! processes answer the API and run the jobs, and the machine. Two runs can
//! only be compared when these are known.
//!
//! The version comes from `/system/info`, for any chap-core. The process
//! counts and the docker engine come from docker, so only for a chap-core
//! that this deployment runs. The counts are the processes that run, from
//! `docker compose top`, not the settings: a setting that chap-core does not
//! use would give a wrong number.

use crate::api::Api;
use crate::commands::Ctx;
use serde::Serialize;
use std::collections::BTreeSet;

/// What the run was measured on. A field is `None` when it is not known.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Server {
    pub chap_core_version: Option<String>,
    pub revision: Option<String>,
    /// The gunicorn worker processes of the `chap` service.
    pub api_workers: Option<usize>,
    /// The celery worker processes of the `worker` service.
    pub worker_processes: Option<usize>,
    /// Where chap-devops runs: `macos aarch64`.
    pub machine: String,
    /// The CPUs of the machine chap-devops runs on.
    pub cpus: Option<usize>,
    /// The CPUs and the memory of the docker engine, for a chap-core that
    /// this deployment runs.
    pub docker_cpus: Option<u64>,
    pub docker_memory_bytes: Option<u64>,
    /// Whether chap-core packed an answer with gzip when asked, as the
    /// DHIS2 route asks. `None` when the answer was too small to tell.
    pub gzip: Option<bool>,
    /// What the answer says came between: `Server` and `Via`. A proxy, or
    /// the DHIS2 route, can pack, unpack or hold back answers.
    pub transport: crate::api::Transport,
}

/// chap-core packs answers of this size and more (`minimum_size` of its
/// `GZipMiddleware`); a smaller answer says nothing about gzip.
const GZIP_MINIMUM: usize = 1000;

/// Whether an answer to a request for gzip came packed, and what it says
/// came between.
pub fn encoding(answer: &crate::api::Answer) -> (Option<bool>, crate::api::Transport) {
    let packed = answer
        .transport
        .content_encoding
        .as_deref()
        .is_some_and(|e| e.eq_ignore_ascii_case("gzip"));
    let known = packed || answer.body.len() >= GZIP_MINIMUM;
    (known.then_some(packed), answer.transport.clone())
}

/// Ask chap-core and, for a local chap-core, docker.
pub fn describe(ctx: &Ctx, api: &Api, local: bool) -> Server {
    let info = api.get_json("/system/info").ok();
    let field = |key: &str| {
        info.as_ref()
            .and_then(|i| i.get(key))
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    // An answer that is always large enough to be packed: the configured
    // models, with their templates.
    let (gzip, transport) = match api.send("GET", "/v1/crud/configured-models", None) {
        Ok(answer) if answer.is_success() => encoding(&answer),
        _ => (None, crate::api::Transport::default()),
    };
    let mut server = Server {
        gzip,
        transport,
        chap_core_version: field("chap_core_version"),
        revision: field("revision"),
        machine: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        cpus: std::thread::available_parallelism().ok().map(|n| n.get()),
        ..Server::default()
    };
    let no_docker = matches!(
        std::env::var("CHAP_DEVOPS_NO_DOCKER_PROBE").as_deref(),
        Ok("1") | Ok("true")
    );
    if !local || no_docker {
        return server;
    }
    if let Ok(project) = ctx.project() {
        let args = ["top", "chap", "worker"].map(str::to_string);
        if let Ok((0, text, _)) = crate::docker::compose_output(&project, &args) {
            server.api_workers = workers(&text, "chap");
            server.worker_processes = workers(&text, "worker");
        }
    }
    if let Some((cpus, memory)) = crate::docker::engine_resources() {
        server.docker_cpus = Some(cpus);
        server.docker_memory_bytes = Some(memory);
    }
    server
}

/// The worker processes of `service` in `docker compose top` output: the
/// children of its main process. The main process is the one whose parent
/// is not in the service, or is docker's init.
pub fn workers(text: &str, service: &str) -> Option<usize> {
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next()?.split_whitespace().collect();
    let col = |name: &str| header.iter().position(|h| *h == name);
    let (svc, pid, ppid, cmd) = (col("SERVICE")?, col("PID")?, col("PPID")?, col("CMD")?);
    let rows: Vec<(u64, u64, String)> = lines
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            (parts.get(svc)? == &service).then_some(())?;
            Some((
                parts.get(pid)?.parse().ok()?,
                parts.get(ppid)?.parse().ok()?,
                parts.get(cmd..)?.join(" "),
            ))
        })
        .collect();
    if rows.is_empty() {
        return None;
    }
    let pids: BTreeSet<u64> = rows.iter().map(|r| r.0).collect();
    let inits: BTreeSet<u64> = rows
        .iter()
        .filter(|r| r.2.contains("docker-init"))
        .map(|r| r.0)
        .collect();
    let mains: BTreeSet<u64> = rows
        .iter()
        .filter(|r| !inits.contains(&r.0))
        .filter(|r| !pids.contains(&r.1) || inits.contains(&r.1))
        .map(|r| r.0)
        .collect();
    Some(rows.iter().filter(|r| mains.contains(&r.1)).count())
}

/// One line for the report: `chap-core 2.7.0 (62bae69c), 1 API worker,
/// 8 worker processes, docker with 8 CPUs and 7.7 GB, macos aarch64`.
pub fn line(server: &Server) -> String {
    let mut parts = Vec::new();
    match (&server.chap_core_version, &server.revision) {
        (Some(v), Some(r)) => parts.push(format!("chap-core {v} ({})", short(r))),
        (Some(v), None) => parts.push(format!("chap-core {v}")),
        _ => parts.push("chap-core of unknown version".to_string()),
    }
    let count = |n: usize, one: &str, many: &str| match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    };
    if let Some(n) = server.api_workers {
        parts.push(count(n, "API worker", "API workers"));
    }
    if let Some(n) = server.worker_processes {
        parts.push(count(n, "worker process", "worker processes"));
    }
    if let (Some(cpus), Some(memory)) = (server.docker_cpus, server.docker_memory_bytes) {
        parts.push(format!(
            "docker with {cpus} CPUs and {}",
            super::measure::human_bytes(memory as usize)
        ));
    }
    match server.gzip {
        Some(true) => parts.push("answers packed with gzip".to_string()),
        Some(false) => parts.push("answers not packed, though gzip was asked for".to_string()),
        None => {}
    }
    if let Some(name) = &server.transport.server
        && !name.eq_ignore_ascii_case("uvicorn")
    {
        parts.push(format!("server `{name}`"));
    }
    if let Some(via) = &server.transport.via {
        parts.push(format!("via `{via}`"));
    }
    match server.cpus {
        Some(cpus) => parts.push(format!("{} with {cpus} CPUs", server.machine)),
        None => parts.push(server.machine.clone()),
    }
    format!("measured on {}", parts.join(", "))
}

fn short(revision: &str) -> &str {
    revision.get(..8).unwrap_or(revision)
}
