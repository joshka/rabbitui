//! The cell buffer: a grid of styled grapheme cells and a double-buffer diff.
//!
//! rabbitui renders through a cell buffer, ratatui-compatible in shape (a
//! grapheme plus a style per cell), which is composited, double-buffer diffed,
//! and emitted inside synchronized-output framing — see
//! `docs/adr/0003-rendering.md`. This module owns the buffer and the diff; the
//! double-buffer diff *is* the damage tracking, computed for free after the
//! fact, so there are no damage regions to keep correct.
//!
//! Wide graphemes (CJK, many emoji) occupy two cells: the grapheme is written
//! into its lead cell and an empty *continuation* marker into the following
//! cell. The diff reports only the lead cell; the continuation cell rides along
//! with it. This mirrors ratatui's tested wide-grapheme rules.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::buffer::Buffer;
//! use rabbitui_core::geometry::{Position, Size};
//! use rabbitui_core::style::{Color, Style};
//!
//! let mut buffer = Buffer::new(Size::new(10, 1));
//! buffer.set_string(Position::ORIGIN, "hi", Style::new().fg(Color::GREEN));
//! assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "h");
//! ```

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::geometry::{Position, Size};
use crate::style::Style;

/// A single terminal cell: one grapheme cluster plus a style.
///
/// The `symbol` holds one user-perceived character (a grapheme cluster, which
/// may be several `char`s, e.g. a base plus combining marks or an emoji with a
/// variation selector). An empty `symbol` marks a *continuation* cell — the
/// second half of a wide grapheme whose lead is in the cell to its left.
///
/// The default cell is a single space in the default style, so a fresh buffer
/// reads as blank.
///
/// Storage is a `String` for now. A future optimization is to inline short
/// symbols the way ratatui does with `CompactString`, avoiding a heap
/// allocation per cell; this is deferred until the buffer is a measured cost.
///
/// # Examples
///
/// ```
/// use rabbitui_core::buffer::Cell;
/// use rabbitui_core::style::{Color, Style};
///
/// let cell = Cell::new("a", Style::new().fg(Color::RED));
/// assert_eq!(cell.symbol, "a");
/// assert!(!cell.is_continuation());
///
/// assert_eq!(Cell::default().symbol, " ");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The grapheme cluster shown in this cell, or empty for a continuation
    /// cell (the right half of a wide grapheme).
    pub symbol: String,
    /// The visual style applied to this cell.
    pub style: Style,
}

impl Cell {
    /// Creates a cell from a symbol and a style.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Cell;
    /// use rabbitui_core::style::Style;
    ///
    /// let cell = Cell::new("x", Style::new());
    /// assert_eq!(cell.symbol, "x");
    /// ```
    #[must_use]
    pub fn new(symbol: impl Into<String>, style: Style) -> Self {
        Self { symbol: symbol.into(), style }
    }

    /// Returns true if this cell is a continuation cell — the empty right half
    /// of a wide grapheme.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Cell;
    /// use rabbitui_core::style::Style;
    ///
    /// assert!(Cell::new("", Style::new()).is_continuation());
    /// assert!(!Cell::new("a", Style::new()).is_continuation());
    /// ```
    #[must_use]
    pub fn is_continuation(&self) -> bool {
        self.symbol.is_empty()
    }

    /// The number of terminal cells this cell's grapheme advances (0, 1, or 2).
    ///
    /// A continuation cell reports 0; a narrow grapheme 1; a wide grapheme 2.
    /// The encoder uses this to advance across a wide grapheme's skipped
    /// continuation cell when coalescing a diff into styled runs. Width comes
    /// from the same oracle as [`Buffer::set_string`], so a cell never disagrees
    /// with the layout that placed it.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Cell;
    /// use rabbitui_core::style::Style;
    ///
    /// assert_eq!(Cell::new("a", Style::new()).width(), 1);
    /// assert_eq!(Cell::new("世", Style::new()).width(), 2);
    /// assert_eq!(Cell::new("", Style::new()).width(), 0);
    /// ```
    #[must_use]
    pub fn width(&self) -> usize {
        grapheme_width(&self.symbol)
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self { symbol: String::from(" "), style: Style::new() }
    }
}

