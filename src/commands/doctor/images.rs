//! Checks of the images the deployment pulls, the users they run as, and whether it is up.

use super::*;

/// One enabled model's image, from what `docker manifest inspect` said.
pub fn image_verdict(
    marketplace_id: &str,
    service_id: &str,
    reference: &str,
    outcome: &Outcome,
) -> Check {
    let id = format!("image-{service_id}");
    let name = format!("image {service_id}");
    match outcome {
        Outcome::Done {
            ok: true, stdout, ..
        } => match manifest_has_amd64(stdout) {
            Some(false) => Check::warn(
                id,
                name,
                format!("{reference} has no linux/amd64 image"),
                format!(
                    "the overlay pins platform: linux/amd64, so this tag cannot run; \
                     re-resolve it with `chap-devops models enable {marketplace_id}`"
                ),
            ),
            Some(true) => Check::ok(id, name, format!("{reference} (linux/amd64)")),
            None => Check::ok(id, name, format!("{reference} exists")),
        },
        Outcome::TimedOut => Check::skip(
            id,
            name,
            format!(
                "ghcr.io did not answer for {reference} within {}s",
                MANIFEST_TIMEOUT.as_secs()
            ),
        ),
        Outcome::Missing => Check::skip(id, name, "no docker CLI to ask"),
        Outcome::Failed(why) => Check::skip(id, name, why.clone()),
        Outcome::Done { stderr, .. } if stderr.to_ascii_lowercase().contains("experimental") => {
            Check::skip(
                id,
                name,
                "this docker CLI has `docker manifest` behind its experimental flag",
            )
        }
        Outcome::Done { stderr, .. } if !manifest_missing(stderr) => {
            unanswered(id, name, reference, stderr)
        }
        Outcome::Done { stderr, .. } => Check::fail(
            id,
            name,
            format!("{reference} was not found: {}", first_line(stderr)),
            format!("run `chap-devops models enable {marketplace_id}` to resolve the pin again"),
        ),
    }
}

/// Whether `docker manifest inspect` said the tag does not exist, as opposed
/// to the registry refusing or failing to answer - a rate limit, a 5xx, an
/// `unauthorized` - which says nothing about the tag.
pub fn manifest_missing(stderr: &str) -> bool {
    let text = stderr.to_ascii_lowercase();
    text.contains("no such manifest")
        || text.contains("manifest unknown")
        || text.contains("not found")
}

/// A registry that answered without saying whether the tag exists: a warning
/// that quotes it, never a "not found" that sends the operator to re-pin.
fn unanswered(id: String, name: String, reference: &str, stderr: &str) -> Check {
    Check::warn(
        id,
        name,
        format!(
            "the registry did not say whether {reference} exists: {}",
            first_line(stderr)
        ),
        "a rate limit or a registry outage reads like this; run `chap-devops doctor` again later",
    )
}

/// Why the image manifests were not looked up, or `None` when they were.
///
/// `--offline` is answered first and on its own: it is the reason the user
/// gave, it holds whether or not this machine has a docker CLI, and a
/// checklist that said "no docker CLI to ask" on a runner without Docker but
/// "--offline" on a laptop with it would report the same run two ways. A
/// missing CLI is the reason only for a run that did go looking, where it is
/// what stopped the lookup; `docker-cli` has failed on its own line by then.
pub fn image_skip_reason(probed: Option<&Probed>, have_cli: bool) -> Option<String> {
    let Some(probed) = probed else {
        return Some("--offline: the registry was not asked".to_string());
    };
    if !have_cli {
        return Some("no docker CLI to ask".to_string());
    }
    probed
        .ghcr
        .as_ref()
        .err()
        .map(|why| format!("ghcr.io is unreachable: {why}"))
}

/// One enabled component's image, from what `docker manifest inspect` said.
///
/// Unlike a model overlay, a component pins no platform: both images are
/// multi-arch, so "the tag exists" is the whole question here.
pub fn component_image_verdict(name: &str, reference: &str, outcome: &Outcome) -> Check {
    let id = format!("image-{name}");
    let check_name = format!("image {name}");
    match outcome {
        Outcome::Done { ok: true, .. } => Check::ok(id, check_name, format!("{reference} exists")),
        Outcome::TimedOut => Check::skip(
            id,
            check_name,
            format!(
                "the registry did not answer for {reference} within {}s",
                MANIFEST_TIMEOUT.as_secs()
            ),
        ),
        Outcome::Missing => Check::skip(id, check_name, "no docker CLI to ask"),
        Outcome::Failed(why) => Check::skip(id, check_name, why.clone()),
        Outcome::Done { stderr, .. } if stderr.to_ascii_lowercase().contains("experimental") => {
            Check::skip(
                id,
                check_name,
                "this docker CLI has `docker manifest` behind its experimental flag",
            )
        }
        Outcome::Done { stderr, .. } if !manifest_missing(stderr) => {
            unanswered(id, check_name, reference, stderr)
        }
        Outcome::Done { stderr, .. } => Check::fail(
            id,
            check_name,
            format!("{reference} was not found: {}", first_line(stderr)),
            format!(
                "check the tag: `{OCS_TAG_ENV_VAR}`, `{S3_TAG_ENV_VAR}` and \
                 `{DHIS2_TAG_ENV_VAR}` in .env pin the component images"
            ),
        ),
    }
}

