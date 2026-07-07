//! A counter driven through the declared frame.
//!
//! Increments on `+` or space, decrements on `-`, and quits on `q`, Escape, or
//! Ctrl-C. State lives in a plain `i64`; each key folds into it in `update`, and
//! `view` declares a title and the current count as two [`Text`] widgets into
//! the frame. Run with `cargo run --example counter`.

use std::ops::ControlFlow;

use qwertty::{ControlInput, InputEvent};
use rabbitui::app::{self, Event};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::style::{Color, Style};
use rabbitui_widgets::Text;

/// The ETX byte (Ctrl-C) as delivered in raw mode. qwertty classifies it as
/// [`ControlInput::Other`] since it is not one of the named control variants.
const CTRL_C: u8 = 0x03;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run(0i64, update, view).await?;
    Ok(())
}

/// Folds one event into the count, or asks to quit.
fn update(count: &mut i64, event: Event) -> ControlFlow<()> {
    let Event::Input(input) = event else {
        return ControlFlow::Continue(());
    };
    match input {
        InputEvent::Text('+' | ' ') => *count += 1,
        InputEvent::Text('-') => *count -= 1,
        InputEvent::Text('q')
        | InputEvent::Control(ControlInput::Escape | ControlInput::Other(CTRL_C)) => {
            return ControlFlow::Break(());
        }
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
