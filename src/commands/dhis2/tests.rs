use super::*;
use crate::output::Report;

fn out() -> Out {
    Out::detect(false, true)
}

/// The lines a summary builds, with the level in front of hints and warnings.
fn lines(build: impl FnOnce(&mut Report)) -> String {
    let mut report = Report::default();
    build(&mut report);
    report.text()
}

/// What `route`, `analytics`, `apps` and `connect` print.
fn human_report(report: &Dhis2Report) -> String {
    lines(|l| report_summary(report, l))
}

/// What `show` prints: the rows, then the lines.
fn human_show(report: &ShowReport, out: &Out) -> String {
    format!(
        "{}{}",
        show_table(report, out),
        lines(|l| show_summary(report, l))
    )
}

/// A [`Ctx`] for the one thing in here that writes a file.
fn ctx(dir: &std::path::Path) -> Ctx {
    Ctx {
        out: Out::default(),
        project_dir: dir.to_path_buf(),
        registry: crate::registry::RegistryOptions::default(),
        cli_version: "0.1.0",
    }
}

/// A timestamp in the shape [`record_connect`] writes.
const CONNECTED: &str = "2026-09-27T09:12:33Z";

fn instance() -> Instance {
    Instance {
        url: "http://localhost:18080".to_string(),
        version: "2.42.6".to_string(),
        user: "admin".to_string(),
        external: false,
        auth: AuthKind::Basic,
        credential_from: CredentialSource::Default,
    }
}

fn route_report(outcome: RouteOutcome) -> RouteReport {
    RouteReport {
        outcome,
        code: dhis2::ROUTE_CODE,
        url: "http://chap:8000/**".to_string(),
        previous: None,
        reasons: Vec::new(),
        id: Some("abc123".to_string()),
        verified: true,
        answered: "healthy".to_string(),
        token_refused: false,
        way_out: String::new(),
    }
}

fn report(route: Option<RouteReport>) -> Dhis2Report {
    Dhis2Report {
        instance: instance(),
        route,
        apps: None,
        analytics: None,
        skipped: Vec::new(),
        record: None,
        configured: None,
        next: "run `chap-devops dhis2 show` to see what is still missing".to_string(),
    }
}

fn apps_report(outcomes: &[AppOutcome]) -> AppsReport {
    AppsReport {
        apps: outcomes
            .iter()
            .zip(dhis2::HUB_APPS)
            .map(|(outcome, app)| AppReport {
                name: app.name.to_string(),
                outcome: *outcome,
                version: "7.1.0".to_string(),
                previous: None,
                reason: None,
                build: None,
            })
            .collect(),
    }
}

/// What a `connect` has to have got through before anything is recorded:
/// the route proved, and both apps in place. Analytics is not part of it -
/// the Modeling App reaches chap-core and works without the tables, and an
/// empty `analytics_*` is a deployment with no data rather than one that
/// cannot talk to itself.
///
/// And the third answer, which is the one that keeps `connect` honest: a
/// run that did not look. `--offline` skips the app install and reports the
/// skip, so it knows nothing about the apps in DHIS2 - very likely the two
/// an earlier run put there - and says so rather than calling the
/// deployment broken over a flag on an unrelated step.
#[test]
fn only_a_proved_route_with_both_apps_counts_as_connected() {
    let both = apps_report(&[AppOutcome::Installed, AppOutcome::Unchanged]);
    let created = route_report(RouteOutcome::Created);
    assert_eq!(judge(&created, Some(&both)), Judgement::Connected);
    // A route that was already right is as good as one just written.
    assert_eq!(
        judge(&route_report(RouteOutcome::Unchanged), Some(&both)),
        Judgement::Connected
    );

    // A row in DHIS2's database proves nothing; the proxied request does.
    let unverified = RouteReport {
        verified: false,
        answered: "HTTP 502 Bad Gateway".to_string(),
        ..route_report(RouteOutcome::Created)
    };
    assert_eq!(judge(&unverified, Some(&both)), Judgement::Broken);
    // A failed request is this run's own evidence, so it counts whether or
    // not the apps were attempted.
    assert_eq!(judge(&unverified, None), Judgement::Broken);

    // One app short is a DHIS2 with no Chap user interface in it.
    assert_eq!(
        judge(
            &created,
            Some(&apps_report(&[AppOutcome::Installed, AppOutcome::Failed])),
        ),
        Judgement::Broken
    );
    assert_eq!(
        judge(&created, Some(&apps_report(&[AppOutcome::Installed]))),
        Judgement::Broken
    );

    // And `--offline`, which installs none of them and reports the skip:
    // nothing was looked at, so nothing is judged.
    assert_eq!(judge(&created, None), Judgement::Unknown);
}

