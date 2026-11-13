use super::*;

fn answer(status: u16, body: &str) -> Answer {
    Answer {
        status,
        reason: "Not Found".to_string(),
        content_type: JSON.to_string(),
        body: body.as_bytes().to_vec(),
        wire_bytes: body.len(),
        transport: Default::default(),
    }
}

#[test]
fn the_url_is_the_base_and_the_path_with_one_slash_between_them() {
    let api = Api::new("http://localhost:8140/", None, DEFAULT_TIMEOUT);
    assert_eq!(api.base(), "http://localhost:8140");
    assert_eq!(api.url("/v1/jobs"), "http://localhost:8140/v1/jobs");
    assert_eq!(api.url("v1/jobs"), "http://localhost:8140/v1/jobs");
    assert!(!api.has_token());
    assert!(Api::new("http://x", Some("t".into()), DEFAULT_TIMEOUT).has_token());
}

#[test]
fn the_methods_are_the_five_and_case_does_not_matter() {
    for spelling in ["get", "GET", " Get "] {
        assert_eq!(method_of(spelling).unwrap(), "GET");
    }
    assert_eq!(method_of("delete").unwrap(), "DELETE");
    let err = method_of("BREW").expect_err("not a method chap-devops sends");
    assert!(err.to_string().contains("GET, POST, PUT, PATCH, DELETE"));
}

#[test]
fn a_path_has_to_start_with_a_slash() {
    assert_eq!(path_of("/v1/jobs").unwrap(), "/v1/jobs");
    assert_eq!(path_of(" /v1/jobs?limit=2 ").unwrap(), "/v1/jobs?limit=2");
    let err = path_of("v1/jobs").expect_err("no leading slash");
    assert!(err.to_string().contains("starts with `/`"), "{err}");
}

#[test]
fn a_status_line_names_the_code_and_the_reason() {
    assert_eq!(answer(404, "{}").status_line(), "HTTP 404 Not Found");
    let mut bare = answer(499, "{}");
    bare.reason = String::new();
    assert_eq!(bare.status_line(), "HTTP 499");
    assert!(!answer(404, "{}").is_success());
    let mut ok = answer(204, "");
    ok.status = 204;
    assert!(ok.is_success());
}

#[test]
fn the_detail_field_is_what_chap_core_said() {
    assert_eq!(
        answer(404, r#"{"detail":"Job not found"}"#).detail(),
        "Job not found"
    );
    // A 422 answers with a list of problems rather than a sentence.
    assert_eq!(
        answer(422, r#"{"detail":[{"loc":["body"]}]}"#).detail(),
        r#"[{"loc":["body"]}]"#
    );
    // No `detail` at all: the body, trimmed.
    assert_eq!(answer(500, "  boom \n").detail(), "boom");
    // And something enormous is cut rather than printed into an error.
    let long = "x".repeat(500);
    let cut = answer(500, &long).detail();
    assert_eq!(cut.chars().count(), 203);
    assert!(cut.ends_with("..."));
}

#[test]
fn a_json_body_reads_back_as_json_and_anything_else_does_not() {
    assert_eq!(
        answer(200, r#"{"id":"abc"}"#).json().unwrap()["id"],
        serde_json::json!("abc")
    );
    assert!(answer(200, "<html></html>").json().is_none());
    assert_eq!(answer(200, "  text ").text(), "  text ");
}

#[test]
fn query_values_are_escaped_and_appended() {
    assert_eq!(query_pair("status", "SUCCESS"), "status=SUCCESS");
    assert_eq!(
        query_pair("type", "create backtest/1"),
        "type=create%20backtest%2F1"
    );
    assert_eq!(with_query("/v1/jobs", &[]), "/v1/jobs");
    assert_eq!(
        with_query("/v1/jobs", &[query_pair("status", "SUCCESS")]),
        "/v1/jobs?status=SUCCESS"
    );
    assert_eq!(
        with_query(
            "/v1/jobs?type=x",
            &[query_pair("status", "A"), query_pair("status", "B")]
        ),
        "/v1/jobs?type=x&status=A&status=B"
    );
}

/// A port nothing listens on covers the transport-error path without a
/// network.
#[test]
fn an_unreachable_api_is_the_error_status_uses() {
    let base = crate::test_support::unreachable_base();
    let api = Api::new(&base, None, Duration::from_secs(2));
    let err = api
        .send("GET", "/v1/jobs", None)
        .expect_err("nothing is listening there");
    assert!(
        matches!(
            err.downcast_ref::<ChapError>(),
            Some(ChapError::Unreachable { .. })
        ),
        "{err}"
    );
    let text = err.to_string();
    assert!(text.contains(&base), "{text}");
    assert!(text.contains("is not responding"), "{text}");
    assert!(text.contains("chap-devops status"), "{text}");
}

#[test]
fn the_deployments_token_wins_and_the_environment_is_the_fallback() {
    let project = || Some("from-env-file".to_string());
    let shell = || Some("from-shell".to_string());
    assert_eq!(
        pick_token(project(), shell()).as_deref(),
        Some("from-env-file")
    );
    assert_eq!(pick_token(None, shell()).as_deref(), Some("from-shell"));
    assert_eq!(
        pick_token(project(), None).as_deref(),
        Some("from-env-file")
    );
    assert_eq!(pick_token(None, None), None);
    // An empty value is no token, from either side.
    assert_eq!(
        pick_token(Some("  ".into()), shell()).as_deref(),
        Some("from-shell")
    );
    assert_eq!(pick_token(None, Some(String::new())), None);
    assert_eq!(
        pick_token(None, Some(" spaced ".into())).as_deref(),
        Some("spaced")
    );
}

#[test]
fn same_origin_compares_scheme_host_and_port() {
    assert!(same_origin(
        "http://localhost:8700",
        "http://127.0.0.1:8700/v1/jobs"
    ));
    assert!(same_origin(
        "https://chap.example.org",
        "https://CHAP.example.org:443/x"
    ));
    assert!(!same_origin(
        "http://localhost:8700",
        "http://localhost:8701"
    ));
    assert!(!same_origin(
        "http://chap.example.org",
        "https://chap.example.org"
    ));
    assert!(!same_origin(
        "http://localhost:8700",
        "http://staging.example.org:8700"
    ));
    assert!(!same_origin(
        "http://user@evil.example:8700",
        "http://localhost:8700"
    ));
    assert!(!same_origin("not a url", "not a url"));
}

#[test]
fn gunzip_unpacks_a_gzip_body_and_refuses_a_broken_one() {
    use std::io::Write;
    let text = r#"[{"orgUnit":"loc01","period":"2024W01","value":12}]"#.repeat(50);
    let mut packer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    packer.write_all(text.as_bytes()).expect("packed");
    let packed = packer.finish().expect("packed");
    assert!(packed.len() < text.len());
    assert_eq!(gunzip(&packed).expect("unpacked"), text.as_bytes());
    assert!(gunzip(b"not gzip").is_err());
}
