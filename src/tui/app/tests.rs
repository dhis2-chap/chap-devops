use super::*;
use super::{edit::*, selection::*};
use crate::compose::PortRequest;
use crate::registry::load_embedded;

mod components;

const EWARS: &str = "chapkit_ewars_model";
const ARIMA: &str = "auto_arima_chapkit";
const STARTER: &str = "chapkit_minimalist_example_py";

fn registry() -> Registry {
    load_embedded().expect("embedded snapshot parses")
}

/// Entries in the embedded snapshot, and how many of those are models
/// rather than starters. Read off the snapshot so a marketplace refresh
/// moves the counts below with it.
fn total(registry: &Registry) -> usize {
    registry.models.len()
}

fn without_starters(registry: &Registry) -> usize {
    registry.deployable().count()
}

fn empty_state() -> ProjectState {
    ProjectState::default()
}

fn state_with(registry: &Registry, id: &str, channel: Option<Channel>) -> ProjectState {
    state_with_port(registry, id, channel, None)
}

/// The same, with an explicit host port for the enabled model.
fn state_with_port(
    registry: &Registry,
    id: &str,
    channel: Option<Channel>,
    host_port: Option<u16>,
) -> ProjectState {
    let model = registry.get(id).expect("vendored model");
    let version = model
        .resolve(&VersionSelector::Channel(
            channel.unwrap_or(Channel::Stable),
        ))
        .expect("channel resolves");
    let mut state = ProjectState::default();
    state.models.insert(
        model.id.clone(),
        EnabledModel {
            previous: None,
            reads_port: false,
            service_id: model.service_id.clone(),
            image: model.source.image.clone(),
            image_tag: version.image_tag.clone(),
            image_digest: None,
            version: version.version.clone(),
            channel,
            host_port,
            bind: None,
            data_dir: "/work/data".to_string(),
            user: "chapkit:chapkit".to_string(),
            user_from: Default::default(),
            platform: None,
            compose_file: format!("compose.{}.yml", model.service_id),
        },
    );
    state
}

/// Pick a channel through the dialog the way `v` does.
fn pick_channel(app: &mut App, channel: Channel) {
    app.reduce(Action::ChannelPrompt);
    app.reduce(Action::ChannelChar(match channel {
        Channel::Stable => 's',
        Channel::Latest => 'l',
    }));
    app.reduce(Action::ChannelApply);
}

/// Type something into the port prompt and take it.
fn ask_for_port(app: &mut App, text: &str) {
    app.reduce(Action::PortPrompt);
    app.port_input.clear();
    for c in text.chars() {
        app.reduce(Action::PortChar(c));
    }
    app.reduce(Action::PortApply);
}

/// Put the cursor on a model id; panics when it is not visible.
fn focus(app: &mut App, id: &str) {
    let position = app
        .visible
        .iter()
        .position(|row_idx| app.model(&app.rows[*row_idx]).id == id)
        .unwrap_or_else(|| panic!("{id} is not visible"));
    app.cursor = position;
}

#[test]
fn a_fresh_browser_shows_models_but_not_starters() {
    let registry = registry();
    let app = App::new(&registry, &empty_state());
    assert_eq!(app.rows.len(), total(&registry));
    assert_eq!(
        app.visible.len(),
        without_starters(&registry),
        "the two starters are hidden"
    );
    assert!(!app.show_starters);
    assert_eq!(app.mode, Mode::Browse);
    assert!(!app.dirty);
    assert_eq!(
        app.counts(),
        Counts {
            total: total(&registry),
            enabled: 0,
            pending: 0
        }
    );
}

#[test]
fn starters_appear_after_toggling_them_on() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    assert!(app.reduce(Action::ToggleStarters).is_none());
    assert!(app.show_starters);
    assert_eq!(app.visible.len(), total(&registry));
    app.reduce(Action::ToggleStarters);
    assert_eq!(app.visible.len(), without_starters(&registry));
}

#[test]
fn toggling_a_row_marks_the_browser_dirty_and_enables_it() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    focus(&mut app, EWARS);

    assert!(app.reduce(Action::Toggle).is_none());
    assert!(app.dirty);
    assert!(app.selected().unwrap().enabled);

    let selection = app.selection();
    assert_eq!(selection.disable.len(), 0);
    assert_eq!(selection.enable.len(), 1);
    let request = &selection.enable[0];
    assert_eq!(request.id, EWARS);
    assert_eq!(
        request.selector,
        VersionSelector::Channel(Channel::Stable),
        "a fresh row follows stable"
    );
    assert!(
        request.port.is_none(),
        "a fresh row publishes nothing, which is apply's default"
    );
    assert!(!app.selected().unwrap().publishes());
    assert!(!request.allow_starter);
    assert_eq!(app.counts().pending, 1);
}

