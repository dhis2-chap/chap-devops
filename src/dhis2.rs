//! DHIS2's Web API, and the three things that let the Modeling App reach Chap.
//!
//! Nothing in a Chap deployment talks to DHIS2 and nothing in DHIS2 talks to
//! chap-core. The only thing that talks to both is the Modeling App running in
//! a browser, and it reaches chap-core through **DHIS2's Route API**: a `Route`
//! row inside DHIS2 with `code: "chap"`, which DHIS2 then reverse-proxies under
//! `/api/routes/chap/run/`. So the work of connecting the two is three
//! requests to DHIS2 and none to chap-core:
//!
//! - the route, or the app redirects to `/get-started`;
//! - the analytics tables, or the app has nothing to send;
//! - the apps themselves, installed from the App Hub, or there is no Chap user
//!   interface at all.
//!
//! This is a second HTTP client rather than a second caller of
//! [`crate::api`]. That one is chap-core's: it sends `Authorization: Bearer`,
//! and its transport failure is the sentence `chap-devops status` prints about
//! chap-core. DHIS2 takes HTTP Basic, answers a `WebMessage` rather than
//! FastAPI's `{"detail": ...}`, and a DHIS2 that is not answering is a
//! different sentence with a different command at the end of it. The response
//! shape is shared - [`crate::api::Answer`] - because a status, a body and a
//! content type are the same three things whoever sent them.
//!
//! The password is never printed, never traced and never returned by
//! [`Credentials`]'s `Debug`. `-v` narrates the request line and says that an
//! `Authorization: Basic` header went out, exactly as [`crate::api`] does for
//! the token.

mod analytics;
mod apps;
mod client;
mod credentials;
mod route;
mod unstable;

pub use analytics::{
    AnalyticsEvidence, Progress, analytics_evidence, finished_here, job_id_of, jobs_path,
    queued_job, queued_jobs_query, recorded_success_of, running_job, shown_success,
};
pub use apps::{
    InstalledApp, OFFLINE_APPS, app_hub_base, exact_version, fetch_hub_app, install_path,
    installed_app, pick_version,
};
pub use client::{Dhis2, Ready, said};
pub use credentials::{
    AuthKind, CREDENTIALS_WAY_OUT, CredentialSource, Credentials, credentials_for,
    describe_credential,
};
pub use route::{
    RouteAction, allowlist_hint, external_route_target, is_allowlist_refusal, route_action,
    route_in, route_origin, route_payload, route_run_path, route_target, routes_query,
};
pub use unstable::{FRONTEND_REPO, UNSTABLE_ASSET, app_upload, download_unstable, fetch_unstable};

use std::time::Duration;

/// The one route DHIS2 answers without credentials, and what the wait polls.
pub const PING_PATH: &str = crate::status::DHIS2_PING_PATH;

/// Who the credential belongs to, for a token that does not say.
pub const ME_PATH: &str = "/api/me?fields=username";

/// Where DHIS2 reports its own version, behind authentication.
pub const SYSTEM_INFO_PATH: &str = "/api/system/info";

/// The Route API collection.
pub const ROUTES_PATH: &str = "/api/routes";

/// The `code` the Modeling App looks the route up by. Not a name and not an
/// id: the app asks for `/api/routes/chap/run/...` and nothing else will do.
pub const ROUTE_CODE: &str = "chap";

/// The route's `name`, which is only what DHIS2's own maintenance app shows.
pub const ROUTE_NAME: &str = "Chap Modeling App";

/// The authority a DHIS2 user needs to be allowed through the route.
pub const ROUTE_AUTHORITY: &str = "F_CHAP_MODELING_APP";

/// What a route URL has to end in for DHIS2 to proxy paths under it.
///
/// Without it the route proxies exactly one path, so every request the app
/// makes past `/health` answers 404.
pub const ROUTE_SUFFIX: &str = "/**";

/// Seconds DHIS2 waits for chap-core to answer through the route.
///
/// chap-core answers its own endpoints in milliseconds; what takes time there
/// is a job, and a job is started and polled rather than waited on.
pub const ROUTE_TIMEOUT_SECONDS: u32 = 30;

/// The route `auth` type that sends fixed headers with every proxied request,
/// which is how chap-core's bearer token rides along.
///
/// DHIS2 stores the headers and never hands them back: a listing shows
/// `"auth": {"type": "api-headers"}` and nothing else, so whether the token in
/// there is the current one is only ever known by asking through the route.
pub const ROUTE_AUTH_TYPE: &str = "api-headers";

