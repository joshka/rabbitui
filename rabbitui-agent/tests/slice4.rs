//! Slice-4 acceptance tests: the tool-use loop, driven headlessly.
//!
//! Like slice 1, these exercise the *pure reducer* directly (`apply_message`,
//! `run_pending`, `deny_pending`, `continue_with_results`) via `TestApp::send`,
//! and assert app state and the alt-screen buffer. The reducer runs the modal
//! decision path without the real `update` closure, so tool execution is driven
//! with an explicit temp-dir root — exactly the confinement `tools::execute`
//! enforces in production against the cwd.
//!
//! Offline-verified here (per the slice-4 brief): the reducer accumulates a
//! streamed tool_use turn, the modal appears, Allow drives the Tool cells
//! Pending → Running → Done and produces a single `tool_result` user message
//! that the continuation request carries, and a final assistant reply commits.
//! The Deny path yields `is_error` results instead. What is NOT exercised here
//! (and cannot be, offline) is the live API's acceptance of the continuation —
//! especially thinking-block replay; that smoke test is pending a real key.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rabbitui_agent::app::{self, Agent, Msg};
use rabbitui_agent::backend::replay::ReplayBackend;
use rabbitui_agent::backend::{ChatMessage, ContentBlock, Role, StreamEvent};
use rabbitui_agent::transcript::{ToolStatus, TranscriptCell};
use rabbitui_core::geometry::Size;
use rabbitui_testing::TestApp;

/// A fresh alt-screen app over an empty replay backend.
fn alt_screen_app() -> Agent {
    let mut agent = Agent::new("test-model", Box::new(ReplayBackend::new(Vec::new())));
    agent.inline = false;
    agent
}

/// A temp dir seeded with `hello.txt`, the file the fixture's tool call reads.
fn temp_root() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("rabbitui-slice4-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.txt"), "hi there").unwrap();
    dir
}

/// Feeds `count` events of the fixture (from `cursor`) through the reducer as if
/// the effect stream had delivered each; returns the new cursor.
fn feed(app: &mut TestApp<Agent>, events: &[StreamEvent], cursor: usize, count: usize) -> usize {
    for event in &events[cursor..cursor + count] {
        let event = event.clone();
        app.send(
            move |state| {
                app::apply_message(state, Msg::Event(Ok(event.clone())));
            },
            app::view,
        );
    }
    cursor + count
}

/// Parses the tool-use fixture into a flat event list.
fn fixture_events() -> Vec<StreamEvent> {
    include_str!("fixtures/tool_use.jsonl")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("fixture parses"))
        .collect()
}

