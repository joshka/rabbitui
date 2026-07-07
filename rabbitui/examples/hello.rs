//! The walking skeleton: run the full loop to draw a greeting and quit.
//!
//! Draws "Hello, rabbitui!" and a styled hint, then quits on `q`, Escape, or
//! Ctrl-C. Run with `cargo run --example hello`.

use std::ops::ControlFlow;

use rabbitui::app::{self, Event};
use rabbitui_core::buffer::Buffer;
use rabbitui_core::geometry::Position;
use rabbitui_core::style::{Color, Style};
use qwertty::{ControlInput, InputEvent};

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
        |(): &(), buffer: &mut Buffer| {
            let title = Style::new().fg(Color::GREEN).bold();
            let hint = Style::new().fg(Color::Indexed(245)).italic();
            buffer.set_string(Position::new(2, 1), "Hello, rabbitui!", title);
            buffer.set_string(Position::new(2, 3), "press q, Esc, or Ctrl-C to quit", hint);
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
