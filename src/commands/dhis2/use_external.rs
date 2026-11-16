//! `chap-devops dhis2 use`: a DHIS2 that runs elsewhere, recorded and asked.

use super::*;
use crate::commands::components::Note;

/// How long `use` gives DHIS2 to answer: it asks once, and a DHIS2 that is
/// still migrating is `chap-devops dhis2 show`'s to wait for.
const USE_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Why an external DHIS2 cannot be recorded beside the component.
const COMPONENT_IS_ON: &str = "the `dhis2` component is on, and it is this deployment's DHIS2; \
     run `chap-devops components disable dhis2` first, then `chap-devops dhis2 use` again";

/// What `use` did to the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UseOutcome {
    /// There was no external DHIS2, and now there is one.
    Recorded,
    /// There was one, and its URL or its chap-core URL moved.
    Changed,
    /// What was asked for is what was recorded. Nothing was written.
    Unchanged,
    /// The record was removed.
    Cleared,
    /// Nothing was asked for, so the record was reported and not touched.
    Shown,
}

/// What `use` found when it asked the DHIS2 it recorded.
#[derive(Debug, Clone, Serialize)]
pub struct UseProbe {
    /// Whether [`dhis2::PING_PATH`] answered.
    pub answered: bool,
    /// What it said, or why nothing did.
    pub answer: String,
    /// The credential `chap-devops dhis2` would send, by what it is and where it was
    /// found; `null` when there is none.
    pub credential: Option<String>,
    /// Whether DHIS2 accepted it. `null` when it was not tried: no
    /// credential, or no DHIS2 answering to try it on.
    pub accepted: Option<bool>,
    /// Why there is no credential, or why DHIS2 did not accept it.
    pub problem: Option<String>,
}

/// What `chap-devops dhis2 use` did.
#[derive(Debug, Clone, Serialize)]
pub struct UseReport {
    pub outcome: UseOutcome,
    /// The external DHIS2 recorded now, or `null` for none.
    pub external: Option<crate::components::ExternalDhis2>,
    /// The one recorded before, for a change or a clear.
    pub previous: Option<crate::components::ExternalDhis2>,
    /// Where the `chap` route has to point for it.
    pub target: Option<String>,
    /// Whether the `dhis2` component is on, which is what `chap-devops dhis2` talks
    /// to when nothing is recorded.
    pub component: bool,
    /// What asking it just now found; `null` when there is nothing to ask.
    pub probe: Option<UseProbe>,
    pub notes: Vec<Note>,
    pub next: String,
    /// Whether `next` is an optional command rather than a step the reader
    /// has to do. Only for the human lines.
    #[serde(skip)]
    pub next_is_hint: bool,
}

