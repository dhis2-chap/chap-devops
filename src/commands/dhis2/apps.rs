//! `chap-devops dhis2 apps`: the Modeling App and the Climate App from the App Hub.

use super::*;
use crate::components::ModelingApp;

/// `chap-devops dhis2 apps` — install the Modeling App and the Climate App.
pub fn apps(ctx: &Ctx, args: &Dhis2AppsArgs) -> Result<()> {
    if ctx.registry.offline {
        return Err(anyhow::anyhow!(dhis2::OFFLINE_APPS));
    }
    let mut session = open_session(ctx, &args.common)?;
    let which = args
        .modeling_app
        .clone()
        .unwrap_or_else(|| session.project.state.components.modeling_app().clone());
    let apps = install_apps(ctx, &session, &which);
    record_modeling_app(&mut session.project, args.modeling_app.as_ref(), &apps)?;
    let failed = failed_apps(&apps);
    let report = Dhis2Report {
        instance: session.instance(),
        next: "open DHIS2 with `chap-devops open dhis2`; the Modeling App is in its apps menu"
            .to_string(),
        route: None,
        apps: Some(apps),
        analytics: None,
        skipped: Vec::new(),
        record: None,
        configured: None,
    };
    ctx.out
        .report(&report, |lines| report_summary(&report, lines))?;
    match failed {
        Some(why) => Err(anyhow::anyhow!(why)),
        None => Ok(()),
    }
}

/// Install both apps, taking each one as far as it goes.
///
/// One app's failure does not stop the other: they are independent, and an
/// operator who can get the Modeling App installed while the Climate App is
/// having a bad day is better off than one who gets neither. `which` is the
/// Modeling App build; the Climate App is always the stable one.
pub(super) fn install_apps(ctx: &Ctx, session: &Session, which: &ModelingApp) -> AppsReport {
    let listing = session
        .dhis2
        .get_json(dhis2::APPS_PATH)
        .unwrap_or(serde_json::Value::Null);
    let hub = dhis2::app_hub_base();
    AppsReport {
        apps: dhis2::HUB_APPS
            .iter()
            .map(|app| {
                let installed = match (app.id == dhis2::MODELING_APP_ID, which) {
                    (true, ModelingApp::Unstable) => install_unstable(ctx, session, app, &listing),
                    (true, which) => install_app(ctx, session, &hub, app, &listing, which),
                    (false, _) => {
                        install_app(ctx, session, &hub, app, &listing, &ModelingApp::Stable)
                    }
                };
                installed.unwrap_or_else(|why| AppReport {
                    name: app.name.to_string(),
                    outcome: AppOutcome::Failed,
                    version: String::new(),
                    previous: None,
                    reason: Some(first_line(&why.to_string())),
                    build: None,
                })
            })
            .collect(),
    }
}

/// Resolve one app's version on the App Hub and have DHIS2 install it: the
/// newest one this DHIS2 can run, or the one `which` names.
fn install_app(
    ctx: &Ctx,
    session: &Session,
    hub: &str,
    app: &HubAppRef,
    listing: &serde_json::Value,
    which: &ModelingApp,
) -> Result<AppReport> {
    let published = dhis2::fetch_hub_app(hub, app.id, dhis2::DEFAULT_TIMEOUT)?;
    // One name in the report, whichever spelling each side uses: the App Hub
    // publishes the Modeling App as `Modeling` and an instance lists it under a
    // third name again, so a report that used all three would read as three
    // apps. The App Hub's own is still what the instance's listing is matched
    // on, because that is the spelling the instance got its name from.
    let name = app.name.to_string();
    let published_as = match published.name.trim().is_empty() {
        true => app.name,
        false => published.name.trim(),
    };
    let version = match which {
        ModelingApp::Version(wanted) => {
            dhis2::exact_version(&published.versions, wanted, &session.ready.version).map_err(
                |why| {
                    anyhow::anyhow!(
                        "{name} {wanted} cannot be installed: {why}; pass another \
                         `--modeling-app`, or `--modeling-app stable`"
                    )
                },
            )?
        }
        _ => dhis2::pick_version(&published.versions, &session.ready.version).ok_or_else(|| {
            anyhow::anyhow!(
                "the App Hub publishes no version of {name} that DHIS2 {} can run; install it \
                 from DHIS2's own App Management page",
                display_version(&session.ready.version)
            )
        })?,
    };
    let installed = dhis2::installed_app(listing, published_as);
    // An unstable build carries the version of the release before it, so the
    // same version number can still be the unstable build. The record says
    // which of the two chap-devops put there.
    let unstable_there = app.id == dhis2::MODELING_APP_ID
        && session
            .project
            .state
            .components
            .modeling_app_build()
            .is_some();
    if let Some(installed) = &installed
        && !unstable_there
        && dhis2::compare_versions(&installed.version, &version.version)
            == std::cmp::Ordering::Equal
    {
        return Ok(AppReport {
            name,
            outcome: AppOutcome::Unchanged,
            version: version.version.clone(),
            previous: None,
            reason: None,
            build: None,
        });
    }

    let path = dhis2::install_path(&version.id);
    ctx.out
        .verbose(&format!("installing {name} {}", version.version));
    // The instance downloads the app itself, so this one request is a transfer.
    let client = session.dhis2.with_timeout(dhis2::INSTALL_TIMEOUT);
    let answer = client.send("POST", &path, None)?;
    if !answer.is_success() {
        return Err(client.status_error(&path, &answer));
    }
    let previous = match unstable_there {
        true => session
            .project
            .state
            .components
            .modeling_app_build()
            .map(unstable_label),
        false => installed.as_ref().map(|app| app.version.clone()),
    };
    Ok(AppReport {
        name,
        outcome: match &installed {
            Some(_) => AppOutcome::Moved,
            None => AppOutcome::Installed,
        },
        version: version.version.clone(),
        previous,
        reason: None,
        build: None,
    })
}

