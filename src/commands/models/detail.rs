//! `chap-devops models info`: one model in full, and its `--json`.

use super::{MANUAL_KIND, channel_image, enabled_entry, kind_label, open_project};
use crate::cli::ModelsInfoArgs;
use crate::commands::Ctx;
use crate::error::{ChapError, Result};
use crate::output::{self, Out, Report};
use crate::project::{EnabledModel, ManualModel};
use crate::registry::{Channel, Model, VersionStatus};

/// `models info`, and the shape of its `--json`: the whole marketplace entry
/// plus what the CLI derives from it.
#[derive(Debug, serde::Serialize)]
pub(super) struct ModelDetail<'a> {
    #[serde(flatten)]
    pub(super) model: &'a Model,
    /// This project's entry for the model, when it is enabled.
    pub(super) enabled: Option<&'a EnabledModel>,
    /// The definition behind a manually added model, which is where its date
    /// and its branch are recorded.
    #[serde(skip_serializing)]
    pub(super) manual: Option<&'a ManualModel>,
    /// Whether this ran inside a deployment directory at all, which is what
    /// tells "not enabled here" from "there is no here yet".
    pub(super) in_project: bool,
    /// How to reach it from this machine, when it is enabled: its own host
    /// port, or chap-core's proxy for a service that publishes none.
    pub(super) reach: Option<String>,
    pub(super) image_stable: Option<String>,
    pub(super) image_latest: Option<String>,
    pub(super) needs_amd64: bool,
}

/// Show one model in full.
pub fn info(ctx: &Ctx, args: &ModelsInfoArgs) -> Result<()> {
    let project = open_project(ctx, false)?;
    let registry = crate::commands::registry_for(ctx, project.as_ref())?;

    let model = registry
        .get(&args.id)
        .ok_or_else(|| ChapError::UnknownModel(args.id.clone()))?;
    let enabled = enabled_entry(project.as_ref(), model);
    let detail = ModelDetail {
        model,
        enabled,
        manual: project.as_ref().and_then(|p| p.state.manual.get(&model.id)),
        in_project: project.is_some(),
        reach: enabled
            .zip(project.as_ref())
            .map(|(e, p)| crate::status::reach(p, e.host_port, &e.service_id)),
        image_stable: channel_image(model, Channel::Stable),
        image_latest: channel_image(model, Channel::Latest),
        needs_amd64: model.needs_amd64(),
    };

    if !ctx.out.json {
        print!("{}", render_info(&ctx.out, &detail));
        // The line that says the model is not enabled stands apart from the
        // page, as the project block would.
        if detail.enabled.is_none() && detail.in_project {
            println!();
        }
    }
    ctx.out.report(&detail, |lines| info_lines(&detail, lines))
}

/// The lines under the `models info` page: whether this project runs the
/// model, and how to enable it when it does not.
pub(super) fn info_lines(detail: &ModelDetail, lines: &mut Report) {
    let id = &detail.model.id;
    if detail.enabled.is_some() {
        return;
    }
    match detail.in_project {
        // The absence of the project block is easy to miss.
        true => lines
            .info(format!("{id} is not enabled in this deployment"))
            .hint(format!("`chap-devops models enable {id}` enables it")),
        false => lines.hint(format!(
            "this is not a deployment directory; `chap-devops init` creates one, then \
             `chap-devops models enable {id}` enables the model"
        )),
    };
}

/// The STATUS cell of one version in `models info`.
fn version_status_cell(out: &Out, status: VersionStatus) -> String {
    let label = version_status_label(status);
    match status {
        VersionStatus::Verified => out.ok(label),
        VersionStatus::Unstable => out.warn(label),
        VersionStatus::Deprecated => out.warn(label),
        VersionStatus::Yanked => out.bad(label),
    }
}