/// `chap-devops dhis2 use` — record a DHIS2 that runs elsewhere, change it, forget
/// it, or say which DHIS2 `chap-devops dhis2` talks to.
///
/// Every shape that records something also asks: [`dhis2::PING_PATH`] to
/// prove the URL, and one authenticated request to prove the credential. A
/// record that is wrong is still written - the URL can be right for a DHIS2
/// that is down for maintenance - and the report says what did not answer.
pub fn use_external(ctx: &Ctx, args: &Dhis2UseArgs) -> Result<()> {
    let (mut project, _lock) = ctx.project_mut()?;
    let before = project.state.components.dhis2_external.clone();
    let component = project.state.components.dhis2.enabled;
    let mut listed_login = false;

    if args.clear {
        let outcome = match before {
            Some(_) => {
                project.state.components.dhis2_external = None;
                project.save()?;
                UseOutcome::Cleared
            }
            None => UseOutcome::Unchanged,
        };
        let report = UseReport {
            outcome,
            external: None,
            previous: before,
            target: None,
            component,
            probe: None,
            notes: Vec::new(),
            next_is_hint: component,
            next: match component {
                true => "`chap-devops dhis2` talks to the `dhis2` component; run `chap-devops dhis2 show`",
                false => {
                    "run `chap-devops dhis2 use URL --chap-url URL` to record a DHIS2, or `chap-devops \
                     components enable dhis2` to deploy one"
                }
            }
            .to_string(),
        };
        return ctx.out.report(&report, |lines| use_summary(&report, lines));
    }

    let (external, outcome) = match (&args.url, &args.chap_url) {
        (None, None) => (before.clone(), UseOutcome::Shown),
        (url, chap_url) => {
            if component {
                return Err(anyhow::anyhow!(COMPONENT_IS_ON));
            }
            let url = match (url, &before) {
                (Some(url), _) => external_url(url)?,
                (None, Some(before)) => before.url.clone(),
                (None, None) => {
                    return Err(anyhow::anyhow!(
                        "no external DHIS2 is recorded yet, so there is no URL to keep; run \
                         `chap-devops dhis2 use URL --chap-url URL`"
                    ));
                }
            };
            let chap_url = match (chap_url, &before) {
                (Some(chap_url), _) => external_url(chap_url)?,
                (None, Some(before)) => before.chap_url.clone(),
                (None, None) => {
                    return Err(anyhow::anyhow!(
                        "`--chap-url URL` is needed the first time: the route points at \
                         chap-core as DHIS2 reaches it, which chap-devops cannot know; run `chap-devops \
                         dhis2 use {url} --chap-url https://chap.example.org`"
                    ));
                }
            };
            let same = before
                .as_ref()
                .is_some_and(|before| before.url == url && before.chap_url == chap_url);
            let external = crate::components::ExternalDhis2 {
                url,
                chap_url,
                // A connect is a fact about one DHIS2 and one route target;
                // either moving is a DHIS2 no connect has been recorded for.
                connected_at: match same {
                    true => before.as_ref().and_then(|b| b.connected_at.clone()),
                    false => None,
                },
            };
            let outcome = match (&before, same) {
                (None, _) => UseOutcome::Recorded,
                (Some(_), true) => UseOutcome::Unchanged,
                (Some(_), false) => UseOutcome::Changed,
            };
            if outcome != UseOutcome::Unchanged {
                ctx.out
                    .verbose("recording `dhis2-external` in `.chap-devops/components.yaml`");
                project.state.components.dhis2_external = Some(external.clone());
                project.save()?;
                // The login variables, commented out, so the names the
                // credentials message asks for are already in the file.
                listed_login = crate::compose::sync::append_env_pins(&project, false)?.is_some();
            }
            (Some(external), outcome)
        }
    };

    let Some(recorded) = &external else {
        let report = UseReport {
            outcome,
            external: None,
            previous: None,
            target: None,
            component,
            probe: None,
            notes: Vec::new(),
            next_is_hint: component,
            next: match component {
                true => "`chap-devops dhis2` talks to the `dhis2` component; run `chap-devops dhis2 show`",
                false => {
                    "run `chap-devops dhis2 use URL --chap-url URL` to record a DHIS2, or `chap-devops \
                     components enable dhis2` to deploy one"
                }
            }
            .to_string(),
        };
        return ctx.out.report(&report, |lines| use_summary(&report, lines));
    };

    let probe = probe_external(&project, recorded);
    let mut notes = Vec::new();
    if listed_login {
        notes.push(Note::hint(
            "listed the DHIS2 login variables in `.env`, commented out, for the credentials",
        ));
    }
    notes.extend(loopback_note(&recorded.url, &recorded.chap_url).map(Note::warning));
    notes.extend(cleartext_note(&recorded.url).map(Note::warning));
    if !project.state.components.has_chap_core_api() {
        notes.push(Note::warning(NO_CHAP_CORE));
    }
    let next = match (&probe, &recorded.connected_at) {
        (probe, _) if !probe.answered => {
            "check the URL, then run `chap-devops dhis2 use` again to ask it".to_string()
        }
        // The problem line above has just named the ways to give it
        // credentials; this one only says what follows.
        (probe, _) if probe.credential.is_none() => {
            "once one of them is in `.env`, run `chap-devops dhis2 connect`".to_string()
        }
        (probe, _) if probe.accepted == Some(false) => {
            "fix the credential named above, then run `chap-devops dhis2 use` again to ask it"
                .to_string()
        }
        (_, None) => "run `chap-devops dhis2 connect` to point its route at this Chap".to_string(),
        (_, Some(_)) => "run `chap-devops dhis2 show` to see what it has".to_string(),
    };
    // A recorded connect leaves nothing the reader has to do.
    let next_is_hint = probe.answered
        && probe.credential.is_some()
        && probe.accepted != Some(false)
        && recorded.connected_at.is_some();
    let report = UseReport {
        outcome,
        target: Some(dhis2::external_route_target(&recorded.chap_url)),
        external: external.clone(),
        previous: before.filter(|_| outcome == UseOutcome::Changed),
        component,
        probe: Some(probe),
        notes,
        next,
        next_is_hint,
    };
    if !ctx.out.json {
        print!("{}", use_table(&report, &ctx.out));
    }
    ctx.out.report(&report, |lines| use_summary(&report, lines))
}