/// The record moves on evidence, in either direction, and a run with no
/// evidence leaves it alone.
///
/// The transition that matters is the middle one: a deployment that has
/// been connected, then `chap-devops dhis2 connect --offline`, which proves the
/// route and skips the apps. The record is still there afterwards, because
/// nothing that run saw says it stopped being true. The same deployment
/// with a route nothing answered through loses it, and the hint comes back.
#[test]
fn a_run_that_could_not_look_leaves_the_record_as_it_found_it() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = ctx(temp.path());
    let mut project = Project {
        dir: temp.path().to_path_buf(),
        state: crate::project::ProjectState::default(),
    };
    project.state.components.set_enabled(Component::Dhis2, true);
    project.state.components.dhis2.connected_at = Some(CONNECTED.to_string());
    project.save().unwrap();
    let on_disk = || {
        Project::load(temp.path())
            .unwrap()
            .state
            .components
            .dhis2
            .connected_at
    };

    // The `--offline` shape, on a deployment chap-devops did connect.
    assert_eq!(
        record_connect(&ctx, &mut project, Judgement::Unknown).unwrap(),
        ConnectRecord::Unchanged
    );
    assert_eq!(on_disk().as_deref(), Some(CONNECTED));

    // Evidence of breakage, on the same deployment.
    assert_eq!(
        record_connect(&ctx, &mut project, Judgement::Broken).unwrap(),
        ConnectRecord::Cleared
    );
    assert_eq!(project.state.components.dhis2.connected_at, None);
    // On disk, not only in memory: the next command reads the file.
    assert_eq!(on_disk(), None);
    assert!(project.state.components.dhis2_needs_connecting());

    // A deployment with no record is given none by a run that judged
    // nothing, and a broken run has nothing left to forget.
    assert_eq!(
        record_connect(&ctx, &mut project, Judgement::Unknown).unwrap(),
        ConnectRecord::Unchanged
    );
    assert_eq!(
        record_connect(&ctx, &mut project, Judgement::Broken).unwrap(),
        ConnectRecord::Unchanged
    );
    assert_eq!(on_disk(), None);

    // And a run that got there stamps the time it ran.
    assert_eq!(
        record_connect(&ctx, &mut project, Judgement::Connected).unwrap(),
        ConnectRecord::Recorded
    );
    let at = on_disk().expect("recorded");
    assert!(at.ends_with('Z'), "a UTC timestamp: {at}");
    assert!(!project.state.components.dhis2_needs_connecting());

    // Which an `--offline` run after it keeps exactly as it is.
    assert_eq!(
        record_connect(&ctx, &mut project, Judgement::Unknown).unwrap(),
        ConnectRecord::Unchanged
    );
    assert_eq!(on_disk().as_deref(), Some(at.as_str()));
}

/// The record says what it is and what it is not, in the place a reader
/// could otherwise take it for a verdict.
#[test]
fn the_record_line_never_claims_the_route_is_right() {
    let recorded = lines(|l| record_lines(ConnectRecord::Recorded, l));
    assert_eq!(
        recorded,
        "hint: recorded the connect in `.chap-devops/components.yaml`, so `chap-devops up` and \
         `chap-devops status` stop asking for it\n\
         hint: the record says that this ran, not that the route is still right; \
         `chap-devops dhis2 show` asks DHIS2\n"
    );

    let cleared = lines(|l| record_lines(ConnectRecord::Cleared, l));
    assert!(
        cleared.starts_with("hint: cleared the earlier `chap-devops dhis2 connect`"),
        "{cleared}"
    );
    assert!(cleared.contains("ask for it again"), "{cleared}");

    // Nothing moved, nothing said: the report is already a full account of
    // what the run did.
    assert_eq!(lines(|l| record_lines(ConnectRecord::Unchanged, l)), "");
}

