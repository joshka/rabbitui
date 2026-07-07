//! Committed lines: the append-once scrollback channel's payload (ADR 0013).
//!
//! In inline mode, finalized content is *committed* into the terminal's own
//! scrollback exactly once and never repainted (`docs/adr/0013-screen-modes.md`).
//! A [`CommitLine`] is one such logical line: a `String` plus one [`Style`]. The
//! engine emits it unwrapped, terminated `\r\n`, and lets the terminal soft-wrap
//! and thereafter own its reflow, selection, and copy — the tui2 retirement
//! lesson made a renderer rule.
//!
//! # Why one span, for now
//!
//! v1 is a single style per line, not a `Vec` of styled spans. Multi-span commit
//! lines are deliberately deferred to the transcript work (slice 8 flagship) —
//! recorded as a known ceiling, not an oversight (slice-5 design note). The
//! [`From<&str>`](CommitLine::from) and [`From<String>`] conversions cover the
//! common unstyled case ergonomically.
//!
//! [`Style`]: crate::style::Style
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::commit::CommitLine;
//! use rabbitui_core::style::{Color, Style};
//!
//! // An unstyled line from a string slice.
//! let plain = CommitLine::from("build finished");
//! assert_eq!(plain.text(), "build finished");
//! assert_eq!(plain.style(), Style::new());
//!
//! // A styled line.
//! let ok = CommitLine::new("all tests passed", Style::new().fg(Color::GREEN));
//! assert_eq!(ok.style().fg, Some(Color::GREEN));
//! ```

use crate::style::Style;

/// One line committed into native scrollback: text plus a single style.
///
/// Committed lines are immutable — committed once, never repainted, never
/// addressed again (ADR 0013). The engine emits the text unwrapped so the
/// terminal owns wrapping and reflow.
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
    text: String,
    style: Style,
}

impl CommitLine {
    /// Creates a commit line from text and a style.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::commit::CommitLine;
    /// use rabbitui_core::style::{Color, Style};
    ///
    /// let line = CommitLine::new("done", Style::new().fg(Color::GREEN).bold());
    /// assert_eq!(line.text(), "done");
    /// assert!(line.style().attrs.contains(rabbitui_core::style::Attrs::BOLD));
    /// ```
    #[must_use]
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self { text: text.into(), style }
    }

    /// The line's text (no trailing newline — the engine adds `\r\n`).
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The line's style, applied to the whole line when committed.
    #[must_use]
    pub fn style(&self) -> Style {
        self.style
    }
}

impl From<&str> for CommitLine {
    fn from(text: &str) -> Self {
        Self { text: text.to_string(), style: Style::new() }
    }
}

impl From<String> for CommitLine {
    fn from(text: String) -> Self {
        Self { text, style: Style::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Attrs, Color};

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
        assert!(line.style().attrs.contains(Attrs::BOLD));
    }

    #[test]
    fn into_conversion_works() {
        let line: CommitLine = "x".into();
        assert_eq!(line.text(), "x");
    }
}
