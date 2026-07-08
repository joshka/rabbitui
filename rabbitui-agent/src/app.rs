//! The app: state, the pure reducer, the update closure, and both view modes.
//!
//! The reducer ([`apply_message`], [`on_submit`]) only *mutates state* — it never
//! commits or spawns. That purity is what lets `rabbitui_testing::TestApp`
//! drive it: the harness does not run the real [`update`] closure, so tests inject
//! state mutations and assert the rendered buffer. The closure is the thin layer
//! that turns reducer results into framework side effects (scrollback commits,
//! effect spawns, persistence).
//!
//! Scrollback commits are decoupled from state via [`Agent::committed`]: after the
//! reducer runs, [`update`] commits any not-yet-committed cells in inline mode.
//! Commits go to native scrollback, which the test buffer does not model, so tests
//! render in alt-screen mode.

use std::ops::ControlFlow;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_core::Stream;
use futures_util::StreamExt as _;
use rabbitui::App;
use rabbitui::app::{Event, Update};
use rabbitui::effect::Cmd;
use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::Rect;
use rabbitui_core::id::key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::mode::Mode;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::spacing;
use rabbitui_core::theme::{Role, Theme};
use rabbitui_widgets::{Button, Collapsible, HelpOverlay, Panel, Text, TextInput};

use crate::backend::{
    Backend, ChatMessage, ChatRequest, ContentBlock, Role as ApiRole, StopReason, StreamEvent,
};
use crate::keymap::{Action, KEYMAP, base_help_rows};
use crate::session::Session;
use crate::transcript::{
    PendingToolUse, Streaming, ToolStatus, TranscriptCell, commit_lines_for,
};

/// The bounded live-tail height in inline mode, in rows.
pub const TAIL_HEIGHT: u16 = 8;

/// The cancel-previous group each agent turn is spawned into, so a new prompt (or
/// a cancel) aborts the running stream.
const AGENT_GROUP: &str = "agent";
/// The independently-cancellable group the spinner ticker runs in.
const SPINNER_GROUP: &str = "spinner";
/// The spinner frames cycled while streaming.
const SPINNER: [&str; 4] = ["⠋", "⠙", "⠹", "⠸"];
/// The cap on tool-continuation round-trips per user turn, so a model that
/// keeps calling tools cannot loop forever.
const MAX_CONTINUATIONS: usize = 5;

/// The pending tool calls awaiting the user's allow/deny decision, held while
/// the confirmation modal is up.
#[derive(Debug, Clone)]
pub struct Awaiting {
    /// The tool calls to run (or deny), in order — one Tool cell each.
    pub calls: Vec<PendingToolUse>,
    /// The `cells` index of each call's Tool cell, parallel to `calls`, so
    /// execution can flip them Pending → Running → Done/Error.
    pub cell_indices: Vec<usize>,
    /// How many continuation round-trips this turn has already made, so the loop
    /// is capped at [`MAX_CONTINUATIONS`].
    pub continuations: usize,
}

/// The whole app's owned state.
pub struct Agent {
    /// The committed transcript, in order — the same cells both modes render.
    pub cells: Vec<TranscriptCell>,
    /// The in-flight assistant turn, if streaming.
    pub streaming: Option<Streaming>,
    /// Whether the app is in inline mode (vs alt-screen).
    pub inline: bool,
    /// The composer draft, tracked from `Changed` outcomes.
    pub draft: String,
    /// The spinner animation frame.
    pub spinner: usize,
    /// Whether the spinner ticker stream is running.
    pub ticking: bool,
    /// How many `cells` have been committed to inline scrollback.
    pub committed: usize,
    /// The API conversation history — sent on every request and persisted.
    pub history: Vec<ChatMessage>,
    /// How many `history` messages have been written to the session file.
    pub persisted: usize,
    /// The model id requests are sent against.
    pub model: String,
    /// The backend (mutated in `update` to build request streams).
    pub backend: Box<dyn Backend>,
    /// The session persistence handle, if persistence is enabled.
    pub session: Option<Session>,
    /// The pending tool calls awaiting the confirmation modal's decision, if the
    /// turn stopped on `tool_use`.
    pub awaiting: Option<Awaiting>,
    /// Whether a focus request into the modal is still owed (set when the modal
    /// opens, cleared once honored — the declare-then-focus handshake, per
    /// `examples/form.rs`).
    pub focus_modal: bool,
    /// Whether the generated help overlay is up. Toggled by [`Action::Help`],
    /// dismissed by Esc (or the same chord again). Display-only, so it holds no
    /// focus — the composer keeps it while help is shown.
    pub showing_help: bool,
}

impl Agent {
    /// A fresh app over `backend`, in inline mode, with no persistence.
    #[must_use]
    pub fn new(model: impl Into<String>, backend: Box<dyn Backend>) -> Self {
        Self {
            cells: Vec::new(),
            streaming: None,
            inline: true,
            draft: String::new(),
            spinner: 0,
            ticking: false,
            committed: 0,
            history: Vec::new(),
            persisted: 0,
            model: model.into(),
            backend,
            session: None,
            awaiting: None,
            focus_modal: false,
            showing_help: false,
        }
    }

