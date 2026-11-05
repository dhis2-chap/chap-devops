use super::files::{Snapshot, describe};
use super::plan::*;
use crate::cli::ChapImage;
use std::path::{Path, PathBuf};

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string()).collect()
}

#[test]
fn the_model_name_is_read_in_both_spellings() {
    let args = strings(&["eval", "--model-name", "http://m:8000", "--x", "1"]);
    assert_eq!(model_name(&args), Some("http://m:8000"));
    let args = strings(&["eval", "--model-name=./my_model"]);
    assert_eq!(model_name(&args), Some("./my_model"));
    assert_eq!(model_name(&strings(&["plot-backtest", "x.nc"])), None);
}

#[test]
fn a_model_name_is_a_chapkit_service_a_repository_or_a_directory() {
    assert_eq!(model_kind(None), ModelKind::None);
    assert_eq!(
        model_kind(Some("builtin:seasonal_median@1.0.0")),
        ModelKind::Builtin("builtin:seasonal_median@1.0.0".to_string())
    );
    assert_eq!(
        model_kind(Some("http://auto-arima:8000")),
        ModelKind::Chapkit("http://auto-arima:8000".to_string())
    );
    assert_eq!(
        model_kind(Some("https://github.com/dhis2-chap/Minimalist_R@a1b2c3")),
        ModelKind::GitHub {
            owner: "dhis2-chap".to_string(),
            repo: "Minimalist_R".to_string(),
            git_ref: Some("a1b2c3".to_string()),
        }
    );
    assert_eq!(
        model_kind(Some("https://www.github.com/o/r.git/")),
        ModelKind::GitHub {
            owner: "o".to_string(),
            repo: "r".to_string(),
            git_ref: None,
        }
    );
    assert_eq!(
        model_kind(Some("models/ewars")),
        ModelKind::Local(PathBuf::from("models/ewars"))
    );
}

#[test]
fn a_built_in_model_is_not_a_model_id() {
    let kind = model_kind(Some("builtin:persistence"));
    assert_eq!(id_candidate(&kind, false), None);
}

#[test]
fn as_chapkit_is_read_as_a_flag_or_with_a_value() {
    assert!(as_chapkit(&strings(&["eval", "--run-config.as-chapkit"])));
    assert!(as_chapkit(&strings(&[
        "eval",
        "--run-config.as-chapkit=True"
    ])));
    assert!(!as_chapkit(&strings(&[
        "eval",
        "--run-config.as-chapkit=false"
    ])));
    assert!(!as_chapkit(&strings(&["eval", "--model-name", "m"])));
}

#[test]
fn only_the_models_that_are_not_chapkit_services_need_the_worker_image() {
    let repo = model_kind(Some("https://github.com/o/r"));
    let chapkit = model_kind(Some("http://m:8000"));
    // Nothing here: the smaller download.
    assert_eq!(image_for(&ModelKind::None, false, false), ChapImage::Core);
    assert_eq!(image_for(&chapkit, false, false), ChapImage::Core);
    let builtin = model_kind(Some("builtin:persistence"));
    assert_eq!(image_for(&builtin, false, false), ChapImage::Core);
    assert_eq!(image_for(&builtin, false, true), ChapImage::Worker);
    // An R model needs the worker image, whatever is here.
    assert_eq!(image_for(&repo, true, false), ChapImage::Worker);
    assert_eq!(
        image_for(&ModelKind::Local("m".into()), true, true),
        ChapImage::Worker
    );
    // Only the worker image here, as in a deployment that runs chap-core:
    // it does the rest too, and nothing is downloaded.
    assert_eq!(image_for(&chapkit, false, true), ChapImage::Worker);
    assert_eq!(image_for(&ModelKind::None, false, true), ChapImage::Worker);
    // Both here: the core image, wherever the run is. A deployment without
    // chap-core no longer pulls the 12 GB worker image for `--help`.
    assert_eq!(image_for(&ModelKind::None, true, true), ChapImage::Core);
    assert_eq!(image_for(&ModelKind::None, true, false), ChapImage::Core);
}

#[test]
fn docker_env_is_found_in_an_mlproject_and_only_there() {
    let docker = "name: ewars\n\ndocker_env:\n  image: ivargr/r_inla:latest\n";
    assert!(uses_docker_env(docker));
    assert!(!uses_docker_env("name: x\nrenv_env: renv.lock\n"));
    assert!(!uses_docker_env(
        "name: x\n# docker_env: no longer\nuv_env: pyproject.toml\n"
    ));
}

