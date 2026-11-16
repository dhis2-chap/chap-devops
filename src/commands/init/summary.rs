//! What `init` prints once the directory is written.

use super::chap_core::ChapCore;
use super::env::{EnvAction, Secrets};
use crate::components::{Components, DHIS2_FIRST_START_NOTE, S3_SOON_NOTE, S3_WITHOUT_OCS_NOTE};
use crate::compose::ApplyReport;
use crate::output::Report;
use crate::project::{CHAP_TAG_ENV_VAR, Project};
use std::path::{Path, PathBuf};

/// What the summary says for a deployment that enables no models.
pub(super) const NO_MODELS: &str =
    "no models are enabled; run `chap-devops models enable ID` to add one";

/// The hint that goes with it in a deployment chap-core is not a component
/// of, where a model enabled later runs on its own: it registers nowhere and
/// is published on a host port.
pub(super) const NO_MODELS_WITHOUT_CHAP_CORE: &str =
    "without chap-core, each model runs on its own, on a published host port";

/// The no-models line and its hint, as a pure function of the component set
/// so the choice is testable without a deployment on disk.
///
/// Neither, for a deployment of components without chap-core - OCS, DHIS2 -
/// where models were not the point and a line about adding one is noise. A
/// deployment of nothing but models (`--only none`) still gets it.
pub(super) fn no_models_line(
    components: &Components,
) -> Option<(&'static str, Option<&'static str>)> {
    // A chap-core elsewhere in front of components of this deployment's own
    // (a DHIS2 for the chap-core being developed) has its models there.
    if components.chap_core_external.is_some() && !components.enabled().is_empty() {
        None
    } else if components.has_chap_core_api() {
        Some((NO_MODELS, None))
    } else if components.enabled().is_empty() {
        Some((NO_MODELS, Some(NO_MODELS_WITHOUT_CHAP_CORE)))
    } else {
        None
    }
}