    /// Enables persistence to `session`, seeding history from a resumed file.
    #[must_use]
    pub fn with_session(mut self, session: Session, resumed: Vec<ChatMessage>) -> Self {
        for message in resumed {
            // Render a resumed message as its text; tool_use / tool_result /
            // thinking blocks carry no user-visible prose here (they replay into
            // the request from `history`, not the transcript).
            let text = message.text();
            let cell = match message.role {
                ApiRole::User if !text.trim().is_empty() => Some(TranscriptCell::User(text)),
                ApiRole::Assistant if !text.trim().is_empty() => {
                    Some(TranscriptCell::Assistant(text))
                }
                _ => None,
            };
            if let Some(cell) = cell {
                self.cells.push(cell);
            }
            self.history.push(message);
        }
        // Resumed history is already on disk; do not re-append it.
        self.persisted = self.history.len();
        self.committed = self.cells.len();
        self.session = Some(session);
        self
    }

    /// Whether a response is currently streaming.
    #[must_use]
    pub fn is_streaming(&self) -> bool {
        self.streaming.is_some()
    }

    /// Whether the confirmation modal is up with real pending calls (a
    /// placeholder `Awaiting` carrying only a continuation count is not).
    #[must_use]
    pub fn is_confirming(&self) -> bool {
        self.awaiting
            .as_ref()
            .is_some_and(|awaiting| !awaiting.calls.is_empty())
    }
}

/// A message an effect delivers to [`update`].
#[derive(Debug, Clone)]
pub enum Msg {
    /// A backend event (or error) for the in-flight turn.
    Event(Result<StreamEvent, crate::backend::BackendError>),
    /// The spinner ticker fired.
    Tick,
}

/// What [`apply_message`] did, so the caller can run the side effects the pure
/// reducer cannot (stop the spinner, persist, open the modal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    /// Streaming continues (or nothing happened).
    None,
    /// The turn finished normally (end/max/refusal): stop the spinner, persist.
    TurnComplete,
    /// The turn stopped on `tool_use`: `app.awaiting` now holds the pending
    /// calls; the caller opens the confirmation modal (spinner stops too).
    AwaitConfirmation,
}

// ---------------------------------------------------------------------------
// Reducer (pure — mutates state only; no commits, no spawns, no I/O)
// ---------------------------------------------------------------------------

/// Folds one delivered [`Msg`] into the app, returning what happened.
pub fn apply_message(app: &mut Agent, msg: Msg) -> Reaction {
    match msg {
        Msg::Tick => {
            app.spinner = (app.spinner + 1) % SPINNER.len();
            Reaction::None
        }
        Msg::Event(Ok(event)) => apply_event(app, event),
        Msg::Event(Err(error)) => {
            // A failed turn commits nothing but the error, in order.
            app.streaming = None;
            app.cells.push(TranscriptCell::Error(error.to_string()));
            Reaction::TurnComplete
        }
    }
}

/// Folds one backend [`StreamEvent`] into the in-flight turn.
fn apply_event(app: &mut Agent, event: StreamEvent) -> Reaction {
    match event {
        StreamEvent::TextDelta { text } => {
            if let Some(streaming) = app.streaming.as_mut() {
                streaming.source.push_str(&text);
            }
            Reaction::None
        }
        StreamEvent::ThinkingDelta { text } => {
            if let Some(streaming) = app.streaming.as_mut() {
                streaming.thinking.push_str(&text);
            }
            Reaction::None
        }
        StreamEvent::ThinkingSignatureDelta { signature } => {
            if let Some(streaming) = app.streaming.as_mut() {
                streaming.thinking_signature.push_str(&signature);
            }
            Reaction::None
        }
        // A tool-use block opens: record it and show its name in the status line.
        StreamEvent::ToolUseStart { id, name } => {
            if let Some(streaming) = app.streaming.as_mut() {
                streaming.running_tool = Some(name.clone());
                streaming.tool_uses.push(PendingToolUse {
                    id,
                    name,
                    input_json: String::new(),
                });
            }
            Reaction::None
        }
        // Streamed JSON input accretes onto the matching open block.
        StreamEvent::ToolUseInputDelta { id, json } => {
            if let Some(streaming) = app.streaming.as_mut()
                && let Some(call) = streaming.tool_uses.iter_mut().find(|call| call.id == id)
            {
                call.input_json.push_str(&json);
            }
            Reaction::None
        }
        StreamEvent::ToolUseStop { .. } => Reaction::None,
        StreamEvent::MessageDone { stop_reason, .. } => finish_turn(app, stop_reason),
    }
}

/// Closes the streaming turn. On a `tool_use` stop, pushes the assistant block
/// message (thinking + text + tool_use per call), adds Pending Tool cells, and
/// arms `app.awaiting` for the confirmation modal. On any other stop reason,
/// commits thinking + prose as cells and appends the assistant text to history.
fn finish_turn(app: &mut Agent, stop_reason: StopReason) -> Reaction {
    let Some(streaming) = app.streaming.take() else {
        return Reaction::TurnComplete;
    };

    if stop_reason == StopReason::ToolUse && !streaming.tool_uses.is_empty() {
        return arm_tool_use(app, streaming);
    }

    // A normal end-of-turn closes the tool loop (if one was running): clear the
    // continuation-count placeholder left by `continue_with_results`.
    app.awaiting = None;

    // Normal end-of-turn: commit thinking + prose, extend history with the
    // assistant text.
    if !streaming.thinking.trim().is_empty() {
        app.cells.push(TranscriptCell::Thinking(streaming.thinking));
    }
    if !streaming.source.trim().is_empty() {
        app.cells
            .push(TranscriptCell::Assistant(streaming.source.clone()));
        app.history.push(ChatMessage::assistant(streaming.source));
    }
    Reaction::TurnComplete
}

