//! chap-core's job list: the shape of a job, and how it is put on a terminal.
//!
//! A job is one unit of work chap-core handed to its Celery worker - a
//! dataset being built, a backtest, a prediction - and `/v1/jobs` is the only
//! place a deployment can be asked what it has been doing. Everything here is
//! pure: the requests live in [`crate::commands::jobs`], so the table, the
//! closing line, the id matching and the reading of a failed job's log can all
//! be tested without a server.
//!
//! The one thing worth knowing about the API: `GET /v1/jobs/{id}/logs` is
//! where the real error text is. The job description carries a status and
//! nothing else, so a `FAILURE` says only that something went wrong; the
//! traceback and the model's own stdout and stderr are in the log.

use crate::output;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The endpoint the job list comes from.
pub const JOBS_PATH: &str = "/v1/jobs";

/// The table `chap-devops jobs` prints.
pub const HEADERS: &[&str] = &["ID", "TYPE", "NAME", "STATUS", "STARTED", "DURATION"];

/// Characters of an id shown in the `ID` column when that is enough.
///
/// chap-core's ids are Celery task ids, which are UUIDs: eight hex characters
/// separate them in any list a deployment will ever hold, and the full 36 make
/// every other column unreadable.
pub const SHORT_ID: usize = 8;

/// What a cell with nothing to say prints, as everywhere else in this CLI.
const EMPTY: &str = "-";

/// The line that separates a model's output from its error stream in a job
/// log. chapkit writes it, and it is the fastest way to the real message.
pub const STDERR_MARKER: &str = "--- stderr ---";

/// How much of a stderr line the failure hint keeps.
const MAX_HINT: usize = 160;

/// What `chap-devops jobs` says when chap-core has never run anything.
pub const NO_JOBS: &str = "no jobs yet; a backtest or prediction started from the Modeling App \
                           or `chap-devops api` shows up here";

/// The status in the body of `GET /v1/jobs/{id}`.
///
/// chap-core after v2.5.1 answers with the whole job, as one entry of
/// `GET /v1/jobs`, and earlier releases with the bare status string.
pub fn status_in(body: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(serde_json::Value::String(status)) => status,
        Ok(serde_json::Value::Object(job)) => job
            .get("status")
            .and_then(|status| status.as_str())
            .unwrap_or_default()
            .to_string(),
        _ => body.trim().trim_matches('"').to_string(),
    }
}

/// One entry of `GET /v1/jobs`.
///
/// Every field is optional on the way in, because the list is chap-core's to
/// grow: a description that gains a field must not stop `chap-devops jobs` from
/// printing the rows it already understands.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Job {
    pub id: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub end_time: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub prediction_setup_id: Option<i64>,
}

/// Which of the four things a status means.
///
/// chap-core passes Celery's states straight through, so the set is Celery's:
/// `PENDING`, `STARTED`, `RETRY`, `SUCCESS`, `FAILURE`, `REVOKED`. Anything
/// else is counted as running, because a state this CLI has not heard of is
/// more likely a new one for work in flight than a new way to be finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Running,
    Done,
    Failed,
    Cancelled,
}

impl Outcome {
    /// The outcome a status string names.
    pub fn of(status: &str) -> Outcome {
        match status.trim().to_ascii_uppercase().as_str() {
            "SUCCESS" => Outcome::Done,
            "FAILURE" => Outcome::Failed,
            "REVOKED" => Outcome::Cancelled,
            _ => Outcome::Running,
        }
    }

    /// Whether the job is still in flight.
    pub fn is_running(self) -> bool {
        self == Outcome::Running
    }
}

impl Job {
    /// What this job's status means.
    pub fn outcome(&self) -> Outcome {
        Outcome::of(&self.status)
    }

    /// Seconds since the epoch the job started at, `None` while it is queued.
    pub fn started_at(&self) -> Option<u64> {
        self.start_time
            .as_deref()
            .and_then(crate::status::parse_rfc3339)
    }

    /// Seconds since the epoch the job finished at, `None` while it runs.
    pub fn ended_at(&self) -> Option<u64> {
        self.end_time
            .as_deref()
            .and_then(crate::status::parse_rfc3339)
    }

    /// How long the job took, once it is finished.
    pub fn duration(&self) -> Option<Duration> {
        let (start, end) = (self.started_at()?, self.ended_at()?);
        Some(Duration::from_secs(end.saturating_sub(start)))
    }

    /// The one line `chap-devops jobs logs` opens with, on stderr.
    pub fn headline(&self) -> String {
        format!(
            "job {} {} ({} {})",
            self.id,
            or_empty(&self.status),
            or_empty(&self.kind),
            or_empty(&self.name)
        )
    }
}

