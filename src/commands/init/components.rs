//! The component set `init` writes, from `--with`, `--without`, `--only` and
//! the per-component flags.

use crate::cli::{ComponentPortArg, InitArgs};
use crate::components::{Component, Components};
use crate::error::Result;

/// The `init` flags that shape the component set, so [`parse_components`] takes
/// one argument per concern rather than one per flag.
#[derive(Debug, Clone, Default)]
pub(super) struct ComponentFlags<'a> {
    pub(super) with: Option<&'a str>,
    pub(super) without: Option<&'a str>,
    /// `--only`: the whole set, chap-core included only when it is named.
    pub(super) only: Option<&'a str>,
    pub(super) ocs_base_url: Option<&'a str>,
    pub(super) ocs_port: Option<ComponentPortArg>,
    pub(super) s3_port: Option<ComponentPortArg>,
    pub(super) dhis2_port: Option<ComponentPortArg>,
    pub(super) dhis2_seed: Option<&'a str>,
    pub(super) dhis2_seed_password: Option<&'a str>,
    pub(super) dhis2_tag: Option<&'a str>,
    pub(super) dhis2_image: Option<&'a str>,
    pub(super) ocs_read_only: bool,
    /// The global `--offline`, which is not a component flag but decides one
    /// thing here: a seed that has to be downloaded cannot be asked for by a run
    /// that was told not to touch the network.
    pub(super) offline: bool,
}

