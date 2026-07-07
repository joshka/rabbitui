//! Focus, traversal, and outcomes: the slice-3 flagship.
//!
//! Two [`Button`]s and a status [`Text`]. Tab / Shift-Tab cycle focus through the
//! buttons (drawn reversed when focused); Enter or Space activates the focused
//! button, which emits [`Outcome::Activated`]; the app reads that outcome in
//! `update` and names the last-activated button in the status line. `q` or
//! Escape quits — proving unconsumed events still reach `update` even while
//! focused widgets consume their own keys. Run with `cargo run --example focus`.
//!
//! Note (substrate gap): qwertty does not yet decode Shift-Tab, so backward
//! traversal is unavailable in the terminal until it lands; forward Tab wraps,
//! so every button is still reachable. See `crate::input`.

use std::ops::ControlFlow;

use rabbitui::app::{self, Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::style::{Color, Style};
use rabbitui_widgets::{Button, Text};

/// The app's owned state: which button was last activated, if any.
#[derive(Default)]
struct App {
    last: Option<&'static str>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run(App::default(), update, view).await?;
    Ok(())
}

/// Folds one update into the app: record activations, quit on `q`/Escape.
fn update(app: &mut App, update: Update<'_>) -> ControlFlow<()> {
    // Outcomes arrive keyed by the widget's root-relative key path.
    if update.outcome_for(&[key("ok")]) == Some(&Outcome::Activated) {
        app.last = Some("OK");
    }
    if update.outcome_for(&[key("cancel")]) == Some(&Outcome::Activated) {
        app.last = Some("Cancel");
    }

    // `q` / Escape are never consumed by the buttons, so they fall through here.
    if let Event::Input(input) = update.event() {
        if matches!(input.as_key().map(|k| k.key), Some(Key::Char('q') | Key::Escape)) {
            return ControlFlow::Break(());
        }
    }
    ControlFlow::Continue(())
}

/// Declares the two buttons and the status line.
fn view(app: &App, frame: &mut Frame<'_>) {
    let hint = Style::new().fg(Color::Indexed(245)).italic();

    let [_, ok_row, cancel_row, _, status_row, hint_row] = frame.rows([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ]);

    frame.widget(key("ok"), ok_row, &Button::new("OK"));
    frame.widget(key("cancel"), cancel_row, &Button::new("Cancel"));

    let status = match app.last {
        Some(name) => format!("last activated: {name}"),
        None => "last activated: (none)".to_string(),
    };
    frame.widget(key("status"), status_row, &Text::new(&status));
    frame.widget(
        key("hint"),
        hint_row,
        &Text::new("Tab to move focus, Enter/Space to activate, q to quit").style(hint),
    );
}
