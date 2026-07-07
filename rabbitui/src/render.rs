//! Turning a buffer diff into terminal bytes.
//!
//! The paint model is cells, but the *wire* is runs: this module coalesces a
//! diff's changed cells into styled runs and emits one cursor-move + SGR + text
//! write per run, the whole frame wrapped in synchronized-output (mode 2026)
//! framing so the terminal presents it atomically (ADR 0003 — run-merging lives
//! in the encoder, not the paint primitive).
//!
//! An empty diff writes nothing at all (no-op frame suppression): an idle app
//! emits zero bytes.

use qwertty::{CommandBuffer, ProtocolPosition, commands};
use rabbitui_core::buffer::CellChange;
use rabbitui_core::style::Style;

use crate::encode;
use crate::terminal::{Result, Terminal};

/// Renders `diff` to `terminal` inside synchronized-output framing.
///
/// Consecutive changes on the same row that share a style are grouped into one
/// write: a single cursor move to the run's start, one SGR sequence, then the
/// run's text. Runs break on a row change, a style change, or a gap (a cell
/// whose position is not the one the previous cell's width would advance to).
/// An empty `diff` returns without writing or flushing.
///
/// # Errors
///
/// Returns an error if writing the frame to the terminal fails.
pub(crate) async fn render(terminal: &mut Terminal, diff: &[CellChange]) -> Result<()> {
    if diff.is_empty() {
        return Ok(());
    }

    let mut frame = CommandBuffer::new();
    frame.bytes(encode::BEGIN_SYNC);

    let mut run = Run::default();
    for change in diff {
        if run.extends_to(change) {
            run.push(change);
        } else {
            run.emit(&mut frame);
            run = Run::start(change);
        }
    }
    run.emit(&mut frame);

    frame.bytes(encode::END_SYNC);
    terminal.write_frame(frame).await
}

/// A run of adjacent same-row, same-style cells accumulated for one write.
#[derive(Default)]
struct Run {
    /// The run's starting position, or `None` when the run is empty.
    start: Option<rabbitui_core::geometry::Position>,
    /// The column the next contiguous cell would occupy.
    next_x: u16,
    /// The row every cell in the run shares.
    y: u16,
    /// The style every cell in the run shares.
    style: Style,
    /// The concatenated grapheme text of the run.
    text: String,
}

impl Run {
    /// Begins a fresh run at `change`.
    fn start(change: &CellChange) -> Self {
        let mut run = Self::default();
        run.push(change);
        run.start = Some(change.position);
        run.y = change.position.y;
        run.style = change.cell.style;
        run
    }

    /// Returns true if `change` continues this run: same row, same style, and
    /// positioned exactly where the previous cell's width left off.
    fn extends_to(&self, change: &CellChange) -> bool {
        self.start.is_some()
            && change.position.y == self.y
            && change.position.x == self.next_x
            && change.cell.style == self.style
    }

    /// Appends `change`'s symbol and advances the expected next column by the
    /// grapheme's width (so a wide grapheme's skipped continuation cell does
    /// not break the run).
    fn push(&mut self, change: &CellChange) {
        if self.start.is_none() {
            self.start = Some(change.position);
            self.y = change.position.y;
            self.style = change.cell.style;
        }
        let width = change.cell.width().max(1) as u16;
        self.text.push_str(&change.cell.symbol);
        self.next_x = change.position.x + width;
    }

    /// Emits the run as a cursor move + SGR + text into `frame`, unless empty.
    fn emit(&self, frame: &mut CommandBuffer) {
        let Some(start) = self.start else {
            return;
        };
        let position =
            ProtocolPosition::new(start.y.saturating_add(1), start.x.saturating_add(1));
        frame.command(commands::cursor::move_to(position));
        frame.bytes(encode::sgr(self.style));
        frame.text(&self.text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rabbitui_core::buffer::Cell;
    use rabbitui_core::geometry::Position;
    use rabbitui_core::style::{Color, Style};

    /// Builds a frame the way [`render`] does but into a returned buffer, so
    /// tests can assert on the exact bytes without a live terminal.
    fn encode_frame(diff: &[CellChange]) -> Vec<u8> {
        let mut frame = CommandBuffer::new();
        if diff.is_empty() {
            return frame.into_bytes();
        }
        frame.bytes(encode::BEGIN_SYNC);
        let mut run = Run::default();
        for change in diff {
            if run.extends_to(change) {
                run.push(change);
            } else {
                run.emit(&mut frame);
                run = Run::start(change);
            }
        }
        run.emit(&mut frame);
        frame.bytes(encode::END_SYNC);
        frame.into_bytes()
    }

    fn change(x: u16, y: u16, symbol: &str, style: Style) -> CellChange {
        CellChange { position: Position::new(x, y), cell: Cell::new(symbol, style) }
    }

    #[test]
    fn empty_diff_emits_no_bytes() {
        assert!(encode_frame(&[]).is_empty());
    }

    #[test]
    fn frame_is_wrapped_in_sync_framing() {
        let bytes = encode_frame(&[change(0, 0, "a", Style::new())]);
        assert!(bytes.starts_with(encode::BEGIN_SYNC));
        assert!(bytes.ends_with(encode::END_SYNC));
    }

    #[test]
    fn adjacent_same_style_cells_merge_into_one_run() {
        let style = Style::new().fg(Color::GREEN);
        let diff =
            [change(0, 0, "a", style), change(1, 0, "b", style), change(2, 0, "c", style)];
        let bytes = encode_frame(&diff);
        // One cursor move (CSI ... H) and the text "abc" in a single run.
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(text.matches('H').count(), 1);
        assert!(text.contains("abc"));
    }

    #[test]
    fn style_change_breaks_the_run() {
        let a = Style::new().fg(Color::RED);
        let b = Style::new().fg(Color::BLUE);
        let diff = [change(0, 0, "x", a), change(1, 0, "y", b)];
        let text = String::from_utf8(encode_frame(&diff)).unwrap();
        // Two runs means two cursor moves.
        assert_eq!(text.matches('H').count(), 2);
    }

    #[test]
    fn row_change_breaks_the_run() {
        let style = Style::new();
        let diff = [change(0, 0, "x", style), change(1, 0, "y", style), change(0, 1, "z", style)];
        let text = String::from_utf8(encode_frame(&diff)).unwrap();
        assert_eq!(text.matches('H').count(), 2);
    }

    #[test]
    fn column_gap_breaks_the_run() {
        let style = Style::new();
        // A gap at x=1 (unchanged) means x=2 starts a new run.
        let diff = [change(0, 0, "x", style), change(2, 0, "y", style)];
        let text = String::from_utf8(encode_frame(&diff)).unwrap();
        assert_eq!(text.matches('H').count(), 2);
    }

    #[test]
    fn wide_grapheme_does_not_break_a_run() {
        let style = Style::new();
        // A wide grapheme at x=0 advances the next column to 2, so a cell at
        // x=2 continues the same run despite the skipped continuation cell.
        let diff = [change(0, 0, "世", style), change(2, 0, "x", style)];
        let text = String::from_utf8(encode_frame(&diff)).unwrap();
        assert_eq!(text.matches('H').count(), 1);
        assert!(text.contains("世x"));
    }
}
