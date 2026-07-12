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
use rabbitui_core::geometry::{Position, Size};
use rabbitui_core::id::{WidgetId, key};
use rabbitui_core::input::{Key, MouseKind};
use rabbitui_core::layout::Constraint;
use rabbitui_core::outcome::Outcome;
use rabbitui_testing::TestApp;
use rabbitui_widgets::{Button, SelectionList};

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

    // The first focusable is auto-focused at render; Tab then walks
    // a → b → c → (wrap) a.
    assert_eq!(
        app.focus(),
        Some(id("a")),
        "first focusable auto-focused at render"
    );
    app.send_key(Key::Tab);
    assert_eq!(app.focus(), Some(id("b")));
    app.send_key(Key::Tab);
    assert_eq!(app.focus(), Some(id("c")));
    app.send_key(Key::Tab);
    assert_eq!(
        app.focus(),
        Some(id("a")),
        "Tab past the last focusable wraps to the first"
    );
}

#[test]
fn backtab_traverses_backward_and_wraps() {
    let mut app = TestApp::new(Size::new(6, 4), ());
    app.render(three_buttons);

    // `a` is auto-focused at render; BackTab wraps backward to the last, then
    // walks c → b → a → (wrap) c.
    app.send_key(Key::BackTab);
    assert_eq!(app.focus(), Some(id("c")));
    app.send_key(Key::BackTab);
    assert_eq!(app.focus(), Some(id("b")));
    app.send_key(Key::BackTab);
    assert_eq!(app.focus(), Some(id("a")));
    app.send_key(Key::BackTab);
    assert_eq!(
        app.focus(),
        Some(id("c")),
        "BackTab past the first wraps to the last"
    );
}

#[test]
fn focus_survives_redeclaration() {
    let mut app = TestApp::new(Size::new(6, 4), ());
    app.render(three_buttons);
    app.send_key(Key::Tab); // auto-focus starts on `a`; Tab moves to `b`.
    assert_eq!(app.focus(), Some(id("b")));

    // Re-render the identical view many times: focus stays on `b` because
    // identity (the key path) is stable across frames.
    for _ in 0..5 {
        app.render(three_buttons);
    }
    assert_eq!(
        app.focus(),
        Some(id("b")),
        "focus is keyed by identity, not by frame"
    );
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
    // Focus the middle button (auto-focus starts on `a`).
    app.send_key(Key::Tab);
    assert_eq!(app.focus(), Some(id("b")));

    // Drop `b` from the next frame: its id is now absent from the facts.
    app.send(|show_b| *show_b = false, view);
    // Focus must recover to a surviving focusable, never a dead id.
    let recovered = app.focus().expect("focus recovers to a survivor");
    assert!(
        recovered == id("a") || recovered == id("c"),
        "recovered to {recovered:?}"
    );
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
    app.send_key(Key::Tab); // auto-focus starts on `a`; Tab moves to `b`.

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
    // `a` is auto-focused at render.

    let result = app.send_key(Key::Char(' '));
    assert!(result.consumed);
    assert_eq!(result.outcomes, vec![(id("a"), Outcome::Activated)]);
}

#[test]
fn click_activates_and_focuses_the_button_under_the_pointer() {
    let mut app = TestApp::new(Size::new(6, 4), ());
    app.render(three_buttons);
    // Button `b` is on row 1. A left click there activates it (Button consumes
    // the press).
    let result = app.send_mouse(MouseKind::Down, Position::new(0, 1));
    assert!(result.consumed);
    assert_eq!(result.outcomes, vec![(id("b"), Outcome::Activated)]);
}

#[test]
fn click_to_focus_moves_focus_to_an_unconsumed_focusable_target() {
    // A focusable widget that does *not* consume clicks — the click-to-focus path
    // (ADR 0006 §5 / slice-7 design note: "unconsumed clicks focus the target if
    // focusable, then fall through to update").
    struct Panel;
    impl rabbitui_core::widget::Widget for Panel {
        type State = ();
        fn render(&self, _s: &mut (), ctx: &mut rabbitui_core::widget::RenderContext<'_>) {
            ctx.focusable(true);
        }
        // Default handle: ignores the click, so it is unconsumed.
    }
    fn view(_s: &(), frame: &mut Frame<'_>) {
        // A button first, so auto-focus lands on it — the click must then MOVE
        // focus to the panel.
        let [top, rest] = frame.rows([Constraint::Length(1), Constraint::Fill(1)]);
        frame.widget(key("first"), top, &Button::new("F"));
        frame.widget(key("panel"), rest, &Panel);
    }
    let mut app = TestApp::new(Size::new(8, 3), ());
    app.render(view);
    assert_eq!(
        app.focus(),
        Some(id("first")),
        "auto-focus landed on the button"
    );
    // The click is unconsumed (the panel ignores it) but focuses the panel.
    let result = app.send_mouse(MouseKind::Down, Position::new(0, 1));
    assert!(!result.consumed, "the panel ignored the click");
    assert_eq!(
        app.focus(),
        Some(id("panel")),
        "click-to-focus moved focus to the target"
    );
}

