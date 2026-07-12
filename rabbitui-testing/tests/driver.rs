//! Integration tests for the headless [`TestApp`] driver.
//!
//! These drive tiny views through the same [`StateStore`]/[`Frame`] path the
//! real loop uses, asserting on rendered lines, on state mutation across
//! frames, and on state-store persistence via a stateful probe widget.
//!
//! [`StateStore`]: rabbitui_core::store::StateStore
//! [`Frame`]: rabbitui_core::frame::Frame

use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::{Position, Size};
use rabbitui_core::id::key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::style::Style;
use rabbitui_core::widget::{RenderContext, Widget};
use rabbitui_testing::{TestApp, assert_snapshot};

/// A stateless label, painting borrowed text from the origin of its area.
struct Label<'a>(&'a str);

impl Widget for Label<'_> {
    type State = ();
    fn render(&self, (): &mut (), ctx: &mut RenderContext<'_>) {
        ctx.set_string(Position::ORIGIN, self.0, Style::new());
    }
}

/// A stateful widget that counts how many frames it has been declared in,
/// painting the running total — a probe for state-store persistence.
#[derive(Default)]
struct Renders(u32);

struct Probe;

impl Widget for Probe {
    type State = Renders;
    fn render(&self, state: &mut Renders, ctx: &mut RenderContext<'_>) {
        state.0 += 1;
        ctx.set_string(Position::ORIGIN, &state.0.to_string(), Style::new());
    }
}

/// The view under test: a title and the counter value on two rows.
fn counter_view(count: &i64, frame: &mut Frame<'_>) {
    let [title, _, value] = frame.rows([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ]);
    let value_text = format!("count: {count}");
    frame.widget(key("title"), title, &Label("Counter"));
    frame.widget(key("count"), value, &Label(&value_text));
}

#[test]
fn renders_mutates_and_rerenders() {
    let mut app = TestApp::new(Size::new(20, 3), 0i64);

    // First frame: initial state.
    app.render(counter_view);
    app.assert_buffer_lines(&["Counter", "", "count: 0"]);

    // Mutate state as an update would, re-render, and observe the new frame.
    app.send(|count| *count += 5, counter_view);
    assert_eq!(app.state(), &5);
    app.assert_buffer_lines(&["Counter", "", "count: 5"]);

    // Decrement past zero: negative counts render too.
    app.send(|count| *count -= 8, counter_view);
    app.assert_buffer_lines(&["Counter", "", "count: -3"]);
}

#[test]
fn state_store_persists_across_frames_via_a_stateful_probe() {
    let mut app = TestApp::new(Size::new(4, 1), ());

    // Each render advances the probe's own retained state, keyed by identity —
    // proof the driver runs the real StateStore path across frames.
    for expected in 1..=4 {
        app.render(|(), frame| frame.widget(key("probe"), frame.area(), &Probe));
        app.assert_buffer_lines(&[&expected.to_string()]);
    }
    // One identity held its state the whole time; nothing leaked.
    assert_eq!(app.store_len(), 1);
}

#[test]
fn snapshot_of_the_counter_view() {
    let mut app = TestApp::new(Size::new(20, 3), 42i64);
    app.render(counter_view);
    assert_snapshot!("counter_view", app.buffer_text());
}
