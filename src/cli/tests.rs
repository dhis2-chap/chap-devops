use super::*;
use clap::{CommandFactory, FromArgMatches};

#[test]
fn command_tree_is_valid() {
    Cli::command().debug_assert();
    command().debug_assert();
}

/// The help line and the URL the browser opens must not drift apart.
#[test]
fn the_help_line_points_at_the_documentation_url() {
    assert!(DOCS_LINE.ends_with(DOCS_URL), "{DOCS_LINE} vs {DOCS_URL}");
    assert!(DOCS_URL.ends_with('/'), "a chapter is appended to it");
}

/// Why the blank line at the end of `chap-devops --help` is not in `after_help`:
/// clap trims the rendered help and appends exactly one newline, so the
/// docs line is the last thing it will print whatever `after_help` ends
/// with. `main` adds the blank line where the help is printed instead, and
/// `tests/cli/help.rs` pins what the binary then prints.
#[test]
fn clap_renders_the_help_flush_against_the_docs_line() {
    for rendered in [
        Cli::command()
            .after_help(format!("{DOCS_LINE}\n"))
            .render_help(),
        Cli::command()
            .after_help(format!("{DOCS_LINE}\n\n"))
            .render_long_help(),
    ] {
        let help = rendered.to_string();
        assert!(
            help.ends_with(&format!("{DOCS_LINE}\n")),
            "clap no longer trims `after_help`; the blank line may belong there again:\n{help}"
        );
    }
}

/// Asking for help is not asking for the manual: the explanations live in
/// the book, so no command's help may grow into a page and no option's
/// help into a paragraph. The whole tree is walked, so a command added
/// later is held to the same limits.
///
/// The count is taken from `render_help`, which is what both `-h` and
/// `--help` print now that nothing in the tree has a `long_about` or a
/// `long_help`. `render_long_help` is clap's expanded layout - every
/// default value on a line of its own - and is measured only for the help
/// texts it carries, which are the same ones.
#[test]
fn no_help_is_longer_than_a_screen_and_no_option_longer_than_a_clause() {
    /// One screen of `--help`, and one clause of option help.
    ///
    /// The root listing is the only thing near the line: it grows by one
    /// row per top-level command, and there are now thirty of them,
    /// and it lists both option blocks: the global and the registry ones.
    /// Outside a deployment the listing is shorter still, because the
    /// commands that need a project are hidden from it.
    const MAX_LINES: usize = 54;
    const MAX_CHARS: usize = 90;

    fn check(command: &mut clap::Command, path: &str) {
        let help = command.render_help().to_string();
        let lines = help.lines().count();
        assert!(
            lines <= MAX_LINES,
            "`{path} --help` is {lines} lines, over {MAX_LINES}:\n{help}"
        );
        assert!(
            command.get_long_about().is_none(),
            "`{path}` has a long_about; the chapters carry the prose"
        );
        // The long rendering says the same thing, in clap's roomier
        // layout: nothing is written twice, at two lengths.
        let long = command.render_long_help().to_string();
        for arg in command.get_arguments() {
            let text = arg
                .get_help()
                .map(|help| help.to_string())
                .unwrap_or_default();
            let chars = text.chars().count();
            assert!(
                chars <= MAX_CHARS,
                "`{path}` {}: {chars} characters of help, over {MAX_CHARS}: {text}",
                arg.get_id()
            );
            assert!(
                arg.get_long_help().is_none(),
                "`{path}` {} has a long_help",
                arg.get_id()
            );
            assert!(
                text.is_empty() || arg.is_hide_set() || long.contains(&text),
                "`{path}` {} is missing from the long help",
                arg.get_id()
            );
        }

        let names: Vec<String> = command
            .get_subcommands()
            .map(|sub| sub.get_name().to_string())
            .collect();
        for name in names {
            let path = format!("{path} {name}");
            let sub = command
                .find_subcommand_mut(&name)
                .expect("just listed as a subcommand");
            check(sub, &path);
        }
    }

    let mut root = command();
    // The globals only reach the subcommands, and the usage lines only
    // exist, once clap has built the tree.
    root.build();
    check(&mut root, "chap-devops");
}

