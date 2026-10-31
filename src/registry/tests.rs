use super::*;

fn registry() -> Registry {
    load_embedded().expect("embedded snapshot parses")
}

#[test]
fn embedded_snapshot_parses_into_a_registry() {
    let r = registry();
    assert_eq!(r.models.len(), 7);
    assert_eq!(r.url, DEFAULT_REGISTRY_URL);
    assert!(matches!(r.provenance, Provenance::Embedded));
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    // Model order follows the index order.
    assert_eq!(r.models[0].id, "chapkit_ewars_model");
}

/// The unit tests embed the frozen copy in `tests/fixtures/marketplace/`, so
/// this reads the snapshot the binary ships, `vendor/marketplace/`, from disk:
/// every file the index lists is there, and the whole set parses.
#[test]
fn the_vendored_snapshot_parses_into_a_registry() {
    let vendor = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/marketplace");
    let read = |rel: &str| {
        std::fs::read_to_string(vendor.join(rel))
            .unwrap_or_else(|e| panic!("reading vendor/marketplace/{rel}: {e}"))
    };
    let index_yaml = read("registry.yaml");
    let index: model::RegistryIndex =
        serde_yaml_ng::from_str(&index_yaml).expect("the vendored registry.yaml parses");
    let files: Vec<(String, String)> = index
        .models
        .iter()
        .map(|rel| (rel.clone(), read(rel)))
        .collect();
    let r = Registry::parse(
        DEFAULT_REGISTRY_URL,
        Provenance::Embedded,
        &index_yaml,
        &files,
    )
    .expect("the vendored snapshot parses");
    assert_eq!(r.models.len(), index.models.len());
    assert!(r.models.iter().any(|m| !m.is_starter()));
}

/// The fields the configured models of a model are made from: the
/// configurations and the default covariates of the entry.
#[test]
fn the_snapshot_carries_configurations_and_default_covariates() {
    let r = registry();
    let ewars = r.get("chapkit_ewars_model").unwrap();
    let names: Vec<&str> = ewars.configurations.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "monthly_climate",
            "monthly_population_only",
            "monthly_region_seasonal"
        ]
    );
    let climate = &ewars.configurations["monthly_climate"].config;
    assert_eq!(climate["prediction_periods"], 3);
    assert_eq!(
        climate["additional_continuous_covariates"],
        serde_json::json!(["rainfall", "mean_temperature"])
    );
    assert_eq!(ewars.covariates.defaults, ["rainfall", "mean_temperature"]);
    // Every model in the snapshot has at least one configuration.
    for model in r.deployable() {
        assert!(!model.configurations.is_empty(), "{}", model.id);
    }
}

#[test]
fn get_matches_id_and_service_id() {
    let r = registry();
    assert_eq!(
        r.get("chapkit_ewars_model").unwrap().id,
        "chapkit_ewars_model"
    );
    assert_eq!(
        r.get("chapkit-ewars-model").unwrap().id,
        "chapkit_ewars_model"
    );
    assert!(r.get("nope").is_none());
}

/// A manual definition as `.chap-devops/models-manual.yaml` holds one.
fn manual(id: &str, service_id: &str) -> crate::project::ManualModel {
    crate::project::ManualModel {
        reads_port: false,
        service_id: service_id.to_string(),
        display_name: id.to_string(),
        repository: Some(format!("https://github.com/chap-models/{id}")),
        image: format!("ghcr.io/chap-models/{id}"),
        tag: "sha-b1d6c31".into(),
        commit: None,
        follow: Some("main".into()),
        data_dir: Some("/work/data".into()),
        user: Some("10001:10001".into()),
        runtime_amd64: true,
        added: "2026-09-24".into(),
    }
}

/// A local build may share a marketplace model's service name, since it
/// is that model built from a checkout; a registry image may not.
#[test]
fn a_local_build_may_stand_in_for_a_marketplace_model() {
    let mut r = registry();
    let mut local = manual("ewars_dev", "chapkit-ewars-model");
    local.image = "ewars".to_string();
    let mut remote = manual("ewars_fork", "chapkit-ewars-model");
    remote.image = "ghcr.io/me/ewars".to_string();
    let entries: crate::project::ManualModels = [
        ("ewars_dev".to_string(), local),
        ("ewars_fork".to_string(), remote),
    ]
    .into_iter()
    .collect();
    let warnings = r.with_manual(&entries);
    assert!(r.models.iter().any(|m| m.id == "ewars_dev"));
    assert!(!r.models.iter().any(|m| m.id == "ewars_fork"));
    assert_eq!(warnings.len(), 1, "{warnings:?}");
}

