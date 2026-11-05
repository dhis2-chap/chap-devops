//! The details overlay: everything known about the model or component under
//! the cursor.

use super::strip::compose_of;
use super::text::{
    LABEL_WIDTH, centered, channel_label, field, fit, horizon, list_or_dash, version_and_tag,
    version_status, with_kept_tail, wrap, wrapped_field,
};
use crate::components::Component;
use crate::registry::Version;
use crate::tui::app::{App, Page, Row};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

/// The details overlay: everything the catalogue and the project know about
/// the row under the cursor, scrolled by `j` and `k`.
pub(super) fn draw_info(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let row = match app.page {
        Page::Models => match app.selected() {
            Some(row) => Some(row),
            None => return,
        },
        Page::Components => None,
    };

    let width = area.width.saturating_sub(6).clamp(1, 86);
    // One column of padding inside the border, as the list has.
    let inner_w = width.saturating_sub(3) as usize;
    let body = match row {
        Some(row) => info_lines(app, row, inner_w, theme),
        None => component_info_lines(app, inner_w, theme),
    };
    let lines: Vec<Line> = body
        .into_iter()
        .map(|line| {
            let mut spans = vec![Span::raw(" ")];
            spans.extend(line.spans);
            Line::from(spans)
        })
        .collect();
    // As tall as it needs to be, with the list still visible around it; what
    // does not fit is what `j` and `k` scroll.
    let height = (lines.len() as u16 + 2)
        .min(area.height.saturating_sub(6))
        .max(3.min(area.height));
    let popup = centered(area, width, height);
    let inner_h = popup.height.saturating_sub(2) as usize;
    app.info_max.set(lines.len().saturating_sub(inner_h));
    let scroll = app.info_scroll.min(app.info_max.get()) as u16;

    let title_w = popup.width.saturating_sub(2) as usize;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.accent_style())
        .title(match row {
            Some(row) => info_title(app, row, title_w, theme),
            None => component_info_title(app, title_w, theme),
        });

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(block).scroll((scroll, 0)),
        popup,
    );
}

/// The overlay's title: what the model is called, what `models.yaml` calls
/// it, and whether this deployment runs it.
///
/// The state is right-aligned and always fits; the id gives way first,
/// because the display name in front of it already says which model this is.
/// The assessed status is not up here - it has a row of its own.
pub(super) fn info_title<'a>(app: &App, row: &Row, width: usize, theme: &Theme) -> Line<'a> {
    let model = app.model(row);
    let (state, style) = match app.recorded(row) {
        Some(recorded) => (
            match recorded.host_port {
                Some(port) => format!("enabled on port {port}"),
                None => "enabled, via chap-core".to_string(),
            },
            theme.ok_style(),
        ),
        None => ("not enabled here".to_string(), theme.dim_style()),
    };
    let head = vec![
        Span::raw(" "),
        Span::styled(model.display_name.clone(), theme.accent_style()),
        Span::raw("  "),
        Span::styled(model.id.clone(), theme.dim_style()),
    ];
    Line::from(with_kept_tail(
        head,
        Span::styled(format!("{state} "), style),
        width,
    ))
}

/// The component overlay's title: the component, and whether this deployment
/// has it.
///
/// What the deployment has, not what the session wants: the model overlay's
/// title reads off the project the same way, and the `state` row inside is
/// where a pending change is said.
fn component_info_title<'a>(app: &App, width: usize, theme: &Theme) -> Line<'a> {
    let component = app.selected_component();
    let enabled = app.initial_components.is_enabled(component);
    let (state, style) = match enabled {
        true => ("enabled here".to_string(), theme.ok_style()),
        false => ("not enabled here".to_string(), theme.dim_style()),
    };
    let head = vec![
        Span::raw(" "),
        Span::styled(component.name().to_string(), theme.accent_style()),
    ];
    Line::from(with_kept_tail(
        head,
        Span::styled(format!("{state} "), style),
        width,
    ))
}

