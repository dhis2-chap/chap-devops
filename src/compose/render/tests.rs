use super::dhis2::DHIS2_TEMPLATE;
use super::ocs::{OCS_TEMPLATE, S3_TEMPLATE};
use super::overlay::OVERLAY_TEMPLATE;
use super::*;
use crate::compose::spec::{
    ChapDevopsOverlaySpec, Dhis2ConfigSpec, Dhis2SeedSource, Dhis2Spec, OcsConfigSpec, OcsSpec,
    OverlaySpec, S3Spec, UpstreamCompose,
};
use crate::compose::{tag_env_var, volume_name};
use crate::registry::{Channel, VersionSelector, load_embedded};
use serde_yaml_ng::Value;

mod components;

/// Nothing may leave an `@KEY@` behind; `@` on its own is legal YAML (the
/// base file has one in a postgres URL), so only the token shape counts.
fn assert_no_tokens(text: &str) {
    let re = regex::Regex::new(r"@[A-Z][A-Z0-9_]*@").unwrap();
    assert!(
        !re.is_match(text),
        "unfilled token: {:?}",
        re.find(text).map(|m| m.as_str())
    );
}

/// A spec for a model that publishes no host port: the default.
fn overlay_spec(id: &str) -> OverlaySpec {
    let registry = load_embedded().unwrap();
    let m = registry.get(id).unwrap();
    let v = m
        .resolve(&VersionSelector::Channel(Channel::Stable))
        .unwrap();
    OverlaySpec::from_model(m, v, None, None, None)
}

/// The same, published on `host_port`.
fn published_spec(id: &str, host_port: u16) -> OverlaySpec {
    OverlaySpec {
        host_port: Some(host_port),
        ..overlay_spec(id)
    }
}

/// `compose.chap-devops.yml` over upstream's services, outside a `chap-devops run` group.
fn chap_devops_overlay(
    api_port: u16,
    project_name: Option<&str>,
    checkout: Option<&str>,
) -> String {
    render_chap_devops_overlay(&ChapDevopsOverlaySpec {
        project_name: project_name.map(str::to_string),
        checkout: checkout.map(str::to_string),
        ..ChapDevopsOverlaySpec::new(api_port)
    })
}

fn parse(text: &str) -> Value {
    serde_yaml_ng::from_str(text).expect("generated compose parses as YAML")
}

fn service<'a>(doc: &'a Value, name: &str) -> &'a Value {
    &doc["services"][name]
}

#[test]
fn fill_replaces_every_token() {
    let out = fill("a=@A@ b=@B@ a=@A@", &[("A", "1"), ("B", "2")]);
    assert_eq!(out, "a=1 b=2 a=1");
}

#[test]
fn fill_leaves_compose_variables_alone() {
    let out = fill("${TAG:-sha-1234567} @TAG@", &[("TAG", "x")]);
    assert_eq!(out, "${TAG:-sha-1234567} x");
}

#[test]
fn overlay_matches_the_golden_fixture() {
    let text = render_overlay(&overlay_spec("chapkit_ewars_model"));
    let golden = normalize_newlines(include_str!(
        "../../../tests/fixtures/compose.chapkit-ewars-model.yml"
    ));
    assert_eq!(text, golden);
}

/// A digest goes in the default of the tag variable, so a tag set in `.env`
/// replaces the whole pin rather than being overruled by the digest.
#[test]
fn an_overlay_pins_the_digest_in_the_default_of_the_tag_variable() {
    let mut spec = overlay_spec("chapkit_ewars_model");
    let digest = format!("sha256:{}", "d".repeat(64));
    spec.image_tag = "sha-1111111".to_string();
    spec.image_digest = Some(digest.clone());
    let text = render_overlay(&spec);
    assert!(
        text.contains(&format!(
            "    image: {}:${{CHAPKIT_EWARS_MODEL_IMAGE_TAG:-sha-1111111@{digest}}}\n",
            spec.image
        )),
        "{text}"
    );
}

/// The same shape under the other WorkingDir: `/work/data`, which is
/// where most of the catalogue keeps its database, rather than the EWARS
/// image's `/app/data`.
#[test]
fn a_work_dir_overlay_matches_its_own_golden_fixture() {
    let text = render_overlay(&overlay_spec("chapkit_rwanda_malaria_bym_model"));
    let golden = normalize_newlines(include_str!(
        "../../../tests/fixtures/compose.chapkit-rwanda-malaria-bym-model.yml"
    ));
    assert_eq!(text, golden);
}

