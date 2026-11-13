use super::detail::{ModelDetail, first_line, indent_block, info_lines, render_info};
use super::*;
use crate::output;
use crate::project::ManualModel;

fn registry() -> crate::registry::Registry {
    crate::registry::load_embedded().expect("the embedded snapshot parses")
}

fn model(id: &str) -> Model {
    registry().get(id).expect("a vendored model").clone()
}

fn enabled() -> EnabledModel {
    enabled_on(Some(5001))
}

fn enabled_on(host_port: Option<u16>) -> EnabledModel {
    EnabledModel {
        previous: None,
        reads_port: false,
        service_id: "chapkit-ewars-model".into(),
        image: "ghcr.io/chap-models/chapkit_ewars_model".into(),
        image_tag: "sha-fa880a1".into(),
        image_digest: None,
        version: "1.0.0".into(),
        channel: Some(Channel::Stable),
        host_port,
        bind: None,
        data_dir: "/app/data".into(),
        // What `models enable` records today: the numeric pair the image's
        // `USER chapkit` resolves to.
        user: "1000:1000".into(),
        user_from: Default::default(),
        platform: Some("linux/amd64".into()),
        multi_arch: false,
        compose_file: "compose.chapkit-ewars-model.yml".into(),
    }
}

#[test]
fn a_row_carries_the_channels_and_the_stable_image() {
    let m = model("chapkit_ewars_model");
    let r = row(&m, None);
    assert_eq!(r.id, "chapkit_ewars_model");
    assert_eq!(r.service_id, "chapkit-ewars-model");
    assert_eq!(r.kind, Kind::Model);
    assert_eq!(r.stable, m.channels.stable);
    assert_eq!(r.latest, m.channels.latest);
    assert_eq!(r.enabled_port, None);
    assert!(!r.enabled);
    assert_eq!(port_cell(&Out::default(), &r), "-");
    assert!(
        r.image.starts_with(&format!("{}:", m.source.image)),
        "{} should be a tagged reference",
        r.image
    );
}

#[test]
fn a_row_reports_the_port_of_an_enabled_model() {
    let r = row(&model("chapkit_ewars_model"), Some(&enabled()));
    assert!(r.enabled);
    assert_eq!(r.enabled_port, Some(5001));
    assert_eq!(port_cell(&Out::default(), &r), "5001");

    // Enabled without a port: the way in, not a dash.
    let r = row(&model("chapkit_ewars_model"), Some(&enabled_on(None)));
    assert!(r.enabled);
    assert_eq!(r.enabled_port, None);
    assert_eq!(port_cell(&Out::default(), &r), "via chap-core");
}

#[test]
fn the_kind_column_is_optional_and_last() {
    let out = Out::default();
    let rows = vec![row(&model("chapkit_minimalist_example_py"), None)];

    let plain = table(&out, &rows, false);
    assert_eq!(
        plain.lines().next().unwrap().split_whitespace().last(),
        Some("PORT")
    );
    assert!(!plain.contains("starter"));

    let with_kind = table(&out, &rows, true);
    assert_eq!(
        with_kind.lines().next().unwrap().split_whitespace().last(),
        Some("KIND")
    );
    assert!(with_kind.lines().nth(1).unwrap().ends_with("starter"));
    assert!(with_kind.lines().all(|l| !l.ends_with(' ')));
}

/// `models list` and `models search` list the catalogue by maturity, the
/// same order the browser puts it in.
#[test]
fn a_listing_is_ordered_by_maturity_then_by_name() {
    let registry = registry();
    let mut listed: Vec<&Model> = registry.models.iter().collect();
    listed.sort_by_key(|m| m.order_key());
    let rows: Vec<ModelRow> = listed
        .iter()
        .filter(|m| !m.is_starter())
        .map(|m| row(m, None))
        .collect();
    assert_eq!(
        rows.iter()
            .map(|r| r.display_name.as_str())
            .collect::<Vec<&str>>(),
        vec![
            "CHAP-EWARS",
            "Simple Multistep",
            "Auto-ARIMA",
            "GHRmodel",
            "Rwanda Malaria BYM",
        ]
    );
    // A manual entry is sorted among the marketplace ones, not appended:
    // where it came from says nothing about how far it can be trusted.
    let manual = manual_entry().to_model("chapkit_dengue_model");
    let mut with_manual: Vec<&Model> = registry.models.iter().collect();
    with_manual.push(&manual);
    with_manual.sort_by_key(|m| m.order_key());
    let names: Vec<&str> = with_manual
        .iter()
        .filter(|m| !m.is_starter())
        .map(|m| m.display_name.as_str())
        .collect();
    assert!(
        names.iter().position(|n| *n == "chapkit_ghr_model")
            < names.iter().position(|n| *n == "Rwanda Malaria BYM"),
        "a gray marketplace model still comes last: {names:?}"
    );
}

