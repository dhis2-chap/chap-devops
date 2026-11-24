//! `chap-devops dhis2 connect`: the route, the apps, then analytics, and the record of it.

use super::*;

/// `chap-devops dhis2 connect` — the route, the apps, then analytics.
///
/// That order, and not the order the chapter lists them in. The two cheap steps
/// come first so a failure in either is reported in seconds rather than after
/// the analytics wait, and analytics is last because it is the long one and
/// nothing else waits on it: with the route and the apps already in place, the
/// deployment is usable the moment the tables land.
pub fn connect(ctx: &Ctx, args: &Dhis2ConnectArgs) -> Result<()> {
    let mut session = open_session(ctx, &args.common)?;
    let mut skipped = Vec::new();

    let route = write_route(ctx, &session)?;
    // The Modeling App lists chap-core's configured models, and chap-core 2.4
    // and later makes none from a registration; the step gives each model
    // chap-devops runs its own before the report says the app can use Chap.
    let configured = crate::commands::configs::sync::auto(ctx, &session.project);

    // A DHIS2 someone else runs gets the route and nothing more. Installing
    // apps and generating analytics tables change a server chap-devops does not own,
    // and on a national instance an analytics run takes hours of its CPU; its
    // admin decides, with `chap-devops dhis2 apps` and `chap-devops dhis2 analytics`.
    if session.external().is_some() {
        let mut left = Vec::new();
        // Only a listing that names both apps lets the report say the Modeling
        // App can reach Chap now rather than once it is installed.
        let listed = match chap_apps(&session) {
            Ok(apps) => {
                left.extend(missing_labels(&apps));
                true
            }
            Err(err) => {
                ctx.out.verbose(&format!("could not list the apps: {err}"));
                false
            }
        };
        let notes = external_left_alone(&left);
        // On a DHIS2 someone else runs, the route is all that `connect` does,
        // so a proved route is a connect that got through: the record stops
        // `chap-devops up` and `chap-devops status` asking for a command that cannot do
        // more. The apps are its admin's step, and `chap-devops dhis2 show` names
        // them for as long as they are missing.
        let judgement = match route.verified {
            true => Judgement::Connected,
            false => Judgement::Broken,
        };
        let record = record_connect(ctx, &mut session.project, judgement)?;
        let report = Dhis2Report {
            instance: session.instance(),
            next: external_next(route.verified, listed.then_some(left.as_slice())),
            route: Some(route),
            apps: None,
            analytics: None,
            skipped: notes,
            record: Some(record),
            configured,
        };
        return ctx
            .out
            .report(&report, |lines| report_summary(&report, lines));
    }

    // `--offline` and the App Hub ask for opposite things, so the step is a
    // reported skip rather than a failure: the route and the analytics tables
    // need nothing but this deployment, and both still happen.
    let apps = match ctx.registry.offline {
        true => {
            skipped.push(dhis2::OFFLINE_APPS.to_string());
            None
        }
        false => {
            let which = args
                .modeling_app
                .clone()
                .unwrap_or_else(|| session.project.state.components.modeling_app().clone());
            let apps = install_apps(ctx, &session, &which);
            record_modeling_app(&mut session.project, args.modeling_app.as_ref(), &apps)?;
            Some(apps)
        }
    };
    let failed = apps.as_ref().and_then(failed_apps);

    let analytics = run_analytics(ctx, &session, args.timeout, args.no_wait)?;

    let done = route.verified && failed.is_none() && analytics.finished && skipped.is_empty();
    // Written before the report is printed, so what the screen says and what
    // `.chap-devops/components.yaml` holds cannot disagree.
    let record = record_connect(ctx, &mut session.project, judge(&route, apps.as_ref()))?;
    let report = Dhis2Report {
        instance: session.instance(),
        next: match done {
            true => "the Modeling App can reach Chap; open DHIS2 with `chap-devops open dhis2`"
                .to_string(),
            false => "run `chap-devops dhis2 show` to see what is still missing".to_string(),
        },
        route: Some(route),
        apps,
        analytics: Some(analytics),
        skipped,
        record: Some(record),
        configured,
    };
    ctx.out
        .report(&report, |lines| report_summary(&report, lines))?;
    match failed {
        Some(why) => Err(anyhow::anyhow!(why)),
        None => Ok(()),
    }
}

/// The last line of `connect` on an external DHIS2. `missing` is the apps
/// the listing did not find, or `None` when the apps could not be listed.
pub(super) fn external_next(verified: bool, missing: Option<&[&str]>) -> String {
    match (verified, missing) {
        (false, _) => "run `chap-devops dhis2 show` to see what is still missing".to_string(),
        (true, Some([])) => {
            "the Modeling App can reach Chap; open DHIS2 with `chap-devops open dhis2`".to_string()
        }
        (true, Some(missing)) => format!(
            "the Modeling App can reach Chap after the admin of this DHIS2 installs {}; \
             `chap-devops dhis2 show` says when they are there",
            missing.join(" and ")
        ),
        (true, None) => "the Modeling App can reach Chap after the Modeling App and the Climate \
                         App are installed; `chap-devops dhis2 show` says which are there"
            .to_string(),
    }
}