/// The indices of `jobs`, newest first, with a running job ahead of a
/// finished one that started at the same second.
///
/// A queued job has no start time at all and belongs at the top: it is the
/// most recent thing that happened to this deployment, and it is the one an
/// operator is waiting on.
///
/// Indices rather than a sorted list, because the caller holds the wire
/// documents beside the parsed jobs and has to reorder both: `--json` hands
/// back what chap-core sent, fields this CLI has never heard of included, and
/// it has to be in the order the table was.
pub fn order(jobs: &[Job]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..jobs.len()).collect();
    order.sort_by(|a, b| newest_first(&jobs[*a], &jobs[*b]));
    order
}

/// [`order`]'s comparison, written out so the rule reads in one place.
fn newest_first(a: &Job, b: &Job) -> std::cmp::Ordering {
    let key = |job: &Job| {
        (
            job.started_at().unwrap_or(u64::MAX),
            job.outcome().is_running(),
        )
    };
    key(b)
        .cmp(&key(a))
        // Two jobs that started in the same second still have to come out in
        // the same order every time, or a table changes under a reader.
        .then_with(|| a.id.cmp(&b.id))
}

/// Whether [`SHORT_ID`] characters tell every one of these jobs apart.
///
/// All or nothing: a table where some ids are short and others are not reads
/// as though the long ones mean something.
pub fn ids_are_short(jobs: &[Job]) -> bool {
    let mut seen: Vec<&str> = Vec::with_capacity(jobs.len());
    for job in jobs {
        if job.id.chars().count() <= SHORT_ID {
            return false;
        }
        let prefix = &job.id[..prefix_end(&job.id)];
        if seen.contains(&prefix) {
            return false;
        }
        seen.push(prefix);
    }
    true
}

/// The id as the `ID` column prints it.
pub fn id_cell(id: &str, short: bool) -> String {
    if !short || id.chars().count() <= SHORT_ID {
        return id.to_string();
    }
    format!("{}...", &id[..prefix_end(id)])
}

/// The byte offset [`SHORT_ID`] characters into `id`.
fn prefix_end(id: &str) -> usize {
    id.char_indices()
        .nth(SHORT_ID)
        .map(|(at, _)| at)
        .unwrap_or(id.len())
}

/// One table row per job, in the order [`HEADERS`] names.
pub fn rows(jobs: &[Job], now: u64) -> Vec<Vec<String>> {
    let short = ids_are_short(jobs);
    jobs.iter()
        .map(|job| {
            vec![
                id_cell(&job.id, short),
                or_empty(&job.kind),
                or_empty(&job.name),
                or_empty(&job.status),
                started_cell(job, now),
                duration_cell(job),
            ]
        })
        .collect()
}

/// The `STARTED` cell: how long ago, or `queued` for a job that has not begun.
fn started_cell(job: &Job, now: u64) -> String {
    match job.started_at() {
        Some(at) => output::ago(Duration::from_secs(now.saturating_sub(at))),
        None if job.outcome().is_running() => "queued".to_string(),
        None => EMPTY.to_string(),
    }
}

/// The `DURATION` cell, which only a finished job has.
fn duration_cell(job: &Job) -> String {
    match job.duration() {
        Some(took) => took_text(took),
        None => EMPTY.to_string(),
    }
}

/// A duration as a cell: `9s`, `2m 9s`, `1h 3m`.
///
/// Two units at most, and never the smallest one on something that took
/// hours: a column of durations is read for the shape of it.
pub fn took_text(took: Duration) -> String {
    let secs = took.as_secs();
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m {}s", secs / 60, secs % 60),
        _ => format!("{}h {}m", secs / 3600, (secs % 3600) / 60),
    }
}

/// `8 jobs: 7 done, 1 failed` - what the table adds up to.
///
/// Only the buckets that have something in them are named, so a deployment
/// where nothing has failed is not handed a `0 failed` to read past.
pub fn summary(jobs: &[Job]) -> String {
    let count = |want: Outcome| jobs.iter().filter(|j| j.outcome() == want).count();
    let parts: Vec<String> = [
        (Outcome::Running, "running"),
        (Outcome::Done, "done"),
        (Outcome::Failed, "failed"),
        (Outcome::Cancelled, "cancelled"),
    ]
    .iter()
    .filter_map(|(outcome, label)| match count(*outcome) {
        0 => None,
        n => Some(format!("{n} {label}")),
    })
    .collect();
    let total = format!("{} {}", jobs.len(), plural(jobs.len(), "job"));
    if parts.is_empty() {
        return total;
    }
    format!("{total}: {}", parts.join(", "))
}