/// A note for a DHIS2 reached over plain `http://` on another machine: every
/// request carries the password or the token, and over `http://` they cross
/// the network as they are. `None` for `https://`, and for this machine or a
/// private address, where `http://` is the usual setup.
pub(super) fn cleartext_note(url: &str) -> Option<String> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split('/').next().unwrap_or_default();
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => authority,
    }
    .trim_matches(['[', ']']);
    let local = match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Err(_) => host == "localhost" || host.ends_with(".local") || !host.contains('.'),
    };
    (!local).then(|| {
        format!(
            "{url} is plain http on another machine, so the DHIS2 password or token crosses \
             the network unencrypted with every request; record its `https://` address with \
             `chap-devops dhis2 use` if it has one"
        )
    })
}

/// A URL as it is recorded: `http://` or `https://` with a host, and without a
/// trailing slash.
pub(super) fn external_url(raw: &str) -> Result<String> {
    let url = raw.trim().trim_end_matches('/');
    let host = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .map(|rest| rest.split('/').next().unwrap_or_default())
        .unwrap_or_default();
    if host.is_empty() {
        return Err(anyhow::anyhow!(
            "`{raw}` is not a URL chap-devops can record; give it with http:// or https://, like \
             `https://dhis2.example.org`"
        ));
    }
    // The URL is stored in `.chap-devops/` and shown in traces, so it must
    // not carry a password. The `@` is not quoted back: what is before it
    // may be one.
    if host.contains('@') {
        return Err(anyhow::anyhow!(
            "the URL has a user name or a password before an `@`, which chap-devops would store \
             in `.chap-devops/`; give the URL without it, and for the DHIS2 login {}",
            crate::dhis2::CREDENTIALS_WAY_OUT
        ));
    }
    Ok(url.to_string())
}

/// What a `--chap-url` on this machine's own address means for the DHIS2 it
/// is recorded for.
///
/// A DHIS2 on another machine resolves `localhost` to itself, so the address
/// is wrong there. A DHIS2 on this machine is either a process on it, for
/// which `localhost` is exactly right, or a container, which needs
/// `host.docker.internal`; chap-devops cannot tell which, so the note says both.
pub(super) fn loopback_note(dhis2_url: &str, chap_url: &str) -> Option<String> {
    if !is_loopback(chap_url) {
        return None;
    }
    Some(match is_loopback(dhis2_url) {
        true => format!(
            "{chap_url} is right for a DHIS2 running directly on this machine; one running in \
             Docker reaches chap-core at `{}` instead",
            chap_url
                .replacen("localhost", "host.docker.internal", 1)
                .replacen("127.0.0.1", "host.docker.internal", 1)
        ),
        false => format!(
            "{chap_url} is this machine's own address, and DHIS2 resolves it to its own server; \
             give `--chap-url` the address chap-core is served at on the network"
        ),
    })
}

/// Whether a URL names this machine, which is a different machine for DHIS2.
fn is_loopback(url: &str) -> bool {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or_default()
        .split(['/', ':'])
        .next()
        .unwrap_or_default();
    host == "localhost" || host.starts_with("127.") || host == "[::1]"
}

/// Ask the recorded DHIS2 whether it answers, and whether it takes the
/// credential `chap-devops dhis2` would send it.
fn probe_external(project: &Project, external: &crate::components::ExternalDhis2) -> UseProbe {
    let credentials = dhis2::credentials_for(&project.dir, None, false, false);
    // `send_open` carries no credential, so a placeholder serves for the ping
    // when there is none to carry.
    let client = Dhis2::new(
        &external.url,
        credentials
            .as_ref()
            .ok()
            .cloned()
            .unwrap_or_else(|| dhis2::Credentials::token("", CredentialSource::Default)),
        USE_PROBE_TIMEOUT,
    )
    .external();
    let (answered, answer) = match client.send_open(dhis2::PING_PATH) {
        Ok(answer) if answer.is_success() => (true, format!("answers {}", dhis2::PING_PATH)),
        Ok(answer) => (
            false,
            format!("{} answered {}", dhis2::PING_PATH, answer.status_line()),
        ),
        Err(err) => (false, first_line(&err.to_string())),
    };
    let (credential, accepted, problem) = match credentials {
        Err(why) => (None, None, Some(why.to_string())),
        Ok(credentials) if !answered => (Some(credentials.describe()), None, None),
        Ok(credentials) => {
            let described = credentials.describe();
            match client.send("GET", dhis2::ME_PATH, None) {
                Ok(answer) if answer.is_success() => (Some(described), Some(true), None),
                Ok(answer) => (
                    Some(described),
                    Some(false),
                    Some(client.refusal(&answer).unwrap_or_else(|| {
                        client.status_error(dhis2::ME_PATH, &answer).to_string()
                    })),
                ),
                Err(err) => (Some(described), None, Some(first_line(&err.to_string()))),
            }
        }
    };
    UseProbe {
        answered,
        answer,
        credential,
        accepted,
        problem,
    }
}
