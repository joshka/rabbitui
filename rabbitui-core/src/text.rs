//! Styled text spans: a run of text carrying one [`Style`].
//!
//! A [`Span`] is the atom of multi-style text — a `String` plus the [`Style`]
//! that paints all of it. The transcript work (slice 8 flagship) needs a
//! committed scrollback line to carry several differently-styled runs (bold
//! headings, dim code, an accent-colored status), which slice 5 deferred as a
//! recorded ceiling. `Span` is that lift: [`CommitLine`](crate::commit::CommitLine)
//! becomes a `Vec<Span>` and the inline engine emits one SGR per span within a
//! committed line.
//!
//! Unifying this with the widget-side styled `Text` (a wrapped, laid-out run) is
//! deliberately left to the catalog phase; commit lines are the only consumer
//! this slice (`docs/design/slice8-agent-chrome.md`).
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::style::{Color, Style};
//! use rabbitui_core::text::Span;
//!
//! let plain = Span::raw("hello");
//! assert_eq!(plain.text, "hello");
//! assert_eq!(plain.style, Style::new());
//!
//! let ok = Span::styled("passed", Style::new().fg(Color::GREEN));
//! assert_eq!(ok.style.fg, Some(Color::GREEN));
//! ```

use crate::style::Style;

/// A run of text painted in one [`Style`].
///
/// Spans are the building block of a multi-style line: a committed transcript
/// line is a `Vec<Span>`, each rendered with its own SGR and concatenated with
/// no separator. A `Span` owns its text so it can outlive the frame that built
/// it (a committed line is retained until it is flushed into scrollback).
///
/// # Examples
///
/// ```
/// use rabbitui_core::style::{Attrs, Style};
/// use rabbitui_core::text::Span;
///
/// let heading = Span::styled("Title", Style::new().bold());
/// assert!(heading.style.attrs.contains(Attrs::BOLD));
/// assert_eq!(heading.text, "Title");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// The run's text.
    pub text: String,
    /// The style applied to the whole run.
    pub style: Style,
}

impl Span {
    /// A span of `text` in the unstyled default [`Style`].
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::Style;
    /// use rabbitui_core::text::Span;
    ///
    /// assert_eq!(Span::raw("x").style, Style::new());
    /// ```
    #[must_use]
    pub fn raw(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: Style::new(),
        }
    }

    /// A span of `text` in `style`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::{Color, Style};
    /// use rabbitui_core::text::Span;
    ///
    /// let span = Span::styled("err", Style::new().fg(Color::RED));
    /// assert_eq!(span.style.fg, Some(Color::RED));
    /// ```
    #[must_use]
    pub fn styled(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

impl From<&str> for Span {
    fn from(text: &str) -> Self {
        Self::raw(text)
    }
}

impl From<String> for Span {
    fn from(text: String) -> Self {
        Self::raw(text)
    }
}

impl From<(String, Style)> for Span {
    fn from((text, style): (String, Style)) -> Self {
        Self::styled(text, style)
    }
}

impl From<(&str, Style)> for Span {
    fn from((text, style): (&str, Style)) -> Self {
        Self::styled(text, style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Attrs, Color};

    #[test]
    fn raw_is_unstyled() {
        assert_eq!(Span::raw("hi").style, Style::new());
        assert_eq!(Span::raw("hi").text, "hi");
    }

    #[test]
    fn styled_carries_the_style() {
        let span = Span::styled("hi", Style::new().fg(Color::GREEN).bold());
        assert_eq!(span.style.fg, Some(Color::GREEN));
        assert!(span.style.attrs.contains(Attrs::BOLD));
    }

    #[test]
    fn from_conversions() {
        assert_eq!(Span::from("a"), Span::raw("a"));
        assert_eq!(Span::from(String::from("b")), Span::raw("b"));
        let style = Style::new().fg(Color::RED);
        assert_eq!(Span::from(("c", style)), Span::styled("c", style));
        assert_eq!(
            Span::from((String::from("d"), style)),
            Span::styled("d", style)
        );
    }
}