/// `p` opens a prompt prefilled with what the row asks for today, and
/// Enter on it is the old "give me any free port".
#[test]
fn p_prompts_for_a_port_and_auto_is_what_enter_takes() {
    let registry = registry();

    // A model that publishes nothing: the prompt opens on `auto`.
    let state = state_with_port(&registry, EWARS, Some(Channel::Stable), None);
    let mut app = App::new(&registry, &state);
    focus(&mut app, EWARS);
    assert!(!app.selected().unwrap().publishes());

    assert!(app.reduce(Action::PortPrompt).is_none());
    assert_eq!(app.mode, Mode::Port);
    assert_eq!(app.port_input, "auto");
    assert!(app.reduce(Action::PortApply).is_none());
    assert_eq!(app.mode, Mode::Browse);

    assert_eq!(app.selected().unwrap().want, PortWant::Auto);
    assert!(app.dirty);
    let selection = app.selection();
    assert!(selection.disable.is_empty());
    assert_eq!(selection.enable.len(), 1);
    assert_eq!(selection.enable[0].id, EWARS);
    assert_eq!(selection.enable[0].port, Some(PortRequest::Auto));
    assert_eq!(
        selection.enable[0].selector,
        VersionSelector::Channel(Channel::Stable),
        "the pin does not move because a port did"
    );
    assert!(
        selection.enable[0].keep_version,
        "and the version the project recorded is kept, not re-resolved"
    );

    // Saying none is back where we started, so nothing to apply.
    ask_for_port(&mut app, "none");
    assert!(!app.has_changes());

    // A model that does publish one: the prompt opens on its number.
    let state = state_with_port(&registry, EWARS, Some(Channel::Stable), Some(5001));
    let mut app = App::new(&registry, &state);
    focus(&mut app, EWARS);
    assert!(app.selected().unwrap().publishes());
    assert_eq!(app.selected().unwrap().port, Some(5001));
    app.reduce(Action::PortPrompt);
    assert_eq!(app.port_input, "5001");
    app.reduce(Action::PortApply);
    assert!(
        !app.has_changes(),
        "taking back the port it already has changes nothing"
    );

    // An empty line takes the port away.
    ask_for_port(&mut app, "");
    let selection = app.selection();
    assert_eq!(selection.enable.len(), 1);
    assert_eq!(selection.enable[0].port, Some(PortRequest::None));
    assert!(selection.disable.is_empty());
}

/// The prompt takes a number, the way `chap-devops models expose --port` does.
#[test]
fn the_port_prompt_takes_a_number_auto_or_none() {
    let registry = registry();
    let state = state_with_port(&registry, EWARS, Some(Channel::Stable), None);
    let mut app = App::new(&registry, &state);
    focus(&mut app, EWARS);

    ask_for_port(&mut app, "5010");
    assert_eq!(app.mode, Mode::Browse);
    assert_eq!(app.selected().unwrap().want, PortWant::Exact(5010));
    assert_eq!(
        app.selection().enable[0].port,
        Some(PortRequest::Fixed(5010))
    );
    assert!(app.selection().enable[0].keep_version);

    ask_for_port(&mut app, "AUTO");
    assert_eq!(app.selected().unwrap().want, PortWant::Auto);
    ask_for_port(&mut app, "None");
    assert_eq!(app.selected().unwrap().want, PortWant::None);
    assert_eq!(app.selection().enable.len(), 0, "it had none to begin with");

    // Esc leaves the row alone whatever was typed.
    app.reduce(Action::PortPrompt);
    for c in "5010".chars() {
        app.reduce(Action::PortChar(c));
    }
    app.reduce(Action::PortBackspace);
    app.reduce(Action::FilterCancel);
    assert_eq!(app.mode, Mode::Browse);
    assert!(app.port_input.is_empty());
    assert_eq!(app.selected().unwrap().want, PortWant::None);
}

/// A refused port keeps the prompt up with the reason on it: the number is
/// corrected, not typed again from nothing.
#[test]
fn the_port_prompt_says_why_it_refuses_and_stays_open() {
    let registry = registry();
    let state = state_with_port(&registry, EWARS, Some(Channel::Stable), None);
    let mut app = App::new(&registry, &state);
    focus(&mut app, EWARS);

    for (typed, reason) in [
        ("80", "outside this deployment's range"),
        ("6000", "outside this deployment's range"),
        ("8700", "chap-core's own API port"),
        ("five thousand", "is not a port"),
    ] {
        ask_for_port(&mut app, typed);
        assert_eq!(app.mode, Mode::Port, "{typed} left the prompt");
        let why = app.port_error.clone().unwrap_or_default();
        assert!(why.contains(reason), "{typed}: {why}");
        assert_eq!(app.port_input, typed, "what was typed is still there");
        assert!(!app.has_changes());
        app.reduce(Action::FilterCancel);
    }

    // A port another enabled row is already asking for is refused too.
    let mut app = App::new(&registry, &empty_state());
    focus(&mut app, EWARS);
    app.reduce(Action::Toggle);
    ask_for_port(&mut app, "5010");
    focus(&mut app, ARIMA);
    app.reduce(Action::Toggle);
    ask_for_port(&mut app, "5010");
    assert_eq!(app.mode, Mode::Port);
    assert!(
        app.port_error
            .as_deref()
            .unwrap_or_default()
            .contains("already taken by another model"),
        "{:?}",
        app.port_error
    );
    ask_for_port(&mut app, "5011");
    assert_eq!(app.selected().unwrap().want, PortWant::Exact(5011));
}

