use super::*;
use super::{info::*, strip::*, table::*, text::*};
use crate::project::ProjectState;
use crate::registry::{Channel, Registry, VersionStatus, load_embedded};
use crate::tui::app::Action;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

mod components;

const EWARS: &str = "chapkit_ewars_model";

fn registry() -> Registry {
    load_embedded().expect("embedded snapshot parses")
}

fn theme() -> Theme {
    Theme::default()
}

fn render(app: &App, width: u16, height: u16) -> String {
    render_with(app, width, height, &theme())
}

fn render_with(app: &App, width: u16, height: u16, theme: &Theme) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend starts");
    terminal
        .draw(|frame| draw(frame, app, theme))
        .expect("draw");
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| {
                    buffer
                        .cell((x, y))
                        .map(|c| c.symbol().to_string())
                        .unwrap_or_default()
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The line of the rendered screen that contains `needle`.
fn line_with<'a>(screen: &'a str, needle: &str) -> &'a str {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line holds {needle}:\n{screen}"))
}

/// Which column of a rendered line `needle` starts in. Counted in
/// characters, because `│` and `▸` are three bytes each and byte offsets
/// would make two columns that line up look as if they did not.
fn column_of(line: &str, needle: &str) -> usize {
    let byte = line
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} is not on {line}"));
    line[..byte].chars().count()
}

fn line_text(line: &Line) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[test]
fn a_normal_terminal_shows_the_title_the_table_and_the_summary() {
    let registry = registry();
    let app = App::new(&registry, &ProjectState::default());
    let screen = render(&app, 120, 40);
    assert!(screen.contains("chap-devops · models"));
    assert!(screen.contains("registry: embedded"));
    assert!(screen.contains(&format!(
        "{} models · 0 enabled · 0 pending",
        registry.models.len()
    )));
    assert!(screen.contains("Marketplace"));
    assert!(
        !screen.contains("Details"),
        "the details pane is an overlay now, not a second column"
    );
    // Rounded corners, not square ones.
    assert!(screen.contains('╭') && screen.contains('╯'), "{screen}");
    assert!(screen.contains("MODEL") && screen.contains("PORT"));
    assert!(screen.contains("CHAP-EWARS"));
    assert!(screen.contains("chapkit_ewars_model"));
    assert!(
        screen.contains("● limited data"),
        "the dot carries the colour, the word its meaning:\n{screen}"
    );
    assert!(
        !screen.contains("● orange"),
        "a bare colour word says nothing:\n{screen}"
    );
    assert!(screen.contains("[space] toggle"), "{screen}");
    // The summary strip stands in for the pane that used to be there.
    assert!(screen.contains("i for details"), "{screen}");
    assert!(screen.contains("requires population"), "{screen}");
    assert!(
        !screen.contains("Bayesian hierarchical"),
        "the summary text is behind `i`, not on the strip:\n{screen}"
    );
}

/// The headings mean nothing if they sit over the wrong columns.
#[test]
fn the_column_headings_line_up_with_the_rows() {
    let registry = registry();
    let app = App::new(&registry, &ProjectState::default());
    let screen = render(&app, 120, 40);
    let head = line_with(&screen, "MODEL");
    let row = line_with(&screen, "CHAP-EWARS  ");

    assert_eq!(column_of(head, "MODEL"), column_of(row, "CHAP-EWARS"));
    assert_eq!(column_of(head, "ID"), column_of(row, "chapkit_ewars_model"));
    assert_eq!(column_of(head, "STATUS"), column_of(row, "●"));
    assert_eq!(column_of(head, "VERSION"), column_of(row, "1.0.4"));
    // And the version has lost the `v` the old list put in front of it.
    assert!(!row.contains("v1.0.4"), "{row}");

    let enabled = state_with_ewars(&registry, Some(5001));
    let app = App::new(&registry, &enabled);
    let screen = render(&app, 120, 40);
    let head = line_with(&screen, "MODEL");
    let row = line_with(&screen, "✓ CHAP-EWARS");
    assert_eq!(column_of(head, "PORT"), column_of(row, "5001"));
}

/// A project state with `chapkit_ewars_model` enabled on `host_port`.
fn state_with_ewars(registry: &Registry, host_port: Option<u16>) -> ProjectState {
    let mut state = ProjectState::default();
    let model = registry.get(EWARS).unwrap();
    state.models.insert(
        model.id.clone(),
        crate::project::EnabledModel {
            previous: None,
            reads_port: false,
            service_id: model.service_id.clone(),
            image: model.source.image.clone(),
            image_tag: model
                .version(&model.channels.stable)
                .expect("stable resolves")
                .image_tag
                .clone(),
            image_digest: None,
            version: model.channels.stable.clone(),
            channel: Some(Channel::Stable),
            host_port,
            bind: None,
            data_dir: "/app/data".into(),
            user: "chapkit:chapkit".into(),
            user_from: Default::default(),
            platform: Some("linux/amd64".into()),
            multi_arch: false,
            compose_file: "compose.chapkit-ewars-model.yml".into(),
        },
    );
    state
}

#[test]
fn enabled_rows_are_ticked_and_show_their_port() {
    let registry = registry();
    let state = state_with_ewars(&registry, Some(5001));
    let app = App::new(&registry, &state);
    let screen = render(&app, 120, 40);
    assert!(screen.contains('✓'), "{screen}");
    assert!(screen.contains("5001"));
    // The strip says the same thing in words.
    assert!(screen.contains("enabled on port 5001"), "{screen}");
    assert!(
        !screen.contains("user chapkit:chapkit"),
        "the service user is behind `i`, not on the strip:\n{screen}"
    );
}

