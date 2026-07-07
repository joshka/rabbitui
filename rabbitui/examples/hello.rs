//! The walking skeleton: run the full loop to draw a greeting and quit.
//!
//! Draws "Hello, rabbitui!" and a styled hint through the declared frame, then
//! quits on `q` or Escape. Run with `cargo run --example hello`.

use std::ops::ControlFlow;

use rabbitui::app::{self, Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::style::{Color, Style};
use rabbitui_widgets::Text;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run(
        (),
        |(): &mut (), update: Update<'_>| {
            if quit_requested(&update) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
        |(): &(), frame: &mut Frame<'_>| {
            let title = Style::new().fg(Color::GREEN).bold();
            let hint = Style::new().fg(Color::Indexed(245)).italic();
            // A blank spacer row, the title, another spacer, then the hint.
            let [_, title_row, _, hint_row, _] = frame.rows([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Fill(1),
            ]);
            frame.widget(key("title"), title_row, &Text::new("Hello, rabbitui!").style(title));
            frame.widget(
                key("hint"),
                hint_row,
                &Text::new("press q or Esc to quit").style(hint),
            );
        },
    )
    .await?;
    Ok(())
}

/// Returns true if this update carries a quit key: `q` or Escape.
///
/// The event reaches `update` because no widget consumed it — the hello view has
/// no interactive widgets, so every key falls through to the app (ADR 0006's
/// unconsumed-event path).
fn quit_requested(update: &Update<'_>) -> bool {
    let Event::Input(input) = update.event() else {
        return false;
    };
    matches!(input.as_key().map(|k| k.key), Some(Key::Char('q') | Key::Escape))
}