/// The prompt only makes sense on a model this project runs.
#[test]
fn the_port_prompt_asks_for_the_model_to_be_enabled_first() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    focus(&mut app, ARIMA);
    app.reduce(Action::PortPrompt);
    assert_eq!(app.mode, Mode::Browse);
    assert_eq!(app.message.as_deref(), Some(PUBLISH_NEEDS_ENABLED_HINT));
}

/// A port change and a channel change are two different requests. The
/// first has to keep the version the deployment is running - `state_with`
/// records an exact pin as `channel: None`, which is what re-resolving
/// would silently turn into "whatever stable points at today" - and the
/// second is asking for a new one.
#[test]
fn a_port_only_change_keeps_the_version_and_a_channel_change_does_not() {
    let registry = registry();
    let state = state_with_port(&registry, EWARS, None, None);
    let mut app = App::new(&registry, &state);
    focus(&mut app, EWARS);

    ask_for_port(&mut app, "auto");
    let selection = app.selection();
    assert_eq!(selection.enable.len(), 1);
    assert_eq!(selection.enable[0].port, Some(PortRequest::Auto));
    assert!(
        selection.enable[0].keep_version,
        "`p` asks for a port, not for an upgrade"
    );

    // Cycling the channel is a request about the version, port and all.
    pick_channel(&mut app, Channel::Latest);
    let selection = app.selection();
    assert_eq!(selection.enable.len(), 1);
    assert_eq!(
        selection.enable[0].selector,
        VersionSelector::Channel(Channel::Latest)
    );
    assert_eq!(selection.enable[0].port, Some(PortRequest::Auto));
    assert!(!selection.enable[0].keep_version);

    // A row this session enabled has no recorded version to keep.
    let mut app = App::new(&registry, &empty_state());
    focus(&mut app, EWARS);
    app.reduce(Action::Toggle);
    ask_for_port(&mut app, "auto");
    assert!(!app.selection().enable[0].keep_version);
}

#[test]
fn p_on_a_row_that_is_not_enabled_only_hints() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    focus(&mut app, ARIMA);

    app.reduce(Action::PortPrompt);
    assert_eq!(app.message.as_deref(), Some(PUBLISH_NEEDS_ENABLED_HINT));
    assert!(!app.selected().unwrap().publishes());
    assert!(!app.has_changes());

    // Enabled first, then published: one request carrying both.
    app.reduce(Action::Toggle);
    ask_for_port(&mut app, "auto");
    let selection = app.selection();
    assert_eq!(selection.enable.len(), 1);
    assert_eq!(selection.enable[0].port, Some(PortRequest::Auto));
}

#[test]
fn disabling_a_published_row_forgets_the_port_too() {
    let registry = registry();
    let state = state_with_port(&registry, EWARS, Some(Channel::Stable), Some(5001));
    let mut app = App::new(&registry, &state);
    focus(&mut app, EWARS);

    // Off, then on again: the model comes back reachable only through
    // chap-core rather than silently re-publishing a port.
    app.reduce(Action::Toggle);
    assert!(!app.selected().unwrap().publishes());
    app.reduce(Action::Toggle);
    assert!(app.selected().unwrap().enabled);
    assert!(!app.selected().unwrap().publishes());
    let selection = app.selection();
    assert_eq!(selection.enable.len(), 1);
    assert_eq!(selection.enable[0].port, Some(PortRequest::None));
}

#[test]
fn a_port_change_is_only_a_change_when_it_differs_from_the_record() {
    assert_eq!(port_change(PortWant::Auto, None), Some(PortRequest::Auto));
    assert_eq!(
        port_change(PortWant::None, Some(5001)),
        Some(PortRequest::None)
    );
    assert_eq!(
        port_change(PortWant::Exact(5001), Some(5001)),
        None,
        "it already has that one"
    );
    assert_eq!(
        port_change(PortWant::Exact(5010), Some(5001)),
        Some(PortRequest::Fixed(5010)),
        "a different one is a claim to make"
    );
    assert_eq!(
        port_change(PortWant::Exact(5010), None),
        Some(PortRequest::Fixed(5010))
    );
    assert_eq!(port_change(PortWant::None, None), None, "already proxied");
}

/// What the prompt accepts, away from the reducer.
#[test]
fn the_port_prompt_parses_the_three_things_it_takes() {
    let range = (5001, 5999);
    let ok = |text: &str| parse_port(text, range, 8000, &[5010]).expect(text);
    assert_eq!(ok(""), PortWant::None);
    assert_eq!(ok("  "), PortWant::None);
    assert_eq!(ok("none"), PortWant::None);
    assert_eq!(ok("NONE"), PortWant::None);
    assert_eq!(ok("auto"), PortWant::Auto);
    assert_eq!(ok(" 5011 "), PortWant::Exact(5011));

    let why = |text: &str| parse_port(text, range, 8000, &[5010]).expect_err(text);
    assert!(why("5000").contains("5001-5999"));
    assert!(why("6000").contains("5001-5999"));
    assert!(why("8000").contains("API port"));
    assert!(why("5010").contains("another model"));
    assert!(why("abc").contains("not a port"));
    assert!(why("-1").contains("not a port"));
}

