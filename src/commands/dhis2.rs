//! `chap-devops dhis2` — the three things that let the DHIS2 Modeling App reach Chap.
//!
//! Phase 1 of the `dhis2` component put a DHIS2 and a Chap side by side and
//! left them unable to talk. This is the other half, and it is entirely
//! requests to DHIS2: the app reaches chap-core through a DHIS2 Route, reads
//! its figures out of the `analytics_*` tables, and does not exist at all until
//! it is installed. [`crate::dhis2`] holds the protocol and the rules; this
//! module is the command surface, the waits and what is said about them.
//!
//! Four verbs that change DHIS2, one that does not, and one that changes
//! only this deployment's record of which DHIS2 it is:
//!
//! - `show` asks and reports, and is safe on anything;
//! - `route` creates the `chap` route, or **repoints** one that is there;
//! - `analytics` generates the tables and waits for them;
//! - `apps` installs the Modeling App and the Climate App from the App Hub;
//! - `connect` does the route, the apps and then analytics, in that order;
//! - `use` records a DHIS2 that runs elsewhere, which the others then talk to
//!   in place of the `dhis2` component.
//!
//! Every one of them is idempotent: run twice and the second run says there was
//! nothing to do. Nothing here is done by `chap-devops up`, and
//! [the chapter](../../docs/dhis2.md) says why.

use crate::cli::{
    Dhis2AnalyticsArgs, Dhis2AppsArgs, Dhis2CommonArgs, Dhis2ConnectArgs, Dhis2RouteArgs,
    Dhis2ShowArgs, Dhis2UseArgs,
};
use crate::commands::Ctx;
use crate::components::Component;
use crate::dhis2::{
    self, AuthKind, CredentialSource, Dhis2, HubAppRef, Progress, Ready, RouteAction,
};
use crate::docker;
use crate::error::Result;
use crate::open::{Openable, resolve};
use crate::output::Out;
use crate::project::Project;
use serde::Serialize;
use std::time::{Duration, Instant};

mod analytics;
mod apps;
mod connect;
mod human;
mod route;
mod show;
mod use_external;

pub use analytics::*;
pub use apps::*;
pub use connect::*;
use human::*;
pub use route::*;
pub use show::*;
pub use use_external::*;

/// Why the route has nothing to point at on a deployment without chap-core.
///
/// `--without chap-core` is a supported shape - a deployment that is only a
/// DHIS2 and an OCS - and on one of those the route would resolve to a
/// container that does not exist, which is worse than no route at all.
const NO_CHAP_CORE: &str = "chap-core is not a component of this deployment, so the route would \
     point at a service that is not there; run `chap-devops components enable chap-core` first";

/// What to say when docker knows DHIS2 is not running.
const NOT_RUNNING: &str = "no dhis2 container is running, so its API cannot be asked; run \
     `chap-devops up` to start this deployment";

/// Why there is no DHIS2 here to ask.
///
/// [`crate::open::off_reason`] says the same thing about the same component and
/// ends in "nothing to open", which is the browser's verb; these commands make
/// requests, so they say what they cannot do.
const NO_DHIS2: &str = "dhis2 is not a component of this deployment, so there is no DHIS2 to ask; \
     run `chap-devops components enable dhis2` to add it";

/// Why a DHIS2 that publishes no host port cannot be asked from this machine.
fn no_host_port(inside: &str) -> String {
    format!(
        "dhis2 publishes no host port, so its API cannot be reached from this machine: it answers \
         at {inside} inside the deployment; run `chap-devops components enable dhis2 --port N` to \
         publish one"
    )
}

// ---------------------------------------------------------------------------
// What the reports are made of
// ---------------------------------------------------------------------------