/// The opening line says where, what and who - and nothing whatsoever
/// about the password beyond where it was found.
#[test]
fn the_instance_line_never_carries_the_password() {
    let text = instance_line(&instance());
    assert_eq!(text, "DHIS2 2.42.6 at http://localhost:18080, as `admin`");
    let hint = credential_hint(&instance());
    assert_eq!(hint, "the credential is the DHIS2 default password");
    assert!(!text.contains("district") && !hint.contains("district"));

    let from_file = Instance {
        credential_from: CredentialSource::EnvFile,
        ..instance()
    };
    assert!(credential_hint(&from_file).contains("password from `.env`"));

    // A token is named by what it is, with the user DHIS2 said owns it,
    // and an external DHIS2 says that it is one.
    let token = Instance {
        user: "ops".to_string(),
        external: true,
        auth: AuthKind::Token,
        credential_from: CredentialSource::Environment,
        ..instance()
    };
    assert_eq!(
        instance_line(&token),
        "external DHIS2 2.42.6 at http://localhost:18080, as `ops`"
    );
    assert_eq!(
        credential_hint(&token),
        "the credential is API token from CHAP_DEVOPS_DHIS2_TOKEN"
    );

    // An instance that did not say its version still gets a line.
    let bare = Instance {
        version: String::new(),
        ..instance()
    };
    assert!(
        instance_line(&bare).starts_with("DHIS2 (version unknown) at"),
        "{}",
        instance_line(&bare)
    );
}

/// A repoint says where it pointed before: that is the whole reason the
/// step is not "create if absent".
#[test]
fn a_repointed_route_prints_the_reason_it_was_wrong() {
    let mut route = route_report(RouteOutcome::Repointed);
    route.previous = Some("http://158.39.75.126/stable/**".to_string());
    route.reasons = vec!["it pointed at http://158.39.75.126/stable/**".to_string()];
    let text = human_report(&report(Some(route)));
    assert!(
        text.contains(
            "repointed the `chap` route at http://chap:8000/**; chap-core answered through it\n"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "hint: the route was rewritten because it pointed at http://158.39.75.126/stable/**"
        ),
        "{text}"
    );
    assert!(text.contains("hint: chap-core said: healthy"), "{text}");
}

/// Nothing to do is still said out loud, and named as nothing to do.
#[test]
fn a_route_that_is_already_right_says_there_was_nothing_to_change() {
    let text = human_report(&report(Some(route_report(RouteOutcome::Unchanged))));
    assert!(
        text.contains("already points at http://chap:8000/**; chap-core answered through it"),
        "{text}"
    );
    assert!(
        text.contains("hint: run `chap-devops dhis2 show`"),
        "{text}"
    );
}

/// A route that was written but proved nothing says so rather than
/// claiming the path works.
#[test]
fn an_unverified_route_does_not_claim_chap_core_answered() {
    let mut route = route_report(RouteOutcome::Created);
    route.verified = false;
    route.answered = "HTTP 502 Bad Gateway".to_string();
    let text = human_report(&report(Some(route)));
    assert!(
        text.contains(
            "warning: the `chap` route is in place but nothing answered through it: \
             HTTP 502 Bad Gateway"
        ),
        "{text}"
    );
    assert!(!text.contains("chap-core answered through it"), "{text}");
}

/// A route that nothing answered through names the cause that chap-devops checked:
/// chap-core down, the allowlist for an empty 503, or the address that DHIS2
/// uses for chap-core.
#[test]
fn an_unanswered_route_names_the_cause_chap_devops_checked() {
    let proof = |empty_503| RouteProof {
        verified: false,
        answered: "HTTP 503 Service Unavailable".to_string(),
        token_refused: false,
        empty_503,
    };
    let at = || Some("http://localhost:8700".to_string());
    let local = "http://chap:8000/**";
    let external = "http://localhost:8700/**";

    assert_eq!(
        way_out_for(&proof(true), None, None, local),
        "run `chap-devops status` to see whether chap-core is up"
    );
    assert_eq!(
        way_out_for(&proof(true), at(), None, local),
        "chap-core answers at http://localhost:8700, so the DHIS2 allowlist may refuse \
         http://chap:8000; add it to `route.remote_servers_allowed` in `dhis2/dhis.conf` and \
         run `chap-devops restart dhis2`"
    );
    assert!(
        way_out_for(&proof(true), at(), Some("http://localhost:8700"), external)
            .ends_with("in the `dhis.conf` of the DHIS2 server")
    );
    assert_eq!(
        way_out_for(&proof(false), at(), Some("http://localhost:8700"), external),
        "chap-core answers at http://localhost:8700, so DHIS2 may not reach it at \
         http://localhost:8700; give the address DHIS2 reaches chap-core at with \
         `chap-devops dhis2 use --chap-url URL`"
    );
    assert!(
        way_out_for(&proof(false), at(), None, local)
            .ends_with("`chap-devops logs dhis2` may say why")
    );
}