/// Everything the browser knows about a component: what it is, what `sync`
/// renders for it, where its data lives, and - for a component with a config
/// file of its own - that file plus the settings this page deliberately leaves
/// to `.chap-devops/`.
fn component_info_lines<'a>(app: &App, width: usize, theme: &Theme) -> Vec<Line<'a>> {
    let component = app.selected_component();
    let enabled = app.components.is_enabled(component);
    let mut lines: Vec<Line> = Vec::new();

    for line in wrap(component.summary(), width) {
        lines.push(Line::raw(line));
    }
    lines.push(Line::raw(""));

    lines.push(Line::from(vec![
        Span::styled(fit("state", LABEL_WIDTH), theme.label_style()),
        Span::styled(
            match (app.initial_components.is_enabled(component), enabled) {
                (false, true) => "off · this session would add it".to_string(),
                (true, false) => "enabled · this session would remove it".to_string(),
                (_, true) => "enabled".to_string(),
                (_, false) => "off".to_string(),
            },
            match enabled {
                true => theme.ok_style(),
                false => theme.dim_style(),
            },
        ),
    ]));
    lines.push(field(theme, "reach", &app.component_reach(component)));
    lines.extend(wrapped_field(
        theme,
        "compose",
        &format!(
            "{} · rendered from .chap-devops/{} by `chap-devops sync`",
            compose_of(component),
            crate::components::COMPONENTS_FILE
        ),
        width,
    ));
    lines.extend(wrapped_field(
        theme,
        "volume",
        &volume_field(component.volumes()),
        width,
    ));
    if component == Component::Ocs {
        lines.extend(wrapped_field(
            theme,
            "config",
            &format!(
                "{}/{} · yours to edit; chap-devops changes only its read_only and plugins_dir keys",
                crate::components::OCS_DIR,
                crate::components::OCS_CONFIG_FILE
            ),
            width,
        ));
        lines.push(Line::raw(""));
        // The two OCS settings the browser has no dialog for, named here so
        // this overlay is where someone learns they exist.
        lines.extend(not_on_this_page(
            theme,
            width,
            &[
                (
                    "`chap-devops components enable ocs --base-url URL`",
                    "the public origin OCS builds its STAC and openEO links from",
                ),
                (
                    "`chap-devops components enable ocs --read-only`",
                    "refuse ingestion over HTTP (--read-write allows it again)",
                ),
            ],
        ));
    }
    if component == Component::Dhis2 {
        lines.extend(wrapped_field(
            theme,
            "config",
            &format!(
                "{}/{} · yours to edit, and DHIS2 will not start without it",
                crate::components::DHIS2_DIR,
                crate::components::DHIS2_CONFIG_FILE
            ),
            width,
        ));
        lines.extend(wrapped_field(
            theme,
            "seed",
            &dhis2_seed_field(&app.components),
            width,
        ));
        // The `volume` field above names all three, so this says which of them
        // is not data: the one an operator would otherwise try to keep.
        lines.extend(wrapped_field(
            theme,
            "cache",
            &format!(
                "{} holds the downloaded dump rather than data; the dhis2-dump one-shot \
                 fetches it again when the volume is empty",
                crate::compose::render::DHIS2_DUMP_VOLUME
            ),
            width,
        ));
        lines.extend(wrapped_field(
            theme,
            "first start",
            "minutes, not seconds: DHIS2 migrates its schema on the way up, and \
             `chap-devops logs dhis2` is where that shows",
            width,
        ));
        lines.push(Line::raw(""));
        // The seed moves with `chap-devops components enable dhis2 --seed`. The
        // image tag moves with `v` on this row or `chap-devops components enable
        // dhis2 --tag`.
        lines.extend(not_on_this_page(
            theme,
            width,
            &[
                (
                    "`chap-devops components enable dhis2 --seed SPEC`",
                    "the dump a database being created is restored from: default, none, \
                     a URL, or a path in the deployment directory",
                ),
                (
                    "`v` on this row, or `chap-devops components enable dhis2 --tag TAG`",
                    &format!(
                        "the DHIS2 version, {} here; it migrates a schema forward only, \
                         so run `chap-devops backup create` first",
                        app.components.dhis2.image_tag
                    ),
                ),
            ],
        ));
    }
    if component == Component::ChapCore {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "its host port is the API port, which lives in .chap-devops/project.yaml",
            theme.dim_style(),
        )));
    }
    lines
}

/// The closing block of a component's overlay: the settings the browser has no
/// dialog for, each with what it does.
///
/// Named rather than left out, so this overlay is where someone learns they
/// exist. Each `how` carries its own backticks, because one component's are
/// whole commands and another's are a key in `.chap-devops/components.yaml` followed
/// by a sync, and the reader has to see which of the two they are looking at.
fn not_on_this_page<'a>(theme: &Theme, width: usize, settings: &[(&str, &str)]) -> Vec<Line<'a>> {
    let mut lines = vec![Line::from(Span::styled(
        "not on this page:",
        theme.label_style(),
    ))];
    for (how, what) in settings {
        lines.push(Line::from(Span::styled(
            format!("  {how}"),
            theme.accent_style(),
        )));
        for chunk in wrap(what, width.saturating_sub(4).max(1)) {
            lines.push(Line::from(Span::styled(
                format!("    {chunk}"),
                theme.dim_style(),
            )));
        }
    }
    lines
}

