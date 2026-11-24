//! The human rendering of every `chap-devops dhis2` report.

use super::*;
use crate::commands::components::{Note, NoteLevel};
use crate::output::Report;

/// The line every verb opens with: where DHIS2 is, what it is, and who it was
/// asked as. Never the password; the hint says only where it was found.
pub(super) fn instance_line(instance: &Instance) -> String {
    let what = match instance.external {
        true => "external DHIS2",
        false => "DHIS2",
    };
    format!(
        "{what} {} at {}, as `{}`",
        display_version(&instance.version),
        instance.url,
        instance.user
    )
}

/// Where the credential that DHIS2 was asked with came from.
pub(super) fn credential_hint(instance: &Instance) -> String {
    format!(
        "the credential is {}",
        dhis2::describe_credential(instance.auth, instance.credential_from)
    )
}

/// The table `use` prints for the DHIS2 it recorded: what answered and with
/// which login. Empty when there is nothing recorded to ask.
pub(super) fn use_table(report: &UseReport, out: &Out) -> String {
    let (Some(external), Some(probe)) = (&report.external, &report.probe) else {
        return String::new();
    };
    let dhis2 = match probe.answered {
        true => format!(
            "{} {}",
            out.value(&external.url),
            out.ok(&format!("({})", probe.answer))
        ),
        false => format!(
            "{} {}",
            out.value(&external.url),
            out.warn(&format!("({})", probe.answer))
        ),
    };
    let login = match (&probe.credential, probe.accepted) {
        (None, _) => out.warn("none"),
        (Some(credential), Some(true)) => format!("{credential} {}", out.ok("(accepted)")),
        (Some(credential), Some(false)) => format!("{credential} {}", out.warn("(refused)")),
        (Some(credential), None) => format!("{credential} {}", out.dim("(not tried)")),
    };
    let connected = match &external.connected_at {
        Some(at) => out.value(at),
        None => out.dim("never recorded"),
    };
    crate::output::fields_with(
        0,
        &[
            ("dhis2", dhis2),
            ("chap-url", out.value(&external.chap_url)),
            (
                "route",
                out.dim(report.target.as_deref().unwrap_or_default()),
            ),
            ("login", login),
            ("connected", connected),
        ],
        &|label| out.key(label),
    )
}

/// The lines of `use`, after its table.
pub(super) fn use_summary(report: &UseReport, lines: &mut Report) {
    let headline = match (report.outcome, &report.external, &report.previous) {
        (UseOutcome::Cleared, _, Some(previous)) => {
            format!("forgot the external DHIS2 at {}", previous.url)
        }
        (UseOutcome::Unchanged, None, _) => "no external DHIS2 is recorded".to_string(),
        (UseOutcome::Recorded, Some(external), _) => format!(
            "recorded the external DHIS2 at {} in `.chap-devops/components.yaml`",
            external.url
        ),
        (UseOutcome::Changed, Some(external), previous) => {
            let url_moved = previous
                .as_ref()
                .is_none_or(|previous| previous.url != external.url);
            match url_moved {
                true => format!(
                    "changed the external DHIS2 to {} in `.chap-devops/components.yaml`",
                    external.url
                ),
                false => format!(
                    "changed the chap-url of the external DHIS2 at {} to {} in \
                     `.chap-devops/components.yaml`",
                    external.url, external.chap_url
                ),
            }
        }
        (UseOutcome::Unchanged, Some(external), _) => {
            format!("the external DHIS2 at {} is already recorded", external.url)
        }
        (_, Some(external), _) => {
            format!(
                "`chap-devops dhis2` talks to the external DHIS2 at {}",
                external.url
            )
        }
        (_, None, _) => match report.component {
            true => {
                "no external DHIS2 is recorded; `chap-devops dhis2` talks to the `dhis2` component"
                    .to_string()
            }
            false => "no external DHIS2 is recorded, and the `dhis2` component is off".to_string(),
        },
    };
    lines.info(headline);
    if let Some(problem) = report
        .probe
        .as_ref()
        .and_then(|probe| probe.problem.as_ref())
    {
        lines.warning(problem.as_str());
    }
    note_lines(&report.notes, lines);
    match report.next_is_hint {
        true => lines.hint(report.next.as_str()),
        false => lines.info(report.next.as_str()),
    };
}

/// Every note at its own level.
pub(super) fn note_lines(notes: &[Note], lines: &mut Report) {
    for note in notes {
        match note.level {
            NoteLevel::Info => lines.info(note.text.as_str()),
            NoteLevel::Hint => lines.hint(note.text.as_str()),
            NoteLevel::Warning => lines.warning(note.text.as_str()),
        };
    }
}

