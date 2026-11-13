use super::analytics::{progress_of, recorded_success, server_offset, success_as_unix};
use super::apps::{HubApp, HubVersion};
use super::credentials::{CredentialInputs, credentials_of};
use super::route::{ROUTE_ADD_AUTHORITY, Route, RouteAuth, route_origin};
use super::*;
use crate::api::Answer;
use crate::error::ChapError;

fn answer(status: u16, body: &str) -> Answer {
    Answer {
        status,
        reason: "Conflict".to_string(),
        content_type: crate::api::JSON.to_string(),
        body: body.as_bytes().to_vec(),
        wire_bytes: body.len(),
    }
}

fn route(url: &str) -> Route {
    Route {
        id: "abc123".to_string(),
        name: ROUTE_NAME.to_string(),
        code: ROUTE_CODE.to_string(),
        url: url.to_string(),
        disabled: false,
        authorities: vec![ROUTE_AUTHORITY.to_string()],
        auth: None,
    }
}

#[test]
fn base64_is_the_standard_encoding_with_padding() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foob"), "Zm9vYg==");
    assert_eq!(base64(b"fooba"), "Zm9vYmE=");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    // The header every one of these commands sends.
    assert_eq!(base64(b"admin:district"), "YWRtaW46ZGlzdHJpY3Q=");
}

#[test]
fn the_authorization_header_is_basic_and_the_password_is_nowhere_else() {
    let credentials = Credentials::new("admin", "district", CredentialSource::Default);
    assert_eq!(credentials.header(), "Basic YWRtaW46ZGlzdHJpY3Q=");
    // Debug is written by hand precisely so this holds.
    let shown = format!("{credentials:?}");
    assert!(!shown.contains("district"), "{shown}");
    assert!(shown.contains("admin"), "{shown}");
    assert!(shown.contains("<hidden>"), "{shown}");
    assert!(!credentials.traced().contains("district"));
}

/// DHIS2's scheme for a personal access token, and the token nowhere a
/// trace line or a panic could carry it.
#[test]
fn a_token_goes_out_as_apitoken_and_nowhere_else() {
    let credentials = Credentials::token("d2p_sekret", CredentialSource::EnvFile);
    assert_eq!(credentials.header(), "ApiToken d2p_sekret");
    assert_eq!(credentials.kind(), AuthKind::Token);
    assert_eq!(credentials.user(), None);
    for shown in [format!("{credentials:?}"), credentials.traced()] {
        assert!(!shown.contains("sekret"), "{shown}");
    }
    assert_eq!(credentials.describe(), "API token from `.env`");
}

/// A mistyped token is a 400 `Checksum validation failed`, not a 401, and
/// it gets the same way out as a revoked one. A 400 about anything else is
/// not a token problem.
#[test]
fn a_malformed_token_gets_the_token_message_and_its_way_out() {
    let client = Dhis2::new(
        "http://localhost:8780",
        Credentials::token("d2p_notreal", CredentialSource::Environment),
        DEFAULT_TIMEOUT,
    );
    let checksum = answer(
        400,
        r#"{"httpStatus":"Bad Request","httpStatusCode":400,"status":"ERROR","message":"Checksum validation failed"}"#,
    );
    let text = client.refusal(&checksum).expect("a token message");
    assert!(text.contains("did not accept the API token"), "{text}");
    assert!(text.contains("Checksum validation failed"), "{text}");
    assert!(text.contains("`DHIS2_API_TOKEN` in `.env`"), "{text}");
    assert!(text.contains("`CHAP_DEVOPS_DHIS2_TOKEN`"), "{text}");
    assert!(!text.contains("notreal"), "{text}");

    let other = answer(400, r#"{"message":"Invalid filter"}"#);
    assert_eq!(client.refusal(&other), None);
    let password = Dhis2::new(
        "http://localhost:8780",
        Credentials::new("admin", "district", CredentialSource::Default),
        DEFAULT_TIMEOUT,
    );
    assert_eq!(password.refusal(&checksum), None);
}

/// Only the environment half of the inputs, over `body`.
fn inputs<'a>(body: &'a str) -> CredentialInputs<'a> {
    CredentialInputs {
        env_body: body,
        deployed: true,
        ..CredentialInputs::default()
    }
}

fn resolved(inputs: CredentialInputs) -> Credentials {
    credentials_of(&inputs).expect("credentials")
}

#[test]
fn credentials_come_from_env_then_the_environment_then_the_default() {
    // Nothing anywhere: the DHIS2 default, which a seeded dump and an
    // empty database both give.
    let plain = resolved(inputs(""));
    assert_eq!(plain.user(), Some("admin"));
    assert_eq!(plain.source, CredentialSource::Default);
    assert_eq!(plain.header(), "Basic YWRtaW46ZGlzdHJpY3Q=");

    // The environment, for an operator who will not have it on disk.
    let exported = resolved(CredentialInputs {
        env_password: Some("sekret"),
        ..inputs("")
    });
    assert_eq!(exported.source, CredentialSource::Environment);
    assert_eq!(exported.user(), Some("admin"));

    // `.env` is the deployment's own answer and wins, the way
    // `api::token_for` reads the chap-core token.
    let body = "DHIS2_ADMIN_USERNAME=ops\nDHIS2_ADMIN_PASSWORD=from-the-file\n";
    let from_file = resolved(CredentialInputs {
        env_password: Some("sekret"),
        ..inputs(body)
    });
    assert_eq!(from_file.user(), Some("ops"));
    assert_eq!(from_file.source, CredentialSource::EnvFile);

    // A commented placeholder sets nothing, and neither does an empty one.
    let commented = resolved(inputs("# DHIS2_ADMIN_PASSWORD=district\n"));
    assert_eq!(commented.source, CredentialSource::Default);
    let empty = resolved(CredentialInputs {
        env_password: Some(" "),
        ..inputs("DHIS2_ADMIN_PASSWORD=\n")
    });
    assert_eq!(empty.source, CredentialSource::Default);
}