/// The shape a deployment without chap-core renders: no orchestrator URL
/// and no registration key, since the service registers nowhere, and no
/// `chap` to wait for. The rest is byte for byte the usual overlay.
#[test]
fn a_standalone_overlay_matches_its_own_golden_fixture() {
    let mut spec = overlay_spec("chapkit_ewars_model");
    spec.standalone = true;
    let text = render_overlay(&spec);
    assert_no_tokens(&text);
    let golden = normalize_newlines(include_str!(
        "../../../tests/fixtures/compose.chapkit-ewars-model.standalone.yml"
    ));
    assert_eq!(text, golden);
    let doc: Value = serde_yaml_ng::from_str(&text).unwrap();
    let svc = service(&doc, "chapkit-ewars-model");
    assert!(svc.get("environment").is_none());
    assert!(svc["depends_on"].get("chap").is_none());
}

/// The shape for a chap-core elsewhere: the service registers there over
/// the host gateway, under this machine's name and its published port,
/// and waits for no `chap`. The ewars image fixes `--port 8000`, so the host
/// port maps to 8000 and no `PORT` is set.
#[test]
fn an_external_chap_core_overlay_matches_its_own_golden_fixture() {
    let mut spec = overlay_spec("chapkit_ewars_model");
    spec.host_port = Some(5001);
    spec.standalone = true;
    spec.external_chap_core = Some(crate::compose::spec::ExternalRegistration {
        register_url: "http://host.docker.internal:8000".to_string(),
        models_host: "localhost".to_string(),
    });
    let text = render_overlay(&spec);
    assert_no_tokens(&text);
    let golden = normalize_newlines(include_str!(
        "../../../tests/fixtures/compose.chapkit-ewars-model.external.yml"
    ));
    assert_eq!(text, golden);
    let doc: Value = serde_yaml_ng::from_str(&text).unwrap();
    let svc = service(&doc, "chapkit-ewars-model");
    assert_eq!(
        svc["environment"]["SERVICEKIT_ORCHESTRATOR_URL"].as_str(),
        Some("http://host.docker.internal:8000/v2/services/$$register")
    );
    assert_eq!(svc["environment"]["SERVICEKIT_PORT"].as_str(), Some("5001"));
    assert!(svc["environment"].get("PORT").is_none());
    assert!(svc["depends_on"].get("chap").is_none());
}

/// An image that reads `PORT` (servicekit 2 checks only `SERVICEKIT_PORT`
/// before it registers) listens on its host port, so that port is mapped on
/// both sides and handed to the app.
#[test]
fn an_external_chap_core_overlay_moves_an_image_that_reads_port_to_its_host_port() {
    let mut spec = overlay_spec("chapkit_simple_multistep_model");
    spec.host_port = Some(5001);
    spec.standalone = true;
    spec.reads_port = true;
    spec.external_chap_core = Some(crate::compose::spec::ExternalRegistration {
        register_url: "http://host.docker.internal:8000".to_string(),
        models_host: "localhost".to_string(),
    });
    let text = render_overlay(&spec);
    let doc: Value = serde_yaml_ng::from_str(&text).unwrap();
    let svc = service(&doc, "chapkit-simple-multistep-model");
    assert_eq!(svc["ports"][0].as_str(), Some("5001:5001"), "{text}");
    assert_eq!(svc["expose"][0].as_str(), Some("5001"));
    assert_eq!(svc["environment"]["PORT"].as_str(), Some("5001"));
    assert_eq!(svc["environment"]["SERVICEKIT_PORT"].as_str(), Some("5001"));

    // Without a chap-core elsewhere, reading PORT changes nothing: the model
    // listens on 8000 on the compose network.
    spec.external_chap_core = None;
    let text = render_overlay(&spec);
    assert!(text.contains("\"5001:8000\""), "{text}");
    assert!(!text.contains("PORT: \"5001\""), "{text}");
}

/// A locally built image has no registry behind it, so compose must never
/// try to pull it; a registry image gets no such line.
#[test]
fn a_local_image_is_never_pulled() {
    let mut spec = overlay_spec("chapkit_ewars_model");
    assert!(!render_overlay(&spec).contains("pull_policy"));
    spec.image = "my-model".to_string();
    spec.image_tag = "dev".to_string();
    let text = render_overlay(&spec);
    let doc: Value = serde_yaml_ng::from_str(&text).unwrap();
    let svc = service(&doc, "chapkit-ewars-model");
    assert_eq!(svc["pull_policy"].as_str(), Some("never"));
    assert!(
        svc["image"].as_str().unwrap().starts_with("my-model:"),
        "{text}"
    );
}