/// The port column is about how the model is reached, not about whether
/// it is enabled - the tick already says that.
#[test]
fn an_enabled_model_with_no_host_port_is_reached_through_chap_core() {
    let registry = registry();
    let state = state_with_ewars(&registry, None);
    let mut app = App::new(&registry, &state);
    let screen = render(&app, 120, 40);
    assert!(screen.contains('✓'));
    assert!(screen.contains("via chap-core"), "{screen}");
    assert!(
        screen.contains("enabled, via chap-core"),
        "and so does the strip"
    );
    assert!(!screen.contains("5001"), "no host port is published");
    assert!(!screen.contains("internal"), "{screen}");

    // The dialog is visible before saving, and so is what it took: the
    // port itself is only picked when the selection is applied.
    app.reduce(Action::PortPrompt);
    let prompt = render(&app, 120, 40);
    assert!(prompt.contains("Host port for CHAP-EWARS"), "{prompt}");
    assert!(prompt.contains("port  › auto_"), "{prompt}");
    assert!(prompt.contains("[enter] apply"), "{prompt}");
    app.reduce(Action::PortApply);
    let screen = render(&app, 120, 40);
    assert!(screen.contains("auto"), "{screen}");
}

#[test]
fn a_pending_change_is_marked_before_it_is_saved() {
    let registry = registry();
    let state = state_with_ewars(&registry, Some(5001));
    let mut app = App::new(&registry, &state);
    let columns = columns(118, false);

    // The first row is the one the project already runs; turning it off is
    // a removal, not an absence.
    app.reduce(Action::Toggle);
    let row = app.selected().expect("a row is selected");
    let line = row_line(&app, row, &columns, true, &theme());
    assert_eq!(line.spans[1].content, "- ");
    assert_eq!(line.spans[1].style, theme().bad_style());
    assert!(line_text(&line).contains("will be disabled"), "{line:?}");

    // And back on again is no change at all.
    app.reduce(Action::Toggle);
    let row = app.selected().expect("a row is selected");
    let line = row_line(&app, row, &columns, true, &theme());
    assert_eq!(line.spans[1].content, "✓ ");
    assert!(
        line_text(&line).contains("via chap-core"),
        "a row toggled off and on again comes back without its port"
    );

    // A row the project never had reads as an addition.
    let mut fresh = App::new(&registry, &ProjectState::default());
    fresh.reduce(Action::Toggle);
    let row = fresh.selected().expect("a row is selected");
    let line = row_line(&fresh, row, &columns, true, &theme());
    assert_eq!(line.spans[1].content, "+ ");
    assert_eq!(line.spans[1].style, theme().ok_style());
    assert!(line_text(&line).contains("will be enabled"));
}

#[test]
fn a_row_nobody_enabled_carries_no_mark_and_says_nothing_about_ports() {
    let registry = registry();
    let app = App::new(&registry, &ProjectState::default());
    let row = app.selected().expect("a row is selected");
    let line = row_line(&app, row, &columns(118, false), false, &theme());
    assert_eq!(
        line.spans[0].content, "   ",
        "no cursor on an unselected row"
    );
    assert_eq!(line.spans[1].content, "  ");
    let text = line_text(&line);
    assert!(!text.contains("internal"), "{text}");
    assert!(!text.contains(':'), "{text}");
}

#[test]
fn the_status_column_carries_the_assessment_colour() {
    let registry = registry();
    let app = App::new(&registry, &ProjectState::default());
    let row = app.selected().expect("a row is selected");
    let line = row_line(&app, row, &columns(118, false), true, &theme());
    let dot = line
        .spans
        .iter()
        .find(|s| s.content.starts_with('●'))
        .expect("the status column is there");
    assert_eq!(
        dot.style,
        theme().status_style(app.model(row).assessed_status)
    );
    assert_eq!(dot.style.fg, Some(theme().warn));
    assert!(line_text(&line).contains("● limited data"));
}

/// The strip under the table is what the details pane used to be, boiled
/// down to the one line that says whether this model is worth opening.
#[test]
fn the_summary_strip_describes_the_row_under_the_cursor() {
    let registry = registry();
    let app = App::new(&registry, &ProjectState::default());
    let lines = summary_lines(&app, 118, &theme());
    assert_eq!(lines.len(), 1, "one line, not three");

    let head = line_text(&lines[0]);
    assert!(head.starts_with("CHAP-EWARS"), "{head}");
    assert!(head.contains("● limited data"), "{head}");
    assert!(head.contains("1.0.4 (sha-"), "{head}");
    assert!(head.contains("not enabled"), "{head}");
    assert!(head.contains("requires population"), "{head}");
    assert!(head.ends_with("i for details"), "{head}");
    assert_eq!(head.chars().count(), 118, "the hint is right-aligned");

    // What the strip dropped is in the overlay, not on the line.
    for gone in ["Bayesian hierarchical", "defaults", "horizon", "user "] {
        assert!(!head.contains(gone), "{gone} is still on the strip: {head}");
    }

    // Too narrow for the covariates: they are cut, the hint stays.
    let narrow = line_text(&summary_lines(&app, 70, &theme())[0]);
    assert!(narrow.starts_with("CHAP-EWARS"), "{narrow}");
    assert!(
        narrow.ends_with("i for details"),
        "the hint always fits: {narrow}"
    );
    assert!(narrow.contains('~'), "the cut is marked: {narrow}");
    assert_eq!(narrow.chars().count(), 70);
}

/// With something unsaved, the strip stops describing a model and starts
/// listing what saving would write.
#[test]
fn pending_changes_replace_the_summary_strip() {
    let registry = registry();
    let state = state_with_ewars(&registry, None);
    let mut app = App::new(&registry, &state);
    app.reduce(Action::Toggle);
    app.reduce(Action::Down);
    app.reduce(Action::Toggle);

    let screen = render(&app, 120, 40);
    assert!(screen.contains("2 pending changes"), "{screen}");
    assert!(
        screen.contains("nothing is written until you save"),
        "{screen}"
    );
    let removed = line_with(&screen, "the data volume");
    assert!(removed.contains("- CHAP-EWARS"), "{removed}");
    assert!(removed.contains("the data volume is kept"), "{removed}");
    let added = line_with(&screen, "enable at");
    assert!(added.contains("+ "), "{added}");
    assert!(added.contains("via chap-core"), "{added}");
    assert!(!screen.contains("i for details"), "{screen}");
    // And the header counts them, in the colour that says "unsaved".
    assert!(screen.contains("2 pending"), "{screen}");
}

