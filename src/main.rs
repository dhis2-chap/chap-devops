//! `chap-devops` — deploy and manage Chap (climate-informed disease forecasting):
//! chap-core and marketplace model services on Docker Compose.

// Stubs owned by agents A, B and C are not called yet; remove after A/B/C land.

mod api;
mod auth;
mod backup;
mod chapcore;
mod cli;
mod commands;
mod components;
mod compose;
mod configs;
mod dhis2;
mod diagnose;
mod docker;
mod dotenv;
mod error;
mod github;
mod interrupt;
mod jobs;
mod known;
mod manual;
mod modeltest;
mod open;
mod output;
mod paths;
mod ports;
mod project;
mod registry;
mod scaffold;
mod selfupdate;
mod status;
mod top;
mod tui;

use clap::FromArgMatches;
use cli::{
    AuthSub, BackupSub, Cli, Command, ComponentsCmd, ConfigsCmd, Dhis2Sub, DockerCmd, JobsCmd,
    ModelsCmd, SelfSub,
};
use commands::Ctx;
use error::ChapError;
use output::Out;
use project::Project;
use std::path::{Path, PathBuf};

/// Commands that need a deployment directory, hidden from `--help` when there
/// is none. They still run when typed, and say what is missing.
const PROJECT_ONLY: &[&str] = &[
    "up",
    "down",
    "logs",
    "restart",
    "docker",
    "backup",
    "status",
    "jobs",
    "bench",
    "sync",
    "ui",
    "auth",
    "components",
    "open",
    "dhis2",
];

/// The same, for the subcommands of `models`: browsing the marketplace works
/// anywhere, changing a project's model set does not.
const PROJECT_ONLY_MODELS: &[&str] = &[
    "add", "remove", "enable", "disable", "expose", "unexpose", "test",
];

/// The mirror of [`PROJECT_ONLY`]: commands that answer "there is no
/// deployment here", hidden from `--help` inside one, where that is not the
/// question. They still run when typed: `chap-devops init --force` is how a
/// deployment's settings are rewritten, and `chap-devops init sub` nests a second
/// one.
const OUTSIDE_ONLY: &[&str] = &["init"];

/// The [`PROJECT_ONLY`] commands that are a group rather than a verb, so clap
/// answers the bare command with the group's help.
///
/// Outside a deployment that help lists only subcommands that cannot run, so
/// the missing project is the answer instead - the same one `chap-devops components
/// list` gives. `jobs` is not here: its bare form is `list`, which already
/// says so. `models` and `registry` are not project-only at all.
const PROJECT_ONLY_GROUPS: &[&str] = &["auth", "backup", "components", "dhis2", "docker"];

fn main() {
    // Windows cannot rename over a running image, so `chap-devops self update`
    // parks the outgoing binary beside the new one and the next run is the
    // first moment it can be deleted.
    #[cfg(windows)]
    if let Ok(path) = std::env::current_exe() {
        selfupdate::clean_backup(&path);
    }

    let cli = parse();
    let ctx = Ctx::from_cli(&cli);

    if let Err(err) = dispatch(&ctx, &cli) {
        fail(&ctx.out, err);
    }

    // Only after the command said what it did, and only when it worked: a
    // notice ahead of the answer would be noise, and one after a failure
    // would be advice about the wrong problem.
    if wants_update_notice(&cli.command) {
        commands::selfcmd::notify(&ctx);
    }
}

/// Report a failure the way every `chap-devops` failure is reported, and exit.
fn fail(out: &Out, err: anyhow::Error) -> ! {
    let rendered = out.error(&err);
    // JSON consumers read stdout, so a --json error goes there too.
    if out.json {
        println!("{rendered}");
    } else {
        eprintln!("{rendered}");
    }
    std::process::exit(exit_code(&err));
}

/// Whether this command may be followed by the once-a-day update notice.
///
/// Everything except the `self` group, which is the thing the notice would be
/// telling you to run, and `docs-markdown`, whose output is a file. The rest
/// of the conditions (a terminal, no `--json`, no `--offline`) are
/// [`commands::selfcmd::notify`]'s to check.
fn wants_update_notice(command: &Command) -> bool {
    !matches!(command, Command::SelfCmd(_) | Command::DocsMarkdown(_))
}

