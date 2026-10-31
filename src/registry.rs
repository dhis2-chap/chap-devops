//! The marketplace catalogue: loading it, and querying what was loaded.

pub mod cache;
pub mod changes;
pub mod embedded;
pub mod fetch;
pub mod model;

pub use model::{
    AssessedStatus, Channel, Kind, Model, RegistryIndex, Version, VersionSelector, VersionStatus,
};

use crate::error::{ChapError, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;

/// Raw URL of the upstream marketplace index.
pub const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/dhis2-chap/model-marketplace/main/registry.yaml";

/// How long a cached snapshot is considered fresh.
pub const CACHE_TTL: Duration = Duration::from_hours(24);

/// Default network timeout for registry requests.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Where a loaded [`Registry`] came from.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Provenance {
    /// Fetched over the network just now.
    Network,
    /// Read from a cache entry younger than [`CACHE_TTL`].
    Cache { age_secs: u64 },
    /// Read from a cache entry older than [`CACHE_TTL`] because the network failed.
    StaleCache { age_secs: u64 },
    /// The snapshot compiled into the binary.
    Embedded,
}

impl Provenance {
    /// One-word label for human output.
    pub fn label(&self) -> &'static str {
        match self {
            Provenance::Network => "network",
            Provenance::Cache { .. } => "cache",
            Provenance::StaleCache { .. } => "stale cache",
            Provenance::Embedded => "embedded",
        }
    }

    /// The label plus the age of the snapshot, where there is one.
    pub fn describe(&self) -> String {
        match self {
            Provenance::Cache { age_secs } | Provenance::StaleCache { age_secs } => format!(
                "{} ({} old)",
                self.label(),
                crate::output::human_age(Duration::from_secs(*age_secs))
            ),
            _ => self.label().to_string(),
        }
    }

    /// Age of a cache-backed snapshot.
    #[cfg(test)]
    pub fn age(&self) -> Option<Duration> {
        match self {
            Provenance::Cache { age_secs } | Provenance::StaleCache { age_secs } => {
                Some(Duration::from_secs(*age_secs))
            }
            _ => None,
        }
    }

    /// Provenance for a cache entry of the given age.
    fn for_age(age: Duration) -> Provenance {
        if age < CACHE_TTL {
            Provenance::Cache {
                age_secs: age.as_secs(),
            }
        } else {
            Provenance::StaleCache {
                age_secs: age.as_secs(),
            }
        }
    }
}

/// Everything [`load`] and [`update`] need to find the catalogue.
#[derive(Debug, Clone)]
pub struct RegistryOptions {
    pub url: String,
    pub offline: bool,
    pub cache_dir: PathBuf,
    pub timeout: Duration,
}

