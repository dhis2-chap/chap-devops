//! The numbers `chap-devops bench` reports for each request.

use serde::Serialize;
use std::collections::BTreeMap;

/// One request as it went.
#[derive(Debug, Clone, PartialEq)]
pub struct Timing {
    pub ms: f64,
    /// The HTTP status, or `None` when no answer came.
    pub status: Option<u16>,
    pub bytes: usize,
    /// Why it failed: no answer, or what chap-core said.
    pub error: Option<String>,
}

impl Timing {
    pub fn failed(&self) -> bool {
        self.error.is_some()
    }
}

/// Every request to one route, in numbers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RouteResult {
    pub label: String,
    pub method: String,
    pub path: String,
    pub requests: usize,
    pub failed: usize,
    pub min_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    /// The body size of the last answer.
    pub bytes: usize,
    pub statuses: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The nearest-rank percentile of `sorted`, which must be sorted. `0.0` for
/// an empty list.
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// The result of one route from its timings, failed requests included: a
/// request that timed out took that long for the person who waited.
pub fn summarize(label: &str, method: &str, path: &str, timings: &[Timing]) -> RouteResult {
    let mut ms: Vec<f64> = timings.iter().map(|t| t.ms).collect();
    ms.sort_by(|a, b| a.total_cmp(b));
    let mut statuses = BTreeMap::new();
    for t in timings {
        let key = t
            .status
            .map(|s| s.to_string())
            .unwrap_or_else(|| "no answer".to_string());
        *statuses.entry(key).or_insert(0) += 1;
    }
    RouteResult {
        label: label.to_string(),
        method: method.to_string(),
        path: path.to_string(),
        requests: timings.len(),
        failed: timings.iter().filter(|t| t.failed()).count(),
        min_ms: ms.first().copied().unwrap_or_default(),
        p50_ms: percentile(&ms, 50.0),
        p95_ms: percentile(&ms, 95.0),
        max_ms: ms.last().copied().unwrap_or_default(),
        bytes: timings.last().map(|t| t.bytes).unwrap_or_default(),
        statuses,
        error: timings.iter().find_map(|t| t.error.clone()),
    }
}

/// `12 ms` under a second, `1.24 s` from one second.
pub fn human_ms(ms: f64) -> String {
    if ms < 1000.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// `512 B`, `12.3 kB`, `4.5 MB`.
pub fn human_bytes(bytes: usize) -> String {
    let b = bytes as f64;
    if b < 1000.0 {
        format!("{bytes} B")
    } else if b < 1e6 {
        format!("{:.1} kB", b / 1e3)
    } else {
        format!("{:.1} MB", b / 1e6)
    }
}
