//! Integration tests for slice-3 routing, focus, and outcomes.
//!
//! These drive a real app through [`TestApp`] and the shared [`route`] path
//! (`docs/adr/0006-input-focus-events.md`), proving the behaviors every prior
//! framework got wrong first: tab traversal order and wrap, focus surviving
//! re-declaration, dead-id focus recovery, outcome delivery, and unconsumed-event
//! fallthrough. Everything here is runtime-free: no tokio, no terminal.
//!
//! [`route`]: rabbitui_core::routing::route

use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::Size;
use rabbitui_core::id::{WidgetId, key};
use rabbitui_core::input::Key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::outcome::Outcome;
use rabbitui_testing::TestApp;
use rabbitui_widgets::Button;

/// A view with three stacked buttons keyed `a`, `b`, `c`.
fn three_buttons(_state: &(), frame: &mut Frame<'_>) {
    let [a, b, c, _] = frame.rows([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ]);
    frame.widget(key("a"), a, &Button::new("A"));
    frame.widget(key("b"), b, &Button::new("B"));
    frame.widget(key("c"), c, &Button::new("C"));
}

fn id(name: &str) -> WidgetId {
    WidgetId::ROOT.child(key(name))
}

#[test]
fn tab_traverses_declaration_order_and_wraps() {
    let mut app = TestApp::new(Size::new(6, 4), ());
    app.render(three_buttons);

    // No focus yet; the first Tab focuses the first focusable in declaration
    // order, then Tab walks a → b → c → (wrap) a.
    app.send_key(Key::Tab);
    assert_eq!(app.focus(), Some(id("a")));
    app.send_key(Key::Tab);
    assert_eq!(app.focus(), Some(id("b")));
    app.send_key(Key::Tab);
    assert_eq!(app.focus(), Some(id("c")));
    app.send_key(Key::Tab);
    assert_eq!(app.focus(), Some(id("a")), "Tab past the last focusable wraps to the first");
}

#[test]
fn backtab_traverses_backward_and_wraps() {
    let mut app = TestApp::new(Size::new(6, 4), ());
    app.render(three_buttons);

    // BackTab with no focus selects the last, then walks c → b → a → (wrap) c.
    app.send_key(Key::BackTab);
    assert_eq!(app.focus(), Some(id("c")));
    app.send_key(Key::BackTab);
    assert_eq!(app.focus(), Some(id("b")));
    app.send_key(Key::BackTab);
    assert_eq!(app.focus(), Some(id("a")));
    app.send_key(Key::BackTab);
    assert_eq!(app.focus(), Some(id("c")), "BackTab past the first wraps to the last");
}

#[test]
fn focus_survives_redeclaration() {
    let mut app = TestApp::new(Size::new(6, 4), ());
    app.render(three_buttons);
    app.send_key(Key::Tab);
    app.send_key(Key::Tab);
    assert_eq!(app.focus(), Some(id("b")));

    // Re-render the identical view many times: focus stays on `b` because
    // identity (the key path) is stable across frames.
    for _ in 0..5 {
        app.render(three_buttons);
    }
    assert_eq!(app.focus(), Some(id("b")), "focus is keyed by identity, not by frame");
}

#[test]
fn dead_id_focus_recovers_to_a_survivor() {
    // A view whose middle button can disappear.
    fn view(show_b: &bool, frame: &mut Frame<'_>) {
        let [a, b, c, _] = frame.rows([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ]);
        frame.widget(key("a"), a, &Button::new("A"));
        if *show_b {
            frame.widget(key("b"), b, &Button::new("B"));
        }
        frame.widget(key("c"), c, &Button::new("C"));
    }

    let mut app = TestApp::new(Size::new(6, 4), true);
    app.render(view);
    // Focus the middle button.
    app.send_key(Key::Tab);
    app.send_key(Key::Tab);
    assert_eq!(app.focus(), Some(id("b")));

    // Drop `b` from the next frame: its id is now absent from the facts.
    app.send(|show_b| *show_b = false, view);
    // Focus must recover to a surviving focusable, never a dead id.
    let recovered = app.focus().expect("focus recovers to a survivor");
    assert!(recovered == id("a") || recovered == id("c"), "recovered to {recovered:?}");
    assert_ne!(recovered, id("b"));
}

#[test]
fn outcome_is_delivered_for_the_focused_button() {
    // The app records the last-activated button, reading routing outcomes the
    // way `rabbitui::app::run` hands them to `update`.
    #[derive(Default)]
    struct App {
        last: Option<&'static str>,
    }

    fn view(_app: &App, frame: &mut Frame<'_>) {
        three_buttons(&(), frame);
    }

    let mut app = TestApp::new(Size::new(6, 4), App::default());
    app.render(view);
    app.send_key(Key::Tab); // focus `a`
    app.send_key(Key::Tab); // focus `b`

    let result = app.send_key(Key::Enter);
    assert!(result.consumed, "the focused button consumes Enter");
    // The outcome is keyed by the focused widget's id.
    assert_eq!(result.outcomes, vec![(id("b"), Outcome::Activated)]);

    // Fold it into app state, as `update` would via `outcome_for`.
    let activated = result
        .outcomes
        .iter()
        .any(|(w, o)| *w == id("b") && *o == Outcome::Activated);
    if activated {
        app.state_mut().last = Some("B");
    }
    assert_eq!(app.state().last, Some("B"));
}

#[test]
fn unconsumed_event_falls_through_to_the_app() {
    let mut app = TestApp::new(Size::new(6, 4), ());
    app.render(three_buttons);
    app.send_key(Key::Tab); // focus `a`

    // A key no button binds and no framework default claims: not consumed, no
    // outcomes — the app's `update` would see the raw Input event.
    let result = app.send_key(Key::Char('x'));
    assert!(!result.consumed);
    assert!(result.outcomes.is_empty());
}

#[test]
fn space_activates_the_focused_button() {
    let mut app = TestApp::new(Size::new(6, 4), ());
    app.render(three_buttons);
    app.send_key(Key::Tab); // focus `a`

    let result = app.send_key(Key::Char(' '));
    assert!(result.consumed);
    assert_eq!(result.outcomes, vec![(id("a"), Outcome::Activated)]);
}

#[test]
fn facts_are_duplicate_free_across_repeated_renders() {
    // Rendering the same view repeatedly must not accumulate duplicate facts:
    // each frame's facts describe exactly that frame. A duplicate id in one
    // frame would also trip the store's debug assertion, so a clean run here is
    // the positive proof.
    let mut app = TestApp::new(Size::new(6, 4), ());
    for _ in 0..10 {
        app.render(three_buttons);
    }
    // Three focusable buttons, one identity each — the store holds exactly three.
    assert_eq!(app.store_len(), 3, "one retained-state entry per distinct id, no duplicates");

    // And traversal visits each id exactly once before wrapping — proof the
    // focus order has no duplicates.
    app.send_key(Key::Tab);
    let first = app.focus();
    app.send_key(Key::Tab);
    let second = app.focus();
    app.send_key(Key::Tab);
    let third = app.focus();
    assert_ne!(first, second);
    assert_ne!(second, third);
    assert_ne!(first, third);
    app.send_key(Key::Tab);
    assert_eq!(app.focus(), first, "the fourth Tab wraps, confirming exactly three entries");
}