#[test]
fn toggling_back_leaves_nothing_to_apply() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    focus(&mut app, EWARS);
    app.reduce(Action::Toggle);
    app.reduce(Action::Toggle);

    assert!(!app.dirty);
    let selection = app.selection();
    assert!(selection.enable.is_empty() && selection.disable.is_empty());
    assert_eq!(app.counts().pending, 0);
}

#[test]
fn disabling_a_recorded_model_yields_the_disable_list() {
    let registry = registry();
    let state = state_with(&registry, EWARS, Some(Channel::Stable));
    let mut app = App::new(&registry, &state);
    focus(&mut app, EWARS);
    assert!(app.selected().unwrap().enabled);
    assert_eq!(app.selected().unwrap().port, None);

    app.reduce(Action::Toggle);
    let selection = app.selection();
    assert_eq!(selection.disable, vec![EWARS.to_string()]);
    assert!(selection.enable.is_empty());
    assert!(app.dirty);
}

#[test]
fn picking_a_channel_for_an_enabled_row_re_pins_it() {
    let registry = registry();
    let state = state_with(&registry, EWARS, Some(Channel::Stable));
    let mut app = App::new(&registry, &state);
    focus(&mut app, EWARS);

    pick_channel(&mut app, Channel::Latest);
    assert_eq!(app.selected().unwrap().channel, Some(Channel::Latest));
    let selection = app.selection();
    assert!(selection.disable.is_empty());
    assert_eq!(selection.enable.len(), 1);
    assert_eq!(selection.enable[0].id, EWARS);
    assert_eq!(
        selection.enable[0].selector,
        VersionSelector::Channel(Channel::Latest)
    );

    // Picking the one it started on is once again a no-op.
    pick_channel(&mut app, Channel::Stable);
    assert_eq!(app.selected().unwrap().channel, Some(Channel::Stable));
    assert!(!app.has_changes());

    // And Esc takes nothing.
    app.reduce(Action::ChannelPrompt);
    assert_eq!(app.mode, Mode::Channel);
    assert_eq!(app.channel_cursor, 0, "it opens on the one in force");
    app.reduce(Action::Down);
    assert_eq!(app.channel_cursor, 1);
    app.reduce(Action::Down);
    assert_eq!(app.channel_cursor, 1, "there are only two");
    app.reduce(Action::FilterCancel);
    assert_eq!(app.mode, Mode::Browse);
    assert_eq!(app.selected().unwrap().channel, Some(Channel::Stable));
    assert!(!app.has_changes());
}

#[test]
fn a_row_nobody_enabled_takes_a_channel_but_changes_nothing() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    focus(&mut app, ARIMA);
    pick_channel(&mut app, Channel::Latest);
    assert_eq!(app.selected().unwrap().channel, Some(Channel::Latest));
    assert!(!app.has_changes(), "a disabled row has nothing to apply");
}

#[test]
fn an_untouched_browser_saves_nothing() {
    let registry = registry();
    let state = state_with(&registry, EWARS, Some(Channel::Stable));
    let app = App::new(&registry, &state);
    assert!(!app.has_changes());
    assert_eq!(app.counts().enabled, 1);
}

#[test]
fn an_exact_pin_survives_a_save_that_does_not_touch_it() {
    let registry = registry();
    // channel: None means .chap-devops/models.yaml holds an exact version pin.
    let state = state_with(&registry, EWARS, None);
    let app = App::new(&registry, &state);
    assert!(
        !app.has_changes(),
        "opening and saving must not convert a pin into a channel"
    );
}

/// A pin to a version no channel points at stays that version until a
/// channel is picked, and picking stable is then a change.
#[test]
fn a_pin_off_the_channels_resolves_to_the_pin_until_a_channel_is_picked() {
    let registry = registry();
    let mut state = state_with(&registry, EWARS, None);
    let model = registry.get(EWARS).unwrap();
    let older = model
        .versions
        .iter()
        .find(|v| v.version != model.channels.stable)
        .expect("ewars has an older version");
    state.models.get_mut(EWARS).unwrap().version = older.version.clone();
    let mut app = App::new(&registry, &state);
    app.filter = EWARS.to_string();
    app.refilter();
    let row = app.selected().unwrap().clone();
    assert_eq!(row.channel, None);
    assert_eq!(app.resolved(&row).unwrap().version, older.version);
    assert_eq!(app.pinned(&row), Some(older.version.as_str()));
    assert!(!app.has_changes());

    pick_channel(&mut app, Channel::Stable);
    let selection = app.selection();
    assert_eq!(selection.enable.len(), 1, "stable moves the pin");
    assert_eq!(
        selection.enable[0].selector,
        VersionSelector::Channel(Channel::Stable)
    );
}

#[test]
fn filtering_narrows_the_list_and_keeps_the_cursor_valid() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::Bottom);
    let last = app.cursor;
    assert!(last > 0);

    app.reduce(Action::StartFilter);
    assert_eq!(app.mode, Mode::Filter);
    for c in "ewars".chars() {
        app.reduce(Action::FilterChar(c));
    }
    assert_eq!(app.filter, "ewars");
    assert_eq!(app.visible.len(), 1);
    assert!(app.cursor < app.visible.len());
    assert_eq!(app.selected_model().unwrap().id, EWARS);

    // Backspacing widens it again.
    app.reduce(Action::FilterBackspace);
    assert_eq!(app.filter, "ewar");
    assert_eq!(app.visible.len(), 1);

    app.reduce(Action::FilterDone);
    assert_eq!(app.mode, Mode::Browse);
    assert_eq!(app.filter, "ewar", "Enter keeps the filter");
}