#[test]
fn the_newest_tag_is_the_highest_release() {
    let tags = strings(&["master", "v2.3.1", "v2.10.0", "latest", "v2.9.9"]);
    assert_eq!(newest_tag(&tags).as_deref(), Some("v2.10.0"));
    assert_eq!(
        newest_tag(&strings(&["master", "latest"])).as_deref(),
        Some("master")
    );
    assert_eq!(newest_tag(&[]), None);
}

// Unix paths: the Windows builds ship untested.
#[cfg(unix)]
#[test]
fn only_paths_outside_the_current_directory_need_a_mount() {
    let cwd = Path::new("/home/u/work");
    let args = strings(&[
        "eval",
        "--dataset-csv",
        "data.csv",
        "--output-file",
        "/data/out/x.nc",
        "--plot-file=../reports/x.html",
        "--model-name",
        "https://github.com/o/r",
        "./inside/a.nc",
        "/home/u/work/also-inside.nc",
    ]);
    assert_eq!(
        outside_paths(&args, cwd),
        vec![
            PathBuf::from("/data/out/x.nc"),
            PathBuf::from("/home/u/reports/x.html"),
        ]
    );
}

// Unix paths: the Windows builds ship untested.
#[cfg(unix)]
#[test]
fn the_nearest_existing_directory_is_mounted_once() {
    let exists = |path: &Path| {
        [
            "/data",
            "/data/out",
            "/home",
            "/home/u",
            "/home/u/work",
            "/srv",
        ]
        .iter()
        .any(|dir| path == Path::new(dir))
    };
    let paths = vec![
        PathBuf::from("/data/out/x.nc"),
        PathBuf::from("/data/out/new/y.nc"),
        PathBuf::from("/data/z.csv"),
        PathBuf::from("/home/u/work/sub/a.nc"),
        PathBuf::from("/srv/missing/deep/b.nc"),
    ];
    let covered = vec![PathBuf::from("/home/u/work")];
    assert_eq!(
        mount_dirs(&paths, &covered, exists).unwrap(),
        vec![PathBuf::from("/data"), PathBuf::from("/srv")]
    );
}

// Unix paths: the Windows builds ship untested.
#[cfg(unix)]
#[test]
fn the_root_directory_is_never_mounted() {
    let none_exist = |path: &Path| path == Path::new("/");
    let err = mount_dirs(&[PathBuf::from("/nowhere/x.nc")], &[], none_exist).unwrap_err();
    assert_eq!(err, PathBuf::from("/nowhere/x.nc"));
}

fn spec() -> RunSpec {
    RunSpec {
        image: "ghcr.io/dhis2-chap/chap-core:v2.3.1".to_string(),
        cwd: PathBuf::from("/home/u/work"),
        mounts: vec![PathBuf::from("/data")],
        data: PathBuf::from("/home/u/.local/share/chap-devops/chap"),
        user: Some((1000, 1000)),
        network: None,
        docker: None,
        tty: false,
        program: "chap",
        args: strings(&["eval", "--model-name", "http://m:8000"]),
    }
}

/// The value after `flag`, every time it occurs.
fn values<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
        .collect()
}

// Unix paths: the Windows builds ship untested.
#[cfg(unix)]
#[test]
fn a_run_mounts_every_directory_at_the_same_path_and_runs_as_the_user() {
    let args = docker_args(&spec());
    assert_eq!(args[..3], strings(&["run", "--rm", "-i"])[..]);
    assert!(!args.contains(&"-t".to_string()));
    assert_eq!(values(&args, "--user"), ["1000:1000"]);
    assert_eq!(values(&args, "--platform"), ["linux/amd64"]);
    assert_eq!(
        values(&args, "-v"),
        [
            "/home/u/work:/home/u/work",
            "/data:/data",
            "/home/u/.local/share/chap-devops/chap:/home/u/.local/share/chap-devops/chap",
        ]
    );
    assert_eq!(values(&args, "-w"), ["/home/u/work"]);
    let env = values(&args, "-e");
    assert!(env.contains(&"CHAP_RUNS_DIR=/home/u/.local/share/chap-devops/chap/runs"));
    assert!(env.contains(&"RENV_CONFIG_SANDBOX_ENABLED=FALSE"));
    assert!(values(&args, "--network").is_empty());
    assert_eq!(
        values(&args, "--add-host"),
        ["host.docker.internal:host-gateway"]
    );
    assert_eq!(
        args[args.len() - 5..],
        strings(&[
            "ghcr.io/dhis2-chap/chap-core:v2.3.1",
            "chap",
            "eval",
            "--model-name",
            "http://m:8000",
        ])[..]
    );
}