/// A deployment built from a chap-core checkout builds chap and worker
/// from it on every `chap-devops up`, into images of their own.
#[test]
fn a_checkout_builds_chap_and_the_worker() {
    assert!(!chap_devops_overlay(8000, None, None).contains("build:"));
    let text = chap_devops_overlay(8000, None, Some("/src/chap-core"));
    let doc: Value = serde_yaml_ng::from_str(&text).unwrap();
    let chap = service(&doc, "chap");
    assert_eq!(chap["image"].as_str(), Some(CHECKOUT_CHAP_IMAGE));
    assert_eq!(chap["build"]["context"].as_str(), Some("/src/chap-core"));
    assert_eq!(chap["pull_policy"].as_str(), Some("build"));
    let worker = service(&doc, "worker");
    assert_eq!(worker["image"].as_str(), Some(CHECKOUT_WORKER_IMAGE));
    assert_eq!(
        worker["build"]["dockerfile"].as_str(),
        Some("Dockerfile.worker")
    );

    // With a project name the images are that deployment's own.
    let named = chap_devops_overlay(8000, Some("mychap-1ab2c3"), Some("/src/chap-core"));
    let doc: Value = serde_yaml_ng::from_str(&named).unwrap();
    assert_eq!(
        service(&doc, "chap")["image"].as_str(),
        Some("mychap-1ab2c3-chap:checkout")
    );
    assert_eq!(
        service(&doc, "worker")["image"].as_str(),
        Some("mychap-1ab2c3-worker:checkout")
    );
    assert_eq!(worker["pull_policy"].as_str(), Some("build"));
}

/// The third shape, said as assertions rather than as a golden file: an
/// image that runs as root, which gets no `user:` line and an init
/// container that chowns its volume to `0:0`. Auto-ARIMA is one of the
/// three marketplace images that still end on `USER root`.
#[test]
fn a_root_image_gets_no_user_line_and_an_init_container_that_chowns_to_zero() {
    let text = render_overlay(&overlay_spec("auto_arima_chapkit"));
    assert_no_tokens(&text);
    let doc = parse(&text);
    let svc = service(&doc, "auto-arima-chapkit");
    // The one line a root image does not get: the image's own user
    // applies, and a `user:` here could only take a permission away.
    assert!(svc.get("user").is_none(), "root needs no override");

    // The one it does. A volume docker has just created is root-owned and
    // needs no chown, but one carried over from a deployment that ran
    // this model as 1000:1000 is owned by 1000 - and the overlay drops
    // `CAP_DAC_OVERRIDE` with every other capability, so root cannot
    // write to it. This is what makes that upgrade heal itself.
    let init = service(&doc, "auto-arima-chapkit-init");
    assert_eq!(
        init["command"][2].as_str(),
        Some("chown -R 0:0 /work/data"),
        "{text}"
    );
    assert_eq!(init["user"].as_str(), Some("0:0"));
    assert_eq!(init["restart"].as_str(), Some("no"));
    assert_eq!(init["volumes"][0]["target"].as_str(), Some("/work/data"));

    // The model waits for it, and still waits for chap-core.
    assert_eq!(
        svc["depends_on"]["auto-arima-chapkit-init"]["condition"].as_str(),
        Some("service_completed_successfully")
    );
    assert_eq!(
        svc["depends_on"]["chap"]["condition"].as_str(),
        Some("service_healthy")
    );
    assert_eq!(svc["volumes"][1]["target"].as_str(), Some("/work/data"));
    assert!(doc["volumes"]["ck_auto_arima_chapkit_data"].is_mapping());
    // And the hardening it shares with every other overlay is untouched.
    assert_eq!(svc["read_only"].as_bool(), Some(true));
    assert_eq!(svc["init"].as_bool(), Some(true));

    // Every spelling of root renders the same file, chown included.
    let mut spec = overlay_spec("auto_arima_chapkit");
    for user in ["root", "0", "0:0", "root:root", ""] {
        spec.user = user.to_string();
        assert_eq!(render_overlay(&spec), text, "{user:?}");
    }
}

#[test]
fn an_overlay_publishes_a_port_only_when_one_was_asked_for() {
    // The default: exposed on the compose network, invisible to the host.
    let internal = render_overlay(&overlay_spec("chapkit_ewars_model"));
    assert_no_tokens(&internal);
    assert!(internal.contains("\n    expose:\n      - \"8000\"\n"));
    assert!(!internal.contains("    ports:"));
    // Dropping the ports block must not leave a blank line behind.
    assert!(internal.contains("\n    environment:\n"));
    assert!(!internal.contains("\n\n    environment:"));

    // With a port, both keys: `expose` still documents the container port,
    // and `ports` is the mapping Docker publishes.
    let published = render_overlay(&published_spec("chapkit_ewars_model", 5002));
    assert_no_tokens(&published);
    let svc = &parse(&published)["services"]["chapkit-ewars-model"];
    assert_eq!(
        svc["expose"],
        Value::Sequence(vec![Value::String("8000".into())])
    );
    assert_eq!(
        svc["ports"],
        Value::Sequence(vec![Value::String("5002:8000".into())])
    );
}

