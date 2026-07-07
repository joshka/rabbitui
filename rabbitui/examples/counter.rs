//! A counter driven through the declared frame.
//!
//! Increments on `+` or space, decrements on `-`, and quits on `q` or Escape.
//! State lives in a plain `i64`; each key folds into it in `update`, and `view`
//! declares a title and the current count as two [`Text`] widgets. The counter
//! has no interactive widgets, so every key falls through routing to `update`
//! (ADR 0006's unconsumed-event path). Run with `cargo run --example counter`.

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
    app::run(0i64, update, view).await?;
    Ok(())
}

/// Folds one update into the count, or asks to quit.
fn update(count: &mut i64, update: Update<'_>) -> ControlFlow<()> {
    let Event::Input(input) = update.event() else {
        return ControlFlow::Continue(());
    };
    match input.as_key().map(|k| k.key) {
        Some(Key::Char('+' | ' ')) => *count += 1,
        Some(Key::Char('-')) => *count -= 1,
        Some(Key::Char('q') | Key::Escape) => return ControlFlow::Break(()),
        _ => {}
    }
    ControlFlow::Continue(())
}

/// Declares the counter UI: a title row and the current count, each a [`Text`].
fn view(count: &i64, frame: &mut Frame<'_>) {
    let title = Style::new().fg(Color::GREEN).bold();
    let value = Style::new().fg(Color::YELLOW).bold();
    let hint = Style::new().fg(Color::Indexed(245)).italic();

    let [title_row, _, count_row, _, hint_row] = frame.rows([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ]);

    let count_text = format!("count: {count}");
    frame.widget(key("title"), title_row, &Text::new("Counter").style(title));
    frame.widget(key("count"), count_row, &Text::new(&count_text).style(value));
    frame.widget(
        key("hint"),
        hint_row,
        &Text::new("press +/space to add, - to subtract, q to quit").style(hint),
    );
}