/// Both spellings of help say the same thing, rather than a short one and
/// a long one that drift apart. Clap's two renderings differ only in
/// layout - the long one gives every default a line of its own - so the
/// words have to match exactly.
#[test]
fn the_long_help_is_the_short_help() {
    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    let short = Cli::command().render_help().to_string();
    let long = Cli::command().render_long_help().to_string();
    assert!(Cli::command().get_long_about().is_none(), "{long}");
    assert!(short.starts_with(ABOUT), "{short}");
    assert!(long.starts_with(ABOUT), "{long}");
    assert_eq!(words(&short), words(&long));
}

#[test]
fn global_defaults() {
    let cli = Cli::try_parse_from(["chap", "status"]).unwrap();
    assert!(!cli.json);
    assert!(!cli.offline);
    assert_eq!(cli.project_dir, PathBuf::from("."));
    assert_eq!(cli.registry_url, DEFAULT_REGISTRY_URL);
    assert!(cli.cache_dir.is_none());
}

#[test]
fn globals_are_accepted_after_the_subcommand() {
    let cli = Cli::try_parse_from(["chap", "models", "list", "--json", "--offline"]).unwrap();
    assert!(cli.json);
    assert!(cli.offline);
}

#[test]
fn init_defaults() {
    let cli = Cli::try_parse_from(["chap", "init"]).unwrap();
    let Command::Init(args) = cli.command else {
        panic!("expected init");
    };
    assert_eq!(args.dir, PathBuf::from("."));
    assert_eq!(args.models, "none");
    assert_eq!(args.chap_tag, "latest");
    assert_eq!(args.api_port, 8700);
    assert_eq!(args.port_base, 5001);
    assert!(!args.force && !args.no_env && !args.fresh_env && !args.interactive);
    assert!(args.source.is_none());
    assert_eq!(args.api_token, None, "no flag, no authentication");

    let cli = Cli::try_parse_from(["chap", "init", "--api-port", "8123"]).unwrap();
    let Command::Init(args) = cli.command else {
        panic!("expected init");
    };
    assert_eq!(args.api_port, 8123);
}

/// The `InitArgs` behind `chap init <argv..>`.
fn init_args(argv: &[&str]) -> InitArgs {
    let mut args = vec!["chap", "init"];
    args.extend_from_slice(argv);
    let cli = Cli::try_parse_from(args).unwrap();
    let Command::Init(args) = cli.command else {
        panic!("expected init");
    };
    *args
}

#[test]
fn the_api_token_flag_takes_an_optional_value() {
    // Bare: the command generates one.
    assert_eq!(init_args(&["--api-token"]).api_token, Some(None));
    // With a value: used verbatim.
    assert_eq!(
        init_args(&["--api-token", "sekret"]).api_token,
        Some(Some("sekret".to_string()))
    );
    assert_eq!(
        init_args(&["--api-token=sekret"]).api_token,
        Some(Some("sekret".to_string()))
    );
    // And it still parses next to the positional directory.
    assert_eq!(
        init_args(&["mychap", "--api-token"]).dir,
        PathBuf::from("mychap")
    );

    // A file that is not written cannot hold a secret.
    assert!(Cli::try_parse_from(["chap", "init", "--api-token", "--no-env"]).is_err());
    assert!(Cli::try_parse_from(["chap", "init", "--no-env"]).is_ok());
}

/// The `AuthSub` behind `chap auth <argv..>`.
fn auth_sub(argv: &[&str]) -> AuthSub {
    let mut args = vec!["chap", "auth"];
    args.extend_from_slice(argv);
    let cli = Cli::try_parse_from(args).unwrap();
    let Command::Auth(a) = cli.command else {
        panic!("expected auth");
    };
    a.command
}

#[test]
fn auth_has_show_enable_disable_and_rotate() {
    let AuthSub::Show(args) = auth_sub(&["show"]) else {
        panic!("expected auth show");
    };
    assert!(!args.reveal);
    let AuthSub::Show(args) = auth_sub(&["show", "--reveal"]) else {
        panic!("expected auth show");
    };
    assert!(args.reveal);

    let AuthSub::Enable(args) = auth_sub(&["enable"]) else {
        panic!("expected auth enable");
    };
    assert_eq!(args.token, None, "one is generated");
    let AuthSub::Enable(args) = auth_sub(&["enable", "--token", "sekret"]) else {
        panic!("expected auth enable");
    };
    assert_eq!(args.token.as_deref(), Some("sekret"));

    assert!(matches!(auth_sub(&["disable"]), AuthSub::Disable(_)));
    assert!(matches!(auth_sub(&["rotate"]), AuthSub::Rotate(_)));

    // `--token` on `enable` needs a value; rotating takes none at all.
    assert!(Cli::try_parse_from(["chap", "auth", "enable", "--token"]).is_err());
    assert!(Cli::try_parse_from(["chap", "auth", "rotate", "--token", "x"]).is_err());
}

