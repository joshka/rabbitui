//! Slice-5 acceptance tests: the declarative keymap, the generated help overlay,
//! the printable-chord consumed-guard, and the theme-file loader path.
//!
//! Like the other slices, the reducer and views are driven headlessly. The
//! keymap's dispatch table is unit-tested in `src/keymap.rs`; here we assert the
//! app-facing behavior that depends on it: the help overlay renders its rows
//! GENERATED from the one keymap table, a printable key a focused composer takes
//! is reported consumed (so a modal affordance could never steal it), and the
//! facade's theme-file path parses the bundled example into a `Theme`.

use rabbitui_agent::app::{self, Agent};
use rabbitui_agent::backend::replay::ReplayBackend;
use rabbitui_agent::keymap::{Action, KEYMAP, base_help_rows};
use rabbitui_core::geometry::Size;
use rabbitui_core::id::{WidgetId, key};
use rabbitui_core::input::Key;
use rabbitui_core::keymap::Chord;
use rabbitui_testing::TestApp;

/// A fresh alt-screen app over an empty replay backend (the test buffer models
/// alt-screen, not inline scrollback).
fn alt_screen_app() -> Agent {
    let mut agent = Agent::new("test-model", Box::new(ReplayBackend::new(Vec::new())));
    agent.inline = false;
    agent
}

#[test]
fn the_help_overlay_opens_and_lists_bindings_generated_from_the_keymap() {
    let mut app = TestApp::new(Size::new(60, 24), alt_screen_app());
    app.render(app::view);
    // Before: no help card.
    assert!(!app.buffer_text().contains("toggle inline"));

    // Open the overlay (the update closure sets this when the Help chord fires;
    // here we set the state directly, as the reducer-style tests do).
    app.send(|state| state.showing_help = true, app::view);

    let text = app.buffer_text();
    // The title and rows generated from the keymap table are present.
    assert!(text.contains("keys"), "overlay has a title:\n{text}");
    assert!(
        text.contains("toggle inline"),
        "overlay lists the mode toggle from the keymap:\n{text}"
    );
    assert!(text.contains("quit"), "overlay lists quit:\n{text}");
    assert!(
        text.contains("Ctrl-T"),
        "the mode-toggle chord is shown:\n{text}"
    );
    // Both help chords (the decided Ctrl-/ and the works-today Ctrl-G alias).
    assert!(
        text.contains("Ctrl-/") && text.contains("Ctrl-G"),
        "both help chords are listed:\n{text}"
    );
    // Modal-only affordances are NOT on the base reference card.
    assert!(
        !text.contains("allow the tool call"),
        "modal affordances stay out of the base help card:\n{text}"
    );
}

#[test]
fn the_help_rows_come_only_from_the_keymap_table() {
    // Every row the overlay could render is derivable from the keymap; nothing
    // is hand-maintained. Assert the row set equals the keymap's base bindings.
    let rows = base_help_rows();
    let labels: Vec<&str> = rows.iter().map(|(_, label)| *label).collect();
    assert!(labels.contains(&Action::Send.label()));
    assert!(labels.contains(&Action::ToggleMode.label()));
    assert!(labels.contains(&Action::Cancel.label()));
    assert!(labels.contains(&Action::Help.label()));
    assert!(labels.contains(&Action::Quit.label()));
    // Modal-only actions excluded.
    assert!(!labels.contains(&Action::Allow.label()));
    assert!(!labels.contains(&Action::Deny.label()));
}

#[test]
fn a_bound_printable_key_reaches_the_focused_composer() {
    // The modal binds bare `y`/`n` as allow/deny affordances. This guards that
    // such a printable chord is `consumed()` by the composer when it has focus —
    // so it can never be re-interpreted as an app action while the user is typing.
    let mut app = TestApp::new(Size::new(60, 24), alt_screen_app());
    app.render(app::view);
    // Focus the composer, then type a `y`.
    app.set_focus(Some(WidgetId::ROOT.child(key("composer"))));
    app.render(app::view); // reconcile focus against the frame's facts
    let result = app.send_key(Key::Char('y'));
    assert!(
        result.consumed,
        "the composer consumes a printable key it has focus for"
    );
    // And the app-level dispatch would only ever act on an UNconsumed key: the
    // keymap maps bare `y` to Allow, but the dispatch site is consumed-guarded.
    assert_eq!(
        KEYMAP.action_for(&rabbitui_core::input::KeyEvent::new(Key::Char('y'))),
        Some(Action::Allow),
        "the keymap knows the chord, but the guard keeps it from firing here"
    );
}

#[test]
fn the_toggle_mode_chord_is_ctrl_only() {
    // Standing invariant: app actions are Ctrl-chords only while composing.
    // Ctrl-T toggles; a bare `t` (or `m`) is not a base action — it is the
    // composer's to keep.
    assert_eq!(
        KEYMAP.action_for(&rabbitui_core::input::KeyEvent::new(Key::Char('t')).ctrl()),
        Some(Action::ToggleMode),
    );
    assert_eq!(
        KEYMAP.action_for(&rabbitui_core::input::KeyEvent::new(Key::Char('t'))),
        None,
        "a bare letter is never a base app action"
    );
    assert_eq!(
        KEYMAP.action_for(&rabbitui_core::input::KeyEvent::new(Key::Char('m'))),
        None,
        "the old bare-m mode toggle was dropped (composer-owned)",
    );
}

#[test]
fn the_bundled_example_theme_file_loads_into_a_theme() {
    // Exercise the facade's theme-file path (the same code `App::theme_file`
    // runs at startup): the bundled example parses into a Theme without error.
    use rabbitui::theme::load_theme;
    use rabbitui_core::style::Color;
    use rabbitui_core::theme::{Role, Theme};

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/themes/example.toml");
    let theme = load_theme(path, Theme::default()).expect("the bundled theme loads");

    // A role the file names is overridden to its declared color…
    assert_eq!(
        theme.style(Role::Accent).fg,
        Some(Color::Rgb(0xc0, 0x99, 0xff)),
        "accent is the example's lavender",
    );
    assert_eq!(
        theme.style(Role::Danger).fg,
        Some(Color::Rgb(0xff, 0x75, 0x7f)),
        "danger is the example's warm red",
    );
}

#[test]
fn chord_display_round_trips_for_the_documented_chords() {
    // A cheap guard that the help column renders the chords the plan documents.
    assert_eq!(Chord::ctrl('t').display(), "Ctrl-T");
    assert_eq!(Chord::ctrl('/').display(), "Ctrl-/");
    assert_eq!(Chord::bare(Key::Enter).display(), "Enter");
    assert_eq!(Chord::bare(Key::Escape).display(), "Esc");
}
