//! Integration test: the full todo flow through the headless [`TestApp`].
//!
//! Drives the same shape as `examples/todo.rs` — a [`TextInput`] and a
//! [`SelectionList`] — through the real router: type a todo, Enter to add it,
//! watch it appear in the list, Tab to the list, move the selection, and delete
//! the row. This exercises two focusables of different types, outcome-driven app
//! state, the **widget-command clear** (slice 6, replacing the slice-4 re-keying
//! workaround), and re-render from mutated state.

use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::Size;
use rabbitui_core::id::{WidgetId, key};
use rabbitui_core::input::Key as InputKey;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::pending::Pending;
use rabbitui_core::routing::RouteResult;
use rabbitui_testing::TestApp;
use rabbitui_widgets::{SelectionList, TextInput};

/// The app state, mirroring `examples/todo.rs`.
#[derive(Default)]
struct Todo {
    todos: Vec<String>,
    draft: String,
    selected: usize,
}

/// The view: input above, list below. The input's key is stable — a submit
/// clears it with a widget command, not by re-keying.
fn view(app: &Todo, frame: &mut Frame<'_>) {
    let [input_row, list_area] = frame.rows([
        rabbitui_core::layout::Constraint::Length(1),
        rabbitui_core::layout::Constraint::Fill(1),
    ]);
    frame.widget(key("input"), input_row, &TextInput::new().placeholder("Add a todo…"));
    frame.widget(key("list"), list_area, &SelectionList::new(app.todos.clone()));
}

/// Folds routing outcomes and the raw key into the state, mirroring the example's
/// `update`. Returns whether a submit fired (so the caller clears the field via a
/// widget command, exactly as the runtime would from `Update::widget`).
fn apply(app: &mut Todo, key_pressed: InputKey, result: &RouteResult) -> bool {
    let mut submitted = false;
    for (id, outcome) in &result.outcomes {
        if *id == WidgetId::ROOT.child(key("input")) {
            match outcome {
                Outcome::Changed(value) => app.draft = value.clone(),
                Outcome::Submitted => {
                    let todo = app.draft.trim().to_string();
                    if !todo.is_empty() {
                        app.todos.push(todo);
                    }
                    app.draft.clear();
                    submitted = true;
                }
                _ => {}
            }
        }
        if *id == WidgetId::ROOT.child(key("list")) {
            if let Outcome::Selected(index) = outcome {
                app.selected = *index;
            }
        }
    }
    // `d` deletes only when unconsumed (the input consumes chars while focused).
    if !result.consumed && key_pressed == InputKey::Char('d') && !app.todos.is_empty() {
        let index = app.selected.min(app.todos.len() - 1);
        app.todos.remove(index);
        app.selected = app.selected.min(app.todos.len().saturating_sub(1));
    }
    submitted
}

/// Presses one key, folds the result into state, and re-renders — one loop step.
///
/// On a submit it drives the between-frames widget-command clear through the
/// harness's [`TestApp::apply_pending`] (the same `core::pending` apply the
/// runtime uses), so the field is empty on the next frame — no generation key.
fn step(app: &mut TestApp<Todo>, key_pressed: InputKey) {
    let result = app.send_key(key_pressed);
    let submitted = apply(app.state_mut(), key_pressed, &result);
    if submitted {
        let id = WidgetId::ROOT.child(key("input"));
        app.apply_pending(|p: &mut Pending| p.command::<TextInput>(id, |s| s.clear()), view);
    } else {
        app.render(view);
    }
}

fn type_str(app: &mut TestApp<Todo>, text: &str) {
    for ch in text.chars() {
        step(app, InputKey::Char(ch));
    }
}

#[test]
fn type_enter_appears_tab_select_delete() {
    let mut app = TestApp::new(Size::new(20, 5), Todo::default());
    app.render(view);

    // The first focusable (the input) is auto-focused on the first frame —
    // no Tab needed before typing (Focus::reconcile's default).
    let input_id = WidgetId::ROOT.child(key("input"));
    assert_eq!(app.focus(), Some(input_id), "input is auto-focused on first render");

    // Type a todo and submit it.
    type_str(&mut app, "milk");
    assert_eq!(app.state().draft, "milk");
    step(&mut app, InputKey::Enter);

    // The todo is committed and the draft cleared. The field itself is cleared by
    // the widget command (its identity is stable), and it keeps focus.
    assert_eq!(app.state().todos, vec!["milk".to_string()]);
    assert_eq!(app.state().draft, "");
    assert_eq!(app.focus(), Some(input_id), "the input keeps focus across a submit");
    // The input row (the first line) is cleared — the submitted text is gone.
    assert!(!app.buffer_text().lines().next().unwrap_or("").contains("milk"));

    // Add a second todo the same way.
    type_str(&mut app, "eggs");
    step(&mut app, InputKey::Enter);
    assert_eq!(app.state().todos, vec!["milk".to_string(), "eggs".to_string()]);

    // The list shows both rows.
    app.render(view);
    assert!(app.buffer_text().contains("milk"));
    assert!(app.buffer_text().contains("eggs"));

    // The input still holds focus; Tab moves it to the list.
    step(&mut app, InputKey::Tab);
    assert_eq!(
        app.focus(),
        Some(rabbitui_core::id::WidgetId::ROOT.child(key("list"))),
        "Tab moves focus to the list",
    );

    // Move the selection to the second row, then delete it.
    step(&mut app, InputKey::Down);
    assert_eq!(app.state().selected, 1);
    step(&mut app, InputKey::Char('d'));

    // "eggs" is gone; "milk" remains; selection clamped back to 0.
    assert_eq!(app.state().todos, vec!["milk".to_string()]);
    assert_eq!(app.state().selected, 0);
    app.render(view);
    assert!(app.buffer_text().contains("milk"));
    assert!(!app.buffer_text().contains("eggs"));
}

#[test]
fn delete_key_reaches_app_only_when_list_focused() {
    // While the input is focused, 'd' types into the field (consumed), so the
    // app's delete binding must not fire.
    let mut app = TestApp::new(Size::new(20, 5), Todo::default());
    app.render(view);
    app.state_mut().todos = vec!["one".to_string()];
    app.render(view);

    // The input is auto-focused from the first frame (Focus::reconcile's
    // first-focusable default); 'd' should insert, not delete the todo.
    step(&mut app, InputKey::Char('d'));
    assert_eq!(app.state().todos, vec!["one".to_string()], "'d' typed, did not delete");
    assert_eq!(app.state().draft, "d");
}