/// Handles a turn that stopped on `tool_use`: builds the assistant block message
/// (thinking, text, one `tool_use` per call), pushes it to history, adds a
/// Pending Tool cell per call, and arms `app.awaiting`. Returns
/// [`Reaction::AwaitConfirmation`] so the caller opens the modal.
fn arm_tool_use(app: &mut Agent, streaming: Streaming) -> Reaction {
    // Assemble the assistant message exactly as it must replay on continuation:
    // thinking block (with its signature) first, then any prose, then a
    // tool_use block per call.
    let mut blocks: Vec<ContentBlock> = Vec::new();
    if !streaming.thinking.trim().is_empty() {
        blocks.push(ContentBlock::Thinking {
            thinking: streaming.thinking.clone(),
            signature: streaming.thinking_signature.clone(),
        });
    }
    if !streaming.source.trim().is_empty() {
        blocks.push(ContentBlock::Text {
            text: streaming.source.clone(),
        });
    }
    for call in &streaming.tool_uses {
        // A finished block's input parses; a malformed one falls back to null so
        // the request stays well-formed and the tool reports the error.
        let input = serde_json::from_str(&call.input_json).unwrap_or(serde_json::Value::Null);
        blocks.push(ContentBlock::ToolUse {
            id: call.id.clone(),
            name: call.name.clone(),
            input,
        });
    }

    // Surface any thinking and prose as cells too (the same way a normal turn
    // would), so the reasoning that led to the tool call is visible.
    if !streaming.thinking.trim().is_empty() {
        app.cells
            .push(TranscriptCell::Thinking(streaming.thinking.clone()));
    }
    if !streaming.source.trim().is_empty() {
        app.cells
            .push(TranscriptCell::Assistant(streaming.source.clone()));
    }

    app.history.push(ChatMessage::assistant_blocks(blocks));

    // One Pending Tool cell per call; remember its index for status updates.
    let mut cell_indices = Vec::with_capacity(streaming.tool_uses.len());
    for call in &streaming.tool_uses {
        let input = serde_json::from_str(&call.input_json).unwrap_or(serde_json::Value::Null);
        cell_indices.push(app.cells.len());
        app.cells.push(TranscriptCell::Tool {
            name: call.name.clone(),
            summary: crate::tools::summarize(&call.name, &input),
            output: String::new(),
            status: ToolStatus::Pending,
        });
    }

    let continuations = app
        .awaiting
        .as_ref()
        .map_or(0, |awaiting| awaiting.continuations);
    app.awaiting = Some(Awaiting {
        calls: streaming.tool_uses,
        cell_indices,
        continuations,
    });
    Reaction::AwaitConfirmation
}

/// The outcome of one resolved tool call, ready to fold into state.
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    /// The tool-call id this answers.
    pub id: String,
    /// The result (or error) text.
    pub content: String,
    /// Whether it was an error (execution failure or a denial).
    pub is_error: bool,
}

/// Runs every awaiting tool call against `root` (the cwd in production), marking
/// each Tool cell Running then Done/Error, and returns the outcomes. Denial is
/// handled by [`deny_pending`] instead; this is the Allow path.
///
/// This touches the filesystem, so it is a side effect the update closure runs —
/// but it takes an explicit `root` so tests can drive it against a temp dir.
pub fn run_pending(app: &mut Agent, root: &std::path::Path) -> Vec<ToolOutcome> {
    let Some(awaiting) = app.awaiting.as_ref() else {
        return Vec::new();
    };
    let calls = awaiting.calls.clone();
    let cell_indices = awaiting.cell_indices.clone();
    let mut outcomes = Vec::with_capacity(calls.len());
    for (call, &cell_index) in calls.iter().zip(cell_indices.iter()) {
        set_tool_status(app, cell_index, ToolStatus::Running, None);
        let input = serde_json::from_str(&call.input_json).unwrap_or(serde_json::Value::Null);
        let (content, is_error) = match crate::tools::execute_in(root, &call.name, &input) {
            Ok(output) => (output, false),
            Err(message) => (message, true),
        };
        let status = if is_error {
            ToolStatus::Failed
        } else {
            ToolStatus::Ok
        };
        set_tool_status(app, cell_index, status, Some(content.clone()));
        outcomes.push(ToolOutcome {
            id: call.id.clone(),
            content,
            is_error,
        });
    }
    outcomes
}

/// The Deny path: marks every Tool cell Failed and returns a "user denied"
/// error outcome per call, so the model still sees a result and can react.
pub fn deny_pending(app: &mut Agent) -> Vec<ToolOutcome> {
    let Some(awaiting) = app.awaiting.as_ref() else {
        return Vec::new();
    };
    let calls = awaiting.calls.clone();
    let cell_indices = awaiting.cell_indices.clone();
    let mut outcomes = Vec::with_capacity(calls.len());
    for (call, &cell_index) in calls.iter().zip(cell_indices.iter()) {
        let message = "user denied this tool call".to_string();
        set_tool_status(app, cell_index, ToolStatus::Failed, Some(message.clone()));
        outcomes.push(ToolOutcome {
            id: call.id.clone(),
            content: message,
            is_error: true,
        });
    }
    outcomes
}