impl Default for RegistryOptions {
    fn default() -> Self {
        RegistryOptions {
            url: DEFAULT_REGISTRY_URL.to_string(),
            offline: false,
            cache_dir: crate::paths::cache_dir(),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

/// A parsed marketplace catalogue.
#[derive(Debug, Clone, Serialize)]
pub struct Registry {
    pub url: String,
    pub provenance: Provenance,
    pub index: RegistryIndex,
    pub models: Vec<Model>,
    /// Each model file that did not parse, which the catalogue goes
    /// without; [`load`] and [`refresh`] warn with them.
    #[serde(skip)]
    pub skipped: Vec<Skipped>,
}

/// A model file that did not parse.
#[derive(Debug, Clone)]
pub struct Skipped {
    /// The path the index lists, such as `models/<id>.yaml`.
    pub path: String,
    /// The warning that names it.
    pub line: String,
}

impl Registry {
    /// Parse an index plus its model files.
    ///
    /// `model_files` are `(relative path as listed in the index, contents)`
    /// pairs; every path the index lists must be present. A model file that
    /// does not parse is left out and named in [`Registry::skipped`]: the
    /// marketplace can add a field value before chap-devops knows it, and
    /// one entry must not take the other models with it.
    pub fn parse(
        url: &str,
        provenance: Provenance,
        index_yaml: &str,
        model_files: &[(String, String)],
    ) -> Result<Registry> {
        let index: RegistryIndex = serde_yaml_ng::from_str(index_yaml)
            .map_err(|e| anyhow::anyhow!("{url}: invalid registry index: {e}"))?;

        let mut models = Vec::with_capacity(index.models.len());
        let mut skipped = Vec::new();
        for path in &index.models {
            let body = model_files
                .iter()
                .find(|(name, _)| name == path)
                .map(|(_, body)| body)
                .ok_or_else(|| anyhow::anyhow!("registry lists {path} but it is missing"))?;
            match serde_yaml_ng::from_str::<Model>(body) {
                Ok(model) if !model.identity_matches(path) => skipped.push(Skipped {
                    path: path.clone(),
                    line: format!(
                        "left out {path} of the marketplace registry: its id is {} and its \
                         service_id is {}, which do not match the file name; report it to the \
                         marketplace",
                        model.id, model.service_id
                    ),
                }),
                Ok(model) => models.push(model),
                Err(e) => skipped.push(Skipped {
                    path: path.clone(),
                    line: format!(
                        "left out {path} of the marketplace registry: {e}; this chap-devops may \
                         be older than the registry, so run `chap-devops self update`"
                    ),
                }),
            }
        }

        Ok(Registry {
            url: url.to_string(),
            provenance,
            index,
            models,
            skipped,
        })
    }

    /// Append a deployment's own model definitions to the catalogue.
    ///
    /// This is what makes `chap-devops models add` cost so little everywhere else:
    /// from here on a manually added model is a [`Model`] like any other, so
    /// `enable`, `sync`, `update`, `list`, `info`, `doctor` and the browser
    /// need no branch for it. The marketplace wins a collision - the
    /// catalogue is the shared truth, and an id it has since published is a
    /// reason to rename the local one, not to shadow it - and every collision
    /// is returned as a warning to show.
    pub fn with_manual(&mut self, manual: &crate::project::ManualModels) -> Vec<String> {
        let mut warnings = Vec::new();
        for (id, entry) in manual {
            if let Some(existing) = self.models.iter().find(|m| m.id == *id) {
                warnings.push(format!(
                    "{id} is in .chap-devops/models-manual.yaml and in the marketplace; \
                     the marketplace entry ({}) is the one being used - \
                     `chap-devops models remove {id}` drops the local one",
                    existing.display_name
                ));
                continue;
            }
            // A local build of a marketplace model shares its service name on
            // purpose; `apply` keeps the two from being enabled together.
            let local = crate::compose::is_local_image(&entry.image);
            if let Some(existing) = self
                .models
                .iter()
                .find(|m| m.service_id == entry.service_id)
                .filter(|_| !local)
            {
                warnings.push(format!(
                    "{id} and {} both want the compose service `{}`; \
                     the marketplace entry keeps it",
                    existing.id, entry.service_id
                ));
                continue;
            }
            self.models.push(entry.to_model(id));
        }
        warnings
    }

    /// Look a model up by `id` or by `service_id`.
    pub fn get(&self, id: &str) -> Option<&Model> {
        self.models
            .iter()
            .find(|m| m.id == id)
            .or_else(|| self.models.iter().find(|m| m.service_id == id))
    }

    /// Entries that can actually be deployed, i.e. everything but starters.
    #[cfg(test)]
    pub fn deployable(&self) -> impl Iterator<Item = &Model> {
        self.models.iter().filter(|m| m.kind == Kind::Model)
    }

    /// Case-insensitive substring search over id, display name and summary.
    pub fn search(&self, q: &str) -> Vec<&Model> {
        let needle = q.to_lowercase();
        self.models
            .iter()
            .filter(|m| {
                m.id.to_lowercase().contains(&needle)
                    || m.service_id.to_lowercase().contains(&needle)
                    || m.display_name.to_lowercase().contains(&needle)
                    || m.summary.to_lowercase().contains(&needle)
            })
            .collect()
    }
}

/// Load the catalogue: fresh cache > network > stale cache > embedded.
///
/// A fresh cache short-circuits the network so the common case costs one
/// directory read. When the cache is cold or stale the network is tried,
/// unless `opts.offline`, and a successful fetch refreshes the cache. If the
/// fetch fails the ladder continues downwards — a stale cache, then the
/// snapshot compiled into the binary — so `chap-devops` still works on a plane.
///
/// Falling back after a failed fetch warns on stderr, because a silently
/// out-of-date catalogue is how a user ends up pinning a version the
/// marketplace has already yanked. `--offline` does not warn: the user asked.
pub fn load(opts: &RegistryOptions) -> Result<Registry> {
    let registry = load_chosen(opts)?;
    warn_skipped(&registry);
    crate::output::verbose(&format!(
        "registry: {} from {} ({} models)",
        registry.url,
        registry.provenance.describe(),
        registry.models.len()
    ));
    Ok(registry)
}

/// [`load`] itself: cache, then network, then whatever can still be parsed.
fn load_chosen(opts: &RegistryOptions) -> Result<Registry> {
    let cached = cache::read(opts)?;

    if let Some((index, files, age)) = &cached
        && age < &CACHE_TTL
        && let Some(registry) = parse_cached(opts, *age, index, files)
    {
        return Ok(registry);
    }

    if opts.offline {
        if let Some((index, files, age)) = &cached
            && let Some(registry) = parse_cached(opts, *age, index, files)
        {
            return Ok(registry);
        }
        return load_embedded();
    }

    let fetched = match fetch::fetch_registry(opts) {
        Ok(fetched) => fetched,
        Err(err) => return fall_back(opts, cached, &err),
    };

    let (index_yaml, model_files) = fetched;
    // A registry we cannot cache is still a registry: warn, do not fail.
    if let Err(err) = cache::write(opts, &index_yaml, &model_files) {
        crate::output::warn(&format!("could not write the registry cache: {err:#}"));
    }
    Registry::parse(&opts.url, Provenance::Network, &index_yaml, &model_files)
}

/// Force a network refresh and rewrite the cache.
///
/// Unlike [`load`] this has no fallback: the point of `chap-devops registry update`
/// is to know whether the refresh worked, so every failure surfaces as
/// [`ChapError::RegistryUnavailable`], and `--offline` as a usage error.
pub fn update(opts: &RegistryOptions) -> Result<Registry> {
    refresh(opts).map(|refreshed| refreshed.registry)
}

/// What [`refresh`] fetched, and what changed against the cache.
pub struct Refreshed {
    pub registry: Registry,
    /// `None` when there was no usable cache to compare with.
    pub changes: Option<changes::Changes>,
    /// The age of the cached copy it was compared with.
    pub compared_age: Option<Duration>,
}

/// [`update`], and what changed against the copy in the cache before it.
pub fn refresh(opts: &RegistryOptions) -> Result<Refreshed> {
    if opts.offline {
        return Err(ChapError::Usage(
            "`chap-devops registry update` refreshes the marketplace registry, which needs the \
             network; drop --offline"
                .to_string(),
        )
        .into());
    }

    let (index_yaml, model_files) = fetch::fetch_registry(opts)
        .map_err(|e| ChapError::RegistryUnavailable(format!("{e:#}")))?;
    // The copy the refresh replaces, read before the write; a corrupt one
    // warns and compares as none.
    let before = cache::read(opts)?.and_then(|(index, files, age)| {
        parse_cached(opts, age, &index, &files).map(|registry| (registry, age))
    });
    cache::write(opts, &index_yaml, &model_files)?;
    let registry = Registry::parse(&opts.url, Provenance::Network, &index_yaml, &model_files)?;
    warn_skipped(&registry);
    Ok(Refreshed {
        changes: before
            .as_ref()
            .map(|(old, _)| changes::between(old, &registry)),
        compared_age: before.map(|(_, age)| age),
        registry,
    })
}

/// Warn once for each model file the catalogue goes without.
fn warn_skipped(registry: &Registry) {
    for skipped in &registry.skipped {
        crate::output::warn(&skipped.line);
    }
}

/// Parse a cache entry, reporting a corrupt one as "no cache" so the caller
/// can continue down the ladder instead of failing outright.
fn parse_cached(
    opts: &RegistryOptions,
    age: Duration,
    index_yaml: &str,
    model_files: &[(String, String)],
) -> Option<Registry> {
    match Registry::parse(&opts.url, Provenance::for_age(age), index_yaml, model_files) {
        Ok(registry) => Some(registry),
        Err(err) => {
            crate::output::warn(&format!(
                "ignoring the registry cache: {err:#}; run `chap-devops registry update` to write it again"
            ));
            None
        }
    }
}

/// The network failed: use the stale cache if there is one, else the embedded
/// snapshot, and say which and why.
fn fall_back(
    opts: &RegistryOptions,
    cached: Option<cache::CachedSnapshot>,
    err: &anyhow::Error,
) -> Result<Registry> {
    if let Some((index, files, age)) = &cached
        && let Some(registry) = parse_cached(opts, *age, index, files)
    {
        crate::output::warn(&format!(
            "could not reach {} ({err}); using the cached snapshot from {} ago",
            opts.url,
            crate::output::human_age(*age)
        ));
        return Ok(registry);
    }
    crate::output::warn(&format!(
        "could not reach {} ({err}); using the snapshot built into this binary",
        opts.url
    ));
    load_embedded()
}

/// Parse the snapshot compiled into the binary.
pub fn load_embedded() -> Result<Registry> {
    Registry::parse(
        DEFAULT_REGISTRY_URL,
        Provenance::Embedded,
        embedded::index_yaml(),
        &embedded::model_files(),
    )
}

/// Resolve a model file's URL relative to the index URL.
///
/// Strips the filename from `index_url` and appends `rel`.
pub fn model_url(index_url: &str, rel: &str) -> String {
    let base = match index_url.rfind('/') {
        Some(i) => &index_url[..i + 1],
        None => "",
    };
    format!("{base}{rel}")
}

#[cfg(test)]
mod tests;
