//! Integration test: the agent-chrome flagship flow — the slice-8 acceptance test.
//!
//! Mirrors `examples/agent.rs` (the todo-flow precedent: an example's `update`/
//! `view` are private to its binary, so the test reconstructs the same logic and
//! drives it through the real router / engine). It proves the thesis end to end:
//!
//! - **Live tail (inline).** A prompt is sent and stream chunks are injected; the
//!   in-progress message renders in the live tail, soft-wrapped, from accumulated
//!   source — before anything commits.
//! - **Commit on completion (inline, vt100).** When the message completes, its
//!   markdown is rendered to multi-span [`CommitLine`]s, fed through the real
//!   [`InlineEngine`], and the *emitted bytes* are asserted on a [`VtScreen`]:
//!   the heading, prose, and bullets land in native scrollback with per-span
//!   styling. Append-once holds — only completion commits.
//! - **Transcript view (alt).** The same transcript renders as a column; a tool
//!   cell is a [`Collapsible`] **collapsed by default** (body hidden), and Enter
//!   expands it (body shown) — the alt affordance the immutable inline scrollback
//!   cannot offer.

use rabbitui::engine::InlineEngine;
use rabbitui_core::buffer::Buffer;
use rabbitui_core::commit::CommitLine;
use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::{Position, Rect, Size};
use rabbitui_core::id::{Key as WidgetKey, WidgetId, key};
use rabbitui_core::input::Key as InputKey;
use rabbitui_core::layout::Constraint;
use rabbitui_core::style::{Attrs, Color, Style};
use rabbitui_core::text::Span;
use rabbitui_testing::TestApp;
use rabbitui_testing::vt::{VtColor, VtScreen};
use rabbitui_widgets::{Collapsible, Text, TextInput};

use pulldown_cmark::{CodeBlockKind, Event as MdEvent, Parser, Tag, TagEnd};

// ---------------------------------------------------------------------------
// The transcript model and app state (mirroring examples/agent.rs)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolStatus {
    Ok,
    Failed,
}

#[derive(Debug, Clone)]
enum TranscriptCell {
    User(String),
    Assistant {
        source: String,
    },
    Tool {
        name: String,
        summary: String,
        output: String,
        status: ToolStatus,
    },
}

#[derive(Debug, Default, Clone)]
struct Streaming {
    source: String,
}

#[derive(Default)]
struct Agent {
    cells: Vec<TranscriptCell>,
    streaming: Option<Streaming>,
    inline: bool,
    input_generation: u64,
}

impl Agent {
    fn inline() -> Self {
        Self {
            inline: true,
            ..Default::default()
        }
    }

    fn alt() -> Self {
        Self {
            inline: false,
            ..Default::default()
        }
    }

    fn is_streaming(&self) -> bool {
        self.streaming.is_some()
    }
}

// ---------------------------------------------------------------------------
// Commit rendering (mirroring the example)
// ---------------------------------------------------------------------------