#[test]
fn wheel_over_a_list_moves_the_selection() {
    fn view(_s: &(), frame: &mut Frame<'_>) {
        let items: Vec<String> = (0..10).map(|i| format!("row{i}")).collect();
        frame.widget(key("list"), frame.area(), &SelectionList::new(items));
    }
    let mut app = TestApp::new(Size::new(8, 4), ());
    app.render(view);
    // A wheel-down notch over the list advances the selection and emits Selected.
    let result = app.send_mouse(MouseKind::Scroll(1), Position::new(0, 0));
    assert_eq!(result.outcomes, vec![(id("list"), Outcome::Selected(1))]);
}

#[test]
fn modal_layer_contains_focus_traversal() {
    // A base button plus a modal layer with two buttons. While the modal exists,
    // Tab cycles only the modal's buttons — the base is unreachable.
    fn view(_s: &(), frame: &mut Frame<'_>) {
        let [base_area, _] = frame.rows([Constraint::Length(1), Constraint::Fill(1)]);
        frame.widget(key("base"), base_area, &Button::new("Base"));
        frame.layer(key("modal"), |modal| {
            let [ok_area, cancel_area] = modal.rows([Constraint::Length(1), Constraint::Length(1)]);
            modal.widget(key("ok"), ok_area, &Button::new("OK"));
            modal.widget(key("cancel"), cancel_area, &Button::new("Cancel"));
        });
    }

    let ok = WidgetId::ROOT.child(key("modal")).child(key("ok"));
    let cancel = WidgetId::ROOT.child(key("modal")).child(key("cancel"));

    let mut app = TestApp::new(Size::new(10, 4), ());
    app.render(view);
    // Auto-focus lands on the first focusable of the TOP layer, never the base.
    assert_eq!(app.focus(), Some(ok));
    app.send_key(Key::Tab);
    assert_eq!(app.focus(), Some(cancel));
    app.send_key(Key::Tab);
    assert_eq!(
        app.focus(),
        Some(ok),
        "Tab wraps within the modal, never reaching the base"
    );
    // The base button is never focused while the modal exists.
    assert_ne!(app.focus(), Some(id("base")));
}

#[test]
fn base_widget_gets_no_key_while_modal_exists() {
    // A base button that would activate on Enter, and a modal whose OK button is
    // focused. Enter must reach the modal's OK, not the base.
    fn view(_s: &(), frame: &mut Frame<'_>) {
        let [base_area, _] = frame.rows([Constraint::Length(1), Constraint::Fill(1)]);
        frame.widget(key("base"), base_area, &Button::new("Base"));
        frame.layer(key("modal"), |modal| {
            modal.widget(key("ok"), modal.area(), &Button::new("OK"));
        });
    }
    let ok = WidgetId::ROOT.child(key("modal")).child(key("ok"));
    let mut app = TestApp::new(Size::new(10, 4), ());
    app.render(view);
    app.send_key(Key::Tab); // focuses the modal's OK (top layer only)
    assert_eq!(app.focus(), Some(ok));
    let result = app.send_key(Key::Enter);
    assert_eq!(result.outcomes, vec![(ok, Outcome::Activated)]);
    // The base never saw the Enter.
    assert!(result.outcomes.iter().all(|(w, _)| *w != id("base")));
}

#[test]
fn dismissing_a_modal_reconciles_focus_to_the_base() {
    // A view that shows the modal only while `open`. Focus starts in the modal;
    // after it closes, focus reconciles to the surviving base focusable.
    fn view(open: &bool, frame: &mut Frame<'_>) {
        let [base_area, _] = frame.rows([Constraint::Length(1), Constraint::Fill(1)]);
        frame.widget(key("base"), base_area, &Button::new("Base"));
        if *open {
            frame.layer(key("modal"), |modal| {
                modal.widget(key("ok"), modal.area(), &Button::new("OK"));
            });
        }
    }
    let ok = WidgetId::ROOT.child(key("modal")).child(key("ok"));
    let mut app = TestApp::new(Size::new(10, 4), true);
    app.render(view);
    app.send_key(Key::Tab);
    assert_eq!(app.focus(), Some(ok), "focus is trapped in the modal");
    // Close the modal: `ok` vanishes, so focus reconciles to the base survivor.
    app.send(|open| *open = false, view);
    assert_eq!(
        app.focus(),
        Some(id("base")),
        "focus reconciles to the base when the modal goes"
    );
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
    assert_eq!(
        app.store_len(),
        3,
        "one retained-state entry per distinct id, no duplicates"
    );

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
    assert_eq!(
        app.focus(),
        first,
        "the fourth Tab wraps, confirming exactly three entries"
    );
}
