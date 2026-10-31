use crate::common::*;
use assert_cmd::Command;
use serde_json::Value as Json;
use serde_yaml_ng::Value as Yaml;
use std::path::Path;

/// `init` is hidden inside a deployment, and every way of reaching it there
/// still works: its help, the refusal that names `--force`, `--force` itself,
/// and a nested deployment in a subdirectory.
#[test]
fn init_still_runs_inside_a_project_where_the_help_hides_it() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();

    chap_in(&sandbox, &dir, &["init", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Create a deployment directory: compose files, .env and .chap-devops/",
        ))
        .stdout(predicates::str::contains("--force"));

    // Bare, it still refuses rather than overwriting what is there.
    chap_in(&sandbox, &dir, &["init"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "already contains a chap-devops deployment; choose another directory, or use --force to overwrite it",
        ));

    // `chap-devops init --api-port N --force` rewrites the project, but the
    // `CHAP_API_PORT` line the first init wrote into `.env` is what compose
    // publishes, and `init` says so rather than claiming the port moved.
    let port = port_base();
    let mut force = sandbox.chap();
    force
        .current_dir(&dir)
        .args(["init", "--force", "--models", "none", "--api-port"])
        .arg(port.to_string());
    force
        .assert()
        .success()
        .stderr(predicates::str::contains(format!(
            "so the API stays on 8700 rather than {port}; edit that line to move it"
        )));
    assert!(
        read(&dir.join(".chap-devops/project.yaml")).contains(&format!("api_port: {port}")),
        "--force rewrote the project"
    );
    // And the listing follows `.env`, like every other command does.
    chap_in(&sandbox, &dir, &["components", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("http://localhost:8700"));

    // And a nested deployment in a subdirectory still works, warning about
    // the one it is inside.
    let mut nested = sandbox.chap();
    nested
        .current_dir(&dir)
        .args(["init", "inner", "--models", "none"]);
    nested.assert().success().stderr(predicates::str::contains(
        "is inside the chap-devops deployment at",
    ));
    assert!(dir.join("inner/.chap-devops/project.yaml").is_file());
}