/// The bug `--user` used to have: it replaced the username and kept the
/// password, so `--user alice` sent admin's password in alice's name.
#[test]
fn a_password_is_only_ever_sent_as_the_user_it_belongs_to() {
    let body = "DHIS2_ADMIN_USERNAME=ops\nDHIS2_ADMIN_PASSWORD=from-the-file\n";

    // `.env`'s password is `ops`'s, so `--user ops` gets it...
    let ops = resolved(CredentialInputs {
        user: Some("ops"),
        ..inputs(body)
    });
    assert_eq!(ops.user(), Some("ops"));
    assert_eq!(ops.source, CredentialSource::EnvFile);

    // ...and `--user alice` does not, and nothing else is found for her.
    let err = credentials_of(&CredentialInputs {
        user: Some("alice"),
        ..inputs(body)
    })
    .unwrap_err()
    .to_string();
    assert!(err.contains("no password for DHIS2 user `alice`"), "{err}");
    assert!(err.contains("CHAP_DEVOPS_DHIS2_PASSWORD"), "{err}");
    assert!(!err.contains("from-the-file"), "{err}");

    // An exported password with no owner named is the one for this run's
    // user, which is how a one-off run as someone else works.
    let alice = resolved(CredentialInputs {
        user: Some("alice"),
        env_password: Some("hers"),
        ..inputs(body)
    });
    assert_eq!(alice.user(), Some("alice"));
    assert_eq!(alice.source, CredentialSource::Environment);

    // Unless CHAP_DEVOPS_DHIS2_USERNAME gives it to somebody else.
    assert!(
        credentials_of(&CredentialInputs {
            user: Some("alice"),
            env_username: Some("bob"),
            env_password: Some("his"),
            ..inputs(body)
        })
        .is_err()
    );

    // The default is admin's, on a DHIS2 chap-devops deployed.
    let admin = resolved(CredentialInputs {
        user: Some("admin"),
        ..inputs(body)
    });
    assert_eq!(admin.source, CredentialSource::Default);

    // A blank `--user` is no `--user` at all.
    let blank = resolved(CredentialInputs {
        user: Some("  "),
        ..inputs(body)
    });
    assert_eq!(blank.user(), Some("ops"));
}

