//! Model overlays: one `compose.<service_id>.yml` per enabled model.

use super::labels::{ROLE_MODEL, labels_block};
use super::{normalize_newlines, render_template};
use crate::compose::overrides;
use crate::compose::spec::OverlaySpec;
use std::sync::LazyLock;

/// One model service and its init container, as a compose overlay.
///
/// A model needs no host port to work: chap-core reaches it at
/// `http://<service_id>:8000` on the compose default network, which is the URL
/// the service registers, and a human reaches it through chap-core's read-only
/// proxy. A published port is for people and is opt-in per model. See
/// `docs/ports.md`.
pub(super) static OVERLAY_TEMPLATE: LazyLock<String> =
    LazyLock::new(|| normalize_newlines(include_str!("../templates/compose.overlay.yml")));

/// Render one `compose.<service_id>.yml` overlay.
///
/// An image that runs as root gets no `user:` line: there is nothing to
/// override, and root can write a fresh volume as docker seeded it. Every
/// image gets an init container, carrying the same numeric `uid:gid` as the
/// `user:` line where there is one - so compose and the `chown` cannot drift
/// apart, and the numbers because the init container is busybox, which
/// resolves no account name of its own.
pub fn render_overlay(spec: &OverlaySpec) -> String {
    let external = spec.external_chap_core.as_ref();
    render_template(
        "compose.overlay.yml",
        &OVERLAY_TEMPLATE,
        minijinja::context! {
            id => &spec.id,
            version => &spec.version,
            repository => &spec.repository,
            service_id => &spec.service_id,
            // The model and its init container carry the same labels, so a
            // tool that lists a model's containers by its id finds the
            // one-shot as well.
            labels => labels_block(ROLE_MODEL, Some(&spec.id), spec.group.as_deref()),
            image => &spec.image,
            // A digest carries its own `@`, so the reference is joined with
            // nothing rather than with a colon.
            tag_sep => crate::compose::tag_separator(&spec.image_tag),
            tag_var => &spec.tag_env_var,
            image_tag => &spec.image_tag,
            local => crate::compose::is_local_image(&spec.image),
            platform => spec.platform.as_deref(),
            container_port => container_port(spec),
            host_port => spec.host_port,
            bind => match spec.bind {
                Some(std::net::IpAddr::V4(addr)) => format!("{addr}:"),
                Some(std::net::IpAddr::V6(addr)) => format!("[{addr}]:"),
                None => String::new(),
            },
            external => external.is_some(),
            register_url => external.map(|e| e.register_url.as_str()),
            models_host => external.map(|e| e.models_host.as_str()),
            host_gateway => crate::components::HOST_GATEWAY,
            standalone => spec.standalone,
            reads_port => spec.reads_port,
            // With authentication on, compose reads the key from the `.env`
            // next to these files, so it is never copied into an overlay.
            registration_key => spec.registration_key,
            // The numeric form, so this line and the chown are the same two
            // numbers; a name nothing could resolve is kept as it is, which is
            // what `chap-devops sync` warns about. A root image gets no line.
            user => (!overrides::is_root(&spec.user)).then(|| {
                overrides::numeric_pair(&spec.user).unwrap_or_else(|| spec.user.clone())
            }),
            uid_gid => overrides::chown_pair(&spec.user),
            data_dir => &spec.data_dir,
            volume => &spec.volume_name,
        },
    )
}

/// The port the model listens on inside its container.
///
/// 8000, except for a model registered with a chap-core elsewhere whose image
/// reads `PORT`: that model listens on its host port.
///
/// Before servicekit registers, it checks that the app answers inside the
/// container. servicekit 2 checks only `SERVICEKIT_PORT`, the host port, so
/// its app has to listen there, and it is told through `PORT`. servicekit 3
/// also checks 8000, so an image that fixes `--port 8000` (every marketplace
/// model but one) stays on 8000, with its host port mapped to it: `PORT`
/// would not move it, and a mapping to the host port would reach nothing.
fn container_port(spec: &OverlaySpec) -> u16 {
    match (&spec.external_chap_core, spec.host_port) {
        (Some(_), Some(port)) if spec.reads_port => port,
        _ => 8000,
    }
}