pub(super) fn render_info(out: &Out, detail: &ModelDetail) -> String {
    let m = detail.model;

    let mut runtime = m.source.runtime_image.clone();
    if detail.needs_amd64 {
        // The R-INLA base has no arm64 build, so an Apple Silicon host runs it
        // under emulation; that is worth saying before someone waits for it.
        runtime.push_str("  (amd64 only)");
    }

    let mut attribution = m.attribution.author.clone();
    if let Some(org) = &m.attribution.organization {
        attribution.push_str(&format!(", {org}"));
    }
    if let Some(contact) = &m.attribution.contact {
        attribution.push_str(&format!(" <{contact}>"));
    }
    if let Some(citation) = &m.attribution.citation {
        attribution.push_str(&format!("\n{citation}"));
    }

    // A manually added entry has no marketplace metadata to show, and a
    // column of zeroes and `no`s would read as facts nobody established.
    // What it does have is where it came from and what it follows.
    let (source, follows) = match detail.manual {
        Some(manual) => (
            format!("{MANUAL_KIND} (`chap-devops models add`, {})", manual.added),
            match &manual.follow {
                Some(branch) => branch.clone(),
                None => "nothing (pinned)".to_string(),
            },
        ),
        None => (String::new(), String::new()),
    };
    let marketplace = |text: String| match detail.manual {
        Some(_) => String::new(),
        None => text,
    };

    let mut covariates = Vec::new();
    if !m.covariates.required.is_empty() {
        covariates.push(format!("required: {}", m.covariates.required.join(", ")));
    }
    if !m.covariates.defaults.is_empty() {
        covariates.push(format!("defaults: {}", m.covariates.defaults.join(", ")));
    }
    covariates.push(format!(
        "extra covariates: {}",
        yes_no(m.covariates.allow_free_additional)
    ));

    let fields = output::fields_with(
        2,
        &[
            ("id", m.id.clone()),
            ("service", m.service_id.clone()),
            (
                "kind",
                match detail.manual {
                    Some(_) => MANUAL_KIND.to_string(),
                    None => kind_label(m.kind).to_string(),
                },
            ),
            ("source", source),
            ("follows", follows),
            (
                "status",
                marketplace(output::wrapped(
                    &format!(
                        "{}, {}",
                        m.assessed_status.colour(),
                        m.assessed_status.describe()
                    ),
                    DETAIL_WRAP,
                )),
            ),
            ("summary", output::wrapped(&m.summary, DETAIL_WRAP)),
            ("repository", m.source.repository.clone()),
            (
                "image",
                channel_line(detail.image_stable.as_deref(), &m.channels.stable),
            ),
            (
                "latest",
                channel_line(detail.image_latest.as_deref(), &m.channels.latest),
            ),
            ("runtime", runtime),
            ("period types", m.compatibility.period_types.join(", ")),
            (
                "horizon",
                marketplace(format!(
                    "{}-{} prediction periods",
                    m.compatibility.min_prediction_periods, m.compatibility.max_prediction_periods
                )),
            ),
            (
                "requires geo",
                marketplace(yes_no(m.compatibility.requires_geo).to_string()),
            ),
            ("covariates", marketplace(covariates.join("\n"))),
            (
                "channels",
                marketplace(format!(
                    "stable {}, latest {}",
                    m.channels.stable, m.channels.latest
                )),
            ),
            ("maintainers", m.maintainers.join(", ")),
            ("attribution", attribution),
        ],
        &|label| out.dim(label),
    );

    let mut text = format!("{}\n\n{fields}", out.heading(&m.display_name));

    let versions: Vec<Vec<String>> = m
        .versions
        .iter()
        .map(|v| {
            vec![
                v.version.clone(),
                out.dim(&v.image_tag),
                version_status_cell(out, v.status),
                v.chapkit.clone(),
                out.dim(&first_line(v.changelog.as_deref())),
            ]
        })
        .collect();
    if !versions.is_empty() {
        text.push_str(&format!("\n{}\n", out.heading("versions")));
        text.push_str(&indent_block(
            &out.table(
                &["VERSION", "IMAGE TAG", "STATUS", "CHAPKIT", "CHANGELOG"],
                &versions,
            ),
            2,
        ));
    }

    if !m.configurations.is_empty() {
        text.push_str(&format!("\n{}\n", out.heading("configurations")));
        for (name, cfg) in &m.configurations {
            text.push_str(&format!("  {}\n", out.cmd(name)));
            if let Some(description) = &cfg.description {
                text.push_str(&indent_block(&output::wrapped(description, DETAIL_WRAP), 4));
            }
            let body = serde_json::to_string_pretty(&cfg.config)
                .unwrap_or_else(|_| cfg.config.to_string());
            text.push_str(&indent_block(&body, 4));
        }
    }

    if let Some(enabled) = detail.enabled {
        text.push_str(&format!(
            "\n{}\n",
            out.heading("enabled in this deployment")
        ));
        text.push_str(&output::fields_with(
            2,
            &[
                (
                    "reach",
                    detail
                        .reach
                        .clone()
                        .unwrap_or_else(|| match enabled.host_port {
                            Some(port) => port.to_string(),
                            None => "internal".to_string(),
                        }),
                ),
                (
                    "version",
                    match enabled.channel {
                        Some(channel) => {
                            format!("{} ({})", enabled.version, channel.as_str())
                        }
                        None => format!("{} (pinned)", enabled.version),
                    },
                ),
                ("image", enabled.image_ref()),
                ("data dir", enabled.data_dir.clone()),
                // Where the user came from is half the answer: `table` is the
                // last resort, and the difference between `root` from the
                // image and `root` from a guess is what someone debugging a
                // permission error needs to see.
                (
                    "user",
                    format!("{}  ({})", enabled.user, enabled.user_from.label()),
                ),
                ("overlay", enabled.compose_file.clone()),
            ],
            &|label| out.dim(label),
        ));
    }

    text
}

/// Width the prose in `models info` is wrapped to: the 80-column budget minus
/// the indent and label column [`output::fields`] adds.
const DETAIL_WRAP: usize = output::WRAP_WIDTH - 18;

/// `image:tag  (version)`, or just the version when the channel points at a
/// version the model file does not list.
fn channel_line(image: Option<&str>, version: &str) -> String {
    match image {
        Some(image) => format!("{image}  ({version})"),
        None => format!("{version} (no such version in this model file)"),
    }
}

pub(super) fn first_line(text: Option<&str>) -> String {
    text.unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

pub(super) fn indent_block(text: &str, spaces: usize) -> String {
    let lead = " ".repeat(spaces);
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::from("\n")
            } else {
                format!("{lead}{line}\n")
            }
        })
        .collect()
}

fn version_status_label(status: VersionStatus) -> &'static str {
    match status {
        VersionStatus::Verified => "verified",
        VersionStatus::Unstable => "unstable",
        VersionStatus::Deprecated => "deprecated",
        VersionStatus::Yanked => "yanked",
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
