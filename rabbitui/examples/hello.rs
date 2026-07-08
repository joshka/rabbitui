//! The walking skeleton: run the full loop to draw a greeting and quit.
//!
//! Draws "Hello, rabbitui!" inside a centered, titled [`Panel`] with a muted hint
//! line, then quits on `q` or Escape. The panel is the pre-composition backdrop
//! (`rabbitui_widgets::panel`): declare it first, then declare content into its
//! [`Panel::inner`] area. Run with `cargo run --example hello`.

use std::ops::ControlFlow;

use rabbitui::app::{self, Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::{Constraint, center, split_rows};
use rabbitui_core::theme::Role;
use rabbitui_widgets::{Panel, Text};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run((), update, view).await?;
    Ok(())
}

/// Quits on `q` or Escape; every other event falls through untouched.
///
/// The event reaches `update` because no widget consumed it — the hello view has
/// no interactive widgets, so every key falls through to the app (ADR 0006's
/// unconsumed-event path).
fn update((): &mut (), update: Update<'_>) -> ControlFlow<()> {
    if let Event::Input(input) = update.event()
        && matches!(
            input.as_key().map(|k| k.key),
            Some(Key::Char('q') | Key::Escape)
        ) {
            return ControlFlow::Break(());
        }
    ControlFlow::Continue(())
}

/// Declares the greeting inside a centered, titled panel.
fn view((): &(), frame: &mut Frame<'_>) {
    // A small panel, centered on the screen: the app no longer starts flush at
    // the top-left corner spanning the void.
    let area = center(frame.area(), 40, 7);
    let panel = Panel::new().title("hello").padding(1);
    frame.widget(key("panel"), area, &panel);

    // Content goes into the panel's inner area: a greeting, a spacer, the hint.
    let inner = Panel::inner(area, &panel);
    let [greeting_row, _, hint_row] = split_rows(
        inner,
        [
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ],
    );
    frame.widget(
        key("greeting"),
        greeting_row,
        &Text::new("Hello, rabbitui!").role(Role::Accent),
    );
    frame.widget(
        key("hint"),
        hint_row,
        &Text::new("press q to quit").role(Role::Muted),
    );
}