#[test]
fn escaping_the_filter_clears_it() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::StartFilter);
    for c in "arima".chars() {
        app.reduce(Action::FilterChar(c));
    }
    assert_eq!(app.visible.len(), 1);
    app.reduce(Action::FilterCancel);
    assert_eq!(app.mode, Mode::Browse);
    assert!(app.filter.is_empty());
    assert_eq!(app.visible.len(), without_starters(&registry));
}

#[test]
fn a_filter_that_matches_nothing_leaves_no_selection() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::StartFilter);
    for c in "zzzz".chars() {
        app.reduce(Action::FilterChar(c));
    }
    assert!(app.visible.is_empty());
    assert_eq!(app.cursor, 0);
    assert!(app.selected().is_none());
    // Movement and toggling on an empty list must not panic.
    app.reduce(Action::FilterDone);
    app.reduce(Action::Down);
    app.reduce(Action::Toggle);
    pick_channel(&mut app, Channel::Latest);
    assert!(!app.has_changes());
}

#[test]
fn enabling_a_starter_warns_and_allows_it() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::ToggleStarters);
    focus(&mut app, STARTER);
    app.reduce(Action::Toggle);

    assert_eq!(app.message.as_deref(), Some(STARTER_WARNING));
    let selection = app.selection();
    assert_eq!(selection.enable.len(), 1);
    assert!(
        selection.enable[0].allow_starter,
        "apply must accept the starter the browser showed"
    );
}

#[test]
fn a_project_running_a_starter_shows_starters_from_the_start() {
    let registry = registry();
    let state = state_with(&registry, STARTER, Some(Channel::Stable));
    let app = App::new(&registry, &state);
    assert!(app.show_starters);
    assert_eq!(app.visible.len(), total(&registry));
}

#[test]
fn movement_stays_inside_the_list() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::Up);
    assert_eq!(app.cursor, 0);
    app.reduce(Action::PageUp);
    assert_eq!(app.cursor, 0);

    app.reduce(Action::PageDown);
    assert_eq!(app.cursor, app.visible.len() - 1);
    app.reduce(Action::Down);
    assert_eq!(app.cursor, app.visible.len() - 1);
    app.reduce(Action::Top);
    assert_eq!(app.cursor, 0);
    app.reduce(Action::Bottom);
    assert_eq!(app.cursor, app.visible.len() - 1);
}

#[test]
fn quitting_a_dirty_browser_asks_first() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    focus(&mut app, EWARS);
    app.reduce(Action::Toggle);

    assert!(app.reduce(Action::Quit).is_none());
    assert_eq!(app.mode, Mode::ConfirmQuit);

    // Answering "no" returns to the list with the change intact.
    assert!(app.reduce(Action::ConfirmNo).is_none());
    assert_eq!(app.mode, Mode::Browse);
    assert!(app.dirty);

    app.reduce(Action::Quit);
    assert_eq!(app.reduce(Action::ConfirmYes), Some(Outcome::Quit));
}

#[test]
fn quitting_a_clean_browser_leaves_at_once() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    assert_eq!(app.reduce(Action::Quit), Some(Outcome::Quit));
}

#[test]
fn saving_ends_the_browser_from_browse_mode() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    focus(&mut app, ARIMA);
    app.reduce(Action::Toggle);
    assert_eq!(app.reduce(Action::Save), Some(Outcome::Save));
    assert_eq!(app.selection().enable[0].id, ARIMA);
}

#[test]
fn help_swallows_everything_until_it_closes() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::Help);
    assert_eq!(app.mode, Mode::Help);

    app.reduce(Action::Down);
    assert_eq!(app.cursor, 0, "movement is inert behind the overlay");
    app.reduce(Action::Toggle);
    assert!(!app.has_changes(), "toggling is inert behind the overlay");

    app.reduce(Action::Help);
    assert_eq!(app.mode, Mode::Browse);
}

#[test]
fn the_footer_note_clears_on_the_next_action() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::ToggleStarters);
    focus(&mut app, STARTER);
    app.reduce(Action::Toggle);
    assert!(app.message.is_some());
    app.reduce(Action::Down);
    assert!(app.message.is_none());
}