/// The images the enabled components pull, as `(component, reference)`.
///
/// One image per component: the one whose tag a deployment can move. A
/// component that is several services pulls more than that - `dhis2` brings a
/// PostGIS database and an alpine one-shot beside its web image - but those two
/// references are exact and compiled into this binary, the same on every
/// deployment, so a check on them would spend a registry round trip per
/// `chap-devops doctor` to report a chap-devops bug as the operator's problem, and the
/// `fix` line would name a variable that does not exist. chap-core's own
/// postgres and valkey are left out on the same rule.
///
/// Each tag as compose will read it: the variable in `.env` (`env`) when it
/// sets one, the recorded tag otherwise - the compose files render
/// `${VAR:-recorded}`, so an override there is what actually gets pulled.
pub fn component_images(components: &Components, env: &str) -> Vec<(String, String)> {
    let tag = |var: &str, recorded: &str| {
        crate::dotenv::non_empty(env, var).unwrap_or_else(|| recorded.to_string())
    };
    let mut images = Vec::new();
    if components.ocs.enabled {
        images.push((
            crate::compose::OCS_SERVICE.to_string(),
            format!(
                "{OCS_IMAGE}:{}",
                tag(OCS_TAG_ENV_VAR, &components.ocs.image_tag)
            ),
        ));
    }
    if components.s3.enabled {
        images.push((
            crate::compose::S3_SERVICE.to_string(),
            format!("{S3_IMAGE}:{}", tag(S3_TAG_ENV_VAR, S3_DEFAULT_TAG)),
        ));
    }
    if components.dhis2.enabled {
        images.push((
            crate::compose::DHIS2_SERVICE.to_string(),
            format!(
                "{}:{}",
                components.dhis2.image,
                tag(DHIS2_TAG_ENV_VAR, &components.dhis2.image_tag)
            ),
        ));
    }
    images
}

/// The image reference compose pulls for one model: its tag variable in
/// `.env` when that sets one, the pinned tag and digest otherwise.
pub fn model_reference(id: &str, model: &crate::project::EnabledModel, env: &str) -> String {
    let tag = crate::dotenv::non_empty(env, &crate::compose::tag_env_var(id))
        .unwrap_or_else(|| model.pinned_tag());
    crate::compose::image_ref(&model.image, &tag)
}

/// This deployment's `.env`, or nothing.
fn env_body(project: &Project) -> String {
    std::fs::read_to_string(project.dir.join(ENV_FILE)).unwrap_or_default()
}

/// One line per enabled model: does the user its overlay runs it as still
/// match what the image declares.
///
/// The one class of breakage `chap-devops sync` cannot see: the recorded user is
/// what the overlay renders from, and an image that runs as root under a
/// `user: 1000:1000` starts fine and then fails on its own binaries - the
/// Rwanda BYM model's `inla.run: Permission denied`, up to its 0.1.1 pin. It
/// is also what a model that has moved the other way looks like, which is why
/// the check compares rather than looks for root. Only a locally pulled
/// image can be asked without a network round trip per model, so a model whose
/// amd64 image is not in this machine's image store is skipped rather than
/// guessed at.
pub(super) fn user_checks(project: &Project, daemon: bool) -> Vec<Check> {
    user_checks_with(project, daemon, &crate::docker::image_config)
}

/// [`user_checks`] with the daemon injected, so the verdicts can be tested.
fn user_checks_with(
    project: &Project,
    have_cli: bool,
    declared: &dyn Fn(&str) -> Option<(String, String)>,
) -> Vec<Check> {
    let env = env_body(project);
    project
        .state
        .models
        .iter()
        .map(|(id, model)| {
            let reference = model_reference(id, model, &env);
            let found = have_cli
                .then(|| declared(&reference))
                .flatten()
                .map(|(user, _)| user);
            let asked = have_cli.then_some(reference.as_str());
            user_check(id, &model.service_id, asked, &model.user, found.as_deref())
        })
        .collect()
}