#[test]
fn the_environment_pair_names_its_own_user() {
    let pair = resolved(CredentialInputs {
        env_username: Some("bob"),
        env_password: Some("his"),
        ..inputs("")
    });
    assert_eq!(pair.user(), Some("bob"));
    assert_eq!(pair.source, CredentialSource::Environment);

    // `.env` naming a user with no password, and nothing else: that user
    // has no password chap-devops knows, and the default is admin's, not theirs.
    let err = credentials_of(&inputs("DHIS2_ADMIN_USERNAME=ops\n"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("no password for DHIS2 user `ops`"), "{err}");
    assert!(err.contains("DHIS2_ADMIN_PASSWORD"), "{err}");
}

#[test]
fn a_token_wins_within_its_source_and_loses_to_an_earlier_one() {
    let token = resolved(inputs(
        "DHIS2_API_TOKEN=d2p_file\nDHIS2_ADMIN_PASSWORD=pw\n",
    ));
    assert_eq!(token.header(), "ApiToken d2p_file");
    assert_eq!(token.source, CredentialSource::EnvFile);

    // `.env`'s password is an earlier source than an exported token.
    let file_password = resolved(CredentialInputs {
        env_token: Some("d2p_env"),
        ..inputs("DHIS2_ADMIN_PASSWORD=pw\n")
    });
    assert_eq!(file_password.kind(), AuthKind::Basic);

    // An exported token beats an exported password and the default.
    let env_token = resolved(CredentialInputs {
        env_token: Some("d2p_env"),
        env_password: Some("pw"),
        ..inputs("")
    });
    assert_eq!(env_token.header(), "ApiToken d2p_env");
    assert_eq!(
        env_token.describe(),
        "API token from CHAP_DEVOPS_DHIS2_TOKEN"
    );

    // `--user` asks for a user by name, so it is a password even with a
    // token on offer.
    let flagged = resolved(CredentialInputs {
        user: Some("admin"),
        ..inputs("DHIS2_API_TOKEN=d2p_file\n")
    });
    assert_eq!(flagged.kind(), AuthKind::Basic);
}

/// chap-devops did not create an external DHIS2, so `admin` / `district` is
/// nobody's password there, and is never sent.
#[test]
fn an_external_dhis2_gets_no_default() {
    let external = |inputs: CredentialInputs| {
        credentials_of(&CredentialInputs {
            deployed: false,
            ..inputs
        })
    };
    let err = external(inputs("")).unwrap_err().to_string();
    assert!(err.contains("did not deploy it"), "{err}");
    assert!(err.contains("DHIS2_API_TOKEN"), "{err}");
    assert!(
        err.contains("`DHIS2_ADMIN_USERNAME` and `DHIS2_ADMIN_PASSWORD`"),
        "{err}"
    );
    assert!(
        external(CredentialInputs {
            user: Some("admin"),
            ..inputs("")
        })
        .is_err()
    );
    // What is set is used as anywhere else.
    let token = external(inputs("DHIS2_API_TOKEN=d2p_file\n")).unwrap();
    assert_eq!(token.kind(), AuthKind::Token);
}

#[test]
fn every_credential_says_what_it_is_and_where_it_came_from() {
    for kind in [AuthKind::Basic, AuthKind::Token] {
        for source in [
            CredentialSource::EnvFile,
            CredentialSource::Environment,
            CredentialSource::Default,
        ] {
            assert!(!describe_credential(kind, source).is_empty());
        }
    }
    assert_eq!(
        describe_credential(AuthKind::Basic, CredentialSource::EnvFile),
        "password from `.env`"
    );
}

/// The target is the compose alias and the `/**` suffix, never localhost:
/// the requests are made by DHIS2, from inside the deployment.
#[test]
fn the_route_target_is_the_service_alias_and_a_wildcard() {
    assert_eq!(route_target(""), "http://chap:8000/**");
    assert!(!route_target("").contains("localhost"));
    // A deployment behind a proxy moves every chap-core route with
    // CHAP_ROOT_PATH, this one included.
    assert_eq!(route_target("/master"), "http://chap:8000/master/**");
    assert_eq!(route_target("/master/"), "http://chap:8000/master/**");
}

#[test]
fn the_route_payload_is_the_one_that_works() {
    let payload: serde_json::Value =
        serde_json::from_str(&route_payload("http://chap:8000/**", None))
            .expect("the payload is JSON");
    assert_eq!(payload["code"], "chap");
    assert_eq!(payload["name"], ROUTE_NAME);
    assert_eq!(payload["url"], "http://chap:8000/**");
    assert_eq!(payload["authorities"][0], ROUTE_AUTHORITY);
    assert_eq!(payload["headers"]["Content-Type"], "application/json");
    assert_eq!(payload["responseTimeoutSeconds"], 30);
}

/// chap-core's token rides in `auth`, which DHIS2 keeps to itself, and
/// never in `headers`, which it lists back to anyone who can read the route.
#[test]
fn the_token_goes_in_the_route_auth_and_nowhere_else() {
    let payload: serde_json::Value =
        serde_json::from_str(&route_payload("http://chap:8000/**", Some("s3cret")))
            .expect("the payload is JSON");
    assert_eq!(payload["auth"]["type"], ROUTE_AUTH_TYPE);
    assert_eq!(payload["auth"]["headers"]["Authorization"], "Bearer s3cret");
    assert!(payload["headers"].get("Authorization").is_none());
    assert!(!payload["headers"].to_string().contains("s3cret"));

    let open: serde_json::Value = serde_json::from_str(&route_payload("http://chap:8000/**", None))
        .expect("the payload is JSON");
    assert!(open.get("auth").is_none());
}

/// A route that reaches `/health` and nothing else is what a tokenless
/// route in front of a token-protected chap-core is, so it is rewritten.
#[test]
fn a_route_without_the_token_is_rewritten_when_chap_core_wants_one() {
    let target = route_target("");
    let bare = route(&target);
    let RouteAction::Rewrite(reasons) = route_action(Some(&bare), &target, true) else {
        panic!("a tokenless route in front of a token has to be rewritten");
    };
    assert_eq!(reasons, vec!["it carried no chap-core API token"]);

    let mut carrying = route(&target);
    carrying.auth = Some(RouteAuth {
        kind: ROUTE_AUTH_TYPE.to_string(),
    });
    assert_eq!(
        route_action(Some(&carrying), &target, true),
        RouteAction::Keep
    );
    // Without a token the auth is nobody's business.
    assert_eq!(route_action(Some(&bare), &target, false), RouteAction::Keep);
}

/// DHIS2 lists the type and holds the secret back.
#[test]
fn the_listed_auth_is_read_as_its_type() {
    let listing = serde_json::json!({"routes": [
        {"id": "two", "code": "chap", "url": "http://chap:8000/**",
         "authorities": ["F_CHAP_MODELING_APP"], "auth": {"type": "api-headers"}},
    ]});
    let found = route_in(&listing).expect("the chap route");
    assert_eq!(
        found.auth.map(|auth| auth.kind).as_deref(),
        Some(ROUTE_AUTH_TYPE)
    );
}

/// The whole point of the step: the demo dumps ship this route pointed at
/// somebody else's Chap, so a missing route and a wrong one are both work.
#[test]
fn a_route_pointing_elsewhere_is_repointed_and_not_skipped() {
    let target = route_target("");
    let external = route("http://158.39.75.126/stable/**");
    let RouteAction::Rewrite(reasons) = route_action(Some(&external), &target, false) else {
        panic!("a route aimed elsewhere has to be repointed");
    };
    assert_eq!(
        reasons,
        vec!["it pointed at http://158.39.75.126/stable/**"]
    );
}

#[test]
fn a_route_that_already_matches_is_left_alone() {
    let target = route_target("");
    assert_eq!(
        route_action(Some(&route(&target)), &target, false),
        RouteAction::Keep
    );
    assert_eq!(route_action(None, &target, false), RouteAction::Create);
}

/// The URL is the point and not the whole of it: a route aimed correctly
/// but switched off proxies nothing.
#[test]
fn a_disabled_or_unauthorised_route_is_repaired_with_its_reason() {
    let target = route_target("");
    let mut off = route(&target);
    off.disabled = true;
    let RouteAction::Rewrite(reasons) = route_action(Some(&off), &target, false) else {
        panic!("a disabled route has to be rewritten");
    };
    assert_eq!(reasons, vec!["it was disabled"]);

    let mut bare = route(&target);
    bare.authorities.clear();
    let RouteAction::Rewrite(reasons) = route_action(Some(&bare), &target, false) else {
        panic!("a route without the authority has to be rewritten");
    };
    assert_eq!(
        reasons,
        vec!["it was missing the F_CHAP_MODELING_APP authority"]
    );

    // All three at once, in the order they are checked.
    let mut every = route("http://elsewhere/**");
    every.disabled = true;
    every.authorities.clear();
    let RouteAction::Rewrite(reasons) = route_action(Some(&every), &target, false) else {
        panic!("three reasons is still a rewrite");
    };
    assert_eq!(reasons.len(), 3);
}

#[test]
fn the_route_is_found_by_code_in_either_listing_shape() {
    let listing = serde_json::json!({"routes": [
        {"id": "one", "code": "other", "url": "http://x/**"},
        {"id": "two", "code": "chap", "url": "http://chap:8000/**",
         "authorities": ["F_CHAP_MODELING_APP"]},
    ]});
    let found = route_in(&listing).expect("the chap route");
    assert_eq!(found.id, "two");
    assert_eq!(found.url, "http://chap:8000/**");
    assert!(!found.disabled, "absent means not disabled");

    // A bare array answers too, and a listing without the code answers
    // nothing rather than the first row.
    let bare = serde_json::json!([{"id": "two", "code": "chap", "url": "u"}]);
    assert_eq!(route_in(&bare).expect("the chap route").id, "two");
    assert_eq!(route_in(&serde_json::json!({"routes": []})), None);
    assert_eq!(route_in(&serde_json::json!("nope")), None);
}

#[test]
fn the_run_path_is_the_one_the_app_itself_uses() {
    assert_eq!(route_run_path("/health"), "/api/routes/chap/run/health");
    assert_eq!(
        route_run_path("system/info"),
        "/api/routes/chap/run/system/info"
    );
}

/// The body a live DHIS2 2.42.6 answers a refused route write with, exactly
/// as it comes off the wire.
const REFUSED_ROUTE_WRITE: &str = r#"{"httpStatus":"Conflict","httpStatusCode":409,"status":"ERROR","message":"Route URL is not permitted","errorCode":"E1004"}"#;

#[test]
fn the_allowlist_refusal_names_the_file_and_the_way_out() {
    let text = allowlist_hint("http://chap:8000/**", true);
    assert!(text.contains("`dhis2/dhis.conf`"), "{text}");
    assert!(text.contains("route.remote_servers_allowed"), "{text}");
    // A plain restart applies it: `restart` recreates a service whose
    // mounted config changed, which compose alone would not.
    assert!(text.contains("run `chap-devops restart dhis2`"), "{text}");
    assert!(!text.contains("--all"), "{text}");
}

/// DHIS2 will not start with a path in `route.remote_servers_allowed`
/// (`Allowed route URL must not have a path`, measured on 2.42.6), so the
/// refusal names the target's origin and never the `/**` target itself.
#[test]
fn the_allowlist_refusal_names_the_origin_without_a_path() {
    let external = allowlist_hint("http://localhost:8700/**", false);
    assert!(
        external.contains("and http://localhost:8700 has to be"),
        "{external}"
    );
    assert!(!external.contains("/**"), "{external}");
    assert!(external.contains("with no path"), "{external}");

    let deployed = allowlist_hint("http://chap:8000/**", true);
    assert!(
        deployed.contains("and http://chap:8000 has to be"),
        "{deployed}"
    );
    assert_eq!(
        route_origin("https://chap.example.org/api/**"),
        "https://chap.example.org"
    );
    assert_eq!(route_origin("http://chap:8000"), "http://chap:8000");
}

/// Measured, not guessed. The three phrases this used to look for -
/// `remote server`, `not allowed`, `allowlist` - appear in no DHIS2 answer
/// that has ever shipped, so the branch never fired and the operator got a
/// passthrough naming neither the cause nor the file.
#[test]
fn the_refusal_is_recognised_from_what_dhis2_actually_says() {
    assert!(is_allowlist_refusal(&answer(409, REFUSED_ROUTE_WRITE)));
    // 2.43 and later append a sentence to the same refusal.
    assert!(is_allowlist_refusal(&answer(
        409,
        r#"{"message":"Route URL is not permitted. Ask your DHIS2 server administrator to allow it","errorCode":"E1004"}"#
    )));

    // And `errorCode` is why the message is the key rather than the code:
    // DHIS2 gives `E1004` - `API query cannot be performed` - to every
    // plain `ConflictException`, these four included, all four raised by
    // `validateRoute` beside the allowlist check itself. Keying on the code
    // would send all of them to `dhis.conf`.
    for other in [
        "Malformed route URL",
        "Route URL scheme must be either http or https",
        "Placeholders are only permitted in the route URL path or query",
        "Route response timeout must be greater than 0 seconds and less than or equal to 60 \
             seconds",
    ] {
        let body = serde_json::json!({
            "httpStatus": "Conflict",
            "httpStatusCode": 409,
            "status": "ERROR",
            "message": other,
            "errorCode": "E1004",
        })
        .to_string();
        assert!(!is_allowlist_refusal(&answer(409, &body)), "{other}");
    }

    assert!(!is_allowlist_refusal(&answer(
        409,
        r#"{"message":"Route with code chap already exists"}"#
    )));
    assert!(!is_allowlist_refusal(&answer(404, "{}")));
}

#[test]
fn a_dhis2_refusal_names_the_user_and_never_the_password() {
    let dhis2 = Dhis2::new(
        "http://localhost:18080",
        Credentials::new("admin", "district", CredentialSource::Default),
        DEFAULT_TIMEOUT,
    );
    let text = dhis2.refusal(&answer(401, "{}")).expect("a 401 sentence");
    assert!(
        text.contains("did not accept the password for `admin`"),
        "{text}"
    );
    assert!(text.contains("DHIS2_ADMIN_PASSWORD"), "{text}");
    assert!(text.contains("CHAP_DEVOPS_DHIS2_PASSWORD"), "{text}");
    assert!(text.contains("the DHIS2 default"), "{text}");
    assert!(!text.contains("district"), "{text}");

    let text = dhis2.refusal(&answer(403, "{}")).expect("a 403 sentence");
    assert!(text.contains("--user NAME"), "{text}");
    assert!(!text.contains("district"), "{text}");

    // A refused route write says DHIS2's reason and the authority it needs:
    // the Sierra Leone demo's `admin` is not a superuser and lacks it.
    let body = r#"{"httpStatusCode":403,"message":"You don't have the proper permissions to create this object.","errorCode":"E1006"}"#;
    let text = dhis2.route_refusal(&answer(403, body));
    assert!(text.contains("`chap` route as `admin`"), "{text}");
    assert!(text.contains("proper permissions"), "{text}");
    assert!(text.contains(ROUTE_ADD_AUTHORITY), "{text}");
    assert!(text.contains("--user NAME"), "{text}");

    // A token is refused as a token, with the two places a current one
    // goes, and never quoted.
    let dhis2 = Dhis2::new(
        "http://localhost:18080",
        Credentials::token("d2p_sekret", CredentialSource::EnvFile),
        DEFAULT_TIMEOUT,
    );
    let text = dhis2.refusal(&answer(401, "{}")).expect("a 401 sentence");
    assert!(text.contains("did not accept the API token"), "{text}");
    assert!(text.contains("DHIS2_API_TOKEN"), "{text}");
    assert!(text.contains("CHAP_DEVOPS_DHIS2_TOKEN"), "{text}");
    assert!(!text.contains("sekret"), "{text}");
    let text = dhis2.refusal(&answer(403, "{}")).expect("a 403 sentence");
    assert!(text.contains("as the token's user"), "{text}");

    assert_eq!(dhis2.refusal(&answer(404, "{}")), None);
    // And the generic error keeps DHIS2's own message.
    let err = dhis2.status_error(ROUTES_PATH, &answer(409, r#"{"message":"nope"}"#));
    assert!(err.to_string().contains("nope"), "{err}");
    assert!(err.to_string().contains("HTTP 409"), "{err}");
}

#[test]
fn what_dhis2_said_is_its_web_message() {
    assert_eq!(
        said(&answer(409, r#"{"message":"Already exists"}"#)),
        "Already exists"
    );
    assert_eq!(said(&answer(500, r#"{"devMessage":"NPE"}"#)), "NPE");
    assert_eq!(said(&answer(404, "  Not Found  ")), "Not Found");
    // An empty document says nothing, so the status line stands alone.
    assert_eq!(said(&answer(404, "{}")), "");
    assert_eq!(said(&answer(404, " [] ")), "");
    let long = answer(500, &"x".repeat(400));
    assert!(said(&long).ends_with("..."));
    assert_eq!(said(&long).chars().count(), 203);
}

/// The analytics endpoint must never carry `lastYears`: measured, it wrote
/// zero rows into every table and called it a success.
#[test]
fn the_analytics_request_skips_tracked_entities_and_nothing_else() {
    assert_eq!(
        ANALYTICS_PATH,
        "/api/resourceTables/analytics?skipTrackedEntities=true"
    );
    assert!(!ANALYTICS_PATH.contains("lastYears"));
}

#[test]
fn a_jobs_progress_is_read_off_its_notifications() {
    let running = serde_json::json!([
        {"time": "2026-09-25T10:00:00.000", "level": "INFO", "message": "started",
         "completed": false},
        {"time": "2026-09-25T10:00:30.000", "level": "INFO", "message": "analytics_2024",
         "completed": false},
    ]);
    assert_eq!(
        progress_of(&running),
        Some(Progress::Running("analytics_2024".to_string()))
    );

    // The latest is by `time`, not by position: the two ends of the array
    // have swapped between DHIS2 versions.
    let reversed = serde_json::json!([
        {"time": "2026-09-25T10:00:30.000", "message": "analytics_2024"},
        {"time": "2026-09-25T10:00:00.000", "message": "started"},
    ]);
    assert_eq!(
        progress_of(&reversed),
        Some(Progress::Running("analytics_2024".to_string()))
    );

    let done = serde_json::json!([
        {"time": "2026-09-25T10:00:00.000", "message": "started", "completed": false},
        {"time": "2026-09-25T10:01:00.000", "message": "Analytics tables updated",
         "completed": true},
    ]);
    assert_eq!(
        progress_of(&done),
        Some(Progress::Done("Analytics tables updated".to_string()))
    );

    // A failure announces both, and the error is what matters.
    let failed = serde_json::json!([
        {"time": "2026-09-25T10:00:00.000", "message": "started"},
        {"time": "2026-09-25T10:00:10.000", "level": "ERROR", "message": "out of memory"},
        {"time": "2026-09-25T10:00:11.000", "message": "done", "completed": true},
    ]);
    assert_eq!(
        progress_of(&failed),
        Some(Progress::Failed("out of memory".to_string()))
    );

    // An empty notifier is a job queued behind another one, which is not
    // the same thing as a job that has finished.
    assert_eq!(progress_of(&serde_json::json!([])), None);
    assert_eq!(progress_of(&serde_json::json!({})), None);
}

/// DHIS2 runs one analytics job at a time, so the one already going is
/// what to watch: a second POST queues and its notifier stays empty.
#[test]
fn the_running_job_is_the_one_to_watch() {
    let tasks = serde_json::json!({
        "old": [{"time": "2026-09-25T09:00:00.000", "message": "done", "completed": true}],
        "now": [{"time": "2026-09-25T10:00:00.000", "message": "working"}],
    });
    assert_eq!(running_job(&tasks).as_deref(), Some("now"));

    // Two running at once cannot happen, but the newer one is the answer.
    let two = serde_json::json!({
        "a": [{"time": "2026-09-25T09:00:00.000", "message": "working"}],
        "b": [{"time": "2026-09-25T10:00:00.000", "message": "working"}],
    });
    assert_eq!(running_job(&two).as_deref(), Some("b"));

    // Nothing running, and nothing at all.
    let finished = serde_json::json!({
        "old": [{"time": "2026-09-25T09:00:00.000", "message": "done", "completed": true}],
    });
    assert_eq!(running_job(&finished), None);
    assert_eq!(running_job(&serde_json::json!({})), None);
}

#[test]
fn the_job_id_is_read_in_every_spelling_that_web_message_has_had() {
    assert_eq!(
        job_id_of(&serde_json::json!({"response": {"id": "abc"}})).as_deref(),
        Some("abc")
    );
    assert_eq!(
        job_id_of(&serde_json::json!({"response": {
            "relativeNotifierEndpoint": "/api/system/tasks/ANALYTICS_TABLE/xyz"
        }}))
        .as_deref(),
        Some("xyz")
    );
    assert_eq!(
        job_id_of(&serde_json::json!({"id": "top"})).as_deref(),
        Some("top")
    );
    assert_eq!(job_id_of(&serde_json::json!({"status": "OK"})), None);
    assert_eq!(
        job_id_of(&serde_json::json!({"response": {"id": " "}})),
        None
    );
}

#[test]
fn a_version_is_compared_part_by_part() {
    assert_eq!(version_parts("7.1.0"), vec![7, 1, 0]);
    assert_eq!(version_parts("v1.16.2"), vec![1, 16, 2]);
    assert_eq!(version_parts("1.16.2-rc1"), vec![1, 16, 2]);
    // An instance that did not say has no version, which is not version 0.
    assert_eq!(version_parts(""), Vec::<u64>::new());
    assert_eq!(version_parts("unknown"), Vec::<u64>::new());
    assert_eq!(compare_versions("7.1.0", "7.2.0"), std::cmp::Ordering::Less);
    assert_eq!(compare_versions("7.2", "7.2.0"), std::cmp::Ordering::Equal);
    assert_eq!(
        compare_versions("10.0.0", "9.9.9"),
        std::cmp::Ordering::Greater
    );
}

/// The same DHIS2 release is written `2.42` and `42`, and comparing the
/// two spellings as they stand would make every app look incompatible.
#[test]
fn a_dhis2_version_is_the_same_whichever_way_it_is_spelled() {
    assert_eq!(dhis_version_parts("2.42.6"), vec![42, 6]);
    assert_eq!(dhis_version_parts("2.40"), vec![40]);
    assert_eq!(dhis_version_parts("42"), vec![42]);
    assert_eq!(dhis_version_parts("42.1"), vec![42, 1]);
    // Nothing to strip: a bare `2` stays a 2.
    assert_eq!(dhis_version_parts("2"), vec![2]);
}

#[test]
fn the_newest_version_the_instance_can_run_is_the_one_picked() {
    let versions = vec![
        HubVersion {
            id: "old".into(),
            version: "6.0.0".into(),
            min_dhis_version: Some("2.38".into()),
            max_dhis_version: None,
        },
        HubVersion {
            id: "new".into(),
            version: "7.1.0".into(),
            min_dhis_version: Some("2.40".into()),
            max_dhis_version: None,
        },
        HubVersion {
            id: "future".into(),
            version: "8.0.0".into(),
            min_dhis_version: Some("2.43".into()),
            max_dhis_version: None,
        },
    ];
    assert_eq!(pick_version(&versions, "2.42.6").unwrap().id, "new");
    assert_eq!(pick_version(&versions, "2.38.1").unwrap().id, "old");
    assert_eq!(pick_version(&versions, "2.43").unwrap().id, "future");
    // An instance that did not say its version takes the newest rather
    // than nothing at all.
    assert_eq!(pick_version(&versions, "").unwrap().id, "future");
    // A version with no installable id is not a candidate.
    assert_eq!(pick_version(&[HubVersion::default()], "2.42"), None);
    assert_eq!(pick_version(&[], "2.42"), None);

    // An upper bound is honoured where the App Hub sets one.
    let capped = vec![HubVersion {
        id: "capped".into(),
        version: "1.0.0".into(),
        min_dhis_version: None,
        max_dhis_version: Some("2.41".into()),
    }];
    assert_eq!(pick_version(&capped, "2.42"), None);
    assert_eq!(pick_version(&capped, "2.41").unwrap().id, "capped");
}

/// A bound the App Hub has not set arrives as `""`, which is not a bound.
///
/// Read as a version it has no parts, and an instance compares above it -
/// so a blank maximum used to exclude every instance there is.
#[test]
fn a_blank_bound_is_no_bound_on_either_side() {
    let blank = |min: &str, max: &str| {
        vec![HubVersion {
            id: "only".into(),
            version: "7.1.0".into(),
            min_dhis_version: Some(min.to_string()),
            max_dhis_version: Some(max.to_string()),
        }]
    };
    // The shape the App Hub actually sends: a minimum, and a maximum that
    // is an empty string rather than an absent field.
    assert_eq!(
        pick_version(&blank("2.40", ""), "2.42.6").unwrap().id,
        "only"
    );
    // Both blank, whitespace-only, and text with no digits in it: chap-devops
    // cannot compare against any of them, so none of them is a bound.
    assert_eq!(pick_version(&blank("", ""), "2.42.6").unwrap().id, "only");
    assert_eq!(
        pick_version(&blank("  ", " \t"), "2.42.6").unwrap().id,
        "only"
    );
    assert_eq!(
        pick_version(&blank("none", "none"), "2.42.6").unwrap().id,
        "only"
    );
    // A blank minimum is the same rule: it never excludes an old instance.
    assert_eq!(pick_version(&blank("", "2.43"), "2.30").unwrap().id, "only");
    // And a bound that is set is still a bound.
    assert_eq!(pick_version(&blank("2.43", ""), "2.42.6"), None);
    assert_eq!(pick_version(&blank("", "2.41"), "2.42.6"), None);
}

/// The App Hub's real answer for the Modeling App, trimmed to three of its
/// thirty-two versions and to a one-line description, and otherwise exactly
/// what `apps.dhis2.org` returned for
/// `a29851f9-82a7-4ecd-8b2c-58e0f220bc75`.
///
/// **The empty `maxDhisVersion` strings are the point.** A stub that leaves
/// the field out is tidier than reality and hides the one bug that stopped
/// any app from ever being installed, so the regression test is written
/// against the payload as it comes off the wire.
const REAL_MODELING_APP: &str = r#"{
      "appType": "APP",
      "status": "APPROVED",
      "id": "a29851f9-82a7-4ecd-8b2c-58e0f220bc75",
      "created": 1738081755106,
      "lastUpdated": 1789983819971,
      "name": "Modeling",
      "description": "Modeling App provides a general user interface in DHIS2.",
      "hasChangelog": true,
      "hasPlugin": null,
      "pluginType": null,
      "coreApp": false,
      "developer": {"address": "", "email": "herman@dhis2.org",
                    "organisation": "DHIS2", "organisation_slug": "dhis2"},
      "owner": "4d8eae29-d2a2-4dfa-ad94-ed0746129b32",
      "images": [],
      "sourceUrl": "https://github.com/dhis2-chap/chap-frontend",
      "reviews": [],
      "userCanEditApp": false,
      "versions": [
        {"created": 1789983819971, "demoUrl": "", "changeSummary": "",
         "downloadUrl": "https://apps.dhis2.org/api/v1/apps/download/dhis2/modeling_7.1.0.zip",
         "id": "4c895dd5-e121-43ab-af44-c853c3ea3297", "lastUpdated": 1789983819971,
         "maxDhisVersion": "", "minDhisVersion": "2.40", "version": "7.1.0",
         "channel": "stable"},
        {"created": 1756467119465, "demoUrl": "", "changeSummary": "",
         "downloadUrl": "https://apps.dhis2.org/api/v1/apps/download/dhis2/modeling_2.2.4.zip",
         "id": "eac4cb12-a1f7-4351-8ba7-e12361b9d917", "lastUpdated": 1756467119465,
         "maxDhisVersion": "", "minDhisVersion": "2.40", "version": "2.2.4",
         "channel": "stable"},
        {"created": 1738081755106, "demoUrl": "", "changeSummary": "",
         "downloadUrl": "https://apps.dhis2.org/api/v1/apps/download/dhis2/modeling_1.0.2.zip",
         "id": "0fc03933-54cd-4fe3-9285-59ffec6f6c09", "lastUpdated": 1738081755106,
         "maxDhisVersion": "", "minDhisVersion": "2.41", "version": "1.0.2",
         "channel": "stable"}
      ]
    }"#;

/// The headline capability, against the payload that broke it: DHIS2
/// 2.42.6 can run the Modeling App, and chap-devops has to say which version.
#[test]
fn the_app_hubs_own_answer_yields_a_version_for_a_real_instance() {
    let app: HubApp =
        serde_json::from_str(REAL_MODELING_APP).expect("the App Hub's own answer parses");
    // The App Hub publishes it as `Modeling`; `Modeling App` is what a
    // sentence and an installed instance call it.
    assert_eq!(app.name, "Modeling");
    assert_eq!(app.versions.len(), 3);
    // Every published version, without exception, carries a blank maximum.
    assert!(
        app.versions
            .iter()
            .all(|version| version.max_dhis_version.as_deref() == Some("")),
        "{:?}",
        app.versions
    );

    let picked = pick_version(&app.versions, "2.42.6")
        .expect("2.42.6 can run the Modeling App; a blank maximum is no maximum");
    assert_eq!(picked.version, "7.1.0");
    assert_eq!(picked.id, "4c895dd5-e121-43ab-af44-c853c3ea3297");

    // The minimum, which the App Hub does set, is still honoured: 1.0.2
    // wants 2.41, and an instance below that gets nothing.
    let oldest = &app.versions[2..];
    assert_eq!(oldest[0].min_dhis_version.as_deref(), Some("2.41"));
    assert_eq!(pick_version(oldest, "2.40"), None);
    assert_eq!(pick_version(oldest, "2.41").unwrap().version, "1.0.2");
}

/// A notifier that remembers a finished run is a run that happened on this
/// DHIS2: the notifier is in the process, not in the database, so nothing a
/// dump carries can put an entry in it.
#[test]
fn a_finished_run_in_the_notifier_is_a_run_that_happened_here() {
    let done = serde_json::json!({
        "lGTLZ5HrhHL": [
            {"time": "2026-09-27T10:10:40.063", "level": "INFO",
             "message": "Analytics table update process", "completed": false},
            {"time": "2026-09-27T10:10:56.185", "level": "INFO",
             "message": "Analytics tables updated: 00:00:16.098", "completed": true},
        ],
    });
    assert!(finished_here(&done));

    // A run still going is not one that finished, and neither is a failure.
    let running = serde_json::json!({
        "job": [{"time": "2026-09-27T10:10:40.063", "message": "analytics_2024"}],
    });
    assert!(!finished_here(&running));
    let failed = serde_json::json!({
        "job": [{"time": "2026-09-27T10:10:40.063", "level": "ERROR", "message": "no memory"}],
    });
    assert!(!finished_here(&failed));

    // An empty notifier is what a seeded deployment starts with, and what
    // every deployment has again after a restart.
    assert!(!finished_here(&serde_json::json!({})));
    assert!(!finished_here(&serde_json::json!("nope")));
}

/// The timestamp alone is worth nothing on a seeded deployment: it comes
/// out of the dump, and the tables it talks about were never restored.
#[test]
fn an_inherited_analytics_timestamp_is_not_evidence_of_analytics() {
    // The measured case: a freshly seeded Laos demo, reporting a last
    // success from the dump while `analytics_2024` did not exist at all.
    assert_eq!(
        analytics_evidence("2026-06-16T07:51:00.093", false, true),
        AnalyticsEvidence::Unconfirmed
    );
    // The same deployment once a run has finished on it.
    assert_eq!(
        analytics_evidence("2026-09-27T10:10:40.043", true, true),
        AnalyticsEvidence::RanHere
    );
    // A database DHIS2 migrated from empty has no dump to inherit from, so
    // what it records was recorded here.
    assert_eq!(
        analytics_evidence("2026-09-27T10:10:40.043", false, false),
        AnalyticsEvidence::Recorded
    );
    // Nothing recorded anywhere is the one unambiguous answer.
    assert_eq!(
        analytics_evidence("", false, true),
        AnalyticsEvidence::Never
    );
    assert_eq!(
        analytics_evidence("   ", false, false),
        AnalyticsEvidence::Never
    );
    // A run seen here outranks a missing record rather than contradicting
    // it: chap-devops watched it happen.
    assert_eq!(
        analytics_evidence("", true, true),
        AnalyticsEvidence::RanHere
    );
}

/// DHIS2 2.42.6 on an empty database sends the epoch as its last analytics
/// success. That is the default of the setting, not a run.
#[test]
fn the_epoch_as_the_last_analytics_success_is_no_run() {
    assert_eq!(recorded_success("1970-01-01T00:00:00.000"), "");
    assert_eq!(recorded_success(" 1970-01-01T00:00:00.000 "), "");
    assert_eq!(recorded_success(""), "");
    assert_eq!(
        recorded_success("2026-09-27T10:10:40.043"),
        "2026-09-27T10:10:40.043"
    );
    for seeded in [true, false] {
        assert_eq!(
            analytics_evidence("1970-01-01T00:00:00.000", false, seeded),
            AnalyticsEvidence::Never
        );
    }
}

#[test]
fn an_installed_app_is_matched_on_its_name_or_its_key() {
    let apps = serde_json::json!([
        {"name": "Modeling App", "key": "modeling-app", "version": "7.0.0"},
        {"name": "DHIS2 Climate App", "key": "dhis2-climate-app", "version": "1.16.2"},
    ]);
    let found = installed_app(&apps, "Modeling App").expect("the modeling app");
    assert_eq!(found.version, "7.0.0");
    // The key is the name in another spelling, and either one answers.
    assert_eq!(
        installed_app(&apps, "dhis2 climate app").unwrap().version,
        "1.16.2"
    );
    assert_eq!(
        installed_app(&apps, "DHIS2-Climate-App").unwrap().key,
        "dhis2-climate-app"
    );
    assert_eq!(installed_app(&apps, "Something Else"), None);
    assert_eq!(installed_app(&serde_json::json!([]), "Modeling App"), None);
    assert_eq!(installed_app(&serde_json::json!({}), "Modeling App"), None);

    // A longer or shorter listing name is the same app: the App Hub's
    // `DHIS2 Climate App` is installed as `Climate`, and the other way
    // round.
    assert_eq!(
        installed_app(
            &serde_json::json!([{"name": "Climate App", "version": "1.16.2"}]),
            "DHIS2 Climate App"
        )
        .unwrap()
        .version,
        "1.16.2"
    );
    // And two short names cannot match each other by accident.
    assert_eq!(
        installed_app(&serde_json::json!([{"name": "Maps"}]), "Data"),
        None
    );
}

#[test]
fn every_app_carries_the_name_a_listing_is_matched_on() {
    for app in HUB_APPS {
        assert!(!app.name.is_empty());
        let listing = serde_json::json!([{"name": app.name, "version": "1.0.0"}]);
        assert!(
            installed_app(&listing, app.name).is_some(),
            "{} has to match its own name",
            app.name
        );
    }
}

#[test]
fn the_install_path_is_the_app_hub_one() {
    assert_eq!(install_path("abc-123"), "/api/appHub/abc-123");
    // An id with a slash in it would otherwise be a path of its own.
    assert_eq!(install_path("a/b"), "/api/appHub/a%2Fb");
}

#[test]
fn the_two_apps_are_the_modeling_app_and_the_climate_app() {
    assert_eq!(HUB_APPS.len(), 2);
    assert_eq!(HUB_APPS[0].id, "a29851f9-82a7-4ecd-8b2c-58e0f220bc75");
    assert_eq!(HUB_APPS[1].id, "effb986c-a3c7-485e-a2f6-5e54ff9df7c3");
    for app in HUB_APPS {
        assert!(!app.label.is_empty() && !app.what.is_empty());
    }
}

#[test]
fn the_app_hub_base_is_public_unless_the_variable_moves_it() {
    assert_eq!(
        crate::github::base_of(None, DEFAULT_APP_HUB),
        "https://apps.dhis2.org/api/v1/apps"
    );
    assert_eq!(
        crate::github::base_of(Some("http://127.0.0.1:18099/apps/".into()), DEFAULT_APP_HUB),
        "http://127.0.0.1:18099/apps"
    );
}

#[test]
fn the_url_is_the_base_and_the_path_with_one_slash_between_them() {
    let dhis2 = Dhis2::new(
        "http://localhost:18080/",
        Credentials::new("admin", "district", CredentialSource::Default),
        DEFAULT_TIMEOUT,
    );
    assert_eq!(dhis2.base(), "http://localhost:18080");
    assert_eq!(dhis2.url("/api/me"), "http://localhost:18080/api/me");
    assert_eq!(dhis2.url("api/me"), "http://localhost:18080/api/me");
    assert_eq!(dhis2.credentials().user(), Some("admin"));
    assert_eq!(dhis2.credentials().source, CredentialSource::Default);
    assert_eq!(
        dhis2.with_timeout(INSTALL_TIMEOUT).url("/api/me"),
        "http://localhost:18080/api/me"
    );
}

/// A port nothing listens on covers the transport-error path without a
/// network - and the sentence is DHIS2's, not chap-core's.
#[test]
fn an_unreachable_dhis2_says_so_in_its_own_words() {
    let base = crate::test_support::unreachable_base();
    let dhis2 = Dhis2::new(
        &base,
        Credentials::new("admin", "district", CredentialSource::Default),
        Duration::from_secs(2),
    );
    let err = dhis2
        .send("GET", SYSTEM_INFO_PATH, None)
        .expect_err("nothing is listening there");
    assert!(
        matches!(
            err.downcast_ref::<ChapError>(),
            Some(ChapError::Dhis2Unreachable { .. })
        ),
        "{err}"
    );
    let text = err.to_string();
    assert!(text.contains(&format!("DHIS2 at {base}")), "{text}");
    assert!(text.contains("`chap-devops status`"), "{text}");
    assert!(!text.contains("chap-core"), "{text}");
}

/// The wait gives up with the two things the reader needs: where the
/// migration shows, and the flag that waits longer.
#[test]
fn a_wait_that_runs_out_names_the_logs_and_the_flag() {
    let dhis2 = Dhis2::new(
        &crate::test_support::unreachable_base(),
        Credentials::new("admin", "district", CredentialSource::Default),
        Duration::from_millis(200),
    );
    let mut said = 0;
    let err = dhis2
        .wait_for_api(
            Duration::from_millis(1),
            Duration::from_millis(1),
            &mut |_| said += 1,
        )
        .expect_err("nothing is listening there");
    let text = err.to_string();
    assert!(text.contains("`chap-devops logs dhis2`"), "{text}");
    assert!(text.contains("--wait SECONDS"), "{text}");
    assert_eq!(said, 1, "the wait announces itself once");
}

#[test]
fn the_offline_refusal_names_both_halves_of_the_network_it_needs() {
    assert!(OFFLINE_APPS.contains("App Hub"));
    assert!(OFFLINE_APPS.contains("`--offline`"));
    assert!(OFFLINE_APPS.contains("App Management"));
}

/// The seed password serves every user of a DHIS2 chap-devops deployed from a
/// seed, after `.env` and the environment, and never another instance.
#[test]
fn the_seed_password_serves_every_user_of_a_seeded_dhis2() {
    let seeded = |user| CredentialInputs {
        user,
        seed_password: Some("district"),
        ..inputs("")
    };
    let em = resolved(seeded(Some("em")));
    assert_eq!(em.user(), Some("em"));
    assert_eq!(em.source, CredentialSource::SeedPassword);
    assert_eq!(
        em.describe(),
        "the seed password from `.chap-devops/components.yaml`"
    );
    // Without --user: the user `.env` names, or admin.
    let admin = resolved(seeded(None));
    assert_eq!(admin.user(), Some("admin"));
    assert_eq!(admin.source, CredentialSource::SeedPassword);
    // The environment still wins.
    let exported = resolved(CredentialInputs {
        env_password: Some("sekret"),
        ..seeded(Some("em"))
    });
    assert_eq!(exported.source, CredentialSource::Environment);
    // Not on a DHIS2 chap-devops did not deploy.
    let external = credentials_of(&CredentialInputs {
        deployed: false,
        ..seeded(Some("em"))
    });
    assert!(external.is_err());
}

/// A run that was asked for a moment ago is in the job list before the
/// notifier lists it. Finished runs and the scheduled nightly job are not.
#[test]
fn a_queued_analytics_job_is_read_off_the_job_list() {
    let listing = serde_json::json!({"jobConfigurations": [
        {"id": "nightly", "jobStatus": "SCHEDULED", "schedulingType": "CRON",
         "enabled": true},
        {"id": "done", "jobStatus": "DISABLED", "schedulingType": "ONCE_ASAP",
         "enabled": false, "lastFinished": "2026-10-08T09:00:00.000"},
        {"id": "lwKM9gOUwzR", "jobStatus": "SCHEDULED", "schedulingType": "ONCE_ASAP",
         "enabled": true},
    ]});
    assert_eq!(queued_job(&listing).as_deref(), Some("lwKM9gOUwzR"));

    let running = serde_json::json!({"jobConfigurations": [
        {"id": "second", "jobStatus": "SCHEDULED", "schedulingType": "ONCE_ASAP",
         "enabled": true},
        {"id": "first", "jobStatus": "RUNNING", "schedulingType": "ONCE_ASAP",
         "enabled": true},
    ]});
    assert_eq!(queued_job(&running).as_deref(), Some("first"));

    let none = serde_json::json!({"jobConfigurations": [
        {"id": "nightly", "jobStatus": "SCHEDULED", "schedulingType": "CRON"},
    ]});
    assert_eq!(queued_job(&none), None);
    assert_eq!(queued_job(&serde_json::json!({})), None);
    assert!(queued_jobs_query().contains("filter=jobType:eq:ANALYTICS_TABLE"));
}

/// DHIS2 writes `lastAnalyticsTableSuccess` with no zone, in the zone of the
/// server. 2.42.6 in a container sent `serverDate` 07:17 at 09:17 CEST: the
/// gap to `now` is the server offset, and the answer time is rounded out.
#[test]
fn the_server_offset_comes_from_server_date_and_rounds_to_the_quarter_hour() {
    // 2026-10-20T07:17:21Z
    let now = 1_792_480_641;
    assert_eq!(server_offset("2026-10-20T07:17:21.579", now), Some(0));
    assert_eq!(server_offset("2026-10-20T07:17:24.000", now), Some(0));
    assert_eq!(
        server_offset("2026-10-20T09:17:20.900", now),
        Some(2 * 3600)
    );
    assert_eq!(
        server_offset("2026-10-20T01:47:22.000", now),
        Some(-(5 * 3600 + 1800))
    );
    assert_eq!(server_offset("2026-10-22T07:17:21.000", now), None);
    assert_eq!(server_offset("", now), None);
}

#[test]
fn a_recorded_success_is_read_in_the_zone_of_the_server() {
    // 2026-06-16T07:51:00Z
    let utc = 1_781_596_260;
    assert_eq!(success_as_unix("2026-06-16T07:51:00.093", 0), Some(utc));
    assert_eq!(
        success_as_unix("2026-06-16T09:51:00.093", 2 * 3600),
        Some(utc)
    );
    assert_eq!(success_as_unix("", 0), None);
}

/// With no `serverDate`, the time stays as DHIS2 wrote it, and the human
/// output says whose clock it is.
#[test]
fn a_recorded_success_with_no_server_date_stays_as_dhis2_wrote_it() {
    let info = serde_json::json!({"lastAnalyticsTableSuccess": "2026-06-16T07:51:00.093"});
    let recorded = recorded_success_of(&info, 1_792_480_641);
    assert_eq!(recorded, "2026-06-16T07:51:00.093");
    assert_eq!(
        shown_success(&recorded),
        "2026-06-16T07:51:00.093 (DHIS2 server time)"
    );
    assert_eq!(shown_success(""), "");
}

#[test]
fn a_recorded_success_with_a_server_date_has_the_offset_of_this_machine() {
    let info = serde_json::json!({
        "lastAnalyticsTableSuccess": "2026-06-16T07:51:00.093",
        "serverDate": "2026-10-20T07:17:21.579",
    });
    let recorded = recorded_success_of(&info, 1_792_480_641);
    assert_eq!(
        crate::status::parse_rfc3339(&recorded),
        Some(1_781_596_260),
        "{recorded}"
    );
    assert_eq!(
        shown_success(&recorded),
        crate::output::local_date_time(1_781_596_260)
    );
}