#[test]
fn auth_needs_a_subcommand() {
    let err = Cli::try_parse_from(["chap", "auth"]).expect_err("a subcommand is required");
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
    let help = err.to_string();
    for name in ["show", "enable", "disable", "rotate"] {
        assert!(help.contains(name), "{name} is missing from:\n{help}");
    }
}

#[test]
fn init_fresh_env_and_no_env_conflict() {
    assert!(Cli::try_parse_from(["chap", "init", "--fresh-env"]).is_ok());
    assert!(Cli::try_parse_from(["chap", "init", "--no-env"]).is_ok());
    assert!(Cli::try_parse_from(["chap", "init", "--fresh-env", "--no-env"]).is_err());
}

#[test]
fn enable_channel_and_version_conflict() {
    let ok = Cli::try_parse_from(["chap", "models", "enable", "x", "--channel", "latest"]);
    assert!(ok.is_ok());
    let ok = Cli::try_parse_from(["chap", "models", "enable", "x", "--version", "1.0.0"]);
    assert!(ok.is_ok());
    let bad = Cli::try_parse_from([
        "chap",
        "models",
        "enable",
        "x",
        "--channel",
        "stable",
        "--version",
        "1.0.0",
    ]);
    assert!(bad.is_err(), "--channel and --version must conflict");
}

#[test]
fn enable_parses_every_override() {
    let cli = Cli::try_parse_from([
        "chap",
        "models",
        "enable",
        "chapkit_minimalist_example_r",
        "--port",
        "5010",
        "--data-dir",
        "/app/data",
        "--user",
        "chap:chap",
        "--allow-starter",
    ])
    .unwrap();
    let Command::Models(m) = cli.command else {
        panic!("expected models");
    };
    let ModelsCmd::Enable(args) = m.command else {
        panic!("expected enable");
    };
    assert_eq!(args.port, Some(PortArg(PortRequest::Fixed(5010))));
    assert_eq!(args.data_dir.as_deref(), Some("/app/data"));
    assert_eq!(args.user.as_deref(), Some("chap:chap"));
    assert!(args.allow_starter);
    assert_eq!(args.channel, None);
}

/// The `ModelsCmd` behind `chap models <argv..>`.
fn models_cmd(argv: &[&str]) -> ModelsCmd {
    let mut args = vec!["chap", "models"];
    args.extend_from_slice(argv);
    let cli = Cli::try_parse_from(args).unwrap();
    let Command::Models(m) = cli.command else {
        panic!("expected models");
    };
    m.command
}

#[test]
fn a_port_is_a_number_or_auto_and_nothing_else() {
    use std::str::FromStr;

    assert_eq!(
        PortArg::from_str("5010").unwrap(),
        PortArg(PortRequest::Fixed(5010))
    );
    assert_eq!(
        PortArg::from_str("auto").unwrap(),
        PortArg(PortRequest::Auto)
    );
    assert_eq!(
        PortArg::from_str("AUTO").unwrap(),
        PortArg(PortRequest::Auto)
    );
    assert_eq!(
        PortArg::from_str(" auto ").unwrap(),
        PortArg(PortRequest::Auto)
    );
    for bad in ["", "none", "0", "-1", "70000", "50a0"] {
        assert!(PortArg::from_str(bad).is_err(), "{bad} should not parse");
    }

    // And clap rejects it next to the flag rather than later.
    assert!(Cli::try_parse_from(["chap", "models", "enable", "x", "--port", "nope"]).is_err());
}