/// A version to print, or the words for an instance that did not say.
pub(super) fn display_version(version: &str) -> String {
    match version.trim().is_empty() {
        true => "(version unknown)".to_string(),
        false => version.trim().to_string(),
    }
}

/// The lines of `route`, `analytics`, `apps` and `connect`.
///
/// `connect` is the one with a record, and its `next` is the verdict of the
/// whole run, so it is info; the next step of one verb is a hint.
pub(super) fn report_summary(report: &Dhis2Report, lines: &mut Report) {
    lines
        .info(instance_line(&report.instance))
        .hint(credential_hint(&report.instance));
    if let Some(route) = &report.route {
        route_lines(route, lines);
    }
    if let Some(apps) = &report.apps {
        app_lines(apps, lines);
    }
    if let Some(analytics) = &report.analytics {
        analytics_lines(analytics, lines);
    }
    for skip in &report.skipped {
        lines.info(format!("skipped: {skip}"));
    }
    if let Some(record) = report.record {
        record_lines(record, lines);
    }
    crate::commands::configs::sync::fold(&report.configured, true, lines);
    match report.record.is_some() {
        true => lines.info(report.next.as_str()),
        false => lines.hint(report.next.as_str()),
    };
}

/// What `connect` says about the note it left behind.
///
/// The caveat is said where the record is, because this is the one moment a
/// reader could take it for a verdict. It is a note that this command ran;
/// `chap-devops dhis2 show` is what asks DHIS2.
pub(super) fn record_lines(record: ConnectRecord, lines: &mut Report) {
    match record {
        ConnectRecord::Unchanged => {}
        ConnectRecord::Recorded => {
            lines
                .hint(
                    "recorded the connect in `.chap-devops/components.yaml`, so `chap-devops up` and \
                     `chap-devops status` stop asking for it",
                )
                .hint(
                    "the record says that this ran, not that the route is still right; \
                     `chap-devops dhis2 show` asks DHIS2",
                );
        }
        ConnectRecord::Cleared => {
            lines.hint(
                "cleared the earlier `chap-devops dhis2 connect` from `.chap-devops/components.yaml`; \
                 `chap-devops up` and `chap-devops status` ask for it again",
            );
        }
    }
}

pub(super) fn route_lines(route: &RouteReport, lines: &mut Report) {
    let code = route.code;
    let headline = match route.outcome {
        RouteOutcome::Created => format!("created the `{code}` route at {}", route.url),
        RouteOutcome::Repointed => format!("repointed the `{code}` route at {}", route.url),
        RouteOutcome::Unchanged => format!("the `{code}` route already points at {}", route.url),
    };
    match route.verified {
        true => lines.info(format!("{headline}; chap-core answered through it")),
        false => lines.info(headline),
    };
    for reason in &route.reasons {
        lines.hint(format!("the route was rewritten because {reason}"));
    }
    // The route is correct whatever chap-core did, so a chap-core that is not
    // answering is a warning rather than an error: it is `chap-devops up`'s
    // problem, and `verified: false` says it to a script.
    match (route.verified, route.token_refused) {
        (true, _) => lines.hint(format!("chap-core said: {}", route.answered)),
        (false, true) => lines.warning(format!(
            "the `{code}` route carries this deployment's API token and chap-core refused it: \
             {}; `chap-devops auth show` says which token chap-devops has, and chap-core has to be \
             running with the same one",
            route.answered
        )),
        (false, false) => lines.warning(format!(
            "the `{code}` route is in place but nothing answered through it: {}; {}",
            route.answered, route.way_out
        )),
    };
}

pub(super) fn app_lines(apps: &AppsReport, lines: &mut Report) {
    for app in &apps.apps {
        let name = &app.name;
        match app.outcome {
            AppOutcome::Installed => lines.info(format!("installed {name} {}", app.version)),
            AppOutcome::Moved => lines.info(format!(
                "moved {name} from {} to {}",
                app.previous.clone().unwrap_or_default(),
                app.version
            )),
            AppOutcome::Unchanged => {
                lines.info(format!("{name} {} is already installed", app.version))
            }
            AppOutcome::Failed => lines.warning(format!(
                "{name} was not installed: {}",
                app.reason.clone().unwrap_or_default()
            )),
        };
    }
}

