//! Checks of the hosts Chap needs: ghcr.io, the marketplace, GitHub and chap-devops's own releases.

use super::*;

/// The five lookups that need the network, still running.
pub(super) struct Probes<'s> {
    ghcr: std::thread::ScopedJoinHandle<'s, std::result::Result<u16, String>>,
    marketplace: std::thread::ScopedJoinHandle<'s, std::result::Result<u16, String>>,
    chap_core: std::thread::ScopedJoinHandle<'s, std::result::Result<String, String>>,
    chap_devops: std::thread::ScopedJoinHandle<'s, std::result::Result<String, String>>,
    github: std::thread::ScopedJoinHandle<'s, std::result::Result<github::Quota, String>>,
}

/// The same five, answered.
///
/// Two of them do double duty: the release lookups are what tells the
/// `net-releases` and `chap-devops` lines that the host answered, and the tags they
/// came back with are what the pin checks compare against.
pub struct Probed {
    pub ghcr: std::result::Result<u16, String>,
    pub marketplace: std::result::Result<u16, String>,
    pub chap_core: std::result::Result<String, String>,
    pub chap_devops: std::result::Result<String, String>,
    /// What is left of this address's hour on the GitHub API, which is what
    /// the release, pin and `models add` lookups are all spending.
    pub github: std::result::Result<github::Quota, String>,
}

impl<'s> Probes<'s> {
    /// Start all five at once. Five sequential three-second timeouts would be
    /// fifteen seconds of a command that has to stay interactive.
    pub(super) fn spawn(scope: &'s std::thread::Scope<'s, '_>, registry_url: &str) -> Probes<'s> {
        let registry_url = registry_url.to_string();
        Probes {
            ghcr: scope.spawn(|| probe(GHCR_PROBE_URL, NET_TIMEOUT)),
            marketplace: scope.spawn(move || probe(&registry_url, NET_TIMEOUT)),
            chap_core: scope
                .spawn(|| chapcore::latest_release(NET_TIMEOUT).map_err(|e| format!("{e:#}"))),
            chap_devops: scope.spawn(|| {
                selfupdate::latest_release(NET_TIMEOUT)
                    .map(|release| release.tag)
                    .map_err(|e| format!("{e:#}"))
            }),
            // `/rate_limit` is documented as not counting against the limit
            // it reports, so asking cannot be what uses up the last request.
            github: scope.spawn(|| github::quota(NET_TIMEOUT).map_err(|e| format!("{e:#}"))),
        }
    }

    /// Wait for all five. A panicked probe reads as an unreachable host,
    /// which is the verdict it would have produced anyway.
    pub(super) fn join(self) -> Probed {
        fn joined<T>(
            handle: std::thread::ScopedJoinHandle<'_, std::result::Result<T, String>>,
        ) -> std::result::Result<T, String> {
            handle
                .join()
                .unwrap_or_else(|_| Err("the probe did not finish".to_string()))
        }
        Probed {
            ghcr: joined(self.ghcr),
            marketplace: joined(self.marketplace),
            chap_core: joined(self.chap_core),
            chap_devops: joined(self.chap_devops),
            github: joined(self.github),
        }
    }
}

/// What one release lookup came back with, or why it came back with nothing.
///
/// The two empty cases are kept apart because they read differently on the
/// checklist: `--offline` is a flag the user set, and a check it held back
/// says so whatever else is installed on the machine; a lookup that was made
/// and did not arrive is a fact about the network instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseList<'a> {
    /// The newest tag the host named.
    Newest(&'a str),
    /// `--offline`: the list was never asked for.
    Offline,
    /// It was asked for and did not arrive.
    Unreachable,
}

impl<'a> ReleaseList<'a> {
    /// The reason every `--offline` line gives, worded the same way in all of
    /// them so a reader can tell the flag from a broken network at a glance.
    pub const OFFLINE_WHY: &'static str = "--offline: the release list was not asked";

    /// One joined probe, where `None` is `--offline`: that is the only reason
    /// a probe was never started.
    pub fn of(probe: Option<&'a std::result::Result<String, String>>) -> ReleaseList<'a> {
        match probe {
            None => ReleaseList::Offline,
            Some(Ok(tag)) => ReleaseList::Newest(tag),
            Some(Err(_)) => ReleaseList::Unreachable,
        }
    }
}

/// Ask one host whether it is there, within `timeout`.
///
/// A GET whose body is never read, which costs the same as a HEAD and works
/// on the endpoints that answer HEAD with 405. Any HTTP status counts as an
/// answer: ghcr.io replies 401 to an anonymous `/v2/`, and a host that
/// refuses us is still a host this machine can reach.
pub fn probe(url: &str, timeout: Duration) -> std::result::Result<u16, String> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .user_agent(USER_AGENT)
            .build(),
    );
    let started = Instant::now();
    let answer = match agent.get(url).call() {
        Ok(response) => Ok(response.status().as_u16()),
        Err(ureq::Error::StatusCode(code)) => Ok(code),
        Err(err) => Err(err.to_string()),
    };
    crate::output::verbose(&format!(
        "GET {url} -> {} in {}ms",
        match &answer {
            Ok(code) => code.to_string(),
            Err(err) => err.clone(),
        },
        started.elapsed().as_millis()
    ));
    answer
}

