//! `chap-devops doctor` - one bounded checklist that says whether this machine, and
//! this deployment, can run Chap.
//!
//! Every other command answers one question. `doctor` answers the ones an
//! operator would otherwise ask in sequence: is Docker there, is the daemon
//! up, is Compose new enough, is there disk, can this machine reach the hosts
//! Chap pulls from - and, inside a deployment, are the files, the ports, the
//! pins and the running stack what they should be.
//!
//! Two rules shape the module. Every check is bounded: external commands are
//! killed when they overrun and every network probe carries a timeout, so
//! `chap-devops doctor` always finishes. And every verdict is a pure function of
//! data someone else gathered, so the half that decides what to say is
//! testable without Docker, a network or a project.

use crate::auth;
use crate::chapcore;
use crate::cli::DoctorArgs;
use crate::commands::Ctx;
use crate::components::{
    COMPONENTS_FILE, Component, Components, DHIS2_CONFIG_FILE, DHIS2_DIR, DHIS2_JAVA_ENV_VAR,
    DHIS2_TAG_ENV_VAR, Dhis2Seed, OCS_CONFIG_FILE, OCS_DIR, OCS_IMAGE, OCS_PLUGINS_DIR,
    OCS_TAG_ENV_VAR, S3_DEFAULT_TAG, S3_IMAGE, S3_TAG_ENV_VAR,
};
use crate::compose::render::OCS_EXAMPLE_MARKER;
use crate::compose::{API_SERVICE, sync};
use crate::docker;
use crate::error::Result;
use crate::github;
use crate::output::Out;
use crate::ports::{self, PortClaim};
use crate::project::{
    API_WORKERS_ENV_VAR, ApiPortSource, BASE_COMPOSE, CHAP_DEVOPS_COMPOSE, CHAP_DEVOPS_DIR,
    CHAP_TAG_ENV_VAR, ENV_FILE, MARKETPLACE_COMPOSE, MODELS_FILE, PROJECT_FILE, Project,
};
use crate::registry;
use crate::selfupdate::{self, TARGET, VERSION};
use crate::status::{ApiHealth, ComponentState, StatusReport};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod components;
mod deadline;
mod images;
mod machine;
mod network;
mod pins;
mod project;
#[cfg(test)]
mod test_support;

pub use components::*;
pub use deadline::*;
pub use images::*;
pub use machine::*;
pub use network::*;
pub use pins::*;
pub use project::*;

/// How long one network probe may take. Three seconds is long enough for a
/// slow link and short enough that four of them in parallel still leave the
/// whole command inside a few seconds.
pub const NET_TIMEOUT: Duration = Duration::from_secs(3);

/// How long the `docker` CLI may take to answer one question.
pub const DOCKER_TIMEOUT: Duration = Duration::from_secs(5);

/// How long one `docker manifest inspect` may take. Longer than a probe: it
/// is a registry round trip with an authentication handshake in front of it.
pub const MANIFEST_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the `stack` check waits on chap-core's API.
pub const STACK_TIMEOUT: Duration = Duration::from_secs(5);

/// How often a bounded child is asked whether it has finished.
const POLL: Duration = Duration::from_millis(25);

/// One gibibyte, the unit the disk thresholds are written in.
pub const GB: u64 = 1024 * 1024 * 1024;

/// Free space below which `disk` warns: the images total several GB and a
/// pull that runs out halfway leaves a half-populated cache behind.
pub const DISK_WARN: u64 = 10 * GB;

/// Free space below which `disk` fails outright.
pub const DISK_FAIL: u64 = 3 * GB;

/// Memory below which `memory` warns on a deployment that runs DHIS2: room for
/// DHIS2's own default heap and nothing else in the deployment.
pub const DHIS2_MEMORY_WARN: u64 = 6 * GB;

/// Memory below which it fails. DHIS2 wants about this much to get through the
/// analytics populate phase, and below it the JVM is killed part-way through -
/// a failure that looks like anything except memory, because what is left
/// behind is an analytics run that never finishes.
pub const DHIS2_MEMORY_FAIL: u64 = 4 * GB;

/// Columns the status word is padded to (`warn`, `fail` and `skip` are the
/// widest).
pub const STATUS_WIDTH: usize = 4;

/// Spaces between the columns of one line, and the width of the indent a fix
/// line is written at.
const GAP: usize = 2;

/// The container registry every Chap image is pulled from. The anonymous
/// `/v2/` endpoint answers 401, which is still the host answering.
pub const GHCR_PROBE_URL: &str = "https://ghcr.io/v2/";

/// `User-Agent` sent with the probes, matching the registry fetch.
const USER_AGENT: &str = concat!("chap-devops/", env!("CARGO_PKG_VERSION"));

