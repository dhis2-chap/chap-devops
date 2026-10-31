//! The `ocs` component, the `s3` store beside it, and the scaffolded OCS
//! instance config.

use super::labels::{ROLE_OCS, ROLE_S3, labels_block};
use super::{fill, normalize_newlines, render_template};
use crate::components::{
    OCS_BASE_URL_ENV_VAR, OCS_COMPUTE_ENV_VARS, OCS_CONFIG_FILE, OCS_CONTAINER_PORT,
    OCS_DATA_SOURCE_ENV_VARS, OCS_DIR, OCS_IMAGE, OCS_PLUGINS_DIR, OCS_PLUGINS_TARGET,
    OCS_TAG_ENV_VAR, S3_ACCESS_KEY_ENV_VAR, S3_BUCKET, S3_CONTAINER_PORT, S3_IMAGE,
    S3_SECRET_KEY_ENV_VAR, S3_TAG_ENV_VAR,
};
use crate::compose::spec::{OcsConfigSpec, OcsSpec, S3Spec};
use std::sync::LazyLock;

/// The `ocs` component, as a compose file of its own.
pub(super) static OCS_TEMPLATE: LazyLock<String> =
    LazyLock::new(|| normalize_newlines(include_str!("../templates/compose.ocs.yml")));
/// The `s3` component, likewise.
pub(super) static S3_TEMPLATE: LazyLock<String> =
    LazyLock::new(|| normalize_newlines(include_str!("../templates/compose.s3.yml")));
/// The OCS instance config `init --with ocs` scaffolds.
static OCS_CONFIG_TEMPLATE: LazyLock<String> =
    LazyLock::new(|| normalize_newlines(include_str!("../templates/climate-service.yaml")));

/// Named volume holding OCS's data directory.
pub const OCS_VOLUME: &str = "ocs_data";
/// Named volume holding the object store's data directory.
pub const S3_VOLUME: &str = "s3_data";
/// Region the bucket request is signed for. RustFS ignores it, but SigV4 has
/// to name one.
const S3_REGION: &str = "us-east-1";

/// The marker line the scaffolded `ocs/climate-service.yaml` carries while it
/// still holds OCS's example values.
///
/// `chap-devops doctor` looks for exactly this, and the note tells the reader to
/// delete it once the file is theirs, so the check goes quiet by itself.
pub const OCS_EXAMPLE_MARKER: &str =
    "# These are the Laos example values, the country of the DHIS2 demo.";

/// Render `compose.ocs.yml`: the `ocs` component.
///
/// Three blocks are conditional. The host port is published only when the
/// component records one, so an instance behind a reverse proxy is `expose`d on
/// the compose network and nowhere else. The plugin mount appears only when the
/// project has an `ocs/plugins/` to mount, since compose fails to start a
/// service whose bind source does not exist. And the `S3_*` block only appears
/// when the `s3` component is on: OCS does not read those variables yet - it is
/// about to - so they are written as forward-looking settings with a comment
/// that says as much, rather than being left out until the day the contract
/// lands.
///
/// The five dataset credential variables are unconditional, all of them
/// `${VAR:-}`. OCS reads each one as `os.getenv(...) or <the file>`, so an empty
/// value is one it does not have: passing them always costs a deployment that
/// sets none of them nothing, and means the operator only ever has to put a
/// credential in `.env` rather than in `.env` and then in `.chap-devops/` as well.
pub fn render_ocs(spec: &OcsSpec) -> String {
    render_template(
        "compose.ocs.yml",
        &OCS_TEMPLATE,
        minijinja::context! {
            labels => labels_block(ROLE_OCS, None, spec.group.as_deref()),
            image => OCS_IMAGE,
            tag_var => OCS_TAG_ENV_VAR,
            image_tag => &spec.image_tag,
            container_port => OCS_CONTAINER_PORT,
            host_port => spec.host_port,
            base_url_var => OCS_BASE_URL_ENV_VAR,
            base_url => spec.base_url.as_deref().unwrap_or_default(),
            data_source_vars => OCS_DATA_SOURCE_ENV_VARS,
            compute_vars => OCS_COMPUTE_ENV_VARS,
            s3 => spec.s3,
            s3_port => S3_CONTAINER_PORT,
            s3_access_key_var => S3_ACCESS_KEY_ENV_VAR,
            s3_secret_key_var => S3_SECRET_KEY_ENV_VAR,
            s3_bucket => S3_BUCKET,
            config_path => format!("{OCS_DIR}/{OCS_CONFIG_FILE}"),
            plugins => spec.plugins,
            plugins_path => format!("{OCS_DIR}/{OCS_PLUGINS_DIR}"),
            plugins_target => OCS_PLUGINS_TARGET,
            volume => OCS_VOLUME,
        },
    )
}

/// Render `compose.s3.yml`: the `s3` component.
pub fn render_s3(spec: &S3Spec) -> String {
    render_template(
        "compose.s3.yml",
        &S3_TEMPLATE,
        minijinja::context! {
            labels => labels_block(ROLE_S3, None, spec.group.as_deref()),
            image => S3_IMAGE,
            tag_var => S3_TAG_ENV_VAR,
            image_tag => &spec.image_tag,
            container_port => S3_CONTAINER_PORT,
            host_port => spec.host_port,
            access_key_var => S3_ACCESS_KEY_ENV_VAR,
            secret_key_var => S3_SECRET_KEY_ENV_VAR,
            bucket => S3_BUCKET,
            region => S3_REGION,
            volume => S3_VOLUME,
        },
    )
}

/// Render the scaffolded `ocs/climate-service.yaml`.
///
/// Written once, when the component is enabled, and never again: it is the
/// operator's file from that moment on.
pub fn render_ocs_config(spec: &OcsConfigSpec) -> String {
    // The token starts its own line and carries its own newlines, so a file
    // written from real values leaves no blank comment block behind.
    let note = if spec.example {
        format!("{OCS_EXAMPLE_MARKER} Edit them, then delete this note.\n")
    } else {
        String::new()
    };
    fill(
        &OCS_CONFIG_TEMPLATE,
        &[
            ("EXAMPLE_NOTE", &note),
            ("ID", &spec.id),
            ("NAME", &spec.name),
            ("EXTENT_NAME", &spec.extent_name),
            ("BBOX", &spec.bbox),
            ("COUNTRY_CODE", &spec.country_code),
        ],
    )
}