fn dispatch(ctx: &Ctx, cli: &Cli) -> error::Result<()> {
    match &cli.command {
        Command::Init(args) => commands::init::run(ctx, args),
        Command::Run(args) => commands::run::run(ctx, args),
        Command::Ps(args) => commands::run::ps(ctx, args),
        Command::Stop(args) => commands::run::stop(ctx, args),
        Command::Top(args) => commands::top::run(ctx, args),

        Command::Models(m) => match &m.command {
            ModelsCmd::List(args) => commands::models::list(ctx, args),
            ModelsCmd::Search(args) => commands::models::search(ctx, args),
            ModelsCmd::Info(args) => commands::models::info(ctx, args),
            ModelsCmd::Test(args) => commands::modeltest::run(ctx, args),
            // No subcommand is `list`, as for `chap-devops jobs`.
            ModelsCmd::Configs(c) => match &c.command {
                None => commands::configs::list(ctx, &c.list),
                Some(ConfigsCmd::List(args)) => commands::configs::list(ctx, args),
                Some(ConfigsCmd::Add(args)) => commands::configs::add(ctx, args),
                Some(ConfigsCmd::Update(args)) => commands::configs::update(ctx, args),
                Some(ConfigsCmd::Archive(args)) => commands::configs::archive(ctx, args),
                Some(ConfigsCmd::Export(args)) => commands::configs::export(ctx, args),
                Some(ConfigsCmd::Sync(args)) => commands::configs::sync::run(ctx, args),
            },
            ModelsCmd::New(args) => commands::models_new::run(ctx, args),
            ModelsCmd::Add(args) => commands::manual_models::add(ctx, args),
            ModelsCmd::Remove(args) => commands::manual_models::remove(ctx, args),
            ModelsCmd::Enable(args) => commands::enable::enable(ctx, args),
            ModelsCmd::Disable(args) => commands::enable::disable(ctx, args),
            ModelsCmd::Expose(args) => commands::enable::expose(ctx, args),
            ModelsCmd::Unexpose(args) => commands::enable::unexpose(ctx, args),
        },
        Command::Components(c) => match &c.command {
            ComponentsCmd::List(args) => commands::components::list(ctx, args),
            ComponentsCmd::Enable(args) => commands::components::enable(ctx, args),
            ComponentsCmd::Disable(args) => commands::components::disable(ctx, args),
        },

        Command::Ui(args) => run_ui(ctx, args),

        Command::Registry(r) => commands::registry::run(ctx, &r.command),

        Command::Sync(args) => commands::sync::run(ctx, args),
        Command::Update(args) => commands::update::run(ctx, args),

        Command::Up(args) => commands::docker::run(ctx, &DockerCmd::Up(args.clone())),
        Command::Down(args) => commands::docker::run(ctx, &DockerCmd::Down(args.clone())),
        Command::Logs(args) => commands::docker::run(ctx, &DockerCmd::Logs(args.clone())),
        Command::Restart(args) => commands::docker::run(ctx, &DockerCmd::Restart(args.clone())),
        Command::Docker(d) => commands::docker::run(ctx, &DockerCmd::from(&d.command)),

        Command::Backup(b) => match &b.command {
            BackupSub::Create(args) => commands::backup::run(ctx, args),
            BackupSub::Restore(args) => commands::restore::run(ctx, args),
        },

        Command::Status(args) => commands::status::run(ctx, args),

        Command::Open(args) => commands::open::run(ctx, args),

        // No subcommand is `list`: "what has this deployment been doing" is
        // the question `chap-devops jobs` is typed to answer.
        Command::Jobs(j) => match &j.command {
            None => commands::jobs::list(ctx, &j.list),
            Some(JobsCmd::List(args)) => commands::jobs::list(ctx, args),
            Some(JobsCmd::Show(args)) => commands::jobs::show(ctx, args),
            Some(JobsCmd::Logs(args)) => commands::jobs::logs(ctx, args),
            Some(JobsCmd::Cancel(args)) => commands::jobs::cancel(ctx, args),
            Some(JobsCmd::Delete(args)) => commands::jobs::delete(ctx, args),
        },

        Command::Api(args) => commands::api::run(ctx, args),
        Command::Bench(args) => commands::bench::run(ctx, args),
        Command::Chap(args) => commands::chap::run(ctx, args),
        Command::Chapkit(args) => commands::chapkit::run(ctx, args),

        Command::Doctor(args) => commands::doctor::run(ctx, args),
        Command::Cleanup(args) => commands::cleanup::run(ctx, args),

        Command::Auth(a) => match &a.command {
            AuthSub::Show(args) => commands::auth::show(ctx, args),
            AuthSub::Token(args) => commands::auth::token(ctx, args),
            AuthSub::Enable(args) => commands::auth::enable(ctx, args),
            AuthSub::Disable(args) => commands::auth::disable(ctx, args),
            AuthSub::Rotate(args) => commands::auth::rotate(ctx, args),
        },

        Command::Dhis2(d) => match &d.command {
            Dhis2Sub::Show(args) => commands::dhis2::show(ctx, args),
            Dhis2Sub::Route(args) => commands::dhis2::route(ctx, args),
            Dhis2Sub::Analytics(args) => commands::dhis2::analytics(ctx, args),
            Dhis2Sub::Apps(args) => commands::dhis2::apps(ctx, args),
            Dhis2Sub::Connect(args) => commands::dhis2::connect(ctx, args),
            Dhis2Sub::Use(args) => commands::dhis2::use_external(ctx, args),
        },

        Command::SelfCmd(s) => match &s.command {
            SelfSub::Update(args) => commands::selfcmd::update(ctx, args),
            SelfSub::Version(args) => commands::selfcmd::version(ctx, args),
        },

        Command::Completions(args) => commands::completions::run(ctx, args),

        Command::DocsMarkdown(args) => commands::docs::run(ctx, args),
    }
}

