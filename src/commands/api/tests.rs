use super::*;

#[test]
fn the_three_spellings_of_data_are_told_apart() {
    assert_eq!(source_of("-"), Source::Stdin);
    assert_eq!(source_of("@body.json"), Source::File("body.json"));
    assert_eq!(source_of("@/tmp/body.json"), Source::File("/tmp/body.json"));
    assert_eq!(source_of(r#"{"a":1}"#), Source::Inline(r#"{"a":1}"#));
    // A body that happens to start with a dash is still inline: only the
    // bare `-` is stdin.
    assert_eq!(source_of("-1"), Source::Inline("-1"));
}

#[test]
fn an_inline_body_is_read_and_checked() {
    assert_eq!(read_data(r#"{"a":1}"#).unwrap(), r#"{"a":1}"#);
    // Byte for byte: the spacing a caller chose is the spacing chap-core
    // receives.
    assert_eq!(read_data(r#"{ "a" : 1 }"#).unwrap(), r#"{ "a" : 1 }"#);
    // A bare JSON value is a body too.
    assert_eq!(read_data("[1,2]").unwrap(), "[1,2]");
    assert_eq!(read_data("\"text\"").unwrap(), "\"text\"");

    let err = read_data("{a:1}").expect_err("not JSON");
    assert!(err.to_string().contains("given inline"), "{err}");
    assert!(err.to_string().contains("not valid JSON"), "{err}");

    let err = read_data("   ").expect_err("nothing to send");
    assert!(err.to_string().contains("is empty"), "{err}");
}

#[test]
fn a_file_body_is_read_from_the_path_after_the_at_sign() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("body.json");
    std::fs::write(&path, "{\n  \"name\": \"eval\"\n}\n").unwrap();
    let spec = format!("@{}", path.display());
    assert_eq!(read_data(&spec).unwrap(), "{\n  \"name\": \"eval\"\n}\n");

    let bad = dir.path().join("broken.json");
    std::fs::write(&bad, "{nope}").unwrap();
    let err = read_data(&format!("@{}", bad.display())).expect_err("not JSON");
    assert!(err.to_string().contains("broken.json"), "{err}");

    let err = read_data("@/nowhere/at/all.json").expect_err("no such file");
    assert!(err.to_string().contains("/nowhere/at/all.json"), "{err}");
}

#[test]
fn a_body_that_is_not_json_is_a_usage_error() {
    let err = read_data("{a:1}").expect_err("not JSON");
    assert!(
        matches!(err.downcast_ref::<ChapError>(), Some(ChapError::Usage(_))),
        "{err}"
    );
}

/// The rendering rules, without a server: the same match `print_body`
/// makes, asserted on the bodies it makes it for.
#[test]
fn a_json_string_reads_as_text_and_everything_else_as_json() {
    let string: serde_json::Value = serde_json::from_str(r#""line one\nline two\n""#).unwrap();
    assert_eq!(string.as_str().unwrap(), "line one\nline two\n");

    let object: serde_json::Value = serde_json::from_str(r#"{"id":"abc","n":1}"#).unwrap();
    assert_eq!(
        serde_json::to_string_pretty(&object).unwrap(),
        "{\n  \"id\": \"abc\",\n  \"n\": 1\n}"
    );
}

#[test]
fn text_is_written_with_exactly_one_trailing_newline() {
    let mut out = Vec::new();
    write_text(&mut out, "one").unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), "one\n");

    let mut out = Vec::new();
    write_text(&mut out, "one\n").unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), "one\n");

    let mut out = Vec::new();
    write_text(&mut out, "").unwrap();
    assert!(out.is_empty(), "an empty body prints nothing at all");
}

#[test]
fn a_401_says_which_half_of_the_token_is_missing() {
    let with = Api::new("http://x", Some("t".into()), Duration::from_secs(1));
    let without = Api::new("http://x", None, Duration::from_secs(1));
    let answer = |status| Answer {
        status,
        reason: "Unauthorized".to_string(),
        content_type: crate::api::JSON.to_string(),
        body: br#"{"detail":"Missing or invalid API token"}"#.to_vec(),
    };
    assert!(
        next_step(&with, &answer(401))
            .unwrap()
            .contains("did not accept")
    );
    assert!(
        next_step(&without, &answer(401))
            .unwrap()
            .contains("none was sent")
    );
    assert_eq!(next_step(&with, &answer(404)), None);
}
