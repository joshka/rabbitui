//! A todo list: the slice-4 flagship for widgets and theming.
//!
//! A [`TextInput`] adds a todo on Enter, a [`SelectionList`] shows the todos,
//! and a status line reports the count and hints. Tab / Shift-Tab move focus
//! between the two widgets (the focused one draws its focus style); `d` deletes
//! the selected todo (an app-level binding on an unconsumed key while the list is
//! focused); `q` or Ctrl-C quits. Run with `cargo run --example todo`.
//!
//! # What it exercises
//!
//! - Two focusables of *different* types in one frame, with Tab traversal.
//! - Outcomes driving app state: [`Outcome::Changed`] tracks the input draft,
//!   [`Outcome::Submitted`] commits it, [`Outcome::Selected`]/[`Outcome::Activated`]
//!   report list moves.
//! - Re-render from mutated app state (a new todo appears in the list).
//! - Theme roles end to end (the whole UI is styled by role, re-skinnable with a
//!   theme file — see `App::theme` / `App::theme_file`).
//!
//! # The uncontrolled-input workaround, honestly
//!
//! The value lives in the [`TextInput`]'s retained state, not the app (slice-4
//! design note). The app cannot force-clear it until widget commands land (slice
//! 6), so after a submit it **re-keys** the input: the key carries a generation
//! counter, and bumping it gives the widget fresh (empty) state. This is the
//! documented interim pattern; it works cleanly here because a submit is exactly
//! when a fresh field is wanted.
//!
//! Note (substrate gap): qwertty does not yet decode Shift-Tab, Home/End, or a
//! forward Delete key, so backward traversal and those edit keys are unavailable
//! in the terminal until it lands; forward Tab wraps, so both widgets are still
//! reachable. See `rabbitui::input`.

use std::ops::ControlFlow;

use rabbitui::App;
use rabbitui::app::{Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::theme::{Role, Theme};
use rabbitui_widgets::{SelectionList, Text, TextInput};

/// The app's owned state: the todos, the current input draft, and the input's
/// generation (bumped to re-key — and thus clear — the input after a submit).
#[derive(Default)]
struct App0 {
    todos: Vec<String>,
    /// The current text of the input, tracked from `Changed` outcomes so a
    /// `Submitted` (which carries no payload) can commit it.
    draft: String,
    /// Bumped on submit to re-key the `TextInput`, giving it fresh empty state.
    input_generation: u64,
    /// The list's selected index, mirrored from `Selected` outcomes so `d`
    /// deletes the highlighted row.
    selected: usize,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A pretty preset by default; pass a path to `theme_file` to hot-reload a
    // TOML theme in debug builds.
    App::new(App0::default(), update, view)
        .theme(Theme::catppuccin_mocha())
        .run()
        .await?;
    Ok(())
}

/// Folds one update into the app.
fn update(app: &mut App0, update: Update<'_>) -> ControlFlow<()> {
    // Track the input's draft on every edit; commit it on submit.
    if let Some(Outcome::Changed(value)) = update.outcome_for(&[input_key(app)]) {
        app.draft = value.clone();
    }
    if update.outcome_for(&[input_key(app)]) == Some(&Outcome::Submitted) {
        let todo = app.draft.trim().to_string();
        if !todo.is_empty() {
            app.todos.push(todo);
        }
        // Re-key the input to clear it (uncontrolled workaround) and reset the
        // draft we track alongside it.
        app.input_generation += 1;
        app.draft.clear();
    }

    // `d` deletes the selected todo. The `TextInput` consumes every printable
    // char while focused, so a `d` only reaches the app when the list (or
    // nothing) is focused — the app-level binding on an unconsumed key the design
    // note calls for. Selection is widget state, mirrored into `app.selected`
    // from `Selected` outcomes.
    if let Event::Input(input) = update.event() {
        match input.as_key().map(|k| k.key) {
            Some(Key::Char('d')) if !app.todos.is_empty() => {
                let index = app.selected.min(app.todos.len() - 1);
                app.todos.remove(index);
                app.selected = app.selected.min(app.todos.len().saturating_sub(1));
            }
            Some(Key::Char('q') | Key::Escape) => return ControlFlow::Break(()),
            _ => {}
        }
    }

    // Mirror the list's selection so `d` deletes the right row.
    if let Some(Outcome::Selected(index)) = update.outcome_for(&[key("list")]) {
        app.selected = *index;
    }

    ControlFlow::Continue(())
}

/// Declares the input, the list, and the status line.
fn view(app: &App0, frame: &mut Frame<'_>) {
    let [input_row, _gap, list_area, status_row, hint_row] = frame.rows([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);

    // The input is re-keyed by generation so a submit clears it.
    frame.widget(input_key(app), input_row, &TextInput::new().placeholder("Add a todo…"));

    // The list borrows the app's todos as its source.
    frame.widget(key("list"), list_area, &SelectionList::new(app.todos.clone()));

    let status = format!("{} todo(s)", app.todos.len());
    frame.widget(key("status"), status_row, &Text::new(&status).role(Role::Accent));

    frame.widget(
        key("hint"),
        hint_row,
        &Text::new("Tab: focus  Enter: add  d: delete  q: quit").role(Role::Muted),
    );
}

/// The input's key for this frame, carrying the generation so a submit re-keys
/// (and clears) it.
fn input_key(app: &App0) -> rabbitui_core::id::Key {
    key("input").index(usize::try_from(app.input_generation).unwrap_or(usize::MAX))
}