#[test]
fn the_app_lines_say_which_of_the_four_happened() {
    let apps = AppsReport {
        apps: vec![
            AppReport {
                name: "Modeling App".to_string(),
                outcome: AppOutcome::Installed,
                version: "7.1.0".to_string(),
                previous: None,
                reason: None,
                build: None,
            },
            AppReport {
                name: "Climate".to_string(),
                outcome: AppOutcome::Moved,
                version: "1.16.2".to_string(),
                previous: Some("1.15.0".to_string()),
                reason: None,
                build: None,
            },
            AppReport {
                name: "Old".to_string(),
                outcome: AppOutcome::Unchanged,
                version: "1.0.0".to_string(),
                previous: None,
                reason: None,
                build: None,
            },
            AppReport {
                name: "Broken".to_string(),
                outcome: AppOutcome::Failed,
                version: String::new(),
                previous: None,
                reason: Some("HTTP 409".to_string()),
                build: None,
            },
        ],
    };
    let text = lines(|l| app_lines(&apps, l));
    assert!(text.contains("installed Modeling App 7.1.0"), "{text}");
    assert!(
        text.contains("moved Climate from 1.15.0 to 1.16.2"),
        "{text}"
    );
    assert!(text.contains("Old 1.0.0 is already installed"), "{text}");
    assert!(
        text.contains("warning: Broken was not installed: HTTP 409"),
        "{text}"
    );

    let why = failed_apps(&apps).expect("one app failed");
    assert!(why.contains("1 of 4 could not be installed"), "{why}");
    assert!(why.contains("HTTP 409"), "{why}");
    assert_eq!(
        failed_apps(&AppsReport { apps: Vec::new() }),
        None,
        "nothing attempted is not a failure"
    );
}

#[test]
fn the_analytics_lines_tell_a_started_run_from_a_finished_one() {
    let running = AnalyticsReport {
        job: "abc".to_string(),
        started: true,
        finished: false,
        seconds: 0,
        message: String::new(),
        last_success: String::new(),
    };
    let text = lines(|l| analytics_lines(&running, l));
    assert!(
        text.contains("started the analytics run (job abc)"),
        "{text}"
    );

    let adopted = AnalyticsReport {
        started: false,
        ..running.clone()
    };
    let text = lines(|l| analytics_lines(&adopted, l));
    assert!(
        text.contains("an analytics run was already going"),
        "{text}"
    );

    let done = AnalyticsReport {
        finished: true,
        seconds: 65,
        message: "Analytics tables updated".to_string(),
        last_success: "2026-09-25T10:01:00.000".to_string(),
        ..running
    };
    let text = lines(|l| analytics_lines(&done, l));
    assert!(text.contains("analytics finished in 1 minute"), "{text}");
    assert!(text.contains("Analytics tables updated"), "{text}");
    assert!(
        text.contains("DHIS2 records its last analytics success as 2026-09-25T10:01:00.000"),
        "{text}"
    );
}

/// A skipped step is printed, never swallowed.
#[test]
fn a_skipped_step_is_reported_with_the_reason() {
    let mut report = report(Some(route_report(RouteOutcome::Created)));
    report.skipped = vec![dhis2::OFFLINE_APPS.to_string()];
    let text = human_report(&report);
    assert!(text.contains("skipped:"), "{text}");
    assert!(text.contains("App Hub"), "{text}");
}

/// The two apps chap-devops is about, in the order it installs them.
fn shown_apps(versions: &[&str]) -> Vec<ShownApp> {
    dhis2::HUB_APPS
        .iter()
        .zip(versions)
        .map(|(app, version)| ShownApp {
            name: app.name.to_string(),
            installed: !version.is_empty(),
            version: version.to_string(),
            build: None,
        })
        .collect()
}