// Unix paths: the Windows builds ship untested.
#[cfg(unix)]
#[test]
fn a_terminal_a_network_and_the_socket_are_added_when_asked_for() {
    let mut spec = spec();
    spec.tty = true;
    spec.network = Some("demo-ab12cd_default".to_string());
    spec.docker = Some(Some(0));
    spec.user = None;
    let args = docker_args(&spec);
    assert!(args.contains(&"-t".to_string()));
    assert_eq!(values(&args, "--network"), ["demo-ab12cd_default"]);
    assert!(values(&args, "-v").contains(&"/var/run/docker.sock:/var/run/docker.sock"));
    assert_eq!(values(&args, "--group-add"), ["0"]);
    assert!(values(&args, "--user").is_empty());

    // A socket whose group could not be read is still mounted.
    spec.docker = Some(None);
    assert!(values(&docker_args(&spec), "--group-add").is_empty());
}

#[test]
fn no_arguments_ask_chap_for_its_help() {
    let mut spec = spec();
    spec.args = Vec::new();
    let args = docker_args(&spec);
    assert_eq!(args[args.len() - 2..], strings(&["chap", "--help"])[..]);
}

#[test]
fn the_written_files_are_the_new_and_the_changed_ones() {
    let before = Snapshot::from_entries(&[("/w/data.csv", 10), ("/w/old.nc", 10)]);
    let after =
        Snapshot::from_entries(&[("/w/data.csv", 10), ("/w/old.nc", 20), ("/w/new.html", 20)]);
    assert_eq!(
        before.written(&after),
        vec![PathBuf::from("/w/new.html"), PathBuf::from("/w/old.nc")]
    );
    assert!(after.written(&after).is_empty());
}

// Unix paths: the Windows builds ship untested.
#[cfg(unix)]
#[test]
fn the_written_files_are_named_relative_to_the_current_directory() {
    let written = vec![
        PathBuf::from("/w/a.nc"),
        PathBuf::from("/w/out/b.html"),
        PathBuf::from("/data/c.csv"),
    ];
    assert_eq!(
        describe(&written, Path::new("/w"), 8),
        "a.nc, out/b.html, /data/c.csv"
    );
    assert_eq!(describe(&written, Path::new("/w"), 1), "a.nc, and 2 more");
}

#[test]
fn a_url_on_this_machine_is_found_with_its_port() {
    assert_eq!(loopback_port("http://localhost:5001"), Some(5001));
    assert_eq!(loopback_port("http://127.0.0.1:8000/api"), Some(8000));
    assert_eq!(loopback_port("http://[::1]:5002"), Some(5002));
    assert_eq!(loopback_port("http://LOCALHOST"), Some(80));
    assert_eq!(loopback_port("http://auto-arima-chapkit:8000"), None);
    assert_eq!(loopback_port("https://models.example.org:8443"), None);
}

#[test]
fn a_run_only_for_help_is_known_as_such() {
    assert!(is_help(&[]));
    assert!(is_help(&strings(&["--help"])));
    assert!(is_help(&strings(&["eval", "-h"])));
    assert!(is_help(&strings(&["--version"])));
    assert!(!is_help(&strings(&["eval", "--model-name", "x"])));
}

#[test]
fn a_bare_word_that_is_no_file_is_a_model_id() {
    let word = model_kind(Some("chapkit_ewars_model"));
    assert_eq!(
        id_candidate(&word, false).as_deref(),
        Some("chapkit_ewars_model")
    );
    // A directory of that name wins, as it does for chap.
    assert_eq!(id_candidate(&word, true), None);
    assert_eq!(id_candidate(&model_kind(Some("./model")), false), None);
    assert_eq!(id_candidate(&model_kind(Some("models/m")), false), None);
    assert_eq!(
        id_candidate(&model_kind(Some("http://m:8000")), false),
        None
    );
}