#[test]
fn a_bound_port_names_its_host_address() {
    let mut spec = published_spec("chapkit_ewars_model", 5002);
    spec.bind = Some("127.0.0.1".parse().unwrap());
    let svc = &parse(&render_overlay(&spec))["services"]["chapkit-ewars-model"];
    assert_eq!(
        svc["ports"],
        Value::Sequence(vec![Value::String("127.0.0.1:5002:8000".into())])
    );

    spec.bind = Some("::1".parse().unwrap());
    let svc = &parse(&render_overlay(&spec))["services"]["chapkit-ewars-model"];
    assert_eq!(
        svc["ports"],
        Value::Sequence(vec![Value::String("[::1]:5002:8000".into())])
    );
}

#[test]
fn the_chap_devops_overlay_replaces_the_api_port_rather_than_adding_to_it() {
    let text = chap_devops_overlay(8000, None, None);
    assert_no_tokens(&text);
    assert!(text.starts_with(&format!("{GENERATED_HEADER}\n")));
    // `!override` is what makes this a replacement: a plain `ports:` list
    // would merge with the one compose.yml already has.
    assert!(text.contains("    ports: !override\n      - \"${CHAP_API_PORT:-8000}:8000\"\n"));
    assert!(text.ends_with('\n'));

    // The recorded port is the variable's default, so the file works
    // without .env and moves when `.chap-devops/project.yaml` does.
    assert!(chap_devops_overlay(8123, None, None).contains("${CHAP_API_PORT:-8123}:8000"));

    // It parses, tag and all, and only chap has its ports replaced; the
    // other services are named for their labels alone.
    let doc = parse(&text);
    let services = doc["services"].as_mapping().unwrap();
    assert_eq!(services.len(), 4);
    for (name, svc) in services {
        assert_eq!(
            svc.get("ports").is_some(),
            name.as_str() == Some("chap"),
            "{name:?}"
        );
    }
}

#[test]
fn the_chap_devops_overlay_names_the_compose_project() {
    let text = chap_devops_overlay(8000, Some("mychap-1ab2c3"), None);
    assert_no_tokens(&text);
    assert!(text.contains("\nname: mychap-1ab2c3\n"), "{text}");
    assert_eq!(
        parse(&text)["name"].as_str(),
        Some("mychap-1ab2c3"),
        "compose reads it as the project name"
    );
    // Without a name the key is absent rather than empty: compose would
    // reject `name:` with nothing after it.
    assert!(!chap_devops_overlay(8000, None, None).contains("name:"));
}

#[test]
fn the_chap_devops_overlay_hands_chap_core_the_registration_key() {
    // Upstream's compose.ghcr.yml passes only CHAP_API_TOKEN into the
    // container, so without this line a protected chap-core has no key to
    // check a model's X-Service-Key against and answers 401.
    let text = chap_devops_overlay(8000, Some("mychap-1ab2c3"), None);
    let env = &parse(&text)["services"]["chap"]["environment"];
    assert_eq!(
        env["SERVICEKIT_REGISTRATION_KEY"].as_str(),
        Some("${SERVICEKIT_REGISTRATION_KEY:-}")
    );
    // Rendered whether or not the deployment has a key: compose
    // substitutes an empty value, which chap-core reads as no key.
    assert!(chap_devops_overlay(8000, None, None).contains("SERVICEKIT_REGISTRATION_KEY:"));
}

#[test]
fn the_chap_devops_overlay_points_gunicorns_control_socket_at_the_tmpfs() {
    // gunicorn 26 falls back to $HOME/.gunicorn/, which the service's
    // read-only root refuses, and logs an error on every start; /tmp is
    // the tmpfs upstream already mounts.
    let text = chap_devops_overlay(8000, None, None);
    assert_eq!(
        parse(&text)["services"]["chap"]["environment"]["XDG_RUNTIME_DIR"].as_str(),
        Some("/tmp")
    );
    // As a key and nothing else: why it is there is in the docs.
    assert!(!text.contains("# Keeps gunicorn"), "{text}");
    // The worker runs celery, not gunicorn, so it is left alone.
    assert!(
        parse(&text)["services"]["worker"]
            .get("environment")
            .is_none()
    );
}