/// A changed cell reported by [`Buffer::diff`]: a position and the new cell.
///
/// A diff is a list of these, in row-major order, naming only the cells the
/// encoder must repaint. Continuation cells are never reported on their own;
/// repainting a wide grapheme's lead cell repaints the whole grapheme.
///
/// # Examples
///
/// ```
/// use rabbitui_core::buffer::Buffer;
/// use rabbitui_core::geometry::{Position, Size};
/// use rabbitui_core::style::Style;
///
/// let previous = Buffer::new(Size::new(3, 1));
/// let mut current = previous.clone();
/// current.set_string(Position::new(1, 0), "x", Style::new());
///
/// let changes = current.diff(&previous);
/// assert_eq!(changes.len(), 1);
/// assert_eq!(changes[0].position, Position::new(1, 0));
/// assert_eq!(changes[0].cell.symbol, "x");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellChange {
    /// Where the changed cell is, in zero-based cells.
    pub position: Position,
    /// The cell's new content, to be painted at `position`.
    pub cell: Cell,
}

/// A grid of [`Cell`]s addressed by [`Position`].
///
/// The buffer is row-major (`index = y * width + x`) and holds `size.area()`
/// cells. Widgets paint into it with [`set_string`](Self::set_string); the
/// runtime double-buffer diffs it against the previous frame with
/// [`diff`](Self::diff) and emits only what changed (ADR 0003).
///
/// # Examples
///
/// ```
/// use rabbitui_core::buffer::Buffer;
/// use rabbitui_core::geometry::{Position, Size};
/// use rabbitui_core::style::Style;
///
/// let mut buffer = Buffer::new(Size::new(5, 2));
/// buffer.set_string(Position::new(1, 0), "ok", Style::new());
/// assert_eq!(buffer.get(Position::new(1, 0)).unwrap().symbol, "o");
/// assert_eq!(buffer.get(Position::new(2, 0)).unwrap().symbol, "k");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Buffer {
    size: Size,
    cells: Vec<Cell>,
}

impl Buffer {
    /// Creates a buffer of `size`, every cell the default (blank) cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::Size;
    ///
    /// let buffer = Buffer::new(Size::new(4, 3));
    /// assert_eq!(buffer.size(), Size::new(4, 3));
    /// ```
    #[must_use]
    pub fn new(size: Size) -> Self {
        Self { size, cells: vec![Cell::default(); size.area() as usize] }
    }

    /// The buffer's size in cells.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::Size;
    ///
    /// assert_eq!(Buffer::new(Size::new(8, 2)).size(), Size::new(8, 2));
    /// ```
    #[must_use]
    pub const fn size(&self) -> Size {
        self.size
    }

    /// Returns a shared reference to the cell at `position`, or `None` if the
    /// position is outside the buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::{Position, Size};
    ///
    /// let buffer = Buffer::new(Size::new(2, 1));
    /// assert!(buffer.get(Position::new(0, 0)).is_some());
    /// assert!(buffer.get(Position::new(2, 0)).is_none());
    /// ```
    #[must_use]
    pub fn get(&self, position: Position) -> Option<&Cell> {
        self.index_of(position).map(|index| &self.cells[index])
    }

    /// Returns a mutable reference to the cell at `position`, or `None` if the
    /// position is outside the buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::style::{Color, Style};
    ///
    /// let mut buffer = Buffer::new(Size::new(2, 1));
    /// let cell = buffer.get_mut(Position::ORIGIN).unwrap();
    /// cell.symbol = String::from("z");
    /// cell.style = Style::new().fg(Color::BLUE);
    /// assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "z");
    /// ```
    #[must_use]
    pub fn get_mut(&mut self, position: Position) -> Option<&mut Cell> {
        self.index_of(position).map(|index| &mut self.cells[index])
    }