/// Analytics generation, with tracked entities left out.
///
/// **No `lastYears`.** The parameter looks like an optimisation and is a trap:
/// measured on the climate demo, `&lastYears=8` produced zero rows in every
/// analytics table while reporting success in 16.9 seconds, where the same run
/// without it wrote 146,129 rows into `analytics_2024` in 15.7. A silent
/// success that populates nothing leaves the Modeling App with no data and
/// nothing on screen explaining why, so chap-devops never sends it.
pub const ANALYTICS_PATH: &str = "/api/resourceTables/analytics?skipTrackedEntities=true";

/// The job type analytics runs under, which is also its notifier's key.
pub const ANALYTICS_JOB_TYPE: &str = "ANALYTICS_TABLE";

/// Where a job's notifications are read from.
pub const TASKS_PATH: &str = "/api/system/tasks";

/// Where DHIS2 lists its jobs, the ones that wait to run included.
pub const JOB_CONFIGURATIONS_PATH: &str = "/api/jobConfigurations";

/// The installed apps of an instance.
pub const APPS_PATH: &str = "/api/apps";

/// Server-side installation from the App Hub: `POST /api/appHub/{versionId}`.
///
/// DHIS2 downloads the app itself, so there is nothing to fetch here and no
/// multipart upload to build - but it does mean the instance needs to reach
/// the App Hub, which is why this is the one step `--offline` refuses.
pub const APP_HUB_INSTALL_PATH: &str = "/api/appHub";

/// The variable the App Hub base URL is moved with, for the tests.
pub const APP_HUB_VAR: &str = "CHAP_DEVOPS_APP_HUB";

/// The public App Hub, unless [`APP_HUB_VAR`] points somewhere else.
pub const DEFAULT_APP_HUB: &str = "https://apps.dhis2.org/api/v1/apps";

/// `User-Agent` sent with every request, matching the rest of the CLI.
const USER_AGENT: &str = concat!("chap-devops/", env!("CARGO_PKG_VERSION"));

/// How long one request may take, all of connect, send and receive.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long `POST /api/appHub/{versionId}` may take.
///
/// Longer than the rest: DHIS2 downloads the app from the App Hub inside that
/// request, so it is a transfer rather than a query.
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);

/// How long to wait for DHIS2's API by default: twenty minutes.
///
/// DHIS2 migrates its whole schema before it serves a request, and a seeded
/// deployment restores the dump before that; under emulation the two together
/// are a quarter of an hour. The reference deployment waits exactly this long,
/// and a shorter default would turn a slow first start into a failure.
pub const DEFAULT_API_WAIT: u64 = 1200;

/// How long to wait for a DHIS2 that runs elsewhere by default: one minute.
///
/// An external DHIS2 is not migrating behind this deployment's back; it is up
/// or it is not, and a mistyped URL or one behind single sign-on should fail
/// in a minute rather than after twenty.
pub const EXTERNAL_API_WAIT: u64 = 60;

/// How long to wait for an analytics run by default: one hour.
///
/// Tens of seconds on demo data and much longer on real data, and a run that
/// is cut short is a set of half-populated tables.
pub const DEFAULT_ANALYTICS_TIMEOUT: u64 = 3600;

/// How often the waits ask again.
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// The `.env` variable naming the DHIS2 user chap-devops authenticates as.
///
/// Only chap-devops reads it: it is not passed to any container, so it never reaches
/// a compose file or the DHIS2 process.
pub const ADMIN_USERNAME_ENV_VAR: &str = "DHIS2_ADMIN_USERNAME";

/// The `.env` variable holding that user's password. Also chap-devops's alone.
pub const ADMIN_PASSWORD_ENV_VAR: &str = "DHIS2_ADMIN_PASSWORD";

/// The `.env` variable holding a DHIS2 personal access token. Also chap-devops'
/// alone, and preferred to the password pair when both are set.
pub const API_TOKEN_ENV_VAR: &str = "DHIS2_API_TOKEN";

/// The environment variable a password is read from when `.env` has none for
/// the user being asked as.
///
/// For an operator who will not have the password on disk: exporting it for
/// one shell is the way to run these commands without writing it down.
pub const PASSWORD_ENV_VAR: &str = "CHAP_DEVOPS_DHIS2_PASSWORD";

/// The user [`PASSWORD_ENV_VAR`] belongs to, when it is not the one `.env`
/// names.
pub const USERNAME_ENV_VAR: &str = "CHAP_DEVOPS_DHIS2_USERNAME";

/// The environment variable a personal access token is read from.
pub const TOKEN_ENV_VAR: &str = "CHAP_DEVOPS_DHIS2_TOKEN";

/// The user a DHIS2 with no `.env` line is asked as.
pub const DEFAULT_USERNAME: &str = "admin";

