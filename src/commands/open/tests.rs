use super::*;
use crate::components::{Components, Dhis2Component, OcsComponent};

/// A model's `/docs` is on its own port when it publishes one, and through
/// chap-core's proxy when it does not.
#[test]
fn a_model_opens_its_docs_on_its_port_or_through_chap_core() {
    let mut project = Project {
        dir: std::path::PathBuf::from("/tmp/chapx"),
        state: Default::default(),
    };
    let mut model = crate::project::EnabledModel {
        previous: None,
        reads_port: false,
        service_id: "chapkit-ewars-model".into(),
        image: String::new(),
        image_tag: String::new(),
        image_digest: None,
        version: String::new(),
        channel: None,
        host_port: Some(5001),
        bind: None,
        data_dir: String::new(),
        user: String::new(),
        user_from: Default::default(),
        platform: None,
        compose_file: String::new(),
    };
    let url = |project: &Project, model: &crate::project::EnabledModel| {
        model_docs_url(project, "chapkit_ewars_model", model)
    };
    assert_eq!(url(&project, &model).unwrap(), "http://localhost:5001/docs");
    model.host_port = None;
    project.state.components.chap_core.enabled = true;
    assert!(
        url(&project, &model)
            .unwrap()
            .ends_with("/v2/services/chapkit-ewars-model/run/docs")
    );
    project.state.components.chap_core.enabled = false;
    let err = url(&project, &model).unwrap_err().to_string();
    assert!(
        err.contains("run `chap-devops models expose chapkit_ewars_model`"),
        "{err}"
    );
}

/// The three answers about the container each say something different, and
/// only the one that knows nothing is running names `chap-devops up`.
#[test]
fn the_container_note_says_which_of_the_three_it_is() {
    assert_eq!(running_note(Running::Yes, Component::Dhis2), None);
    let no = running_note(Running::No, Component::Dhis2).expect("a note");
    assert!(no.contains("no dhis2 container is running"), "{no}");
    assert!(no.contains("run `chap-devops up`"), "{no}");
    let unknown = running_note(Running::Unknown, Component::Ocs).expect("a note");
    assert!(unknown.contains("docker could not be asked"), "{unknown}");
    assert!(unknown.contains("run `chap-devops doctor`"), "{unknown}");
    assert!(
        !unknown.contains("chap-devops up"),
        "nothing was learned, so nothing is prescribed: {unknown}"
    );
}

/// chap-core's service is `chap`, so the note has to name the container and
/// not the component.
#[test]
fn the_container_note_names_the_service_not_the_component() {
    let note = running_note(Running::No, Component::ChapCore).expect("a note");
    assert!(note.contains("no chap container is running"), "{note}");
}

/// The proxy note only appears for the instance that has no port of its own,
/// and it names the way to publish one.
#[test]
fn the_proxy_note_is_only_for_an_unpublished_instance() {
    assert_eq!(proxy_note(false, Component::Ocs), None);
    let note = proxy_note(true, Component::Ocs).expect("a note");
    assert!(
        note.contains("`chap-devops components enable ocs --port N`"),
        "{note}"
    );
}

/// The listing names every component, with an address for the ones that
/// have one and the reason for the ones that do not.
#[test]
fn the_listing_covers_every_component_with_its_reason() {
    let components = Components {
        dhis2: Dhis2Component {
            enabled: true,
            port: Some(18080),
            ..Dhis2Component::default()
        },
        ocs: OcsComponent {
            enabled: true,
            port: None,
            ..OcsComponent::default()
        },
        ..Components::default()
    };
    let rows: Vec<OpenRow> = Component::ALL
        .iter()
        .map(|component| {
            let (url, what) = match resolve(*component, &components, "http://localhost:18000") {
                Openable::Url { url, what, .. } => (Some(url), what.to_string()),
                Openable::Internal { inside } => (None, inside),
                Openable::NoWeb => (None, "no web interface".to_string()),
                Openable::Off => (None, "off".to_string()),
            };
            OpenRow {
                name: component.name().to_string(),
                url,
                what,
            }
        })
        .collect();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["chap-core", "ocs", "s3", "dhis2"]);
    assert_eq!(
        rows[0].url.as_deref(),
        Some("http://localhost:18000/docs"),
        "chap-core opens its API documentation"
    );
    assert_eq!(rows[1].url, None, "an unpublished OCS opens nothing");
    assert_eq!(rows[2].url, None, "the object store never opens");
    assert_eq!(rows[3].url.as_deref(), Some("http://localhost:18080"));
}

/// The closing line counts what can be opened, and says something else
/// entirely when nothing can.
#[test]
fn the_listing_closes_on_what_can_be_opened() {
    let row = |name: &str, url: Option<&str>| OpenRow {
        name: name.to_string(),
        url: url.map(str::to_string),
        what: String::new(),
    };
    let text = |report: &OpenListReport| {
        let mut lines = Report::default();
        say_list(report, &mut lines);
        lines.text()
    };
    let none = OpenListReport {
        components: vec![row("ocs", None), row("s3", None)],
        models: Vec::new(),
        unpublished: Vec::new(),
        proxy: false,
    };
    assert_eq!(
        text(&none),
        "nothing in this deployment has a web interface on this machine\n\
         hint: `chap-devops components list` says what it is made of\n"
    );

    let some = OpenListReport {
        components: vec![
            row("chap-core", Some("http://localhost:8000/docs")),
            row("s3", None),
        ],
        models: Vec::new(),
        unpublished: Vec::new(),
        proxy: true,
    };
    assert_eq!(
        text(&some),
        "1 of them can be opened: run `chap-devops open NAME`\n\
         hint: `chap-devops status` shows what is running\n"
    );
    let table = human_list(&some, &Out::detect(false, true));
    assert!(table.starts_with("COMPONENT"), "{table}");
}