/// The instance every verb opens with: where it is, what it is, and who it was
/// asked as.
///
/// `credential_from` is where the password or token was found and never the
/// secret itself, which is also the only thing about it a human report prints.
#[derive(Debug, Clone, Serialize)]
pub struct Instance {
    pub url: String,
    /// DHIS2's own version, or empty when it did not say.
    pub version: String,
    /// Who the requests went as. For a token, whoever DHIS2 said owns it.
    pub user: String,
    /// Whether this is an external DHIS2 recorded by `chap-devops dhis2 use`,
    /// rather than the deployment's own `dhis2` component.
    pub external: bool,
    /// A password or a personal access token.
    pub auth: AuthKind,
    pub credential_from: CredentialSource,
}

/// What `route` did to the `chap` route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RouteOutcome {
    /// There was none, and now there is one.
    Created,
    /// There was one, aimed somewhere else or broken, and it was rewritten.
    Repointed,
    /// There was one, already pointing here. Nothing was written.
    Unchanged,
}

/// What became of the route.
#[derive(Debug, Clone, Serialize)]
pub struct RouteReport {
    pub outcome: RouteOutcome,
    pub code: &'static str,
    /// Where it points now.
    pub url: String,
    /// Where it pointed before, for a route that was repointed.
    pub previous: Option<String>,
    /// Why it had to be rewritten, one clause each.
    pub reasons: Vec<String>,
    pub id: Option<String>,
    /// Whether chap-core answered a request proxied through it. This is the
    /// check worth making: it proves the whole path rather than the row.
    pub verified: bool,
    /// What chap-core said through the route, or why nothing did.
    pub answered: String,
    /// Whether chap-core refused the API token the route carries. Only for
    /// the human lines; `verified` already says it to a script.
    #[serde(skip)]
    pub token_refused: bool,
    /// What to do about a route that nothing answered through, empty when it
    /// was answered. Only for the human lines.
    #[serde(skip)]
    pub way_out: String,
}

/// What became of the analytics tables.
#[derive(Debug, Clone, Serialize)]
pub struct AnalyticsReport {
    /// The DHIS2 job, empty when its id could not be read off the answer.
    pub job: String,
    /// Whether this run started the job, as opposed to watching one that was
    /// already going. DHIS2 runs one at a time.
    pub started: bool,
    /// Whether it had finished when the command stopped watching.
    pub finished: bool,
    pub seconds: u64,
    /// The last step DHIS2 announced.
    pub message: String,
    /// What DHIS2 records as the last successful analytics run afterwards.
    pub last_success: String,
}

/// What became of one app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppOutcome {
    Installed,
    /// A different version was installed, and this one replaced it.
    Moved,
    /// The version this instance can run was already installed.
    Unchanged,
    Failed,
}

/// One app, and what happened to it.
#[derive(Debug, Clone, Serialize)]
pub struct AppReport {
    /// The App Hub's own name for it.
    pub name: String,
    pub outcome: AppOutcome,
    /// The version that is installed now, or that was going to be.
    pub version: String,
    /// The version that was there before, for one that moved.
    pub previous: Option<String>,
    pub reason: Option<String>,
    /// The unstable Modeling App build that is installed now, which the
    /// state records. `None` for an App Hub version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<crate::components::ModelingAppBuild>,
}

/// Every app this command touched.
#[derive(Debug, Clone, Serialize)]
pub struct AppsReport {
    pub apps: Vec<AppReport>,
}

/// What one `connect` left in `.chap-devops/components.yaml` about itself.
///
/// The record is [`crate::components::Dhis2Component::connected_at`], and its
/// only job is to stop `chap-devops up` and `chap-devops status` asking for a connect that
/// has already happened. It is never read as evidence that the route is right:
/// `chap-devops dhis2 show` is the one command that asks DHIS2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectRecord {
    /// Written now: the route is verified and both apps are in place, or, on
    /// an external DHIS2, where `connect` sets only the route, it is verified.
    Recorded,
    /// An earlier record was cleared, because this run found something broken.
    /// The hint comes back, which is the answer that errs the safe way.
    Cleared,
    /// The file was not touched: either the run judged nothing (see
    /// [`Judgement::Unknown`]) or there was nothing recorded to clear.
    Unchanged,
}