/// A port change is not an enable, and the strip says which it is.
#[test]
fn a_changed_port_is_listed_as_an_update() {
    let registry = registry();
    let state = state_with_ewars(&registry, None);
    let mut app = App::new(&registry, &state);
    app.reduce(Action::PortPrompt);
    app.port_input.clear();
    for c in "5010".chars() {
        app.reduce(Action::PortChar(c));
    }
    app.reduce(Action::PortApply);
    let screen = render(&app, 120, 40);
    assert!(screen.contains("1 pending change "), "{screen}");
    let line = line_with(&screen, "publish port 5010");
    assert!(line.contains("+ CHAP-EWARS"), "{line}");
    assert!(screen.contains("5010"), "the column shows it too");
}

/// The port dialog: what is being typed, what the row has now, and - when
/// there is one - the reason the last entry was refused, all inside the
/// box, with only the way out on the bar underneath.
#[test]
fn the_port_dialog_draws_the_state_and_what_it_refused() {
    let registry = registry();
    let state = state_with_ewars(&registry, None);
    let mut app = App::new(&registry, &state);
    app.reduce(Action::PortPrompt);
    let screen = render(&app, 120, 40);
    assert!(screen.contains("Host port for CHAP-EWARS"), "{screen}");
    assert!(screen.contains("port  › auto_"), "{screen}");
    assert!(
        screen.contains("now: via chap-core · range 5001-5999 · api port 8700 is taken"),
        "{screen}"
    );
    assert!(
        screen.contains("[enter] apply  [esc] cancel  [auto] any free port  [none] no host port"),
        "the dialog carries its own keys:\n{screen}"
    );
    let bar = screen.lines().last().expect("a key bar");
    assert!(bar.trim() == "[esc] cancel", "{bar}");
    // Rounded like the other overlays, and centred.
    assert!(screen.contains('╭') && screen.contains('╯'), "{screen}");

    // A refusal keeps the box up with the reason inside it.
    app.port_input.clear();
    for c in "80".chars() {
        app.reduce(Action::PortChar(c));
    }
    app.reduce(Action::PortApply);
    let screen = render(&app, 120, 40);
    assert!(screen.contains("port  › 80_"), "{screen}");
    assert!(
        screen.contains("port 80 is outside this deployment's range 5001-5999"),
        "{screen}"
    );
    assert!(screen.contains("Host port for CHAP-EWARS"), "still open");

    // Cancelling leaves nothing behind.
    app.reduce(Action::FilterCancel);
    let screen = render(&app, 120, 40);
    assert!(!screen.contains("Host port for"), "{screen}");
    assert!(
        !screen.contains("outside this deployment's range"),
        "{screen}"
    );
}

/// The channel dialog: the two channels, what each resolves to, and which
/// one the row follows today.
#[test]
fn the_channel_dialog_lists_both_channels_and_marks_the_one_in_force() {
    let registry = registry();
    let state = state_with_ewars(&registry, None);
    let mut app = App::new(&registry, &state);
    app.reduce(Action::ChannelPrompt);
    let screen = render(&app, 120, 40);
    assert!(screen.contains("Channel for CHAP-EWARS"), "{screen}");
    let stable = line_with(&screen, "stable");
    assert!(stable.contains("▸ ✓ stable"), "{stable}");
    assert!(stable.contains("1.0.4"), "the version it resolves to");
    let latest = line_with(&screen, "latest");
    assert!(latest.contains("latest"), "{latest}");
    assert!(!latest.contains('▸'), "{latest}");
    assert!(
        screen.contains("[enter] apply  [esc] cancel  [j/k] move  [s/l] pick one"),
        "{screen}"
    );
    assert_eq!(
        screen.lines().last().expect("a key bar").trim(),
        "[esc] cancel"
    );

    // `l` moves the cursor, Enter takes it, and the list says so.
    app.reduce(Action::ChannelChar('l'));
    let screen = render(&app, 120, 40);
    assert!(line_with(&screen, "latest").contains("▸"), "{screen}");
    app.reduce(Action::ChannelApply);
    let screen = render(&app, 120, 40);
    assert!(!screen.contains("Channel for"), "{screen}");
    assert!(
        screen.contains("follow latest"),
        "the strip says what moved"
    );
}

/// A model pinned with `--version` follows neither channel: the table shows
/// the pin, and the dialog ticks `pinned`, not `stable`.
#[test]
fn a_pinned_row_shows_its_pin_and_the_dialog_ticks_no_channel() {
    let registry = registry();
    let mut state = state_with_ewars(&registry, None);
    let model = registry.get(EWARS).unwrap();
    let older = model
        .versions
        .iter()
        .find(|v| v.version != model.channels.stable)
        .expect("ewars has an older version");
    let entry = state.models.get_mut(EWARS).unwrap();
    entry.channel = None;
    entry.version = older.version.clone();
    entry.image_tag = older.image_tag.clone();
    let mut app = App::new(&registry, &state);
    let screen = render(&app, 120, 40);
    let row = line_with(&screen, "CHAP-EWARS");
    assert!(row.contains(&older.version), "{row}");
    assert!(!row.contains(&model.channels.stable), "{row}");

    app.reduce(Action::ChannelPrompt);
    let screen = render(&app, 120, 40);
    assert!(!line_with(&screen, "stable").contains('✓'), "{screen}");
    assert!(!line_with(&screen, "latest").contains('✓'), "{screen}");
    let pinned = line_with(&screen, "pinned");
    assert!(
        pinned.contains('✓') && pinned.contains(&older.version),
        "{pinned}"
    );
}

