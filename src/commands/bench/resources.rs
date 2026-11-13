//! `chap-devops bench --resources`: what the services of this deployment
//! used while `bench` ran, phase by phase.
//!
//! A thread samples `docker stats` for the services of the deployment
//! (chap, worker, postgres, redis) for as long as the run takes. Each phase
//! gets, for each service, its CPU seconds, its average and peak cores and
//! its peak memory. Before and after each phase, `bench` reads the
//! counters of Postgres (`pg_stat_database`) and Valkey (`INFO`), and the
//! phase gets the difference.
//!
//! Only for a chap-core that this deployment runs: with `--url`, docker
//! cannot see chap-core.

use crate::project::Project;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// The services whose use is reported.
const SERVICES: &[&str] = &["chap", "worker", "postgres", "redis"];

/// One reading of one service.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// Seconds since the sampler started.
    pub at: f64,
    pub service: String,
    /// Cores in use: `150%` is 1.5.
    pub cores: f64,
    pub memory_bytes: u64,
}

/// The use of one service in one phase.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Usage {
    pub service: String,
    pub cpu_seconds: f64,
    pub average_cores: f64,
    pub peak_cores: f64,
    pub peak_memory_bytes: u64,
    pub samples: usize,
}

/// One phase of the run, with what each service used and what the
/// database and Valkey did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Phase {
    pub phase: String,
    pub seconds: f64,
    pub services: Vec<Usage>,
    /// `pg_stat_database` counters, the difference over the phase.
    pub postgres: BTreeMap<String, i64>,
    /// Valkey `INFO` counters, the difference over the phase, and its peak
    /// memory at the end.
    pub valkey: BTreeMap<String, i64>,
}

/// The sampler and the phases so far.
pub struct Recorder {
    project: Project,
    containers: BTreeMap<String, String>,
    started: Instant,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Vec<Sample>>>,
    marks: Vec<(String, f64, f64, Counters, Counters)>,
    open: Option<(String, f64, Counters)>,
}

#[derive(Debug, Clone, Default)]
struct Counters {
    postgres: BTreeMap<String, i64>,
    valkey: BTreeMap<String, i64>,
}

