//! What the run prints: the plan, and the lines it ends on.

use super::UpdateReport;
use super::chap_core::chap_core_cell;
use super::components::component_line;
use super::models::ModelUpdate;
use crate::output::{Out, Report};

/// What this run actually changed, as the closing line names it, or `None`
/// when the answer is "nothing".
///
/// Three clauses at most, in the order they matter: the pins that moved, and
/// then the images that arrived. A moving tag that pulled the same image
/// again is not a change and says nothing here, which is the whole point of
/// asking the image ids rather than trusting that a pull did something.
pub fn updated_phrase(
    models: usize,
    chap_core: Option<(&str, &str)>,
    pulled_new: &[String],
) -> Option<String> {
    let mut parts = Vec::new();
    if models > 0 {
        parts.push(format!(
            "{models} model pin{}",
            if models == 1 { "" } else { "s" }
        ));
    }
    if let Some((old, new)) = chap_core {
        parts.push(format!("chap-core {old} -> {new}"));
    }
    let pins = (!parts.is_empty()).then(|| parts.join(", "));
    // The images are a clause of their own, with its own verb: a pull is not
    // an update of anything, and "updated new image for ocs" is not English.
    let images = (!pulled_new.is_empty()).then(|| match pulled_new {
        [one] => format!("pulled a new image for {one}"),
        many => format!("pulled new images for {}", many.join(", ")),
    });
    match (pins, images) {
        (Some(pins), Some(images)) => Some(format!("{pins} and {images}")),
        (pins, images) => pins.or(images),
    }
}

/// [`updated_phrase`] for a finished report.
pub(super) fn updated(report: &UpdateReport) -> Option<String> {
    let chap_core = report
        .chap_core
        .as_ref()
        .filter(|c| c.changed)
        .map(|c| (c.old_tag.as_str(), c.new_tag.as_str()));
    updated_phrase(report.changed().count(), chap_core, &report.pulled_new)
}

/// The lines the run ends on.
///
/// They answer the only question left: is there anything to do now? A stale
/// running service is that answer whether or not this run is what made it
/// stale, so the restart comes first and stands on its own.
pub fn closing(
    lines: &mut Report,
    what: Option<&str>,
    restart_needed: &[String],
    running: Option<bool>,
) {
    let head = match what {
        // A run that only pulled says so in its own verb.
        Some(what) if what.starts_with("pulled ") => what.to_string(),
        Some(what) => format!("updated {what}"),
        None => "already up to date".to_string(),
    };
    if !restart_needed.is_empty() {
        lines
            .info(format!(
                "{head}; restart needed: {}",
                restart_needed.join(", ")
            ))
            .info("run `chap-devops restart` to apply");
        return;
    }
    lines.info(head);
    if what.is_none() {
        return;
    }
    match running {
        Some(false) => lines.hint("Chap is not running; `chap-devops up` starts the new versions"),
        Some(true) => lines.hint("nothing running needs a restart"),
        // Docker would not say what is running, so neither will we.
        None => lines.info("run `chap-devops restart` to apply it to what is running"),
    };
}

/// The lines a `--dry-run` ends on.
///
/// They say what the pins would do and nothing about restarting: the images
/// were not pulled and the files were not written, so what a running
/// container would then be out of step with does not exist yet.
pub fn dry_run(lines: &mut Report, what: Option<&str>) {
    match what {
        Some(what) => lines
            .info(format!("would update {what}"))
            .hint("`chap-devops update` does it"),
        None => lines.info("already up to date"),
    };
}

/// Where the catalogue came from, as a hint under the closing lines.
pub(super) fn registry_hint(report: &UpdateReport, lines: &mut Report) {
    lines.hint(format!(
        "the registry is {} ({})",
        report.registry.url,
        report.registry.provenance.describe()
    ));
}

