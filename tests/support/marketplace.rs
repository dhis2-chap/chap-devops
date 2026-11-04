//! The frozen marketplace data, put in a test's registry cache.
//!
//! The binary falls back to the snapshot in `vendor/marketplace/`, which
//! follows upstream. The tests must not: a model the marketplace drops would
//! break every test that names it. So each sandbox starts with
//! `tests/fixtures/marketplace/` as a fresh cache entry for the default
//! registry URL, and `--offline` reads that entry before the snapshot.
//!
//! Shared with `#[path]` by every test crate that runs the binary.

use std::path::Path;

/// The registry URL the cache entry is for: `DEFAULT_REGISTRY_URL` in
/// `src/registry.rs`.
const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/dhis2-chap/chap-marketplace/main/registry.yaml";

/// Write the frozen marketplace data into `cache_dir` as the cache entry
/// `src/registry/cache.rs` reads for the default registry.
pub fn seed_registry_cache(cache_dir: &Path) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/marketplace");
    let entry = cache_dir.join("registries").join(cache_key(REGISTRY_URL));
    let models = entry.join("models");
    std::fs::create_dir_all(&models).expect("the cache entry can be created");
    std::fs::copy(fixture.join("registry.yaml"), entry.join("registry.yaml"))
        .expect("the fixture registry.yaml is copied");
    for file in std::fs::read_dir(fixture.join("models")).expect("the fixture models are there") {
        let file = file.expect("a fixture model file");
        std::fs::copy(file.path(), models.join(file.file_name()))
            .expect("a fixture model file is copied");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs();
    std::fs::write(
        entry.join("meta.json"),
        format!("{{\"url\": \"{REGISTRY_URL}\", \"fetched_at_unix\": {now}}}\n"),
    )
    .expect("meta.json is written");
}

/// `cache_key` in `src/registry/cache.rs`: the URL as a slug, then its
/// FNV-1a hash.
fn cache_key(url: &str) -> String {
    let mut slug = String::with_capacity(url.len());
    for ch in url.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let slug = slug.trim_matches('_');
    let slug = &slug[..slug.len().min(64)];
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{slug}-{hash:016x}")
}