/// A model this deployment added itself, synthesised the way
/// `with_manual` does.
fn manual_entry() -> ManualModel {
    ManualModel {
        reads_port: false,
        service_id: "chapkit-ghr-model".into(),
        display_name: "chapkit_ghr_model".into(),
        repository: Some("https://github.com/chap-models/chapkit_ghr_model".into()),
        image: "ghcr.io/chap-models/chapkit_ghr_model".into(),
        tag: "sha-b1d6c31".into(),
        commit: Some("b1d6c31f4b2f".into()),
        follow: Some("main".into()),
        data_dir: Some("/work/data".into()),
        user: Some("10001:10001".into()),
        runtime_amd64: true,
        added: "2026-09-24".into(),
    }
}

#[test]
fn the_kind_cell_marks_a_manually_added_model() {
    let out = Out::default();
    let manual = manual_entry().to_model("chapkit_ghr_model");
    let rows = vec![row(&manual, None), row(&model("chapkit_ewars_model"), None)];
    assert!(rows[0].manual && !rows[1].manual);
    assert_eq!(row_kind(&rows[0]), "manual");
    assert_eq!(row_kind(&rows[1]), "model");

    // The column carries it, and the marketplace row keeps its own word.
    let text = table(&out, &rows, true);
    assert!(text.lines().nth(1).unwrap().ends_with("manual"), "{text}");
    assert!(text.lines().nth(2).unwrap().ends_with("model"), "{text}");
    assert!(text.lines().all(|l| !l.ends_with(' ')));

    // And `--json` says so per row.
    let value: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&rows).unwrap()).unwrap();
    assert_eq!(value[0]["manual"], true);
    assert_eq!(value[1]["manual"], false);
}

#[test]
fn info_names_the_source_and_the_branch_of_a_manual_model() {
    let entry = manual_entry();
    let manual = entry.to_model("chapkit_ghr_model");
    let text = detail_with(&manual, None, Some(&entry));
    assert!(text.contains("kind        manual"), "{text}");
    assert!(
        text.contains("source      manual (`chap-devops models add`, 2026-09-24)"),
        "{text}"
    );
    assert!(text.contains("follows     main"), "{text}");
    assert!(
        text.contains("image       ghcr.io/chap-models/chapkit_ghr_model:sha-b1d6c31"),
        "{text}"
    );
    // Nothing the marketplace would have established is claimed.
    for absent in [
        "horizon",
        "requires geo",
        "covariates",
        "channels",
        "status ",
    ] {
        assert!(!text.contains(absent), "{absent} in:\n{text}");
    }
    assert!(text.contains("(amd64 only)"), "{text}");

    // A pinned entry says so where the branch would have been.
    let pinned = ManualModel {
        follow: None,
        ..entry
    };
    let text = detail_with(&pinned.to_model("chapkit_ghr_model"), None, Some(&pinned));
    assert!(text.contains("follows     nothing (pinned)"), "{text}");

    // A marketplace model is unchanged: no source line, and every
    // marketplace field still there.
    let text = detail_of(&model("chapkit_ewars_model"), None);
    assert!(!text.contains("source  "), "{text}");
    assert!(text.contains("horizon"), "{text}");
}

#[test]
fn info_json_carries_the_manual_flag() {
    let manual = manual_entry().to_model("chapkit_ghr_model");
    let detail = ModelDetail {
        model: &manual,
        enabled: None,
        manual: None,
        in_project: true,
        reach: None,
        image_stable: channel_image(&manual, Channel::Stable),
        image_latest: channel_image(&manual, Channel::Latest),
        needs_amd64: manual.needs_amd64(),
    };
    let value: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&detail).unwrap()).unwrap();
    assert_eq!(value["manual"], true);
    assert_eq!(value["id"], "chapkit_ghr_model");
    // Both channels resolve to the one published build.
    assert_eq!(
        value["image_stable"],
        "ghcr.io/chap-models/chapkit_ghr_model:sha-b1d6c31"
    );
    assert_eq!(value["image_stable"], value["image_latest"]);

    // A marketplace entry says so too, rather than leaving the field out.
    let ewars = model("chapkit_ewars_model");
    let detail = ModelDetail {
        model: &ewars,
        enabled: None,
        manual: None,
        in_project: false,
        reach: None,
        image_stable: None,
        image_latest: None,
        needs_amd64: false,
    };
    let value: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&detail).unwrap()).unwrap();
    assert_eq!(value["manual"], false);
}