/// What one run of `route`, `analytics`, `apps` or `connect` did.
///
/// One shape for all four, so `--json` reads the same whichever was typed and a
/// step that was not part of the command is `null` rather than absent.
#[derive(Debug, Clone, Serialize)]
pub struct Dhis2Report {
    pub instance: Instance,
    pub route: Option<RouteReport>,
    pub apps: Option<AppsReport>,
    pub analytics: Option<AnalyticsReport>,
    /// Steps that were not attempted, and why. A skip is reported, never
    /// swallowed.
    pub skipped: Vec<String>,
    /// What this run left recorded about itself. `null` for `route`,
    /// `analytics` and `apps`, none of which records anything: one step is not
    /// a connection, and a route with no apps is a DHIS2 nobody can use.
    pub record: Option<ConnectRecord>,
    /// What `connect` did about chap-core's configured models (see
    /// [`crate::configs::sync`]). `null` for the other commands.
    pub configured: Option<crate::configs::sync::Folded>,
    /// The command to run next.
    pub next: String,
}

/// The `chap` route as it stands, for `show`.
#[derive(Debug, Clone, Serialize)]
pub struct ShownRoute {
    pub url: String,
    pub disabled: bool,
    pub authorised: bool,
    /// Whether it points at this deployment's chap-core.
    pub ours: bool,
    pub verified: bool,
    pub answered: String,
    /// Whether chap-core answered `/health` through it and turned the next
    /// call away for want of its API token.
    pub token_refused: bool,
    /// What to do about a route that nothing answered through, empty when it
    /// was answered. Only for the human lines.
    #[serde(skip)]
    pub way_out: String,
}

/// One of the two apps chap-devops installs, as `show` found it.
///
/// One row per app chap-devops is about, present or not - never a row per app the
/// instance happens to have. A real DHIS2 ships 29 bundled apps of its own, and
/// listing them buried the two this command answers for.
#[derive(Debug, Clone, Serialize)]
pub struct ShownApp {
    /// The name chap-devops knows it by, which is also what the docs call it.
    pub name: String,
    /// Whether this instance has it at all.
    pub installed: bool,
    /// The version it has, empty when it does not have it.
    pub version: String,
    /// The unstable build chap-devops recorded when it installed it, such as
    /// `unstable 0c30f8d`. DHIS2 lists that build under the version of the
    /// release before it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
}

/// What `show` found.
#[derive(Debug, Clone, Serialize)]
pub struct ShowReport {
    pub instance: Instance,
    /// Where the route has to point for this deployment.
    pub target: String,
    /// The route that is there, or `null` when there is none.
    pub route: Option<ShownRoute>,
    /// When analytics last succeeded, as DHIS2 records it. Empty for an
    /// instance that has never run it.
    pub last_analytics: String,
    /// What that timestamp is worth. A seeded deployment inherits it from the
    /// dump, so on its own it says nothing about this deployment's tables.
    pub analytics: dhis2::AnalyticsEvidence,
    /// The two apps chap-devops installs, in order, present or not.
    pub apps: Vec<ShownApp>,
    /// Whichever of the three is missing, one clause each.
    pub missing: Vec<String>,
    pub next: String,
}

// ---------------------------------------------------------------------------
// The session every verb opens with
// ---------------------------------------------------------------------------

/// A DHIS2 that has answered, and the deployment it belongs to.
struct Session {
    project: Project,
    dhis2: Dhis2,
    ready: Ready,
}

impl Session {
    /// The instance line every report opens with.
    fn instance(&self) -> Instance {
        Instance {
            url: self.dhis2.base().to_string(),
            version: self.ready.version.clone(),
            user: self.ready.user.clone(),
            external: self.external().is_some(),
            auth: self.dhis2.credentials().kind(),
            credential_from: self.dhis2.credentials().source,
        }
    }

    /// The external DHIS2 this session talks to, if that is what it is.
    fn external(&self) -> Option<&crate::components::ExternalDhis2> {
        self.project.state.components.dhis2_external.as_ref()
    }

