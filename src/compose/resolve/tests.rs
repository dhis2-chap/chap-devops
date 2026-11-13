use super::*;

const IMAGE: &str = "ghcr.io/chap-models/chapkit_ewars_model";
const TAG: &str = "sha-fa880a1";

fn request<'a>(id: &'a str, user: Option<&'a str>) -> Request<'a> {
    Request {
        id,
        image: IMAGE,
        image_tag: TAG,
        image_digest: None,
        data_dir_flag: None,
        user_flag: user,
    }
}

/// Endpoints that reach nothing: the registry stub decides what is known.
fn endpoints() -> Endpoints {
    Endpoints {
        docker_probe: false,
        ..Endpoints::default()
    }
}

fn config(user: &str, working_dir: &str) -> ghcr::ImageConfig {
    ghcr::ImageConfig {
        user: user.to_string(),
        working_dir: working_dir.to_string(),
        amd64_only: false,
        arm64: false,
        command: Vec::new(),
    }
}

/// [`from_image_with`] against a registry that answers with one config.
fn resolve(
    req: &Request,
    endpoints: &Endpoints,
    declared: Option<ghcr::ImageConfig>,
) -> Resolution {
    from_image_with(
        req,
        endpoints,
        &|_, _| Ok(declared.clone()),
        &|_| None,
        &|_| None,
        &|_| true,
        &|_, _| None,
    )
}

#[test]
fn an_image_that_runs_as_root_resolves_to_root() {
    for declared in ["root", "0", "0:0", ""] {
        let resolution = resolve(
            &request("chapkit_rwanda_malaria_bym_model", None),
            &endpoints(),
            Some(config(declared, "/work")),
        );
        assert_eq!(resolution.user, "root", "{declared:?}");
        assert_eq!(resolution.user_from, UserSource::ImageConfig);
        assert_eq!(resolution.data_dir, "/work/data");
        assert!(resolution.notes.is_empty(), "{:?}", resolution.notes);
    }
}

#[test]
fn an_account_name_resolves_to_the_numbers_the_chown_uses() {
    let resolution = resolve(
        &request("chapkit_ewars_model", None),
        &endpoints(),
        Some(config("chapkit", "/app")),
    );
    assert_eq!(resolution.user, "1000:1000");
    assert_eq!(resolution.user_from, UserSource::ImageConfig);
    assert_eq!(resolution.data_dir, "/app/data");
    assert!(resolution.notes.is_empty(), "{:?}", resolution.notes);
}

#[test]
fn a_name_nothing_knows_is_kept_with_a_note() {
    let resolution = resolve(
        &request("chapkit_ewars_model", None),
        &endpoints(),
        Some(config("app", "/app")),
    );
    assert_eq!(resolution.user, "app");
    assert_eq!(resolution.user_from, UserSource::ImageConfig);
    assert_eq!(resolution.notes.len(), 1, "{:?}", resolution.notes);
    assert!(
        resolution.notes[0].contains("--user"),
        "{:?}",
        resolution.notes
    );

    // Unless the image itself can be asked, which is the docker probe.
    let probed = from_image_with(
        &request("chapkit_ewars_model", None),
        &Endpoints::default(),
        &|_, _| Ok(Some(config("app", "/app"))),
        &|_| None,
        &|_| None,
        &|_| true,
        &|_, name| (name == "app").then_some((10001, 10001)),
    );
    assert_eq!(probed.user, "10001:10001");
    assert_eq!(probed.user_from, UserSource::DockerProbe);
}

#[test]
fn the_flag_wins_over_the_image() {
    let resolution = resolve(
        &request("chapkit_ewars_model", Some("1500:1600")),
        &endpoints(),
        Some(config("root", "/app")),
    );
    assert_eq!(resolution.user, "1500:1600");
    assert_eq!(resolution.user_from, UserSource::Flag);
    // The data directory still follows the image; only the user was asked
    // for on the command line.
    assert_eq!(resolution.data_dir, "/app/data");
}

/// A registry that will not answer falls through to the local image, and
/// only then to the table - with a note saying which one was used.
#[test]
fn the_local_image_comes_before_the_table_and_the_table_says_so() {
    let local = from_image_with(
        &request("chapkit_rwanda_malaria_bym_model", None),
        &Endpoints::default(),
        &|_, _| Err(anyhow::anyhow!("no route to host")),
        &|_| Some(("root".to_string(), "/work".to_string())),
        &|_| None,
        &|_| true,
        &|_, _| None,
    );
    assert_eq!(local.user, "root");
    assert_eq!(local.user_from, UserSource::DockerProbe);
    assert_eq!(local.data_dir, "/work/data");
    assert_eq!(local.notes.len(), 1, "the registry failure is worth saying");

    let table = from_image_with(
        &request("chapkit_ewars_model", None),
        &Endpoints {
            offline: true,
            ..Endpoints::default()
        },
        &|_, _| panic!("--offline asks no registry"),
        &|_| None,
        &|_| None,
        &|_| true,
        &|_, _| None,
    );
    assert_eq!(table.user, "1000:1000", "the table's `chapkit`, as numbers");
    assert_eq!(table.user_from, UserSource::Table);
    assert_eq!(table.data_dir, "/app/data");
    assert_eq!(table.notes.len(), 1);
    assert!(
        table.notes[0].contains("built-in table"),
        "{:?}",
        table.notes
    );
}