/// The `seed` field of the DHIS2 overlay: what the first start restores, and
/// that a restore only ever happens to a database being created.
///
/// Four answers rather than the three `.chap-devops/components.yaml` records, because
/// a `default` on a minor line chap-devops publishes no dump for starts empty as well,
/// and for a reason worth saying: it otherwise looks exactly like `none`, and
/// nothing else on this page would explain it.
pub(super) fn dhis2_seed_field(components: &crate::components::Components) -> String {
    let volume = crate::compose::render::DHIS2_DB_VOLUME;
    if components.dhis2_seed_is_unknown() {
        return format!(
            "default · chap-devops knows no dump for {}, so {volume} starts empty",
            crate::components::dhis2_minor(&components.dhis2.image_tag)
        );
    }
    match components.dhis2_seed_source() {
        Some(source) => {
            format!(
                "{source} · restored once, into the database the first `chap-devops up` creates"
            )
        }
        None => format!("none · {volume} starts empty and DHIS2 migrates a new database into it"),
    }
}

/// The `volume` field of the component overlay: every volume the component
/// keeps, or what to run for a component that keeps none of its own.
///
/// All of them on the one field, comma separated, since the field is what the
/// overlay exists for - saying where a component's data lives - and a component
/// with two volumes has it in two places.
pub(super) fn volume_field(volumes: &[&str]) -> String {
    match volumes {
        [] => {
            "none of its own · `chap-devops down --volumes` removes this deployment's".to_string()
        }
        volumes => format!(
            "{} · kept when the component is disabled",
            volumes.join(", ")
        ),
    }
}

