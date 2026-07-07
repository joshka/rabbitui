//! Markdown → styled scrollback lines (app-land).
//!
//! Rendering markdown is not the framework's job (the slice-8 design note's
//! deliberate boundary), so it lives here. This is the pulldown-cmark port from
//! the flagship example; slice 3 replaces it with a hand-rolled, streaming-aware
//! parser that can commit completed blocks mid-stream. Coverage: headings, bold /
//! italic, inline and fenced code, and bullet lists.

use pulldown_cmark::{CodeBlockKind, Event as MdEvent, Parser, Tag, TagEnd};
use rabbitui_core::commit::CommitLine;
use rabbitui_core::style::{Attrs, Color, Style};
use rabbitui_core::text::Span;

/// Renders markdown `source` into committed transcript lines (per-span SGR).
#[must_use]
pub fn markdown_to_commit_lines(source: &str) -> Vec<CommitLine> {
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
    /// The active inline attributes (bold/italic).
    attrs: Attrs,
    /// The active foreground override for headings/code, if any.
    fg: Option<Color>,
    /// Whether we are inside a fenced/indented code block.
    in_code_block: bool,
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

    /// Appends text in the current style; inside a code block each `'\n'` is a new
    /// line so multi-line fences keep their shape.
    fn text(&mut self, text: &str) {
        self.emit_bullet();
        if self.in_code_block {
            let mut first = true;
            for line in text.split('\n') {
                if !first {
                    self.break_line();
                }
                first = false;
                if !line.is_empty() {
                    self.current.push(Span::styled(line.to_string(), self.style()));
                }
            }
        } else {
            self.current.push(Span::styled(text.to_string(), self.style()));
        }
    }

    /// Appends an inline `code` span in a dim code style.
    fn inline_code(&mut self, code: &str) {
        self.emit_bullet();
        self.current
            .push(Span::styled(code.to_string(), Style::new().fg(Color::Ansi(8))));
    }

    /// Emits a pending list bullet, if one is queued.
    fn emit_bullet(&mut self) {
        if self.bullet_pending {
            self.current
                .push(Span::styled("• ", Style::new().fg(Color::CYAN)));
            self.bullet_pending = false;
        }
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