#[test]
fn with_manual_appends_the_deployments_own_models() {
    let mut r = registry();
    let before = r.models.len();
    // An id the marketplace does not list, which is the only kind a
    // deployment can add for itself.
    assert!(r.get("chapkit_example_manual_model").is_none());
    let manual = crate::project::ManualModels::from([(
        "chapkit_example_manual_model".to_string(),
        manual(
            "chapkit_example_manual_model",
            "chapkit-example-manual-model",
        ),
    )]);
    assert!(r.with_manual(&manual).is_empty(), "nothing collides");
    assert_eq!(r.models.len(), before + 1);

    // Found by id and by service id, like any other entry, and marked as
    // this deployment's own.
    for wanted in [
        "chapkit_example_manual_model",
        "chapkit-example-manual-model",
    ] {
        let found = r.get(wanted).unwrap_or_else(|| panic!("{wanted}"));
        assert_eq!(found.id, "chapkit_example_manual_model");
        assert!(found.manual, "{wanted}");
    }
    assert!(r.search("example_manual").iter().any(|m| m.manual));
    // And the marketplace entries are untouched.
    assert!(!r.get("chapkit_ewars_model").unwrap().manual);
}

#[test]
fn the_marketplace_wins_a_collision_and_says_so() {
    let mut r = registry();
    let before = r.models.len();
    let manual = crate::project::ManualModels::from([
        // An id the marketplace has since published.
        (
            "chapkit_ewars_model".to_string(),
            manual("chapkit_ewars_model", "my-ewars"),
        ),
        // And a service name it already uses.
        (
            "my_arima".to_string(),
            manual("my_arima", "auto-arima-chapkit"),
        ),
    ]);
    let warnings = r.with_manual(&manual);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("chapkit_ewars_model") && w.contains("models remove")),
        "{warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("auto-arima-chapkit") && w.contains("keeps it")),
        "{warnings:?}"
    );
    assert_eq!(r.models.len(), before, "neither was appended");
    assert!(!r.get("chapkit_ewars_model").unwrap().manual);
    assert!(r.get("my_arima").is_none());
}

#[test]
fn deployable_excludes_templates() {
    let r = registry();
    let ids: Vec<&str> = r.deployable().map(|m| m.id.as_str()).collect();
    assert_eq!(ids.len(), 5);
    assert!(!ids.iter().any(|id| id.contains("minimalist_example")));
}

#[test]
fn search_is_case_insensitive_across_fields() {
    let r = registry();
    assert_eq!(r.search("EWARS").len(), 1);
    assert_eq!(
        r.search("chapkit_ewars_model")[0].display_name,
        "CHAP-EWARS"
    );
    assert!(
        r.search("arima")
            .iter()
            .any(|m| m.id == "auto_arima_chapkit")
    );
    assert!(r.search("zzzz").is_empty());
}

#[test]
fn parse_rejects_a_missing_model_file() {
    let err = Registry::parse(
        DEFAULT_REGISTRY_URL,
        Provenance::Embedded,
        embedded::index_yaml(),
        &[],
    )
    .expect_err("missing model files are an error");
    assert!(err.to_string().contains("missing"));
}

/// A port nothing listens on, so every test in this module exercises the
/// "network failed" branch without a network. One URL for the whole run: the
/// cache is keyed by it.
fn unreachable() -> String {
    format!(
        "{}/registry.yaml",
        crate::test_support::shared_unreachable_base()
    )
}

fn opts(cache_dir: &std::path::Path, offline: bool) -> RegistryOptions {
    RegistryOptions {
        url: unreachable(),
        offline,
        cache_dir: cache_dir.to_path_buf(),
        timeout: Duration::from_secs(2),
    }
}

/// Seed the cache with a one-model catalogue, backdated by `age`.
///
/// One model is what makes a cache hit unmistakable: the embedded
/// snapshot has several, so a count of one cannot have come from the
/// fallback.
fn seed_cache(opts: &RegistryOptions, age: Duration) {
    const INDEX: &str = "\
schema_version: 2
marketplace:
  name: test marketplace
  description: seeded by the registry tests
  repository: https://example.test/marketplace
review_policy:
  required_approvals: 3
models:
  - models/chapkit_ewars_model.yaml
";
    let ewars = embedded::model_files()
        .into_iter()
        .find(|(name, _)| name == "models/chapkit_ewars_model.yaml")
        .expect("the snapshot ships ewars");
    cache::write(opts, INDEX, &[ewars]).unwrap();

    // meta.json is the only record of when the entry was written, so
    // rewriting it is how a test makes an entry old.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let meta = serde_json::json!({
        "url": opts.url,
        "fetched_at_unix": now - age.as_secs(),
    });
    std::fs::write(cache::dir_for(opts).join("meta.json"), meta.to_string()).unwrap();
}