#[test]
fn show_names_every_missing_piece_and_the_command_that_fixes_them() {
    let report = ShowReport {
        instance: instance(),
        target: "http://chap:8000/**".to_string(),
        route: None,
        last_analytics: String::new(),
        analytics: dhis2::AnalyticsEvidence::Never,
        apps: shown_apps(&["", ""]),
        missing: vec![
            "there is no `chap` route".to_string(),
            "analytics has never run".to_string(),
            "the Modeling App is not installed".to_string(),
        ],
        next: "run `chap-devops dhis2 connect` to do the rest".to_string(),
    };
    let text = human_show(&report, &out());
    assert!(text.contains("route      none"), "{text}");
    assert!(text.contains("analytics  never run"), "{text}");
    assert!(
        text.contains("apps       Modeling App not installed, DHIS2 Climate App not installed"),
        "{text}"
    );
    assert!(text.contains("missing: there is no `chap` route"), "{text}");
    assert!(text.contains("run `chap-devops dhis2 connect`"), "{text}");
}

/// The row is the two apps and nothing else. A real instance lists 29
/// bundled apps of its own, and they are not what this command answers for.
#[test]
fn the_apps_row_is_the_two_apps_chap_devops_installs_and_no_others() {
    let bundled = serde_json::json!([
        {"name": "Reports", "key": "reports", "version": "100.2.4"},
        {"name": "Maintenance app", "key": "maintenance", "version": "32.34.1-v42.0"},
        // The instance's own spelling of the Modeling App, which is not
        // the App Hub's and not chap-devops'.
        {"name": "Modeling", "key": "modeling", "version": "7.1.0"},
    ]);
    let apps: Vec<ShownApp> = dhis2::HUB_APPS
        .iter()
        .map(|app| shown_app(app, dhis2::installed_app(&bundled, app.name), None))
        .collect();
    assert_eq!(apps.len(), 2, "{apps:?}");
    assert_eq!(apps[0].name, "Modeling App");
    assert!(apps[0].installed);
    assert_eq!(apps[0].version, "7.1.0");
    assert!(!apps[1].installed, "{apps:?}");

    let text = human_show(
        &ShowReport {
            instance: instance(),
            target: "http://chap:8000/**".to_string(),
            route: None,
            last_analytics: String::new(),
            analytics: dhis2::AnalyticsEvidence::Never,
            apps,
            missing: Vec::new(),
            next: String::new(),
        },
        &out(),
    );
    assert!(
        text.contains("apps       Modeling App 7.1.0, DHIS2 Climate App not installed"),
        "{text}"
    );
    for other in ["Reports", "Maintenance", "100.2.4"] {
        assert!(!text.contains(other), "{other} is DHIS2's business: {text}");
    }
}

/// The row a seeded deployment gets must not read as "analytics is done",
/// and the one chap-devops watched finish must not read as more than that.
#[test]
fn the_analytics_row_says_what_the_timestamp_is_worth() {
    let recorded = "2026-06-16T09:51:00+02:00";
    // The row is on the clock of this machine, whatever its zone is.
    let when = &crate::output::local_date_time(1_781_596_260);
    let inherited = analytics_cell(dhis2::AnalyticsEvidence::Unconfirmed, recorded, &out());
    assert_eq!(
        inherited,
        format!("{when} (unconfirmed on a seeded database)")
    );

    let here = analytics_cell(dhis2::AnalyticsEvidence::RanHere, recorded, &out());
    assert_eq!(here, format!("{when} (a run finished on this deployment)"));
    // What chap-devops saw is a run finishing, which is not a row count: a run
    // can finish having written nothing, which is why it says neither
    // "done" nor anything about the tables.
    assert!(!here.contains("done"), "{here}");

    assert_eq!(
        analytics_cell(dhis2::AnalyticsEvidence::Recorded, recorded, &out()),
        *when
    );
    // With no offset to read, the time is the server's, and the row says so.
    assert_eq!(
        analytics_cell(
            dhis2::AnalyticsEvidence::Recorded,
            "2026-06-16T07:51:00.093",
            &out()
        ),
        "2026-06-16T07:51:00.093 (DHIS2 server time)"
    );
    assert_eq!(
        analytics_cell(dhis2::AnalyticsEvidence::Never, "", &out()),
        "never run"
    );
}