pub(super) fn analytics_lines(analytics: &AnalyticsReport, lines: &mut Report) {
    let job = format!("(job {})", analytics.job);
    if !analytics.finished {
        match analytics.started {
            true => lines.info(format!("started the analytics run {job}")),
            false => lines.info(format!("an analytics run was already going {job}")),
        };
        return;
    }
    lines.info(format!(
        "{} in {} {job}",
        match analytics.started {
            true => "analytics finished",
            // The distinction matters: this run watched somebody else's job,
            // so the duration is how long it watched, not how long it took.
            false => "the analytics run that was already going finished",
        },
        crate::output::human_age(Duration::from_secs(analytics.seconds)),
    ));
    if !analytics.message.is_empty() {
        lines.hint(format!("DHIS2 said: {}", analytics.message));
    }
    if !analytics.last_success.is_empty() {
        lines.hint(format!(
            "DHIS2 records its last analytics success as {}",
            dhis2::shown_success(&analytics.last_success)
        ));
    }
}

/// What `show` prints first: the instance, then its rows.
pub(super) fn show_table(report: &ShowReport, out: &Out) -> String {
    let mut text = format!("{}\n", instance_line(&report.instance));
    let route = match &report.route {
        None => out.warn("none"),
        Some(route) => {
            let mut cell = out.value(&route.url);
            if !route.ours && !report.target.is_empty() {
                cell = format!("{cell} {}", out.warn("(another chap-core)"));
            }
            if route.disabled {
                cell = format!("{cell} {}", out.warn("(disabled)"));
            }
            if !route.authorised {
                cell = format!(
                    "{cell} {}",
                    out.warn(&format!("(no {} authority)", dhis2::ROUTE_AUTHORITY))
                );
            }
            if route.verified {
                cell = format!("{cell} {}", out.ok(&format!("({})", route.answered)));
            } else if route.ours {
                cell = format!("{cell} {}", out.warn("(nothing answered through it)"));
            }
            cell
        }
    };
    let apps = match report.apps.is_empty() {
        true => out.warn("none"),
        false => report
            .apps
            .iter()
            .map(|app| app_cell(app, out))
            .collect::<Vec<String>>()
            .join(", "),
    };
    text.push_str(&crate::output::fields_with(
        0,
        &[
            ("route", route),
            (
                "target",
                match report.target.is_empty() {
                    true => out.dim("none (this deployment has no chap-core)"),
                    false => out.dim(&report.target),
                },
            ),
            (
                "analytics",
                analytics_cell(report.analytics, &report.last_analytics, out),
            ),
            ("apps", apps),
        ],
        &|label| out.key(label),
    ));
    text
}

/// The lines of `show`, after its rows: whatever is missing, then what to do.
pub(super) fn show_summary(report: &ShowReport, lines: &mut Report) {
    lines.hint(credential_hint(&report.instance));
    for missing in &report.missing {
        lines.info(format!("missing: {missing}"));
    }
    lines.info(report.next.as_str());
}

/// One of the two apps in the `apps` row: the version, or that it is absent.
fn app_cell(app: &ShownApp, out: &Out) -> String {
    match (app.installed, &app.build) {
        (true, Some(build)) => format!("{} {} ({build})", app.name, out.value(&app.version)),
        (true, None) => format!("{} {}", app.name, out.value(&app.version)),
        (false, _) => format!("{} {}", app.name, out.warn("not installed")),
    }
}

/// The `analytics` row: what DHIS2 records, and what chap-devops knows it is worth.
///
/// A timestamp on its own reads as "analytics is done", and on a seeded
/// deployment it is not that at all - it is the dump's. So the row says which
/// of the two it is, and never claims more than chap-devops checked: a run that
/// finished is a run that finished, which is not the same as tables with rows
/// in them. Nothing here can see a row count; `lastYears` is the reason that
/// distinction is worth keeping, and `docs/dhis2.md` carries it.
pub(super) fn analytics_cell(evidence: dhis2::AnalyticsEvidence, last: &str, out: &Out) -> String {
    let when = out.value(&dhis2::shown_success(last));
    match evidence {
        dhis2::AnalyticsEvidence::Never => out.warn("never run"),
        dhis2::AnalyticsEvidence::Recorded => when,
        dhis2::AnalyticsEvidence::RanHere if last.trim().is_empty() => {
            out.ok("a run finished on this deployment")
        }
        dhis2::AnalyticsEvidence::RanHere => {
            format!("{when} {}", out.ok("(a run finished on this deployment)"))
        }
        dhis2::AnalyticsEvidence::Unconfirmed => {
            format!("{when} {}", out.warn("(unconfirmed on a seeded database)"))
        }
    }
}

/// The first line of an error, for a cell that has one line to say it in.
pub(super) fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_string()
}