#[test]
fn overlay_keeps_the_chap_core_deployment_invariants() {
    let text = render_overlay(&overlay_spec("chapkit_ewars_model"));
    assert_no_tokens(&text);
    // `$$register` must survive verbatim: `$register` would expand away.
    assert!(text.contains("http://chap:8000/v2/services/$$register"));

    let doc = parse(&text);
    let svc = service(&doc, "chapkit-ewars-model");
    assert_eq!(svc["restart"].as_str(), Some("unless-stopped"));
    assert_eq!(svc["init"].as_bool(), Some(true));
    assert_eq!(svc["read_only"].as_bool(), Some(true));
    // The numeric form, so this line and the init container's chown are
    // the same two numbers.
    assert_eq!(svc["user"].as_str(), Some("1000:1000"));

    // chap-core/tests/test_compose_deployment.py: a pinned sha build behind
    // an overridable ${<ID>_IMAGE_TAG:-sha-xxxxxxx}, and no pull_policy.
    let image = svc["image"].as_str().unwrap();
    let re =
        regex::Regex::new(r"^ghcr\.io/[a-z0-9._/-]+:\$\{[A-Z0-9_]+_IMAGE_TAG:-sha-[0-9a-f]{7}\}$")
            .unwrap();
    assert!(re.is_match(image), "unexpected image {image:?}");
    assert!(
        svc.get("pull_policy").is_none(),
        "pull_policy defeats the pin"
    );

    // No networks: the default network reaches chap but not redis/postgres.
    assert!(svc.get("networks").is_none());
    // chapkit images ship their own HEALTHCHECK.
    assert!(svc.get("healthcheck").is_none());
    // Both facts live in docs/models.md, not in a comment in every file.
    assert!(!text.contains("# No `networks:`"));
    assert!(!text.contains("# No healthcheck"));

    // No host port by default: only the container port is declared.
    assert_eq!(
        svc["expose"],
        Value::Sequence(vec![Value::String("8000".into())])
    );
    assert!(svc.get("ports").is_none(), "a model publishes nothing");
    assert_eq!(
        svc["depends_on"]["chap"]["condition"].as_str(),
        Some("service_healthy")
    );

    // The named volume is declared as well as mounted.
    let volume = volume_name("chapkit_ewars_model");
    assert_eq!(svc["volumes"][1]["source"].as_str(), Some(volume.as_str()));
    assert_eq!(svc["volumes"][1]["target"].as_str(), Some("/app/data"));
    assert!(doc["volumes"].get(volume.as_str()).is_some());

    // Two services, and only the model carries the restart policy: the
    // init container is a one-shot by design.
    let services = doc["services"].as_mapping().unwrap();
    assert_eq!(services.len(), 2);
}

#[test]
fn overlay_hands_the_data_volume_to_the_model_user_before_it_starts() {
    let text = render_overlay(&overlay_spec("chapkit_ewars_model"));
    let doc = parse(&text);
    let init = service(&doc, "chapkit-ewars-model-init");

    // Docker seeds a fresh named volume from the image, ownership
    // included, and root-owns it when the image has no such directory;
    // the model would then fail to open its SQLite file.
    assert_eq!(init["image"].as_str(), Some("busybox:1.38"));
    assert_eq!(init["user"].as_str(), Some("0:0"));
    // Numeric: busybox knows no `chapkit` account.
    assert_eq!(
        init["command"],
        Value::Sequence(vec![
            "sh".into(),
            "-c".into(),
            "chown -R 1000:1000 /app/data".into()
        ])
    );
    assert!(!text.contains("chown chapkit"));
    // Quoted, or YAML would read it as the boolean false.
    assert!(text.contains("    restart: \"no\"\n"));
    assert_eq!(init["restart"].as_str(), Some("no"));
    // Multi-arch, so no platform pin is needed.
    assert!(init.get("platform").is_none());
    assert!(init.get("ports").is_none());

    // Same volume at the same path as the model service.
    let volume = volume_name("chapkit_ewars_model");
    assert_eq!(init["volumes"][0]["source"].as_str(), Some(volume.as_str()));
    assert_eq!(init["volumes"][0]["target"].as_str(), Some("/app/data"));

    // And the model waits for it to finish.
    let svc = service(&doc, "chapkit-ewars-model");
    assert_eq!(
        svc["depends_on"]["chapkit-ewars-model-init"]["condition"].as_str(),
        Some("service_completed_successfully")
    );
}

#[test]
fn overlay_chowns_to_the_ids_of_the_user_the_model_runs_as() {
    // The simple multistep image runs as `chap`, which is uid/gid 1001.
    let text = render_overlay(&overlay_spec("chapkit_simple_multistep_model"));
    assert!(text.contains("chown -R 1001:1001 /app/data"), "{text}");

    // A numeric --user is passed through, and the data dir follows it.
    let mut spec = overlay_spec("auto_arima_chapkit");
    spec.user = "1500:1600".into();
    spec.data_dir = "/srv/data".into();
    assert!(render_overlay(&spec).contains("chown -R 1500:1600 /srv/data"));

    // An account no image of ours creates falls back to the chapkit ids
    // rather than rendering a chown busybox would reject.
    spec.user = "nobody".into();
    let text = render_overlay(&spec);
    assert!(text.contains("chown -R 1000:1000 /srv/data"), "{text}");
    assert_eq!(
        service(&parse(&text), "auto-arima-chapkit")["user"].as_str(),
        Some("nobody")
    );
}