#[test]
fn the_details_overlay_opens_closes_and_scrolls() {
    let registry = registry();
    let state = state_with_ewars(&registry, Some(5001));
    let mut app = App::new(&registry, &state);
    assert!(!render(&app, 120, 40).contains("data dir"));

    app.reduce(Action::Info);
    let screen = render(&app, 120, 40);
    // Everything the old pane held is here.
    for needle in [
        // The colour word is only worth printing next to what it means.
        "● orange, shows promise on limited data",
        "http://localhost:5001",
        "1.0.4 (sha-964eea8) · verified · channels stable, latest",
        "ghcr.io/chap-models/chapkit_ewars_model:sha-",
        "amd64 only",
        "/app/data",
        "chapkit:chapkit",
        "Bayesian hierarchical",
        "covariates",
        "period      monthly, weekly · horizon 0 to 100 periods",
        "target      disease cases",
        "maintainers",
        "author      ",
        "citation    ",
        "github.com/chap-models/chapkit_ewars_model",
        // The title carries the state, in the words the strip uses.
        "enabled on port 5001",
    ] {
        assert!(screen.contains(needle), "{needle} is missing:\n{screen}");
    }
    assert!(screen.contains("[esc] close"), "{screen}");
    assert!(screen.contains("[o] open repository"), "{screen}");
    assert!(screen.contains("[c] image ref"), "{screen}");

    // A short terminal cannot hold it all, so j and k move it.
    let short = render(&app, 80, 24);
    assert!(short.contains("Bayesian hierarchical"), "{short}");
    assert!(app.info_max.get() > 0, "the overlay has more to show");
    app.reduce(Action::Down);
    app.reduce(Action::Down);
    assert_eq!(app.info_scroll, 2);
    let scrolled = render(&app, 80, 24);
    assert_ne!(scrolled, short, "j scrolled the overlay");
    assert!(
        !scrolled.contains("Bayesian hierarchical"),
        "the top scrolled away:\n{scrolled}"
    );

    // It never scrolls past the end, and it comes back.
    for _ in 0..50 {
        app.reduce(Action::Down);
    }
    render(&app, 80, 24);
    assert_eq!(app.info_scroll, app.info_max.get());
    app.reduce(Action::Top);
    assert_eq!(app.info_scroll, 0);

    app.reduce(Action::Info);
    assert_eq!(app.mode, Mode::Browse);
    assert!(!render(&app, 120, 40).contains("data dir"));
}

/// The title is the one line that can collide: three things competing
/// for the top border. The state is the one that has to survive.
#[test]
fn the_overlay_title_keeps_the_state_and_cuts_the_id() {
    let registry = registry();
    let state = state_with_ewars(&registry, None);
    let mut app = App::new(&registry, &state);
    app.reduce(Action::Info);

    let screen = render(&app, 120, 40);
    let wide = line_with(&screen, "CHAP-EWARS  chapkit_ewars_model");
    assert!(wide.contains("enabled, via chap-core"), "{wide}");
    assert!(
        !wide.contains("● limited data"),
        "the assessment has a row of its own: {wide}"
    );

    // The id gives way, the state does not, and they never touch.
    let title = line_text(&info_title(&app, app.selected().unwrap(), 50, &theme()));
    assert_eq!(title.chars().count(), 50);
    assert!(title.starts_with(" CHAP-EWARS"), "{title}");
    assert!(title.ends_with("enabled, via chap-core "), "{title}");
    assert!(title.contains('~'), "the id is cut: {title}");
    assert!(
        title.contains("  enabled"),
        "two columns of daylight, not a collision: {title}"
    );
}

/// The order is the one the Chap Modeling App uses: what it does, what
/// it is, who wrote it, then how this deployment runs it.
#[test]
fn the_overlay_leads_with_what_the_model_does() {
    let registry = registry();
    let state = state_with_ewars(&registry, Some(5001));
    let app = App::new(&registry, &state);
    let row = app.selected().expect("a row");
    let lines: Vec<String> = info_lines(&app, row, 83, &theme())
        .iter()
        .map(line_text)
        .collect();

    assert!(lines[0].starts_with("Bayesian hierarchical"), "{lines:?}");
    let at = |label: &str| {
        lines
            .iter()
            .position(|l| l.starts_with(label))
            .unwrap_or_else(|| panic!("no {label} row in {lines:?}"))
    };
    let order = [
        "version",
        "author",
        "status",
        "period",
        "target",
        "covariates",
        "reach",
        "image",
        "runtime",
        "data dir",
        "user",
        "maintainers",
        "repository",
        "citation",
    ];
    let mut last = 0;
    for label in order {
        let at = at(label);
        assert!(at > last, "{label} is out of order in {lines:?}");
        last = at;
    }
    // A blank line before the deployment facts, and before the credits.
    assert_eq!(lines[at("reach") - 1], "", "{lines:?}");
    assert_eq!(lines[at("maintainers") - 1], "", "{lines:?}");
    // Long values wrap under their label instead of being cut.
    assert!(!lines.iter().any(|l| l.contains('~')), "{lines:?}");
}

/// One version is the whole story on the `version` row; a model with a
/// history lists the rest of it underneath.
#[test]
fn a_model_with_more_than_one_version_lists_them_all() {
    let mut registry = registry();
    let model = registry
        .models
        .iter_mut()
        .find(|m| m.id == EWARS)
        .expect("the ewars model");
    // Only the stable release and the one added here, whatever else the
    // catalogue holds by now.
    model.versions.truncate(1);
    let mut older = model.versions[0].clone();
    older.version = "1.0.1".to_string();
    older.image_tag = "sha-0000001".to_string();
    older.status = VersionStatus::Deprecated;
    model.versions.push(older);

    let app = App::new(&registry, &ProjectState::default());
    let row = app.selected().expect("a row");
    let lines: Vec<String> = info_lines(&app, row, 83, &theme())
        .iter()
        .map(line_text)
        .collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with("version"))
        .expect("a version row");
    assert!(
        lines[at].contains("1.0.4 (sha-964eea8) · verified · channels stable, latest"),
        "{:?}",
        lines[at]
    );
    assert!(
        lines[at + 1]
            .trim()
            .starts_with("1.0.1 (sha-0000001) · deprecated"),
        "the older release follows it: {:?}",
        lines[at + 1]
    );
    assert!(
        !lines[at + 1].contains("channels"),
        "no channel points at it: {:?}",
        lines[at + 1]
    );
}