/// The verdict for one model, given what its image declares.
///
/// `declared` is `None` when the image the overlay pins is not in this
/// machine's image store as the `linux/amd64` variant every overlay runs -
/// the usual state before the first `chap-devops up`, and no reason to say anything
/// is wrong. `reference` is the tag that was asked about, or `None` when
/// there was no docker CLI to ask through.
pub fn user_check(
    id: &str,
    service_id: &str,
    reference: Option<&str>,
    recorded: &str,
    declared: Option<&str>,
) -> Check {
    let check_id = format!("user-{service_id}");
    let name = format!("user {service_id}");
    let Some(declared) = declared else {
        let Some(reference) = reference else {
            return Check::skip(
                check_id,
                name,
                format!("{recorded}; no docker daemon to ask"),
            );
        };
        return Check::skip_with(
            check_id,
            name,
            format!("{recorded}; the amd64 variant of {reference} is not in the local image store"),
            format!(
                "run `chap-devops docker pull`, or `docker pull --platform linux/amd64 {reference}`"
            ),
        );
    };
    let wanted = crate::compose::resolve::normalize(declared);
    // An account name only the image itself can resolve says nothing about
    // whether the numbers beside it are right.
    if crate::compose::overrides::numeric_pair(&wanted).is_none() {
        return Check::skip(
            check_id,
            name,
            format!("the image runs as `{wanted}`, which only the image can turn into numbers"),
        );
    }
    if crate::compose::resolve::normalize(recorded) == wanted {
        return Check::ok(check_id, name, format!("{recorded}, as the image declares"));
    }
    Check::warn(
        check_id,
        name,
        format!("{recorded}, but the image runs as {wanted}"),
        format!(
            "run `chap-devops models enable {id}` to read the user off the image again, or `chap-devops update`"
        ),
    )
}

/// One line per enabled model, asked in parallel.
pub(super) fn image_checks(
    project: &Project,
    probed: Option<&Probed>,
    have_cli: bool,
) -> Vec<Check> {
    // Marketplace id, service id and the exact reference compose pulls.
    let env = env_body(project);
    let models: Vec<(String, String, String)> = project
        .state
        .models
        .iter()
        .map(|(id, model)| {
            (
                id.clone(),
                model.service_id.clone(),
                model_reference(id, model, &env),
            )
        })
        .collect();
    let components = component_images(&project.state.components, &env);

    if let Some(reason) = image_skip_reason(probed, have_cli) {
        let model_skips = models.iter().map(|(_, service_id, _)| service_id);
        let component_skips = components.iter().map(|(name, _)| name);
        return model_skips
            .chain(component_skips)
            .map(|name| {
                Check::skip(
                    format!("image-{name}"),
                    format!("image {name}"),
                    reason.clone(),
                )
            })
            .collect();
    }

    // One thread per image: each is a registry round trip, and a deployment
    // with four models would otherwise wait out four of them in a row.
    std::thread::scope(|scope| {
        let references: Vec<String> = models
            .iter()
            .map(|(_, _, reference)| reference.clone())
            .chain(components.iter().map(|(_, reference)| reference.clone()))
            .collect();
        let handles: Vec<_> = references
            .iter()
            .map(|reference| {
                let reference = reference.clone();
                scope.spawn(move || {
                    run_bounded(
                        "docker",
                        &["manifest", "inspect", &reference],
                        MANIFEST_TIMEOUT,
                    )
                })
            })
            .collect();
        let mut outcomes = handles.into_iter().map(|handle| {
            handle
                .join()
                .unwrap_or_else(|_| Outcome::Failed("the probe did not finish".to_string()))
        });
        let mut checks: Vec<Check> = models
            .iter()
            .zip(outcomes.by_ref())
            .map(|((marketplace_id, service_id, reference), outcome)| {
                image_verdict(marketplace_id, service_id, reference, &outcome)
            })
            .collect();
        checks.extend(
            components
                .iter()
                .zip(outcomes)
                .map(|((name, reference), outcome)| {
                    component_image_verdict(name, reference, &outcome)
                }),
        );
        checks
    })
}

/// What a deployment without chap-core adds up to: its components alone.
fn components_only_verdict(report: &StatusReport) -> (Status, String, Option<String>) {
    // Models without chap-core are judged by their own /health, the same rows
    // `chap-devops status` prints, and count beside the components.
    let models = report
        .models
        .iter()
        .map(|m| (m.id.as_str(), m.state == crate::status::ModelState::Up));
    let (up, down): (Vec<&str>, Vec<&str>) = report
        .components
        .iter()
        .map(|c| (c.name.as_str(), c.state == ComponentState::Up))
        .chain(models)
        .fold(
            (Vec::new(), Vec::new()),
            |(mut up, mut down), (name, is_up)| {
                if is_up {
                    up.push(name);
                } else {
                    down.push(name);
                }
                (up, down)
            },
        );
    if down.is_empty() {
        return (
            Status::Ok,
            format!("no chap-core here; up: {}", up.join(", ")),
            None,
        );
    }
    (
        Status::Warn,
        format!("no chap-core here; not up: {}", down.join(", ")),
        Some("run `chap-devops status` for what each one is doing".to_string()),
    )
}