#[test]
fn the_host_of_a_url_is_the_service_name() {
    assert_eq!(
        url_host("http://Chapkit-Ewars-Model:8000/").as_deref(),
        Some("chapkit-ewars-model")
    );
    assert_eq!(
        url_host("https://models.example.org").as_deref(),
        Some("models.example.org")
    );
    assert_eq!(url_host("not a url"), None);
}

#[test]
fn the_note_for_a_server_that_does_not_answer_fits_the_url() {
    let service = unreachable_message("http://nothing:8000");
    assert!(service.contains("the model server at http://nothing:8000 is not running"));
    assert!(service.contains("`nothing` is not a model of this deployment or group"));
    let outside = unreachable_message("https://models.example.org");
    assert!(outside.contains("check that the server runs"));
    assert!(!outside.contains("chap-devops run"));
}

#[test]
fn the_model_name_is_replaced_in_either_spelling() {
    let mut args = strings(&["eval", "--model-name", "chapkit_ewars_model", "--x"]);
    super::set_model_name(&mut args, "http://m:8000");
    assert_eq!(
        args,
        strings(&["eval", "--model-name", "http://m:8000", "--x"])
    );
    let mut args = strings(&["eval", "--model-name=chapkit_ewars_model"]);
    super::set_model_name(&mut args, "http://m:8000");
    assert_eq!(args, strings(&["eval", "--model-name=http://m:8000"]));
}

#[test]
fn the_output_files_are_read_in_both_spellings() {
    let args = strings(&[
        "eval",
        "--output-file",
        "out/a.nc",
        "--output-file=b.html",
        "x",
    ]);
    assert_eq!(output_files(&args), ["out/a.nc", "b.html"]);
    assert!(output_files(&strings(&["validate", "d.csv"])).is_empty());
    let simulate = strings(&["simulate", "--out-file", "sim/out.csv"]);
    assert_eq!(output_files(&simulate), ["sim/out.csv"]);
}

#[test]
fn plot_dataset_is_refused_because_it_needs_a_browser() {
    let why = needs_a_browser(&strings(&["plot-dataset", "d.csv"])).unwrap();
    assert!(why.contains("no browser"));
    assert!(why.contains("plot-backtest"));
    assert!(needs_a_browser(&strings(&["plot-dataset", "--help"])).is_none());
    assert!(needs_a_browser(&strings(&["plot-backtest", "a.nc"])).is_none());
}

#[test]
fn what_needs_v2_5_is_refused_at_an_older_release_only() {
    let builtin = strings(&["eval", "--model-name", "builtin:seasonal_median"]);
    let kind = model_kind(model_name(&builtin));
    let why = too_old(&builtin, &kind, "v2.4.0").expect("refused");
    assert!(
        why.starts_with("built-in models need chap-core v2.5.0"),
        "{why}"
    );
    assert!(why.contains("this run uses v2.4.0"), "{why}");
    assert!(too_old(&builtin, &kind, "v2.5.0").is_none());
    assert!(too_old(&builtin, &kind, "master").is_none());

    let simulate = strings(&["simulate", "--model-name", "./m", "--out-file", "s.csv"]);
    let kind = model_kind(model_name(&simulate));
    assert!(
        too_old(&simulate, &kind, "v2.4.0")
            .unwrap()
            .starts_with("`chap simulate` needs")
    );

    let as_chapkit = strings(&["eval", "--model-name", "./m", "--run-config.as-chapkit"]);
    let kind = model_kind(model_name(&as_chapkit));
    assert!(too_old(&as_chapkit, &kind, "v2.3.1").is_some());

    let plain = strings(&["eval", "--model-name", "./m"]);
    let kind = model_kind(model_name(&plain));
    assert!(too_old(&plain, &kind, "v2.3.1").is_none());
}

/// Only a moving tag that `--tag` named is pulled first, and never offline.
#[test]
fn a_moving_tag_from_the_flag_is_pulled_first() {
    for tag in ["master", "dev", "latest"] {
        assert!(pulls_first(Some(tag), false), "{tag}");
        assert!(!pulls_first(Some(tag), true), "{tag} offline");
    }
    assert!(!pulls_first(Some("v2.6.0"), false));
    assert!(!pulls_first(None, false));
}
