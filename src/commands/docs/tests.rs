use super::*;

#[test]
fn every_command_path_gets_a_section() {
    let out = reference();
    for path in [
        "## chap-devops",
        "## chap-devops init",
        "## chap-devops models",
        "## chap-devops models enable",
        "## chap-devops models unexpose",
        "## chap-devops registry list",
        "## chap-devops docker run",
        "## chap-devops backup restore",
        "## chap-devops status",
        "## chap-devops update",
    ] {
        assert!(out.contains(path), "{path} is missing from the reference");
    }
}

#[test]
fn the_project_only_commands_are_documented_even_though_help_hides_them() {
    let out = reference();
    for path in [
        "## chap-devops up",
        "## chap-devops down",
        "## chap-devops logs",
        "## chap-devops ui",
    ] {
        assert!(out.contains(path), "{path} is missing from the reference");
    }
}

#[test]
fn clap_s_own_commands_and_the_generator_itself_stay_out() {
    let out = reference();
    assert!(!out.contains("## chap-devops help"));
    assert!(!out.contains("## chap-devops docs-markdown"));
    assert!(!out.contains("| `-h, --help` |"));
    // `chap-devops models enable --version` is not clap's `-V` and must stay.
    assert!(out.contains("| `--version <VERSION>` |"), "{out}");
}

#[test]
fn the_globals_are_listed_once_under_the_root() {
    let out = reference();
    assert_eq!(out.matches("| `-C, --project-dir <DIR>` |").count(), 1);
    assert_eq!(out.matches("| `--json` |").count(), 1);
    assert_eq!(out.matches("| Global option | Description |").count(), 1);
}

#[test]
fn usage_names_the_full_command_path() {
    let out = reference();
    assert!(
        out.contains("Usage: chap-devops models enable [OPTIONS] <ID>"),
        "{out}"
    );
}

#[test]
fn a_table_cell_never_ends_early() {
    let out = reference();
    for line in out.lines().filter(|line| line.starts_with("| `")) {
        assert_eq!(
            line.replace("\\|", "").matches('|').count(),
            3,
            "an unescaped pipe splits this row: {line}"
        );
    }
    // `--port PORT|auto` is the row that needs the escaping.
    assert!(out.contains("--port <PORT\\|auto>"), "{out}");
}

#[test]
fn help_texts_and_defaults_reach_the_table() {
    let out = reference();
    assert!(out.contains("Default: `none`"), "init --models none");
    assert!(out.contains("Default: `8700`"), "init --api-port 8700");
    assert!(
        out.contains("Never touch the network"),
        "the global --offline help"
    );
}

#[test]
fn rendering_is_deterministic() {
    assert_eq!(reference(), reference());
}
