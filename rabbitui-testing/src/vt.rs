//! The vt100 escape-level harness (ADR 0009 layer 3).
//!
//! Buffer-equality snapshots validate the *intended* buffer; they cannot see the
//! ANSI *emission* — framing, clears, cursor discipline, commit/tail interleaving
//! — which is exactly where inline mode, resize, and synchronized-output bugs
//! live (the tui2/textual-rs finding, `docs/adr/0009-testing.md`). This module
//! routes an engine's *emitted bytes* through a real [`vt100::Parser`] and lets a
//! test assert on the resulting emulated screen grid.
//!
//! Because rabbitui's render engines are **pure byte producers**
//! (`rabbitui::engine`), tests compose cleanly: engine bytes → [`VtScreen`] →
//! assert the screen a terminal would show. This is the authoritative layer for
//! the ANSI-defined paths.
//!
//! # Scrollback
//!
//! Inline mode commits finalized lines into native scrollback above the live
//! tail. vt100 retains scrollback but reports only the *visible* screen by
//! default; [`VtScreen::all_lines`] scrolls back to reveal committed history so a
//! test can assert a commit landed *above* the tail. The parser is created with a
//! generous scrollback so nothing committed in a test is lost.
//!
//! # Examples
//!
//! ```
//! use rabbitui_testing::vt::VtScreen;
//!
//! // Feed raw bytes as if they came off an engine and assert the grid.
//! let mut screen = VtScreen::new(10, 2);
//! screen.feed(b"hello");
//! screen.assert_row(0, "hello");
//! assert_eq!(screen.cursor(), (0, 5));
//! ```

/// A thin, honest wrapper over [`vt100::Parser`] for escape-level assertions.
///
/// Construct with [`new`](Self::new) at a fixed size, [`feed`](Self::feed) the
/// engine's emitted bytes, then assert on the emulated screen with
/// [`row_text`](Self::row_text), [`assert_row`](Self::assert_row),
/// [`contents`](Self::contents), [`cursor`](Self::cursor), and — for inline
/// scrollback — [`all_lines`](Self::all_lines).
///
/// Row assertions are trailing-space-trimmed to match `assert_buffer_lines`
/// conventions, so tests need not pad to the screen width.
pub struct VtScreen {
    parser: vt100::Parser,
    rows: u16,
    cols: u16,
}

impl VtScreen {
    /// The scrollback depth every harness screen is given, in rows. Ample for the
    /// short transcripts escape-level tests feed; committed lines never fall off.
    const SCROLLBACK: usize = 1000;

    /// Creates a `cols`-by-`rows` emulated screen with generous scrollback.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_testing::vt::VtScreen;
    ///
    /// let screen = VtScreen::new(20, 4);
    /// assert_eq!(screen.size(), (4, 20));
    /// ```
    #[must_use]
    pub fn new(cols: u16, rows: u16) -> Self {
        let parser = vt100::Parser::new(rows, cols, Self::SCROLLBACK);
        Self { parser, rows, cols }
    }

    /// Feeds `bytes` to the emulator, advancing the screen state.
    ///
    /// Call once per engine frame (or accumulate several — vt100 is a streaming
    /// parser, so feeding two frames back to back models two writes).
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// The screen size as `(rows, cols)`.
    #[must_use]
    pub fn size(&self) -> (u16, u16) {
        (self.rows, self.cols)
    }

    /// Row `y` of the *visible* screen as text, trailing spaces trimmed.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_testing::vt::VtScreen;
    ///
    /// let mut screen = VtScreen::new(8, 1);
    /// screen.feed(b"hi");
    /// assert_eq!(screen.row_text(0), "hi");
    /// ```
    #[must_use]
    pub fn row_text(&self, y: u16) -> String {
        self.parser
            .screen()
            .rows(0, self.cols)
            .nth(usize::from(y))
            .unwrap_or_default()
            .trim_end()
            .to_string()
    }

    /// Asserts row `y` of the visible screen equals `expected` (trailing-trimmed).
    ///
    /// # Panics
    ///
    /// Panics with the actual row content if it differs from `expected`.
    pub fn assert_row(&self, y: u16, expected: &str) {
        let actual = self.row_text(y);
        assert_eq!(
            actual, expected,
            "row {y}: expected {expected:?}, got {actual:?}\nfull screen:\n{}",
            self.contents()
        );
    }

