//! Slice-1 acceptance tests: the reducer and transcript, driven headlessly.
//!
//! `TestApp` does not run the real `update` closure — it renders a view and routes
//! input through the same router the runtime uses. So these tests exercise the
//! *pure reducer* (`on_submit`, `apply_message`) directly via `TestApp::send`
//! (mutate state, then re-render) and assert the alt-screen buffer and app state.
//! Inline-mode scrollback commits are not modelled by the test buffer, so the app
//! renders in alt-screen mode here (`inline = false`).

use rabbitui_agent::app::{self, Agent, Msg, Reaction};
use rabbitui_agent::backend::replay::ReplayBackend;
use rabbitui_agent::backend::{BackendError, ChatMessage, Role, StreamEvent};
use rabbitui_agent::transcript::TranscriptCell;
use rabbitui_core::geometry::Size;
use rabbitui_testing::TestApp;

/// A fresh alt-screen app over an empty replay backend (the reducer never calls
/// the backend, so an empty one is fine for reducer-only tests).
fn alt_screen_app() -> Agent {
    let mut agent = Agent::new("test-model", Box::new(ReplayBackend::new(Vec::new())));
    agent.inline = false;
    agent
}

/// Replays the committed greeting fixture through the reducer as if the effect
/// stream had delivered each event, returning the final app.
fn replay_greeting(app: &mut TestApp<Agent>) {
    let fixture = include_str!("fixtures/greeting.jsonl");
    for line in fixture.lines().filter(|line| !line.trim().is_empty()) {
        let event: StreamEvent = serde_json::from_str(line).expect("fixture parses");
        app.send(
            |state| {
                app::apply_message(state, Msg::Event(Ok(event.clone())));
            },
            app::view,
        );
    }
}

#[test]
fn a_conversation_renders_prompt_thinking_and_reply() {
    let mut app = TestApp::new(Size::new(48, 24), alt_screen_app());
    app.render(app::view);

    // Submit a prompt (the reducer part of a composer submit).
    app.send(
        |state| {
            state.draft = "hi there".to_string();
            let request = app::on_submit(state);
            assert!(request.is_some(), "a non-empty prompt yields a request");
        },
        app::view,
    );
    assert!(app.state().is_streaming(), "submitting opens a streaming turn");
    assert!(
        app.buffer_text().contains("hi there"),
        "the prompt shows in the transcript:\n{}",
        app.buffer_text()
    );

    replay_greeting(&mut app);

    // The turn is closed and rendered.
    assert!(!app.state().is_streaming(), "MessageDone ends the turn");
    let cells = &app.state().cells;
    assert!(
        matches!(cells.first(), Some(TranscriptCell::User(p)) if p == "hi there"),
        "first cell is the user prompt"
    );
    assert!(
        cells
            .iter()
            .any(|cell| matches!(cell, TranscriptCell::Thinking(_))),
        "a thinking cell was recorded"
    );
    assert!(
        cells
            .iter()
            .any(|cell| matches!(cell, TranscriptCell::Assistant(s) if s.contains("Hello"))),
        "the assistant reply was recorded"
    );
    let text = app.buffer_text();
    assert!(text.contains("Hello"), "the reply renders:\n{text}");
    assert!(text.contains("rabbit"), "the reply renders:\n{text}");
}

#[test]
fn history_accumulates_user_then_assistant() {
    let mut app = TestApp::new(Size::new(48, 24), alt_screen_app());
    app.render(app::view);
    app.send(
        |state| {
            state.draft = "count to three".to_string();
            app::on_submit(state);
        },
        app::view,
    );
    replay_greeting(&mut app);

    let history = &app.state().history;
    assert_eq!(history.len(), 2, "one user + one assistant message");
    assert_eq!(history[0].role, Role::User);
    assert_eq!(history[0].content, "count to three");
    assert_eq!(history[1].role, Role::Assistant);
    assert!(history[1].content.contains("Hello"));
}

#[test]
fn on_submit_sends_the_full_history() {
    let mut app = alt_screen_app();
    app.draft = "first".to_string();
    let first = app::on_submit(&mut app).expect("request");
    assert_eq!(first.model, "test-model");
    assert_eq!(first.messages, vec![ChatMessage::user("first")]);

    // Close the first turn so history carries the assistant reply.
    app::apply_message(
        &mut app,
        Msg::Event(Ok(StreamEvent::TextDelta {
            text: "ok".to_string(),
        })),
    );
    let reaction = app::apply_message(
        &mut app,
        Msg::Event(Ok(StreamEvent::MessageDone {
            stop_reason: rabbitui_agent::backend::StopReason::EndTurn,
            usage: rabbitui_agent::backend::Usage::default(),
        })),
    );
    assert_eq!(reaction, Reaction::TurnComplete);

    app.draft = "second".to_string();
    let second = app::on_submit(&mut app).expect("request");
    assert_eq!(
        second.messages,
        vec![
            ChatMessage::user("first"),
            ChatMessage::assistant("ok"),
            ChatMessage::user("second"),
        ],
        "the second request carries the whole conversation"
    );
}

#[test]
fn mouse_wheel_scrolls_the_transcript_in_browse_mode() {
    use rabbitui_core::geometry::Position;
    use rabbitui_core::input::MouseKind;

    // A transcript taller than the viewport, so scrolling has somewhere to go.
    let mut agent = alt_screen_app();
    for n in 0..40 {
        agent.cells.push(TranscriptCell::User(format!("line {n:02}")));
    }
    let mut app = TestApp::new(Size::new(48, 24), agent);
    app.render(app::view);

    let top = app.buffer_text();
    assert!(top.contains("line 00"), "starts at the top:\n{top}");
    assert!(
        !top.contains("line 39"),
        "the newest line is below the fold before scrolling:\n{top}"
    );

    // Wheel down over the transcript: positive notches scroll toward newer cells.
    app.send_mouse(MouseKind::Scroll(30), Position::new(10, 10));
    app.render(app::view);

    let scrolled = app.buffer_text();
    assert!(
        scrolled.contains("line 39"),
        "wheeling down reveals the newest cells:\n{scrolled}"
    );
    assert!(
        !scrolled.contains("line 00"),
        "and scrolls the oldest out of view:\n{scrolled}"
    );
}

#[test]
fn empty_submit_is_a_no_op() {
    let mut app = alt_screen_app();
    app.draft = "   ".to_string();
    assert!(app::on_submit(&mut app).is_none(), "blank prompt sends nothing");
    assert!(app.cells.is_empty());
    assert!(!app.is_streaming());
}

#[test]
fn a_backend_error_becomes_an_error_cell() {
    let mut app = alt_screen_app();
    app.draft = "boom".to_string();
    app::on_submit(&mut app);
    assert!(app.is_streaming());

    let reaction = app::apply_message(
        &mut app,
        Msg::Event(Err(BackendError::Transport("connection reset".to_string()))),
    );
    assert_eq!(reaction, Reaction::TurnComplete);
    assert!(!app.is_streaming(), "an error ends the turn");
    assert!(
        matches!(app.cells.last(), Some(TranscriptCell::Error(m)) if m.contains("connection reset")),
        "the error is surfaced as a cell"
    );
}