/// The closing lines of one `chap-devops open NAME`.
fn said(report: &OpenReport) -> String {
    let mut lines = Report::default();
    say(report, &mut lines);
    lines.text()
}

/// A machine with no opener is told the address and not that something
/// failed, and the hint names the opener that is missing.
#[test]
fn no_opener_reports_the_address_rather_than_a_failure() {
    let report = OpenReport {
        name: "dhis2".to_string(),
        url: "http://localhost:18080".to_string(),
        page: crate::open::DHIS2_PAGE,
        opened: false,
        no_browser: false,
        running: Running::Yes,
        answering: Some(true),
        notes: Vec::new(),
    };
    let text = said(&report);
    assert!(
        text.starts_with("the DHIS2 user interface is at http://localhost:18080\n"),
        "{text}"
    );
    let (command, _) = crate::open::opener();
    assert!(
        text.contains(&format!(
            "hint: there is no `{command}` on this machine to open it with"
        )),
        "{text}"
    );
    assert!(!text.contains("warning:"), "{text}");
}

/// A run that opened something says so in one line; that it answered is a
/// hint.
#[test]
fn a_successful_open_reports_the_page_then_the_container() {
    let report = OpenReport {
        name: "dhis2".to_string(),
        url: "http://localhost:18080".to_string(),
        page: crate::open::DHIS2_PAGE,
        opened: true,
        no_browser: false,
        running: Running::Yes,
        answering: Some(true),
        notes: Vec::new(),
    };
    assert_eq!(
        said(&report),
        "opening the DHIS2 user interface at http://localhost:18080\n\
         hint: dhis2 answered at that address; `chap-devops status` reports the rest of this \
         deployment\n"
    );

    // A container that is up and not serving yet is not called answering.
    let starting = OpenReport {
        answering: Some(false),
        ..report
    };
    let text = said(&starting);
    assert!(
        text.contains("warning: the dhis2 container is running and did not answer yet"),
        "{text}"
    );
    assert!(
        text.contains("run `chap-devops status` in a moment"),
        "{text}"
    );
}

/// `--no-browser` says where the page is and nothing about an opener: none
/// was asked, so its absence is not worth a line.
#[test]
fn no_browser_names_the_page_and_no_opener() {
    let report = OpenReport {
        name: "ocs".to_string(),
        url: "http://localhost:8790".to_string(),
        page: crate::open::OCS_PAGE,
        opened: false,
        no_browser: true,
        running: Running::Yes,
        answering: Some(true),
        notes: Vec::new(),
    };
    let text = said(&report);
    assert!(
        text.starts_with("the OCS web interface is at http://localhost:8790\n"),
        "{text}"
    );
    assert!(!text.contains("opening"), "{text}");
    assert!(!text.contains("the whole of it"), "{text}");
}

/// Nothing about the container is claimed when docker said nothing, and the
/// warning is what stands in for the closing line.
#[test]
fn an_unknown_container_says_so_and_claims_nothing() {
    let report = OpenReport {
        name: "ocs".to_string(),
        url: "http://localhost:9000".to_string(),
        page: crate::open::OCS_PAGE,
        opened: true,
        no_browser: false,
        running: Running::Unknown,
        answering: None,
        notes: vec![Note::new(
            Weight::Warning,
            running_note(Running::Unknown, Component::Ocs).expect("a note"),
        )],
    };
    let text = said(&report);
    assert!(
        text.contains("warning: docker could not be asked"),
        "{text}"
    );
    assert!(
        !text.contains("container is running"),
        "nothing is claimed: {text}"
    );
}

/// Under `--json` a note is its text alone, as it was before the levels.
#[test]
fn a_note_serializes_as_its_text() {
    let note = Note::new(Weight::Hint, "a note".to_string());
    assert_eq!(
        serde_json::to_value(&note).unwrap(),
        serde_json::json!("a note")
    );
}

/// A deployment of models alone has something to open: each model that
/// publishes a host port, at its `/docs`. A model without one is named with
/// the way to publish one.
#[test]
fn the_listing_names_the_models_that_publish_a_host_port() {
    let row = |name: &str, url: Option<&str>| OpenRow {
        name: name.to_string(),
        url: url.map(str::to_string),
        what: "the model's API documentation".to_string(),
    };
    let report = OpenListReport {
        components: vec![row("chap-core", None), row("ocs", None)],
        models: vec![row(
            "chapkit_ewars_model",
            Some("http://localhost:5001/docs"),
        )],
        unpublished: vec!["auto_arima".to_string()],
        proxy: false,
    };
    let mut lines = Report::default();
    say_list(&report, &mut lines);
    assert_eq!(
        lines.text(),
        "1 of them can be opened: run `chap-devops open NAME`\n\
         hint: `chap-devops status` shows what is running\n\
         hint: auto_arima publishes no host port; run `chap-devops models expose auto_arima` to \
         publish one\n"
    );
    let table = human_list(&report, &Out::detect(false, true));
    assert!(table.contains("MODEL"), "{table}");
    assert!(table.contains("http://localhost:5001/docs"), "{table}");
}