/// A group command with no subcommand is not a question its own help answers
/// outside a deployment: every subcommand it would list needs the project
/// that is missing, so that is what the reader is told instead.
#[test]
fn a_bare_group_command_outside_a_project_asks_for_a_project() {
    let sandbox = Sandbox::new();
    let missing = "is not a chap-devops deployment (no .chap-devops/project.yaml here or in a parent \
                   directory); run `chap-devops init` first";

    for group in ["components", "auth", "backup", "docker"] {
        chap_in(&sandbox, sandbox.home.path(), &[group])
            .assert()
            .code(1)
            .stderr(predicates::str::contains(missing));
    }

    // Word for word, and exit code for exit code, what the subcommands say.
    for args in [
        vec!["components", "list"],
        vec!["auth", "show"],
        vec!["jobs"],
    ] {
        chap_in(&sandbox, sandbox.home.path(), &args)
            .assert()
            .code(1)
            .stderr(predicates::str::contains(missing));
    }

    // `models` and `registry` read the catalogue, which works anywhere, so
    // their bare form is still their own help.
    for group in ["models", "registry"] {
        chap_in(&sandbox, sandbox.home.path(), &[group])
            .assert()
            .code(2)
            .stderr(predicates::str::contains("Usage: chap-devops"));
    }
    chap_in(&sandbox, sandbox.home.path(), &["models", "list"])
        .assert()
        .success();

    // Inside a deployment the group's help is the right answer again.
    sandbox.init(&["--models", "none"]).assert().success();
    chap_in(&sandbox, &sandbox.project(), &["components"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("Usage: chap-devops components"))
        .stderr(predicates::str::contains("enable"));
}

#[test]
fn the_help_says_what_chap_is() {
    let sandbox = Sandbox::new();

    // The header has to name the platform, not just chap-core: someone typing
    // `chap-devops --help` for the first time may not know what chap-core is.
    chap_in(&sandbox, sandbox.home.path(), &["--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "climate-informed disease forecasting",
        ));

    // The short help is the one-liner, and says the same thing.
    let short = chap_in(&sandbox, sandbox.home.path(), &["-h"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "deploy Chap (climate-informed disease forecasting) and its services",
        ))
        .get_output()
        .stdout
        .clone();

    // The same thing, exactly: there is no longer version to ask for.
    let long = chap_in(&sandbox, sandbox.home.path(), &["--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        String::from_utf8(short).expect("help is text"),
        String::from_utf8(long).expect("help is text"),
        "`chap-devops -h` and `chap-devops --help` print the same thing"
    );

    // And the whole of it fits on a screen, with the book one line away.
    let help = chap_in(&sandbox, sandbox.home.path(), &["--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8(help).expect("help is text");
    assert!(help.lines().count() <= 40, "{help}");
    assert!(
        help.trim_end()
            .ends_with("Docs: https://dhis2-chap.github.io/chap-devops/"),
        "{help}"
    );
}

/// The root help ends on a blank line, so the book's address is not flush
/// against the prompt. Clap cannot do it - it trims the rendered help and
/// appends one newline - so the binary writes the line itself, and this is
/// what keeps it from being dropped again.
#[test]
fn the_root_help_ends_on_a_blank_line() {
    let sandbox = Sandbox::new();
    let ends_blank = "Docs: https://dhis2-chap.github.io/chap-devops/\n\n";

    // Every way of asking the root for help, including the `help` subcommand
    // and a bare `chap-devops`, which answers with the same help on stderr.
    for argv in [vec!["--help"], vec!["-h"], vec!["help"]] {
        let help = chap_in(&sandbox, sandbox.home.path(), &argv)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let help = String::from_utf8(help).expect("help is text");
        assert!(
            help.ends_with(ends_blank),
            "`chap-devops {argv:?}`:\n{help:?}"
        );
    }
    // A bare `chap-devops` - no arguments at all, so not through `chap_in`, which
    // always passes `--offline` - answers with the same help on stderr.
    let mut bare = Command::cargo_bin("chap-devops").expect("the chap-devops binary is built");
    let bare = bare
        .env("CHAP_DEVOPS_CACHE_DIR", sandbox.cache.path())
        .env("CHAP_DEVOPS_DATA_DIR", sandbox.cache.path().join("data"))
        .current_dir(sandbox.home.path())
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();
    let bare = String::from_utf8(bare).expect("help is text");
    assert!(bare.ends_with(ends_blank), "bare `chap-devops`:\n{bare:?}");

    // The blank line belongs to the root help alone: a subcommand's help has
    // no docs line to separate from the prompt, and gains no blank line.
    for argv in [
        vec!["init", "--help"],
        vec!["models", "--help"],
        vec!["models", "list", "--help"],
        vec!["help", "doctor"],
    ] {
        let help = chap_in(&sandbox, sandbox.home.path(), &argv)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let help = String::from_utf8(help).expect("help is text");
        assert!(
            !help.ends_with("\n\n"),
            "`chap-devops {argv:?}` gained a blank line:\n{help:?}"
        );
    }
}

#[test]
fn init_inside_a_project_warns_about_the_parent() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox.init(&["--models", "none"]).assert().success();

    let mut nested = sandbox.chap();
    nested
        .arg("init")
        .arg(dir.join("inner"))
        .args(["--models", "none"]);
    nested.assert().success().stderr(predicates::str::contains(
        "is inside the chap-devops deployment at",
    ));
    assert!(dir.join("inner/.chap-devops/project.yaml").is_file());
}

/// The member names inside a `tar.gz`, via the same `tar` the CLI uses.
fn members(archive: &Path) -> Vec<String> {
    let out = std::process::Command::new("tar")
        .arg("-tzf")
        .arg(archive)
        .output()
        .expect("tar lists an archive");
    assert!(
        out.status.success(),
        "tar -tzf failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim_end_matches('/').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[test]
fn backup_create_packs_the_files_without_touching_docker() {
    let sandbox = Sandbox::new();
    let dir = sandbox.project();
    sandbox
        .init(&["--models", "chapkit_ewars_model"])
        .assert()
        .success();

    let out = sandbox.home.path().join("archives");
    std::fs::create_dir_all(&out).unwrap();
    let text = String::from_utf8(
        chap_in(
            &sandbox,
            &dir,
            &[
                "backup",
                "create",
                "--no-db",
                "--no-models",
                "--out",
                out.to_str().unwrap(),
            ],
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .expect("the report is text");

    let archive = only_archive(&out);
    let name = archive.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("chapx-") || name.starts_with("chap-devops-backup-chapx-"),
        "the archive is named after the project: {name}"
    );
    // Printed in its resolved form, which on macOS is the `/private` one.
    let printed = std::fs::canonicalize(&archive).unwrap();
    assert!(
        text.contains(&format!("wrote {}", printed.display())),
        "{text}"
    );
    // What was left out is a hint, shown only with -v.
    assert!(!text.contains("not included (--no-db)"));

    let members = members(&archive);
    assert!(members.contains(&"manifest.yaml".to_string()));
    for rel in [
        "files/.env",
        "files/.chap-devops/project.yaml",
        "files/.chap-devops/models.yaml",
        "files/compose.yml",
        "files/compose.marketplace.yml",
        "files/compose.chapkit-ewars-model.yml",
    ] {
        assert!(members.contains(&rel.to_string()), "{rel} is missing");
    }
    assert!(
        !members.iter().any(|m| m.starts_with("db/")),
        "--no-db leaves the dump out"
    );
    assert!(
        !members.iter().any(|m| m.starts_with("models/")),
        "--no-models leaves the volumes out"
    );
    // The staging directory is scratch space and is cleaned up after itself.
    assert!(!dir.join(".chap-devops/tmp").exists());

    // The manifest says what the archive is, without unpacking it.
    let manifest = std::process::Command::new("tar")
        .args(["-xzf", archive.to_str().unwrap(), "-O", "manifest.yaml"])
        .output()
        .unwrap();
    let manifest: Json =
        serde_json::to_value(serde_yaml_ng::from_slice::<Yaml>(&manifest.stdout).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["project"], "chapx");
    assert!(manifest["created_at"].as_str().unwrap().ends_with('Z'));
    assert!(manifest["database"].is_null());
    assert_eq!(manifest["models"][0]["service_id"], "chapkit-ewars-model");
    assert_eq!(manifest["models"][0]["skipped"], "--no-models");
}

/// A command that is not one is clap's business: it names what was typed,
/// suggests what was probably meant, and exits 2. `chap-devops` adds nothing of its
/// own, not even for the commands chap-core's developer CLI publishes.
#[test]
fn an_unknown_subcommand_is_claps_own_error() {
    let sandbox = Sandbox::new();
    sandbox
        .chap()
        .arg("serve")
        .assert()
        .code(2)
        .stderr(predicates::str::contains("unrecognized subcommand 'serve'"))
        .stderr(predicates::str::contains("--help"));

    // And a near miss of a real command gets clap's suggestion.
    sandbox
        .chap()
        .arg("stat")
        .assert()
        .code(2)
        .stderr(predicates::str::contains("similar subcommands"))
        .stderr(predicates::str::contains("status"));
}

#[test]
fn verbose_narrates_on_stderr_and_leaves_stdout_alone() {
    let sandbox = Sandbox::new();
    let quiet = sandbox
        .chap()
        .args(["models", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let loud = sandbox
        .chap()
        .args(["-vv", "models", "list"])
        .assert()
        .success()
        .get_output()
        .clone();

    assert_eq!(
        String::from_utf8(quiet).expect("utf-8"),
        String::from_utf8(loud.stdout).expect("utf-8"),
        "-vv must never reach stdout"
    );
    let stderr = String::from_utf8(loud.stderr).expect("utf-8 stderr");
    assert!(
        stderr.contains("registry:"),
        "-vv says which catalogue was used: {stderr}"
    );
    assert!(
        stderr.contains("embedded") || stderr.contains("cache"),
        "-vv names the source it chose: {stderr}"
    );
}

#[test]
fn debug_implies_verbose_and_adds_the_resolved_project() {
    let sandbox = Sandbox::new();
    sandbox.init(&["--models", "none"]).assert().success();

    let out = sandbox
        .chap()
        .arg("-d")
        .arg("-C")
        .arg(sandbox.project())
        .args(["sync", "--check"])
        .assert()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(out).expect("utf-8 stderr");
    // -vv's half: the files the sync compared.
    assert!(stderr.contains("compared"), "{stderr}");
    // -d's own half: where the state it read actually lives.
    assert!(stderr.contains("deployment:"), "{stderr}");
    let state_file = Path::new(".chap-devops").join("project.yaml");
    assert!(
        stderr.contains(&state_file.display().to_string()),
        "{stderr}"
    );

    // The long spellings do the same.
    sandbox
        .chap()
        .arg("--debug")
        .arg("-C")
        .arg(sandbox.project())
        .args(["sync", "--check"])
        .assert()
        .success()
        .stderr(predicates::str::contains("compared"));
}

#[test]
fn self_version_says_what_this_build_is() {
    let (_cache, mut cmd) = bare();
    let out = cmd
        .args(["self", "version"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).expect("utf-8");

    assert!(
        text.contains(env!("CARGO_PKG_VERSION")),
        "the version is in there: {text}"
    );
    assert!(text.contains("target"), "{text}");
    assert!(text.contains("path"), "{text}");
    assert!(text.contains("installed by"), "{text}");
}

#[test]
fn self_version_json_carries_the_fields_a_bug_report_needs() {
    let (_cache, mut cmd) = bare();
    let out = cmd
        .args(["--json", "self", "version"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Json = serde_json::from_slice(&out).expect("--json is JSON");

    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    assert!(
        !value["target"]
            .as_str()
            .expect("a target triple")
            .is_empty(),
        "{value}"
    );
    assert!(value["path"].as_str().is_some(), "{value}");
    assert!(
        [
            "release archive",
            "cargo install",
            "cargo build",
            "local build"
        ]
        .contains(&value["install_method"].as_str().expect("a method")),
        "{value}"
    );
}

/// Through the `cdx` link, `self version` names the `chap-devops` file the link
/// points at, which is the file `self update` replaces.
#[cfg(unix)]
#[test]
fn self_version_through_the_short_form_names_the_real_binary() {
    let (cache, _) = bare();
    let real =
        std::fs::canonicalize(assert_cmd::cargo::cargo_bin("chap-devops")).expect("the binary");
    let link = cache.path().join("cdx");
    std::os::unix::fs::symlink(&real, &link).expect("a cdx link");
    let out = std::process::Command::new(&link)
        .args(["--json", "self", "version"])
        .env("CHAP_DEVOPS_CACHE_DIR", cache.path())
        .env("CHAP_DEVOPS_NO_UPDATE_CHECK", "1")
        .output()
        .expect("cdx runs");
    assert!(out.status.success(), "{out:?}");
    let value: Json = serde_json::from_slice(&out.stdout).expect("--json is JSON");
    assert_eq!(
        std::path::PathBuf::from(value["path"].as_str().expect("a path")),
        real,
        "{value}"
    );
    // A test binary is a local build in `target/`, never a release archive.
    assert_eq!(value["install_method"], "cargo build", "{value}");
}

/// The whole point of `--offline` is that nothing touches the network, and an
/// update is a download, so the two cannot be combined. This is also what
/// keeps the test suite off the network: nothing else in it runs `self
/// update`.
#[test]
fn self_update_refuses_to_run_offline() {
    let (_cache, mut cmd) = bare();
    cmd.args(["--offline", "self", "update", "--check"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--offline"));
}

#[test]
fn self_works_outside_a_project_and_says_what_it_offers() {
    let (_cache, mut cmd) = bare();
    let out = cmd
        .args(["self", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8(out).expect("utf-8");
    assert!(help.contains("update"), "{help}");
    assert!(help.contains("version"), "{help}");

    // And `chap-devops self` on its own is a usage error, not a no-op.
    let (_cache, mut cmd) = bare();
    cmd.arg("self").assert().failure();
}

#[test]
fn completions_are_printed_for_every_shell() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let (_cache, mut cmd) = bare();
        let out = cmd
            .args(["completions", shell])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let script = String::from_utf8(out).unwrap_or_else(|e| panic!("{shell}: {e}"));

        assert!(!script.trim().is_empty(), "{shell} printed nothing");
        assert!(
            script.contains("chap-devops"),
            "{shell} does not name chap-devops"
        );
        // Generated from the live tree, so the newest commands are in there.
        assert!(script.contains("self"), "{shell} misses `self`");
        assert!(script.contains("init"), "{shell} misses `init`");
    }
}

#[test]
fn completions_rejects_a_shell_it_cannot_write_for() {
    let (_cache, mut cmd) = bare();
    cmd.args(["completions", "tcsh"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("possible values"));
}

/// Neither command needs a deployment directory, and both are listed in the
/// help you get outside one.
#[test]
fn the_help_outside_a_project_offers_self_and_completions() {
    let (_cache, mut cmd) = bare();
    let out = cmd
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8(out).expect("utf-8");
    assert!(help.contains("self"), "{help}");
    assert!(help.contains("completions"), "{help}");
}

/// The update notice is a courtesy on a terminal. The test suite is not one,
/// and `assert_cmd` captures stdout, so no command in it may reach the
/// release feed or write the check file.
#[test]
fn a_captured_run_never_checks_for_an_update() {
    let sandbox = Sandbox::new();
    sandbox.init(&["--models", "none"]).assert().success();
    sandbox.chap().args(["models", "list"]).assert().success();

    let check = sandbox.cache.path().join("self-update-check.json");
    assert!(
        !check.exists(),
        "a piped run wrote {}; the notice must be terminal-only",
        check.display()
    );
}

/// `doctor` is one of the commands that works anywhere, so outside a project
/// it runs the machine half and says where the rest would be.
///
/// The exit code depends on whether this machine has Docker, which a test
/// cannot assume either way: only the structure is asserted, and that the
/// command stayed inside the two codes it is allowed to use.
#[test]
fn doctor_outside_a_project_checks_the_machine_and_says_so() {
    let (_cache, mut cmd) = bare();
    let out = cmd
        .args(["--offline", "-v", "doctor"])
        .output()
        .expect("doctor runs");
    assert!(
        matches!(out.status.code(), Some(0) | Some(1)),
        "doctor exited with {:?}",
        out.status.code()
    );
    let text = String::from_utf8(out.stdout).expect("utf-8");

    for name in [
        "docker cli",
        "docker daemon",
        "docker compose",
        "os and arch",
        "disk space",
        "network ghcr.io",
        "github api",
        "chap-devops",
    ] {
        assert!(text.contains(name), "`{name}` is missing from:\n{text}");
    }
    assert!(
        text.contains("hint: no deployment here, so the deployment checks did not run"),
        "{text}"
    );
    // Nothing that needs a deployment ran.
    for name in ["deployment files", "api port", "chap-core pin"] {
        assert!(!text.contains(name), "`{name}` needs a project:\n{text}");
    }
    let summary = regex::Regex::new(r"(?m)^\d+ checks: \d+ ok, \d+ warn, \d+ fail").unwrap();
    assert!(summary.is_match(&text), "no summary line in:\n{text}");
}