/// What the running deployment adds up to.
pub fn stack_verdict(report: &StatusReport) -> (Status, String, Option<String>) {
    match &report.api {
        ApiHealth::Off => components_only_verdict(report),
        // A container that is up and unhealthy is a sharper answer than "the
        // port does not answer", and its own last error line is sharper still.
        ApiHealth::Down { .. } if report.api_container_unhealthy() => {
            let cause = crate::diagnose::headline(&report.unhealthy)
                .map(|line| format!(": {line}"))
                .unwrap_or_default();
            (
                Status::Fail,
                format!(
                    "the chap container is up and unhealthy{}",
                    first_line(&cause)
                ),
                Some(
                    report
                        .unhealthy
                        .iter()
                        .find_map(|entry| entry.hint.clone())
                        .unwrap_or_else(|| "run `chap-devops logs chap` to see why".to_string()),
                ),
            )
        }
        ApiHealth::Down { error } => (
            Status::Fail,
            // Whatever its container last said about itself beats the
            // connection error out here, which is only the symptom.
            match crate::diagnose::headline(&report.unhealthy) {
                Some(line) => format!(
                    "chap-core at {} is down: {}",
                    report.api_url,
                    first_line(&line)
                ),
                None => format!("chap-core at {} is down: {error}", report.api_url),
            },
            Some(
                report
                    .unhealthy
                    .iter()
                    .find_map(|entry| entry.hint.clone())
                    .unwrap_or_else(|| {
                        "run `chap-devops logs chap` to see why, and `chap-devops status` for the detail"
                            .to_string()
                    }),
            ),
        ),
        ApiHealth::Rejected { .. } => (
            Status::Fail,
            format!(
                "chap-core at {} is up and did not accept the API token",
                report.api_url
            ),
            Some(
                "`chap-devops auth show --reveal` prints the token in .env; after `chap-devops auth \
                 rotate` or `enable`, `chap-devops up` hands it to chap-core"
                    .to_string(),
            ),
        ),
        ApiHealth::Up { .. } if report.missing.is_empty() => (
            Status::Ok,
            format!(
                "chap-core up at {}, {} of {} models registered",
                report.api_url,
                report.expected.len(),
                report.expected.len()
            ),
            // Registration is a heartbeat, so a green line here is not proof
            // that any of these models can produce a prediction. There is a
            // check that settles it, and `doctor` cannot make it: it runs the
            // models, which takes minutes.
            (!report.expected.is_empty()).then(|| crate::status::TEST_HINT.to_string()),
        ),
        ApiHealth::Up { .. } => (
            Status::Warn,
            format!(
                "chap-core up at {}, not registered: {}",
                report.api_url,
                report.missing.join(", ")
            ),
            Some("run `chap-devops status` for what to do about each one".to_string()),
        ),
    }
}

/// The way out when nothing runs: "Chap" only where this deployment has a
/// chap-core of its own to start.
fn start_fix(own_chap_core: bool) -> &'static str {
    match own_chap_core {
        true => "run `chap-devops up` to start Chap",
        false => "run `chap-devops up` to start the deployment",
    }
}

/// The `stack` line, which has nothing to check when nothing is up.
pub(super) fn stack_check(
    project: &Project,
    containers: Option<&[docker::Container]>,
    running: &BTreeSet<String>,
) -> Check {
    const ID: &str = "health";
    const NAME: &str = "health";
    if running.is_empty() {
        return Check::skip_with(
            ID,
            NAME,
            "no container of this deployment is running",
            start_fix(project.state.components.chap_core.enabled),
        );
    }
    let url = project.api_url();
    crate::output::verbose(&format!("asking chap-core at {url}"));
    let token = crate::api::token_for(Some(&project.dir));
    let mut report = crate::status::status(
        project,
        &url,
        STACK_TIMEOUT,
        running,
        token.as_deref(),
        true,
    );
    // The same diagnosis `chap-devops status` makes: when the API does not answer,
    // its container has been saying why in its own log.
    if matches!(report.api, ApiHealth::Down { .. })
        && let Some(containers) = containers
    {
        report.unhealthy =
            crate::diagnose::failing_containers(project, containers, Some(API_SERVICE));
    }
    Check::from_verdict(ID, NAME, stack_verdict(&report))
}

#[cfg(test)]
mod tests;