/// The one `docker info` the checklist runs, asked for every field it needs:
/// the engine version for the `docker-daemon` line, the data root for `disk`
/// and the memory for `memory`. One call would otherwise be three round trips
/// to the same daemon.
const INFO_FORMAT: &str = "{{.ServerVersion}}\t{{.DockerRootDir}}\t{{.MemTotal}}";

/// The hint `doctor` adds when it was not run inside a deployment directory.
pub const NO_PROJECT: &str = "no deployment here, so the deployment checks did not run; \
     run `chap-devops doctor` in a deployment directory for them";

/// What one check concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Nothing to do.
    Ok,
    /// It works, but something will bite later.
    Warn,
    /// Chap will not work until this is dealt with.
    Fail,
    /// Not evaluated: there was nothing to ask, or asking was ruled out.
    Skip,
}

impl Status {
    /// The word printed in the first column.
    pub fn word(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
            Status::Skip => "skip",
        }
    }

    /// The word in the colour its meaning has everywhere else in the CLI.
    fn paint(self, out: &Out) -> String {
        match self {
            Status::Ok => out.ok(self.word()),
            Status::Warn => out.warn(self.word()),
            Status::Fail => out.bad(self.word()),
            Status::Skip => out.dim(self.word()),
        }
    }
}

/// One line of the checklist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    /// Stable machine name, for a script that greps one line out of `--json`.
    pub id: String,
    /// What is printed in the second column.
    pub name: String,
    pub status: Status,
    /// The short result: what was found, in as few words as say it.
    pub detail: String,
    /// What to do about it, printed on an indented line of its own. `None`
    /// when there is nothing to do.
    pub fix: Option<String>,
}

impl Check {
    fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        status: Status,
        detail: impl Into<String>,
        fix: Option<String>,
    ) -> Check {
        Check {
            id: id.into(),
            name: name.into(),
            status,
            detail: detail.into(),
            fix,
        }
    }

    /// A check that passed. Nothing passed needs a fix.
    pub fn ok(id: impl Into<String>, name: impl Into<String>, detail: impl Into<String>) -> Check {
        Check::new(id, name, Status::Ok, detail, None)
    }

    pub fn warn(
        id: impl Into<String>,
        name: impl Into<String>,
        detail: impl Into<String>,
        fix: impl Into<String>,
    ) -> Check {
        Check::new(id, name, Status::Warn, detail, Some(fix.into()))
    }

    pub fn fail(
        id: impl Into<String>,
        name: impl Into<String>,
        detail: impl Into<String>,
        fix: impl Into<String>,
    ) -> Check {
        Check::new(id, name, Status::Fail, detail, Some(fix.into()))
    }

    /// A check that was not evaluated, and why.
    pub fn skip(
        id: impl Into<String>,
        name: impl Into<String>,
        detail: impl Into<String>,
    ) -> Check {
        Check::new(id, name, Status::Skip, detail, None)
    }

    /// [`Check::skip`] that still has something worth typing next to it.
    pub fn skip_with(
        id: impl Into<String>,
        name: impl Into<String>,
        detail: impl Into<String>,
        fix: impl Into<String>,
    ) -> Check {
        Check::new(id, name, Status::Skip, detail, Some(fix.into()))
    }

    /// Build one from a verdict: the shape the pure decision functions return.
    fn from_verdict(
        id: impl Into<String>,
        name: impl Into<String>,
        verdict: (Status, String, Option<String>),
    ) -> Check {
        Check::new(id, name, verdict.0, verdict.1, verdict.2)
    }
}

/// How many checks landed in each status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub ok: usize,
    pub warn: usize,
    pub fail: usize,
    pub skip: usize,
}

/// What `chap-devops doctor` prints, and the whole of its `--json`.
#[derive(Debug, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
    pub summary: Summary,
    /// The deployment the project half of the checklist was run against.
    /// Not serialised: `--json` is the two documented keys and nothing else,
    /// and a consumer reads the absence of the project checks instead.
    #[serde(skip)]
    pub project: Option<PathBuf>,
}

/// Count the statuses of a finished checklist.
pub fn summary(checks: &[Check]) -> Summary {
    let mut summary = Summary::default();
    for check in checks {
        match check.status {
            Status::Ok => summary.ok += 1,
            Status::Warn => summary.warn += 1,
            Status::Fail => summary.fail += 1,
            Status::Skip => summary.skip += 1,
        }
    }
    summary
}

/// The process exit code: non-zero only when something failed.
///
/// A warning is a thing to know about, not a thing that stops Chap, so
/// `chap-devops doctor` in a CI step only goes red on the checks that would have
/// stopped the deployment anyway.
pub fn exit_code(summary: &Summary) -> i32 {
    i32::from(summary.fail > 0)
}