/// The line under the summary when something failed, naming the job to look
/// at when there is only one to name.
pub fn failure_hint(jobs: &[Job]) -> Option<String> {
    let failed: Vec<&Job> = jobs
        .iter()
        .filter(|j| j.outcome() == Outcome::Failed)
        .collect();
    let id = match failed.as_slice() {
        [] => return None,
        [one] => one.id.clone(),
        _ => "<id>".to_string(),
    };
    Some(format!("run `chap-devops jobs logs {id}` to see why"))
}

/// What a given id matched in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Matched {
    /// The id, exactly as chap-core spells it.
    One(String),
    /// Nothing in the list starts with it.
    None,
    /// More than one job does, and picking one would be a guess.
    Many(usize),
}

/// Resolve `given` against the list: the exact id, else a unique prefix.
///
/// An exact match always wins, so a full id can never be read as a prefix of
/// a longer one; matching is case-insensitive because a hex id copied out of a
/// log may have come back upper-case.
pub fn resolve(jobs: &[Job], given: &str) -> Matched {
    let given = given.trim();
    if given.is_empty() {
        return Matched::None;
    }
    if let Some(job) = jobs.iter().find(|j| j.id.eq_ignore_ascii_case(given)) {
        return Matched::One(job.id.clone());
    }
    let lower = given.to_ascii_lowercase();
    let hits: Vec<&Job> = jobs
        .iter()
        .filter(|j| j.id.to_ascii_lowercase().starts_with(&lower))
        .collect();
    match hits.as_slice() {
        [] => Matched::None,
        [one] => Matched::One(one.id.clone()),
        many => Matched::Many(many.len()),
    }
}

/// The error for an id that named no job, or too many.
pub fn no_such_job(given: &str, matched: &Matched) -> anyhow::Error {
    match matched {
        Matched::Many(n) => anyhow::anyhow!(
            "job {given} matches {n} jobs; run `chap-devops jobs` to list them and pass more of the id"
        ),
        _ => anyhow::anyhow!("job {given} not found; run `chap-devops jobs` to list them"),
    }
}

/// The last `n` lines of a log, or all of it when it is shorter.
pub fn tail(text: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(n);
    let mut out = lines[from..].join("\n");
    if text.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out
}

/// The one line of a failed job's stderr most likely to say why.
///
/// Best effort, and honest about it: a model writes whatever it writes. The
/// rule is the last line of the `--- stderr ---` section that is not a
/// warning, preferring one that names an error, because an R model ends its
/// stderr with pages of package chatter and a bare `Execution halted`.
pub fn stderr_hint(text: &str) -> Option<String> {
    let section = text.rsplit_once(STDERR_MARKER)?.1;
    let lines: Vec<&str> = section
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let at = error_line(&lines)?;
    let mut hint = lines[at].to_string();
    // R writes `Error in f() :` and puts the message on the next line, so a
    // hint that ends on the colon would be the half without the reason in it.
    if hint.ends_with(':')
        && let Some(next) = lines.get(at + 1)
    {
        hint = format!("{hint} {next}");
    }
    Some(cut(&hint, MAX_HINT))
}

/// The index of the line of `lines` most likely to say why something failed.
///
/// The last line that names an error, else the last line that is neither a
/// warning nor a runtime announcing that it stopped. Shared with
/// [`crate::modeltest`], which applies it to the `stderr tail:` chapkit puts
/// on one line of its own rather than to a log with sections in it.
pub fn error_line(lines: &[&str]) -> Option<usize> {
    let useful = |line: &&str| !is_warning(line) && !is_terminator(line);
    lines
        .iter()
        .rposition(|line| useful(line) && looks_like_error(line))
        .or_else(|| lines.iter().rposition(useful))
}

/// Whether a stderr line is a warning rather than the failure.
fn is_warning(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.starts_with("warning")
        || lower.starts_with("in addition: warning")
        || lower.starts_with("note:")
        // R numbers the warnings it collected: `1: In poly2nb(...) :`.
        || line
            .split_once(": In ")
            .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// Whether a line is the runtime saying it stopped, which no reader needs.
fn is_terminator(line: &str) -> bool {
    matches!(line, "Execution halted" | "Aborted" | "Killed")
}

/// Whether a line names a failure rather than describing the run.
fn looks_like_error(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "error",
        "exception",
        "traceback",
        "fatal",
        "denied",
        "not found",
        "no such",
        "failed",
        "cannot",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// `text` cut to `max` characters, with an ellipsis when it had to be.
pub fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max).collect();
    format!("{}...", kept.trim_end())
}

/// A field with nothing in it, as a cell.
fn or_empty(text: &str) -> String {
    if text.trim().is_empty() {
        EMPTY.to_string()
    } else {
        text.trim().to_string()
    }
}

/// `1 job`, `2 jobs`.
fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

#[cfg(test)]
mod tests;