    /// Writes `text` starting at `position` in `style`, one grapheme cluster
    /// per cell, left to right.
    ///
    /// Writing stops at the right edge of the row; graphemes that would fall
    /// past it are dropped (there is no wrapping — that is layout's job). A wide
    /// grapheme writes its cluster into the lead cell and an empty continuation
    /// marker into the next cell; a wide grapheme that would straddle the right
    /// edge (its continuation cell would be off-row) is not written at all,
    /// leaving the last cell blank rather than showing half a glyph.
    ///
    /// A `position` outside the buffer writes nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::style::Style;
    ///
    /// let mut buffer = Buffer::new(Size::new(3, 1));
    /// buffer.set_string(Position::ORIGIN, "abcd", Style::new());
    /// // Clipped at the right edge: only three cells exist.
    /// assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, "a");
    /// assert_eq!(buffer.get(Position::new(2, 0)).unwrap().symbol, "c");
    /// ```
    pub fn set_string(&mut self, position: Position, text: &str, style: Style) {
        self.set_stringn(position, text, style, usize::MAX);
    }

    /// Writes `text` like [`set_string`], but stops after `max_width` cells.
    ///
    /// The effective limit is the shorter of `max_width` and the distance to
    /// the right edge; a wide grapheme that would straddle either limit is not
    /// written. This is the primitive widget contexts use to clip painting to
    /// a widget's area (`docs/adr/0008-widget-contract.md`).
    ///
    /// [`set_string`]: Self::set_string
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::style::Style;
    ///
    /// let mut buffer = Buffer::new(Size::new(5, 1));
    /// buffer.set_stringn(Position::ORIGIN, "abcd", Style::new(), 2);
    /// assert_eq!(buffer.get(Position::new(1, 0)).unwrap().symbol, "b");
    /// assert_eq!(buffer.get(Position::new(2, 0)).unwrap().symbol, " ");
    /// ```
    pub fn set_stringn(
        &mut self,
        position: Position,
        text: &str,
        style: Style,
        max_width: usize,
    ) {
        if position.y >= self.size.height {
            return;
        }
        let limit = usize::from(self.size.width.saturating_sub(position.x)).min(max_width);
        let Ok(limit) = u16::try_from(limit) else { return };
        let end = position.x.saturating_add(limit);
        let mut x = position.x;
        for grapheme in text.graphemes(true) {
            let width = grapheme_width(grapheme);
            // A zero-width grapheme (a lone combining mark, say) has no cell of
            // its own; skip it rather than clobber the previous cell.
            if width == 0 {
                continue;
            }
            // Stop once the lead cell is at or past the limit.
            if x >= end {
                break;
            }
            // A wide grapheme that would straddle the limit is not written.
            if width == 2 && x + 1 >= end {
                break;
            }
            let Some(index) = self.index_of(Position::new(x, position.y)) else {
                break;
            };
            self.cells[index] = Cell::new(grapheme, style);
            if width == 2 {
                // The continuation cell carries no symbol but inherits the
                // style so a later narrow overwrite of just the lead cell
                // leaves a consistent background behind.
                self.cells[index + 1] = Cell::new("", style);
                x += 2;
            } else {
                x += 1;
            }
        }
    }

    /// Resizes the buffer to `size`, clearing every cell to the default.
    ///
    /// This is the full-repaint path: the caller diffs the resized buffer
    /// against the previous frame and, because the sizes differ, gets every
    /// cell back (ADR 0003's resize case).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::style::Style;
    ///
    /// let mut buffer = Buffer::new(Size::new(2, 1));
    /// buffer.set_string(Position::ORIGIN, "x", Style::new());
    /// buffer.resize(Size::new(4, 2));
    /// assert_eq!(buffer.size(), Size::new(4, 2));
    /// assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, " ");
    /// ```
    pub fn resize(&mut self, size: Size) {
        self.size = size;
        self.cells.clear();
        self.cells.resize(size.area() as usize, Cell::default());
    }