/// Folds resolved tool outcomes into state and returns the continuation request:
/// pushes one user message of all `tool_result` blocks to history, clears
/// `awaiting`, opens a fresh streaming turn, and returns the request to re-send
/// — or `None` once [`MAX_CONTINUATIONS`] is exhausted (the loop is capped).
pub fn continue_with_results(
    app: &mut Agent,
    outcomes: Vec<ToolOutcome>,
) -> Option<ChatRequest> {
    let continuations = app
        .awaiting
        .as_ref()
        .map_or(0, |awaiting| awaiting.continuations);
    app.awaiting = None;

    // Parallel tool_use → all results in a single user message.
    let blocks = outcomes
        .into_iter()
        .map(|outcome| ContentBlock::ToolResult {
            tool_use_id: outcome.id,
            content: outcome.content,
            is_error: outcome.is_error,
        })
        .collect();
    app.history.push(ChatMessage::tool_results(blocks));

    if continuations >= MAX_CONTINUATIONS {
        // Give up gracefully rather than loop forever.
        app.cells.push(TranscriptCell::Error(format!(
            "tool-continuation cap ({MAX_CONTINUATIONS}) reached; stopping."
        )));
        return None;
    }

    app.streaming = Some(Streaming::default());
    // Carry the incremented continuation count on a placeholder `Awaiting` so
    // that if the *next* turn also stops on tool_use, `arm_tool_use` reads it
    // and keeps counting. A turn that ends normally clears it in `finish_turn`.
    app.awaiting = Some(Awaiting {
        calls: Vec::new(),
        cell_indices: Vec::new(),
        continuations: continuations + 1,
    });
    Some(ChatRequest {
        model: app.model.clone(),
        messages: app.history.clone(),
    })
}

/// Sets a Tool cell's status (and optionally its output), by cell index.
fn set_tool_status(
    app: &mut Agent,
    index: usize,
    status: ToolStatus,
    output: Option<String>,
) {
    if let Some(TranscriptCell::Tool {
        status: cell_status,
        output: cell_output,
        ..
    }) = app.cells.get_mut(index)
    {
        *cell_status = status;
        if let Some(text) = output {
            *cell_output = text;
        }
    }
}

/// Handles a composer submit: pushes the user prompt into the transcript and
/// history, opens a streaming turn, and returns the request to send — or `None`
/// on an empty prompt.
pub fn on_submit(app: &mut Agent) -> Option<ChatRequest> {
    let prompt = app.draft.trim().to_string();
    app.draft.clear();
    if prompt.is_empty() {
        return None;
    }
    app.cells.push(TranscriptCell::User(prompt.clone()));
    app.history.push(ChatMessage::user(prompt));
    app.streaming = Some(Streaming::default());
    Some(ChatRequest {
        model: app.model.clone(),
        messages: app.history.clone(),
    })
}

// ---------------------------------------------------------------------------
// Update closure (side effects: commits, spawns, persistence)
// ---------------------------------------------------------------------------

/// The `App` update closure: folds one event, then runs the reducer's side
/// effects (commit, persist, spawn).
pub fn update(app: &mut Agent, update: Update<'_, Msg>) -> ControlFlow<()> {
    // Track the composer draft and act on a submit.
    if let Some(Outcome::Changed(value)) = update.outcome_for(&[key("composer")]) {
        app.draft.clone_from(value);
    }
    if update.outcome_for(&[key("composer")]) == Some(&Outcome::Submitted) {
        submit(app, &update);
    }

    // Absorb effect messages (stream events, spinner ticks).
    if let Event::Message(message) = update.event() {
        match apply_message(app, message.clone()) {
            Reaction::TurnComplete => stop_spinner(app, &update),
            Reaction::AwaitConfirmation => {
                // The turn stopped on tool_use: stop the spinner and open the
                // modal, requesting focus into it (the form.rs handshake).
                stop_spinner(app, &update);
                app.focus_modal = true;
            }
            Reaction::None => {}
        }
    }

    // Help overlay routing (topmost when up): Esc or the Help chord closes it;
    // Ctrl-C still quits. Everything else is swallowed so the overlay is modal.
    if app.showing_help {
        if let Event::Input(input) = update.event()
            && let Some(press) = input.as_key()
        {
            match KEYMAP.action_for(press) {
                Some(Action::Quit) => return ControlFlow::Break(()),
                Some(Action::Help | Action::Dismiss) => app.showing_help = false,
                _ => {}
            }
        }
        // The help overlay is display-only: it holds no focusable widget, so it
        // takes no focus (focusing its Panel would fail the declare-then-focus
        // contract and panic). Its keys — Esc / the Help chord / Ctrl-C — are
        // routed here at the app level regardless of what is focused; the composer
        // keeps focus underneath.
        flush_commits(app, &update);
        persist_history(app);
        return ControlFlow::Continue(());
    }

    // Confirmation modal routing (when up): Allow/Deny buttons, plus y/n and Esc.
    if app.is_confirming() {
        handle_modal(app, &update);
        // Honor the one-shot focus request into the modal.
        if app.focus_modal {
            update.focus(&[key("modal"), key("allow")]);
            app.focus_modal = false;
        }
        // While the modal is up, keys belong to it — skip base app bindings,
        // except the always-available Quit chord below.
        if let Event::Input(input) = update.event()
            && !update.consumed()
            && let Some(press) = input.as_key()
            && KEYMAP.action_for(press) == Some(Action::Quit)
        {
            return ControlFlow::Break(());
        }
        flush_commits(app, &update);
        persist_history(app);
        return ControlFlow::Continue(());
    }

    // App-level key bindings, dispatched through the ONE keymap. `Update::action`
    // applies the consumed-guard (a printable chord a focused widget took is never
    // re-interpreted). `Send` is owned by the composer's `Submitted` outcome (see
    // the top of `update`), so its arm is absent here.
    match update.action(&KEYMAP) {
        Some(Action::ToggleMode) => toggle_mode(app, &update),
        Some(Action::Cancel) => {
            if app.is_streaming() {
                cancel(app, &update);
            }
        }
        Some(Action::Help) => app.showing_help = true,
        Some(Action::Quit) => return ControlFlow::Break(()),
        _ => {}
    }

    flush_commits(app, &update);
    persist_history(app);
    ControlFlow::Continue(())
}

