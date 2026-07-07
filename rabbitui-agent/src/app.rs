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
use rabbitui_core::input::Key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::mode::Mode;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::theme::Role;
use rabbitui_widgets::{Collapsible, Panel, Text, TextInput};

use crate::backend::{Backend, ChatMessage, ChatRequest, Role as ApiRole, StreamEvent};
use crate::session::Session;
use crate::transcript::{Streaming, TranscriptCell, commit_lines_for};

/// The bounded live-tail height in inline mode, in rows.
pub const TAIL_HEIGHT: u16 = 8;

/// The cancel-previous group each agent turn is spawned into, so a new prompt (or
/// a cancel) aborts the running stream.
const AGENT_GROUP: &str = "agent";
/// The independently-cancellable group the spinner ticker runs in.
const SPINNER_GROUP: &str = "spinner";
/// The spinner frames cycled while streaming.
const SPINNER: [&str; 4] = ["⠋", "⠙", "⠹", "⠸"];
/// The one-line help under the composer.
const HINT: &str = "Enter: send  Ctrl-X: cancel  Ctrl-T: mode  Ctrl-C: quit";

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
        }
    }

    /// Enables persistence to `session`, seeding history from a resumed file.
    #[must_use]
    pub fn with_session(mut self, session: Session, resumed: Vec<ChatMessage>) -> Self {
        for message in resumed {
            let cell = match message.role {
                ApiRole::User => TranscriptCell::User(message.content.clone()),
                ApiRole::Assistant => TranscriptCell::Assistant(message.content.clone()),
            };
            self.cells.push(cell);
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
/// reducer cannot (stop the spinner, persist).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    /// Streaming continues (or nothing happened).
    None,
    /// The turn finished.
    TurnComplete,
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
        // Tool execution is slice 4; slice 1 only reflects a running tool in the
        // status line and otherwise ignores tool events.
        StreamEvent::ToolUseStart { name, .. } => {
            if let Some(streaming) = app.streaming.as_mut() {
                streaming.running_tool = Some(name);
            }
            Reaction::None
        }
        StreamEvent::ToolUseInputDelta { .. } | StreamEvent::ToolUseStop { .. } => Reaction::None,
        // Slice 1 treats every stop reason as end-of-turn; the tool-continuation
        // loop for `tool_use` arrives in slice 4.
        StreamEvent::MessageDone { .. } => {
            finish_turn(app);
            Reaction::TurnComplete
        }
    }
}

/// Commits the streaming turn's thinking and prose as transcript cells, then
/// appends the assistant prose to history and clears the streaming state.
fn finish_turn(app: &mut Agent) {
    let Some(streaming) = app.streaming.take() else {
        return;
    };
    if !streaming.thinking.trim().is_empty() {
        app.cells.push(TranscriptCell::Thinking(streaming.thinking));
    }
    if !streaming.source.trim().is_empty() {
        app.cells.push(TranscriptCell::Assistant(streaming.source.clone()));
        app.history.push(ChatMessage::assistant(streaming.source));
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
        let reaction = apply_message(app, message.clone());
        if reaction == Reaction::TurnComplete {
            stop_spinner(app, &update);
        }
    }

    // App-level key bindings on keys no focused widget consumed.
    if let Event::Input(input) = update.event()
        && !update.consumed()
        && let Some(press) = input.as_key()
    {
        match press.key {
            Key::Char('t') if press.modifiers.ctrl => toggle_mode(app, &update),
            Key::Char('m') if !press.modifiers.ctrl => toggle_mode(app, &update),
            Key::Char('x') if press.modifiers.ctrl => {
                if app.is_streaming() {
                    cancel(app, &update);
                }
            }
            Key::Escape => {
                if app.is_streaming() {
                    cancel(app, &update);
                }
            }
            Key::Char('q') if !press.modifiers.ctrl => return ControlFlow::Break(()),
            Key::Char('c') if press.modifiers.ctrl => return ControlFlow::Break(()),
            _ => {}
        }
    }

    flush_commits(app, &update);
    persist_history(app);
    ControlFlow::Continue(())
}

/// Toggles inline ↔ alt-screen and switches the runtime mode.
fn toggle_mode(app: &mut Agent, update: &Update<'_, Msg>) {
    app.inline = !app.inline;
    update.set_mode(if app.inline {
        Mode::inline(TAIL_HEIGHT)
    } else {
        Mode::AltScreen
    });
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

/// Aborts the running turn, discarding any partial prose, and stops the spinner.
fn cancel(app: &mut Agent, update: &Update<'_, Msg>) {
    update.spawn(Cmd::<Msg>::cancel_group(AGENT_GROUP));
    app.streaming = None;
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
    while app.committed < app.cells.len() {
        for line in commit_lines_for(&app.cells[app.committed]) {
            update.commit(line);
        }
        app.committed += 1;
    }
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
            session.set_title_if_empty(title_from(&message.content));
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

/// Declares the frame for the active mode.
pub fn view(app: &Agent, frame: &mut Frame<'_>) {
    if app.inline {
        view_inline(app, frame);
    } else {
        view_alt(app, frame);
    }
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
    frame.widget(
        key("preview"),
        preview,
        &Text::new(preview_text(app)).wrap(true).role(Role::Text),
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
    let panel = Panel::new().title("transcript").padding(1).focused(true);
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
    frame.widget(key("hint"), hint_row, &Text::new(HINT).role(Role::Muted));
}

/// The status line: mode, agent state, and a spinner while streaming.
fn status_line(app: &Agent) -> String {
    let mode = if app.inline { "inline" } else { "alt-screen" };
    if app.is_streaming() {
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

/// The status line's role: accent while streaming, success when idle.
fn status_role(app: &Agent) -> Role {
    if app.is_streaming() {
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
    App::new(app, update, view)
        .mode(Mode::inline(TAIL_HEIGHT))
        .run()
        .await
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