/// `i` opens the details over the list and swallows the keys that would
/// otherwise move it; only scrolling and the way out get through.
#[test]
fn the_details_overlay_opens_scrolls_within_its_bounds_and_closes() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    focus(&mut app, EWARS);
    app.reduce(Action::Info);
    assert_eq!(app.mode, Mode::Info);
    assert_eq!(app.info_scroll, 0);

    // Scrolling is clamped by what the last frame could show.
    app.info_max.set(3);
    for _ in 0..10 {
        app.reduce(Action::Down);
    }
    assert_eq!(app.info_scroll, 3, "it never scrolls past the end");
    app.reduce(Action::Up);
    assert_eq!(app.info_scroll, 2);
    app.reduce(Action::Top);
    assert_eq!(app.info_scroll, 0);
    app.reduce(Action::Bottom);
    assert_eq!(app.info_scroll, 3);
    app.reduce(Action::PageUp);
    assert_eq!(app.info_scroll, 0);

    // The list underneath is untouched.
    app.reduce(Action::Toggle);
    assert!(!app.has_changes(), "toggling is inert behind the overlay");

    app.reduce(Action::FilterCancel);
    assert_eq!(app.mode, Mode::Browse);
    assert_eq!(app.info_scroll, 0, "it opens at the top next time");

    // An empty list has nothing to describe, so nothing opens.
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::StartFilter);
    for c in "zzzz".chars() {
        app.reduce(Action::FilterChar(c));
    }
    app.reduce(Action::FilterDone);
    app.reduce(Action::Info);
    assert_eq!(app.mode, Mode::Browse);
}

/// `o` and `c` are about the model, not about the project: neither writes
/// anything, and both say what they did.
#[test]
fn the_overlay_keys_open_a_repository_and_show_an_image_reference() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    focus(&mut app, EWARS);

    app.reduce(Action::Open);
    let expected = registry.get(EWARS).unwrap().source.repository.clone();
    assert_eq!(app.take_effect(), Some(Effect::Open(expected)));
    assert!(app.take_effect().is_none(), "an effect is taken once");

    app.reduce(Action::ImageRef);
    let message = app.message.clone().expect("the reference is on the line");
    assert!(message.starts_with("ghcr.io/chap-models/chapkit_ewars_model:"));
    assert!(!app.has_changes());
}

#[test]
fn the_palette_narrows_by_substring_and_runs_what_it_matched() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    focus(&mut app, EWARS);
    // A host port is only a question for a model that is enabled.
    app.reduce(Action::Toggle);
    app.reduce(Action::Palette);
    assert_eq!(app.mode, Mode::Palette);
    assert_eq!(app.palette_matches().len(), app.commands().len());
    // Sixteen entries that do not depend on the component set, plus one
    // toggle per component, so a fifth component moves this number and
    // nothing else in the palette has to be counted again.
    assert_eq!(app.commands().len(), 16 + Component::ALL.len());
    assert_eq!(app.commands().len(), 20);

    for c in "PORT".chars() {
        app.reduce(Action::PaletteChar(c));
    }
    let matched = app.palette_matches();
    assert_eq!(matched.len(), 2, "case does not matter: {matched:?}");
    assert_eq!(matched[0].id, CommandId::SetPort);
    assert_eq!(matched[1].id, CommandId::RemovePort);
    let (from, to) = matched[0].hit.expect("the match is marked");
    assert_eq!(
        matched[0]
            .label
            .chars()
            .skip(from)
            .take(to - from)
            .collect::<String>(),
        "port"
    );
    assert!(matched[0].label.contains("CHAP-EWARS"), "{matched:?}");

    // "Set a host port" opens the same prompt `p` does; "remove" needs no
    // prompt, so the two are not each other's undo by accident.
    app.reduce(Action::PaletteRun);
    assert_eq!(app.mode, Mode::Port, "the palette opened the prompt");
    assert_eq!(app.port_input, "auto");
    app.port_input.clear();
    for c in "5010".chars() {
        app.reduce(Action::PortChar(c));
    }
    app.reduce(Action::PortApply);
    assert_eq!(app.selected().unwrap().want, PortWant::Exact(5010));

    app.reduce(Action::Palette);
    for c in "set a host".chars() {
        app.reduce(Action::PaletteChar(c));
    }
    app.reduce(Action::PaletteRun);
    assert_eq!(app.port_input, "5010", "prefilled with what it has");
    app.reduce(Action::FilterCancel);
    assert_eq!(app.selected().unwrap().want, PortWant::Exact(5010));

    app.reduce(Action::Palette);
    for c in "remove the host".chars() {
        app.reduce(Action::PaletteChar(c));
    }
    app.reduce(Action::PaletteRun);
    assert_eq!(app.mode, Mode::Browse, "removing needs no prompt");
    assert!(!app.selected().unwrap().publishes());
}

#[test]
fn the_palette_moves_backspaces_and_leaves_without_running_anything() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::Palette);
    for c in "port".chars() {
        app.reduce(Action::PaletteChar(c));
    }
    app.reduce(Action::Down);
    assert_eq!(app.palette_cursor, 1);
    app.reduce(Action::Down);
    assert_eq!(app.palette_cursor, 1, "the cursor stops at the last match");
    app.reduce(Action::Up);
    assert_eq!(app.palette_cursor, 0);

    app.reduce(Action::PaletteBackspace);
    assert_eq!(app.palette_query, "por");
    app.reduce(Action::FilterCancel);
    assert_eq!(app.mode, Mode::Browse);
    assert!(app.palette_query.is_empty());
    assert!(!app.has_changes(), "leaving the palette runs nothing");

    // A query nothing matches runs nothing either.
    app.reduce(Action::Palette);
    for c in "zzzz".chars() {
        app.reduce(Action::FilterChar(c));
    }
    for c in "zzzz".chars() {
        app.reduce(Action::PaletteChar(c));
    }
    assert!(app.palette_matches().is_empty());
    assert!(app.reduce(Action::PaletteRun).is_none());
    assert_eq!(app.mode, Mode::Palette);
}

