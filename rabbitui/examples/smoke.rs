//! Substrate smoke test: open the terminal, draw styled text, wait for a key,
//! restore. Run with `cargo run --example smoke`.

use rabbitui::Terminal;
use rabbitui::engine::AltEngine;
use rabbitui_core::geometry::Position;
use rabbitui_core::style::{Color, Style};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::open().await?;

    // Screen setup is the engine's job now (ADR 0013): enter the alternate screen
    // through the alt engine's entry bytes, so the demo draws on a dedicated
    // buffer and the terminal restores the prior screen on exit.
    let mut engine = AltEngine::new();
    terminal.write_bytes(&engine.enter()).await?;

    let size = terminal.size()?;
    let title = Style::new().fg(Color::GREEN).bold();
    let hint = Style::new().fg(Color::Indexed(245)).italic();

    terminal
        .print_styled(Position::new(2, 1), "rabbitui smoke test", title)
        .await?;
    terminal
        .print_styled(
            Position::new(2, 2),
            &format!("terminal size: {}x{}", size.width, size.height),
            Style::new(),
        )
        .await?;
    terminal
        .print_styled(Position::new(2, 4), "press any key to exit", hint)
        .await?;
    terminal.flush().await?;

    terminal.next_event().await?;

    terminal.write_bytes(&engine.leave()).await?;
    terminal.close().await?;
    Ok(())
}
