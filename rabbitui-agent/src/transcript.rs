//! The transcript model: the shared source of truth both render modes consume.
//!
//! One `Vec<TranscriptCell>` drives both the inline scrollback (committed once,
//! append-only) and the alt-screen scrollable column. [`commit_lines_for`] renders
//! a cell to the styled lines the inline engine appends to native scrollback.
//!
//! Commit rendering uses concrete [`Style`]s, not theme [`Role`](rabbitui_core::theme::Role)s:
//! the inline engine paints committed lines directly and has no theme handle, so
//! this is the one place app code carries literal colors (the alt-screen path in
//! `app.rs` styles the same cells via roles through widgets). The color intents
//! mirror the roles — cyan/accent, green/success, red/danger, ansi-8/muted.

use rabbitui_core::commit::CommitLine;
use rabbitui_core::style::{Color, Style};
use rabbitui_core::text::Span;

use crate::markdown::markdown_to_commit_lines;

/// The status a tool call reports as it moves through the confirm/run cycle
/// (drives its summary's color and header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    /// Awaiting the user's allow/deny decision.
    Pending,
    /// Allowed and executing.
    Running,
    /// The tool succeeded.
    Ok,
    /// The tool failed (or was denied/cancelled).
    Failed,
}

impl ToolStatus {
    /// Whether this is a settled, final status (`Ok`/`Failed`) rather than one
    /// still in motion (`Pending`/`Running`).
    ///
    /// The inline engine commits each cell to native scrollback exactly once and
    /// cannot rewrite it, so a Tool cell must not be committed until it is
    /// terminal — otherwise its `Pending` glyph freezes there and never shows the
    /// result. See `flush_commits` in `app.rs`.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, ToolStatus::Ok | ToolStatus::Failed)
    }
}

/// One cell of the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptCell {
    /// A prompt the user sent.
    User(String),
    /// A completed assistant message, held as its markdown source.
    Assistant(String),
    /// Summarized thinking that preceded an assistant message.
    Thinking(String),
    /// A completed tool call. Populated by real execution in slice 4; the model
    /// carries it now so that slice does not churn the transcript type.
    Tool {
        /// The tool's name.
        name: String,
        /// The one-line summary shown in scrollback and as the collapsible header.
        summary: String,
        /// The full output, revealed in alt-screen mode.
        output: String,
        /// Whether the call succeeded.
        status: ToolStatus,
    },
    /// A backend failure, surfaced in order with the prose before it.
    Error(String),
}

/// The in-flight assistant turn: accumulating prose, thinking, and any streamed
/// tool-use blocks.
#[derive(Debug, Default, Clone)]
pub struct Streaming {
    /// The markdown prose accumulated from `TextDelta`s so far.
    pub source: String,
    /// The summarized thinking accumulated from `ThinkingDelta`s so far.
    pub thinking: String,
    /// The thinking block's signature, from a `signature_delta` — replayed
    /// verbatim on the continuation after a tool-use turn.
    pub thinking_signature: String,
    /// The name of a tool whose call has started but not finished, if any.
    pub running_tool: Option<String>,
    /// The tool-use blocks accumulated this turn, in wire order. Each opens on a
    /// `ToolUseStart`, accretes JSON on `ToolUseInputDelta`, and finalizes on
    /// `ToolUseStop`.
    pub tool_uses: Vec<PendingToolUse>,
}

/// A tool-use block being (or already) accumulated from the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingToolUse {
    /// The tool-call id, echoed back on the matching `tool_result`.
    pub id: String,
    /// The tool's name.
    pub name: String,
    /// The streamed JSON input, accumulated as raw text then parsed on stop.
    pub input_json: String,
}

/// The committed scrollback lines for one transcript cell.
#[must_use]
pub fn commit_lines_for(cell: &TranscriptCell) -> Vec<CommitLine> {
    match cell {
        TranscriptCell::User(prompt) => vec![CommitLine::from_spans([
            Span::styled("❯ ", Style::new().fg(Color::CYAN).bold()),
            Span::styled(prompt.clone(), Style::new().bold()),
        ])],
        TranscriptCell::Assistant(source) => markdown_to_commit_lines(source),
        TranscriptCell::Thinking(text) => text
            .lines()
            .map(|line| {
                CommitLine::from_spans([Span::styled(
                    format!("  {line}"),
                    Style::new().fg(Color::Ansi(8)).italic(),
                )])
            })
            .collect(),
        TranscriptCell::Tool {
            summary, status, ..
        } => {
            let (glyph, style) = match status {
                ToolStatus::Pending => ("… ", Style::new().fg(Color::Ansi(8))),
                ToolStatus::Running => ("▸ ", Style::new().fg(Color::CYAN)),
                ToolStatus::Ok => ("✓ ", Style::new().fg(Color::GREEN)),
                ToolStatus::Failed => ("✗ ", Style::new().fg(Color::RED)),
            };
            vec![CommitLine::from_spans([Span::styled(
                format!("{glyph}{summary}"),
                style,
            )])]
        }
        TranscriptCell::Error(message) => vec![CommitLine::from_spans([Span::styled(
            format!("⚠ {message}"),
            Style::new().fg(Color::RED).bold(),
        )])],
    }
}