    /// Clears every cell to the default, in place, keeping the size.
    ///
    /// The runtime resets the back buffer before each frame because widgets
    /// declare everything every frame (ADR 0001); the diff against the front
    /// buffer then recovers the damage (ADR 0003).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::style::Style;
    ///
    /// let mut buffer = Buffer::new(Size::new(3, 1));
    /// buffer.set_string(Position::ORIGIN, "abc", Style::new());
    /// buffer.reset();
    /// assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, " ");
    /// ```
    pub fn reset(&mut self) {
        self.cells.fill(Cell::default());
    }

    /// Diffs this (current) buffer against `previous`, returning the changed
    /// cells the encoder must repaint.
    ///
    /// The diff *is* the damage tracking (ADR 0003): unchanged cells are
    /// skipped, and continuation cells (the empty right half of a wide
    /// grapheme) are skipped because the lead cell already carries the change.
    /// When the two buffers differ in size, every non-continuation cell of the
    /// current buffer is returned — a full repaint, since positions no longer
    /// line up.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::style::Style;
    ///
    /// let previous = Buffer::new(Size::new(4, 1));
    /// let mut current = previous.clone();
    /// current.set_string(Position::new(2, 0), "z", Style::new());
    ///
    /// let changes = current.diff(&previous);
    /// assert_eq!(changes.len(), 1);
    /// assert_eq!(changes[0].position, Position::new(2, 0));
    /// ```
    #[must_use]
    pub fn diff(&self, previous: &Buffer) -> Vec<CellChange> {
        let mut changes = Vec::new();
        let full_repaint = self.size != previous.size;
        for (index, cell) in self.cells.iter().enumerate() {
            if cell.is_continuation() {
                continue;
            }
            let index = index as u32;
            let position = Position::new(
                (index % u32::from(self.size.width)) as u16,
                (index / u32::from(self.size.width)) as u16,
            );
            let changed = full_repaint || previous.get(position) != Some(cell);
            if changed {
                changes.push(CellChange { position, cell: cell.clone() });
            }
        }
        changes
    }

    /// Maps a position to a flat index, or `None` if it is outside the buffer.
    fn index_of(&self, position: Position) -> Option<usize> {
        if position.x >= self.size.width || position.y >= self.size.height {
            return None;
        }
        Some(position.y as usize * self.size.width as usize + position.x as usize)
    }
}