fn commit_lines_for(cell: &TranscriptCell) -> Vec<CommitLine> {
    match cell {
        TranscriptCell::User(prompt) => vec![CommitLine::from_spans([
            Span::styled("❯ ", Style::new().fg(Color::CYAN).bold()),
            Span::styled(prompt.clone(), Style::new().bold()),
        ])],
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

fn markdown_to_commit_lines(source: &str) -> Vec<CommitLine> {
    let mut render = MarkdownRender::default();
    for event in Parser::new(source) {
        render.event(event);
    }
    render.finish()
}

#[derive(Default)]
struct MarkdownRender {
    lines: Vec<Vec<Span>>,
    current: Vec<Span>,
    attrs: Attrs,
    fg: Option<Color>,
    in_code_block: bool,
    bullet_pending: bool,
}

impl MarkdownRender {
    fn event(&mut self, event: MdEvent<'_>) {
        match event {
            MdEvent::Start(tag) => self.start(tag),
            MdEvent::End(tag) => self.end(tag),
            MdEvent::Text(text) => self.text(&text),
            MdEvent::Code(code) => {
                self.push_bullet();
                self.current.push(Span::styled(
                    code.to_string(),
                    Style::new().fg(Color::Ansi(8)).dim(),
                ));
            }
            MdEvent::SoftBreak | MdEvent::HardBreak => self.break_line(),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { .. } => {
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
                self.attrs = Attrs::NONE;
                self.fg = None;
                self.break_line();
            }
            TagEnd::Emphasis => self.attrs = remove(self.attrs, Attrs::ITALIC),
            TagEnd::Strong => self.attrs = remove(self.attrs, Attrs::BOLD),
            TagEnd::CodeBlock => {
                self.break_line();
                self.in_code_block = false;
                self.fg = None;
            }
            TagEnd::Paragraph | TagEnd::Item => self.break_line(),
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        self.push_bullet();
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

    fn push_bullet(&mut self) {
        if self.bullet_pending {
            self.current
                .push(Span::styled("• ", Style::new().fg(Color::CYAN)));
            self.bullet_pending = false;
        }
    }

    fn style(&self) -> Style {
        let mut style = Style {
            fg: self.fg,
            bg: None,
            attrs: self.attrs,
        };
        if self.in_code_block {
            style = style.dim();
        }
        style
    }

    fn break_line(&mut self) {
        if !self.current.is_empty() {
            self.lines.push(std::mem::take(&mut self.current));
        }
    }

    fn finish(mut self) -> Vec<CommitLine> {
        self.break_line();
        self.lines.into_iter().map(CommitLine::from_spans).collect()
    }
}

fn remove(attrs: Attrs, remove: Attrs) -> Attrs {
    let mut result = Attrs::NONE;
    for flag in [
        Attrs::BOLD,
        Attrs::DIM,
        Attrs::ITALIC,
        Attrs::UNDERLINE,
        Attrs::REVERSED,
    ] {
        if attrs.contains(flag) && !remove.contains(flag) {
            result |= flag;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Views (mirroring the example)
// ---------------------------------------------------------------------------

const TAIL_HEIGHT: u16 = 8;

fn composer_key(app: &Agent) -> WidgetKey {
    key("composer").index(usize::try_from(app.input_generation).unwrap_or(usize::MAX))
}

fn inline_view(app: &Agent, frame: &mut Frame<'_>) {
    let [preview, status_row, composer_row, hint_row] = frame.rows([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);
    let preview_text = app
        .streaming
        .as_ref()
        .map_or_else(String::new, |s| s.source.clone());
    frame.widget(
        key("preview"),
        preview,
        &Text::new(&preview_text).wrap(true),
    );
    let status = status_line(app);
    frame.widget(
        key("status"),
        status_row,
        &Text::new(&status).style(Style::new().fg(Color::CYAN)),
    );
    frame.widget(
        composer_key(app),
        composer_row,
        &TextInput::new().placeholder("Tab, type…"),
    );
    frame.widget(
        key("hint"),
        hint_row,
        &Text::new("q: quit").style(Style::new().dim()),
    );
}

fn alt_view(app: &Agent, frame: &mut Frame<'_>) {
    let [transcript, _status, _composer, _hint] = frame.rows([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);
    render_transcript(app, frame, transcript);
}

fn render_transcript(app: &Agent, frame: &mut Frame<'_>, area: Rect) {
    const CELL_ROWS: u16 = 4;
    let mut y = area.origin.y;
    let bottom = area.origin.y + area.size.height;
    for (index, cell) in app.cells.iter().enumerate() {
        if y >= bottom {
            break;
        }
        let height = (bottom - y).min(CELL_ROWS);
        let slot = Rect::new(
            Position::new(area.origin.x, y),
            Size::new(area.size.width, height),
        );
        let cell_key = key("cell").index(index);
        match cell {
            TranscriptCell::User(prompt) => {
                let text = format!("❯ {prompt}");
                frame.widget(
                    cell_key,
                    slot,
                    &Text::new(&text).style(Style::new().fg(Color::CYAN).bold()),
                );
            }
            TranscriptCell::Assistant { source } => {
                frame.widget(cell_key, slot, &Text::new(source).wrap(true));
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
                frame.widget(
                    cell_key,
                    slot,
                    &Collapsible::new(&header, output).default_collapsed(true),
                );
            }
        }
        y += height;
    }
}

fn status_line(app: &Agent) -> String {
    let mode = if app.inline { "inline" } else { "alt-screen" };
    if app.is_streaming() {
        format!("[{mode}]  streaming")
    } else {
        format!("[{mode}]  idle")
    }
}

// ---------------------------------------------------------------------------
// The message fold (mirroring the example's handle_message, deterministically)
// ---------------------------------------------------------------------------

/// Appends a prose chunk to the in-flight turn's accumulated source.
fn inject_chunk(app: &mut Agent, chunk: &str) {
    if let Some(streaming) = app.streaming.as_mut() {
        streaming.source.push_str(chunk);
    }
}

/// Commits the accumulated prose as an assistant cell (in inline mode, also
/// records the commit lines), then ends the turn.
fn complete(app: &mut Agent, committed: &mut Vec<CommitLine>) {
    let source = app
        .streaming
        .take()
        .map(|s| s.source)
        .filter(|s| !s.trim().is_empty());
    if let Some(source) = source {
        let cell = TranscriptCell::Assistant { source };
        if app.inline {
            committed.extend(commit_lines_for(&cell));
        }
        app.cells.push(cell);
    }
}

/// Pushes a completed tool cell (committing its summary line in inline mode).
fn finish_tool(app: &mut Agent, cell: TranscriptCell, committed: &mut Vec<CommitLine>) {
    if app.inline {
        committed.extend(commit_lines_for(&cell));
    }
    app.cells.push(cell);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// A deterministic assistant markdown body reused across tests.
fn assistant_source() -> String {
    "## Working on: refactor\n\nLet me run the **test suite**.\n\nResults:\n\n- the `core::text` span type\n- the inline engine".to_string()
}

#[test]
fn live_tail_shows_accumulated_source_before_commit() {
    // Inline mode: send a prompt (opens a streaming turn), inject chunks, and the
    // live-tail preview shows the accumulated, soft-wrapped source — with nothing
    // committed yet (append-once: only completion commits).
    let mut app = TestApp::new(Size::new(24, TAIL_HEIGHT), Agent::inline());
    app.render(inline_view);

    // Begin a streaming turn (as a prompt submit would).
    app.state_mut().streaming = Some(Streaming::default());
    app.render(inline_view);

    // Inject two prose chunks; the preview accumulates and wraps them.
    app.send(|a| inject_chunk(a, "the quick brown fox "), inline_view);
    app.send(|a| inject_chunk(a, "jumps over the lazy dog"), inline_view);

    // The preview region (top rows) shows the wrapped prose. Width 24 wraps the
    // sentence across rows; the first row starts the sentence.
    let text = app.buffer_text();
    assert!(
        text.contains("the quick brown fox"),
        "live tail shows accumulated source: {text:?}"
    );
    // The status line reports streaming.
    assert!(
        text.contains("streaming"),
        "status shows streaming while the tail fills"
    );
}

#[test]
fn completion_commits_markdown_as_per_span_scrollback_via_vt100() {
    // Complete the turn, render its commit lines through the real InlineEngine,
    // and assert the emitted bytes on a vt100 screen: the heading, prose, and
    // bullets land in native scrollback, and the heading carries its per-span
    // (cyan/bold) color — buffer equality could never see this.
    let mut app = Agent::inline();
    app.streaming = Some(Streaming {
        source: assistant_source(),
    });
    let mut committed = Vec::new();
    complete(&mut app, &mut committed);

    // Only completion commits: one turn produced a non-empty set of commit lines,
    // and the turn is now closed.
    assert!(!committed.is_empty(), "completion produced commit lines");
    assert!(!app.is_streaming(), "the turn closed on completion");
    assert_eq!(app.cells.len(), 1, "the assistant cell was appended once");

    // Feed the commit lines through the inline engine into a vt100 screen with an
    // empty tail (the runtime's commit-flush shape), then read scrollback.
    let mut engine = InlineEngine::new();
    let mut screen = VtScreen::new(40, 12);
    screen.feed(&engine.enter());
    let empty = Buffer::new(Size::new(40, 0));
    screen.feed(&engine.render(&empty, &committed));

    let lines = screen.all_lines();
    let joined = lines.join("\n");
    assert!(
        joined.contains("Working on: refactor"),
        "heading committed: {lines:?}"
    );
    assert!(joined.contains("test suite"), "prose committed: {lines:?}");
    assert!(
        joined.contains("• the"),
        "bullets committed with a marker: {lines:?}"
    );

    // The heading's first cell is cyan (its per-span SGR reached the terminal).
    // Find the heading row and inspect its first painted cell on the live screen.
    let mut heading_engine = InlineEngine::new();
    let mut heading_screen = VtScreen::new(40, 2);
    heading_screen.feed(&heading_engine.enter());
    let heading_only = markdown_to_commit_lines("## Heading");
    heading_screen.feed(&heading_engine.render(&buffer_row("tail", 40), &heading_only));
    let cells = heading_screen.row_cells(0);
    assert_eq!(
        cells.iter().map(|(s, _)| s.as_str()).collect::<String>(),
        "Heading"
    );
    assert_eq!(cells[0].1, VtColor::Ansi(6), "the heading is cyan (ANSI 6)");
}

/// A one-row tail buffer holding `text`.
fn buffer_row(text: &str, width: u16) -> Buffer {
    let mut buffer = Buffer::new(Size::new(width, 1));
    buffer.set_string(Position::ORIGIN, text, Style::new());
    buffer
}

#[test]
fn tool_cell_is_collapsed_by_default_then_expands_on_enter() {
    // Alt-screen: a transcript with a user prompt, an assistant reply, and a tool
    // cell. The tool cell renders collapsed (body hidden). Focusing it and
    // pressing Enter expands it (body shown) — the alt affordance over immutable
    // inline scrollback.
    let mut app = TestApp::new(Size::new(40, 16), Agent::alt());
    let mut committed = Vec::new(); // unused in alt mode; commits are inline-only.
    {
        let state = app.state_mut();
        state
            .cells
            .push(TranscriptCell::User("refactor the engine".to_string()));
        state.cells.push(TranscriptCell::Assistant {
            source: "On it.".to_string(),
        });
    }
    // The tool finishes: alt mode records the cell but commits nothing.
    finish_tool(
        app.state_mut(),
        TranscriptCell::Tool {
            name: "cargo test".to_string(),
            summary: "▸ ran cargo test — 396 passed".to_string(),
            output: "running 396 tests\ntest result: ok".to_string(),
            status: ToolStatus::Ok,
        },
        &mut committed,
    );
    assert!(
        committed.is_empty(),
        "alt mode commits nothing (no scrollback)"
    );

    app.render(alt_view);
    // Collapsed by default: the header shows, the body (the tool output) is hidden.
    let before = app.buffer_text();
    assert!(
        before.contains("ran cargo test"),
        "tool header shows collapsed"
    );
    assert!(
        !before.contains("running 396 tests"),
        "tool body is hidden while collapsed"
    );

    // Focus the tool cell (it is the third cell) and press Enter to expand it.
    let tool_id = WidgetId::ROOT.child(key("cell").index(2));
    app.set_focus(Some(tool_id));
    app.render(alt_view);
    let result = app.send_key(InputKey::Enter);
    assert!(
        result.consumed,
        "Enter is consumed by the focused collapsible"
    );
    app.render(alt_view);

    // Expanded: the body (full output) is now visible.
    let after = app.buffer_text();
    assert!(
        after.contains("running 396 tests"),
        "tool body shows after Enter expands it: {after:?}"
    );
}

#[test]
fn assistant_cell_is_expanded_by_default_in_alt() {
    // An assistant cell shows its body immediately (no collapse) — the transcript
    // default the design note calls for (tool cells collapsed, assistant expanded).
    let mut app = TestApp::new(Size::new(40, 12), Agent::alt());
    app.state_mut().cells.push(TranscriptCell::Assistant {
        source: "the visible reply body".to_string(),
    });
    app.render(alt_view);
    assert!(
        app.buffer_text().contains("the visible reply body"),
        "assistant body shows by default"
    );
}

#[test]
fn tool_summary_commit_is_styled_by_status() {
    // A tool cell's committed summary is styled by status: green when it passed,
    // red when it failed (the status role, per the design note).
    let ok = commit_lines_for(&TranscriptCell::Tool {
        name: "cargo test".to_string(),
        summary: "▸ ran cargo test — 396 passed".to_string(),
        output: String::new(),
        status: ToolStatus::Ok,
    });
    assert_eq!(
        ok[0].spans()[0].style.fg,
        Some(Color::GREEN),
        "passed summary is green"
    );

    let failed = commit_lines_for(&TranscriptCell::Tool {
        name: "cargo build".to_string(),
        summary: "▸ ran cargo build — failed".to_string(),
        output: "error[E0308]".to_string(),
        status: ToolStatus::Failed,
    });
    assert_eq!(
        failed[0].spans()[0].style.fg,
        Some(Color::RED),
        "failed summary is red"
    );
}

#[test]
fn markdown_renders_heading_bullets_and_inline_code_with_styles() {
    // The markdown → spans render (app-land) covers headings, bold/italic, inline
    // code, fenced code, and bullets, each with the right style.
    let lines = markdown_to_commit_lines(
        "## Title\n\nSome **bold** and `code` here.\n\n- first\n- second\n\n```\nfn main() {}\n```",
    );
    let text: Vec<String> = lines.iter().map(CommitLine::text).collect();
    let joined = text.join("\n");
    assert!(joined.contains("Title"), "heading text present");
    assert!(joined.contains("bold"), "bold run present");
    assert!(joined.contains("code"), "inline code present");
    assert!(joined.contains("• first"), "bullet marker present");
    assert!(joined.contains("fn main() {}"), "fenced code present");

    // The heading line is bold + cyan (one bold span).
    let heading = &lines[0];
    assert!(
        heading
            .spans()
            .iter()
            .any(|s| s.style.attrs.contains(Attrs::BOLD) && s.style.fg == Some(Color::CYAN)),
        "heading span is bold cyan: {:?}",
        heading.spans(),
    );
    // A bold run exists somewhere with the BOLD attribute set on just that span.
    assert!(
        lines.iter().any(|line| line
            .spans()
            .iter()
            .any(|s| s.text == "bold" && s.style.attrs.contains(Attrs::BOLD))),
        "the word 'bold' is its own bold span",
    );
    // The inline code span is dim.
    assert!(
        lines.iter().any(|line| line
            .spans()
            .iter()
            .any(|s| s.text == "code" && s.style.attrs.contains(Attrs::DIM))),
        "inline code is a dim span",
    );
}
