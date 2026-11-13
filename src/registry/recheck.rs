//! A second look at the marketplace for a model id the cache does not list.
//!
//! [`crate::registry::load`] uses a cached catalogue for [`CACHE_TTL`]
//! without the network, so a model that was listed after the cache was
//! written is "unknown" until the cache expires. When a command fails on an
//! unknown id, this module fetches the catalogue once. If the id is listed
//! now, the error says so; the refresh also updated the cache, so the next
//! run finds the model.

use super::{CACHE_TTL, Registry, RegistryOptions, cache};
use crate::error::{ChapError, Result};
use std::time::Duration;

/// A cache younger than this was written by the same run, so a second fetch
/// cannot list anything new.
const JUST_FETCHED: Duration = Duration::from_secs(60);

/// The error a command ends with, after a second look for an unknown model.
///
/// Every other error, and an unknown id that the marketplace does not list,
/// comes back as it was.
pub fn explain(opts: &RegistryOptions, err: anyhow::Error) -> anyhow::Error {
    let age = cache::read(opts).ok().flatten().map(|(_, _, age)| age);
    explain_with(opts.offline, age, err, &|| super::update(opts))
}

/// [`explain`] with the cache age and the fetch injected, for the tests.
pub fn explain_with(
    offline: bool,
    cache_age: Option<Duration>,
    err: anyhow::Error,
    fetch: &dyn Fn() -> Result<Registry>,
) -> anyhow::Error {
    let Some(ChapError::UnknownModel(id)) = err.downcast_ref::<ChapError>() else {
        return err;
    };
    // Only a fresh cache hides new models: an old one was fetched again by
    // this run, and no cache means the run used the network or the embedded
    // copy. Offline, there is nothing to ask.
    let Some(age) = cache_age.filter(|age| *age >= JUST_FETCHED && *age < CACHE_TTL) else {
        return err;
    };
    if offline {
        return err;
    }
    match fetch() {
        Ok(registry) if registry.get(id).is_some() => anyhow::anyhow!(
            "`{id}` was not in the marketplace list chap-devops kept from {} ago; \
             chap-devops updated the list, so run the command again",
            crate::output::human_age(age)
        ),
        _ => err,
    }
}

#[cfg(test)]
mod tests;