#[test]
fn an_overlay_pinned_to_a_digest_renders_a_digest_reference() {
    let mut spec = overlay_spec("auto_arima_chapkit");
    let digest = format!("@sha256:{}", "b".repeat(64));
    spec.image_tag = digest.clone();
    spec.version = digest.clone();
    let text = render_overlay(&spec);
    assert_no_tokens(&text);
    assert!(
            text.contains(&format!(
                "    image: ghcr.io/chap-models/auto_arima_chapkit${{AUTO_ARIMA_CHAPKIT_IMAGE_TAG:-{digest}}}\n"
            )),
            "{text}"
        );
    // Compose reads it as one reference, digest and all.
    let image = parse(&text)["services"]["auto-arima-chapkit"]["image"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(image.contains(":-@sha256:"), "{image}");
}

#[test]
fn overlay_pins_the_platform_and_drops_the_line_cleanly_without_one() {
    let inla = render_overlay(&overlay_spec("chapkit_ewars_model"));
    let doc = parse(&inla);
    assert_eq!(
        service(&doc, "chapkit-ewars-model")["platform"].as_str(),
        Some("linux/amd64")
    );

    // The pin is a key and nothing else: why it is there is in the docs.
    assert!(!inla.contains("amd64-only"), "{inla}");

    let mut spec = overlay_spec("chapkit_simple_multistep_model");
    spec.platform = None;
    let portable = render_overlay(&spec);
    let doc = parse(&portable);
    let svc = service(&doc, "chapkit-simple-multistep-model");
    assert!(svc.get("platform").is_none(), "no platform when unset");
    // Dropping the platform line leaves no blank line behind.
    assert!(portable.contains("    image: ghcr.io/chap-models/chapkit_simple_multistep_model:"));
    assert!(!portable.contains("amd64-only"));
    assert!(!portable.contains("\n\n    init: true"));
    assert!(portable.contains("\n    init: true\n"));
    assert_no_tokens(&portable);
}

#[test]
fn overlay_registration_key_is_commented_until_asked_for() {
    let mut spec = overlay_spec("auto_arima_chapkit");
    let commented = render_overlay(&spec);
    assert!(commented.contains("      # Uncomment if chap has SERVICEKIT_REGISTRATION_KEY set:"));
    let doc = parse(&commented);
    assert!(
        service(&doc, "auto-arima-chapkit")["environment"]
            .get("SERVICEKIT_REGISTRATION_KEY")
            .is_none()
    );

    spec.registration_key = true;
    let active = render_overlay(&spec);
    assert_no_tokens(&active);
    assert!(!active.contains("# Uncomment"));
    let doc = parse(&active);
    assert_eq!(
        service(&doc, "auto-arima-chapkit")["environment"]["SERVICEKIT_REGISTRATION_KEY"].as_str(),
        // Left for compose to expand, not resolved at generation time.
        Some("${SERVICEKIT_REGISTRATION_KEY:-}")
    );
}

/// Two lines and no more: the shared one, then the model this file is.
#[test]
fn overlay_header_is_the_shared_line_plus_the_model() {
    let spec = overlay_spec("chapkit_ewars_model");
    let text = render_overlay(&spec);
    let header: Vec<&str> = text.lines().take(3).collect();
    assert_eq!(header[0], GENERATED_HEADER);
    assert_eq!(
        header[1],
        format!(
            "# chapkit_ewars_model {} (https://github.com/chap-models/chapkit_ewars_model)",
            spec.version
        )
    );
    assert_eq!(header[2], "services:", "no third comment line");
    // The tag variable is in the image line, which is the only place it
    // does anything.
    assert!(text.contains(&format!(
        "${{{}:-{}}}",
        tag_env_var("chapkit_ewars_model"),
        spec.image_tag
    )));
}

#[test]
fn base_needs_no_env_file_and_keeps_the_networks_split() {
    let text = render_base(&BaseSpec { upstream: None });
    assert_no_tokens(&text);
    assert_eq!(text.lines().next(), Some(GENERATED_HEADER));
    assert_eq!(text.lines().nth(1), Some("services:"), "one header line");

    // chap-core's test_standalone_compose_needs_no_env_file.
    let bare = regex::Regex::new(r"\$\{[A-Z_]+\}").unwrap();
    assert!(
        !bare.is_match(&text),
        "variable without default: {:?}",
        bare.find(&text).map(|m| m.as_str())
    );

    let doc = parse(&text);
    for name in ["chap", "worker"] {
        let svc = service(&doc, name);
        assert_eq!(
            svc["networks"],
            Value::Sequence(vec!["default".into(), "backend".into()]),
            "{name} must join both networks"
        );
        let image = svc["image"].as_str().unwrap();
        assert!(
            image.ends_with(":${CHAP_IMAGE_TAG:-latest}"),
            "{name} pins {image:?}"
        );
    }
    for name in ["redis", "postgres"] {
        assert_eq!(
            service(&doc, name)["networks"],
            Value::Sequence(vec!["backend".into()]),
            "{name} must stay off the default network"
        );
    }
    let services = doc["services"].as_mapping().unwrap();
    assert_eq!(services.len(), 4);
    for (name, svc) in services {
        assert_eq!(
            svc["restart"].as_str(),
            Some("unless-stopped"),
            "{name:?} has no restart policy"
        );
    }
    assert!(doc["networks"].get("backend").is_some());
}

#[test]
fn base_from_upstream_keeps_the_fetched_body_verbatim() {
    // Only the first line differs between the two renderings; the second
    // is the same note, and everything after it is upstream's.
    let body = "services:\n  chap:\n    image: ghcr.io/x:${CHAP_IMAGE_TAG:-latest}\n";
    let text = render_base(&BaseSpec {
        upstream: Some(UpstreamCompose {
            tag: "v2.3.1".into(),
            body: body.to_string(),
        }),
    });
    assert_no_tokens(&text);
    assert_eq!(
        text,
        format!("{GENERATED_HEADER}\n# chap-core compose.ghcr.yml at v2.3.1\n{body}")
    );
    assert!(text.ends_with('\n'));
    assert_eq!(
        service(&parse(&text), "chap")["image"].as_str(),
        Some("ghcr.io/x:${CHAP_IMAGE_TAG:-latest}")
    );

    // A body without a trailing newline still ends the file cleanly.
    let text = render_base(&BaseSpec {
        upstream: Some(UpstreamCompose {
            tag: "master".into(),
            body: "services: {}".into(),
        }),
    });
    assert!(text.ends_with("services: {}\n"));
    assert!(text.contains("# chap-core compose.ghcr.yml at master\n"));
}

/// Every template carries the same first line as the renderers that write
/// one themselves; if it moves, they all have to move with it.
#[test]
fn every_generated_compose_file_opens_with_the_same_line() {
    for template in [
        &*BASE_TEMPLATE,
        &*OVERLAY_TEMPLATE,
        &*OCS_TEMPLATE,
        &*S3_TEMPLATE,
        &*DHIS2_TEMPLATE,
    ] {
        assert_eq!(template.lines().next(), Some(GENERATED_HEADER));
    }
    for rendered in [
        chap_devops_overlay(8000, None, None),
        render_umbrella(&[], None),
        render_umbrella(&["compose.a.yml".to_string()], Some("demo-1ab2c3")),
    ] {
        assert_eq!(rendered.lines().next(), Some(GENERATED_HEADER));
    }
}

#[test]
fn umbrella_with_no_models_is_an_empty_service_map() {
    let text = render_umbrella(&[], None);
    let doc = parse(&text);
    assert!(doc.get("include").is_none(), "an empty include is rejected");
    assert_eq!(
        doc["services"],
        Value::Mapping(serde_yaml_ng::Mapping::new())
    );
}

#[test]
fn umbrella_includes_one_line_per_overlay() {
    let one = render_umbrella(&["compose.chapkit-ewars-model.yml".to_string()], None);
    let doc = parse(&one);
    assert_eq!(
        doc["include"],
        Value::Sequence(vec!["compose.chapkit-ewars-model.yml".into()])
    );
    assert!(doc.get("services").is_none());

    let files: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|n| format!("compose.{n}.yml"))
        .collect();
    let three = render_umbrella(&files, None);
    let doc = parse(&three);
    assert_eq!(
        doc["include"],
        Value::Sequence(files.iter().map(|f| f.as_str().into()).collect())
    );
}