/// A local image store that cannot answer for the amd64 variant is not an
/// image that runs as root: [`crate::docker::image_config`] says "unknown"
/// for an empty config, and this is what that costs - the table, plus the
/// note that says nothing could be asked.
#[test]
fn a_local_image_store_that_cannot_answer_does_not_mean_root() {
    let resolution = from_image_with(
        &request("chapkit_ewars_model", None),
        &Endpoints::default(),
        &|_, _| Err(anyhow::anyhow!("no route to host")),
        &|_| None,
        &|_| None,
        &|_| true,
        &|_, _| None,
    );
    assert_eq!(
        resolution.user, "1000:1000",
        "the table's `chapkit`, not root"
    );
    assert_eq!(resolution.user_from, UserSource::Table);
    assert_eq!(resolution.data_dir, "/app/data");
    assert_eq!(resolution.notes.len(), 2, "{:?}", resolution.notes);
    assert!(
        resolution.notes[1].contains("nothing could say what"),
        "{:?}",
        resolution.notes
    );
}

#[test]
fn the_table_resolver_looks_nothing_up() {
    let resolution = from_table(&request("auto_arima_chapkit", None));
    assert_eq!(resolution.user, "root");
    assert_eq!(resolution.user_from, UserSource::Table);
    assert_eq!(resolution.data_dir, "/work/data");
    assert!(resolution.notes.is_empty());

    // A model the table has never heard of takes the chapkit defaults.
    let unknown = from_table(&request("something_else", None));
    assert_eq!(unknown.user, "1000:1000");
    assert_eq!(unknown.data_dir, overrides::DEFAULT_DATA_DIR);

    // And the flag still wins.
    let explicit = from_table(&request("chapkit_ewars_model", Some("0:0")));
    assert_eq!(explicit.user, "root", "0:0 is root, written the one way");
    assert_eq!(explicit.user_from, UserSource::Flag);
}

#[test]
fn the_repository_is_the_image_without_the_registry_host() {
    assert_eq!(
        request("x", None).repository(),
        "chap-models/chapkit_ewars_model"
    );
    assert_eq!(
        request("x", None).reference(),
        "ghcr.io/chap-models/chapkit_ewars_model:sha-fa880a1"
    );
}

#[test]
fn the_sources_read_as_labels() {
    assert_eq!(UserSource::Flag.label(), "--user");
    assert_eq!(UserSource::ImageConfig.label(), "image config");
    assert_eq!(UserSource::DockerProbe.label(), "docker probe");
    assert_eq!(UserSource::Table.label(), "table");
    assert_eq!(UserSource::default(), UserSource::Table);
}

#[test]
fn a_command_that_names_a_port_fixes_it_and_any_other_reads_port() {
    let words = |text: &str| -> Vec<String> { text.split(' ').map(str::to_string).collect() };
    // The command every marketplace image but one starts with.
    assert!(!reads_port_env(&words(
        "uvicorn main:app --host 0.0.0.0 --port 8000"
    )));
    assert!(!reads_port_env(&words("uvicorn main:app --port=8000")));
    // A shell form keeps the whole line in one part.
    assert!(!reads_port_env(&[
        "/bin/sh".to_string(),
        "-c".to_string(),
        "uvicorn main:app --port 8000".to_string(),
    ]));
    // The simple multistep model: tini, then a module that reads PORT.
    assert!(reads_port_env(&words(
        "/usr/bin/tini -- python -m chapkit_simple_multistep_model"
    )));
    // A command nobody could read is the common case: a fixed 8000.
    assert!(!reads_port_env(&[]));
}

/// With a digest, the registry is asked for the digest, which is what runs,
/// and the local lookup uses the reference with both.
#[test]
fn a_digest_is_what_the_lookups_ask_for() {
    let digest = format!("sha256:{}", "b".repeat(64));
    let req = Request {
        image_digest: Some(&digest),
        ..request("chapkit_ewars_model", None)
    };
    let asked = std::cell::RefCell::new(Vec::new());
    from_image_with(
        &req,
        &endpoints(),
        &|_, reference| {
            asked.borrow_mut().push(reference.to_string());
            Ok(Some(config("root", "/work")))
        },
        &|_| None,
        &|_| None,
        &|_| true,
        &|_, _| None,
    );
    assert_eq!(asked.into_inner(), vec![digest.clone()]);
    assert_eq!(req.reference(), format!("{IMAGE}:{TAG}@{digest}"));
}

/// Only the registry lists the platforms, so only its answer can drop the
/// amd64 pin.
#[test]
fn arm64_comes_from_the_registry_alone() {
    let multi = ghcr::ImageConfig {
        arm64: true,
        ..config("chapkit", "/app")
    };
    let resolution = resolve(
        &request("chapkit_ewars_model", None),
        &endpoints(),
        Some(multi),
    );
    assert!(resolution.arm64);

    // A local image has no index to read, so it says nothing about arm64.
    let local = from_image_with(
        &request("chapkit_ewars_model", None),
        &Endpoints {
            offline: true,
            ..Endpoints::default()
        },
        &|_, _| panic!("offline asks no registry"),
        &|_| Some(("chapkit".to_string(), "/app".to_string())),
        &|_| None,
        &|_| true,
        &|_, _| None,
    );
    assert_eq!(local.user_from, UserSource::DockerProbe);
    assert!(!local.arm64);
}