#[test]
fn show_on_a_connected_instance_says_it_is_connected() {
    let report = ShowReport {
        instance: instance(),
        target: "http://chap:8000/**".to_string(),
        route: Some(ShownRoute {
            url: "http://chap:8000/**".to_string(),
            disabled: false,
            authorised: true,
            ours: true,
            verified: true,
            answered: "healthy".to_string(),
            token_refused: false,
            way_out: String::new(),
        }),
        last_analytics: "2026-09-25T10:01:00+00:00".to_string(),
        analytics: dhis2::AnalyticsEvidence::RanHere,
        apps: shown_apps(&["7.1.0", "1.16.2"]),
        missing: Vec::new(),
        next: "the Modeling App can reach Chap; open DHIS2 with `chap-devops open dhis2`"
            .to_string(),
    };
    let text = human_show(&report, &out());
    assert!(text.contains("http://chap:8000/** (healthy)"), "{text}");
    assert!(
        text.contains("Modeling App 7.1.0, DHIS2 Climate App 1.16.2"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "{} (a run finished on this deployment)",
            crate::output::local_date_time(1_790_330_460)
        )),
        "{text}"
    );
    assert!(!text.contains("missing:"), "{text}");
    assert!(text.contains("`chap-devops open dhis2`"), "{text}");
}

/// A route aimed at somebody else's chap-core is the trap the whole step
/// exists for, so `show` says so in the row as well as in the list.
#[test]
fn show_marks_a_route_that_points_at_another_chap_core() {
    let report = ShowReport {
        instance: instance(),
        target: "http://chap:8000/**".to_string(),
        route: Some(ShownRoute {
            url: "http://158.39.75.126/stable/**".to_string(),
            disabled: false,
            authorised: true,
            ours: false,
            verified: false,
            answered: "it points at another chap-core".to_string(),
            token_refused: false,
            way_out: String::new(),
        }),
        last_analytics: "2026-09-25T10:01:00.000".to_string(),
        analytics: dhis2::AnalyticsEvidence::Recorded,
        apps: shown_apps(&["", ""]),
        missing: vec![
            "the `chap` route points at http://158.39.75.126/stable/**, not at this deployment"
                .to_string(),
        ],
        next: "run `chap-devops dhis2 connect` to do the rest".to_string(),
    };
    let text = human_show(&report, &out());
    assert!(text.contains("(another chap-core)"), "{text}");
    assert!(text.contains("not at this deployment"), "{text}");
}

#[test]
fn the_missing_apps_are_the_ones_the_instance_does_not_have() {
    let missing = missing_apps(&shown_apps(&["", ""]));
    assert_eq!(missing.len(), 2, "{missing:?}");
    assert!(missing[0].contains("the Modeling App"), "{missing:?}");
    assert!(missing[1].contains("the Climate App"), "{missing:?}");

    assert!(missing_apps(&shown_apps(&["7.1.0", "1.16.2"])).is_empty());

    // A row that is there but not installed is a missing app, not a
    // present one: the row exists either way.
    let missing = missing_apps(&shown_apps(&["7.1.0", ""]));
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert!(missing[0].contains("the Climate App"), "{missing:?}");

    // The instance's own spelling is matched, not compared as text: it
    // lists the Modeling App as `Modeling`, and an installed key is the
    // name in a third spelling again.
    let keyed = serde_json::json!([
        {"name": "Modeling", "key": "modeling", "version": "7.1.0"},
        {"name": "", "key": "dhis2-climate-app", "version": "1.16.2"},
    ]);
    let found: Vec<ShownApp> = dhis2::HUB_APPS
        .iter()
        .map(|app| shown_app(app, dhis2::installed_app(&keyed, app.name), None))
        .collect();
    assert!(missing_apps(&found).is_empty(), "{found:?}");
}

/// Every refusal names what is true instead and the command that changes
/// it - and none of them says "open", which is the browser's verb and not
/// what these commands do.
#[test]
fn the_refusals_name_what_is_true_instead_and_the_way_out() {
    assert!(NO_CHAP_CORE.contains("`chap-devops components enable chap-core`"));
    assert!(NOT_RUNNING.contains("`chap-devops up`"));
    assert!(NO_DHIS2.contains("`chap-devops components enable dhis2`"));
    let internal = no_host_port("http://dhis2:8080");
    assert!(internal.contains("http://dhis2:8080"), "{internal}");
    assert!(
        internal.contains("`chap-devops components enable dhis2 --port N`"),
        "{internal}"
    );
    for text in [NO_CHAP_CORE, NOT_RUNNING, NO_DHIS2, &internal] {
        assert!(!text.contains("open"), "{text}");
    }
}

