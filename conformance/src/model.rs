//! A thin [`vt100`] wrapper for escape-level assertions.
//!
//! This is the headless layer's terminal model: feed a case's emitted bytes to a
//! real [`vt100::Parser`] and read back the emulated grid, scrollback, and cursor.
//! It is intentionally the same shape as `rabbitui-testing`'s `VtScreen` (ADR 0009
//! layer 3) — the conformance harness grows from that harness in spirit — but kept
//! self-contained here so `conformance/` builds on its own.

/// A `cols`-by-`rows` emulated terminal screen with generous scrollback.
pub struct Model {
    parser: vt100::Parser,
    rows: u16,
    cols: u16,
}

impl Model {
    /// Scrollback depth in rows; ample for the short transcripts the corpus feeds
    /// so committed lines never fall off the top.
    const SCROLLBACK: usize = 1000;

    /// Creates a `cols`-by-`rows` model.
    #[must_use]
    pub fn new(rows: u16, cols: u16) -> Self {
        let parser = vt100::Parser::new(rows, cols, Self::SCROLLBACK);
        Self { parser, rows, cols }
    }

    /// Feeds `bytes` to the emulator, advancing screen state.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// The cursor position as `(row, col)`, zero-based.
    #[must_use]
    pub fn cursor(&self) -> (u16, u16) {
        self.parser.screen().cursor_position()
    }

    /// Row `y` of the *visible* screen as text, trailing spaces trimmed.
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

    /// Every *visible* row, top to bottom, trailing-trimmed.
    #[must_use]
    pub fn visible_rows(&self) -> Vec<String> {
        self.parser
            .screen()
            .rows(0, self.cols)
            .map(|row| row.trim_end().to_string())
            .collect()
    }

    /// The `(symbol, ansi-fg-index)` of a single visible cell, if the cell exists.
    ///
    /// The ANSI index is `Some` only when the emulator resolved the foreground to a
    /// palette index (which covers the 16 ANSI colors as 0..=15); a default or
    /// truecolor foreground yields `None` for the index.
    #[must_use]
    pub fn cell(&self, row: u16, col: u16) -> Option<(String, Option<u8>)> {
        let screen = self.parser.screen();
        let cell = screen.cell(row, col)?;
        let ansi = match cell.fgcolor() {
            vt100::Color::Idx(index) => Some(index),
            vt100::Color::Default | vt100::Color::Rgb(..) => None,
        };
        Some((cell.contents().to_string(), ansi))
    }

    /// Every logical row from the top of scrollback through the bottom of the
    /// visible screen, trailing-trimmed — committed history *and* the live tail.
    ///
    /// Leading and trailing all-blank rows are trimmed. This is the inline-mode
    /// assertion surface: a line committed into scrollback scrolls above the tail
    /// and leaves the visible screen, but appears here.
    #[must_use]
    pub fn all_lines(&mut self) -> Vec<String> {
        let previous = self.parser.screen().scrollback();

        self.parser.screen_mut().set_scrollback(usize::MAX);
        let scrollback_len = self.parser.screen().scrollback();

        let mut lines = Vec::new();
        for offset in (1..=scrollback_len).rev() {
            self.parser.screen_mut().set_scrollback(offset);
            if let Some(row) = self.parser.screen().rows(0, self.cols).next() {
                lines.push(row.trim_end().to_string());
            }
        }
        self.parser.screen_mut().set_scrollback(0);
        for row in self.parser.screen().rows(0, self.cols) {
            lines.push(row.trim_end().to_string());
        }

        self.parser.screen_mut().set_scrollback(previous);
        trim_blank_ends(lines)
    }

    /// The screen size as `(rows, cols)`.
    #[must_use]
    pub fn size(&self) -> (u16, u16) {
        (self.rows, self.cols)
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