/// Toggles inline ↔ alt-screen browse and switches the runtime mode.
///
/// Entering browse mode moves focus into the transcript scroll so Up/Down/
/// PageUp/PageDown/Home/End (and the wheel) drive it immediately; returning to
/// inline puts focus back on the composer so typing resumes without a Tab.
fn toggle_mode(app: &mut Agent, update: &Update<'_, Msg>) {
    app.inline = !app.inline;
    if app.inline {
        update.set_mode(Mode::inline(TAIL_HEIGHT));
        update.focus(&[key("composer")]);
    } else {
        update.set_mode(Mode::AltScreen);
        update.focus(&[key("transcript")]);
    }
}

/// Sends the composer draft and spawns the response stream and spinner.
fn submit(app: &mut Agent, update: &Update<'_, Msg>) {
    update.widget::<TextInput>(&[key("composer")], |state| state.clear());
    let Some(request) = on_submit(app) else {
        return;
    };
    let stream = app.backend.send(request);
    update.spawn(Cmd::stream(stream.map(Msg::Event)).group(AGENT_GROUP));
    if !app.ticking {
        app.ticking = true;
        update.spawn(Cmd::stream(SpinnerTicker::new()).group(SPINNER_GROUP));
    }
}

/// Routes the confirmation modal: Allow (button / Enter / 'y') runs the tools;
/// Deny (button / Esc / 'n') denies them. Both build the tool_result message and
/// re-send the grown history, looping the turn.
fn handle_modal(app: &mut Agent, update: &Update<'_, Msg>) {
    let allow_button =
        update.outcome_for(&[key("modal"), key("allow")]) == Some(&Outcome::Activated);
    let deny_button =
        update.outcome_for(&[key("modal"), key("deny")]) == Some(&Outcome::Activated);

    // Key affordances on keys the buttons didn't consume, sourced from the same
    // keymap: y allows; n / Esc deny. These are printable/Esc chords, so they are
    // `consumed()`-guarded — a key a focused widget took is never re-interpreted.
    let (mut key_allow, mut key_deny) = (false, false);
    if let Event::Input(input) = update.event()
        && !update.consumed()
        && let Some(press) = input.as_key()
    {
        match KEYMAP.action_for(press) {
            Some(Action::Allow) => key_allow = true,
            Some(Action::Deny | Action::Dismiss) => key_deny = true,
            _ => {}
        }
    }

    if allow_button || key_allow {
        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let outcomes = run_pending(app, &root);
        resend(app, update, outcomes);
    } else if deny_button || key_deny {
        let outcomes = deny_pending(app);
        resend(app, update, outcomes);
    }
}

/// Folds the resolved tool outcomes into history and re-sends the grown
/// conversation, re-arming the spinner — the continuation round-trip.
fn resend(app: &mut Agent, update: &Update<'_, Msg>, outcomes: Vec<ToolOutcome>) {
    let Some(request) = continue_with_results(app, outcomes) else {
        // The cap was hit: `continue_with_results` recorded the notice and left
        // no streaming turn open.
        stop_spinner(app, update);
        return;
    };
    let stream = app.backend.send(request);
    update.spawn(Cmd::stream(stream.map(Msg::Event)).group(AGENT_GROUP));
    if !app.ticking {
        app.ticking = true;
        update.spawn(Cmd::stream(SpinnerTicker::new()).group(SPINNER_GROUP));
    }
}

/// Aborts the running turn, discarding any partial prose, and stops the spinner.
fn cancel(app: &mut Agent, update: &Update<'_, Msg>) {
    update.spawn(Cmd::<Msg>::cancel_group(AGENT_GROUP));
    app.streaming = None;
    app.awaiting = None;
    stop_spinner(app, update);
}

/// Stops the spinner ticker when the turn goes idle.
fn stop_spinner(app: &mut Agent, update: &Update<'_, Msg>) {
    if app.ticking {
        app.ticking = false;
        update.spawn(Cmd::<Msg>::cancel_group(SPINNER_GROUP));
    }
}

/// Commits any not-yet-committed cells to native scrollback, in inline mode.
fn flush_commits(app: &mut Agent, update: &Update<'_, Msg>) {
    if !app.inline {
        return;
    }
    let end = committable_end(&app.cells, app.committed);
    while app.committed < end {
        for line in commit_lines_for(&app.cells[app.committed]) {
            update.commit(line);
        }
        app.committed += 1;
    }
}

/// The exclusive end index up to which cells from `committed` may be committed to
/// native scrollback now: every cell until (not including) the first Tool cell
/// that has not settled. The inline engine commits each cell once and cannot
/// rewrite it, so a Pending/Running Tool cell — and everything after it — must
/// wait, or its in-progress glyph would freeze in scrollback instead of showing
/// the eventual result.
fn committable_end(cells: &[TranscriptCell], committed: usize) -> usize {
    let mut end = committed;
    while end < cells.len() {
        if let TranscriptCell::Tool { status, .. } = &cells[end]
            && !status.is_terminal()
        {
            break;
        }
        end += 1;
    }
    end
}

