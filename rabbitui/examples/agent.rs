//! The agent-chrome flagship: a simulated coding-agent chat — the slice-8 acceptance test.
//!
//! One app state, two viewport philosophies (`docs/design/slice8-agent-chrome.md`):
//!
//! - **Inline mode** (default) commits each finished transcript cell into the
//!   terminal's *native* scrollback — markdown rendered to per-span SGR — so
//!   history is the terminal's (scroll, select, copy, reflow). The in-progress
//!   assistant message renders in a bounded live tail, soft-wrapped to width, and
//!   commits only when it completes (append-once by construction). A tool call
//!   commits a one-line summary; its full output is kept in app state and is
//!   viewable only in alt-screen — committed scrollback is immutable, the honest
//!   inline tradeoff.
//! - **Alt-screen mode** (`m` toggles) renders the *same* transcript as a
//!   retained, scrollable column of [`Collapsible`] cells: tool cells default
//!   collapsed, assistant cells expanded. Up/Down/PageUp/PageDown scroll it.
//!
//! The status line shows the mode, the agent state, and a spinner while
//! streaming. The composer is a [`TextInput`]; Enter sends a prompt and spawns a
//! deterministic simulated agent response (a `Cmd::stream` of chunked markdown,
//! a tool-call interlude, then completion). `Ctrl-X` (or `Esc`, once the substrate flushes lone escapes) cancels a streaming response
//! via `Cmd::cancel_group("agent")` — which also covers re-prompting mid-stream.
//! `q`/Ctrl-C quit.
//!
//! Markdown → spans lives here, in app-land, over `pulldown-cmark` (a
//! dev-dependency): rendering markdown is not the framework's job until the
//! catalog grows a real widget (the design note's deliberate boundary).
//!
//! Run with `cargo run --example agent`, type a prompt, and press `m` to compare
//! the two histories.
//!
//! Note (substrate gap): the composer is reached via Tab; while focused it
//! consumes printable keys, so `m`/`q` and the transcript scroll keys require
//! Tab-ing focus away first. `Esc` and Enter work regardless.

use std::collections::VecDeque;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_core::Stream;
use pulldown_cmark::{CodeBlockKind, Event as MdEvent, Parser, Tag, TagEnd};
use rabbitui::App;
use rabbitui::app::{Event, Update};
use rabbitui::effect::Cmd;
use rabbitui_core::commit::CommitLine;
use rabbitui_core::frame::Frame;
use rabbitui_core::id::{Key as WidgetKey, key};
use rabbitui_core::input::Key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::mode::Mode;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::style::{Attrs, Color, Style};
use rabbitui_core::text::Span;
use rabbitui_core::theme::Role;
use rabbitui_widgets::{Collapsible, Panel, Text, TextInput};

/// The bounded live-tail height in inline mode, in rows: the streaming preview
/// region (a few rows) plus the status line plus the composer plus a hint.
const TAIL_HEIGHT: u16 = 8;

/// The cancel-previous group every simulated agent run is spawned into, so a new
/// prompt (or `Esc`) aborts the running stream (`Cmd::cancel_group("agent")`).
const AGENT_GROUP: &str = "agent";

/// The spinner frames cycled while the agent is streaming.
const SPINNER: [&str; 4] = ["⠋", "⠙", "⠹", "⠸"];

// ---------------------------------------------------------------------------
// Transcript model
// ---------------------------------------------------------------------------

/// The status a finished tool call reports, driving its summary's style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolStatus {
    /// The tool succeeded.
    Ok,
    /// The tool failed.
    Failed,
}

/// One cell of the transcript — the shared source of truth for both modes.
#[derive(Debug, Clone)]
enum TranscriptCell {
    /// A prompt the user sent.
    User(String),
    /// A completed assistant message, held as its markdown source.
    Assistant { source: String },
    /// A completed tool call: its name, one-line summary, full output, status.
    Tool {
        name: String,
        summary: String,
        output: String,
        status: ToolStatus,
    },
}