impl<'a> ComponentFlags<'a> {
    pub(super) fn from_args(args: &'a InitArgs, offline: bool) -> ComponentFlags<'a> {
        ComponentFlags {
            with: args.with.as_deref(),
            without: args.without.as_deref(),
            only: args.only.as_deref(),
            ocs_base_url: args.ocs_base_url.as_deref(),
            ocs_port: args.ocs_port,
            s3_port: args.s3_port,
            dhis2_port: args.dhis2_port,
            dhis2_seed: args.dhis2_seed.as_deref(),
            dhis2_seed_password: args.dhis2_seed_password.as_deref(),
            dhis2_tag: args.dhis2_tag.as_deref(),
            dhis2_image: args.dhis2_image.as_deref(),
            ocs_read_only: args.ocs_read_only,
            offline,
        }
    }
}

/// Expand `--with`, `--without` and the per-component `--ocs-*` / `--s3-*`
/// flags into a component set.
///
/// chap-core is on unless `--without chap-core` says otherwise; everything
/// else is off until `--with` names it. A name in both lists is a
/// contradiction rather than a silent winner, and so is a setting for a
/// component this deployment is not getting: an `--ocs-port` that quietly did
/// nothing would leave the operator waiting for OCS on a port no file mentions.
pub(super) fn parse_components(flags: &ComponentFlags) -> Result<Components> {
    let (on, off) = match flags.only {
        Some(only) => only_lists(only)?,
        None => (
            parse_component_list(flags.with)?,
            parse_component_list(flags.without)?,
        ),
    };
    if let Some(both) = on.iter().find(|c| off.contains(c)) {
        return Err(anyhow::anyhow!(
            "`{}` is in both --with and --without; it cannot be on and off at once",
            both.name()
        ));
    }
    let mut components = Components::default();
    for component in on {
        components.set_enabled(component, true);
    }
    for component in off {
        components.set_enabled(component, false);
    }
    if let Some(base_url) = flags
        .ocs_base_url
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        if !components.ocs.enabled {
            return Err(anyhow::anyhow!(
                "--ocs-base-url needs the ocs component; add `--with ocs`"
            ));
        }
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(anyhow::anyhow!(
                "--ocs-base-url has to be an absolute URL, so `{base_url}` will not do: OCS \
                 builds its STAC and openEO links by appending a path to this value"
            ));
        }
        components.ocs.base_url = Some(base_url.trim_end_matches('/').to_string());
    }
    if let Some(port) = flags.ocs_port {
        if !components.ocs.enabled {
            return Err(anyhow::anyhow!(
                "--ocs-port needs the ocs component; add `--with ocs`"
            ));
        }
        components.ocs.port = port.0;
    }
    if let Some(port) = flags.s3_port {
        if !components.s3.enabled {
            return Err(anyhow::anyhow!(
                "--s3-port needs the s3 component; add `--with s3`"
            ));
        }
        components.s3.port = port.0;
    }
    if let Some(port) = flags.dhis2_port {
        if !components.dhis2.enabled {
            return Err(anyhow::anyhow!(
                "--dhis2-port needs the dhis2 component; add `--with dhis2`"
            ));
        }
        components.dhis2.port = port.0;
    }
    // Before the seed, which `default` resolves against the version.
    if let Some(tag) = flags.dhis2_tag.map(str::trim) {
        if !components.dhis2.enabled {
            return Err(anyhow::anyhow!(
                "--dhis2-tag needs the dhis2 component; add `--with dhis2`"
            ));
        }
        components.dhis2.image_tag = crate::components::dhis2_tag_arg(tag, "--dhis2-tag")?;
    }
    if let Some(image) = flags.dhis2_image.map(str::trim) {
        if !components.dhis2.enabled {
            return Err(anyhow::anyhow!(
                "--dhis2-image needs the dhis2 component; add `--with dhis2`"
            ));
        }
        components.dhis2.image =
            crate::components::dhis2_image_arg(image, "--dhis2-image", "--dhis2-tag")?;
    }
    if let Some(given) = flags.dhis2_seed {
        if !components.dhis2.enabled {
            return Err(anyhow::anyhow!(
                "--dhis2-seed needs the dhis2 component; add `--with dhis2`"
            ));
        }
        let seed = crate::components::Dhis2Seed::parse(given);
        // The dump is downloaded by the one-shot on the first `chap-devops up`, not
        // here, so this refusal is about the deployment being written rather than
        // about this run's network: `--offline` is a deployment that does not
        // reach out, and recording a URL would make its first start do exactly
        // that.
        if flags.offline && seed.is_url() {
            return Err(anyhow::anyhow!(
                "--offline and `--dhis2-seed {}` ask for opposite things: the first `chap-devops up` \
                 would download that dump; pass a path to a dump you already have, or \
                 `--dhis2-seed none`",
                seed.as_str()
            ));
        }
        components.dhis2.seed = seed;
    }
    if let Some(password) = flags.dhis2_seed_password {
        if !components.dhis2.enabled {
            return Err(anyhow::anyhow!(
                "--dhis2-seed-password needs the dhis2 component; add `--with dhis2`"
            ));
        }
        if components.dhis2.seed == crate::components::Dhis2Seed::None {
            return Err(anyhow::anyhow!(
                "--dhis2-seed-password needs a seed, and `--dhis2-seed none` has no users; \
                 drop one of the two"
            ));
        }
        if password.is_empty() {
            return Err(anyhow::anyhow!(
                "--dhis2-seed-password needs a password; drop the value to use `district`"
            ));
        }
        if let Some(problem) = crate::dotenv::literal_problem(password) {
            return Err(anyhow::anyhow!(
                "--dhis2-seed-password has {problem}; choose another password"
            ));
        }
        components.dhis2.seed_password = true;
    }
    if flags.ocs_read_only {
        if !components.ocs.enabled {
            return Err(anyhow::anyhow!(
                "--ocs-read-only needs the ocs component; add `--with ocs`"
            ));
        }
        // The record only; the file it records is settled once the scaffold is
        // on disk, by the one function that edits it.
        components.ocs.read_only = true;
    }
    Ok(components)
}

/// A comma-separated list of component names.
/// `--only LIST` as the `--with` and `--without` lists it stands for: the
/// named components on and every other one off. `none` is the empty set, for a
/// deployment of model services alone.
pub(super) fn only_lists(spec: &str) -> Result<(Vec<Component>, Vec<Component>)> {
    let on = if spec.trim() == "none" {
        Vec::new()
    } else {
        let on = parse_component_list(Some(spec))?;
        if on.is_empty() {
            return Err(anyhow::anyhow!(
                "--only needs a component list, such as `--only ocs,s3`, or `--only none` \
                 for model services alone"
            ));
        }
        on
    };
    let off = Component::ALL
        .iter()
        .copied()
        .filter(|c| !on.contains(c))
        .collect();
    Ok((on, off))
}

pub(super) fn parse_component_list(spec: Option<&str>) -> Result<Vec<Component>> {
    let Some(spec) = spec else {
        return Ok(Vec::new());
    };
    spec.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(Component::from_name)
        .collect()
}