/// The two steps `connect` leaves to the admin of an external DHIS2, with
/// the apps it found missing named, as the `skipped:` lines of its report.
fn external_left_alone(missing: &[&str]) -> Vec<String> {
    let apps = match missing.is_empty() {
        true => "installing apps: both are there already".to_string(),
        false => format!(
            "installing apps on a DHIS2 chap-devops does not run ({}); `chap-devops dhis2 apps` installs \
             them if its admin agrees",
            missing
                .iter()
                .map(|label| format!("{label} is not installed"))
                .collect::<Vec<String>>()
                .join(", ")
        ),
    };
    vec![
        apps,
        "generating analytics tables on a DHIS2 chap-devops does not run; `chap-devops dhis2 analytics` \
         starts a run if its admin agrees"
            .to_string(),
    ]
}

/// What one `connect` run is entitled to say about the deployment it ran
/// against.
///
/// Three states rather than two, because a run can also fail to look. Clearing
/// a record is an assertion that something is broken, and an assertion needs
/// evidence: a step this run skipped is not evidence of anything, and a skip is
/// a reported skip everywhere else in chap-devops rather than a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Judgement {
    /// The route was proved and both apps are in place, or, on an external
    /// DHIS2, the route was proved. Stamp the time.
    Connected,
    /// This run found something wrong - a route nothing answered through, or an
    /// app missing or failed. Forget any earlier record, so the hint comes
    /// back.
    Broken,
    /// This run did not get far enough to have an opinion: the route was proved
    /// but the apps were never attempted, which is `--offline`. Leave the
    /// record exactly as it was found, in either direction.
    Unknown,
}

/// What this run proved about the shape the hint exists to ask for: a route
/// chap-core answered through, and both apps installed.
///
/// **Two of the three steps, and deliberately so.** The hint is there because a
/// DHIS2 beside a Chap cannot be used at all - the Modeling App redirects to
/// `/get-started` without the route, and there is no Chap user interface in
/// DHIS2 without the apps. Neither is true of analytics: the app is installed,
/// reaches chap-core and works, and empty `analytics_*` tables are a deployment
/// with no data rather than one that cannot talk to itself. `chap-devops dhis2 show`
/// reports the analytics question separately and with the evidence it is worth,
/// which is where it belongs, and `chap-devops dhis2 analytics --no-wait` is a
/// supported shape in which nothing here could ever wait for a finished run.
///
/// The route is judged by `verified` rather than by `outcome`: a row written
/// into DHIS2 proves nothing, and the proxied request proves DHIS2 resolved the
/// hostname, was allowed to reach it and got chap-core's health document back.
/// A route that did not verify is [`Judgement::Broken`] whether or not the apps
/// were attempted, because the failure is this run's own evidence.
///
/// The apps are judged as a set, and only when there was a set to judge. An
/// `--offline` run installs none of them and reports the skip, which says
/// nothing whatever about the apps in DHIS2 - they are very likely the two this
/// command put there - so it is [`Judgement::Unknown`] rather than a verdict of
/// breakage built out of a step that was never run.
pub(super) fn judge(route: &RouteReport, apps: Option<&AppsReport>) -> Judgement {
    match (route.verified, apps) {
        (false, _) => Judgement::Broken,
        (true, None) => Judgement::Unknown,
        (true, Some(apps)) => {
            let all_there = apps.apps.len() == dhis2::HUB_APPS.len()
                && apps
                    .apps
                    .iter()
                    .all(|app| app.outcome != AppOutcome::Failed);
            match all_there {
                true => Judgement::Connected,
                false => Judgement::Broken,
            }
        }
    }
}

/// Write what this run is worth into `.chap-devops/components.yaml`, and say which of
/// the three things happened to the file.
///
/// A run that got there stamps the time; a run that found it broken forgets
/// whatever was recorded before. Forgetting is the half worth arguing for: it
/// brings the hint back on a deployment whose connect has stopped working, and
/// the cost of being wrong is one line naming an idempotent command, where the
/// cost of the other mistake is a deployment that reports itself healthy while
/// the Modeling App cannot reach Chap. That trade only holds when the run
/// actually found something wrong, which is why [`Judgement::Unknown`] writes
/// nothing in either direction: absence of evidence would otherwise make
/// `chap-devops up` say "chap-devops has not connected this DHIS2 to Chap" about a
/// deployment chap-devops did connect, because of a flag on an unrelated step.
///
/// The file is only written when the value moves, so re-running `connect` on a
/// deployment that was never connected touches nothing.
pub(super) fn record_connect(
    ctx: &Ctx,
    project: &mut Project,
    judgement: Judgement,
) -> Result<ConnectRecord> {
    let was = project.state.components.dhis2_connected_at_mut().is_some();
    let at = match (judgement, was) {
        (Judgement::Unknown, _) | (Judgement::Broken, false) => {
            return Ok(ConnectRecord::Unchanged);
        }
        (Judgement::Broken, true) => None,
        (Judgement::Connected, _) => Some(crate::backup::timestamp(crate::backup::now())),
    };
    ctx.out.verbose(&match &at {
        Some(at) => format!("recording `connected_at: {at}` in `.chap-devops/components.yaml`"),
        None => "clearing `connected_at` in `.chap-devops/components.yaml`".to_string(),
    });
    let record = match at.is_some() {
        true => ConnectRecord::Recorded,
        false => ConnectRecord::Cleared,
    };
    project.update_saved(|state| *state.components.dhis2_connected_at_mut() = at.clone())?;
    Ok(record)
}