#[test]
fn an_unenabled_model_shows_a_dash() {
    let out = Out::default();
    let rows = vec![row(&model("auto_arima_chapkit"), None)];
    let text = table(&out, &rows, false);
    assert!(text.lines().nth(1).unwrap().ends_with(" -"), "{text}");
}

fn detail_of(m: &Model, enabled: Option<&EnabledModel>) -> String {
    detail_with(m, enabled, None)
}

/// The same, for a model the deployment defines itself.
fn detail_with(m: &Model, enabled: Option<&EnabledModel>, manual: Option<&ManualModel>) -> String {
    let project = Project {
        dir: std::path::PathBuf::from("/tmp/chapx"),
        state: Default::default(),
    };
    let detail = ModelDetail {
        model: m,
        enabled,
        manual,
        // Every `info` test below runs as if inside a deployment.
        in_project: true,
        reach: enabled.map(|e| crate::status::reach(&project, e.host_port, &e.service_id)),
        image_stable: channel_image(m, Channel::Stable),
        image_latest: channel_image(m, Channel::Latest),
        needs_amd64: m.needs_amd64(),
    };
    let mut lines = Report::default();
    info_lines(&detail, &mut lines);
    format!("{}{}", render_info(&Out::default(), &detail), lines.text())
}

#[test]
fn info_covers_every_documented_section() {
    let m = model("chapkit_ewars_model");
    let text = detail_of(&m, None);

    assert!(text.starts_with("CHAP-EWARS\n"));
    for needle in [
        "id            chapkit_ewars_model",
        "service       chapkit-ewars-model",
        "kind          model",
        "status        orange",
        "repository",
        "runtime",
        "period types",
        "horizon",
        "requires geo",
        "covariates",
        "channels",
        "maintainers",
        "attribution",
        "versions",
        "VERSION  IMAGE TAG",
        "configurations",
    ] {
        assert!(text.contains(needle), "{needle} missing from:\n{text}");
    }
    assert!(text.contains("(amd64 only)"), "ewars is an R-INLA model");
    assert!(text.contains(&m.channels.stable));
    // Not enabled here, and the page says so on its last lines rather than
    // leaving the missing block to be noticed.
    assert!(text.ends_with(
        "chapkit_ewars_model is not enabled in this deployment\n\
         hint: `chap-devops models enable chapkit_ewars_model` enables it\n"
    ));
    assert!(text.lines().all(|l| !l.ends_with(' ')), "{text}");
}

#[test]
fn info_wraps_the_summary_inside_eighty_columns() {
    // The versions table and image references can legitimately be wide;
    // prose must not be, so only the summary block is checked.
    for m in registry().models.iter() {
        let text = detail_of(m, None);
        let mut in_summary = false;
        for line in text.lines() {
            if line.starts_with("  summary ") {
                in_summary = true;
            } else if in_summary && !line.starts_with("                  ") {
                in_summary = false;
            }
            if in_summary {
                assert!(
                    line.chars().count() <= output::WRAP_WIDTH,
                    "{}: {line}",
                    m.id
                );
            }
        }
        assert!(text.contains("  summary "), "{} has no summary block", m.id);
    }
}

#[test]
fn info_shows_the_project_entry_when_the_model_is_enabled() {
    let text = detail_of(&model("chapkit_ewars_model"), Some(&enabled()));
    assert!(text.contains("enabled in this deployment"));
    assert!(text.contains("reach     http://localhost:5001"), "{text}");
    assert!(text.contains("1.0.0 (stable)"));
    assert!(text.contains("compose.chapkit-ewars-model.yml"));
    // The user, and where it came from: `table` is the last resort, and a
    // permission error is read off exactly this line.
    assert!(text.contains("data dir  /app/data"), "{text}");
    assert!(text.contains("user      1000:1000  (table)"), "{text}");
}