#[test]
fn the_umbrella_names_the_compose_project_too() {
    // It is the one file always in the `-f` list: a deployment with
    // chap-core turned off has no compose.chap-devops.yml to carry the name.
    for files in [vec![], vec!["compose.chapkit-ewars-model.yml".to_string()]] {
        let text = render_umbrella(&files, Some("mychap-1ab2c3"));
        assert_eq!(
            parse(&text)["name"].as_str(),
            Some("mychap-1ab2c3"),
            "{text}"
        );
    }
    assert!(!render_umbrella(&[], None).contains("name:"));
}

fn env_spec() -> EnvSpec {
    EnvSpec {
        postgres_user: "chap".into(),
        postgres_password: "0123456789abcdef0123456789abcdef".into(),
        postgres_db: "chap_core".into(),
        chap_image_tag: None,
        api_port: crate::project::DEFAULT_API_PORT,
        api_token: None,
        registration_key: None,
        model_tag_pins: Vec::new(),
    }
}

#[test]
fn env_writes_the_postgres_credentials() {
    let text = render_env(&env_spec());
    assert_no_tokens(&text);
    // Its own first line: `.env` is written once, not rendered from
    // `.chap-devops/` like the compose files.
    assert_eq!(
        text.lines().next(),
        Some(
            "# Written once by chap-devops init; chap-devops never rewrites it. \
                 See the docs for each setting."
        )
    );
    assert!(text.contains("\nPOSTGRES_USER=chap\n"));
    assert!(text.contains("\nPOSTGRES_PASSWORD=0123456789abcdef0123456789abcdef\n"));
    assert!(text.contains("\nPOSTGRES_DB=chap_core\n"));
    assert!(text.contains("\n# CHAP_API_TOKEN=\n"));
    assert!(text.contains("\n# SERVICEKIT_REGISTRATION_KEY=\n"));
    assert!(text.contains("\n# CHAP_DATABASE_URL=\n"));
    assert!(text.ends_with('\n'));
}