#[test]
fn the_palette_filters_and_runs_a_command() {
    let registry = registry();
    let mut app = App::new(&registry, &ProjectState::default());
    app.reduce(Action::Palette);
    let screen = render(&app, 120, 40);
    assert!(screen.contains("Commands"), "{screen}");
    assert!(screen.contains("› _"), "{screen}");
    assert!(
        screen.contains(&format!("{0} of {0} commands", app.commands().len())),
        "{screen}"
    );
    assert!(
        screen.contains("Refresh registry"),
        "the strip lists them all"
    );

    for c in "po".chars() {
        app.reduce(Action::PaletteChar(c));
    }
    let screen = render(&app, 120, 40);
    assert!(screen.contains("› po_"), "{screen}");
    assert!(
        screen.contains("Set a host port for CHAP-EWARS"),
        "{screen}"
    );
    assert!(screen.contains("Remove the host port"), "{screen}");
    assert!(
        !screen.contains("Show or hide starters"),
        "the filter narrowed the list:\n{screen}"
    );
    assert!(
        screen.contains(&format!(
            "{} of {} commands",
            app.palette_matches().len(),
            app.commands().len()
        )),
        "{screen}"
    );

    // Typing something no command holds says so rather than showing an
    // empty box.
    app.reduce(Action::PaletteChar('z'));
    assert!(render(&app, 120, 40).contains("no command matches"));
    app.reduce(Action::PaletteBackspace);

    // Enter runs what the cursor is on, and the palette closes.
    let mut app = App::new(&registry, &ProjectState::default());
    app.reduce(Action::Palette);
    for c in "enable or".chars() {
        app.reduce(Action::PaletteChar(c));
    }
    assert_eq!(app.palette_matches().len(), 1);
    app.reduce(Action::PaletteRun);
    assert_eq!(app.mode, Mode::Browse);
    assert!(app.palette_query.is_empty());
    assert!(
        app.selected().expect("a row").enabled,
        "the row was toggled"
    );
    assert!(render(&app, 120, 40).contains("will be enabled"));
}

#[test]
fn the_footer_follows_what_is_pending_and_what_is_filtered() {
    let registry = registry();
    let mut app = App::new(&registry, &ProjectState::default());
    // Wide enough for the whole bar, in the order the design gives.
    let wide = render(&app, 150, 40);
    let bar = wide.lines().last().expect("the bar is the last line");
    let order: Vec<&str> = [
        "[j/k] move",
        "[space] toggle",
        "[i] info",
        "[p] port",
        "[v] channel",
        "[t] starters",
        "[/] filter",
        "[s] save",
        "[ctrl+k] commands",
        "[?] help",
        "[q] quit",
    ]
    .into_iter()
    .collect();
    let mut at = 0;
    for entry in &order {
        let found = bar[at..]
            .find(entry)
            .unwrap_or_else(|| panic!("{entry} is missing or out of order:\n{bar}"));
        at += found + entry.len();
    }

    // At a hundred and twenty the display toggles give way first.
    let plain = render(&app, 120, 40);
    assert!(plain.contains("[j/k] move"), "{plain}");
    assert!(plain.contains("[p] port"), "{plain}");
    assert!(plain.contains("[ctrl+k] commands"), "{plain}");
    assert!(plain.contains("[s] save"), "{plain}");
    assert!(!plain.contains("[u] discard"), "{plain}");
    assert!(!plain.contains("[t] starters"), "{plain}");

    app.reduce(Action::Toggle);
    let pending = render(&app, 150, 40);
    assert!(pending.contains("[s] save 1 change "), "{pending}");
    assert!(pending.contains("[u] discard"), "{pending}");

    app.reduce(Action::StartFilter);
    for c in "arima".chars() {
        app.reduce(Action::FilterChar(c));
    }
    app.reduce(Action::FilterDone);
    let filtered = render(&app, 150, 40);
    assert!(filtered.contains("[esc] clear filter"), "{filtered}");
    assert!(!filtered.contains("[t] starters"), "{filtered}");
    assert!(!filtered.contains("[/] filter"), "{filtered}");
}

/// The save chip is filled rather than written, so it is the one thing on
/// the bar that cannot be mistaken for another key.
#[test]
fn the_save_chip_is_filled_when_there_is_something_to_save() {
    let theme = theme();
    let hints = keys::keybar(Page::Models, Mode::Browse, 2, false);
    let spans = keybar(&hints, 200, &theme);
    let chip = spans
        .iter()
        .find(|span| span.content.contains("save 2 changes"))
        .expect("the chip is on the bar");
    assert_eq!(chip.style, theme.chip_style());
    assert_eq!(chip.style.bg, Some(theme.warn));

    // And the bar gives up the least useful keys before it overflows.
    let narrow = keybar(&hints, 44, &theme);
    let text: String = narrow.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.chars().count() <= 44, "{text}");
    assert!(text.contains("save 2 changes"), "{text}");
    assert!(text.contains("move"), "{text}");
    assert!(!text.contains("channel"), "{text}");
}