/// The same block for a model whose user was read off the image.
#[test]
fn info_says_where_the_user_came_from() {
    let entry = EnabledModel {
        previous: None,
        user: "root".into(),
        user_from: crate::compose::UserSource::ImageConfig,
        ..enabled()
    };
    let text = detail_of(&model("chapkit_ewars_model"), Some(&entry));
    assert!(text.contains("user      root  (image config)"), "{text}");
}

#[test]
fn info_names_the_proxy_for_a_model_with_no_host_port() {
    let text = detail_of(&model("chapkit_ewars_model"), Some(&enabled_on(None)));
    assert!(
        text.contains(
            "reach     internal \
                 (proxy: http://localhost:8700/v2/services/chapkit-ewars-model/run/)"
        ),
        "{text}"
    );
}

#[test]
fn info_json_adds_the_derived_fields_to_the_whole_model() {
    let m = model("chapkit_ewars_model");
    let detail = ModelDetail {
        model: &m,
        enabled: None,
        manual: None,
        in_project: true,
        reach: None,
        image_stable: channel_image(&m, Channel::Stable),
        image_latest: channel_image(&m, Channel::Latest),
        needs_amd64: m.needs_amd64(),
    };
    let value: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&detail).unwrap()).unwrap();

    // Flattened, so the marketplace fields sit at the top level.
    assert_eq!(value["id"], "chapkit_ewars_model");
    assert_eq!(value["kind"], "model");
    assert_eq!(value["assessed_status"], "orange");
    assert!(value["versions"].is_array());
    assert!(value["configurations"].is_object());
    assert_eq!(value["needs_amd64"], true);
    assert_eq!(value["enabled"], serde_json::Value::Null);
    assert!(value["image_stable"].as_str().unwrap().contains(":sha-"));
    assert!(value["image_latest"].as_str().unwrap().contains(":sha-"));
}

#[test]
fn row_json_has_the_documented_shape() {
    let rows = vec![row(&model("chapkit_ewars_model"), Some(&enabled()))];
    let value: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&rows).unwrap()).unwrap();
    let first = &value[0];
    for key in [
        "id",
        "service_id",
        "display_name",
        "kind",
        "assessed_status",
        "stable",
        "latest",
        "enabled",
        "enabled_port",
        "image",
        "requires_geo",
    ] {
        assert!(!first[key].is_null(), "{key} is missing or null");
    }
    assert_eq!(first["enabled_port"], 5001);
    assert_eq!(first["enabled"], true);

    // An enabled model with no host port is `enabled` with a null port,
    // which is what tells it apart from one that is not enabled at all.
    let rows = vec![row(&model("chapkit_ewars_model"), Some(&enabled_on(None)))];
    let value: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&rows).unwrap()).unwrap();
    assert_eq!(value[0]["enabled"], true);
    assert!(value[0]["enabled_port"].is_null());
}

#[test]
fn a_dangling_channel_pointer_does_not_panic() {
    let mut m = model("auto_arima_chapkit");
    m.channels.latest = "9.9.9".to_string();
    assert!(channel_image(&m, Channel::Latest).is_none());
    let text = detail_of(&m, None);
    assert!(text.contains("no such version"), "{text}");
    // The row still has a usable image, from the stable channel.
    assert!(row(&m, None).image.contains(':'));
}

#[test]
fn labels_match_the_yaml_spelling() {
    assert_eq!(AssessedStatus::Gray.colour(), "gray");
    assert_eq!(kind_label(Kind::Starter), "starter");
    assert_eq!(first_line(Some("first\nsecond")), "first");
    assert_eq!(first_line(None), "");
    assert_eq!(indent_block("a\nb", 2), "  a\n  b\n");
}

#[test]
fn a_listing_ends_on_one_count_line_and_the_way_to_enable_is_a_hint() {
    let m = model("chapkit_ewars_model");
    let rows = vec![row(&m, None)];

    let mut lines = Report::default();
    list_lines(&rows, false, true, &mut lines);
    assert_eq!(
        lines.text(),
        "1 listed, none enabled in this deployment\n\
         hint: `chap-devops models enable ID` enables one\n"
    );

    let mut lines = Report::default();
    list_lines(&rows, false, false, &mut lines);
    assert_eq!(lines.text(), "1 listed\n");

    let mut lines = Report::default();
    list_lines(&[], true, true, &mut lines);
    assert_eq!(
        lines.text(),
        "no models enabled\nhint: `chap-devops models enable ID` enables one\n"
    );
}
