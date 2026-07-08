//! Markdown → styled scrollback lines (app-land).
//!
//! Rendering markdown is not the framework's job (the slice-8 design note's
//! deliberate boundary), so it lives here, over pulldown-cmark. It renders a
//! *finished* message: the live streaming tail shows raw source, because partial
//! markdown — an unclosed `**` or an open code fence — renders wrong, so the turn
//! commits the rendered form once, at the end. Coverage: headings, bold / italic /
//! strikethrough, inline and fenced code, ordered and bullet lists (nested), and
//! links (rendered as `text (url)`, since terminals have no clickable links).

use pulldown_cmark::{CodeBlockKind, Event as MdEvent, Options, Parser, Tag, TagEnd};
use rabbitui_core::commit::CommitLine;
use rabbitui_core::style::{Attrs, Color, Style};
use rabbitui_core::text::Span;

/// Renders markdown `source` into committed transcript lines (per-span SGR).
#[must_use]
pub fn markdown_to_commit_lines(source: &str) -> Vec<CommitLine> {
    let mut render = MarkdownRender::default();
    // Strikethrough (`~~x~~`) is a GFM extension pulldown parses only when asked.
    for event in Parser::new_ext(source, Options::ENABLE_STRIKETHROUGH) {
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
    /// The open list nesting: each entry is the next ordered number (`Some`) or
    /// `None` for a bullet list. The stack length is the nesting depth.
    list_stack: Vec<Option<u64>>,
    /// A pending list-item marker (bullet or number) to emit at the next text.
    pending_marker: Option<String>,
    /// The destination of the link currently open, appended as ` (url)` on close.
    link_url: Option<String>,
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
            Tag::Strikethrough => self.attrs |= Attrs::STRIKETHROUGH,
            Tag::Link { dest_url, .. } => {
                self.attrs |= Attrs::UNDERLINE;
                self.link_url = Some(dest_url.to_string());
            }
            Tag::CodeBlock(CodeBlockKind::Fenced(_) | CodeBlockKind::Indented) => {
                self.break_line();
                self.in_code_block = true;
                self.fg = Some(Color::Ansi(8));
            }
            // `Some(n)` is an ordered list starting at `n`; `None` is a bullet list.
            Tag::List(first) => self.list_stack.push(first),
            Tag::Item => self.pending_marker = Some(self.next_marker()),
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
            TagEnd::Strikethrough => self.attrs = self.attrs.remove(Attrs::STRIKETHROUGH),
            TagEnd::Link => {
                self.attrs = self.attrs.remove(Attrs::UNDERLINE);
                // Terminals have no clickable links; keep the URL as trailing text.
                if let Some(url) = self.link_url.take()
                    && !url.is_empty()
                {
                    self.current.push(Span::styled(
                        format!(" ({url})"),
                        Style::new().fg(Color::Ansi(8)),
                    ));
                }
            }
            TagEnd::CodeBlock => {
                self.break_line();
                self.in_code_block = false;
                self.fg = None;
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
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
        self.emit_bullet();
        self.current.push(Span::styled(
            code.to_string(),
            Style::new().fg(Color::Ansi(8)),
        ));
    }

    /// The marker for the next item of the innermost open list: an indented
    /// `N. ` for an ordered list (advancing its counter) or `• ` for a bullet
    /// list. Two spaces of indent per nesting level.
    fn next_marker(&mut self) -> String {
        let indent = "  ".repeat(self.list_stack.len().saturating_sub(1));
        match self.list_stack.last_mut() {
            Some(Some(number)) => {
                let marker = format!("{indent}{number}. ");
                *number += 1;
                marker
            }
            _ => format!("{indent}• "),
        }
    }

    /// Emits a pending list-item marker, if one is queued.
    fn emit_bullet(&mut self) {
        if let Some(marker) = self.pending_marker.take() {
            // A marker always begins its own line: flush any text already on this
            // one first, e.g. a parent item's text that precedes a nested list.
            self.break_line();
            self.current
                .push(Span::styled(marker, Style::new().fg(Color::CYAN)));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The plain text of every rendered line, joined with newlines.
    fn rendered(source: &str) -> String {
        markdown_to_commit_lines(source)
            .iter()
            .map(CommitLine::text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The style of the first span whose text contains `needle`.
    fn style_of(source: &str, needle: &str) -> Style {
        markdown_to_commit_lines(source)
            .iter()
            .flat_map(CommitLine::spans)
            .find(|span| span.text.contains(needle))
            .unwrap_or_else(|| panic!("no span containing {needle:?}"))
            .style
    }

    #[test]
    fn ordered_lists_number_their_items() {
        assert_eq!(
            rendered("1. one\n2. two\n3. three"),
            "1. one\n2. two\n3. three"
        );
    }

    #[test]
    fn ordered_lists_honor_the_starting_number() {
        assert_eq!(rendered("5. five\n6. six"), "5. five\n6. six");
    }

    #[test]
    fn bullet_lists_use_a_bullet() {
        assert_eq!(rendered("- a\n- b"), "• a\n• b");
    }

    #[test]
    fn nested_lists_indent_by_depth() {
        // A bullet under item 1, then a second top-level item.
        let out = rendered("1. one\n    - nested\n2. two");
        assert_eq!(out, "1. one\n  • nested\n2. two");
    }

    #[test]
    fn strikethrough_sets_the_attribute() {
        assert!(
            style_of("~~gone~~", "gone")
                .attrs
                .contains(Attrs::STRIKETHROUGH)
        );
    }

    #[test]
    fn a_link_renders_its_url_as_trailing_text() {
        assert_eq!(
            rendered("[docs](https://example.com)"),
            "docs (https://example.com)"
        );
    }

    #[test]
    fn bold_and_italic_still_style() {
        assert!(style_of("**b**", "b").attrs.contains(Attrs::BOLD));
        assert!(style_of("*i*", "i").attrs.contains(Attrs::ITALIC));
    }
}
