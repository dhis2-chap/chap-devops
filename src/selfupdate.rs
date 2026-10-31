//! `chap-devops` updating `chap-devops`: which release asset this build wants, how to
//! verify it, how to put it in place, and the once-a-day notice that a newer
//! one exists.
//!
//! [`crate::chapcore`] is the equivalent for chap-core's images; the version
//! comparison and the SHA-256 implementation are shared with it rather than
//! written twice. Everything here is pure except [`latest_release`],
//! [`release_by_tag`] and [`download`], so the interesting parts are testable
//! without a network.

mod check;
mod install;
mod release;

pub use check::{
    CheckState, checks_disabled, dev_notice_line, is_stale, notice_line, now_unix, read_check,
    write_check,
};
#[cfg(any(windows, test))]
pub use install::clean_backup;
pub use install::{
    download, download_text, extract, find_binary, is_replaceable, replace_executable, verify,
};
#[cfg(test)]
pub use release::install_method_of;
pub use release::{
    Release, dev_update_available, install_method, is_newer_than_current, latest_release,
    pick_asset, release_by_tag, release_commit, running_binary,
};

use std::time::Duration;

/// The repository releases are published from.
pub const REPO: &str = "dhis2-chap/chap-devops";

/// Release endpoint for the newest final release.
///
/// A function rather than a constant because `CHAP_DEVOPS_GITHUB_API` moves the
/// REST base for the tests, and this is one of the calls it has to move: a
/// test that asked the real GitHub would spend a request of somebody's hourly
/// quota to answer a question it already knows the answer to.
pub fn latest_release_url() -> String {
    format!("{}/repos/{REPO}/releases/latest", crate::github::api_base())
}

/// One release by its tag.
pub fn release_url(tag: &str) -> String {
    format!(
        "{}/repos/{REPO}/releases/tags/{tag}",
        crate::github::api_base()
    )
}

/// The triple this binary was built for, from the build script.
pub const TARGET: &str = env!("TARGET");

/// The short commit of the checkout this binary was built from, empty when
/// there was no git repository to ask.
pub const GIT_REVISION: &str = env!("GIT_REVISION");

/// The version this binary reports, without a leading `v`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The channel this build follows, from the build script: `stable` or `dev`.
pub const BUILD_CHANNEL: &str = env!("CHAP_DEVOPS_BUILD_CHANNEL");

/// Whether the release workflow built this binary, from the build script.
pub const RELEASE_BUILD: bool = !env!("CHAP_DEVOPS_RELEASE_BUILD").is_empty();

/// The tag the rolling pre-release of `main` is published under.
///
/// One tag that moves, rather than one per commit: the release it names is
/// always the newest build of `main`, so `releases/download/dev/<asset>` is a
/// URL that keeps working.
pub const DEV_TAG: &str = "dev";

/// `User-Agent` sent with every request, matching the registry fetch.
const USER_AGENT: &str = concat!("chap-devops/", env!("CARGO_PKG_VERSION"));

/// The file inside the cache directory holding the last update check.
pub const CHECK_FILE: &str = "self-update-check.json";

/// How long an update check is considered fresh.
pub const CHECK_INTERVAL: Duration = Duration::from_hours(24);

/// Timeout for the background check, short enough that a slow network costs a
/// command nothing worth noticing.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// Set to `1` to turn the once-a-day update notice off entirely.
pub const NO_CHECK_ENV: &str = "CHAP_DEVOPS_NO_UPDATE_CHECK";

/// Largest archive `self update` will read into memory.
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

/// The name of the checksum manifest attached to every release.
pub const SUMS_FILE: &str = "SHA256SUMS";

// --------------------------------------------------------------------------
// Channels
// --------------------------------------------------------------------------

/// Which release series a build follows.
///
/// The two are not versions of each other: a `dev` build carries the Cargo
/// version of the last tag, because it is built from `main` after that tag,
/// so comparing them as versions says nothing. What separates two dev builds
/// is the commit they came from, which is why the dev side of this module
/// compares commits and the stable side compares versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Channel {
    /// Built from a `vX.Y.Z` tag, or built anywhere else without being told
    /// otherwise: what `releases/latest` serves.
    #[default]
    Stable,
    /// The rolling build of `main`, published as the `dev` pre-release.
    Dev,
}

impl Channel {
    /// The name this channel is printed and recorded under.
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Dev => "dev",
        }
    }

    /// The channel `name` spells. Anything but `dev` is stable, so an unset or
    /// misspelled value can only ever mean the conservative answer.
    pub fn from_name(name: &str) -> Channel {
        if name.trim().eq_ignore_ascii_case("dev") {
            Channel::Dev
        } else {
            Channel::Stable
        }
    }

    /// The release tag this channel follows without an explicit `--version`,
    /// or `None` for stable, which asks `releases/latest` instead.
    pub fn tag(self) -> Option<&'static str> {
        match self {
            Channel::Stable => None,
            Channel::Dev => Some(DEV_TAG),
        }
    }
}

/// The channel this binary was built on.
pub fn channel() -> Channel {
    Channel::from_name(BUILD_CHANNEL)
}

// --------------------------------------------------------------------------
// Assets
// --------------------------------------------------------------------------

/// The target whose archive `target` should install.
///
/// macOS is the one place where a build does not take its own triple: the
/// universal archive carries both slices, and it is what the install script and
/// the download table point at, so a single-architecture Mac build still updates to it.
pub fn asset_target(target: &str) -> &str {
    if target.ends_with("-apple-darwin") {
        "universal-apple-darwin"
    } else {
        target
    }
}

/// `tar.gz` everywhere but Windows, which gets `zip`.
pub fn archive_extension(target: &str) -> &str {
    if target.contains("-windows-") {
        "zip"
    } else {
        "tar.gz"
    }
}

/// The release asset `target` wants, e.g.
/// `chap-devops-universal-apple-darwin.tar.gz`.
///
/// No version in the name: the tag is already in the download URL, and one
/// name per target is what `releases/latest/download/<name>` needs to keep a
/// link working across releases. The directory inside the archive does carry
/// the version, because that is what someone sees after unpacking it.
pub fn asset_name(target: &str) -> String {
    let target = asset_target(target);
    format!("chap-devops-{target}.{}", archive_extension(target))
}

/// The name of the executable inside the archive.
pub fn binary_name(target: &str) -> &str {
    if target.contains("-windows-") {
        "chap-devops.exe"
    } else {
        "chap-devops"
    }
}

/// Where a release's assets are served from, without the file name.
pub fn download_base(tag: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{tag}")
}

#[cfg(test)]
mod tests;