#[test]
fn allow_runs_tools_and_a_continuation_carries_the_result() {
    let root = temp_root();
    let events = fixture_events();
    let mut app = TestApp::new(Size::new(60, 24), alt_screen_app());
    app.render(app::view);

    // Submit a prompt and open the streaming turn.
    app.send(
        |state| {
            state.draft = "read hello.txt".to_string();
            app::on_submit(state);
        },
        app::view,
    );

    // Feed the first turn: thinking, signature, text, tool_use(...), MessageDone(tool_use).
    let cursor = feed(&mut app, &events, 0, 8);

    // The modal is up and a Pending Tool cell exists.
    assert!(app.state().is_confirming(), "tool_use stop opens the modal");
    let cells = &app.state().cells;
    assert!(
        cells.iter().any(|cell| matches!(
            cell,
            TranscriptCell::Tool {
                status: ToolStatus::Pending,
                ..
            }
        )),
        "a Pending Tool cell was added"
    );
    // The assistant tool_use turn was pushed to history with a thinking block
    // carrying its signature (so it can replay on continuation).
    let assistant = app
        .state()
        .history
        .iter()
        .rev()
        .find(|message| message.role == Role::Assistant)
        .expect("assistant tool_use message");
    assert!(
        assistant.content.iter().any(|block| matches!(
            block,
            ContentBlock::Thinking { signature, .. } if signature == "sig-abc123"
        )),
        "the thinking signature is preserved for replay"
    );
    assert!(
        assistant.content.iter().any(
            |block| matches!(block, ContentBlock::ToolUse { name, .. } if name == "read_file")
        ),
        "the tool_use block is in the assistant message"
    );
    // The modal renders its buttons.
    let modal_text = app.buffer_text();
    assert!(
        modal_text.contains("Allow"),
        "modal shows Allow:\n{modal_text}"
    );
    assert!(
        modal_text.contains("Deny"),
        "modal shows Deny:\n{modal_text}"
    );

    // Allow: run the tools against the temp root, then continue.
    let request = {
        let state = app.state_mut();
        let outcomes = app::run_pending(state, &root);
        app::continue_with_results(state, outcomes)
    };
    app.render(app::view);
    let request = request.expect("a continuation request is produced");

    // The Tool cell ended Done, holding the file contents.
    let cells = &app.state().cells;
    let tool_cell = cells
        .iter()
        .find_map(|cell| match cell {
            TranscriptCell::Tool { status, output, .. } => Some((*status, output.clone())),
            _ => None,
        })
        .expect("a tool cell");
    assert_eq!(tool_cell.0, ToolStatus::Ok, "the tool succeeded");
    assert_eq!(
        tool_cell.1, "hi there",
        "the tool cell holds the file contents"
    );

    // The continuation request carries exactly one user message of tool_result
    // blocks (parallel-safe: all results in one message).
    let last = request.messages.last().expect("a last message");
    assert_eq!(last.role, Role::User);
    assert_eq!(last.content.len(), 1, "one result block for one call");
    assert!(
        matches!(
            &last.content[0],
            ContentBlock::ToolResult { tool_use_id, content, is_error }
                if tool_use_id == "toolu_01" && content == "hi there" && !is_error
        ),
        "the tool_result echoes the id and carries the output, not an error"
    );
    assert!(!app.state().is_confirming(), "the modal closed after Allow");

    // Feed the continuation turn: final text + MessageDone(end_turn).
    feed(&mut app, &events, cursor, 2);
    assert!(!app.state().is_confirming());
    let text = app.buffer_text();
    assert!(
        app.state()
            .cells
            .iter()
            .any(|cell| matches!(cell, TranscriptCell::Assistant(s) if s.contains("hi there"))),
        "the final assistant reply committed:\n{text}"
    );
}

#[test]
fn deny_sends_an_is_error_result() {
    let events = fixture_events();
    let mut app = TestApp::new(Size::new(60, 24), alt_screen_app());
    app.render(app::view);
    app.send(
        |state| {
            state.draft = "read hello.txt".to_string();
            app::on_submit(state);
        },
        app::view,
    );
    feed(&mut app, &events, 0, 8);
    assert!(app.state().is_confirming());

    // Deny: no execution, an is_error result, cell marked Failed.
    let request = {
        let state = app.state_mut();
        let outcomes = app::deny_pending(state);
        app::continue_with_results(state, outcomes)
    }
    .expect("deny still re-sends so the model reacts");
    app.render(app::view);

    let cells = &app.state().cells;
    assert!(
        cells.iter().any(|cell| matches!(
            cell,
            TranscriptCell::Tool {
                status: ToolStatus::Failed,
                ..
            }
        )),
        "the denied tool cell is Failed"
    );
    let last = request.messages.last().expect("a last message");
    assert!(
        matches!(
            &last.content[0],
            ContentBlock::ToolResult { is_error, content, .. }
                if *is_error && content.contains("denied")
        ),
        "denial produces an is_error result with a 'denied' message"
    );
}

#[test]
fn on_submit_still_sends_text_history() {
    // The content-block model keeps the constructor equality tests valid.
    let mut app = alt_screen_app();
    app.draft = "hi".to_string();
    let request = app::on_submit(&mut app).expect("request");
    assert_eq!(request.messages, vec![ChatMessage::user("hi")]);
}
