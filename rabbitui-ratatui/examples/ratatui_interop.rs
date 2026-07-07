//! ratatui widgets rendered inside a rabbitui app, next to native ones.
//!
//! The bridge (ADR 0010) is a drawing escape hatch: this example draws a native
//! rabbitui [`Text`] title, then three ratatui widgets — a `Paragraph` inside a
//! bordered `Block`, and a `Gauge` — each declared into the frame through
//! [`RatatuiWidget`], alongside a native rabbitui hint line. Press `+`/`-` to
//! move the gauge, `q` or Escape to quit. Run with
//! `cargo run -p rabbitui-ratatui --example ratatui_interop`.
//!
//! The ratatui widgets carry cells only — no focus, no theming — so they sit as
//! inert styled rectangles among the native widgets, exactly as ADR 0010
//! §Decision.5 describes.

use std::ops::ControlFlow;

use rabbitui::app::{self, Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::{Constraint, center, split_rows};
use rabbitui_core::theme::Role;
use rabbitui_ratatui::RatatuiWidget;
use rabbitui_widgets::{Panel, Text};
use ratatui::style::{Color as RatColor, Modifier, Style as RatStyle};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, Paragraph, Wrap};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // State: the gauge's fill percentage, 0..=100.
    app::run(40u16, update, view).await?;
    Ok(())
}

/// Folds a key into the gauge percentage, or asks to quit.
fn update(percent: &mut u16, update: Update<'_>) -> ControlFlow<()> {
    let Event::Input(input) = update.event() else {
        return ControlFlow::Continue(());
    };
    match input.as_key().map(|k| k.key) {
        Some(Key::Char('+' | ' ')) => *percent = (*percent + 5).min(100),
        Some(Key::Char('-')) => *percent = percent.saturating_sub(5),
        Some(Key::Char('q') | Key::Escape) => return ControlFlow::Break(()),
        _ => {}
    }
    ControlFlow::Continue(())
}

/// Declares the UI: a native title, a bridged bordered paragraph, a bridged
/// gauge, and a native hint — native and ratatui widgets side by side.
fn view(percent: &u16, frame: &mut Frame<'_>) {
    // A native rabbitui Panel frames the whole demo; the ratatui widgets (each
    // with its own ratatui Block) sit inside it, native and bridged chrome nested.
    let area = center(frame.area(), 52, 15);
    let outer = Panel::new().title("interop").padding(1);
    frame.widget(key("panel"), area, &outer);
    let inner = Panel::inner(area, &outer);

    let [title_row, _, ratatui_panel, _, gauge_row, _, hint_row] = split_rows(
        inner,
        [
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Fill(1),
        ],
    );

    // A native rabbitui widget, styled by theme role.
    frame.widget(
        key("title"),
        title_row,
        &Text::new("rabbitui + ratatui interop").role(Role::Accent),
    );

    // A ratatui Paragraph inside a ratatui bordered Block — a classic ratatui
    // drawing, dropped into the rabbitui frame through the bridge.
    let body = Paragraph::new(vec![
        Line::from("This panel is a ratatui Block + Paragraph,"),
        Line::from(vec![
            Span::raw("rendered through the "),
            Span::styled(
                "rabbitui-ratatui",
                RatStyle::default()
                    .fg(RatColor::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" bridge."),
        ]),
    ])
    .block(Block::bordered().title("ratatui panel"))
    .wrap(Wrap { trim: true });
    frame.widget(key("body"), ratatui_panel, &RatatuiWidget::new(body));

    // A ratatui Gauge, driven by app state.
    let gauge = Gauge::default()
        .block(Block::bordered().title("gauge"))
        .gauge_style(RatStyle::default().fg(RatColor::Magenta))
        .percent(*percent);
    frame.widget(key("gauge"), gauge_row, &RatatuiWidget::new(gauge));

    // A native rabbitui hint line.
    frame.widget(
        key("hint"),
        hint_row,
        &Text::new("+/-: move the gauge   q/Esc: quit").role(Role::Muted),
    );
}
