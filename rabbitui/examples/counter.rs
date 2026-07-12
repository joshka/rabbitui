//! A counter driven through the declared frame.
//!
//! Increments on `+` or space, decrements on `-`, and quits on `q` or Escape.
//! State lives in the `Counter` struct; each key folds into it in `update`, and
//! `view` declares a title and the current count as two [`Text`] widgets. The
//! counter has no interactive widgets, so every key falls through routing to
//! `update` (ADR 0006's unconsumed-event path). Run with `cargo run --example
//! counter`.

use std::ops::ControlFlow;

use rabbitui::App;
use rabbitui::app::{Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::{Constraint, center, split_rows};
use rabbitui_core::theme::Role;
use rabbitui_widgets::{Panel, Text};

/// The counter's owned state.
#[derive(Default)]
struct Counter {
    count: i64,
}

impl App for Counter {
    /// Folds one update into the count, or asks to quit.
    fn update(&mut self, update: Update<'_>) -> ControlFlow<()> {
        let Event::Input(input) = update.event() else {
            return ControlFlow::Continue(());
        };
        match input.as_key().map(|k| k.key) {
            Some(Key::Char('+' | ' ')) => self.count += 1,
            Some(Key::Char('-')) => self.count -= 1,
            Some(Key::Char('q') | Key::Escape) => return ControlFlow::Break(()),
            _ => {}
        }
        ControlFlow::Continue(())
    }

    /// Declares the counter UI inside a centered, titled panel: the count value
    /// centered, a hint muted at the foot.
    fn view(&self, frame: &mut Frame<'_>) {
        let area = center(frame.area(), 44, 7);
        let panel = Panel::new().title("counter").padding(1);
        frame.widget(key("panel"), area, &panel);

        let inner = Panel::inner(area, &panel);
        let [_, count_row, _, hint_row] = split_rows(
            inner,
            [
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Fill(1),
                Constraint::Length(1),
            ],
        );

        let count_text = format!("count: {}", self.count);
        frame.widget(
            key("count"),
            count_row,
            &Text::new(&count_text).role(Role::Accent),
        );
        let hint = "+/space: add   -: subtract   q: quit";
        frame.widget(key("hint"), hint_row, &Text::new(hint).role(Role::Muted));
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Counter::default().run().await?;
    Ok(())
}