#[test]
fn filter_mode_shows_what_is_being_typed_in_the_title() {
    let registry = registry();
    let mut app = App::new(&registry, &ProjectState::default());
    app.reduce(Action::StartFilter);
    for c in "ewars".chars() {
        app.reduce(Action::FilterChar(c));
    }
    let screen = render(&app, 120, 40);
    assert!(
        screen.contains("chap-devops · models · filter ewars_"),
        "{screen}"
    );
    assert!(screen.contains("CHAP-EWARS"));
    assert!(screen.contains("1 of 7 match \"ewars\""), "{screen}");
    assert!(screen.contains("[esc] clear"), "{screen}");
}

#[test]
fn starters_and_manual_entries_get_a_kind_column() {
    let registry = registry();
    let mut app = App::new(&registry, &ProjectState::default());
    let screen = render(&app, 120, 40);
    assert!(!screen.contains("KIND"), "{screen}");

    app.reduce(Action::ToggleStarters);
    let screen = render(&app, 120, 40);
    assert!(screen.contains("KIND"), "{screen}");
    assert!(screen.contains("starter"), "{screen}");
    let row = line_with(&screen, "CHAP-EWARS  ");
    let head = line_with(&screen, "MODEL");
    assert_eq!(column_of(head, "KIND"), column_of(row, "model "));
}

/// A model the deployment added itself is marked in the table and named
/// as such in the overlay, so the browser never presents a local
/// definition as a reviewed marketplace entry.
#[test]
fn a_manually_added_model_is_marked_and_named() {
    let mut state = ProjectState::default();
    // An id the marketplace does not list: what a deployment can add.
    state.manual.insert(
        "example_manual_model".to_string(),
        crate::project::ManualModel {
            reads_port: false,
            service_id: "example-manual-model".into(),
            display_name: "example_manual_model".into(),
            repository: Some("https://github.com/example/example_manual_model".into()),
            image: "ghcr.io/example/example_manual_model".into(),
            tag: "sha-b1d6c31".into(),
            commit: None,
            follow: Some("main".into()),
            data_dir: Some("/work/data".into()),
            user: Some("10001:10001".into()),
            runtime_amd64: true,
            added: "2026-09-24".into(),
        },
    );
    let mut registry = registry();
    assert!(registry.with_manual(&state.manual).is_empty());

    let mut app = App::new(&registry, &state);
    let screen = render(&app, 120, 40);
    assert!(screen.contains("KIND"), "{screen}");
    let row = line_with(&screen, "example_manual_model");
    assert!(row.contains("manual "), "{row}");
    assert!(!row.contains("starter"), "{row}");

    // The overlay for that row says what kind of entry it is.
    while app.selected().map(|row| app.model(row).id.as_str()) != Some("example_manual_model") {
        app.reduce(Action::Down);
    }
    // The row shows the tag as the tag, not as a version number.
    let screen = render(&app, 120, 40);
    assert!(screen.contains("sha-b1"), "{screen}");
    assert!(!screen.contains("vsha-"), "{screen}");

    app.reduce(Action::Info);
    let screen = render(&app, 120, 40);
    assert!(
        screen.contains("manual (added with `chap-devops"),
        "{screen}"
    );
    assert!(
        screen.contains("ghcr.io/example/example_manual_model:sha-"),
        "{screen}"
    );
}

/// The key help is as wide as its longest line: every line is whole in a
/// 120-column terminal, and a narrow one marks the cut with `~`.
#[test]
fn the_key_help_is_never_cut_without_a_mark() {
    let registry = registry();
    let mut app = App::new(&registry, &ProjectState::default());
    app.reduce(Action::Help);
    let screen = render(&app, 120, 40);
    for (_, what) in crate::tui::keys::help_entries() {
        assert!(screen.contains(what), "{what} is cut:\n{screen}");
    }
    let screen = render(&app, 50, 40);
    let line = line_with(&screen, "publish a host port");
    assert!(line.contains('~'), "{line}");
}

#[test]
fn the_overlays_render_on_top_in_rounded_boxes() {
    let registry = registry();
    let mut app = App::new(&registry, &ProjectState::default());
    app.reduce(Action::Help);
    let screen = render(&app, 100, 30);
    assert!(screen.contains("Keys"));
    assert!(screen.contains("enable or disable the row"));
    assert!(screen.contains("the command palette"), "{screen}");
    assert!(screen.contains("discard the pending changes"), "{screen}");
    assert!(screen.contains('╭'));

    let mut app = App::new(&registry, &ProjectState::default());
    app.reduce(Action::Toggle);
    app.reduce(Action::Quit);
    let screen = render(&app, 100, 30);
    assert!(screen.contains("Quit"));
    assert!(screen.contains("unsaved changes"));
    assert!(screen.contains("Quit anyway?"));
}

#[test]
fn an_empty_filter_result_still_draws() {
    let registry = registry();
    let mut app = App::new(&registry, &ProjectState::default());
    app.reduce(Action::StartFilter);
    for c in "zzzz".chars() {
        app.reduce(Action::FilterChar(c));
    }
    let screen = render(&app, 120, 40);
    assert!(screen.contains("nothing matches this filter"));
    // And `i` has nothing to open.
    app.reduce(Action::FilterDone);
    app.reduce(Action::Info);
    assert_eq!(app.mode, Mode::Browse);
}

#[test]
fn the_layout_holds_from_a_tiny_terminal_to_a_huge_one() {
    let registry = registry();
    let app = App::new(&registry, &ProjectState::default());
    for (width, height) in [(60, 16), (80, 24), (200, 60)] {
        let screen = render(&app, width, height);
        assert!(screen.contains("chap-devops · models"), "{width}x{height}");
        assert!(screen.contains("Marketplace"), "{width}x{height}");
        assert!(screen.contains("CHAP-EWARS"), "{width}x{height}");
        assert!(screen.contains("[space] toggle"), "{width}x{height}");
        assert_eq!(
            screen.lines().count(),
            height as usize,
            "the frame fills the terminal exactly"
        );
        for line in screen.lines() {
            assert_eq!(
                line.chars().count(),
                width as usize,
                "a line overflowed at {width}x{height}:\n{screen}"
            );
        }
    }
}

