//! `docker stats`: what each running container uses right now.

use super::docker_capture;
use std::collections::BTreeMap;

/// One container's CPU and memory, as `docker stats` prints them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Usage {
    /// `CPUPerc`, e.g. `12.34%`.
    pub cpu: String,
    /// `MemUsage`, e.g. `181.2MiB / 7.654GiB`.
    pub memory: String,
}

/// Every running container's usage, keyed by container name. Empty when
/// docker cannot be asked: the numbers are a nicety, never a reason to fail.
pub fn container_usage() -> BTreeMap<String, Usage> {
    let args = ["stats", "--no-stream", "--format", "{{json .}}"].map(str::to_string);
    docker_capture(&args)
        .map(|text| parse_stats(&text))
        .unwrap_or_default()
}

/// One JSON object per line, as `--format '{{json .}}'` writes them.
pub(super) fn parse_stats(text: &str) -> BTreeMap<String, Usage> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|row| {
            let name = row.get("Name")?.as_str()?.to_string();
            let field = |key: &str| {
                row.get(key)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            Some((
                name,
                Usage {
                    cpu: field("CPUPerc"),
                    memory: field("MemUsage"),
                },
            ))
        })
        .collect()
}

/// The CPUs and the memory in bytes that the docker engine has, from
/// `docker info`. On macOS and Windows this is the docker VM, not the
/// machine.
pub fn engine_resources() -> Option<(u64, u64)> {
    let args = ["info", "--format", "{{.NCPU}} {{.MemTotal}}"].map(str::to_string);
    parse_engine_resources(&docker_capture(&args)?)
}

pub(super) fn parse_engine_resources(text: &str) -> Option<(u64, u64)> {
    let mut parts = text.split_whitespace();
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}