/// That user's password on a DHIS2 chap-devops deployed.
///
/// The same two words whichever way the database was created: the published
/// demo dumps ship this user, and a Flyway-bootstrapped empty database gets it
/// from `DefaultAdminUserPopulator`, which has it compiled in. It is a default,
/// not a secret - every DHIS2 tutorial in the world prints it - and an instance
/// whose password has been changed says so with [`ADMIN_PASSWORD_ENV_VAR`].
///
/// Never tried against an external DHIS2: chap-devops did not create that instance,
/// so it knows nothing about its passwords, and sending the tutorial one to a
/// production server is a failed login in its audit log and nothing else.
pub const DEFAULT_PASSWORD: &str = "district";

/// The App Hub id of the Modeling App, the one app whose build
/// `--modeling-app` chooses.
pub const MODELING_APP_ID: &str = "a29851f9-82a7-4ecd-8b2c-58e0f220bc75";

/// The two apps a Chap deployment wants, in the order they are installed.
pub const HUB_APPS: &[HubAppRef] = &[
    HubAppRef {
        id: MODELING_APP_ID,
        name: "Modeling App",
        label: "the Modeling App",
        what: "the Chap user interface inside DHIS2",
    },
    HubAppRef {
        id: "effb986c-a3c7-485e-a2f6-5e54ff9df7c3",
        name: "DHIS2 Climate App",
        label: "the Climate App",
        what: "imports climate data into DHIS2 through Google Earth Engine",
    },
];

/// One app chap-devops installs, as the App Hub identifies it.
///
/// The id is the App Hub's own, and the version to install is resolved from it
/// rather than pinned: the App Hub says which versions exist and what each one
/// needs of DHIS2, and a pin here would go stale in the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubAppRef {
    pub id: &'static str,
    /// The App Hub's own name, which is what an installed app is matched on
    /// when there is no App Hub to ask - `chap-devops dhis2 show` reaches only DHIS2.
    pub name: &'static str,
    /// What a report calls it, in the words a sentence needs.
    pub label: &'static str,
    /// What it is for, for the line that explains why it is being installed.
    pub what: &'static str,
}
// ---------------------------------------------------------------------------
// Versions, and the one encoding
// ---------------------------------------------------------------------------

/// The numeric parts of a version, `7.1.0` as `[7, 1, 0]`.
///
/// Leading digits of each dot-separated part, so a pre-release suffix is
/// dropped rather than read as a fourth number: `1.16.2-rc1` is `[1, 16, 2]`.
/// It stops at the first part that does not begin with a digit, which is what
/// makes an empty string - an instance that did not say its version - an empty
/// list rather than a version 0 that nothing is compatible with.
pub fn version_parts(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
        })
        .take_while(|digits| !digits.is_empty())
        .map(|digits| digits.parse().unwrap_or(0))
        .collect()
}

/// [`version_parts`] with DHIS2's historical `2.` prefix dropped.
///
/// The same release is written `2.42` and `42` depending on who is writing it -
/// the App Hub says `minDhisVersion: "2.40"`, other places say `40` - and
/// comparing the two spellings as they stand would make every app look
/// incompatible. Dropping a leading `2` in front of something else normalises
/// both to the minor line.
pub fn dhis_version_parts(version: &str) -> Vec<u64> {
    let parts = version_parts(version);
    match parts.split_first() {
        Some((2, rest)) if !rest.is_empty() => rest.to_vec(),
        _ => parts,
    }
}

/// Compare two version strings part by part, a missing part counting as 0.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    compare_parts(&version_parts(a), &version_parts(b))
}

/// [`compare_versions`] on parts already read.
fn compare_parts(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
    let width = a.len().max(b.len());
    for at in 0..width {
        let ordering = a.get(at).unwrap_or(&0).cmp(b.get(at).unwrap_or(&0));
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    std::cmp::Ordering::Equal
}

/// Standard base64, for the one header that needs it.
///
/// Written out rather than pulled in, the way [`crate::api::encode`] is: HTTP
/// Basic is the only thing in this CLI that encodes anything, and the binary
/// stays dependency-light.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let bits = ((chunk[0] as u32) << 16)
            | ((*chunk.get(1).unwrap_or(&0) as u32) << 8)
            | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(ALPHABET[(bits >> 18 & 63) as usize] as char);
        out.push(ALPHABET[(bits >> 12 & 63) as usize] as char);
        out.push(match chunk.len() {
            1 => '=',
            _ => ALPHABET[(bits >> 6 & 63) as usize] as char,
        });
        out.push(match chunk.len() {
            3 => ALPHABET[(bits & 63) as usize] as char,
            _ => '=',
        });
    }
    out
}

#[cfg(test)]
mod tests;