/// The closing line: how many checks ran and how they came out.
pub fn summary_line(summary: &Summary) -> String {
    let total = summary.ok + summary.warn + summary.fail + summary.skip;
    let mut line = format!(
        "{total} checks: {} ok, {} warn, {} fail",
        summary.ok, summary.warn, summary.fail
    );
    // Skipped checks are left out of the sentence when there are none, so the
    // common case reads as the three words that matter.
    if summary.skip > 0 {
        line.push_str(&format!(", {} skipped", summary.skip));
    }
    line
}

/// The human rendering of the checklist: one line per check and an indented
/// fix under the ones that have one. The closing lines come from [`closing`].
///
/// The name column is padded to the widest name so the results line up, and
/// the padding is written outside the styled spans so a coloured word is the
/// same width as a plain one.
pub fn render(report: &Report, out: &Out) -> String {
    let name_width = report
        .checks
        .iter()
        .map(|check| check.name.chars().count())
        .max()
        .unwrap_or(0);
    let indent = " ".repeat(STATUS_WIDTH + GAP);
    let mut text = String::new();
    for check in &report.checks {
        text.push_str(&check.status.paint(out));
        text.push_str(&" ".repeat(STATUS_WIDTH - check.status.word().len() + GAP));
        text.push_str(&check.name);
        text.push_str(&" ".repeat(name_width - check.name.chars().count() + GAP));
        text.push_str(check.detail.trim());
        // A check with nothing to report must not leave a line of spaces.
        while text.ends_with(' ') {
            text.pop();
        }
        text.push('\n');
        if let Some(fix) = &check.fix {
            text.push_str(&indent);
            text.push_str(&out.backticks(fix.trim()));
            text.push('\n');
        }
    }
    // The blank line between the checklist and the summary under it.
    text.push('\n');
    text
}

/// The lines under the checklist: the summary, and a hint when there was no
/// deployment to check.
pub fn closing(report: &Report, lines: &mut crate::output::Report) {
    lines.info(summary_line(&report.summary));
    if report.project.is_none() {
        lines.hint(NO_PROJECT);
    }
}

/// Run the checklist and print it.
pub fn run(ctx: &Ctx, _args: &DoctorArgs) -> Result<()> {
    // Not `ctx.project()`: outside a deployment `doctor` still has most of a
    // report to give, so a missing project is a section that is left out
    // rather than an error.
    let project = Project::find(&ctx.project_dir).ok();
    let checks = collect(ctx, project.as_ref());
    let report = Report {
        summary: summary(&checks),
        checks,
        project: project.map(|project| project.dir),
    };
    if !ctx.out.json {
        print!("{}", render(&report, &ctx.out));
    }
    ctx.out.report(&report, |lines| closing(&report, lines))?;
    let code = exit_code(&report.summary);
    if code != 0 {
        // Everything there was to say is on the screen already; an `error:`
        // line on top of a checklist that names each failure would repeat it.
        std::process::exit(code);
    }
    Ok(())
}

/// Every check, in the order they are printed.
fn collect(ctx: &Ctx, project: Option<&Project>) -> Vec<Check> {
    let offline = ctx.registry.offline;
    let registry_url = ctx.registry.url.clone();

    std::thread::scope(|scope| {
        // Started first and joined last: the network is the slow half, and
        // the docker questions below are answered while it is in flight.
        let probes = (!offline).then(|| Probes::spawn(scope, &registry_url));

        let mut checks = Vec::new();

        // `docker --version` and not `docker version`: the latter asks the
        // daemon too and exits non-zero when it is down, which would read as
        // a missing CLI and hide the `docker daemon` line that says to start
        // it.
        let client = run_bounded("docker", &["--version"], DOCKER_TIMEOUT);
        let have_cli = client.succeeded();
        checks.push(docker_cli_check(&client));

        let info = if have_cli {
            run_bounded("docker", &["info", "--format", INFO_FORMAT], DOCKER_TIMEOUT)
        } else {
            Outcome::Missing
        };
        let engine = info_fields(info.stdout());
        checks.push(docker_daemon_check(&info, have_cli));

        // Nothing to ask compose when there is no CLI to ask it through.
        let compose = have_cli.then(|| docker::compose_version().ok()).flatten();
        checks.push(compose_check(have_cli, compose));
        checks.push(arch_check(std::env::consts::OS, std::env::consts::ARCH));

        let measured = disk_path(engine.root_dir.as_deref());
        checks.push(disk_check(&measured, free_bytes(&measured)));
        // Only asked of a daemon that answered `docker info`: these are
        // plain docker calls with no deadline of their own.
        checks.push(leftovers_check(
            info.succeeded()
                .then(crate::commands::cleanup::find)
                .flatten()
                .as_ref(),
        ));
        // Only a deployment that runs DHIS2 is asked about memory. It is the
        // one service here whose failure below a few gigabytes is a silent kill
        // rather than an error, and a line about a limit nothing else in a
        // deployment comes near would be noise on every other one.
        if project.is_some_and(|project| project.state.components.dhis2.enabled) {
            checks.push(memory_check(have_cli, engine.memory));
        }

        let probed = probes.map(Probes::join);
        checks.extend(network_checks(probed.as_ref()));
        checks.push(github_check(probed.as_ref().map(|p| &p.github)));
        checks.push(chap_devops_check(ReleaseList::of(
            probed.as_ref().map(|p| &p.chap_devops),
        )));

        if let Some(project) = project {
            checks.extend(project_checks(
                ctx,
                project,
                probed.as_ref(),
                have_cli,
                info.succeeded(),
            ));
        }
        checks
    })
}