/// Both spellings, because `--ocs-bbox -13.5,...` is the one anyone types
/// first and a western or southern extent starts with a minus sign.
#[test]
fn a_negative_bbox_parses_with_and_without_the_equals_sign() {
    const BBOX: &str = "-13.5,6.9,-10.1,10.0";

    for argv in [
        vec![
            "--with".to_string(),
            "ocs".into(),
            "--ocs-bbox".into(),
            BBOX.into(),
        ],
        vec![
            "--with".to_string(),
            "ocs".into(),
            format!("--ocs-bbox={BBOX}"),
        ],
    ] {
        let args = init_args(&argv.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(args.ocs.ocs_bbox.as_deref(), Some(BBOX), "{argv:?}");
    }

    // The same on `components enable`, which shares the flag group.
    for argv in [
        vec![
            "enable".to_string(),
            "ocs".into(),
            "--ocs-bbox".into(),
            BBOX.into(),
        ],
        vec![
            "enable".to_string(),
            "ocs".into(),
            format!("--ocs-bbox={BBOX}"),
        ],
    ] {
        let ComponentsCmd::Enable(args) =
            components_cmd(&argv.iter().map(String::as_str).collect::<Vec<_>>())
        else {
            panic!("expected enable");
        };
        assert_eq!(args.ocs.ocs_bbox.as_deref(), Some(BBOX), "{argv:?}");
    }

    // The flag still needs a value: `allow_hyphen_values` does not let it
    // swallow the next flag.
    assert!(
        Cli::try_parse_from(["chap", "init", "x", "--ocs-bbox"]).is_err(),
        "a value is required"
    );
}

/// `init` settles the component ports too, so a deployment whose OCS port is
/// taken is one command rather than two. The value parser is the one
/// `components enable --port` uses, so `none` means the same thing in both.
#[test]
fn init_takes_the_component_ports_and_the_read_only_switch() {
    let plain = init_args(&["x"]);
    assert_eq!(plain.ocs_port, None, "the flag was absent");
    assert_eq!(plain.s3_port, None);
    assert!(!plain.ocs_read_only);

    let given = init_args(&[
        "x",
        "--with",
        "ocs,s3",
        "--ocs-port",
        "9010",
        "--s3-port",
        "none",
        "--ocs-read-only",
    ]);
    assert_eq!(given.ocs_port, Some(ComponentPortArg(Some(9010))));
    assert_eq!(given.s3_port, Some(ComponentPortArg(None)));
    assert!(given.ocs_read_only);

    // The same refusals the shared parser gives everywhere else.
    assert!(Cli::try_parse_from(["chap", "init", "x", "--ocs-port", "auto"]).is_err());
    assert!(Cli::try_parse_from(["chap", "init", "x", "--s3-port"]).is_err());
}

/// The three ways to spell a component's host port, and the one that takes
/// it away again.
#[test]
fn a_component_port_is_a_number_or_none() {
    use std::str::FromStr;

    assert_eq!(
        ComponentPortArg::from_str("9010").unwrap(),
        ComponentPortArg(Some(9010))
    );
    for spelling in ["none", "NONE", " none "] {
        assert_eq!(
            ComponentPortArg::from_str(spelling).unwrap(),
            ComponentPortArg(None),
            "{spelling}"
        );
    }
    for bad in ["", "auto", "0", "70000", "90a0"] {
        assert!(
            ComponentPortArg::from_str(bad).is_err(),
            "{bad} should not parse"
        );
    }

    let ComponentsCmd::Enable(args) = components_cmd(&["enable", "ocs"]) else {
        panic!("expected enable");
    };
    assert_eq!(args.port, None, "the flag was absent: the record stands");
    assert!(!args.read_only && !args.read_write);

    let ComponentsCmd::Enable(args) = components_cmd(&[
        "enable",
        "ocs",
        "--port",
        "none",
        "--base-url",
        "https://ocs.example.org",
        "--read-only",
    ]) else {
        panic!("expected enable");
    };
    assert_eq!(args.port, Some(ComponentPortArg(None)));
    assert_eq!(args.base_url.as_deref(), Some("https://ocs.example.org"));
    assert!(args.read_only);

    // The two switches are opposites, so asking for both is a clap error.
    assert!(
        Cli::try_parse_from([
            "chap",
            "components",
            "enable",
            "ocs",
            "--read-only",
            "--read-write"
        ])
        .is_err()
    );
}

/// The `ComponentsCmd` behind `chap components <argv..>`.
fn components_cmd(argv: &[&str]) -> ComponentsCmd {
    let mut args = vec!["chap", "components"];
    args.extend_from_slice(argv);
    let cli = Cli::try_parse_from(args).unwrap();
    let Command::Components(c) = cli.command else {
        panic!("expected components");
    };
    c.command
}

#[test]
fn expose_defaults_to_an_automatic_port_and_unexpose_takes_none() {
    let ModelsCmd::Expose(args) = models_cmd(&["expose", "chapkit_ewars_model"]) else {
        panic!("expected expose");
    };
    assert_eq!(args.id, "chapkit_ewars_model");
    assert_eq!(args.port, None, "the command fills in `auto`");

    let ModelsCmd::Expose(args) = models_cmd(&["expose", "chapkit-ewars-model", "--port", "5010"])
    else {
        panic!("expected expose");
    };
    assert_eq!(args.port, Some(PortArg(PortRequest::Fixed(5010))));

    let ModelsCmd::Expose(args) = models_cmd(&["expose", "x", "--port", "auto"]) else {
        panic!("expected expose");
    };
    assert_eq!(args.port, Some(PortArg(PortRequest::Auto)));

    let ModelsCmd::Unexpose(args) = models_cmd(&["unexpose", "chapkit-ewars-model"]) else {
        panic!("expected unexpose");
    };
    assert_eq!(args.id, "chapkit-ewars-model");

    for argv in [
        ["chap", "models", "expose"].as_slice(),
        ["chap", "models", "unexpose"].as_slice(),
    ] {
        assert!(Cli::try_parse_from(argv).is_err(), "the id is required");
    }
}

/// The `DockerSub` behind `chap docker <argv..>`.
fn docker_sub(argv: &[&str]) -> DockerSub {
    let mut args = vec!["chap", "docker"];
    args.extend_from_slice(argv);
    let cli = Cli::try_parse_from(args).unwrap();
    let Command::Docker(d) = cli.command else {
        panic!("expected docker");
    };
    d.command
}

#[test]
fn docker_run_passes_hyphenated_args_through() {
    let DockerSub::Run(args) = docker_sub(&["run", "config", "--services", "--quiet"]) else {
        panic!("expected docker run");
    };
    assert_eq!(args.args, vec!["config", "--services", "--quiet"]);
}

#[test]
fn docker_needs_a_subcommand() {
    let err = Cli::try_parse_from(["chap", "docker"]).expect_err("a subcommand is required");
    // arg_required_else_help prints the whole help, not a one-line error.
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
    assert!(err.to_string().contains("exec"));
}

#[test]
fn docker_ps_and_pull_keep_their_shapes() {
    let DockerSub::Ps(args) = docker_sub(&["ps", "-a"]) else {
        panic!("expected docker ps");
    };
    assert_eq!(args.extra, vec!["-a"]);
    assert!(matches!(docker_sub(&["pull"]), DockerSub::Pull(_)));
}

#[test]
fn docker_exec_takes_a_service_and_an_optional_command() {
    let DockerSub::Exec(args) = docker_sub(&["exec", "chap"]) else {
        panic!("expected docker exec");
    };
    assert_eq!(args.service, "chap");
    assert!(
        args.cmd.is_empty(),
        "the default command is filled in later"
    );

    // A hyphenated command reaches the container rather than clap.
    let DockerSub::Exec(args) = docker_sub(&["exec", "chap", "--", "ls", "-la", "/app"]) else {
        panic!("expected docker exec");
    };
    assert_eq!(args.service, "chap");
    assert_eq!(args.cmd, vec!["ls", "-la", "/app"]);

    assert!(
        Cli::try_parse_from(["chap", "docker", "exec"]).is_err(),
        "the service name is required"
    );
}

#[test]
fn docker_config_takes_extra_arguments() {
    let DockerSub::Config(args) = docker_sub(&["config"]) else {
        panic!("expected docker config");
    };
    assert!(args.extra.is_empty());

    let DockerSub::Config(args) = docker_sub(&["config", "--", "--services"]) else {
        panic!("expected docker config");
    };
    assert_eq!(args.extra, vec!["--services"]);
}

#[test]
fn up_takes_a_flag_then_extra_args() {
    let cli = Cli::try_parse_from(["chap", "up", "--attach", "chap"]).unwrap();
    let Command::Up(args) = cli.command else {
        panic!("expected up");
    };
    assert!(args.attach);
    assert!(!args.pull && !args.no_preflight);
    assert_eq!(args.extra, vec!["chap"]);

    let cli = Cli::try_parse_from(["chap", "up", "--pull"]).unwrap();
    let Command::Up(args) = cli.command else {
        panic!("expected up");
    };
    assert!(args.pull && !args.attach);
    assert!(args.extra.is_empty());

    let cli = Cli::try_parse_from(["chap", "up", "--no-preflight"]).unwrap();
    let Command::Up(args) = cli.command else {
        panic!("expected up");
    };
    assert!(args.no_preflight);
}

/// The `DownArgs` behind `chap down <argv..>`, with the global
/// `--verbose` flag as that same command line left it.
fn down_args(argv: &[&str]) -> (bool, DownArgs) {
    let mut args = vec!["chap", "down"];
    args.extend_from_slice(argv);
    let cli = Cli::try_parse_from(args).unwrap();
    let verbose = cli.verbose > 0;
    let Command::Down(args) = cli.command else {
        panic!("expected down");
    };
    (verbose, args)
}

/// `-v` is the global `--verbose` flag, which is the whole reason
/// `--volumes` has no short spelling: compose's `-v` destroys data, and a
/// flag that means one thing on the way in and another on the way out is
/// not a flag anyone can trust.
#[test]
fn down_takes_volumes_and_yes_while_v_stays_verbose() {
    let (verbose, args) = down_args(&[]);
    assert!(!args.volumes && !args.yes && args.extra.is_empty());
    assert!(!verbose);

    let (_, args) = down_args(&["--volumes", "--yes"]);
    assert!(args.volumes && args.yes && args.extra.is_empty());

    let (_, args) = down_args(&["-y", "--volumes"]);
    assert!(args.volumes && args.yes);

    // The bug this exists for: `-v` is chap-devops's own flag, so compose never
    // sees it and the volumes stay.
    let (verbose, args) = down_args(&["-v"]);
    assert!(verbose);
    assert!(!args.volumes && args.extra.is_empty());

    // Past a `--` it reaches the passthrough, where the command refuses
    // it rather than guess which `-v` was meant.
    let (_, args) = down_args(&["--", "-v"]);
    assert_eq!(args.extra, vec!["-v"]);

    // And compose's own flags still go through.
    let (_, args) = down_args(&["--volumes", "--", "--timeout", "30"]);
    assert!(args.volumes);
    assert_eq!(args.extra, vec!["--timeout", "30"]);
}

#[test]
fn up_attach_answers_to_a_and_to_foreground() {
    for spelling in ["--attach", "-a", "--foreground"] {
        let cli = Cli::try_parse_from(["chap", "up", spelling]).unwrap();
        let Command::Up(args) = cli.command else {
            panic!("expected up");
        };
        assert!(args.attach, "`up {spelling}` runs in the foreground");
        assert!(args.extra.is_empty(), "{spelling} is a flag, not an extra");
    }

    // The alias is in the help, next to the flag it stands for.
    let help = Cli::command()
        .find_subcommand_mut("up")
        .expect("up is a subcommand")
        .render_long_help()
        .to_string();
    assert!(help.contains("--foreground"), "{help}");
    assert!(help.contains("-a"), "{help}");
    assert!(
        help.contains("Run in the foreground and stream all logs (Ctrl-C stops Chap)"),
        "{help}"
    );
}

#[test]
fn sync_and_update_take_their_flags() {
    let cli = Cli::try_parse_from(["chap", "sync"]).unwrap();
    assert!(matches!(
        cli.command,
        Command::Sync(SyncArgs { check: false })
    ));
    let cli = Cli::try_parse_from(["chap", "sync", "--check"]).unwrap();
    assert!(matches!(
        cli.command,
        Command::Sync(SyncArgs { check: true })
    ));

    let cli = Cli::try_parse_from(["chap", "update"]).unwrap();
    let Command::Update(args) = cli.command else {
        panic!("expected update");
    };
    assert!(!args.dry_run && !args.pin_chap_core);
    let cli = Cli::try_parse_from(["chap", "update", "--dry-run", "--pin-chap-core"]).unwrap();
    let Command::Update(args) = cli.command else {
        panic!("expected update");
    };
    assert!(args.dry_run && args.pin_chap_core);
}

/// The `RestartArgs` behind `chap restart <argv..>`.
fn restart_args(argv: &[&str]) -> RestartArgs {
    let mut args = vec!["chap", "restart"];
    args.extend_from_slice(argv);
    let cli = Cli::try_parse_from(args).unwrap();
    let Command::Restart(args) = cli.command else {
        panic!("expected restart");
    };
    args
}

#[test]
fn restart_takes_service_names_with_the_flag_on_either_side() {
    let args = restart_args(&[]);
    assert!(!args.all && args.services.is_empty());

    let args = restart_args(&["--all"]);
    assert!(args.all && args.services.is_empty());

    // The order the status hint prints: flag first, then the service.
    let args = restart_args(&["--all", "chapkit-ewars-model"]);
    assert!(args.all);
    assert_eq!(args.services, vec!["chapkit-ewars-model"]);

    // And the other way round, plus more than one name.
    let args = restart_args(&["chap", "worker", "--all"]);
    assert!(args.all);
    assert_eq!(args.services, vec!["chap", "worker"]);
}

#[test]
fn logs_takes_follow_and_services() {
    let cli = Cli::try_parse_from(["chap", "logs", "-f", "chap", "chapkit-ewars-model"]).unwrap();
    let Command::Logs(args) = cli.command else {
        panic!("expected logs");
    };
    assert!(args.follow);
    assert_eq!(args.services.len(), 2);
}

/// The `BackupSub` behind `chap backup <argv..>`.
fn backup_sub(argv: &[&str]) -> BackupSub {
    let mut args = vec!["chap", "backup"];
    args.extend_from_slice(argv);
    let cli = Cli::try_parse_from(args).unwrap();
    let Command::Backup(b) = cli.command else {
        panic!("expected backup");
    };
    b.command
}

#[test]
fn backup_needs_a_subcommand() {
    let err = Cli::try_parse_from(["chap", "backup"]).expect_err("a subcommand is required");
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
    let help = err.to_string();
    assert!(help.contains("create") && help.contains("restore"));
}

#[test]
fn backup_create_defaults_and_flags() {
    let BackupSub::Create(args) = backup_sub(&["create"]) else {
        panic!("expected backup create");
    };
    assert!(args.out.is_none() && !args.no_db && !args.no_models);

    let BackupSub::Create(args) =
        backup_sub(&["create", "--out", "/backups", "--no-db", "--no-models"])
    else {
        panic!("expected backup create");
    };
    assert_eq!(args.out, Some(PathBuf::from("/backups")));
    assert!(args.no_db && args.no_models);
}

#[test]
fn backup_restore_takes_an_archive_and_its_flags() {
    let BackupSub::Restore(args) = backup_sub(&["restore", "x.tar.gz"]) else {
        panic!("expected backup restore");
    };
    assert_eq!(args.archive, PathBuf::from("x.tar.gz"));
    assert!(!args.yes && !args.files_only && !args.db_only);
    assert!(!args.no_models && !args.no_start);

    let BackupSub::Restore(args) =
        backup_sub(&["restore", "x.tar.gz", "--yes", "--db-only", "--no-start"])
    else {
        panic!("expected backup restore");
    };
    assert!(args.yes && args.db_only && args.no_start);

    assert!(
        Cli::try_parse_from(["chap", "backup", "restore"]).is_err(),
        "the archive is required"
    );
}

#[test]
fn restore_scopes_that_contradict_each_other_are_rejected() {
    for argv in [
        vec!["--files-only", "--db-only"],
        vec!["--files-only", "--no-models"],
        vec!["--files-only", "--no-start"],
        vec!["--db-only", "--no-models"],
    ] {
        let mut args = vec!["chap", "backup", "restore", "x.tar.gz"];
        args.extend_from_slice(&argv);
        assert!(Cli::try_parse_from(&args).is_err(), "{argv:?}");
    }
    assert!(Cli::try_parse_from(["chap", "backup", "restore", "x.tar.gz", "--files-only"]).is_ok());
    assert!(Cli::try_parse_from(["chap", "backup", "restore", "x.tar.gz", "--db-only"]).is_ok());
}

#[test]
fn status_defaults() {
    let cli = Cli::try_parse_from(["chap", "status"]).unwrap();
    let Command::Status(args) = cli.command else {
        panic!("expected status");
    };
    assert_eq!(
        args.url, None,
        "the default follows the project's api_port, so the command fills it in"
    );
    assert_eq!(args.timeout, 5);

    let cli = Cli::try_parse_from(["chap", "status", "--url", "http://host:9000"]).unwrap();
    let Command::Status(args) = cli.command else {
        panic!("expected status");
    };
    assert_eq!(args.url.as_deref(), Some("http://host:9000"));
}

/// `open` takes a component name and nothing else, and the bare command is
/// not a usage error: it lists what there is to open.
#[test]
fn open_takes_one_optional_component_name() {
    let named = Cli::try_parse_from(["chap", "open", "dhis2"]).unwrap();
    let Command::Open(args) = named.command else {
        panic!("open parses as itself");
    };
    assert_eq!(args.name.as_deref(), Some("dhis2"));

    let bare = Cli::try_parse_from(["chap", "open"]).unwrap();
    let Command::Open(args) = bare.command else {
        panic!("the bare command parses");
    };
    assert_eq!(args.name, None, "a listing, not a usage error");

    // One name, not a list: opening two pages at once is not what this is.
    assert!(Cli::try_parse_from(["chap", "open", "ocs", "dhis2"]).is_err());
}

#[test]
fn the_browser_is_the_top_level_ui_command() {
    assert!(matches!(
        Cli::try_parse_from(["chap", "ui"]).unwrap().command,
        Command::Ui(_)
    ));
}

#[test]
fn registry_has_update_and_show() {
    for (argv, want_update) in [
        (["chap", "registry", "update"], true),
        (["chap", "registry", "show"], false),
    ] {
        let cli = Cli::try_parse_from(argv).unwrap();
        let Command::Registry(r) = cli.command else {
            panic!("expected registry");
        };
        assert_eq!(matches!(r.command, RegistryCmd::Update(_)), want_update);
    }
}

/// The help of a command, from the tree `main` parses with.
fn help_of(path: &[&str]) -> String {
    let mut command = command();
    command.build();
    let mut sub = &mut command;
    for name in path {
        sub = sub.find_subcommand_mut(name).expect("a subcommand");
    }
    sub.render_help().to_string()
}

/// The registry options show where the registry is read, and nowhere else.
#[test]
fn only_the_commands_that_read_the_registry_show_its_options() {
    for path in [&["init"][..], &["models", "list"], &["sync"], &["up"]] {
        let help = help_of(path);
        assert!(help.contains("Registry options:"), "{path:?}:\n{help}");
        assert!(help.contains("--registry-url"), "{path:?}:\n{help}");
        assert!(help.contains("--offline"), "{path:?}:\n{help}");
    }
    for path in [&["logs"][..], &["down"], &["status"], &["auth", "show"]] {
        let help = help_of(path);
        assert!(!help.contains("Registry options"), "{path:?}:\n{help}");
        assert!(!help.contains("--registry-url"), "{path:?}:\n{help}");
        assert!(!help.contains("--offline"), "{path:?}:\n{help}");
        assert!(help.contains("--json"), "{path:?}:\n{help}");
    }
    let apps = help_of(&["dhis2", "apps"]);
    assert!(apps.contains("--offline"), "{apps}");
    assert!(!apps.contains("--registry-url"), "{apps}");
    let root = help_of(&[]);
    assert!(root.contains("Registry options:"), "{root}");
}

/// Every entry in the list names a command that exists, so a rename cannot
/// hide the options from a command that reads the registry.
#[test]
fn every_registry_user_is_a_command() {
    let root = command();
    for (path, _) in REGISTRY_USERS {
        let mut sub = &root;
        for name in path.split(' ') {
            sub = sub
                .find_subcommand(name)
                .unwrap_or_else(|| panic!("`chap-devops {path}` is not a command"));
        }
    }
}

/// Hidden is not removed: the options still parse on every command, before
/// the command name and after it.
#[test]
fn hidden_registry_options_still_parse() {
    let parse = |argv: &[&str]| {
        let matches = command()
            .try_get_matches_from(argv)
            .unwrap_or_else(|e| panic!("{argv:?}: {e}"));
        Cli::from_arg_matches(&matches).unwrap()
    };
    let cli = parse(&["chap-devops", "--offline", "status"]);
    assert!(cli.offline);
    assert!(matches!(cli.command, Command::Status(_)));
    assert!(parse(&["chap-devops", "status", "--offline"]).offline);
    assert!(parse(&["chap-devops", "--offline", "init"]).offline);
    let url = "http://127.0.0.1:18001/registry.yaml";
    assert_eq!(
        parse(&["chap-devops", "logs", "--registry-url", url]).registry_url,
        url
    );
    let cli = parse(&["chap-devops", "auth", "show", "--cache-dir", "cache"]);
    assert_eq!(cli.cache_dir, Some(PathBuf::from("cache")));
}