/// The display width of a grapheme cluster in terminal cells (0, 1, or 2).
///
/// This is a thin wrapper over `unicode-width` for now. ADR 0012 calls for one
/// shared width oracle whose answer can depend on terminal mode (mode-2027,
/// emoji VS16); when that module lands, this call moves behind it so there is
/// never a second width table.
fn grapheme_width(grapheme: &str) -> usize {
    // Clamp to 2: a cluster's terminal advance is at most two cells even when
    // `unicode-width` sums several component widths.
    UnicodeWidthStr::width(grapheme).min(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Color;

    #[test]
    fn new_buffer_is_all_default_cells() {
        let buffer = Buffer::new(Size::new(3, 2));
        assert_eq!(buffer.size(), Size::new(3, 2));
        for y in 0..2 {
            for x in 0..3 {
                let cell = buffer.get(Position::new(x, y)).unwrap();
                assert_eq!(cell, &Cell::default());
            }
        }
    }

    #[test]
    fn set_and_get_writes_graphemes_left_to_right() {
        let mut buffer = Buffer::new(Size::new(5, 1));
        let style = Style::new().fg(Color::GREEN);
        buffer.set_string(Position::ORIGIN, "abc", style);
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, "a");
        assert_eq!(buffer.get(Position::new(1, 0)).unwrap().symbol, "b");
        assert_eq!(buffer.get(Position::new(2, 0)).unwrap().symbol, "c");
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().style, style);
        // Untouched cells stay blank.
        assert_eq!(buffer.get(Position::new(3, 0)).unwrap().symbol, " ");
    }

    #[test]
    fn get_and_get_mut_reject_out_of_bounds() {
        let mut buffer = Buffer::new(Size::new(2, 2));
        assert!(buffer.get(Position::new(2, 0)).is_none());
        assert!(buffer.get(Position::new(0, 2)).is_none());
        assert!(buffer.get_mut(Position::new(2, 2)).is_none());
    }

    #[test]
    fn set_string_clips_at_right_edge() {
        let mut buffer = Buffer::new(Size::new(3, 1));
        buffer.set_string(Position::ORIGIN, "abcde", Style::new());
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, "a");
        assert_eq!(buffer.get(Position::new(2, 0)).unwrap().symbol, "c");
        // Only three cells exist; the rest was dropped, not wrapped.
        assert!(buffer.get(Position::new(3, 0)).is_none());
    }

    #[test]
    fn set_string_off_row_writes_nothing() {
        let mut buffer = Buffer::new(Size::new(3, 1));
        buffer.set_string(Position::new(0, 5), "x", Style::new());
        assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, " ");
    }

    #[test]
    fn wide_grapheme_writes_lead_and_continuation() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        buffer.set_string(Position::ORIGIN, "世x", Style::new());
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, "世");
        // The continuation cell is an empty marker.
        assert!(buffer.get(Position::new(1, 0)).unwrap().is_continuation());
        // The next grapheme lands after the wide cell.
        assert_eq!(buffer.get(Position::new(2, 0)).unwrap().symbol, "x");
    }

    #[test]
    fn wide_grapheme_straddling_right_edge_is_not_written() {
        // Width 3, so a wide grapheme at x=2 would need cell 3 which is off-row.
        let mut buffer = Buffer::new(Size::new(3, 1));
        buffer.set_string(Position::new(2, 0), "世", Style::new());
        // The last cell stays blank rather than showing half a glyph.
        assert_eq!(buffer.get(Position::new(2, 0)).unwrap().symbol, " ");
    }

    #[test]
    fn wide_grapheme_fitting_exactly_is_written() {
        let mut buffer = Buffer::new(Size::new(2, 1));
        buffer.set_string(Position::ORIGIN, "世", Style::new());
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, "世");
        assert!(buffer.get(Position::new(1, 0)).unwrap().is_continuation());
    }

    #[test]
    fn diff_reports_only_changed_cells() {
        let previous = Buffer::new(Size::new(4, 1));
        let mut current = previous.clone();
        current.set_string(Position::new(1, 0), "x", Style::new());
        let changes = current.diff(&previous);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].position, Position::new(1, 0));
        assert_eq!(changes[0].cell.symbol, "x");
    }

    #[test]
    fn diff_of_identical_buffers_is_empty() {
        let previous = Buffer::new(Size::new(4, 2));
        let current = previous.clone();
        assert!(current.diff(&previous).is_empty());
    }

    #[test]
    fn diff_skips_wide_continuation_cells() {
        let previous = Buffer::new(Size::new(4, 1));
        let mut current = previous.clone();
        current.set_string(Position::ORIGIN, "世", Style::new());
        let changes = current.diff(&previous);
        // Only the lead cell is reported; the continuation cell rides along.
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].position, Position::ORIGIN);
        assert_eq!(changes[0].cell.symbol, "世");
    }

    #[test]
    fn diff_after_size_change_is_full_repaint() {
        let previous = Buffer::new(Size::new(2, 1));
        let mut current = Buffer::new(Size::new(3, 1));
        current.set_string(Position::ORIGIN, "a", Style::new());
        let changes = current.diff(&previous);
        // Every non-continuation cell of the new buffer is returned.
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].position, Position::new(0, 0));
        assert_eq!(changes[2].position, Position::new(2, 0));
    }

    #[test]
    fn diff_detects_style_only_change() {
        let previous = Buffer::new(Size::new(2, 1));
        let mut current = previous.clone();
        // Same symbol (space) but a new style still counts as a change.
        current.get_mut(Position::ORIGIN).unwrap().style = Style::new().bg(Color::RED);
        let changes = current.diff(&previous);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].position, Position::ORIGIN);
    }
}