    /// Every *visible* row as trailing-trimmed strings, top to bottom.
    #[must_use]
    pub fn rows_text(&self) -> Vec<String> {
        self.parser
            .screen()
            .rows(0, self.cols)
            .map(|row| row.trim_end().to_string())
            .collect()
    }

    /// The visible screen's text contents (plain, newline-joined; vt100's
    /// [`Screen::contents`](vt100::Screen::contents)).
    #[must_use]
    pub fn contents(&self) -> String {
        self.parser.screen().contents()
    }

    /// The cursor position as `(row, col)`, zero-based.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_testing::vt::VtScreen;
    ///
    /// let mut screen = VtScreen::new(8, 2);
    /// screen.feed(b"ab");
    /// assert_eq!(screen.cursor(), (0, 2));
    /// ```
    #[must_use]
    pub fn cursor(&self) -> (u16, u16) {
        self.parser.screen().cursor_position()
    }

    /// The plain text logically between two cells (row/col, zero-based),
    /// vt100's [`Screen::contents_between`](vt100::Screen::contents_between).
    ///
    /// Useful for asserting a clipboard-style selection spanning wrapped lines.
    #[must_use]
    pub fn contents_between(
        &self,
        start_row: u16,
        start_col: u16,
        end_row: u16,
        end_col: u16,
    ) -> String {
        self.parser.screen().contents_between(start_row, start_col, end_row, end_col)
    }

    /// Every logical row from the top of scrollback through the bottom of the
    /// visible screen, trailing-trimmed — committed history *and* the live tail.
    ///
    /// This is the inline-mode assertion surface: a line committed into scrollback
    /// scrolls above the live tail and disappears from the *visible* screen, but
    /// appears here. The harness walks the scrollback from the top down, collecting
    /// each row that scrolled off, then appends the visible rows — so it is exact
    /// regardless of how deep the history is relative to the screen height.
    /// Purely-blank leading and trailing rows are trimmed away.
    #[must_use]
    pub fn all_lines(&mut self) -> Vec<String> {
        let previous = self.parser.screen().scrollback();

        // The scrollback length: max out the offset (vt100 clamps to the real
        // length) and read back the clamped value.
        self.parser.screen_mut().set_scrollback(usize::MAX);
        let scrollback_len = self.parser.screen().scrollback();

        let mut lines = Vec::new();
        // For offset k (scrollback_len..=1), the window's top row is the next
        // row that scrolled off history, so collect just that row.
        for offset in (1..=scrollback_len).rev() {
            self.parser.screen_mut().set_scrollback(offset);
            if let Some(row) = self.parser.screen().rows(0, self.cols).next() {
                lines.push(row.trim_end().to_string());
            }
        }
        // Offset 0 is the live screen; append all its rows.
        self.parser.screen_mut().set_scrollback(0);
        for row in self.parser.screen().rows(0, self.cols) {
            lines.push(row.trim_end().to_string());
        }

        self.parser.screen_mut().set_scrollback(previous);
        trim_blank_ends(lines)
    }
}

/// Drops leading and trailing all-blank rows from `lines`.
fn trim_blank_ends(mut lines: Vec<String>) -> Vec<String> {
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feeds_text_and_reads_rows() {
        let mut screen = VtScreen::new(10, 2);
        screen.feed(b"hello");
        screen.assert_row(0, "hello");
        screen.assert_row(1, "");
    }

    #[test]
    fn tracks_cursor() {
        let mut screen = VtScreen::new(10, 2);
        screen.feed(b"abc");
        assert_eq!(screen.cursor(), (0, 3));
    }

    #[test]
    fn absolute_cursor_move_positions_text() {
        let mut screen = VtScreen::new(10, 3);
        // CSI 2;3 H then "x": row 1 (0-based), col 2 (0-based).
        screen.feed(b"\x1b[2;3Hx");
        assert_eq!(screen.row_text(1), "  x");
        assert_eq!(screen.cursor(), (1, 3));
    }

    #[test]
    fn all_lines_includes_scrollback() {
        // A 1-row screen: writing two lines pushes the first into scrollback.
        let mut screen = VtScreen::new(10, 1);
        screen.feed(b"first\r\nsecond");
        // The visible screen shows only the last line…
        screen.assert_row(0, "second");
        // …but scrollback keeps the first line above it.
        assert_eq!(screen.all_lines(), vec!["first".to_string(), "second".to_string()]);
    }
}
