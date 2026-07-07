//! The walking skeleton: run the full loop to draw a greeting and quit.
//!
//! Draws "Hello, rabbitui!" and a styled hint through the declared frame, then
//! quits on `q`, Escape, or Ctrl-C. Run with `cargo run --example hello`.

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
    app::run(
        (),
        |(): &mut (), event: Event| {
            if quit_requested(&event) {
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
                &Text::new("press q, Esc, or Ctrl-C to quit").style(hint),
            );
        },
    )
    .await?;
    Ok(())
}

/// Returns true if `event` is one of the quit keys: `q`, Escape, or Ctrl-C.
fn quit_requested(event: &Event) -> bool {
    let Event::Input(input) = event else {
        return false;
    };
    matches!(
        input,
        InputEvent::Text('q')
            | InputEvent::Control(ControlInput::Escape)
            | InputEvent::Control(ControlInput::Other(CTRL_C))
    )
}