    /// Where this deployment's route has to point: the compose network's name
    /// for chap-core for a DHIS2 on that network, and the recorded URL for one
    /// that is not.
    ///
    /// A chap-core elsewhere is reached from the DHIS2 container over the host
    /// gateway, which is how the compose file maps it for that DHIS2.
    fn target(&self) -> String {
        match (
            self.external(),
            &self.project.state.components.chap_core_external,
        ) {
            (Some(external), _) => dhis2::external_route_target(&external.chap_url),
            (None, Some(chap_core)) => {
                dhis2::external_route_target(&chap_core.url_from_containers())
            }
            (None, None) => dhis2::route_target(&self.project.root_path()),
        }
    }
}

/// Resolve the instance, check that asking it is possible, and wait for it.
///
/// Four refusals before a single request is made, each naming what is true
/// instead and the command that changes it: a deployment without the component,
/// one whose DHIS2 publishes no host port and so cannot be reached from this
/// machine at all, one whose container is not running, and one that never
/// answers within the wait. A docker that cannot be asked is not a refusal -
/// that would refuse on every machine docker is absent from - so the container
/// check degrades to a trace line and the request is made anyway.
fn open_session(ctx: &Ctx, common: &Dhis2CommonArgs) -> Result<Session> {
    let project = ctx.project()?;
    let components = &project.state.components;
    // An external DHIS2 has no container to look for: its URL is what was
    // recorded, and the wait below is the whole of the check.
    let client = match &components.dhis2_external {
        Some(external) => {
            let credentials =
                dhis2::credentials_for(&project.dir, common.user.as_deref(), false, false)?;
            Dhis2::new(&external.url, credentials, dhis2::DEFAULT_TIMEOUT).external()
        }
        None => {
            let base = match resolve(Component::Dhis2, components, &project.api_base()) {
                Openable::Url { url, .. } => url,
                Openable::Internal { inside } => {
                    return Err(anyhow::anyhow!(no_host_port(&inside)));
                }
                Openable::Off | Openable::NoWeb => return Err(anyhow::anyhow!(NO_DHIS2)),
            };
            match docker::running_containers_or_why(&project) {
                Ok(containers) => {
                    if !docker::running_of(&containers).contains(Component::Dhis2.service()) {
                        return Err(anyhow::anyhow!(NOT_RUNNING));
                    }
                }
                Err(why) => ctx.out.verbose(&format!(
                    "docker could not be asked whether dhis2 is running ({why}), so DHIS2 is \
                     asked directly"
                )),
            }
            // The seed password only when the database came from a seed.
            let seed_password =
                components.dhis2_seed_source().is_some() && components.dhis2.seed_password;
            let credentials =
                dhis2::credentials_for(&project.dir, common.user.as_deref(), true, seed_password)?;
            Dhis2::new(&base, credentials, dhis2::DEFAULT_TIMEOUT)
        }
    };
    let base = client.base().to_string();
    let external = components.dhis2_external.is_some();
    let ready = client.wait_for_api(
        Duration::from_secs(
            common
                .wait
                .unwrap_or(match components.dhis2_external.is_some() {
                    true => crate::dhis2::EXTERNAL_API_WAIT,
                    false => crate::dhis2::DEFAULT_API_WAIT,
                }),
        ),
        dhis2::POLL_INTERVAL,
        // A command that is about to sit here for twenty minutes says so, on
        // stderr, so a `--json` stdout stays one document. A DHIS2 that is
        // already answering says nothing at all.
        &mut |wait| {
            crate::output::notice(&format!(
                "DHIS2 at {base} is not answering {} yet; waiting up to {}{}",
                dhis2::PING_PATH,
                crate::output::human_age(wait),
                match external {
                    true => "",
                    false => ", and `chap-devops logs dhis2` is where the migration shows",
                }
            ));
        },
    )?;
    ctx.out.verbose(&format!(
        "DHIS2 {} answered in {}",
        ready.version,
        crate::output::human_age(ready.waited)
    ));
    Ok(Session {
        project,
        dhis2: client,
        ready,
    })
}

#[cfg(test)]
mod tests;