/// One line per host Chap needs, or three skipped lines under `--offline`.
pub fn network_checks(probed: Option<&Probed>) -> Vec<Check> {
    let hosts: [(&str, &str, &str, &str); 3] = [
        (
            "net-ghcr",
            "network ghcr.io",
            "the image registry",
            "without ghcr.io no image can be pulled; `--offline` keeps chap-devops itself working \
             from the embedded catalogue",
        ),
        (
            "net-marketplace",
            "network marketplace",
            "the model catalogue",
            "the catalogue falls back to the cache and then to the snapshot built into this \
             binary; `chap-devops registry list` says which one is in use",
        ),
        (
            "net-releases",
            "network releases",
            "the chap-core release list",
            "`chap-devops update` needs it; everything else works without it",
        ),
    ];
    let Some(probed) = probed else {
        return hosts
            .iter()
            .map(|(id, name, what, _)| {
                Check::skip(*id, *name, format!("--offline: {what} was not probed"))
            })
            .collect();
    };
    let answers: [&dyn ReachSummary; 3] = [&probed.ghcr, &probed.marketplace, &probed.chap_core];
    hosts
        .iter()
        .zip(answers)
        .map(|((id, name, what, fix), answer)| match answer.reached() {
            Ok(detail) => Check::ok(*id, *name, detail),
            Err(why) => Check::warn(*id, *name, format!("{what} is unreachable: {why}"), *fix),
        })
        .collect()
}

/// The stable id and the printed name of the `github api` line.
pub const GITHUB_ID: &str = "net-github";

pub const GITHUB_NAME: &str = "github api";

/// What the `github api` line says under `--offline`.
pub const GITHUB_OFFLINE: &str = "--offline: the GitHub API rate limit was not asked";

/// What every unreachable GitHub line offers, and what a quota that has run
/// out offers when there is no token to blame.
const GITHUB_FIX: &str = "`chap-devops update`, the pin checks and `chap-devops models add` need it; \
     everything else works without it";

/// The `github api` line: does the REST API answer, and how much of this
/// address's hour is left on it.
///
/// Not a failure whatever it says. Every GitHub lookup this CLI makes already
/// degrades to a skip with the reason on its own line; this one is here so an
/// operator can see the quota before it runs out rather than after, and so a
/// morning of skipped pin checks has an explanation at the top of the report.
pub fn github_check(quota: Option<&std::result::Result<github::Quota, String>>) -> Check {
    let Some(quota) = quota else {
        return Check::skip(GITHUB_ID, GITHUB_NAME, GITHUB_OFFLINE);
    };
    let quota = match quota {
        Ok(quota) => quota,
        Err(why) => {
            return Check::warn(
                GITHUB_ID,
                GITHUB_NAME,
                format!("could not ask what is left of this hour: {why}"),
                GITHUB_FIX,
            );
        }
    };
    // The parenthetical says which of the two limits this is, and where there
    // is no token it says what setting one would buy.
    let credential = match quota.token {
        true => "token".to_string(),
        false => format!("no token; set GITHUB_TOKEN for {}", github::TOKEN_LIMIT),
    };
    let left = format!(
        "reachable, {} of {} requests left this hour ({credential})",
        quota.remaining, quota.limit
    );
    if quota.remaining > 0 {
        return Check::ok(GITHUB_ID, GITHUB_NAME, left);
    }
    let until = github::reset_clock(quota.reset);
    Check::warn(
        GITHUB_ID,
        GITHUB_NAME,
        format!("{left}, until {until}"),
        match quota.token {
            true => format!("wait until {until}; until then the release and pin checks skip"),
            false => format!(
                "set GITHUB_TOKEN or GH_TOKEN for {} requests an hour, or wait until {until}; \
                 until then the release and pin checks skip",
                github::TOKEN_LIMIT
            ),
        },
    )
}

/// How a probe result reads on the line for its host.
///
/// Two of the probes fetch a document rather than only touching the host, so
/// each one describes its own success in its own terms.
pub trait ReachSummary {
    fn reached(&self) -> std::result::Result<String, String>;
}

impl ReachSummary for std::result::Result<u16, String> {
    fn reached(&self) -> std::result::Result<String, String> {
        match self {
            Ok(code) => Ok(format!("reachable (HTTP {code})")),
            Err(why) => Err(why.clone()),
        }
    }
}

impl ReachSummary for std::result::Result<String, String> {
    fn reached(&self) -> std::result::Result<String, String> {
        match self {
            Ok(tag) => Ok(format!("reachable, newest chap-core is {tag}")),
            Err(why) => Err(why.clone()),
        }
    }
}

#[cfg(test)]
mod tests;
