use super::*;
use crate::components::Components;
use crate::compose::render::{
    render_chap_devops_overlay, render_dhis2, render_ocs, render_overlay, render_s3,
};
use crate::compose::spec::{
    ChapDevopsOverlaySpec, Dhis2Spec, OcsSpec, OverlaySpec, S3Spec, compose_services,
};
use crate::registry::{Channel, VersionSelector, load_embedded};
use serde_yaml_ng::Value;

/// Each service's labels, by service name.
fn labels_of(text: &str) -> Vec<(String, Value)> {
    let doc: Value = serde_yaml_ng::from_str(text).expect("rendered compose parses");
    doc["services"]
        .as_mapping()
        .unwrap()
        .iter()
        .map(|(name, svc)| (name.as_str().unwrap().to_string(), svc["labels"].clone()))
        .collect()
}

/// Assert every service carries exactly `expected`.
fn assert_labels(text: &str, expected: &[(&str, &str)]) {
    let services = labels_of(text);
    assert!(!services.is_empty(), "{text}");
    for (name, labels) in services {
        let map = labels
            .as_mapping()
            .unwrap_or_else(|| panic!("{name} has no labels:\n{text}"));
        assert_eq!(map.len(), expected.len(), "{name}: {labels:?}");
        for (key, value) in expected {
            assert_eq!(labels[*key].as_str(), Some(*value), "{name}: {key}");
        }
    }
}

fn overlay_spec() -> OverlaySpec {
    let registry = load_embedded().unwrap();
    let m = registry.get("chapkit_ewars_model").unwrap();
    let v = m
        .resolve(&VersionSelector::Channel(Channel::Stable))
        .unwrap();
    OverlaySpec::from_model(m, v, None, None, None)
}

/// A components file with everything on and DHIS2 seeded, so the dump
/// one-shot is rendered too.
fn components() -> Components {
    let mut c = Components::default();
    c.ocs.enabled = true;
    c.s3.enabled = true;
    c.dhis2.enabled = true;
    c
}

#[test]
fn the_block_names_the_kind_from_the_group() {
    assert_eq!(
        labels_block(ROLE_OCS, None, None),
        "    labels:\n      io.github.dhis2-chap.chap-devops.role: ocs\n      io.github.dhis2-chap.chap-devops.kind: init\n"
    );
    assert_eq!(
        labels_block(ROLE_MODEL, Some("m"), Some("123")),
        "    labels:\n      io.github.dhis2-chap.chap-devops.role: model\n      io.github.dhis2-chap.chap-devops.model: \"m\"\n      \
         io.github.dhis2-chap.chap-devops.kind: run\n      io.github.dhis2-chap.chap-devops.group: \"123\"\n"
    );
}

#[test]
fn a_model_and_its_init_container_carry_the_model_labels() {
    let text = render_overlay(&overlay_spec());
    assert_eq!(labels_of(&text).len(), 2, "the service and its init");
    assert_labels(
        &text,
        &[
            (LABEL_ROLE, ROLE_MODEL),
            (LABEL_MODEL, "chapkit_ewars_model"),
            (LABEL_KIND, KIND_INIT),
        ],
    );

    let text = render_overlay(&OverlaySpec {
        group: Some("dengue".into()),
        ..overlay_spec()
    });
    assert_labels(
        &text,
        &[
            (LABEL_ROLE, ROLE_MODEL),
            (LABEL_MODEL, "chapkit_ewars_model"),
            (LABEL_KIND, KIND_RUN),
            (LABEL_GROUP, "dengue"),
        ],
    );
}

#[test]
fn every_chap_core_service_is_labelled_through_the_chap_devops_overlay() {
    let text = render_chap_devops_overlay(&ChapDevopsOverlaySpec::new(8000));
    let names: Vec<String> = labels_of(&text).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["chap", "worker", "redis", "postgres"]);
    assert_labels(
        &text,
        &[(LABEL_ROLE, ROLE_CHAP_CORE), (LABEL_KIND, KIND_INIT)],
    );

    // A checkout builds chap and the worker, and they keep their labels.
    let text = render_chap_devops_overlay(&ChapDevopsOverlaySpec {
        checkout: Some("/src/chap-core".into()),
        group: Some("g".into()),
        ..ChapDevopsOverlaySpec::new(8000)
    });
    assert_labels(
        &text,
        &[
            (LABEL_ROLE, ROLE_CHAP_CORE),
            (LABEL_KIND, KIND_RUN),
            (LABEL_GROUP, "g"),
        ],
    );

    // Only the services the base file defines are named: one compose does
    // not know would be a service with no image.
    let text = render_chap_devops_overlay(&ChapDevopsOverlaySpec {
        services: vec!["chap".into(), "valkey".into()],
        ..ChapDevopsOverlaySpec::new(8000)
    });
    let names: Vec<String> = labels_of(&text).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["chap", "valkey"]);
}

#[test]
fn the_embedded_base_file_defines_upstreams_services() {
    let base =
        crate::compose::render::render_base(&crate::compose::spec::BaseSpec { upstream: None });
    assert_eq!(
        compose_services(&base).unwrap(),
        crate::compose::spec::UPSTREAM_SERVICES
    );
    assert_eq!(compose_services("not: [compose"), None);
}

#[test]
fn every_component_service_carries_its_role() {
    let c = components();
    let ocs = render_ocs(&OcsSpec::from_components(&c, false));
    assert_labels(&ocs, &[(LABEL_ROLE, ROLE_OCS), (LABEL_KIND, KIND_INIT)]);

    let s3 = render_s3(&S3Spec::from_components(&c));
    assert_eq!(labels_of(&s3).len(), 2, "s3 and its bucket one-shot");
    assert_labels(&s3, &[(LABEL_ROLE, ROLE_S3), (LABEL_KIND, KIND_INIT)]);

    let spec = Dhis2Spec::from_components(&c);
    assert!(
        spec.seed.is_some(),
        "the default seed renders the dump one-shot"
    );
    let dhis2 = render_dhis2(&spec);
    let names: Vec<String> = labels_of(&dhis2).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["dhis2", "dhis2-db", "dhis2-dump", "dhis2-prep"]);
    assert_labels(&dhis2, &[(LABEL_ROLE, ROLE_DHIS2), (LABEL_KIND, KIND_INIT)]);

    let run = render_dhis2(&Dhis2Spec {
        group: Some("g".into()),
        ..spec
    });
    assert_labels(
        &run,
        &[
            (LABEL_ROLE, ROLE_DHIS2),
            (LABEL_KIND, KIND_RUN),
            (LABEL_GROUP, "g"),
        ],
    );
}

#[test]
fn a_checkout_builds_the_api_for_the_platform_it_is_given_and_not_the_worker() {
    let text = render_chap_devops_overlay(&ChapDevopsOverlaySpec {
        checkout: Some("/src/chap-core".into()),
        checkout_platform: Some("linux/arm64".into()),
        ..ChapDevopsOverlaySpec::new(8000)
    });
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).expect("YAML");
    assert_eq!(
        doc["services"]["chap"]["platform"].as_str(),
        Some("linux/arm64")
    );
    assert_eq!(
        doc["services"]["chap"]["build"]["context"].as_str(),
        Some("/src/chap-core")
    );
    assert!(
        doc["services"]["worker"].get("platform").is_none(),
        "{text}"
    );

    // Without a checkout there is nothing to build, so no platform.
    let text = render_chap_devops_overlay(&ChapDevopsOverlaySpec::new(8000));
    assert!(!text.contains("platform:"), "{text}");
}