/// The plan: a row per thing that can move.
pub(super) fn plan_text(report: &UpdateReport, out: &Out) -> String {
    let mut text = String::new();
    if report.models.is_empty() {
        text.push_str(&out.dim("no models enabled"));
        text.push('\n');
    }
    // The names are padded to one width so that the versions line up.
    let width = report
        .models
        .iter()
        .map(|m| m.id.len())
        .chain(report.chap_core.iter().map(|_| "chap-core".len()))
        .chain(report.components.iter().map(|c| c.name.len()))
        .max()
        .unwrap_or(0);
    for m in &report.models {
        // The new version is the only thing on the line that changed, so it is
        // the only thing that is coloured.
        let old = version_cell(m, &m.old_version, &m.old_tag);
        let line = if m.changed {
            let (old, new) = match digest_cells(m) {
                Some(cells) => cells,
                None => (old, version_cell(m, &m.new_version, &m.new_tag)),
            };
            format!("  {:<width$}  {} -> {}", m.id, out.dim(&old), out.ok(&new))
        } else {
            format!(
                "  {:<width$}  {}  {}",
                m.id,
                out.dim(&old),
                out.dim(state_of(m))
            )
        };
        text.push_str(&line);
        text.push('\n');
        if let Some(user) = user_line(m) {
            text.push_str(&format!("    {}\n", out.dim(&user)));
        }
    }
    let tense = |line: String| would(line, report.dry_run);
    if let Some(core) = &report.chap_core {
        text.push_str(&format!("  {}\n", tense(chap_core_cell(out, core, width))));
    }
    for component in &report.components {
        text.push_str(&format!(
            "  {}\n",
            out.dim(&tense(pad_name(
                component_line(component),
                &component.name,
                width
            )))
        ));
    }
    text
}

/// `line` with `name` at its start padded to `width`.
pub(super) fn pad_name(line: String, name: &str, width: usize) -> String {
    match line.strip_prefix(name) {
        Some(rest) => format!("{name:<width$}{rest}"),
        None => line,
    }
}

/// A plan line in the tense of the run: a dry run pulls nothing, so a moving
/// tag there *would be* re-pulled rather than being said to have been.
pub(super) fn would(line: String, dry_run: bool) -> String {
    if dry_run {
        line.replace("re-pulled", "would be re-pulled")
    } else {
        line
    }
}

/// What a row says about the account the model runs as, when the new tag
/// changed it: `user 1000:1000 -> root (image config)`.
///
/// Nothing at all when it did not, which is almost every row: an image that
/// kept its `USER` line has nothing to report here.
pub(super) fn user_line(m: &ModelUpdate) -> Option<String> {
    let (old, new) = m.moved_user()?;
    Some(format!("user {old} -> {new} ({})", m.user_from.label()))
}

/// How one row's pin reads: `v1.0.0 (sha-fa880a1)` for a marketplace model,
/// and the tag alone for a manual one, whose version is its tag.
pub(super) fn version_cell(m: &ModelUpdate, version: &str, tag: &str) -> String {
    if m.manual {
        return tag.to_string();
    }
    format!("v{version} ({tag})")
}

/// The two cells of a row where only the digest moved, so the change is
/// visible: `v1.0.2 (sha-e9047d4@380b0218ca3e)`. `None` for every other row.
pub(super) fn digest_cells(m: &ModelUpdate) -> Option<(String, String)> {
    if m.old_version != m.new_version || m.old_tag != m.new_tag {
        return None;
    }
    let cell = |digest: &Option<String>| {
        let digest = match digest {
            Some(digest) => crate::docker::short_digest(digest),
            None => "no digest".to_string(),
        };
        version_cell(m, &m.new_version, &format!("{}@{digest}", m.new_tag))
    };
    Some((cell(&m.old_digest), cell(&m.new_digest)))
}

/// What a row that did not move says for itself.
pub(super) fn state_of(m: &ModelUpdate) -> &'static str {
    if m.pinned {
        return "pinned, skipped";
    }
    if !m.checked {
        return "unchanged (could not check)";
    }
    "unchanged"
}
