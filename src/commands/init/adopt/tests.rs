use super::*;
use crate::registry::{DEFAULT_REGISTRY_URL, load_embedded};

/// The base URL `chap-admin` records in `x-chap-registry` for the default
/// marketplace.
const ADMIN_REGISTRY: &str = "https://raw.githubusercontent.com/dhis2-chap/model-marketplace/main";

/// A `compose.marketplace.yml` as `chap-admin` writes it: a marketplace
/// model at the stable version, one at an older version, one the
/// marketplace no longer lists, and a custom image.
const OVERLAY: &str = r#"
services:
  marketplace-chapkit-simple-multistep-model:
    image: ghcr.io/chap-models/chapkit_simple_multistep_model:sha-1111111
    x-chap-custom: false
    x-chap-version: 0.1.2
    x-chap-registry: https://raw.githubusercontent.com/dhis2-chap/model-marketplace/main
    x-chap-template: chapkit-simple-multistep-model
  marketplace-chapkit-ewars-model:
    image: ghcr.io/chap-models/chapkit_ewars_model:sha-2222222
    x-chap-custom: false
    x-chap-version: 1.0.3
    x-chap-registry: https://raw.githubusercontent.com/dhis2-chap/model-marketplace/main
  marketplace-retired-model:
    image: ghcr.io/chap-models/retired_model:sha-964eea8
    x-chap-custom: false
    x-chap-version: 1.0.4
    x-chap-registry: https://raw.githubusercontent.com/dhis2-chap/model-marketplace/main
  marketplace-my-model:
    image: quay.io/someone/my_model:v1
    x-chap-custom: true
    x-chap-version: custom
    x-chap-template: dengue-model
volumes:
  marketplace-chapkit-simple-multistep-model-data: {}
"#;

const ENV: &str = "POSTGRES_USER=chap\nPOSTGRES_PASSWORD=s3cret\nPOSTGRES_DB=chap_core\n\
                   CHAP_IMAGE_TAG=v2.4.0\nCHAP_API_TOKEN=tok\nREDIS_PASSWORD=x\n# CHAP_DATABASE_URL=\n";

fn adoption(env: &str, overlay: Option<&str>) -> Adoption {
    plan(
        Path::new("/srv/chap"),
        "Chap.Core",
        env,
        overlay,
        &load_embedded().unwrap(),
        DEFAULT_REGISTRY_URL,
    )
    .unwrap()
}

fn how(adoption: &Adoption, id: &str) -> How {
    adoption
        .models
        .iter()
        .find(|m| m.id == id)
        .unwrap_or_else(|| panic!("{id} in {:?}", adoption.models))
        .how
        .clone()
}

#[test]
fn the_project_name_is_what_compose_makes_of_the_directory_name() {
    assert_eq!(compose_default_name("Chap.Core"), "chapcore");
    assert_eq!(compose_default_name("_my chap-2"), "mychap-2");
    assert_eq!(adoption(ENV, None).project, "chapcore");
    let named = adoption("COMPOSE_PROJECT_NAME=prod\n", None);
    assert_eq!(named.project, "prod");
}

#[test]
fn the_secrets_and_the_tag_come_over_and_the_rest_is_named() {
    let a = adoption(ENV, None);
    assert_eq!(a.env.postgres_password, "s3cret");
    assert_eq!(a.env.api_token.as_deref(), Some("tok"));
    assert_eq!(a.chap_tag.as_deref(), Some("v2.4.0"));
    // The commented line sets nothing, so only REDIS_PASSWORD is named.
    assert_eq!(a.warnings.len(), 1);
    assert!(
        a.warnings[0].contains("sets REDIS_PASSWORD, which chap-devops does not carry over"),
        "{:?}",
        a.warnings
    );
    // compose.ghcr.yml's own fallbacks, for a .env without POSTGRES_*.
    let bare = adoption("", None);
    assert_eq!(bare.env.postgres_user, "chap");
    assert_eq!(bare.env.postgres_password, "chap");
    assert_eq!(bare.env.postgres_db, "chap_core");
    assert!(bare.models.is_empty());
}

#[test]
fn each_model_of_the_overlay_comes_over_the_way_it_ran() {
    let a = adoption(ENV, Some(OVERLAY));
    assert_eq!(a.models.len(), 4);
    // At the stable version: follows stable, as `update-all` would move it.
    assert_eq!(
        how(&a, "chapkit_simple_multistep_model"),
        How::Marketplace(VersionSelector::Channel(Channel::Stable))
    );
    // At an older version: stays there, so the takeover changes nothing.
    assert_eq!(
        how(&a, "chapkit_ewars_model"),
        How::Marketplace(VersionSelector::Exact("1.0.3".to_string()))
    );
    // No longer listed: added at the image it ran, under its service id.
    assert_eq!(
        how(&a, "retired_model"),
        How::Manual {
            image: "ghcr.io/chap-models/retired_model:sha-964eea8".to_string(),
            service_id: "retired-model".to_string(),
        }
    );
    // A custom image registers under the template name chap-admin recorded.
    assert_eq!(
        how(&a, "my_model"),
        How::Manual {
            image: "quay.io/someone/my_model:v1".to_string(),
            service_id: "dengue-model".to_string(),
        }
    );
    let multistep = a
        .models
        .iter()
        .find(|m| m.id == "chapkit_simple_multistep_model")
        .unwrap();
    assert_eq!(
        multistep.old_service,
        "marketplace-chapkit-simple-multistep-model"
    );

    // Only the marketplace models are init's selection; the others go
    // through `models add`.
    let ids: Vec<String> = a.selection().enable.into_iter().map(|r| r.id).collect();
    assert_eq!(
        ids,
        ["chapkit_simple_multistep_model", "chapkit_ewars_model"]
    );
    let added: Vec<String> = manual_args(&a).into_iter().map(|r| r.source).collect();
    assert_eq!(
        added,
        [
            "ghcr.io/chap-models/retired_model:sha-964eea8",
            "quay.io/someone/my_model:v1"
        ]
    );
    assert!(
        a.notes
            .iter()
            .any(|n| n.contains("chapkit_ewars_model stays at 1.0.3")),
        "{:?}",
        a.notes
    );
}