/// Parse the command line against a help listing shaped by where we are.
///
/// Outside a deployment directory most of the tree cannot do anything, so it is
/// hidden rather than offered; the commands still parse and run, and fail with
/// the error that names the missing `.chap-devops/project.yaml`. Inside one the
/// hiding runs the other way, over the commands that are about not having a
/// deployment yet.
fn parse() -> Cli {
    let argv: Vec<String> = std::env::args().collect();
    let project_dir = project_dir_of(&argv[1..]);
    let inside = Project::find_root(&project_dir).is_some();
    if !inside && let Some(matches) = bare_project_group(&argv) {
        no_project(&matches, &project_dir);
    }
    let command = if inside {
        hide_outside_commands(cli::command())
    } else {
        hide_project_commands(cli::command())
    };
    let json = argv
        .iter()
        .skip(1)
        .take_while(|a| *a != "--")
        .any(|a| a == "--json");
    let matches = match command.try_get_matches_from(&argv) {
        Ok(matches) => matches,
        Err(err) => print_and_exit(err, json),
    };
    let mut cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(err) => print_and_exit(err, json),
    };
    // A deployment created with `--registry-url` keeps using that registry:
    // the flag only has to be typed again to override it for one run.
    if matches.value_source("registry_url") != Some(clap::parser::ValueSource::CommandLine)
        && let Some(root) = Project::find_root(&project_dir)
        && let Some(url) = Project::saved_registry_url(&root)
    {
        cli.registry_url = url;
    }
    cli
}

