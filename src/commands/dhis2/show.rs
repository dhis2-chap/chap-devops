//! `chap-devops dhis2 show`: what DHIS2 has, asked and reported, changing nothing.

use super::*;

/// `chap-devops dhis2 show` — what this DHIS2 has, changing nothing.
pub fn show(ctx: &Ctx, args: &Dhis2ShowArgs) -> Result<()> {
    let session = open_session(ctx, &args.common)?;
    // Without chap-core the route has nothing to point at: `connect` and
    // `route` refuse, so `show` names no target and judges no route.
    let chap_core = session.project.state.components.has_chap_core_api();
    let target = match chap_core {
        true => session.target(),
        false => String::new(),
    };
    let route = dhis2::route_in(&session.dhis2.get_json(&dhis2::routes_query())?);
    let token_needed = crate::api::token_for(Some(&session.project.dir)).is_some();
    let shown = route.as_ref().map(|route| {
        let (proof, way_out) = match chap_core && route.url == target {
            // Only worth proxying through a route that points here: one aimed
            // at somebody else's chap-core would answer, and the answer would
            // mean nothing about this deployment.
            true => {
                let proof = verify_route(&session.dhis2, token_needed);
                let way_out = match proof.verified || proof.token_refused {
                    true => String::new(),
                    false => unanswered_way_out(&session, &proof, &target),
                };
                (proof, way_out)
            }
            false => (
                RouteProof {
                    verified: false,
                    answered: match chap_core {
                        true => "it points at another chap-core",
                        false => "this deployment has no chap-core",
                    }
                    .to_string(),
                    token_refused: false,
                    empty_503: false,
                },
                String::new(),
            ),
        };
        ShownRoute {
            url: route.url.clone(),
            disabled: route.disabled,
            authorised: route
                .authorities
                .iter()
                .any(|authority| authority == dhis2::ROUTE_AUTHORITY),
            ours: chap_core && route.url == target,
            verified: proof.verified,
            answered: proof.answered,
            token_refused: proof.token_refused,
            way_out,
        }
    });
    let apps = chap_apps(&session)?;
    let analytics = analytics_evidence(ctx, &session);

    let mut missing = Vec::new();
    let route_ok = shown
        .as_ref()
        .is_some_and(|route| route.ours && !route.disabled && route.authorised && route.verified);
    match &shown {
        _ if !chap_core => {}
        None => missing.push("there is no `chap` route".to_string()),
        Some(route) if !route.ours => missing.push(format!(
            "the `chap` route points at {}, not at this deployment",
            route.url
        )),
        Some(route) if route.disabled => missing.push("the `chap` route is disabled".to_string()),
        Some(route) if !route.authorised => missing.push(format!(
            "the `chap` route is missing the {} authority",
            dhis2::ROUTE_AUTHORITY
        )),
        Some(route) if route.token_refused => missing.push(format!(
            "the `chap` route does not carry chap-core's API token: {}; run `chap-devops dhis2 connect` \
             to put it there",
            route.answered
        )),
        // A row that is right in every field and that nothing answers through
        // is the one case the proxied request exists to catch: the app would
        // be told Chap is reachable and then fail on its first request.
        Some(route) if !route.verified => missing.push(format!(
            "nothing answered through the `chap` route: {}; {}",
            route.answered, route.way_out
        )),
        Some(_) => {}
    }
    match analytics {
        dhis2::AnalyticsEvidence::Never => missing.push("analytics has never run".to_string()),
        // Not "analytics has never run": it may well have, and chap-devops cannot
        // see the tables to tell. What it can say is that the timestamp beside
        // it proves nothing, and that one command settles the question.
        dhis2::AnalyticsEvidence::Unconfirmed => missing.push(
            "analytics may never have run on this deployment: a seeded database can carry the \
             dump's timestamp, and no run has finished since DHIS2 started; run \
             `chap-devops dhis2 analytics` to settle it"
                .to_string(),
        ),
        dhis2::AnalyticsEvidence::RanHere | dhis2::AnalyticsEvidence::Recorded => {}
    }
    let analytics_ok = matches!(
        analytics,
        dhis2::AnalyticsEvidence::RanHere | dhis2::AnalyticsEvidence::Recorded
    );
    missing.extend(missing_apps(&apps));
    let next = match chap_core {
        true => show_next(
            session.external().is_some(),
            route_ok,
            !missing_labels(&apps).is_empty(),
            analytics_ok,
        ),
        false => no_chap_core_next(!missing_labels(&apps).is_empty(), analytics_ok),
    };

    let report = ShowReport {
        instance: session.instance(),
        target,
        route: shown,
        last_analytics: session.ready.last_analytics.clone(),
        analytics,
        apps,
        next,
        missing,
    };
    if !ctx.out.json {
        print!("{}", show_table(&report, &ctx.out));
    }
    ctx.out
        .report(&report, |lines| show_summary(&report, lines))
}