/// Every field the overlay lists, in the order it lists them.
pub(super) fn info_lines<'a>(app: &App, row: &Row, width: usize, theme: &Theme) -> Vec<Line<'a>> {
    let model = app.model(row);
    let mut lines: Vec<Line> = Vec::new();

    // What the model does comes first: everything under it is detail about a
    // model the reader has already decided to look at.
    for line in wrap(&model.summary, width) {
        lines.push(Line::raw(line));
    }
    lines.push(Line::raw(""));

    // What this row is pinned to, how far that pin is trusted, and which
    // channels point at it: one fact, so one row.
    let pinned = match app.recorded(row) {
        Some(recorded) => Some((
            version_and_tag(&recorded.version, &recorded.image_tag),
            recorded.version.clone(),
        )),
        None => app
            .resolved(row)
            .map(|v| (version_and_tag(&v.version, &v.image_tag), v.version.clone())),
    };
    match pinned {
        Some((shown, version)) => {
            let entry = model.versions.iter().find(|v| v.version == version);
            lines.push(version_line(model, "version", &shown, entry, theme));
            // The rest of the published history, when there is any.
            if model.versions.len() > 1 {
                for other in model.versions.iter().filter(|v| v.version != version) {
                    lines.push(version_line(
                        model,
                        "",
                        &version_and_tag(&other.version, &other.image_tag),
                        Some(other),
                        theme,
                    ));
                }
            }
        }
        None => lines.push(field(
            theme,
            "version",
            &match row.channel {
                Some(channel) => {
                    format!(
                        "unresolved: channel {} has no version",
                        channel_label(channel)
                    )
                }
                None => "unresolved: the pinned version is not in the registry".to_string(),
            },
        )),
    }

    let mut author = model.attribution.author.clone();
    if let Some(organization) = &model.attribution.organization {
        author.push_str(&format!(" · {organization}"));
    }
    if let Some(contact) = &model.attribution.contact {
        author.push_str(&format!(" · {contact}"));
    }
    lines.extend(wrapped_field(theme, "author", &author, width));

    // The assessment wraps rather than being cut: half of "not intended for
    // use" is worse than no sentence at all.
    let assessment = format!(
        "{}, {}",
        model.assessed_status.colour(),
        model.assessed_status.describe()
    );
    let room = width.saturating_sub(LABEL_WIDTH + 2).max(1);
    for (i, chunk) in wrap(&assessment, room).into_iter().enumerate() {
        lines.push(Line::from(vec![
            Span::styled(
                fit(if i == 0 { "status" } else { "" }, LABEL_WIDTH),
                theme.label_style(),
            ),
            match i {
                0 => Span::styled("● ", theme.status_style(model.assessed_status)),
                _ => Span::raw("  "),
            },
            Span::raw(chunk),
        ]));
    }

    lines.push(field(
        theme,
        "period",
        &format!(
            "{} · {}",
            model.compatibility.period_types.join(", "),
            horizon(app, row)
        ),
    ));
    // The marketplace schema carries no target: chap-core forecasts disease
    // cases for every model in the catalogue, so that is what this says.
    lines.push(field(theme, "target", "disease cases"));

    let mut covariates = format!(
        "{} · defaults {}",
        list_or_dash(&model.covariates.required),
        list_or_dash(&model.covariates.defaults)
    );
    if model.covariates.allow_free_additional {
        covariates.push_str(" · free extras allowed");
    }
    if model.compatibility.requires_geo {
        covariates.push_str(" · geometry required");
    }
    lines.extend(wrapped_field(theme, "covariates", &covariates, width));

    // Everything below is about this deployment rather than the model.
    lines.push(Line::raw(""));

    // A starter is not a forecasting model, and the overlay says so in a
    // colour that is neither "good" nor "bad", just different.
    if model.is_starter() {
        lines.push(Line::from(vec![
            Span::styled(fit("kind", LABEL_WIDTH), theme.label_style()),
            Span::styled(
                "starter (scaffolding, not a forecasting model)",
                theme.starter_style(),
            ),
        ]));
    } else if model.manual {
        lines.push(Line::from(vec![
            Span::styled(fit("kind", LABEL_WIDTH), theme.label_style()),
            Span::styled(
                "manual (added with `chap-devops models add`)",
                theme.dim_style(),
            ),
        ]));
    }

    let reach = match app.recorded(row).and_then(|r| r.host_port) {
        Some(port) => format!("http://localhost:{port}"),
        None => "via chap-core (through chap-core's /run/ proxy)".to_string(),
    };
    lines.push(field(theme, "reach", &reach));

    let image = match app.resolved(row) {
        Some(version) => model.image_ref(version),
        None => model.source.image.clone(),
    };
    lines.extend(wrapped_field(theme, "image", &image, width));

    let runtime = if model.needs_amd64() {
        format!("{} (amd64 only)", model.source.runtime_image)
    } else {
        model.source.runtime_image.clone()
    };
    lines.extend(wrapped_field(theme, "runtime", &runtime, width));

    if let Some(recorded) = app.recorded(row) {
        lines.push(field(theme, "data dir", &recorded.data_dir));
        lines.push(field(
            theme,
            "user",
            &format!("{} ({})", recorded.user, recorded.user_from.label()),
        ));
    }

    lines.push(Line::raw(""));

    if !model.maintainers.is_empty() {
        lines.extend(wrapped_field(
            theme,
            "maintainers",
            &model.maintainers.join(", "),
            width,
        ));
    }
    lines.push(Line::from(vec![
        Span::styled(fit("repository", LABEL_WIDTH), theme.label_style()),
        Span::styled(model.source.repository.clone(), theme.accent_style()),
    ]));
    if let Some(citation) = &model.attribution.citation {
        lines.extend(wrapped_field(theme, "citation", citation, width));
    }
    lines
}

/// One published version: what it is, how far it is trusted, and the channels
/// pointing at it.
fn version_line<'a>(
    model: &crate::registry::Model,
    label: &str,
    shown: &str,
    entry: Option<&Version>,
    theme: &Theme,
) -> Line<'a> {
    let mut spans = vec![
        Span::styled(fit(label, LABEL_WIDTH), theme.label_style()),
        Span::raw(shown.to_string()),
    ];
    let Some(entry) = entry else {
        return Line::from(spans);
    };
    spans.push(Span::styled(" · ", theme.dim_style()));
    spans.push(Span::styled(
        version_status(entry),
        theme.version_style(entry.status),
    ));
    let mut channels: Vec<&str> = Vec::new();
    if entry.version == model.channels.stable {
        channels.push("stable");
    }
    if entry.version == model.channels.latest {
        channels.push("latest");
    }
    if !channels.is_empty() {
        spans.push(Span::styled(" · channels ", theme.dim_style()));
        spans.push(Span::styled(channels.join(", "), theme.accent_style()));
    }
    Line::from(spans)
}