/// Download the unstable Modeling App from GitHub and upload it to DHIS2,
/// unless the build the state records is the one GitHub lists now.
fn install_unstable(
    ctx: &Ctx,
    session: &Session,
    app: &HubAppRef,
    listing: &serde_json::Value,
) -> Result<AppReport> {
    let name = app.name.to_string();
    let unstable = dhis2::fetch_unstable(dhis2::DEFAULT_TIMEOUT)?;
    let installed = dhis2::installed_app(listing, app.name);
    let recorded = session.project.state.components.modeling_app_build();
    let version = unstable_label(&unstable.build);
    // Installed, and the same build: the digest changes with every build of
    // `main`. A DHIS2 that lost the app gets it again, whatever the record says.
    if installed.is_some() && recorded.is_some_and(|build| build.digest == unstable.build.digest) {
        return Ok(AppReport {
            name,
            outcome: AppOutcome::Unchanged,
            version,
            previous: None,
            reason: None,
            build: Some(unstable.build),
        });
    }

    ctx.out.verbose(&format!(
        "downloading {} from {}",
        dhis2::UNSTABLE_ASSET,
        dhis2::FRONTEND_REPO
    ));
    let zip = dhis2::download_unstable(&unstable, dhis2::INSTALL_TIMEOUT)?;
    let (body, content_type) = dhis2::app_upload(&zip);
    ctx.out.verbose(&format!("installing {name} {version}"));
    let client = session.dhis2.with_timeout(dhis2::INSTALL_TIMEOUT);
    let answer = client.send_bytes("POST", dhis2::APPS_PATH, &body, &content_type)?;
    if !answer.is_success() {
        return Err(client.status_error(dhis2::APPS_PATH, &answer));
    }
    let previous = match recorded {
        Some(build) => Some(unstable_label(build)),
        None => installed.as_ref().map(|app| app.version.clone()),
    };
    Ok(AppReport {
        name,
        outcome: match &installed {
            Some(_) => AppOutcome::Moved,
            None => AppOutcome::Installed,
        },
        version,
        previous,
        reason: None,
        build: Some(unstable.build),
    })
}

/// `unstable 0c30f8d`: how a report names an unstable build.
pub(super) fn unstable_label(build: &crate::components::ModelingAppBuild) -> String {
    format!("unstable {}", build.short_commit())
}

/// Write the Modeling App setting and the installed build to
/// `.chap-devops/components.yaml`, for whichever DHIS2 this session talks to.
///
/// `chosen` is a `--modeling-app` given on this run, which later runs follow.
/// Both are recorded only after an install or a check that worked: a failed
/// install leaves the file as it was.
pub(super) fn record_modeling_app(
    project: &mut Project,
    chosen: Option<&ModelingApp>,
    apps: &AppsReport,
) -> Result<()> {
    let modeling_name = dhis2::HUB_APPS
        .iter()
        .find(|app| app.id == dhis2::MODELING_APP_ID)
        .map(|app| app.name);
    let modeling = apps
        .apps
        .iter()
        .find(|app| Some(app.name.as_str()) == modeling_name);
    let build = match modeling {
        Some(app) if app.outcome != AppOutcome::Failed => Some(app.build.clone()),
        _ => None,
    };
    // A choice that could not be installed is not one later runs follow.
    let chosen = chosen.filter(|_| build.is_some());
    let components = &project.state.components;
    let setting_moves = chosen.is_some_and(|chosen| chosen != components.modeling_app());
    let build_moves = build
        .as_ref()
        .is_some_and(|build| build.as_ref() != components.modeling_app_build());
    if !setting_moves && !build_moves {
        return Ok(());
    }
    project.update_saved(|state| {
        if let Some(chosen) = chosen {
            *state.components.modeling_app_mut() = chosen.clone();
        }
        if let Some(build) = &build {
            *state.components.modeling_app_build_mut() = build.clone();
        }
    })
}

/// The one line the command fails with when an app could not be installed.
pub(super) fn failed_apps(report: &AppsReport) -> Option<String> {
    let failed: Vec<&AppReport> = report
        .apps
        .iter()
        .filter(|app| app.outcome == AppOutcome::Failed)
        .collect();
    let first = failed.first()?;
    Some(format!(
        "{} of {} could not be installed: {}",
        failed.len(),
        report.apps.len(),
        first.reason.clone().unwrap_or_default()
    ))
}
