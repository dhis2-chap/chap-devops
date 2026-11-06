//! `chap-devops top --record FILE`: the same look at docker as the screen, written
//! as one JSON line per service at each tick, so a load test has the CPU and
//! memory of every container over time.

use super::data::{Node, Service};
use serde::Serialize;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

/// One service at one tick.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sample {
    /// Unix seconds.
    pub unix: u64,
    /// The same moment in this machine's zone, RFC 3339.
    pub time: String,
    pub deployment: String,
    pub service: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub state: String,
    /// `docker stats` CPU, in percent of one core.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_limit_bytes: Option<u64>,
}

/// What a recording wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub ticks: u64,
    pub lines: u64,
    pub seconds: u64,
}

/// The samples of one tick, one for each service of each deployment.
pub fn samples(nodes: &[Node], unix: u64) -> Vec<Sample> {
    let time = crate::output::local_rfc3339(unix);
    nodes
        .iter()
        .flat_map(|node| {
            let time = time.clone();
            node.services
                .iter()
                .map(move |s| sample(&node.name, s, unix, &time))
        })
        .collect()
}

fn sample(deployment: &str, s: &Service, unix: u64, time: &str) -> Sample {
    let (used, limit) = s
        .memory
        .as_deref()
        .map(parse_memory)
        .unwrap_or((None, None));
    Sample {
        unix,
        time: time.to_string(),
        deployment: deployment.to_string(),
        service: s.service.clone(),
        role: s.role.clone(),
        model: s.model.clone(),
        state: s.state.clone(),
        cpu_percent: s.cpu.as_deref().and_then(parse_percent),
        memory_bytes: used,
        memory_limit_bytes: limit,
    }
}

/// `12.34%` as `12.34`.
pub fn parse_percent(text: &str) -> Option<f64> {
    text.trim().trim_end_matches('%').trim().parse().ok()
}

/// `181.2MiB / 7.654GiB` as the two sizes in bytes.
pub fn parse_memory(text: &str) -> (Option<u64>, Option<u64>) {
    let mut parts = text.split('/');
    let used = parts.next().and_then(parse_size);
    let limit = parts.next().and_then(parse_size);
    (used, limit)
}

/// One size as `docker stats` prints it: `512B`, `1.5KiB`, `181.2MiB`, `2GB`.
pub fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.trim().parse().ok()?;
    let factor: f64 = match unit.trim() {
        "" | "B" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        _ => return None,
    };
    Some((number * factor).round() as u64)
}

/// Append a tick's samples to `file`, one JSON object per line.
pub fn append(file: &mut impl Write, samples: &[Sample]) -> std::io::Result<()> {
    for sample in samples {
        serde_json::to_writer(&mut *file, sample)?;
        file.write_all(b"\n")?;
    }
    file.flush()
}

/// Look at docker every `interval` seconds and append to `path` until
/// `duration` seconds have passed (if one is given) or Ctrl-C.
pub fn record(
    path: &Path,
    known: &[std::path::PathBuf],
    interval: u64,
    duration: Option<u64>,
) -> std::io::Result<Summary> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let start = Instant::now();
    let mut summary = Summary::default();
    loop {
        let nodes = super::data::collect(known);
        let tick = samples(&nodes, unix_now());
        append(&mut file, &tick)?;
        summary.ticks += 1;
        summary.lines += tick.len() as u64;
        let next = Duration::from_secs(interval * summary.ticks);
        loop {
            let elapsed = start.elapsed();
            if crate::interrupt::requested()
                || duration.is_some_and(|d| elapsed >= Duration::from_secs(d))
            {
                summary.seconds = elapsed.as_secs();
                return Ok(summary);
            }
            if elapsed >= next {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