/// The closing lines of `init`.
#[expect(
    clippy::too_many_arguments,
    reason = "one summary line per piece init decided"
)]
pub(super) fn summary(
    dir: &Path,
    written: &[PathBuf],
    report: &ApplyReport,
    env: EnvAction,
    chap_core: &ChapCore,
    project: &Project,
    secrets: Option<&Secrets>,
    dropped_notes: &[String],
    tag_moved: Option<&(String, String)>,
    lines: &mut Report,
) {
    lines.info(format!("created a deployment in {}", dir.display()));
    for path in written {
        lines.hint(format!("wrote {}", file_label(dir, path)));
    }
    for path in &report.removed {
        lines.hint(format!("removed {}", file_label(dir, path)));
    }
    if env == EnvAction::Kept {
        lines.hint("kept .env (already present)");
    }
    if let Some((from, to)) = tag_moved {
        lines.info(format!(
            "moved {CHAP_TAG_ENV_VAR} in the kept .env from {from} to {to}"
        ));
    }
    let components = &project.state.components;
    lines.info(format!("components: {}", components.label()));
    if components.chap_core.enabled {
        lines.info(format!(
            "chap-core: {}, API on {}",
            chap_core.tag,
            project.api_url()
        ));
        lines.hint(format!(
            "chap-core {} is from {}",
            chap_core.tag,
            chap_core.source.describe()
        ));
    }
    if let Some(external) = &components.chap_core_external {
        lines.info(format!("chap-core: {} (elsewhere)", external.url));
        lines.hint(format!(
            "models register at {}, and it calls them back at {}:<port>",
            external.url, external.models_host
        ));
    }
    if components.ocs.enabled {
        lines.info(format!("OCS: {}", components.ocs_reach()));
        lines.hint(format!(
            "the OCS config is in `{}/{}`",
            crate::components::OCS_DIR,
            crate::components::OCS_CONFIG_FILE
        ));
    }
    if components.dhis2.enabled {
        lines.info(format!("DHIS2: {}", components.dhis2_reach()));
        lines.hint(format!(
            "the DHIS2 config is in `{}/{}`",
            crate::components::DHIS2_DIR,
            crate::components::DHIS2_CONFIG_FILE
        ));
    }
    if components.s3.enabled {
        lines.info(match components.s3.port {
            Some(port) => format!("S3: http://localhost:{port}"),
            None => format!(
                "S3: internal (http://s3:{} inside the deployment)",
                crate::components::S3_CONTAINER_PORT
            ),
        });
    }
    // No part of the token is printed: it stays in `.env`, and
    // `chap-devops auth show --reveal` is the way back to it.
    let token = match (secrets.is_some(), project.state.auth.api_token) {
        (true, _) => Some("API token: generated into .env"),
        (false, true) => Some("API token: set in .env"),
        (false, false) => None,
    };
    if let Some(token) = token {
        lines.info(token);
        lines.hint("`chap-devops auth show --reveal` prints the API token");
    }

    if report.enabled.is_empty() {
        if let Some((line, hint)) = no_models_line(components) {
            lines.info(line);
            if let Some(hint) = hint {
                lines.hint(hint);
            }
        }
    } else {
        for (id, model) in &report.enabled {
            // A manually added model's version is its image tag, so `v` in
            // front of it would read as a version number it does not have.
            let version = match model.version == model.image_tag {
                true => model.image_tag.clone(),
                false => format!("v{}", model.version),
            };
            let place = match model.host_port {
                Some(port) => format!("on http://localhost:{port}"),
                None => format!("at {}", project.proxy_url(&model.service_id)),
            };
            lines.info(format!("enabled {id} {version} {place}"));
        }
        if report.enabled.iter().any(|(_, m)| m.host_port.is_none()) {
            lines.hint(
                "model services publish no host port; chap-core reaches them over the compose \
                 network, and `chap-devops models expose ID` publishes one",
            );
        }
    }
    if components.ocs.enabled && !components.s3.enabled {
        lines.hint(S3_SOON_NOTE);
    }
    // The same soft dependency the other way round, in the same words
    // `components enable s3` uses: a store with nothing to put in it.
    if components.s3.enabled && !components.ocs.enabled {
        lines.hint(S3_WITHOUT_OCS_NOTE);
    }
    // What the first start does with the database, and how long it takes.
    if components.dhis2.enabled {
        // The warning says it with the reason when the pinned minor has no
        // dump, so this line would be the same sentence twice.
        if !components.dhis2_seed_is_unknown() {
            lines.hint(crate::components::dhis2_seed_note(
                components.dhis2_seed_source(),
            ));
        }
        if components.dhis2_seed_source().is_some() && components.dhis2.seed_password {
            lines.hint(crate::components::dhis2_seed_password_note());
        }
        lines.hint(DHIS2_FIRST_START_NOTE);
        // Only with a chap-core to connect it to.
        if components.has_chap_core_api() {
            lines.hint(crate::components::DHIS2_CONNECT_NOTE);
        }
    }
    // What the containers of a dropped component or model did on the way out.
    // The warnings already said which components are going; this says what
    // was actually stopped, which is the half a reader cannot infer.
    for note in dropped_notes {
        lines.info(note.as_str());
    }
    for warning in &report.warnings {
        lines.warning(warning.as_str());
    }

    // The last line is the step to do next. A deployment with nothing in it
    // has nothing for `up` to start: adding something is the next step there.
    let empty = components.enabled().is_empty()
        && components.chap_core_external.is_none()
        && report.enabled.is_empty();
    match empty {
        true => {
            lines.info(format!(
                "run `cd {} && chap-devops models add URL` to add a model",
                dir.display()
            ));
            lines.hint("`chap-devops up` then starts it");
        }
        false => {
            lines.info(format!(
                "run `cd {} && chap-devops up` to start the deployment",
                dir.display()
            ));
            lines.hint("`chap-devops status` then shows what runs");
        }
    };
}

/// Paths are printed relative to the project directory; everything written
/// lives inside it.
pub(super) fn file_label(dir: &Path, path: &Path) -> String {
    path.strip_prefix(dir).unwrap_or(path).display().to_string()
}