/// The in-flight assistant turn: accumulated prose and an optional running tool.
#[derive(Debug, Default, Clone)]
struct Streaming {
    /// The markdown source accumulated from stream chunks so far.
    source: String,
    /// The name of a tool currently running (start seen, finish not yet), if any.
    running_tool: Option<String>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// A message the simulated agent stream (or the spinner ticker) delivers.
#[derive(Debug, Clone)]
enum Msg {
    /// A chunk of assistant prose (markdown source) to append to the live tail.
    Chunk(String),
    /// A tool call started; carries the tool's name.
    ToolStarted(String),
    /// A tool call finished with a summary, full output, and status.
    ToolFinished {
        name: String,
        summary: String,
        output: String,
        status: ToolStatus,
    },
    /// The assistant turn completed; commit the accumulated prose.
    Complete,
    /// The spinner ticker fired.
    Tick,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

/// The whole app's owned state.
struct Agent {
    /// The committed transcript, in order — the same cells both modes render.
    cells: Vec<TranscriptCell>,
    /// The in-flight assistant turn, if the agent is currently streaming.
    streaming: Option<Streaming>,
    /// Whether the app is in inline mode (vs alt-screen).
    inline: bool,
    /// The composer draft, tracked from `Changed` outcomes.
    draft: String,
    /// Re-keys the composer to clear it after a submit (the uncontrolled-input
    /// workaround the slice-4 note records; kept here for a single-file example).
    input_generation: u64,
    /// The spinner animation frame.
    spinner: usize,
    /// Whether the spinner ticker stream is currently running.
    ticking: bool,
}

impl Default for Agent {
    fn default() -> Self {
        Self {
            cells: Vec::new(),
            streaming: None,
            inline: true,
            draft: String::new(),
            input_generation: 0,
            spinner: 0,
            ticking: false,
        }
    }
}

impl Agent {
    /// Whether the agent is currently streaming a response.
    fn is_streaming(&self) -> bool {
        self.streaming.is_some()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    App::new(Agent::default(), update, view)
        .mode(Mode::inline(TAIL_HEIGHT))
        .run()
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

/// Folds one update into the app: send prompts, absorb the stream, toggle mode,
/// scroll, cancel, quit.
fn update(app: &mut Agent, update: Update<'_, Msg>) -> ControlFlow<()> {
    // Track the composer draft; a submit sends a prompt and spawns the agent.
    if let Some(Outcome::Changed(value)) = update.outcome_for(&[composer_key(app)]) {
        app.draft = value.clone();
    }
    if update.outcome_for(&[composer_key(app)]) == Some(&Outcome::Submitted) {
        submit_prompt(app, &update);
    }

    // Absorb simulated-agent stream messages and the spinner tick.
    if let Event::Message(message) = update.event() {
        handle_message(app, &update, message.clone());
    }

    // App-level key bindings on keys no focused widget consumed
    // (Update::consumed — the composer eats printables while focused).
    if let Event::Input(input) = update.event()
        && !update.consumed()
        && let Some(k) = input.as_key()
    {
        match k.key {
            // Ctrl-T: mode toggle that works even while the composer is
            // focused (printable 'm' below only fires when it is not).
            Key::Char('t') if k.modifiers.ctrl => {
                app.inline = !app.inline;
                update.set_mode(if app.inline {
                    Mode::inline(TAIL_HEIGHT)
                } else {
                    Mode::AltScreen
                });
            }
            // Ctrl-X: cancel that works while the composer is focused
            // (lone Esc is currently held by the substrate's parser —
            // see the requirements handover).
            Key::Char('x') if k.modifiers.ctrl => {
                if app.is_streaming() {
                    cancel_agent(app, &update);
                }
            }
            // Toggle inline ↔ alt-screen live. The alt-screen transcript
            // owns its own scroll via the frame.scroll scope.
            Key::Char('m') if !k.modifiers.ctrl => {
                app.inline = !app.inline;
                update.set_mode(if app.inline {
                    Mode::inline(TAIL_HEIGHT)
                } else {
                    Mode::AltScreen
                });
            }
            // Esc cancels a streaming response (cancel-previous also covers
            // re-prompting mid-stream); a no-op when idle.
            Key::Escape => {
                if app.is_streaming() {
                    cancel_agent(app, &update);
                }
            }
            // The alt-screen transcript now owns its own scroll: the
            // `frame.scroll` scope consumes Up/Down/PageUp/PageDown/Home/End
            // and the wheel while focused, so the app no longer tracks an
            // offset or handles those keys here.
            Key::Char('q') if !k.modifiers.ctrl => return ControlFlow::Break(()),
            Key::Char('c') if k.modifiers.ctrl => return ControlFlow::Break(()),
            _ => {}
        }
    }

    ControlFlow::Continue(())
}

/// Sends the composer's draft as a user prompt and spawns the simulated agent.
///
/// The prompt cell is pushed (and committed in inline mode); the agent stream is
/// spawned into the cancel-previous `agent` group, so sending again mid-stream
/// aborts the previous run. The spinner ticker starts alongside it.
fn submit_prompt(app: &mut Agent, update: &Update<'_, Msg>) {
    let prompt = app.draft.trim().to_string();
    // Clear the composer regardless (re-key), and reset the tracked draft.
    app.input_generation += 1;
    app.draft.clear();
    if prompt.is_empty() {
        return;
    }

    push_cell(app, update, TranscriptCell::User(prompt.clone()));

    // Begin a fresh streaming turn and spawn the deterministic agent stream. The
    // alt-screen scroll pins to the bottom on new content via a scroll-into-view
    // request from the newest cell, so no app-side offset is tracked.
    app.streaming = Some(Streaming::default());
    update.spawn(Cmd::stream(agent_stream(&prompt)).group(AGENT_GROUP));

    // Start the spinner ticker (its own group so it is independently cancellable).
    if !app.ticking {
        app.ticking = true;
        update.spawn(Cmd::stream(SpinnerTicker::new()).group("spinner"));
    }
}

/// Aborts the running agent stream and drops the in-flight turn.
fn cancel_agent(app: &mut Agent, update: &Update<'_, Msg>) {
    update.spawn(Cmd::<Msg>::cancel_group(AGENT_GROUP));
    // Any prose streamed so far is discarded (a cancelled turn commits nothing) —
    // the append-once rule holds: only completion commits.
    if let Some(streaming) = app.streaming.take() {
        // A cancelled tool leaves a failed summary in the transcript so the record
        // is honest about what was interrupted.
        if let Some(name) = streaming.running_tool {
            push_cell(
                app,
                update,
                TranscriptCell::Tool {
                    name: name.clone(),
                    summary: format!("▸ {name} — cancelled"),
                    output: "(cancelled by Esc)".to_string(),
                    status: ToolStatus::Failed,
                },
            );
        }
    }
    stop_spinner(app, update);
}

/// Folds one stream (or ticker) message into the app.
fn handle_message(app: &mut Agent, update: &Update<'_, Msg>, message: Msg) {
    match message {
        Msg::Chunk(chunk) => {
            if let Some(streaming) = app.streaming.as_mut() {
                streaming.source.push_str(&chunk);
            }
        }
        Msg::ToolStarted(name) => {
            if let Some(streaming) = app.streaming.as_mut() {
                streaming.running_tool = Some(name);
            }
        }
        Msg::ToolFinished {
            name,
            summary,
            output,
            status,
        } => {
            if let Some(streaming) = app.streaming.as_mut() {
                streaming.running_tool = None;
                // Flush any prose accumulated before the tool as its own assistant
                // cell, so the tool cell lands between prose blocks in order.
                flush_prose(app, update);
            }
            push_cell(
                app,
                update,
                TranscriptCell::Tool {
                    name,
                    summary,
                    output,
                    status,
                },
            );
        }
        Msg::Complete => {
            flush_prose(app, update);
            app.streaming = None;
            stop_spinner(app, update);
        }
        Msg::Tick => {
            app.spinner = (app.spinner + 1) % SPINNER.len();
        }
    }
}

/// Commits the streaming turn's accumulated prose as an assistant cell, if any,
/// and clears the accumulator (leaving the turn open for post-tool prose).
fn flush_prose(app: &mut Agent, update: &Update<'_, Msg>) {
    let source = match app.streaming.as_mut() {
        Some(streaming) if !streaming.source.trim().is_empty() => {
            std::mem::take(&mut streaming.source)
        }
        _ => return,
    };
    push_cell(app, update, TranscriptCell::Assistant { source });
}

/// Stops the spinner ticker stream (for good) when the agent goes idle.
fn stop_spinner(app: &mut Agent, update: &Update<'_, Msg>) {
    if app.ticking {
        app.ticking = false;
        update.spawn(Cmd::<Msg>::cancel_group("spinner"));
    }
}

/// Pushes a cell into the transcript and, in inline mode, commits it into native
/// scrollback (markdown-rendered to per-span lines for assistant cells, a
/// single styled summary line for tool cells).
///
/// This is the one place the two modes diverge: alt-screen keeps the cell only in
/// `cells` (it re-renders the whole column each frame), while inline additionally
/// commits it once, append-only, exactly as ADR 0013 requires.
fn push_cell(app: &mut Agent, update: &Update<'_, Msg>, cell: TranscriptCell) {
    if app.inline {
        for line in commit_lines_for(&cell) {
            update.commit(line);
        }
    }
    app.cells.push(cell);
}

/// The committed scrollback lines for a transcript cell.
fn commit_lines_for(cell: &TranscriptCell) -> Vec<CommitLine> {
    match cell {
        TranscriptCell::User(prompt) => {
            vec![CommitLine::from_spans([
                Span::styled("❯ ", Style::new().fg(Color::CYAN).bold()),
                Span::styled(prompt.clone(), Style::new().bold()),
            ])]
        }
        TranscriptCell::Assistant { source } => markdown_to_commit_lines(source),
        TranscriptCell::Tool {
            summary, status, ..
        } => {
            let role = match status {
                ToolStatus::Ok => Style::new().fg(Color::GREEN),
                ToolStatus::Failed => Style::new().fg(Color::RED),
            };
            vec![CommitLine::from_spans([Span::styled(
                summary.clone(),
                role,
            )])]
        }
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

/// Declares the frame for the active mode.
fn view(app: &Agent, frame: &mut Frame<'_>) {
    if app.inline {
        view_inline(app, frame);
    } else {
        view_alt(app, frame);
    }
}

/// The inline live tail: a streaming preview (soft-wrapped), the status line, the
/// composer, and a hint. Everything above is committed history the terminal owns.
///
/// This is the inline path, so the tail is *not* wrapped in a panel — the
/// committed transcript above is native scrollback the terminal owns, and a
/// border around a bottom-pinned strip would fight it. Styling stays inside the
/// tail via theme roles.
fn view_inline(app: &Agent, frame: &mut Frame<'_>) {
    let [preview, status_row, composer_row, hint_row] = frame.rows([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);

    // The in-progress message renders from accumulated source, soft-wrapped to
    // width. Committed cells are already in native scrollback above the tail.
    let preview_text = app
        .streaming
        .as_ref()
        .map_or_else(String::new, |streaming| streaming.source.clone());
    frame.widget(
        key("preview"),
        preview,
        &Text::new(&preview_text).wrap(true).role(Role::Text),
    );

    frame.widget(
        key("status"),
        status_row,
        &Text::new(status_line(app)).role(status_role(app)),
    );
    frame.widget(
        composer_key(app),
        composer_row,
        &TextInput::new().placeholder("Tab, type a prompt, Enter…"),
    );
    frame.widget(key("hint"), hint_row, &Text::new(HINT).role(Role::Muted));
}

/// The alt-screen transcript: a scrollable column of collapsible cells inside a
/// titled panel, plus the status line, composer, and hint pinned to the bottom.
///
/// Alt-screen owns the whole viewport (not native scrollback), so here a panel is
/// the right frame: the transcript reads as a bordered, titled chat column, the
/// composer/status/hint sit below it, and the panel highlights because focus
/// lives inside it.
fn view_alt(app: &Agent, frame: &mut Frame<'_>) {
    let [transcript_area, status_row, composer_row, hint_row] = frame.rows([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);

    // The transcript column gets a titled panel; cells render into its inner area.
    let panel = Panel::new().title("transcript").padding(1).focused(true);
    frame.widget(key("panel"), transcript_area, &panel);
    let inner = Panel::inner(transcript_area, &panel);
    render_transcript(app, frame, inner);

    frame.widget(
        key("status"),
        status_row,
        &Text::new(status_line(app)).role(status_role(app)),
    );
    frame.widget(
        composer_key(app),
        composer_row,
        &TextInput::new().placeholder("Tab, type a prompt, Enter…"),
    );
    frame.widget(key("hint"), hint_row, &Text::new(HINT).role(Role::Muted));
}

/// Renders the transcript as a **measured, virtualized scroll** of collapsible
/// cells into `area` (`frame.scroll`).
///
/// This retires the slice-8 hand-rolled fixed-slot stack (the design note's named
/// acceptance case): each cell is now declared as a scroll `item`, measured at its
/// honest [`desired_height`](rabbitui_core::widget::Widget::desired_height) —
/// [`Text`] by its (wrapped) line count, [`Collapsible`] by 1 collapsed and
/// header+body expanded — so the column stacks at real heights, virtualizes to the
/// viewport, and scrolls with Up/Down/PageUp/PageDown/Home/End and the wheel
/// (handled by the scroll scope, not the app). Expanding a tool cell grows its
/// measured height and the stack reflows; the scrollbar appears on overflow.
fn render_transcript(app: &Agent, frame: &mut Frame<'_>, area: rabbitui_core::geometry::Rect) {
    frame.scroll(key("transcript"), area, |scroll| {
        for (index, cell) in app.cells.iter().enumerate() {
            declare_cell(scroll, index, cell);
        }
    });
}

/// Declares one transcript cell as a scroll item.
///
/// User cells are a one-line accent prompt; assistant cells are wrapped prose
/// (measured to their wrapped height); tool cells are [`Collapsible`]s defaulting
/// collapsed, whose full output is revealed by Enter/click — the alt-screen
/// affordance the immutable inline scrollback cannot offer.
fn declare_cell(
    scroll: &mut rabbitui_core::scroll::ScrollScope<'_, '_>,
    index: usize,
    cell: &TranscriptCell,
) {
    let cell_key = key("cell").index(index);
    match cell {
        TranscriptCell::User(prompt) => {
            let text = format!("❯ {prompt}");
            scroll.item(cell_key, &Text::new(&text).role(Role::Accent));
        }
        TranscriptCell::Assistant { source } => {
            scroll.item(cell_key, &Text::new(source).wrap(true).role(Role::Text));
        }
        TranscriptCell::Tool {
            name,
            summary,
            output,
            ..
        } => {
            // The header carries the tool name and its summary; the body (the full
            // output) is collapsed by default and revealed by Enter/click.
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
    }
}

/// The status line text: the mode, the agent state, and a spinner while streaming.
fn status_line(app: &Agent) -> String {
    let mode = if app.inline { "inline" } else { "alt-screen" };
    if app.is_streaming() {
        let spinner = SPINNER[app.spinner];
        let tool = app
            .streaming
            .as_ref()
            .and_then(|s| s.running_tool.as_deref())
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

/// The one-line help/hint under the composer.
const HINT: &str =
    "Enter: send  Ctrl-X: cancel  Ctrl-T: mode  Ctrl-C: quit  (inline scrollback is immutable)";

/// The composer's key for this frame, carrying the generation so a submit re-keys
/// (and clears) it.
fn composer_key(app: &Agent) -> WidgetKey {
    key("composer").index(usize::try_from(app.input_generation).unwrap_or(usize::MAX))
}

// ---------------------------------------------------------------------------
// Markdown → spans (app-land)
// ---------------------------------------------------------------------------

/// Renders markdown `source` into committed transcript lines (per-span SGR).
///
/// Coverage per the design note: headings (bold accent), bold/italic, inline code
/// and fenced code blocks (a dim "code" style, no syntax highlighting), and
/// bullet lists. This is a small, whole-message render — the streaming case
/// re-renders the in-progress source per frame as plain wrapped text, so no
/// incremental markdown parsing is needed.
fn markdown_to_commit_lines(source: &str) -> Vec<CommitLine> {
    let mut render = MarkdownRender::default();
    for event in Parser::new(source) {
        render.event(event);
    }
    render.finish()
}

/// Accumulates markdown events into styled lines.
#[derive(Default)]
struct MarkdownRender {
    /// Completed logical lines.
    lines: Vec<Vec<Span>>,
    /// The line under construction.
    current: Vec<Span>,
    /// The active inline attributes (bold/italic), nested via a stack.
    attrs: Attrs,
    /// The active foreground override for headings/code, if any.
    fg: Option<Color>,
    /// Whether we are inside a fenced code block (each line is a dim code line).
    in_code_block: bool,
    /// Whether we are inside a heading (the whole line is bold accent).
    in_heading: bool,
    /// A pending list-item bullet prefix to emit at the next text.
    bullet_pending: bool,
}

impl MarkdownRender {
    /// Folds one markdown event into the accumulator.
    fn event(&mut self, event: MdEvent<'_>) {
        match event {
            MdEvent::Start(tag) => self.start(tag),
            MdEvent::End(tag) => self.end(tag),
            MdEvent::Text(text) => self.text(&text),
            MdEvent::Code(code) => self.inline_code(&code),
            MdEvent::SoftBreak | MdEvent::HardBreak => self.break_line(),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { .. } => {
                self.in_heading = true;
                self.attrs |= Attrs::BOLD;
                self.fg = Some(Color::CYAN);
            }
            Tag::Emphasis => self.attrs |= Attrs::ITALIC,
            Tag::Strong => self.attrs |= Attrs::BOLD,
            Tag::CodeBlock(CodeBlockKind::Fenced(_) | CodeBlockKind::Indented) => {
                self.break_line();
                self.in_code_block = true;
                self.fg = Some(Color::Ansi(8));
            }
            Tag::Item => self.bullet_pending = true,
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.in_heading = false;
                self.attrs = Attrs::NONE;
                self.fg = None;
                self.break_line();
            }
            TagEnd::Emphasis => self.attrs = self.attrs.remove(Attrs::ITALIC),
            TagEnd::Strong => self.attrs = self.attrs.remove(Attrs::BOLD),
            TagEnd::CodeBlock => {
                self.break_line();
                self.in_code_block = false;
                self.fg = None;
            }
            TagEnd::Paragraph | TagEnd::Item => self.break_line(),
            _ => {}
        }
    }

    /// Appends text in the current style; inside a code block each `'\n'` is a
    /// new line so multi-line code fences keep their shape.
    fn text(&mut self, text: &str) {
        if self.bullet_pending {
            self.current
                .push(Span::styled("• ", Style::new().fg(Color::CYAN)));
            self.bullet_pending = false;
        }
        if self.in_code_block {
            let mut first = true;
            for line in text.split('\n') {
                if !first {
                    self.break_line();
                }
                first = false;
                if !line.is_empty() {
                    self.current
                        .push(Span::styled(line.to_string(), self.style()));
                }
            }
        } else {
            self.current
                .push(Span::styled(text.to_string(), self.style()));
        }
    }

    /// Appends an inline `code` span in a dim code style.
    fn inline_code(&mut self, code: &str) {
        if self.bullet_pending {
            self.current
                .push(Span::styled("• ", Style::new().fg(Color::CYAN)));
            self.bullet_pending = false;
        }
        self.current.push(Span::styled(
            code.to_string(),
            Style::new().fg(Color::Ansi(8)),
        ));
    }

    /// The current inline style from the active attributes and foreground.
    fn style(&self) -> Style {
        let mut style = Style {
            fg: self.fg,
            bg: None,
            attrs: self.attrs,
        };
        if self.in_code_block {
            style.fg = style.fg.or(Some(Color::Ansi(8)));
        }
        style
    }

    /// Ends the current logical line, if it has content.
    fn break_line(&mut self) {
        if !self.current.is_empty() {
            self.lines.push(std::mem::take(&mut self.current));
        }
    }

    /// Finishes rendering, returning one [`CommitLine`] per logical line.
    fn finish(mut self) -> Vec<CommitLine> {
        self.break_line();
        self.lines.into_iter().map(CommitLine::from_spans).collect()
    }
}

// ---------------------------------------------------------------------------
// The simulated agent stream (deterministic)
// ---------------------------------------------------------------------------

/// A deterministic simulated agent response: chunked markdown prose, a tool-call
/// start/finish pair, more prose, then completion.
///
/// The content is seeded from `prompt` so an integration test can assert the
/// transcript exactly. Pacing is realistic (a short interval between chunks) but
/// the *content* is fixed; a paused-clock test advances time to drive it.
fn agent_stream(prompt: &str) -> AgentStream {
    let topic = prompt.trim();
    let mut steps: VecDeque<Step> = VecDeque::new();

    // Opening prose, chunked mid-word to prove the live-tail accumulation.
    steps.push_back(Step::Chunk(format!("## Working on: {topic}\n\n")));
    steps.push_back(Step::Chunk(
        "Let me start by looking at the relevant ".to_string(),
    ));
    steps.push_back(Step::Chunk(
        "code and running the **test suite**.\n".to_string(),
    ));

    // A tool-call interlude: start, a sleep, then finish.
    steps.push_back(Step::ToolStart("cargo test".to_string()));
    steps.push_back(Step::Sleep(Duration::from_millis(400)));
    steps.push_back(Step::ToolFinish {
        name: "cargo test".to_string(),
        summary: "▸ ran cargo test — 396 passed".to_string(),
        output: "running 396 tests\n....\ntest result: ok. 396 passed; 0 failed".to_string(),
        status: ToolStatus::Ok,
    });

    // Closing prose with a bullet list and inline code.
    steps.push_back(Step::Chunk(
        "\nAll green. The change touches:\n\n".to_string(),
    ));
    steps.push_back(Step::Chunk("- the `core::text` span type\n".to_string()));
    steps.push_back(Step::Chunk(
        "- the inline engine's per-span SGR\n\n".to_string(),
    ));
    steps.push_back(Step::Chunk("Done.".to_string()));
    steps.push_back(Step::Complete);

    AgentStream { steps, delay: None }
}

/// One scripted step of the simulated agent.
enum Step {
    /// Emit a prose chunk.
    Chunk(String),
    /// Emit a tool-started message.
    ToolStart(String),
    /// Sleep before the next step (a tool "running").
    Sleep(Duration),
    /// Emit a tool-finished message.
    ToolFinish {
        name: String,
        summary: String,
        output: String,
        status: ToolStatus,
    },
    /// Emit the completion message.
    Complete,
}

/// The scripted agent stream: yields one [`Msg`] per non-sleep step, pacing each
/// with a short inter-chunk delay so the live tail visibly fills.
struct AgentStream {
    steps: VecDeque<Step>,
    /// A pending inter-step sleep, armed after each yielded message.
    delay: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl Stream for AgentStream {
    type Item = Msg;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Msg>> {
        let this = self.get_mut();
        loop {
            // Wait out any armed inter-step delay first.
            if let Some(delay) = this.delay.as_mut() {
                match delay.as_mut().poll(cx) {
                    Poll::Ready(()) => this.delay = None,
                    Poll::Pending => return Poll::Pending,
                }
            }

            let Some(step) = this.steps.pop_front() else {
                return Poll::Ready(None);
            };
            match step {
                Step::Sleep(duration) => {
                    this.delay = Some(Box::pin(tokio::time::sleep(duration)));
                    // Loop back to poll the freshly-armed delay.
                }
                Step::Chunk(text) => {
                    this.arm_default_delay();
                    return Poll::Ready(Some(Msg::Chunk(text)));
                }
                Step::ToolStart(name) => {
                    this.arm_default_delay();
                    return Poll::Ready(Some(Msg::ToolStarted(name)));
                }
                Step::ToolFinish {
                    name,
                    summary,
                    output,
                    status,
                } => {
                    this.arm_default_delay();
                    return Poll::Ready(Some(Msg::ToolFinished {
                        name,
                        summary,
                        output,
                        status,
                    }));
                }
                Step::Complete => return Poll::Ready(Some(Msg::Complete)),
            }
        }
    }
}

impl AgentStream {
    /// Arms the default inter-chunk delay so streaming paces realistically.
    fn arm_default_delay(&mut self) {
        self.delay = Some(Box::pin(tokio::time::sleep(Duration::from_millis(120))));
    }
}

/// A ticker driving the streaming spinner, one [`Msg::Tick`] every ~120ms.
struct SpinnerTicker {
    interval: tokio::time::Interval,
}

impl SpinnerTicker {
    fn new() -> Self {
        Self {
            interval: tokio::time::interval(Duration::from_millis(120)),
        }
    }
}

impl Stream for SpinnerTicker {
    type Item = Msg;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Msg>> {
        let this = self.get_mut();
        match this.interval.poll_tick(cx) {
            Poll::Ready(_) => Poll::Ready(Some(Msg::Tick)),
            Poll::Pending => Poll::Pending,
        }
    }
}
