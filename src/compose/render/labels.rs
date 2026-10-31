//! The labels every container chap-devops starts carries.
//!
//! They are what lets a tool find every chap-devops container on a machine with one
//! `docker ps --filter label=io.github.dhis2-chap.chap-devops.role`, whatever deployment or
//! `chap-devops run` group it belongs to, and tell what each one is without parsing
//! compose project names. Nothing in them depends on the chap-devops version or the
//! time: a changed label makes compose recreate the container, so an upgrade
//! of this CLI must render the same labels as the one before it.

// The keys are defined once, beside the query that reads them back
// ([`crate::docker::chap_devops_containers`]); these are the names the renderers use.
pub const LABEL_ROLE: &str = crate::docker::ROLE_LABEL;
pub const LABEL_MODEL: &str = crate::docker::MODEL_LABEL;
pub const LABEL_KIND: &str = crate::docker::KIND_LABEL;
pub const LABEL_GROUP: &str = crate::docker::GROUP_LABEL;

/// Every service of the chap-core component: the API, the worker and the
/// databases upstream's `compose.yml` defines.
pub const ROLE_CHAP_CORE: &str = "chap-core";
/// A model service, or the one-shot that prepares its volume.
pub const ROLE_MODEL: &str = "model";
/// The `ocs` component.
pub const ROLE_OCS: &str = "ocs";
/// The `s3` component, its bucket one-shot included.
pub const ROLE_S3: &str = "s3";
/// The `dhis2` component, its database and one-shots included.
pub const ROLE_DHIS2: &str = "dhis2";

/// A deployment `chap-devops init` wrote.
pub const KIND_INIT: &str = "init";
/// A deployment that is a `chap-devops run` group.
pub const KIND_RUN: &str = "run";

/// One service's `labels:` block, indented for a service under `services:`
/// and ending in its own newline.
///
/// `group` is the `chap-devops run` group the deployment is, and decides the kind.
/// The model id and the group name are quoted, since either could read as a
/// number or a boolean to YAML, and compose wants every label value a string.
pub fn labels_block(role: &str, model: Option<&str>, group: Option<&str>) -> String {
    let mut out = format!("    labels:\n      {LABEL_ROLE}: {role}\n");
    if let Some(model) = model {
        out.push_str(&format!("      {LABEL_MODEL}: \"{model}\"\n"));
    }
    let kind = if group.is_some() { KIND_RUN } else { KIND_INIT };
    out.push_str(&format!("      {LABEL_KIND}: {kind}\n"));
    if let Some(group) = group {
        out.push_str(&format!("      {LABEL_GROUP}: \"{group}\"\n"));
    }
    out
}

#[cfg(test)]
mod tests;