#[test]
fn a_model_from_another_registry_comes_over_as_an_image() {
    let overlay = OVERLAY.replace(ADMIN_REGISTRY, "https://example.org/marketplace");
    let a = adoption(ENV, Some(&overlay));
    assert!(matches!(
        how(&a, "chapkit_simple_multistep_model"),
        How::Manual { .. }
    ));
}

fn docker<'a>(
    has_containers: &'a dyn Fn(&str) -> Option<bool>,
    volume_exists: &'a dyn Fn(&str) -> bool,
) -> Docker<'a> {
    Docker {
        has_containers,
        volume_exists,
    }
}

#[test]
fn a_running_deployment_is_refused_with_the_command_that_stops_it() {
    let mut a = adoption(ENV, None);
    let err = check(&mut a, &docker(&|_| Some(true), &|_| true)).unwrap_err();
    assert!(
        err.to_string()
            .contains("stop it with `docker compose -p chapcore down`, which keeps its volumes"),
        "{err}"
    );
}

#[test]
fn a_missing_database_volume_is_refused() {
    let mut a = adoption(ENV, None);
    let err = check(&mut a, &docker(&|_| Some(false), &|_| false)).unwrap_err();
    assert!(
        err.to_string().contains(
            "found no volume chapcore_chap-db, so this would start chap-core with an empty database"
        ),
        "{err}"
    );
    let mut a = adoption(ENV, None);
    check(
        &mut a,
        &docker(&|_| Some(false), &|name| name == "chapcore_chap-db"),
    )
    .unwrap();
    assert!(
        a.notes
            .iter()
            .any(|n| n == "chap-core keeps its database in chapcore_chap-db")
    );
}

#[test]
fn a_development_overlay_that_ran_is_named() {
    let ewars = DevelopmentOverlay {
        file: "compose.ewars.yml".to_string(),
        services: vec!["ewars".to_string()],
        volumes: vec!["ewars_data".to_string()],
    };
    let mut a = Adoption {
        development: vec![ewars.clone()],
        ..adoption(ENV, None)
    };
    check(
        &mut a,
        &docker(&|_| Some(false), &|name| name == "chapcore_chap-db"),
    )
    .unwrap();
    assert!(
        !a.warnings.iter().any(|w| w.contains("also ran")),
        "{:?}",
        a.warnings
    );

    let mut a = Adoption {
        development: vec![ewars],
        ..adoption(ENV, None)
    };
    check(
        &mut a,
        &docker(&|_| Some(false), &|name| {
            name == "chapcore_chap-db" || name == "chapcore_ewars_data"
        }),
    )
    .unwrap();
    assert_eq!(
        a.warnings[1..],
        [
            "the deployment in /srv/chap also ran compose.ewars.yml (chapcore_ewars_data is \
             there), and chap-devops does not take over its services (ewars); if you need the \
             model, add it with `chap-devops models enable`"
        ]
    );
}

#[test]
fn the_development_overlays_and_their_includes_are_read() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("compose.chapkit.yml"),
        "include:\n  - compose.ewars.yml\n  - compose.multistep.yml\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("compose.ewars.yml"),
        "services:\n  ewars:\n    image: x\nvolumes:\n  ewars_data: {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("compose.multistep.yml"),
        "services:\n  multistep:\n    image: y\n",
    )
    .unwrap();
    assert_eq!(
        development_overlays(dir.path()),
        [
            DevelopmentOverlay {
                file: "compose.ewars.yml".to_string(),
                services: vec!["ewars".to_string()],
                volumes: vec!["ewars_data".to_string()],
            },
            DevelopmentOverlay {
                file: "compose.multistep.yml".to_string(),
                services: vec!["multistep".to_string()],
                volumes: vec![],
            },
        ]
    );
}

#[test]
fn without_docker_the_takeover_goes_on_and_says_what_was_not_checked() {
    let mut a = adoption(ENV, None);
    check(&mut a, &docker(&|_| None, &|_| false)).unwrap();
    assert!(
        a.warnings
            .iter()
            .any(|w| w.contains("docker could not be asked")),
        "{:?}",
        a.warnings
    );
}

/// chap-core writes the old name of the marketplace repository in
/// `x-chap-registry`; it is the same registry as the new name.
#[test]
fn the_old_and_the_new_marketplace_repository_name_are_one_registry() {
    assert_eq!(
        registry_base(ADMIN_REGISTRY),
        registry_base(DEFAULT_REGISTRY_URL)
    );
    assert_eq!(
        registry_base("https://raw.githubusercontent.com/dhis2-chap/chap-marketplace/main/"),
        registry_base(DEFAULT_REGISTRY_URL)
    );
    assert_ne!(
        registry_base("https://example.org/fork/main"),
        registry_base(DEFAULT_REGISTRY_URL)
    );
    assert!(matches!(
        how(
            &adoption(ENV, Some(OVERLAY)),
            "chapkit_simple_multistep_model"
        ),
        How::Marketplace(_)
    ));
}