/// The last line of `show`: the command that does what is still missing.
///
/// On the `dhis2` component, `chap-devops dhis2 connect` does all three steps. On
/// an external DHIS2 it sets only the route, so the apps and analytics are
/// named by their own commands, which its admin runs.
pub(super) fn show_next(
    external: bool,
    route_ok: bool,
    apps_missing: bool,
    analytics_ok: bool,
) -> String {
    if route_ok && !apps_missing && analytics_ok {
        return "the Modeling App can reach Chap; open DHIS2 with `chap-devops open dhis2`"
            .to_string();
    }
    if !external {
        return "run `chap-devops dhis2 connect` to do the rest".to_string();
    }
    let mut steps = Vec::new();
    if !route_ok {
        steps.push("`chap-devops dhis2 connect` for the route");
    }
    if apps_missing {
        steps.push("`chap-devops dhis2 apps` for the apps");
    }
    if !analytics_ok {
        steps.push("`chap-devops dhis2 analytics` for the analytics tables");
    }
    let admin = match apps_missing || !analytics_ok {
        true => "; the apps and analytics change this DHIS2, so ask its admin first",
        false => "",
    };
    format!("run {}{admin}", steps.join(", then "))
}

/// The last line of `show` on a deployment without chap-core.
///
/// The route needs chap-core, so `chap-devops dhis2 connect` is not named: it
/// refuses there. The apps and analytics have commands of their own that
/// work without it.
pub(super) fn no_chap_core_next(apps_missing: bool, analytics_ok: bool) -> String {
    let route =
        "the `chap` route needs chap-core; `chap-devops components enable chap-core` adds it";
    let mut steps = Vec::new();
    if apps_missing {
        steps.push("`chap-devops dhis2 apps` for the apps");
    }
    if !analytics_ok {
        steps.push("`chap-devops dhis2 analytics` for the analytics tables");
    }
    match steps.is_empty() {
        true => route.to_string(),
        false => format!("run {}; {route}", steps.join(", then ")),
    }
}

/// The two apps chap-devops installs, as this instance has them.
///
/// **The two, and not the rest.** `show` exists to say whether DHIS2 has what
/// Chap needs; the climate demo ships 29 bundled apps of its own, the same 29
/// every DHIS2 ships, and printing them turned the one row that answers the
/// question into an inventory with the answer somewhere in the middle of it.
/// A count of the others would be a number that is always about thirty and
/// never means anything, so there is none: `GET /api/apps` is one request away
/// for anybody who wants the list.
///
/// Matched on the App Hub's own name rather than on the label a sentence uses,
/// because the two differ: the app published as `DHIS2 Climate App` is `the
/// Climate App` in prose. [`dhis2::installed_app`] is what copes with the third
/// spelling, the instance's own - it lists the Modeling App as `Modeling`.
pub(super) fn chap_apps(session: &Session) -> Result<Vec<ShownApp>> {
    let listing = session.dhis2.get_json(dhis2::APPS_PATH)?;
    let build = session.project.state.components.modeling_app_build();
    Ok(dhis2::HUB_APPS
        .iter()
        .map(|app| {
            let build = build.filter(|_| app.id == dhis2::MODELING_APP_ID);
            shown_app(app, dhis2::installed_app(&listing, app.name), build)
        })
        .collect())
}

/// One [`dhis2::HUB_APPS`] row, found or not.
pub(super) fn shown_app(
    app: &HubAppRef,
    found: Option<dhis2::InstalledApp>,
    build: Option<&crate::components::ModelingAppBuild>,
) -> ShownApp {
    ShownApp {
        name: app.name.to_string(),
        installed: found.is_some(),
        build: build
            .filter(|_| found.is_some())
            .map(super::apps::unstable_label),
        version: found.map(|found| found.version).unwrap_or_default(),
    }
}

/// Which of the two apps this instance does not have.
pub(super) fn missing_apps(apps: &[ShownApp]) -> Vec<String> {
    missing_labels(apps)
        .into_iter()
        .map(|label| format!("{label} is not installed"))
        .collect()
}

/// The prose names of the two apps this instance does not have.
pub(super) fn missing_labels(apps: &[ShownApp]) -> Vec<&'static str> {
    dhis2::HUB_APPS
        .iter()
        .filter(|app| {
            !apps
                .iter()
                .any(|shown| shown.installed && shown.name == app.name)
        })
        .map(|app| app.label)
        .collect()
}

/// What this deployment's `lastAnalyticsTableSuccess` is actually worth.
///
/// Two things it is not read against: DHIS2's notifier, which says whether a
/// run has finished on *this* instance since it started, and
/// `.chap-devops/components.yaml`, which says whether the database was restored from
/// a seed dump and so came with somebody else's timestamp in it.
///
/// A notifier that cannot be read is a trace line and not a failure - `show`
/// reaches DHIS2 and stops there - and it leaves the verdict on the cautious
/// side, which is the side that does not claim analytics is done.
fn analytics_evidence(ctx: &Ctx, session: &Session) -> dhis2::AnalyticsEvidence {
    let ran_here = match session.dhis2.get_json(&dhis2::jobs_path()) {
        Ok(tasks) => dhis2::finished_here(&tasks),
        Err(why) => {
            ctx.out.verbose(&format!(
                "{} could not be read ({why}), so no analytics run is counted as seen here",
                dhis2::jobs_path()
            ));
            false
        }
    };
    // A seed dump is something chap-devops restores into its own `dhis2_db`; an
    // external DHIS2 came with nothing of chap-devops', so its timestamp is its own.
    let seeded = session.external().is_none()
        && session
            .project
            .state
            .components
            .dhis2_seed_source()
            .is_some();
    dhis2::analytics_evidence(&session.ready.last_analytics, ran_here, seeded)
}