#[test]
fn offline_without_a_cache_uses_the_embedded_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let r = load(&opts(tmp.path(), true)).unwrap();
    assert!(
        matches!(r.provenance, Provenance::Embedded),
        "{:?}",
        r.provenance
    );
    assert_eq!(r.models.len(), load_embedded().unwrap().models.len());
}

#[test]
fn an_unreachable_registry_without_a_cache_uses_the_embedded_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let r = load(&opts(tmp.path(), false)).unwrap();
    assert!(
        matches!(r.provenance, Provenance::Embedded),
        "{:?}",
        r.provenance
    );
    assert_eq!(r.models.len(), load_embedded().unwrap().models.len());
}

#[test]
fn an_unreachable_registry_falls_back_to_a_stale_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let opts = opts(tmp.path(), false);
    seed_cache(&opts, CACHE_TTL + Duration::from_secs(3600));

    let r = load(&opts).unwrap();
    match r.provenance {
        Provenance::StaleCache { age_secs } => {
            assert!(age_secs >= CACHE_TTL.as_secs(), "{age_secs}")
        }
        other => panic!("expected a stale cache, got {other:?}"),
    }
    assert_eq!(r.models.len(), 1, "the seeded cache, not the snapshot");
    assert_eq!(r.url, unreachable());
}

#[test]
fn a_fresh_cache_is_used_without_touching_the_network() {
    let tmp = tempfile::tempdir().unwrap();
    let opts = opts(tmp.path(), false);
    seed_cache(&opts, Duration::from_secs(60));

    // The URL is unreachable, so a cache miss would yield the whole
    // embedded snapshot; one model proves the cache answered first.
    let r = load(&opts).unwrap();
    match r.provenance {
        Provenance::Cache { age_secs } => assert!((60..600).contains(&age_secs), "{age_secs}"),
        other => panic!("expected a fresh cache, got {other:?}"),
    }
    assert_eq!(r.models.len(), 1);
    assert_eq!(r.models[0].id, "chapkit_ewars_model");
}

#[test]
fn offline_accepts_a_stale_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let opts = opts(tmp.path(), true);
    seed_cache(&opts, CACHE_TTL + Duration::from_secs(60));

    let r = load(&opts).unwrap();
    assert!(
        matches!(r.provenance, Provenance::StaleCache { .. }),
        "{:?}",
        r.provenance
    );
    assert_eq!(r.models.len(), 1);
}

/// A model file in the cache that does not parse looks the same as one the
/// marketplace wrote with a newer value, so it is left out like one: the
/// cache stays in use, and a fallback would only hide the other models.
#[test]
fn a_model_file_in_the_cache_that_does_not_parse_is_left_out() {
    let tmp = tempfile::tempdir().unwrap();
    let opts = opts(tmp.path(), true);
    seed_cache(&opts, Duration::from_secs(60));
    std::fs::write(
        cache::dir_for(&opts).join("models/chapkit_ewars_model.yaml"),
        "schema_version: 2\nnot: a model\n",
    )
    .unwrap();

    let r = load(&opts).unwrap();
    assert!(
        matches!(r.provenance, Provenance::Cache { .. }),
        "{:?}",
        r.provenance
    );
    assert!(r.models.is_empty());
    assert_eq!(r.skipped.len(), 1);
}

#[test]
fn a_corrupt_cache_index_does_not_stop_the_ladder() {
    let tmp = tempfile::tempdir().unwrap();
    let opts = opts(tmp.path(), true);
    seed_cache(&opts, Duration::from_secs(60));
    std::fs::write(
        cache::dir_for(&opts).join("registry.yaml"),
        "schema_version: [not, an, index\n",
    )
    .unwrap();

    let r = load(&opts).unwrap();
    assert!(
        matches!(r.provenance, Provenance::Embedded),
        "{:?}",
        r.provenance
    );
}

#[test]
fn update_fails_loudly_when_the_network_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let err = update(&opts(tmp.path(), false)).expect_err("nothing is listening");
    assert!(
        matches!(
            err.downcast_ref::<ChapError>(),
            Some(ChapError::RegistryUnavailable(_))
        ),
        "{err:#}"
    );
}