/// Eighty columns is the terminal chap-devops has to assume: the id gives way
/// first, marked, and the strip's lines are cut rather than wrapped.
#[test]
fn eighty_columns_truncates_the_id_and_the_strip() {
    let registry = registry();
    let app = App::new(&registry, &ProjectState::default());
    let screen = render(&app, 80, 24);
    let row = line_with(&screen, "Rwanda Malaria BYM");
    assert!(row.contains("chapkit_rwa~"), "{row}");
    assert!(!row.contains("chapkit_rwanda_malaria_bym_model"), "{row}");
    assert!(
        screen.contains("● not for use"),
        "the status column survives"
    );

    let strip = line_with(&screen, "i for details");
    assert!(strip.contains("CHAP-EWARS"), "{strip}");
    assert!(
        strip.contains('~'),
        "the strip is cut, not wrapped: {strip}"
    );
    assert!(
        strip
            .trim_end_matches('│')
            .trim_end()
            .ends_with("i for details"),
        "the hint survives eighty columns: {strip}"
    );
    assert_eq!(strip.chars().count(), 80);
    assert_eq!(
        screen.matches("i for details").count(),
        1,
        "the strip is one row, so the hint is on it once:\n{screen}"
    );
}

#[test]
fn the_title_bar_drops_what_does_not_fit_instead_of_overflowing() {
    let registry = registry();
    let app = App::new(&registry, &ProjectState::default());
    // Wide: both halves.
    let wide = render(&app, 140, 20);
    let first = wide.lines().next().unwrap();
    assert!(first.contains("registry: embedded"));
    assert!(first.contains(&format!("{} models", registry.models.len())));

    // Narrow: the counts are worth more than where the file came from.
    let narrow = render(&app, 60, 16);
    let first = narrow.lines().next().unwrap();
    assert_eq!(first.chars().count(), 60);
    assert!(!first.contains("registry: embedded"), "{first}");
    assert!(first.contains("chap-devops · models"));
}

#[test]
fn a_cached_catalogue_says_how_old_it_is() {
    assert_eq!(
        registry_label(&Provenance::Cache { age_secs: 120 }),
        "registry: cache · 2 min"
    );
    assert_eq!(
        registry_label(&Provenance::StaleCache { age_secs: 172_800 }),
        "registry: stale cache · 2 d"
    );
    assert_eq!(registry_label(&Provenance::Network), "registry: network");
    assert_eq!(registry_label(&Provenance::Embedded), "registry: embedded");
    assert_eq!(short_age(0), "0 sec");
    assert_eq!(short_age(59), "59 sec");
    assert_eq!(short_age(60), "1 min");
    assert_eq!(short_age(7200), "2 hr");
}

#[test]
fn a_monochrome_theme_draws_the_same_shape_without_colour() {
    let registry = registry();
    let state = state_with_ewars(&registry, Some(5001));
    let app = App::new(&registry, &state);
    assert_eq!(
        render_with(&app, 100, 30, &Theme::monochrome()),
        render(&app, 100, 30),
        "NO_COLOR changes the styles, never the characters"
    );

    let mono = Theme::monochrome();
    let row = app.selected().expect("a row is selected");
    let line = row_line(&app, row, &columns(98, false), true, &mono);
    assert!(line.spans.iter().all(|s| s.style.fg.is_none()));
    assert_ne!(
        line.spans[1].style,
        ratatui::style::Style::default(),
        "the tick still stands out"
    );
}

#[test]
fn narrow_tables_drop_columns_instead_of_wrapping() {
    // Wide enough for everything, with the id capped so a wide terminal
    // does not push the columns apart.
    let wide = columns(160, false);
    assert_eq!(wide.id, ID_MAX);
    assert_eq!(wide.status, STATUS_W);
    assert_eq!(wide.port, PORT_W);
    assert_eq!(wide.kind, 0);

    assert_eq!(columns(160, true).kind, KIND_W);

    // Eighty columns: everything, with a shorter id and the narrow form
    // of the enabled column.
    let eighty = columns(78, false);
    assert!(eighty.id >= ID_MIN, "{eighty:?}");
    assert!(eighty.id < ID_MAX);
    assert_eq!(eighty.status, STATUS_W, "the assessment keeps its words");
    assert_eq!(eighty.port, PORT_MIN);

    // Sixty: the id goes first, then the enabled column narrows.
    let sixty = columns(58, false);
    assert_eq!(sixty.id, 0, "{sixty:?}");
    assert_eq!(sixty.version, VERSION_W);

    // Tiny: the name and nothing else, and never a width below zero.
    let tiny = columns(20, false);
    assert_eq!(tiny.id, 0);
    assert_eq!(tiny.status, 0);
    assert_eq!(tiny.model, 15);
    assert_eq!(columns(0, true), columns(0, false));

    for inner in 0..200usize {
        let c = columns(inner, inner % 2 == 0);
        let used = MARKER_W
            + [c.model, c.kind, c.id, c.status, c.version, c.port]
                .iter()
                .map(|w| if *w > 0 { w + 1 } else { 0 })
                .sum::<usize>();
        assert!(
            used <= inner.max(MARKER_W) + 1,
            "{inner}: {c:?} needs {used}"
        );
    }
}