/// The commands that leave the terminal do not do it themselves: they ask
/// the caller, which is what keeps the reducer pure.
#[test]
fn the_palette_asks_the_caller_for_the_things_it_cannot_do() {
    let registry = registry();
    let run = |query: &str| {
        let mut app = App::new(&registry, &empty_state());
        app.reduce(Action::Palette);
        for c in query.chars() {
            app.reduce(Action::PaletteChar(c));
        }
        assert_eq!(app.palette_matches().len(), 1, "{query} is ambiguous");
        let outcome = app.reduce(Action::PaletteRun);
        (app.effect.clone(), app.mode, outcome)
    };

    assert_eq!(run("refresh the registry").0, Some(Effect::Refresh));
    assert_eq!(
        run("chap-devops documentation").0,
        Some(Effect::Open(format!(
            "{}{DOCS_CHAPTER}",
            crate::cli::DOCS_URL
        )))
    );
    assert!(matches!(
        run("open the repository").0,
        Some(Effect::Open(_))
    ));
    assert_eq!(run("every key").1, Mode::Help);
    assert_eq!(run("filter the model").1, Mode::Filter);
    assert_eq!(run("save the changes").2, Some(Outcome::Save));
    assert_eq!(run("quit the browser").2, Some(Outcome::Quit));
    assert!(run("show or hide").0.is_none());
}

#[test]
fn discarding_puts_every_row_back_where_the_project_had_it() {
    let registry = registry();
    let state = state_with_port(&registry, EWARS, Some(Channel::Stable), Some(5001));
    let mut app = App::new(&registry, &state);
    focus(&mut app, EWARS);
    app.reduce(Action::Toggle);
    focus(&mut app, ARIMA);
    app.reduce(Action::Toggle);
    pick_channel(&mut app, Channel::Latest);
    assert_eq!(app.counts().pending, 2);

    app.reduce(Action::Discard);
    assert_eq!(app.message.as_deref(), Some(DISCARDED_HINT));
    assert!(!app.has_changes());
    assert!(!app.dirty);
    focus(&mut app, EWARS);
    assert!(app.selected().unwrap().enabled);
    assert_eq!(app.selected().unwrap().port, Some(5001));
    assert_eq!(app.selected().unwrap().want, PortWant::Exact(5001));
    focus(&mut app, ARIMA);
    assert!(!app.selected().unwrap().enabled);
    assert_eq!(app.selected().unwrap().channel, Some(Channel::Stable));

    // With nothing pending it says so rather than doing nothing quietly.
    app.reduce(Action::Discard);
    assert_eq!(app.message.as_deref(), Some(NOTHING_TO_DISCARD_HINT));
}

/// Esc is the way out of a filter first and the way out of the browser
/// second, so it never loses a filter and a session in one press.
#[test]
fn esc_clears_a_filter_before_it_quits() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::StartFilter);
    for c in "arima".chars() {
        app.reduce(Action::FilterChar(c));
    }
    app.reduce(Action::FilterDone);
    assert_eq!(app.visible.len(), 1);

    assert!(app.reduce(Action::ClearFilterOrQuit).is_none());
    assert!(app.filter.is_empty());
    assert_eq!(app.visible.len(), without_starters(&registry));

    assert_eq!(
        app.reduce(Action::ClearFilterOrQuit),
        Some(Outcome::Quit),
        "with no filter left it is the way out"
    );

    // And it still asks before throwing changes away.
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::Toggle);
    assert!(app.reduce(Action::ClearFilterOrQuit).is_none());
    assert_eq!(app.mode, Mode::ConfirmQuit);
}

/// The pending strip is the summary of what saving writes, so it has to
/// tell an enable from a re-pin from a disable.
#[test]
fn the_change_list_says_what_each_pending_change_does() {
    let registry = registry();
    let state = state_with_port(&registry, EWARS, Some(Channel::Stable), None);
    let mut app = App::new(&registry, &state);
    assert!(app.changes().is_empty(), "an untouched browser has none");

    focus(&mut app, ARIMA);
    app.reduce(Action::Toggle);
    focus(&mut app, EWARS);
    pick_channel(&mut app, Channel::Latest);
    ask_for_port(&mut app, "5010");

    let changes = app.changes();
    assert_eq!(changes.len(), 2);
    let added = changes
        .iter()
        .find(|c| c.kind == ChangeKind::Add)
        .expect("the new model is an addition");
    assert!(added.detail.starts_with("enable at "), "{added:?}");
    assert!(added.detail.ends_with("via chap-core"), "{added:?}");

    let updated = changes
        .iter()
        .find(|c| c.kind == ChangeKind::Update)
        .expect("the re-pinned model is an update");
    assert_eq!(updated.name, "CHAP-EWARS");
    assert!(updated.detail.contains("follow latest"), "{updated:?}");
    assert!(updated.detail.contains("publish port 5010"), "{updated:?}");

    // Turning it off instead is a removal, and it says what is kept.
    app.reduce(Action::Toggle);
    let changes = app.changes();
    let removed = changes
        .iter()
        .find(|c| c.kind == ChangeKind::Remove)
        .expect("a disabled model is a removal");
    assert_eq!(removed.name, "CHAP-EWARS");
    assert_eq!(removed.detail, "disable · the data volume is kept");
    assert_eq!(changes.len(), 2);
    assert_eq!(app.counts().pending, 2, "and the header counts the same");
}

