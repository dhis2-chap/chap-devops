//! `chap-devops bench --write --second-user`: how long a small page takes
//! for a second person while the run imports a dataset and backtests it.
//!
//! A thread asks for `GET /v1/crud/configured-models`, the list that most
//! pages of the Modeling App ask for first, every half second from the start
//! of the run to its end. Each phase of the run gets the latency of the
//! requests that started in it. A phase whose requests wait long means that
//! the work of the run holds up everyone else on the server.

use super::measure::{human_ms, percentile};
use crate::api::Api;
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The request the second user sends.
pub const PATH: &str = "/v1/crud/configured-models";

/// How often the second user asks.
const EVERY: Duration = Duration::from_millis(500);

/// One request of the second user.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Seconds from the start of the run to the start of the request.
    pub at: f64,
    pub ms: f64,
    pub ok: bool,
}

/// What the second user saw in one phase.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Phase {
    pub phase: String,
    pub requests: usize,
    pub failed: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

/// The thread of the second user.
pub struct SecondUser {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<Vec<Request>>,
}

impl SecondUser {
    /// Start asking, with `started` as the start of the run.
    pub fn start(api: Api, started: Instant) -> SecondUser {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut requests = Vec::new();
                while !stop.load(Ordering::Relaxed) {
                    let at = started.elapsed().as_secs_f64();
                    let sent = Instant::now();
                    let ok = matches!(api.send("GET", PATH, None), Ok(a) if a.is_success());
                    requests.push(Request {
                        at,
                        ms: sent.elapsed().as_secs_f64() * 1000.0,
                        ok,
                    });
                    if let Some(rest) = EVERY.checked_sub(sent.elapsed()) {
                        std::thread::sleep(rest);
                    }
                }
                requests
            })
        };
        SecondUser { stop, thread }
    }

    /// Stop asking, and give what each phase saw. `phases` are the name and
    /// the start of each phase, in seconds from the start of the run; a
    /// phase ends where the next one starts.
    pub fn finish(self, phases: &[(String, f64)]) -> Vec<Phase> {
        self.stop.store(true, Ordering::Relaxed);
        let requests = self.thread.join().unwrap_or_default();
        summarize(&requests, phases)
    }
}

/// What each phase saw, from the requests that started in it.
pub fn summarize(requests: &[Request], phases: &[(String, f64)]) -> Vec<Phase> {
    phases
        .iter()
        .enumerate()
        .map(|(i, (name, from))| {
            let to = phases.get(i + 1).map_or(f64::INFINITY, |(_, t)| *t);
            let inside: Vec<&Request> = requests
                .iter()
                .filter(|r| r.at >= *from && r.at < to)
                .collect();
            let mut ms: Vec<f64> = inside.iter().map(|r| r.ms).collect();
            ms.sort_by(|a, b| a.total_cmp(b));
            Phase {
                phase: name.clone(),
                requests: inside.len(),
                failed: inside.iter().filter(|r| !r.ok).count(),
                p50_ms: percentile(&ms, 50.0),
                p95_ms: percentile(&ms, 95.0),
                max_ms: ms.last().copied().unwrap_or_default(),
            }
        })
        .collect()
}

/// `second user, import: 17 requests, p50 12 ms, p95 1.20 s, max 3.40 s`.
pub fn line(phase: &Phase) -> String {
    let failed = match phase.failed {
        0 => String::new(),
        n => format!(", {n} failed"),
    };
    format!(
        "second user, {}: {} requests of configured-models, p50 {}, p95 {}, max {}{failed}",
        phase.phase,
        phase.requests,
        human_ms(phase.p50_ms),
        human_ms(phase.p95_ms),
        human_ms(phase.max_ms)
    )
}