#[test]
fn an_error_is_reported_on_one_line() {
    assert_eq!(first_line("one\ntwo"), "one");
    assert_eq!(first_line("  one  "), "one");
    assert_eq!(first_line(""), "");
}

/// `localhost` as the chap-core address is wrong for a DHIS2 elsewhere and
/// right for one running directly on this machine; a DHIS2 in Docker on this
/// machine needs `host.docker.internal`, which the note names.
#[test]
fn a_loopback_chap_url_is_judged_by_where_dhis2_runs() {
    let remote = loopback_note("https://dhis2.example.org", "http://localhost:8700").unwrap();
    assert!(remote.contains("give `--chap-url` the address"), "{remote}");

    let here = loopback_note("http://localhost:8080", "http://localhost:8700").unwrap();
    assert!(
        here.contains("is right for a DHIS2 running directly on this machine"),
        "{here}"
    );
    assert!(
        here.contains("`http://host.docker.internal:8700`"),
        "{here}"
    );

    assert_eq!(
        loopback_note("http://localhost:8080", "http://host.docker.internal:8700"),
        None
    );
}

/// Plain http to another machine sends the credentials in the clear, so it is
/// said; this machine, a private address and https are not.
#[test]
fn plain_http_to_another_machine_is_noted() {
    use super::use_external::cleartext_note;
    assert!(cleartext_note("http://dhis2.example.org").is_some());
    assert!(cleartext_note("http://203.0.113.7:8080/dhis").is_some());
    assert!(cleartext_note("https://dhis2.example.org").is_none());
    assert!(cleartext_note("http://localhost:8080").is_none());
    assert!(cleartext_note("http://127.0.0.1:8780").is_none());
    assert!(cleartext_note("http://192.168.1.20:8080").is_none());
    assert!(cleartext_note("http://dhis2:8080").is_none());
}

/// A failed SQL statement keeps its cause and loses the statement, and the
/// memory note is there only for a memory failure.
#[test]
fn a_failed_analytics_run_says_the_cause_and_not_the_statement() {
    use super::analytics::failure_message;
    let reason = format!(
        "StatementCallback; SQL [insert into analytics_2019_temp select ...]; ERROR: \"{}\" \
         is out of range for type double precision",
        "9".repeat(2081)
    );
    let text = failure_message(&reason, "41 seconds");
    assert_eq!(
        text,
        "the analytics run failed after 41 seconds: \"999999999999... (2081 digits)\" is out \
         of range for type double precision; a data value is a number too large for DHIS2; find \
         it in the `datavalue` table, then correct it or delete it in DHIS2"
    );
    assert!(!text.contains("insert into"), "{text}");
    assert!(!text.contains("4 to 5 GB"), "{text}");

    let memory = failure_message("java.lang.OutOfMemoryError: Java heap space", "3 minutes");
    assert!(memory.contains("4 to 5 GB"), "{memory}");
    let other = failure_message("table is locked by job abc", "2 seconds");
    assert!(
        other.ends_with("`chap-devops logs dhis2` has the rest"),
        "{other}"
    );
}

/// On an external DHIS2 the last line of `show` names the command for each
/// missing step, because `connect` sets only the route there.
#[test]
fn show_names_the_command_for_each_missing_step_on_an_external_dhis2() {
    let ready = "the Modeling App can reach Chap; open DHIS2 with `chap-devops open dhis2`";
    assert_eq!(show_next(true, true, false, true), ready);
    assert_eq!(show_next(false, true, false, true), ready);
    assert_eq!(
        show_next(false, true, true, false),
        "run `chap-devops dhis2 connect` to do the rest"
    );
    assert_eq!(
        show_next(true, true, true, true),
        "run `chap-devops dhis2 apps` for the apps; the apps and analytics change this DHIS2, so ask \
         its admin first"
    );
    assert_eq!(
        show_next(true, false, false, true),
        "run `chap-devops dhis2 connect` for the route"
    );
    let all = show_next(true, false, true, false);
    assert!(
        all.starts_with(
            "run `chap-devops dhis2 connect` for the route, then `chap-devops dhis2 apps` for the apps, \
             then `chap-devops dhis2 analytics`"
        ),
        "{all}"
    );
}