/// Print what clap has to say - help, the version, or a usage error - and exit
/// with its code, leaving a blank line after the help that ends on the book.
///
/// The blank line cannot come from `after_help`: clap renders the help,
/// `trim_end`s it and appends exactly one newline, so a newline written into
/// [`cli::DOCS_LINE`] or into the `after_help` value never reaches the
/// terminal. It is added here instead, where the help is printed, which is
/// also what it is: a blank line between the book's address and the prompt,
/// not part of the address. Only the root help gets it - it is the one that
/// ends on the docs line, however it was reached (`chap-devops --help`, `chap-devops -h`,
/// `chap-devops help`, or `chap-devops` with nothing after it) - so a subcommand's help
/// still ends on its last line.
///
/// Under `--json` a usage error is the same JSON object every other failure
/// is, on stdout, so a caller that parses one parses both; help and the
/// version are still clap's text.
fn print_and_exit(err: clap::Error, json: bool) -> ! {
    use clap::error::ErrorKind;
    use std::io::Write;

    if json && err.use_stderr() {
        // clap's text is the message, wrapped over indented lines, then a
        // tip or two and the usage line, which are what the causes carry.
        let rendered = err.render().to_string();
        let lines: Vec<&str> = rendered
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("For more information"))
            .collect();
        let split = lines
            .iter()
            .position(|l| l.starts_with("tip:") || l.starts_with("Usage:"))
            .unwrap_or(lines.len());
        let message = lines[..split]
            .join(" ")
            .trim_start_matches("error: ")
            .to_string();
        let causes = &lines[split..];
        // `Usage: chap-devops stop [OPTIONS] <ID>` names the command to ask.
        let command = causes
            .iter()
            .find_map(|l| l.strip_prefix("Usage: "))
            .map(|usage| {
                usage
                    .split_whitespace()
                    .take_while(|w| !w.starts_with(['<', '[', '-']))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| "chap-devops".to_string());
        let value = serde_json::json!({
            "ok": false,
            "hint": format!("`{command} --help` lists what it takes"),
            "error": message,
            "causes": causes,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        );
        std::process::exit(err.exit_code());
    }

    let ends_on_the_docs_line = matches!(
        err.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    ) && err
        .render()
        .to_string()
        .trim_end()
        .ends_with(cli::DOCS_LINE);
    // Broken pipes are swallowed, as clap's own `exit` does.
    let _ = err.print();
    if ends_on_the_docs_line {
        if err.use_stderr() {
            let mut stream = std::io::stderr();
            let _ = stream.write_all(b"\n");
            let _ = stream.flush();
        } else {
            let mut stream = std::io::stdout();
            let _ = stream.write_all(b"\n");
            let _ = stream.flush();
        }
    }
    std::process::exit(err.exit_code());
}

/// Hide the commands that need a project: outside one they cannot work.
fn hide_project_commands(mut command: clap::Command) -> clap::Command {
    for name in PROJECT_ONLY {
        command = command.mut_subcommand(name, |c| c.hide(true));
    }
    command.mut_subcommand("models", |models| {
        PROJECT_ONLY_MODELS.iter().fold(models, |models, name| {
            models.mut_subcommand(name, |c| c.hide(true))
        })
    })
}

/// Hide the commands that are about not having a project yet: inside one they
/// are not what to run next.
fn hide_outside_commands(mut command: clap::Command) -> clap::Command {
    for name in OUTSIDE_ONLY {
        command = command.mut_subcommand(name, |c| c.hide(true));
    }
    command
}

/// The matches for a [`PROJECT_ONLY_GROUPS`] command typed with nothing after
/// it, and `None` for every other command line.
///
/// Clap's own answer to a bare group is `arg_required_else_help`, and outside
/// a deployment that help lists only subcommands that cannot run either. The
/// classification is a parse of its own, against a throwaway tree that lets
/// the bare form through, so that the tree the command actually runs against
/// keeps clap's required-subcommand usage line and its own help. Anything
/// else - a subcommand, an unknown word, `--help` - stays clap's to answer.
fn bare_project_group(argv: &[String]) -> Option<clap::ArgMatches> {
    let mut command = cli::command();
    for name in PROJECT_ONLY_GROUPS {
        command = command.mut_subcommand(name, |c| {
            c.subcommand_required(false).arg_required_else_help(false)
        });
    }
    let matches = command.try_get_matches_from(argv).ok()?;
    let (name, group) = matches.subcommand()?;
    (PROJECT_ONLY_GROUPS.contains(&name) && group.subcommand().is_none()).then_some(matches)
}

/// The missing-project error, reported before there is a [`Cli`] to carry the
/// flags its rendering reads: only `--json` and `--no-color` matter, and clap
/// has both already.
fn no_project(matches: &clap::ArgMatches, project_dir: &Path) -> ! {
    let out = Out::detect(matches.get_flag("json"), matches.get_flag("no_color"));
    let shown = std::path::absolute(project_dir).unwrap_or_else(|_| project_dir.to_path_buf());
    fail(&out, ChapError::NotAProject(shown).into())
}

/// The `-C` / `--project-dir` value, read straight out of the raw arguments.
///
/// The help text depends on it, and help is built before clap parses, so this
/// runs first. It accepts the spellings clap does (`-C dir`, `-Cdir`, `-C=dir`,
/// `--project-dir dir`, `--project-dir=dir`) and otherwise falls back to `.`,
/// the flag's own default. Getting it wrong only changes which commands are
/// listed, never which ones run.
fn project_dir_of(args: &[String]) -> PathBuf {
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = if let Some(v) = arg.strip_prefix("--project-dir=") {
            Some(v)
        } else if arg == "--project-dir" || arg == "-C" {
            rest.next().map(String::as_str)
        } else if let Some(v) = arg.strip_prefix("-C").filter(|v| !v.is_empty()) {
            Some(v.strip_prefix('=').unwrap_or(v))
        } else {
            None
        };
        if let Some(value) = value {
            return PathBuf::from(value);
        }
    }
    PathBuf::from(".")
}

/// The browser owns the terminal, so there is nothing to serialise: reject
/// `--json` rather than print something a caller cannot parse.
fn run_ui(ctx: &Ctx, args: &cli::UiArgs) -> error::Result<()> {
    if ctx.out.json {
        return Err(anyhow::anyhow!(
            "--json cannot be combined with the model browser"
        ));
    }
    commands::tui::run(ctx, args)
}

/// Mirror docker compose's exit code when it is the thing that failed, so
/// `chap-devops up` is a drop-in for `docker compose up` in scripts.
fn exit_code(err: &anyhow::Error) -> i32 {
    match err.downcast_ref::<ChapError>() {
        Some(ChapError::DockerFailed(code)) if *code != 0 => *code,
        Some(ChapError::Exited { code, .. }) if *code != 0 => *code,
        Some(ChapError::Interrupted(_)) => interrupt::EXIT_CODE,
        // The code clap exits with for a usage error, because that is what
        // this is: a usage error clap could not catch.
        Some(ChapError::Usage(_)) => 2,
        // "Chap is not up" is a different answer from "Chap said no", and a
        // script driving `chap-devops api` has to be able to tell them apart.
        Some(ChapError::Unreachable { .. } | ChapError::Dhis2Unreachable { .. }) => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