impl Recorder {
    /// Start sampling the services of `project`, or `None` when docker
    /// cannot list them.
    pub fn start(project: &Project) -> Option<Recorder> {
        let containers: BTreeMap<String, String> = crate::docker::running_containers(project)?
            .into_iter()
            .filter(|c| SERVICES.contains(&c.service.as_str()))
            .map(|c| (c.name, c.service))
            .collect();
        if containers.is_empty() {
            return None;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        let thread = {
            let stop = stop.clone();
            let containers = containers.clone();
            std::thread::spawn(move || {
                let mut samples = Vec::new();
                while !stop.load(Ordering::Relaxed) {
                    // `docker stats --no-stream` takes about a second to
                    // measure, which is the sampling interval.
                    let at = started.elapsed().as_secs_f64();
                    for (name, usage) in crate::docker::container_usage() {
                        let Some(service) = containers.get(&name) else {
                            continue;
                        };
                        samples.push(Sample {
                            at,
                            service: service.clone(),
                            cores: parse_percent(&usage.cpu).unwrap_or(0.0) / 100.0,
                            memory_bytes: parse_memory(&usage.memory).unwrap_or(0),
                        });
                    }
                }
                samples
            })
        };
        Some(Recorder {
            project: project.clone(),
            containers,
            started,
            stop,
            thread: Some(thread),
            marks: Vec::new(),
            open: None,
        })
    }

    /// Start a phase, which ends the one before it.
    pub fn phase(&mut self, name: &str) {
        self.end_phase();
        let counters = self.counters();
        self.open = Some((
            name.to_string(),
            self.started.elapsed().as_secs_f64(),
            counters,
        ));
    }

    fn end_phase(&mut self) {
        if let Some((name, from, before)) = self.open.take() {
            let after = self.counters();
            let to = self.started.elapsed().as_secs_f64();
            self.marks.push((name, from, to, before, after));
        }
    }

    fn counters(&self) -> Counters {
        Counters {
            postgres: postgres_counters(&self.project),
            valkey: valkey_counters(&self.project),
        }
    }

    /// Stop sampling, and give each phase.
    pub fn finish(mut self) -> Vec<Phase> {
        self.end_phase();
        self.stop.store(true, Ordering::Relaxed);
        let samples = self
            .thread
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or_default();
        let services: Vec<String> = SERVICES
            .iter()
            .filter(|s| self.containers.values().any(|v| v == *s))
            .map(|s| s.to_string())
            .collect();
        self.marks
            .iter()
            .map(|(name, from, to, before, after)| Phase {
                phase: name.clone(),
                seconds: to - from,
                services: services
                    .iter()
                    .map(|s| usage(&samples, s, *from, *to))
                    .collect(),
                postgres: delta(&before.postgres, &after.postgres),
                valkey: {
                    let mut v = delta(&before.valkey, &after.valkey);
                    if let Some(peak) = after.valkey.get("used_memory_peak") {
                        v.insert("used_memory_peak".to_string(), *peak);
                    }
                    v
                },
            })
            .collect()
    }
}

/// What `service` used from `from` to `to`, from the samples in that time.
pub fn usage(samples: &[Sample], service: &str, from: f64, to: f64) -> Usage {
    let inside: Vec<&Sample> = samples
        .iter()
        .filter(|s| s.service == service && s.at >= from && s.at <= to)
        .collect();
    let average = match inside.len() {
        0 => 0.0,
        n => inside.iter().map(|s| s.cores).sum::<f64>() / n as f64,
    };
    Usage {
        service: service.to_string(),
        cpu_seconds: average * (to - from),
        average_cores: average,
        peak_cores: inside.iter().map(|s| s.cores).fold(0.0, f64::max),
        peak_memory_bytes: inside.iter().map(|s| s.memory_bytes).max().unwrap_or(0),
        samples: inside.len(),
    }
}

/// The counters that grew over a phase: `after - before`, for each key in
/// both.
pub fn delta(
    before: &BTreeMap<String, i64>,
    after: &BTreeMap<String, i64>,
) -> BTreeMap<String, i64> {
    after
        .iter()
        .filter(|(k, _)| k.as_str() != "used_memory_peak")
        .filter_map(|(k, v)| Some((k.clone(), v - before.get(k)?)))
        .collect()
}

/// `12.34%` as 12.34.
pub fn parse_percent(text: &str) -> Option<f64> {
    text.trim().trim_end_matches('%').parse().ok()
}

/// The used part of `181.2MiB / 7.654GiB`, in bytes.
pub fn parse_memory(text: &str) -> Option<u64> {
    let used = text.split('/').next()?.trim();
    let at = used.find(|c: char| c.is_ascii_alphabetic())?;
    let (number, unit) = used.split_at(at);
    let number: f64 = number.trim().parse().ok()?;
    let factor = match unit {
        "B" => 1.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0_f64.powi(4),
        _ => return None,
    };
    Some((number * factor) as u64)
}

const POSTGRES_COLUMNS: &[&str] = &[
    "xact_commit",
    "tup_returned",
    "tup_fetched",
    "tup_inserted",
    "tup_updated",
    "tup_deleted",
    "blks_read",
    "blks_hit",
    "temp_bytes",
];

const VALKEY_KEYS: &[&str] = &[
    "total_commands_processed",
    "total_net_input_bytes",
    "total_net_output_bytes",
    "used_memory_peak",
];

fn exec(project: &Project, service: &str, script: &str) -> Option<String> {
    let args = ["exec", "-T", service, "sh", "-c", script].map(str::to_string);
    match crate::docker::compose_output(project, &args) {
        Ok((0, text, _)) => Some(text),
        _ => None,
    }
}

fn postgres_counters(project: &Project) -> BTreeMap<String, i64> {
    let query = format!(
        "select {} from pg_stat_database where datname = current_database()",
        POSTGRES_COLUMNS.join(", ")
    );
    let script = format!("psql -U \"$POSTGRES_USER\" -d \"$POSTGRES_DB\" -At -c '{query}'");
    exec(project, "postgres", &script)
        .map(|text| parse_postgres(&text))
        .unwrap_or_default()
}

/// One `psql -At` row of [`POSTGRES_COLUMNS`].
pub fn parse_postgres(text: &str) -> BTreeMap<String, i64> {
    let row = text.lines().next().unwrap_or_default();
    POSTGRES_COLUMNS
        .iter()
        .zip(row.split('|'))
        .filter_map(|(k, v)| Some((k.to_string(), v.trim().parse().ok()?)))
        .collect()
}

fn valkey_counters(project: &Project) -> BTreeMap<String, i64> {
    exec(
        project,
        "redis",
        "valkey-cli INFO 2>/dev/null || redis-cli INFO",
    )
    .map(|text| parse_valkey(&text))
    .unwrap_or_default()
}

/// The [`VALKEY_KEYS`] of an `INFO` answer.
pub fn parse_valkey(text: &str) -> BTreeMap<String, i64> {
    text.lines()
        .filter_map(|line| line.trim().split_once(':'))
        .filter(|(k, _)| VALKEY_KEYS.contains(k))
        .filter_map(|(k, v)| Some((k.to_string(), v.trim().parse().ok()?)))
        .collect()
}

/// One line for each phase: `import: chap 3.1 CPU s (peak 1.0 cores,
/// 410.0 MB), postgres ...`.
pub fn lines(phases: &[Phase]) -> Vec<String> {
    phases
        .iter()
        .map(|p| {
            let services: Vec<String> = p
                .services
                .iter()
                .filter(|u| u.samples > 0)
                .map(|u| {
                    format!(
                        "{} {:.1} CPU s (peak {:.1} cores, {})",
                        u.service,
                        u.cpu_seconds,
                        u.peak_cores,
                        super::measure::human_bytes(u.peak_memory_bytes as usize)
                    )
                })
                .collect();
            let rows = p.postgres.get("tup_inserted").copied().unwrap_or(0)
                + p.postgres.get("tup_updated").copied().unwrap_or(0)
                + p.postgres.get("tup_deleted").copied().unwrap_or(0);
            let read = p.postgres.get("tup_returned").copied().unwrap_or(0);
            format!(
                "{} ({}): {}; postgres read {read} and wrote {rows} rows",
                p.phase,
                super::measure::human_ms(p.seconds * 1000.0),
                services.join(", ")
            )
        })
        .collect()
}