#[test]
fn small_terminals_do_not_panic() {
    let registry = registry();
    let mut app = App::new(&registry, &ProjectState::default());
    for (width, height) in [
        (1, 1),
        (2, 2),
        (8, 3),
        (20, 5),
        (40, 10),
        (60, 16),
        (79, 23),
        (80, 24),
        (200, 60),
    ] {
        render(&app, width, height);
        render_with(&app, width, height, &Theme::monochrome());
    }
    // Overlays are the easiest thing to draw outside a tiny screen.
    for action in [
        Action::Help,
        Action::Info,
        Action::Palette,
        Action::PortPrompt,
        Action::ChannelPrompt,
    ] {
        // Enabled, so the port dialog has something to open on.
        let mut app = App::new(&registry, &state_with_ewars(&registry, Some(5001)));
        app.reduce(action.clone());
        assert_ne!(app.mode, Mode::Browse, "{action:?} opened nothing");
        for (width, height) in [(1, 1), (10, 4), (30, 6), (60, 16), (80, 24)] {
            render(&app, width, height);
        }
    }
    app.reduce(Action::Toggle);
    app.reduce(Action::Quit);
    for (width, height) in [(1, 1), (10, 4), (30, 6), (60, 16), (80, 24)] {
        render(&app, width, height);
    }
}

#[test]
fn the_box_gives_up_its_rules_before_its_rows() {
    // Roomy and idle: the one-row strip leaves the rest to the list.
    assert_eq!(split_body(20, 1), [1, 1, 16, 1, 1]);
    // Roomy, with a three-line pending strip.
    assert_eq!(split_body(20, 3), [1, 1, 14, 1, 3]);
    // Tight: the rules go, then the strip, then the headings.
    assert_eq!(split_body(7, 3), [1, 1, 1, 1, 3]);
    assert_eq!(split_body(5, 3), [1, 0, 1, 0, 3]);
    assert_eq!(split_body(2, 3), [0, 0, 1, 0, 1]);
    assert_eq!(split_body(1, 3), [0, 0, 1, 0, 0]);
    assert_eq!(split_body(0, 3), [0, 0, 0, 0, 0]);
    for h in 0..40u16 {
        for want in 0..8u16 {
            assert_eq!(split_body(h, want).iter().sum::<u16>(), h, "{h}/{want}");
        }
    }
}

#[test]
fn zz_dump_buffers() {
    let registry = registry();
    let mut app = App::new(&registry, &ProjectState::default());
    eprintln!("=== IDLE 120 (default cursor) ===");
    eprintln!("{}", render(&app, 120, 40));
    while app.selected().map(|r| app.model(r).display_name.clone())
        != Some("Rwanda Malaria BYM".to_string())
    {
        app.reduce(Action::Down);
    }
    eprintln!("=== IDLE 120 (Rwanda) ===");
    eprintln!("{}", render(&app, 120, 40));
    eprintln!("=== OVERLAY 120x40 (Rwanda) ===");
    app.reduce(Action::Info);
    eprintln!("{}", render(&app, 120, 40));
    app.reduce(Action::Info);
    eprintln!("=== IDLE 80x24 ===");
    let plain = App::new(&registry, &ProjectState::default());
    eprintln!("{}", render(&plain, 80, 24));
    let mut app = App::new(&registry, &ProjectState::default());
    app.reduce(Action::Toggle);
    app.reduce(Action::Down);
    app.reduce(Action::Toggle);
    eprintln!("=== PENDING 120 ===");
    eprintln!("{}", render(&app, 120, 40));

    let app = App::new(&registry, &state_with_ewars(&registry, None));
    let mut terminal = Terminal::new(TestBackend::new(120, 11)).expect("test backend starts");
    terminal
        .draw(|frame| draw(frame, &app, &theme()))
        .expect("draw");
    eprintln!("=== SVG ===");
    eprintln!(
        "{}",
        crate::tui::screenshot::svg(terminal.backend().buffer(), &theme())
    );
}

#[test]
fn fit_pads_and_truncates() {
    assert_eq!(fit("ab", 4), "ab  ");
    assert_eq!(fit("abcd", 4), "abcd");
    assert_eq!(fit("abcdef", 4), "abc~");
    assert_eq!(fit("abc", 0), "");
    assert_eq!(fit("", 3), "   ");
    // Multi-byte characters must not be cut in half.
    assert_eq!(fit("æøå-modell", 6).chars().count(), 6);
    assert_eq!(fit_soft("ab", 4), "ab");
    assert_eq!(fit_soft("abcdef", 4), "abc~");
}

#[test]
fn truncating_a_line_marks_where_it_was_cut() {
    let theme = theme();
    let spans = || {
        vec![
            Span::raw("requires population"),
            Span::styled("   defaults ", theme.dim_style()),
            Span::raw("rainfall"),
        ]
    };
    let mut line = spans();
    truncate(&mut line, 100);
    assert_eq!(width_of(&line), 39, "nothing to cut, nothing cut");

    let mut line = spans();
    truncate(&mut line, 24);
    let text: String = line.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text.chars().count(), 24);
    assert!(text.ends_with('~'), "{text}");

    // A cut that lands exactly on a span boundary still marks itself.
    let mut line = spans();
    truncate(&mut line, 19);
    let text: String = line.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text, "requires populatio~");

    let mut line = spans();
    truncate(&mut line, 0);
    assert!(line.is_empty());
}

#[test]
fn wrapping_breaks_on_spaces_and_then_on_anything() {
    assert_eq!(wrap("one two three", 7), vec!["one two", "three"]);
    assert_eq!(wrap("", 10), Vec::<String>::new());
    assert_eq!(wrap("word", 0), Vec::<String>::new());
    // A word longer than the line is broken rather than dropped.
    assert_eq!(wrap("abcdefgh", 3), vec!["abc", "def", "gh"]);
    for line in wrap("a very long sentence about models and ports", 11) {
        assert!(line.chars().count() <= 11, "{line}");
    }
}

#[test]
fn centred_popups_stay_inside_the_screen() {
    let area = Rect::new(0, 0, 10, 4);
    let popup = centered(area, 52, 12);
    assert_eq!(popup, area);

    let area = Rect::new(0, 0, 100, 30);
    let popup = centered(area, 52, 12);
    assert_eq!(popup.width, 52);
    assert_eq!(popup.height, 12);
    assert!(popup.x + popup.width <= area.width);
    assert!(popup.y + popup.height <= area.height);
}