/// The half of the checklist that needs a deployment directory.
fn project_checks(
    ctx: &Ctx,
    project: &Project,
    probed: Option<&Probed>,
    have_cli: bool,
    daemon: bool,
) -> Vec<Check> {
    // The containers are asked for once, before the checks that need them:
    // the ports they hold are not conflicts, whether any of them is up
    // decides the `stack` check, and which of them is up decides where the
    // `components` and `volumes` lines read what OCS holds - from inside the
    // container, or from its volume.
    // Only of a daemon that answered `docker info`: these calls have no
    // deadline of their own, and a daemon that is down or wedged would read
    // as "nothing there" or hold the checklist open.
    let containers = daemon.then(|| docker::all_containers(project)).flatten();
    let running: BTreeSet<String> = containers
        .as_deref()
        .map(docker::running_of)
        .unwrap_or_default();
    let own = containers
        .as_deref()
        .map(ports::OwnPorts::of)
        .unwrap_or_default();

    // The catalogue, read once for the two checks that need it: the pins it
    // carries and whether the rendered files still match them. With the probe
    // timeout rather than the registry's, because a cold cache must not be
    // able to hold the checklist open.
    let catalogue = registry::load(&registry::RegistryOptions {
        timeout: NET_TIMEOUT,
        ..ctx.registry.clone()
    });

    let env = std::fs::read_to_string(project.dir.join(ENV_FILE)).ok();
    let mut checks = vec![
        project_check(project, daemon),
        files_check(&project.dir, &project.state.components),
        components_check(project, &running),
        sync_check(project, &catalogue),
        env_check(env.as_deref()),
        match daemon {
            true => volumes_check(project, &running),
            false => no_daemon("volumes"),
        },
    ];
    // Without chap-core there is no API to give workers to.
    if project.state.components.chap_core.enabled {
        let base = std::fs::read_to_string(project.dir.join(BASE_COMPOSE)).ok();
        checks.push(api_workers_check(env.as_deref(), base.as_deref()));
    }
    checks.extend(manual_checks(ctx, project));

    let claims = ports::claims(project);
    let busy = ports::busy_claims(&claims, &own, &ports::is_busy);
    // A port to point the API at, found the way `chap-devops up` finds one.
    let suggestion = busy
        .iter()
        .find(|claim| claim.service == API_SERVICE)
        .and_then(|claim| {
            ports::first_free(claim.port.saturating_add(1), u16::MAX, &ports::is_busy)
        });
    // Only the API port has two files that could have set it, so only it ever
    // has a note.
    let api_note = api_port_note(project);
    for claim in &claims {
        let note = (claim.service == API_SERVICE)
            .then_some(api_note.as_deref())
            .flatten();
        checks.push(port_check(claim, &own, &ports::is_busy, suggestion, note));
    }

    // The chap-core release feed says nothing about a deployment that does not
    // run chap-core.
    if project.state.components.chap_core.enabled {
        // Which build a moving tag is on, asked only when the tag is one: a
        // release tag names its image, and a `docker inspect` per doctor run
        // is not worth spending to say so twice.
        let running_build = chapcore::is_moving_tag(&project.state.chap_image_tag)
            .then(|| {
                containers
                    .as_deref()
                    .and_then(|containers| docker::running_build(containers, API_SERVICE))
            })
            .flatten();
        checks.push(pin_check(
            &project.state.chap_image_tag,
            ReleaseList::of(probed.map(|p| &p.chap_core)),
            running_build.as_deref(),
        ));
    }
    checks.extend(image_checks(project, probed, have_cli));
    checks.extend(registry_pin_checks(ctx, project, &catalogue));
    checks.extend(user_checks(project, daemon));
    checks.push(match daemon {
        true => stack_check(project, containers.as_deref(), &running),
        false => no_daemon("health"),
    });
    checks
}

/// A project check that needs the daemon, when `docker info` did not answer:
/// a skip that says so, rather than a verdict on what docker would not say.
fn no_daemon(name: &str) -> Check {
    Check::skip_with(
        name,
        name,
        "the docker daemon is not answering",
        "start Docker (see the `docker daemon` line), then run `chap-devops doctor` again",
    )
}

#[cfg(test)]
mod tests;
