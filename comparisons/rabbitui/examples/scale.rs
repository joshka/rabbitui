//! A one-million-row [`Table`], proving virtualization with **zero app-side
//! caching**.
//!
//! This is the scale demonstration for the log-follower's [`Table`] adoption
//! (`docs/plans/wave-b2-virtualization.md`, "What good looks like"): the app holds
//! *no* rows — it hands the widget a [`table_from_fn`] source of 1,000,000 rows and
//! the widget calls `cell(row, col)` only for the screenful it paints. Scroll with
//! ↑/↓, PageUp/PageDown, Home/End, or the mouse wheel; End jumps to row 999,999
//! instantly because nothing between here and there is ever materialized.
//!
//! Contrast the survey's failures (Textual's 800× DataTable, Brick's uniform-height
//! wall): those frameworks force the app to hold or pre-measure every row. Here the
//! whole "data model" is a closure — run it and watch a million rows scroll in
//! constant memory and constant per-frame work.
//!
//! Run with `cd comparisons/rabbitui && cargo run --example scale`. `q` or Ctrl-C
//! quits.

use std::ops::ControlFlow;

use rabbitui::App;
use rabbitui::app::{Config, Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::theme::Role;
use rabbitui_widgets::{Column, Panel, Table, Text, table_from_fn};

/// How many synthetic rows the table spans. Nothing anywhere allocates this many
/// items — the number is only the source's reported `len()`.
const ROWS: usize = 1_000_000;

/// The empty app state: the [`Table`]'s selection and scroll live in
/// framework-owned widget state (keyed by identity), so the app itself carries no
/// per-row data at all — that is the whole point.
#[derive(Default)]
struct Scale;

/// The text of synthetic column `col` for `row`: 0 = seq, 1 = level, 2 = target,
/// 3 = message. Pure function of the row index — the source never stores a thing.
fn cell(row: usize, col: usize) -> String {
    const LEVELS: [&str; 4] = ["DEBUG", "INFO", "WARN", "ERROR"];
    const TARGETS: [&str; 5] = ["http", "cache", "auth", "db", "worker"];
    match col {
        0 => format!("#{row}"),
        1 => LEVELS[row % LEVELS.len()].to_string(),
        2 => TARGETS[row % TARGETS.len()].to_string(),
        _ => format!("synthetic log line {row} — generated on demand, never stored"),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Scale.run().await?;
    Ok(())
}

impl App for Scale {
    /// Capture the mouse so the wheel scrolls the table.
    fn config(&self) -> Config {
        Config::new().mouse(true)
    }

    /// Quit on `q` or Ctrl-C; everything else falls through to the focused table,
    /// which owns all the scrolling.
    fn update(&mut self, update: Update<'_>) -> ControlFlow<()> {
        if let Event::Input(input) = update.event() {
            if let Some(k) = input.as_key() {
                let quit = k.key == Key::Char('q') || (k.key == Key::Char('c') && k.modifiers.ctrl);
                if quit {
                    return ControlFlow::Break(());
                }
            }
        }
        ControlFlow::Continue(())
    }

    /// A titled panel wrapping the million-row table, plus a one-line hint.
    fn view(&self, frame: &mut Frame<'_>) {
        let [body, hint_row] = frame.rows([Constraint::Fill(1), Constraint::Length(1)]);

        let panel = Panel::new()
            .title(" 1,000,000 rows — zero app-side caching ")
            .padding(1)
            .focused(true);
        frame.widget(key("panel"), body, &panel);
        let inner = Panel::inner(body, &panel);

        // The entire data model: a closure. `cell` runs only for the painted
        // window, so a million rows cost one screenful of formatting per frame.
        let source = table_from_fn(ROWS, cell);
        let columns = vec![
            Column::new("#", Constraint::Length(10)),
            Column::new("level", Constraint::Length(6)),
            Column::new("target", Constraint::Length(9)),
            Column::new("message", Constraint::Fill(1)),
        ];
        frame.widget(key("table"), inner, &Table::new(source, columns));

        frame.widget(
            key("hint"),
            hint_row,
            &Text::new("↑↓ / PageUp·Down / Home·End / wheel: scroll   q / Ctrl-C: quit")
                .role(Role::Muted),
        );
    }
}