/// The last line of `connect` on an external DHIS2 names the apps its admin
/// still has to install.
#[test]
fn connect_on_an_external_dhis2_names_the_apps_to_install() {
    assert_eq!(
        external_next(true, Some(&["the Climate App"])),
        "the Modeling App can reach Chap after the admin of this DHIS2 installs the Climate \
         App; `chap-devops dhis2 show` says when they are there"
    );
    assert!(external_next(true, Some(&[])).contains("`chap-devops open dhis2`"));
    assert!(external_next(true, None).contains("says which are there"));
    assert!(external_next(false, Some(&[])).contains("`chap-devops dhis2 show`"));
}

/// `use` names the URL that moved: a change of `--chap-url` alone does not
/// claim that the DHIS2 URL changed.
#[test]
fn use_names_the_url_that_moved() {
    let external = |url: &str, chap_url: &str| crate::components::ExternalDhis2 {
        url: url.to_string(),
        chap_url: chap_url.to_string(),
        connected_at: None,
        ..Default::default()
    };
    let headline = |previous| {
        let report = UseReport {
            outcome: UseOutcome::Changed,
            external: Some(external(
                "http://localhost:8080",
                "http://host.docker.internal:8700",
            )),
            previous: Some(previous),
            target: None,
            component: false,
            probe: None,
            notes: Vec::new(),
            next: String::new(),
            next_is_hint: true,
        };
        lines(|l| use_summary(&report, l))
            .lines()
            .next()
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(
        headline(external("http://localhost:8080", "http://localhost:8700")),
        "changed the chap-url of the external DHIS2 at http://localhost:8080 to \
         http://host.docker.internal:8700 in `.chap-devops/components.yaml`"
    );
    assert_eq!(
        headline(external(
            "http://localhost:8090",
            "http://host.docker.internal:8700"
        )),
        "changed the external DHIS2 to http://localhost:8080 in `.chap-devops/components.yaml`"
    );
}

/// A proxied request that gets no answer is about chap-core behind the
/// route: the sentence does not blame DHIS2 or name its container.
#[test]
fn a_request_through_the_route_that_got_no_answer_is_not_blamed_on_dhis2() {
    let unreachable = |reason: &str| {
        anyhow::Error::from(crate::error::ChapError::Dhis2Unreachable {
            url: "http://localhost:8780".to_string(),
            reason: reason.to_string(),
            next: "run `chap-devops status` to see whether its container is up",
        })
    };
    let timeout = proxied_failure(&unreachable("timeout: global"));
    assert_eq!(timeout, "the request through DHIS2 timed out");
    let refused = proxied_failure(&unreachable("io: Connection refused"));
    assert_eq!(
        refused,
        "the request through DHIS2 failed: io: Connection refused"
    );
    for text in [timeout, refused] {
        assert!(!text.contains("is not responding"), "{text}");
        assert!(!text.contains("chap-devops status"), "{text}");
    }
    let other = proxied_failure(&anyhow::anyhow!("something else"));
    assert_eq!(other, "something else");
}

/// Without chap-core, `show` names the commands that work there and the one
/// that adds chap-core, never `chap-devops dhis2 connect`, which refuses.
#[test]
fn show_without_chap_core_does_not_name_connect() {
    let route =
        "the `chap` route needs chap-core; `chap-devops components enable chap-core` adds it";
    assert_eq!(no_chap_core_next(false, true), route);
    assert_eq!(
        no_chap_core_next(true, false),
        format!(
            "run `chap-devops dhis2 apps` for the apps, then `chap-devops dhis2 analytics` for the \
             analytics tables; {route}"
        )
    );
    for next in [
        no_chap_core_next(true, true),
        no_chap_core_next(false, false),
    ] {
        assert!(!next.contains("connect"), "{next}");
        assert!(
            next.contains("`chap-devops components enable chap-core`"),
            "{next}"
        );
    }
}

/// A URL with a login in it would be stored in `.chap-devops/` and traced.
/// The refusal does not quote it back.
#[test]
fn a_url_with_a_login_in_it_is_refused() {
    use super::use_external::external_url;
    let err = external_url("https://admin:s3cret@dhis2.example.org/")
        .unwrap_err()
        .to_string();
    assert!(err.contains("before an `@`"), "{err}");
    assert!(!err.contains("s3cret"), "{err}");
    assert_eq!(
        external_url("https://dhis2.example.org/").unwrap(),
        "https://dhis2.example.org"
    );
    // An `@` in the path is not a login.
    assert!(external_url("https://dhis2.example.org/a@b").is_ok());
}