#[test]
fn env_carries_an_active_api_port_line_at_every_port() {
    // Active even at the default: the one port the stack publishes has to
    // be discoverable where an operator would change it.
    let text = render_env(&env_spec());
    assert!(text.contains("\nCHAP_API_PORT=8700\n"), "{text}");
    assert!(!text.contains("# CHAP_API_PORT"));

    let text = render_env(&EnvSpec {
        api_port: 8123,
        ..env_spec()
    });
    assert!(text.contains("\nCHAP_API_PORT=8123\n"));
}

#[test]
fn env_activates_the_chap_tag_only_when_it_is_pinned() {
    for tag in [None, Some("latest".to_string())] {
        let text = render_env(&EnvSpec {
            chap_image_tag: tag.clone(),
            ..env_spec()
        });
        assert!(text.contains("\n# CHAP_IMAGE_TAG=latest\n"), "{tag:?}");
    }
    let text = render_env(&EnvSpec {
        chap_image_tag: Some("v1.2.3".into()),
        ..env_spec()
    });
    assert!(text.contains("\nCHAP_IMAGE_TAG=v1.2.3\n"));
    assert!(!text.contains("# CHAP_IMAGE_TAG"));
}

#[test]
fn env_lists_one_commented_pin_per_model() {
    let text = render_env(&EnvSpec {
        model_tag_pins: vec![
            (tag_env_var("chapkit_ewars_model"), "sha-1111111".into()),
            (tag_env_var("auto_arima_chapkit"), "sha-2222222".into()),
        ],
        ..env_spec()
    });
    assert!(text.contains("\n# CHAPKIT_EWARS_MODEL_IMAGE_TAG=sha-1111111\n"));
    assert!(text.contains("\n# AUTO_ARIMA_CHAPKIT_IMAGE_TAG=sha-2222222\n"));
    assert!(!text.contains("none yet"));

    // Without models the section keeps a body rather than a blank line.
    let empty = render_env(&env_spec());
    assert!(empty.contains(NO_TAG_PINS));
    assert!(!empty.ends_with("\n\n"));
}

#[test]
fn env_leaves_both_secrets_commented_without_authentication() {
    let text = render_env(&env_spec());
    assert!(text.contains("\n# CHAP_API_TOKEN=\n"), "{text}");
    assert!(
        text.contains("\n# SERVICEKIT_REGISTRATION_KEY=\n"),
        "{text}"
    );
    assert_eq!(
        crate::auth::state_of(&text),
        crate::project::AuthState::default()
    );
}

#[test]
fn env_writes_both_secrets_as_active_lines_when_authentication_is_on() {
    let token = "a".repeat(64);
    let key = "b".repeat(64);
    let text = render_env(&EnvSpec {
        api_token: Some(token.clone()),
        registration_key: Some(key.clone()),
        ..env_spec()
    });
    assert_no_tokens(&text);
    assert!(
        text.contains(&format!("\nCHAP_API_TOKEN={token}\n")),
        "{text}"
    );
    assert!(
        text.contains(&format!("\nSERVICEKIT_REGISTRATION_KEY={key}\n")),
        "{text}"
    );
    // No placeholder left behind for either one.
    assert!(!text.contains("# CHAP_API_TOKEN="));
    assert!(!text.contains("# SERVICEKIT_REGISTRATION_KEY="));
    // And the rendered file reads back as "both on".
    assert_eq!(
        crate::auth::state_of(&text),
        crate::project::AuthState {
            api_token: true,
            registration_key: true
        }
    );
}

#[test]
fn a_secret_line_is_an_assignment_or_a_placeholder() {
    assert_eq!(
        secret_line("CHAP_API_TOKEN", Some("abc")),
        "CHAP_API_TOKEN=abc"
    );
    assert_eq!(secret_line("CHAP_API_TOKEN", None), "# CHAP_API_TOKEN=");
}