/// The list is ordered by how far a model can be trusted, not by
/// whatever order the marketplace index names its files in.
#[test]
fn the_list_is_ordered_by_maturity_then_by_name() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    let names = |app: &App| -> Vec<String> {
        app.visible
            .iter()
            .map(|i| app.model(&app.rows[*i]).display_name.clone())
            .collect()
    };
    assert_eq!(
        names(&app),
        vec![
            "CHAP-EWARS",
            "Simple Multistep",
            "Auto-ARIMA",
            "GHRmodel",
            "Rwanda Malaria BYM",
        ],
        "orange, orange, red, red, gray - and alphabetical inside each"
    );
    for pair in app.visible.windows(2) {
        let (a, b) = (app.model(&app.rows[pair[0]]), app.model(&app.rows[pair[1]]));
        assert!(
            a.assessed_status.rank() <= b.assessed_status.rank(),
            "{} came before {}",
            a.display_name,
            b.display_name
        );
    }

    // Starters are scaffolding, so they follow the models whatever
    // their own assessment says.
    app.reduce(Action::ToggleStarters);
    let shown = names(&app);
    let first_starter = shown
        .iter()
        .position(|name| name.contains("Minimalist"))
        .expect("the starters are listed");
    assert!(first_starter >= 5, "starters come last: {shown:?}");

    // A filter narrows the list; it does not reorder it.
    app.reduce(Action::ToggleStarters);
    app.reduce(Action::StartFilter);
    for c in "model".chars() {
        app.reduce(Action::FilterChar(c));
    }
    let filtered = names(&app);
    let mut expected = filtered.clone();
    expected.sort_by_key(|name| {
        let model = registry
            .models
            .iter()
            .find(|m| &m.display_name == name)
            .unwrap();
        model.order_key()
    });
    assert_eq!(filtered, expected, "the filter kept the order");
}

/// Nothing the browser does changes a model's maturity, so the cursor
/// stays on the row it was on when a toggle re-reads the list.
#[test]
fn toggling_a_row_never_moves_it() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::Down);
    let before = app.selected_model().map(|m| m.id.clone());
    app.reduce(Action::Toggle);
    assert_eq!(app.selected_model().map(|m| m.id.clone()), before);
    app.reduce(Action::Toggle);
    assert_eq!(app.selected_model().map(|m| m.id.clone()), before);
}

/// The palette is the only way to a screenshot, and it asks the caller
/// for it rather than writing the file from the reducer.
#[test]
fn the_screenshot_command_asks_the_caller_to_take_one() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    app.reduce(Action::Palette);
    for c in "screenshot".chars() {
        app.reduce(Action::PaletteChar(c));
    }
    let matched = app.palette_matches();
    assert_eq!(matched.len(), 1, "{matched:?}");
    assert_eq!(matched[0].id, CommandId::Screenshot);
    assert_eq!(matched[0].label, "Save a screenshot (SVG)");
    assert_eq!(matched[0].key, "", "no key binds it");
    assert!(app.reduce(Action::PaletteRun).is_none());
    assert_eq!(app.mode, Mode::Browse, "the palette is out of the picture");
    assert_eq!(app.take_effect(), Some(Effect::Screenshot));
}

#[test]
fn toggling_a_hidden_starter_only_hints() {
    let registry = registry();
    let mut app = App::new(&registry, &empty_state());
    // Force a starter row under the cursor without showing starters, the
    // way a stale cursor could.
    let starter_row = app
        .rows
        .iter()
        .position(|r| app.model(r).id == STARTER)
        .unwrap();
    app.visible.push(starter_row);
    app.cursor = app.visible.len() - 1;

    app.reduce(Action::Toggle);
    assert_eq!(app.message.as_deref(), Some(STARTER_HIDDEN_HINT));
    assert!(!app.has_changes());
}

/// Ctrl-C from the palette or the filter asks about unsaved edits, as it does
/// from the list: neither may drop a session's toggles without a word.
#[test]
fn quitting_from_the_palette_or_the_filter_asks_first_when_dirty() {
    let registry = registry();
    for open in [Action::Palette, Action::StartFilter] {
        let mut app = App::new(&registry, &empty_state());
        focus(&mut app, EWARS);
        app.reduce(Action::Toggle);
        app.reduce(open.clone());
        assert!(app.reduce(Action::Quit).is_none(), "{open:?}");
        assert_eq!(app.mode, Mode::ConfirmQuit, "{open:?}");
        assert!(app.dirty);
    }
    let mut clean = App::new(&registry, &empty_state());
    clean.reduce(Action::Palette);
    assert_eq!(clean.reduce(Action::Quit), Some(Outcome::Quit));
}
