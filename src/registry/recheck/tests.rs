use super::*;
use std::cell::Cell;

const LISTED: &str = "chapkit_simple_multistep_model";
const HOUR: Duration = Duration::from_secs(3600);

fn unknown(id: &str) -> anyhow::Error {
    ChapError::UnknownModel(id.to_string()).into()
}

fn embedded() -> Result<Registry> {
    crate::registry::load_embedded()
}

/// A model listed after the cache was written: the error says the list is
/// updated now.
#[test]
fn a_model_listed_after_the_cache_says_to_run_again() {
    let err = explain_with(false, Some(2 * HOUR), unknown(LISTED), &embedded);
    let text = format!("{err:#}");
    assert!(text.contains(&format!("`{LISTED}` was not in")), "{text}");
    assert!(text.contains("2 hours ago"), "{text}");
    assert!(text.contains("run the command again"), "{text}");
}

/// An id the marketplace does not list keeps the first error.
#[test]
fn an_id_nobody_lists_keeps_the_error() {
    let err = explain_with(false, Some(2 * HOUR), unknown("nope"), &embedded);
    assert!(matches!(
        err.downcast_ref::<ChapError>(),
        Some(ChapError::UnknownModel(id)) if id == "nope"
    ));
}

/// No second fetch where it cannot help: offline, a cache this run wrote,
/// no cache, an old cache, and any other error.
#[test]
fn no_second_fetch_where_it_cannot_help() {
    let fetched = Cell::new(0);
    let fetch = || {
        fetched.set(fetched.get() + 1);
        embedded()
    };
    explain_with(true, Some(2 * HOUR), unknown(LISTED), &fetch);
    explain_with(false, Some(Duration::from_secs(5)), unknown(LISTED), &fetch);
    explain_with(false, None, unknown(LISTED), &fetch);
    explain_with(false, Some(CACHE_TTL), unknown(LISTED), &fetch);
    explain_with(false, Some(2 * HOUR), ChapError::OutOfSync.into(), &fetch);
    assert_eq!(fetched.get(), 0);
}

/// A fetch that fails keeps the first error.
#[test]
fn a_failed_fetch_keeps_the_error() {
    let err = explain_with(false, Some(2 * HOUR), unknown(LISTED), &|| {
        Err(anyhow::anyhow!("no network"))
    });
    assert!(matches!(
        err.downcast_ref::<ChapError>(),
        Some(ChapError::UnknownModel(_))
    ));
}
