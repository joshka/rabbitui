//! Committed lines: the append-once scrollback channel's payload (ADR 0013).
//!
//! In inline mode, finalized content is *committed* into the terminal's own
//! scrollback exactly once and never repainted (`docs/adr/0013-screen-modes.md`).
//! A [`CommitLine`] is one such logical line: a sequence of styled [`Span`]s. The
//! engine emits each span's SGR in order, then the line's `\r\n`, and lets the
//! terminal soft-wrap and thereafter own its reflow, selection, and copy — the
//! tui2 retirement lesson made a renderer rule.
//!
//! # From one span to many (slice 8)
//!
//! Slice 5 shipped a single style per line and recorded multi-span lines as a
//! known ceiling; the transcript flagship lifts it (`docs/design/slice8-agent-chrome.md`).
//! A `CommitLine` now holds a `Vec<Span>`, so a committed markdown line can carry
//! a bold heading, dim inline code, and plain prose in one line. The
//! single-span constructors ([`new`](CommitLine::new), [`From<&str>`], and
//! [`From<String>`]) and the [`text`](CommitLine::text)/[`style`](CommitLine::style)
//! accessors are preserved for the common unstyled or one-style case.
//!
//! [`Style`]: crate::style::Style
//! [`Span`]: crate::text::Span
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::commit::CommitLine;
//! use rabbitui_core::style::{Color, Style};
//! use rabbitui_core::text::Span;
//!
//! // An unstyled line from a string slice (one span).
//! let plain = CommitLine::from("build finished");
//! assert_eq!(plain.text(), "build finished");
//! assert_eq!(plain.style(), Style::new());
//!
//! // A styled single-span line.
//! let ok = CommitLine::new("all tests passed", Style::new().fg(Color::GREEN));
//! assert_eq!(ok.style().fg, Some(Color::GREEN));
//!
//! // A multi-span line: a bold label followed by plain detail.
//! let mixed = CommitLine::from_spans([
//!     Span::styled("done: ", Style::new().bold()),
//!     Span::raw("396 passed"),
//! ]);
//! assert_eq!(mixed.text(), "done: 396 passed");
//! assert_eq!(mixed.spans().len(), 2);
//! ```

use crate::style::Style;
use crate::text::Span;

/// One line committed into native scrollback: an ordered run of styled
/// [`Span`]s.
///
/// Committed lines are immutable — committed once, never repainted, never
/// addressed again (ADR 0013). The engine emits each span's text unwrapped, in
/// order, so the terminal owns wrapping and reflow.
///
/// # Examples
///
/// ```
/// use rabbitui_core::commit::CommitLine;
/// use rabbitui_core::style::Style;
///
/// let line: CommitLine = "log line".into();
/// assert_eq!(line.text(), "log line");
/// assert_eq!(line.style(), Style::new());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitLine {
    spans: Vec<Span>,
}

impl CommitLine {
    /// Creates a single-span commit line from text and a style.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::commit::CommitLine;
    /// use rabbitui_core::style::{Color, Style};
    ///
    /// let line = CommitLine::new("done", Style::new().fg(Color::GREEN).bold());
    /// assert_eq!(line.text(), "done");
    /// assert!(line.style().attrs.contains(rabbitui_core::style::Attributes::BOLD));
    /// ```
    #[must_use]
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            spans: vec![Span::styled(text, style)],
        }
    }

    /// Creates a commit line from an ordered sequence of [`Span`]s.
    ///
    /// The spans paint left to right with no separator, each in its own style;
    /// this is the multi-style path the transcript work needs (a markdown line
    /// rendered to bold, code, and plain runs).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::commit::CommitLine;
    /// use rabbitui_core::style::{Color, Style};
    /// use rabbitui_core::text::Span;
    ///
    /// let line = CommitLine::from_spans([
    ///     Span::styled("▸ ", Style::new().fg(Color::GREEN)),
    ///     Span::raw("ran cargo test"),
    /// ]);
    /// assert_eq!(line.text(), "▸ ran cargo test");
    /// ```
    #[must_use]
    pub fn from_spans(spans: impl IntoIterator<Item = Span>) -> Self {
        Self {
            spans: spans.into_iter().collect(),
        }
    }

    /// The line's spans, in paint order.
    #[must_use]
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// The line's full text, every span concatenated (no trailing newline — the
    /// engine adds `\r\n`).
    #[must_use]
    pub fn text(&self) -> String {
        self.spans.iter().map(|span| span.text.as_str()).collect()
    }

    /// The first span's style, or the default when the line is empty.
    ///
    /// A convenience for the single-span case (the slice-5 shape); a multi-span
    /// line has a style per span, read through [`spans`](Self::spans).
    #[must_use]
    pub fn style(&self) -> Style {
        self.spans.first().map_or(Style::new(), |span| span.style)
    }
}

impl From<&str> for CommitLine {
    fn from(text: &str) -> Self {
        Self {
            spans: vec![Span::raw(text)],
        }
    }
}

impl From<String> for CommitLine {
    fn from(text: String) -> Self {
        Self {
            spans: vec![Span::raw(text)],
        }
    }
}

impl From<Span> for CommitLine {
    fn from(span: Span) -> Self {
        Self { spans: vec![span] }
    }
}

impl From<Vec<Span>> for CommitLine {
    fn from(spans: Vec<Span>) -> Self {
        Self { spans }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Attributes, Color};

    #[test]
    fn from_str_is_unstyled() {
        let line = CommitLine::from("hello");
        assert_eq!(line.text(), "hello");
        assert_eq!(line.style(), Style::new());
    }

    #[test]
    fn from_string_is_unstyled() {
        let line = CommitLine::from(String::from("hello"));
        assert_eq!(line.text(), "hello");
        assert_eq!(line.style(), Style::new());
    }

    #[test]
    fn new_carries_text_and_style() {
        let style = Style::new().fg(Color::GREEN).bold();
        let line = CommitLine::new("done", style);
        assert_eq!(line.text(), "done");
        assert_eq!(line.style(), style);
        assert!(line.style().attrs.contains(Attributes::BOLD));
    }

    #[test]
    fn into_conversion_works() {
        let line: CommitLine = "x".into();
        assert_eq!(line.text(), "x");
    }

    #[test]
    fn from_spans_concatenates_text_and_keeps_first_style() {
        let bold = Style::new().bold();
        let red = Style::new().fg(Color::RED);
        let line = CommitLine::from_spans([Span::styled("a", bold), Span::styled("bc", red)]);
        assert_eq!(line.text(), "abc");
        assert_eq!(line.spans().len(), 2);
        // `style()` reports the first span's style for the single-style shim.
        assert_eq!(line.style(), bold);
        assert_eq!(line.spans()[1].style, red);
    }

    #[test]
    fn from_span_and_vec_conversions() {
        let single: CommitLine = Span::raw("solo").into();
        assert_eq!(single.text(), "solo");
        let many: CommitLine = vec![Span::raw("a"), Span::raw("b")].into();
        assert_eq!(many.text(), "ab");
    }
}