/// Appends any new history messages to the session file (best effort).
fn persist_history(app: &mut Agent) {
    if app.session.is_none() || app.persisted >= app.history.len() {
        return;
    }
    let new: Vec<ChatMessage> = app.history[app.persisted..].to_vec();
    let session = app.session.as_mut().expect("session present");
    for message in &new {
        if message.role == ApiRole::User {
            let text = message.text();
            if !text.trim().is_empty() {
                session.set_title_if_empty(title_from(&text));
            }
        }
        if let Err(error) = session.append(message) {
            eprintln!("rabbit: could not persist to session file: {error}");
        }
    }
    app.persisted = app.history.len();
}

/// A short session title from the first user prompt (first line, truncated).
fn title_from(prompt: &str) -> String {
    let line = prompt.lines().next().unwrap_or("").trim();
    if line.chars().count() > 60 {
        line.chars().take(57).chain("…".chars()).collect()
    } else {
        line.to_string()
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

/// Declares the frame for the active mode, plus the confirmation modal over it.
pub fn view(app: &Agent, frame: &mut Frame<'_>) {
    if app.inline {
        view_inline(app, frame);
    } else {
        view_alt(app, frame);
    }
    if app.is_confirming() {
        view_modal(app, frame);
    }
    // The help overlay sits above everything, including the confirm modal, so a
    // user can always summon the reference card.
    if app.showing_help {
        view_help(frame);
    }
}

/// The generated help overlay, on a z-layer over the transcript (the same
/// `Frame::layer` pattern the confirm modal uses). Rows are GENERATED from the
/// one [`Keymap`] table — no hand-maintained list — as two aligned columns
/// (chord, action). Esc (or the Help chord) closes it.
///
/// Responsive like the modal: everything derives from `frame.area()`, so a
/// resize just recomputes. In inline mode the frame is only the bounded tail
/// ([`TAIL_HEIGHT`] rows); the row list truncates with an "…and N more" line
/// rather than clipping the panel chrome or the footer hint.
fn view_help(frame: &mut Frame<'_>) {
    use rabbitui_core::layout::center;

    let rows = base_help_rows();
    // The HelpOverlay widget owns the two-column layout, panel chrome, and the
    // responsive "…and N more" truncation this function used to hand-roll.
    let overlay = HelpOverlay::new(&rows).title("keys — Esc to close");

    // Size the layer to fit the rows, clamped to the frame — in inline mode the
    // frame is only the bounded tail (TAIL_HEIGHT rows), and the widget truncates
    // when the area is shorter than the list.
    let avail = frame.area();
    let content_width = rows
        .iter()
        .map(|(chord, label)| chord.len() + COLUMN_GAP + label.len())
        .max()
        .unwrap_or(0);
    let max_width = avail.size.width.saturating_sub(spacing::OVERLAY_MARGIN * 2);
    // Panel chrome (border + padding) eats 2 cols each side; +2 slack so the
    // widget's column layout has room and long labels are not clipped.
    let width = (content_width as u16 + 6).clamp(20, max_width.max(20));
    // Chrome (border 2 + padding 2 + title 1) plus one row per binding.
    let height = (rows.len() as u16 + 5).min(avail.size.height);
    let area = center(avail, width, height);

    frame.layer(key("help"), |help| {
        help.widget(key("panel"), area, &overlay);
    });
}

/// The gap between the chord column and the action column in the help overlay
/// (matches [`HelpOverlay`]'s default column gap).
const COLUMN_GAP: usize = 2;

/// The confirmation modal, on a z-layer over the transcript (the `form.rs`
/// pattern): a centered focused panel listing the pending tool call(s) with
/// Allow / Deny buttons. Focus is moved into it via the declare-then-focus
/// handshake in `update`.
fn view_modal(app: &Agent, frame: &mut Frame<'_>) {
    use rabbitui_core::geometry::{Position, Size};
    use rabbitui_core::layout::{center, split_columns, split_rows};

    let calls = app
        .awaiting
        .as_ref()
        .map(|awaiting| awaiting.calls.as_slice())
        .unwrap_or_default();

    // Fit the modal inside the frame, which in inline mode is only the bounded
    // tail (TAIL_HEIGHT rows). Everything derives from the live area, so a resize
    // just recomputes — and the prompt and button rows are always reserved, so
    // many calls (or a short terminal) never clip the y/n away; the call list
    // truncates with an "…and N more" line instead.
    let avail = frame.area();
    let width = avail
        .size
        .width
        .saturating_sub(spacing::OVERLAY_MARGIN * 2)
        .clamp(24, 60);
    // Panel chrome (border + padding) eats 4 rows; prompt + buttons eat 2.
    // Whatever remains is the call list, at least one row.
    let chrome = 4u16;
    let reserved = chrome + 2;
    let call_capacity = avail.size.height.saturating_sub(reserved).max(1);
    let call_rows = (calls.len() as u16).clamp(1, call_capacity);
    let truncated = calls.len() as u16 > call_rows;
    // When truncated, the last visible row is the "…and N more" summary.
    let individual = if truncated {
        call_rows.saturating_sub(1) as usize
    } else {
        call_rows as usize
    };
    let height = (reserved + call_rows).min(avail.size.height);
    let area = center(avail, width, height);

    frame.layer(key("modal"), |modal| {
        let panel = Panel::new()
            .title("confirm tool use")
            .padding(spacing::PANEL_PADDING)
            .focused(true);
        modal.widget(key("bg"), area, &panel);
        let inner = Panel::inner(area, &panel);

        let [prompt_row, calls_block, button_row] = split_rows(inner, [
            Constraint::Length(1),
            Constraint::Length(call_rows),
            Constraint::Length(1),
        ]);

        modal.widget(
            key("prompt"),
            prompt_row,
            &Text::new("The agent wants to run:").role(Role::Warning),
        );
        let call_row = |offset: u16| {
            Rect::new(
                Position::new(calls_block.origin.x, calls_block.origin.y + offset),
                Size::new(calls_block.size.width, 1),
            )
        };
        for (offset, call) in calls.iter().take(individual).enumerate() {
            let input =
                serde_json::from_str(&call.input_json).unwrap_or(serde_json::Value::Null);
            let summary = crate::tools::summarize(&call.name, &input);
            modal.widget(
                key("call").index(offset),
                call_row(offset as u16),
                &Text::new(format!("  • {summary}")).role(Role::Accent),
            );
        }
        if truncated {
            let more = calls.len() - individual;
            modal.widget(
                key("more"),
                call_row(individual as u16),
                &Text::new(format!("  …and {more} more")).role(Role::Muted),
            );
        }
        let [allow_col, deny_col] =
            split_columns(button_row, [Constraint::Fill(1), Constraint::Fill(1)]);
        modal.widget(key("allow"), allow_col, &Button::new("Allow (y)").filled(true));
        modal.widget(key("deny"), deny_col, &Button::new("Deny (n)").filled(true));
    });
}

/// The inline live tail: a streaming preview, status, composer, and hint.
/// Everything above is committed scrollback the terminal owns.
fn view_inline(app: &Agent, frame: &mut Frame<'_>) {
    let [preview, status_row, composer_row, hint_row] = frame.rows([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);
    // Anchor the preview to the bottom so a reply taller than this region streams
    // its newest lines into view instead of filling the top and then appearing to
    // freeze until the finished message commits to scrollback.
    frame.widget(
        key("preview"),
        preview,
        &Text::new(preview_text(app))
            .wrap(true)
            .role(Role::Text)
            .anchor_bottom(true),
    );
    render_footer(app, frame, status_row, composer_row, hint_row);
}

/// The alt-screen transcript: a scrollable column of cells in a titled panel,
/// with the status/composer/hint pinned below.
fn view_alt(app: &Agent, frame: &mut Frame<'_>) {
    let [transcript_area, status_row, composer_row, hint_row] = frame.rows([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);
    let panel = Panel::new()
        .title("transcript")
        .padding(spacing::PANEL_PADDING)
        .focused(true);
    frame.widget(key("panel"), transcript_area, &panel);
    let inner = Panel::inner(transcript_area, &panel);
    frame.scroll(key("transcript"), inner, |scroll| {
        for (index, cell) in app.cells.iter().enumerate() {
            declare_cell(scroll, index, cell);
        }
    });
    render_footer(app, frame, status_row, composer_row, hint_row);
}

/// The streaming preview text: the in-progress prose, or thinking if prose has
/// not started yet.
fn preview_text(app: &Agent) -> String {
    let Some(streaming) = app.streaming.as_ref() else {
        return String::new();
    };
    if streaming.source.trim().is_empty() && !streaming.thinking.trim().is_empty() {
        streaming.thinking.clone()
    } else {
        streaming.source.clone()
    }
}

/// Declares one transcript cell as a scroll item.
fn declare_cell(
    scroll: &mut rabbitui_core::scroll::ScrollScope<'_, '_>,
    index: usize,
    cell: &TranscriptCell,
) {
    let cell_key = key("cell").index(index);
    match cell {
        TranscriptCell::User(prompt) => {
            scroll.item(cell_key, &Text::new(format!("❯ {prompt}")).role(Role::Accent));
        }
        TranscriptCell::Assistant(source) => {
            scroll.item(cell_key, &Text::new(source).wrap(true).role(Role::Text));
        }
        TranscriptCell::Thinking(text) => {
            scroll.item(cell_key, &Text::new(text).wrap(true).role(Role::Muted));
        }
        TranscriptCell::Tool {
            name,
            summary,
            output,
            ..
        } => {
            let header = if summary.contains(name.as_str()) {
                summary.clone()
            } else {
                format!("{summary} ({name})")
            };
            scroll.item(
                cell_key,
                &Collapsible::new(&header, output).default_collapsed(true),
            );
        }
        TranscriptCell::Error(message) => {
            scroll.item(cell_key, &Text::new(format!("⚠ {message}")).role(Role::Danger));
        }
    }
}

/// The status, composer, and hint rows common to both modes.
fn render_footer(
    app: &Agent,
    frame: &mut Frame<'_>,
    status_row: Rect,
    composer_row: Rect,
    hint_row: Rect,
) {
    frame.widget(
        key("status"),
        status_row,
        &Text::new(status_line(app)).role(status_role(app)),
    );
    frame.widget(
        key("composer"),
        composer_row,
        &TextInput::new().placeholder("Tab, type a prompt, Enter…"),
    );
    frame.widget(key("hint"), hint_row, &Text::new(hint_line()).role(Role::Muted));
}

/// The one-line footer hint, generated from the keymap so it never drifts from
/// the real bindings. Shows the send chord, the mode toggle, and how to reach
/// the full help card.
fn hint_line() -> String {
    let chord = |action: Action| {
        KEYMAP
            .chords_for(action)
            .first()
            .map_or_else(String::new, |chord| chord.display())
    };
    // The help hint shows the works-today alias (last chord), not the decided
    // Ctrl-/ (first chord) that the current substrate cannot decode — the overlay
    // itself lists both. See `keymap`'s substrate note.
    let help_chord = KEYMAP
        .chords_for(Action::Help)
        .last()
        .map_or_else(String::new, |chord| chord.display());
    format!(
        "{}: send  {}: mode  {help_chord}: help  {}: quit",
        chord(Action::Send),
        chord(Action::ToggleMode),
        chord(Action::Quit),
    )
}

/// The status line: mode, agent state, and a spinner while streaming.
fn status_line(app: &Agent) -> String {
    let mode = if app.inline { "inline" } else { "alt-screen browse" };
    if app.showing_help {
        format!("[{mode}]  help · Esc to close")
    } else if app.is_confirming() {
        format!("[{mode}]  awaiting tool confirmation · y/n")
    } else if app.is_streaming() {
        let spinner = SPINNER[app.spinner];
        let tool = app
            .streaming
            .as_ref()
            .and_then(|streaming| streaming.running_tool.as_deref())
            .map_or_else(String::new, |name| format!(" · running {name}"));
        format!("[{mode}]  {spinner} streaming{tool}")
    } else {
        format!("[{mode}]  idle · {} cells", app.cells.len())
    }
}

/// The status line's role: warning while confirming, accent while streaming,
/// success when idle.
fn status_role(app: &Agent) -> Role {
    if app.is_confirming() {
        Role::Warning
    } else if app.is_streaming() {
        Role::Accent
    } else {
        Role::Success
    }
}

/// Builds and runs the app over `backend`.
///
/// # Errors
///
/// Propagates any terminal error from the run loop.
pub async fn run(app: Agent) -> rabbitui::app::Result<()> {
    run_themed(app, ThemeConfig::default()).await
}

/// How to theme the app: an optional base preset plus an optional TOML override
/// file (the facade's `theme_file` path). Either or both may be set; a file
/// layers its roles over the base ([`Theme::default`] when no base is given).
#[derive(Debug, Clone, Default)]
pub struct ThemeConfig {
    /// The base theme (a built-in preset). `None` means [`Theme::default`].
    pub base: Option<Theme>,
    /// A TOML theme file whose `[roles]` layer over `base`. Loaded once at
    /// startup and (in debug builds) hot-reloaded on change by the facade.
    pub file: Option<std::path::PathBuf>,
}

/// Builds and runs the app under a theme configuration.
///
/// Wires the facade's theme path end to end: `--theme <file>` becomes
/// [`ThemeConfig::file`], loaded via [`App::theme_file`] (the facade parses the
/// TOML into a [`Theme`] over the base, and hot-reloads it in debug builds).
///
/// # Errors
///
/// Propagates terminal errors, and any theme-file load/parse error at startup.
pub async fn run_themed(app: Agent, theme: ThemeConfig) -> rabbitui::app::Result<()> {
    let mut builder = App::new(app, update, view).mode(Mode::inline(TAIL_HEIGHT));
    if let Some(base) = theme.base {
        builder = builder.theme(base);
    }
    if let Some(file) = theme.file {
        builder = builder.theme_file(file);
    }
    builder.run().await
}

// ---------------------------------------------------------------------------
// Spinner ticker
// ---------------------------------------------------------------------------

/// A ticker driving the streaming spinner, one [`Msg::Tick`] every ~120ms.
struct SpinnerTicker {
    /// The underlying interval.
    interval: tokio::time::Interval,
}

impl SpinnerTicker {
    /// A ticker firing every ~120ms.
    fn new() -> Self {
        Self {
            interval: tokio::time::interval(Duration::from_millis(120)),
        }
    }
}

impl Stream for SpinnerTicker {
    type Item = Msg;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Msg>> {
        match self.get_mut().interval.poll_tick(cx) {
            Poll::Ready(_) => Poll::Ready(Some(Msg::Tick)),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::ToolStatus;

    fn tool(status: ToolStatus) -> TranscriptCell {
        TranscriptCell::Tool {
            name: "read_file".to_string(),
            summary: "read_file(x)".to_string(),
            output: String::new(),
            status,
        }
    }

    #[test]
    fn committable_end_holds_at_a_non_terminal_tool_cell() {
        let cells = vec![
            TranscriptCell::User("hi".to_string()),
            TranscriptCell::Assistant("reading".to_string()),
            tool(ToolStatus::Pending),
            TranscriptCell::Assistant("done".to_string()),
        ];
        // The user + assistant prose commit; the Pending tool cell (and the reply
        // after it) are held.
        assert_eq!(committable_end(&cells, 0), 2);
    }

    #[test]
    fn committable_end_releases_once_the_tool_settles() {
        let cells = vec![
            TranscriptCell::User("hi".to_string()),
            tool(ToolStatus::Ok),
            TranscriptCell::Assistant("done".to_string()),
        ];
        // A settled tool cell no longer blocks: everything is committable.
        assert_eq!(committable_end(&cells, 0), 3);
        // A Running cell still blocks.
        let running = vec![tool(ToolStatus::Running)];
        assert_eq!(committable_end(&running, 0), 0);
    }
}