#[test]
fn update_refuses_to_run_offline() {
    let tmp = tempfile::tempdir().unwrap();
    let err = update(&opts(tmp.path(), true)).expect_err("--offline cannot refresh");
    match err.downcast_ref::<ChapError>() {
        Some(ChapError::Usage(why)) => assert!(why.contains("drop --offline"), "{why}"),
        other => panic!("wrong error: {other:?}"),
    }
}

#[test]
fn provenance_describes_its_age() {
    assert_eq!(Provenance::Network.describe(), "network");
    assert_eq!(Provenance::Embedded.describe(), "embedded");
    assert_eq!(
        Provenance::Cache { age_secs: 7200 }.describe(),
        "cache (2 hours old)"
    );
    assert_eq!(
        Provenance::StaleCache { age_secs: 172_800 }.describe(),
        "stale cache (2 days old)"
    );
    assert_eq!(
        Provenance::for_age(CACHE_TTL - Duration::from_secs(1)).label(),
        "cache"
    );
    assert_eq!(Provenance::for_age(CACHE_TTL).label(), "stale cache");
    assert_eq!(Provenance::Network.age(), None);
}

#[test]
fn model_url_replaces_the_index_filename() {
    assert_eq!(
        model_url(DEFAULT_REGISTRY_URL, "models/chapkit_ewars_model.yaml"),
        "https://raw.githubusercontent.com/dhis2-chap/model-marketplace/main/models/chapkit_ewars_model.yaml"
    );
    assert_eq!(model_url("registry.yaml", "models/x.yaml"), "models/x.yaml");
}

/// A model file with a value this chap-devops does not know, such as a new
/// `kind`, is left out with a sentence that says why; the other models stay.
#[test]
fn a_model_file_that_does_not_parse_is_left_out_and_named() {
    let embedded = registry();
    let index_yaml = serde_yaml_ng::to_string(&embedded.index).expect("the index serializes");
    let mut files: Vec<(String, String)> = embedded
        .index
        .models
        .iter()
        .zip(&embedded.models)
        .map(|(rel, model)| {
            (
                rel.clone(),
                serde_yaml_ng::to_string(model).expect("a model serializes"),
            )
        })
        .collect();
    let (broken, body) = &mut files[0];
    *body = body.replace("kind: model", "kind: something_new");
    let broken = broken.clone();

    let r = Registry::parse(
        DEFAULT_REGISTRY_URL,
        Provenance::Network,
        &index_yaml,
        &files,
    )
    .expect("one bad file does not fail the registry");
    assert_eq!(r.models.len(), embedded.models.len() - 1);
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].path, broken);
    let line = &r.skipped[0].line;
    assert!(
        line.starts_with(&format!(
            "left out {broken} of the marketplace registry: kind"
        )),
        "{line}"
    );
    assert!(line.ends_with("run `chap-devops self update`"), "{line}");
}

/// An entry whose id or service_id does not match its file is left out, as
/// `chap-admin` refuses it; the other models stay.
#[test]
fn a_model_file_with_another_identity_is_left_out_and_named() {
    let embedded = registry();
    let index_yaml = serde_yaml_ng::to_string(&embedded.index).expect("the index serializes");
    let mut files: Vec<(String, String)> = embedded
        .index
        .models
        .iter()
        .zip(&embedded.models)
        .map(|(rel, model)| {
            (
                rel.clone(),
                serde_yaml_ng::to_string(model).expect("a model serializes"),
            )
        })
        .collect();
    let (renamed, _) = files[0].clone();
    files[0].0 = "models/not_its_name.yaml".to_string();
    let mut index: crate::registry::RegistryIndex =
        serde_yaml_ng::from_str(&index_yaml).expect("the index parses");
    index.models[0] = "models/not_its_name.yaml".to_string();
    let index_yaml = serde_yaml_ng::to_string(&index).expect("the index serializes");

    let r = Registry::parse(
        DEFAULT_REGISTRY_URL,
        Provenance::Network,
        &index_yaml,
        &files,
    )
    .expect("one bad file does not fail the registry");
    assert_eq!(r.models.len(), embedded.models.len() - 1);
    assert_eq!(r.skipped.len(), 1, "{renamed}");
    let line = &r.skipped[0].line;
    assert!(
        line.starts_with("left out models/not_its_name.yaml of the marketplace registry: its id"),
        "{line}"
    );
    assert!(line.ends_with("report it to the marketplace"), "{line}");
}
