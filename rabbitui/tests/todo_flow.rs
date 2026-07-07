//! Integration test: the full todo flow through the headless [`TestApp`].
//!
//! Drives the same shape as `examples/todo.rs` — a [`TextInput`] and a
//! [`SelectionList`] — through the real router: type a todo, Enter to add it,
//! watch it appear in the list, Tab to the list, move the selection, and delete
//! the row. This exercises two focusables of different types, outcome-driven app
//! state, the uncontrolled-input re-key-to-clear workaround, and re-render from
//! mutated state (slice-4 design note's testing section).

use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::Size;
use rabbitui_core::id::{Key, key};
use rabbitui_core::input::Key as InputKey;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::routing::RouteResult;
use rabbitui_testing::TestApp;
use rabbitui_widgets::{SelectionList, TextInput};

/// The app state, mirroring `examples/todo.rs`.
#[derive(Default)]
struct Todo {
    todos: Vec<String>,
    draft: String,
    input_generation: u64,
    selected: usize,
}

/// The input's generation-keyed identity for this frame.
fn input_key(app: &Todo) -> Key {
    key("input").index(usize::try_from(app.input_generation).unwrap_or(usize::MAX))
}

/// The view: input above, list below.
fn view(app: &Todo, frame: &mut Frame<'_>) {
    let [input_row, list_area] = frame.rows([
        rabbitui_core::layout::Constraint::Length(1),
        rabbitui_core::layout::Constraint::Fill(1),
    ]);
    frame.widget(input_key(app), input_row, &TextInput::new().placeholder("Add a todo…"));
    frame.widget(key("list"), list_area, &SelectionList::new(app.todos.clone()));
}

/// Folds routing outcomes and the raw key into the state, mirroring the example's
/// `update`. Returns nothing; the test re-renders after.
fn apply(app: &mut Todo, key_pressed: InputKey, result: &RouteResult) {
    for (id, outcome) in &result.outcomes {
        if *id == rabbitui_core::id::WidgetId::ROOT.child(input_key(app)) {
            match outcome {
                Outcome::Changed(value) => app.draft = value.clone(),
                Outcome::Submitted => {
                    let todo = app.draft.trim().to_string();
                    if !todo.is_empty() {
                        app.todos.push(todo);
                    }
                    app.input_generation += 1;
                    app.draft.clear();
                }
                _ => {}
            }
        }
        if *id == rabbitui_core::id::WidgetId::ROOT.child(key("list")) {
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
}

/// Presses one key, folds the result into state, and re-renders — one loop step.
fn step(app: &mut TestApp<Todo>, key_pressed: InputKey) {
    let result = app.send_key(key_pressed);
    // `apply` needs &mut Todo; take it via state_mut.
    apply_to(app, key_pressed, &result);
    app.render(view);
}

/// Bridges `apply` through `TestApp::state_mut`.
fn apply_to(app: &mut TestApp<Todo>, key_pressed: InputKey, result: &RouteResult) {
    apply(app.state_mut(), key_pressed, result);
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

    // Focus the input (Tab from nothing selects the first focusable = input).
    step(&mut app, InputKey::Tab);
    assert_eq!(
        app.focus(),
        Some(rabbitui_core::id::WidgetId::ROOT.child(input_key(app.state()))),
        "Tab focuses the input first",
    );

    // Type a todo and submit it.
    type_str(&mut app, "milk");
    assert_eq!(app.state().draft, "milk");
    step(&mut app, InputKey::Enter);

    // The todo is committed; the draft cleared; the input re-keyed.
    assert_eq!(app.state().todos, vec!["milk".to_string()]);
    assert_eq!(app.state().draft, "");
    assert_eq!(app.state().input_generation, 1);

    // Add a second todo the same way.
    type_str(&mut app, "eggs");
    step(&mut app, InputKey::Enter);
    assert_eq!(app.state().todos, vec!["milk".to_string(), "eggs".to_string()]);

    // The list shows both rows.
    app.render(view);
    assert!(app.buffer_text().contains("milk"));
    assert!(app.buffer_text().contains("eggs"));

    // After two submits the input was re-keyed twice; Tab now cycles: input →
    // list. Tab once moves focus off the input to the list.
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

    // Focus the input, type 'd' — it should insert, not delete the todo.
    step(&mut app, InputKey::Tab); // focus input
    step(&mut app, InputKey::Char('d'));
    assert_eq!(app.state().todos, vec!["one".to_string()], "'d' typed, did not delete");
    assert_eq!(app.state().draft, "d");
}
