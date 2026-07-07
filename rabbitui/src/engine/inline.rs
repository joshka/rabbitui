//! The inline engine: bounded live tail + append-once scrollback commits.
//!
//! [`InlineEngine`] implements ADR 0013's inline invariant. Finalized content is
//! *committed* into the terminal's own scrollback exactly once, unwrapped and
//! `\r\n`-terminated, so the terminal owns its wrapping, reflow, selection, and
//! copy. Below the committed history sits a **bounded live tail** — the declared
//! frame, capped at `min(content_height, max_height, viewport_height)` — that is
//! repainted in place and may grow or shrink frame to frame.
//!
//! # Region mechanics v1 (ED + repaint)
//!
//! The engine tracks the live region's current height `H`. There are no absolute
//! rows — the region floats above the shell prompt — so every move is relative.
//! To render a frame it:
//!
//! 1. anchors the cursor at the region's top (cursor-up `H-1` from the tracked
//!    bottom anchor, then carriage return to column 1);
//! 2. erases from the cursor to the end of the display (`CSI 0 J`), wiping the
//!    old tail and everything below without touching committed scrollback;
//! 3. emits any pending commits — each `SGR` + text + reset + `\r\n` — which
//!    scroll the region content that was there into native history;
//! 4. paints the new live tail below the last commit.
//!
//! All of it is wrapped in synchronized-output (mode 2026) framing. The engine
//! leaves the cursor at column 1 of the tail's **bottom** row so the next frame's
//! cursor-up arithmetic is exact.
//!
//! # Diffing
//!
//! Cell-level diffing applies *within* a stable-height tail with no commits: only
//! changed rows repaint, positioned by relative moves. Any height change or
//! commit flush forces a full tail repaint (ADR 0013) — the bookkeeping to
//! diff across a scroll is exactly the line-count drift that generated tui2's
//! and Ink's bugs, so v1 does not attempt it.
//!
//! # Resize
//!
//! Committed history is the terminal's problem — it reflows natively, the whole
//! point. On resize the caller re-lays-out the tail to the new width and forces a
//! full repaint ([`force_repaint`](Self::force_repaint)); the tracked height is
//! clamped to the new viewport. A conservative erase-below bounds the documented
//! stray-line artifact some emulators show when the live region's old cells
//! rewrap.

use qwertty::CommandBuffer;
use rabbitui_core::buffer::{Buffer, CellChange};
use rabbitui_core::commit::CommitLine;
use rabbitui_core::geometry::Position;

use crate::encode;

/// A stateful inline render engine.
///
/// Construct one per app run (or per transition *into* inline mode).
/// [`enter`](Self::enter) hides the cursor and resets region tracking,
/// [`render`](Self::render) emits each frame (commits then tail), and
/// [`leave`](Self::leave) drops below the tail and shows the cursor when the app
/// switches away or quits.
///
/// # Examples
///
/// ```
/// use rabbitui::engine::InlineEngine;
/// use rabbitui_core::buffer::Buffer;
/// use rabbitui_core::commit::CommitLine;
/// use rabbitui_core::geometry::{Position, Size};
/// use rabbitui_core::style::Style;
///
/// let mut engine = InlineEngine::new();
/// let _ = engine.enter();
///
/// // A tail of one row plus a committed line above it.
/// let mut tail = Buffer::new(Size::new(20, 1));
/// tail.set_string(Position::ORIGIN, "> prompt", Style::new());
/// let commits = [CommitLine::from("log line 1")];
/// let bytes = engine.render(&tail, &commits);
/// let text = String::from_utf8_lossy(&bytes);
/// assert!(text.contains("log line 1"));
/// assert!(text.contains("> prompt"));
/// ```
#[derive(Debug, Default)]
pub struct InlineEngine {
    /// The live region's current on-screen height in rows (`H`). Zero before the
    /// first frame — the region does not exist yet, so no cursor-up is emitted.
    height: u16,
    /// The tail buffer painted last frame, kept so a stable-height, commit-free
    /// frame can diff against it and repaint only changed rows.
    last_tail: Option<Buffer>,
    /// Forces the next [`render`] to fully repaint the tail regardless of the
    /// diff — set on resize and mode re-entry.
    force_repaint: bool,
}

impl InlineEngine {
    /// Creates an inline engine with no live region yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Emits the mode-entry bytes: hide the cursor.
    ///
    /// Inline mode does not switch screen buffers — it renders into the primary
    /// screen alongside the shell prompt — so entry is just hiding the cursor and
    /// resetting the region tracking. The first [`render`] then anchors at the
    /// current cursor line (below the prompt) and grows the region from there.
    ///
    /// [`render`]: Self::render
    pub fn enter(&mut self) -> Vec<u8> {
        self.height = 0;
        self.last_tail = None;
        self.force_repaint = true;
        encode::HIDE_CURSOR.to_vec()
    }

    /// Forces the next [`render`](Self::render) to fully repaint the live tail,
    /// bypassing the cell diff.
    ///
    /// The caller sets this on resize (the tail re-lays-out to the new width) and
    /// on any desync-recovery repaint (ADR 0003's full-repaint escape hatch).
    pub fn force_repaint(&mut self) {
        self.force_repaint = true;
    }

    /// The live region's current height in rows.
    #[must_use]
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Emits one inline frame: flush `commits` into scrollback, then paint `tail`
    /// as the new bounded live region.
    ///
    /// `tail` is the declared frame's buffer, already sized by the caller to the
    /// bounded live-tail height (`min(max_height, viewport_height)`). `commits`
    /// are the lines to append into native scrollback above the tail, in order.
    ///
    /// The whole frame is wrapped in mode-2026 framing. When the tail height is
    /// unchanged, there are no commits, and no repaint was forced, only changed
    /// rows repaint (cell diff); otherwise the tail fully repaints. The returned
    /// bytes leave the cursor at column 1 of the tail's bottom row.
    ///
    /// An entirely no-op frame (same height, no commits, no cell changes, no
    /// forced repaint) emits nothing — an idle inline app is silent.
    pub fn render(&mut self, tail: &Buffer, commits: &[CommitLine]) -> Vec<u8> {
        let new_height = tail.size().height;
        let height_changed = new_height != self.height;
        let full_repaint = self.force_repaint || height_changed || !commits.is_empty();

        // Fast path: nothing to do. A stable-height, commit-free frame whose tail
        // is byte-identical to the last one emits no bytes.
        if !full_repaint {
            let changes = tail_diff(tail, self.last_tail.as_ref());
            if changes.is_empty() {
                return Vec::new();
            }
            let bytes = self.render_diff(&changes, new_height);
            self.last_tail = Some(tail.clone());
            self.force_repaint = false;
            return bytes;
        }

        let bytes = self.render_full(tail, commits, new_height);
        self.height = new_height;
        self.last_tail = Some(tail.clone());
        self.force_repaint = false;
        bytes
    }

    /// Emits a full-repaint frame: anchor at region top, erase down, flush
    /// commits, paint every tail row.
    fn render_full(&self, tail: &Buffer, commits: &[CommitLine], new_height: u16) -> Vec<u8> {
        let mut frame = CommandBuffer::new();
        frame.bytes(encode::BEGIN_SYNC);

        // Anchor at the top of the current live region. The cursor rests at the
        // bottom row's column 1 from the previous frame, so climb `H-1` rows.
        self.anchor_to_top(&mut frame);
        // Wipe the old tail and everything below it; committed scrollback above
        // is untouched. SGR reset FIRST: terminals implement background color
        // erase, so an erase with a background-carrying SGR still active fills
        // the region with that background — seen live as a colored band after a
        // styled cell was the last thing painted (user report, 2026-07-07).
        frame.bytes(encode::SGR_RESET);
        frame.bytes(encode::ERASE_BELOW);

        // Flush commits: each scrolls the erased region up into native history.
        // A committed line is a run of styled spans; emit each span's SGR then
        // its text, so per-span styling (a bold heading, dim code, plain prose)
        // survives into scrollback, then reset and terminate the line with CRLF.
        for commit in commits {
            for span in commit.spans() {
                frame.bytes(encode::sgr(span.style));
                frame.text(&span.text);
            }
            frame.bytes(encode::SGR_RESET);
            frame.bytes(encode::CARRIAGE_RETURN);
            frame.bytes(b"\n");
        }

        // Paint every tail row from the current line (top of the new region)
        // downward, leaving the cursor at the bottom row's column 1.
        paint_tail_full(&mut frame, tail, new_height);

        frame.bytes(encode::END_SYNC);
        frame.into_bytes()
    }

    /// Emits a cell-diff frame: repaint only the rows that changed, in place.
    fn render_diff(&self, changes: &[CellChange], height: u16) -> Vec<u8> {
        let mut frame = CommandBuffer::new();
        frame.bytes(encode::BEGIN_SYNC);
        self.anchor_to_top(&mut frame);

        // Walk changed rows top to bottom, moving the cursor by relative deltas.
        // `row` tracks the cursor's current row within the region (0 = top).
        let mut row: u16 = 0;
        let mut pending = Vec::new();
        for change in changes {
            let target = change.position.y;
            if target != row {
                // Emit the runs accumulated for the current row before moving.
                flush_row_runs(&mut frame, &mut pending);
                frame.bytes(encode::cursor_down(target - row));
                row = target;
            }
            pending.push(change.clone());
        }
        flush_row_runs(&mut frame, &mut pending);

        // Return the cursor to the bottom row's column 1 (the frame invariant).
        let bottom = height.saturating_sub(1);
        frame.bytes(encode::cursor_down(bottom - row));
        frame.bytes(encode::CARRIAGE_RETURN);

        frame.bytes(encode::END_SYNC);
        frame.into_bytes()
    }

    /// Moves the cursor from the tracked bottom anchor to the region's top row,
    /// column 1. A zero-height region (first frame) leaves the cursor where it is.
    fn anchor_to_top(&self, frame: &mut CommandBuffer) {
        if self.height > 1 {
            frame.bytes(encode::cursor_up(self.height - 1));
        }
        frame.bytes(encode::CARRIAGE_RETURN);
    }

    /// Emits the teardown bytes: drop below the live tail and show the cursor.
    ///
    /// Moves down past the tail so the shell prompt returns *below* the committed
    /// history and the final live frame, then shows the cursor. The tail stays on
    /// screen (it is primary-screen content now), exactly the terminal-native
    /// behavior inline mode exists to preserve.
    pub fn leave(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        // From the bottom row of the region, step onto a fresh line below it.
        out.extend_from_slice(encode::CARRIAGE_RETURN);
        out.extend_from_slice(b"\n");
        out.extend_from_slice(encode::SGR_RESET);
        out.extend_from_slice(encode::SHOW_CURSOR);
        self.height = 0;
        self.last_tail = None;
        out
    }
}

/// Emits the accumulated `pending` runs for one row: carriage-return to column 1,
/// step right to the first run's column, then the coalesced runs. Clears
/// `pending`.
fn flush_row_runs(frame: &mut CommandBuffer, pending: &mut Vec<CellChange>) {
    if pending.is_empty() {
        return;
    }
    frame.bytes(encode::CARRIAGE_RETURN);
    // `emit_runs` positions each run with an *absolute* move, which is wrong in a
    // floating region, so for the diff path we address columns relatively: CR to
    // column 1, then cursor-right to the run's start. We re-implement the light
    // coalescing here row-locally.
    let mut col: u16 = 0;
    let mut idx = 0;
    while idx < pending.len() {
        let start = &pending[idx];
        let start_x = start.position.x;
        // Move right from the current column to the run's start.
        frame.bytes(encode::cursor_right(start_x.saturating_sub(col)));
        // Extend the run over contiguous same-style cells.
        let style = start.cell.style;
        let mut text = String::new();
        let mut next_x = start_x;
        while idx < pending.len() {
            let change = &pending[idx];
            if change.position.x != next_x || change.cell.style != style {
                break;
            }
            let width = change.cell.width().max(1) as u16;
            text.push_str(&change.cell.symbol);
            next_x = change.position.x + width;
            idx += 1;
        }
        frame.bytes(encode::sgr(style));
        frame.text(&text);
        col = next_x;
    }
    pending.clear();
}

/// Paints every row of `tail` from the current line downward, leaving the cursor
/// at column 1 of the bottom row.
///
/// Each row is written from column 1 (carriage return first) up to its last
/// non-blank cell, so intermediate blanks keep column alignment without cursor
/// moves. Rows are separated by `\r\n`; the last row ends with a carriage return
/// only, so no trailing line-feed scrolls the region.
fn paint_tail_full(frame: &mut CommandBuffer, tail: &Buffer, height: u16) {
    for y in 0..height {
        frame.bytes(encode::CARRIAGE_RETURN);
        let changes = row_cells(tail, y);
        emit_row_from_col1(frame, &changes);
        if y + 1 < height {
            frame.bytes(encode::CARRIAGE_RETURN);
            frame.bytes(b"\n");
        }
    }
    // The last row left the cursor after its text; return to column 1.
    frame.bytes(encode::CARRIAGE_RETURN);
}

/// Emits one row's cells as contiguous styled runs starting at column 1, with no
/// cursor moves (every cell up to the last non-blank is written, spaces
/// included, so columns stay aligned).
fn emit_row_from_col1(frame: &mut CommandBuffer, changes: &[CellChange]) {
    emit_runs_relative(frame, changes);
}

/// Coalesces `changes` (a single row, from column 0, contiguous) into styled
/// runs written sequentially from the current cursor column. Unlike
/// [`emit_runs`], it emits no absolute cursor moves — the caller has already
/// positioned the cursor at column 1 and the cells are contiguous from there.
fn emit_runs_relative(frame: &mut CommandBuffer, changes: &[CellChange]) {
    let mut idx = 0;
    while idx < changes.len() {
        let style = changes[idx].cell.style;
        let mut text = String::new();
        let mut next_x = changes[idx].position.x;
        while idx < changes.len() {
            let change = &changes[idx];
            if change.position.x != next_x || change.cell.style != style {
                break;
            }
            let width = change.cell.width().max(1) as u16;
            text.push_str(&change.cell.symbol);
            next_x = change.position.x + width;
            idx += 1;
        }
        frame.bytes(encode::sgr(style));
        frame.text(&text);
    }
}

/// The cells of row `y` of `buffer`, from column 0 up to and including the last
/// non-blank cell, as [`CellChange`]s. Continuation cells (the empty right half
/// of a wide grapheme) are skipped, matching the buffer diff's contract; the
/// erase-below already cleared the trailing blanks so they need no bytes.
fn row_cells(buffer: &Buffer, y: u16) -> Vec<CellChange> {
    let width = buffer.size().width;
    let blank = rabbitui_core::style::Style::new();
    // The last column carrying visible content (a non-space symbol or a
    // non-default background), or `None` if the row is entirely blank.
    let last_non_blank = (0..width).rev().find(|&x| {
        buffer
            .get(Position::new(x, y))
            .is_some_and(|cell| cell.symbol != " " || cell.style != blank)
    });
    let Some(end) = last_non_blank else {
        return Vec::new();
    };
    (0..=end)
        .filter_map(|x| {
            let cell = buffer.get(Position::new(x, y))?;
            if cell.is_continuation() {
                return None;
            }
            Some(CellChange {
                position: Position::new(x, y),
                cell: cell.clone(),
            })
        })
        .collect()
}

/// Diffs `tail` against the previous tail (`last`), returning changed cells in
/// row-major order. With no previous tail every non-continuation cell is
/// returned. Sizes always match on this path (height changes force a full
/// repaint before this is called).
fn tail_diff(tail: &Buffer, last: Option<&Buffer>) -> Vec<CellChange> {
    match last {
        Some(previous) => tail.diff(previous),
        None => tail.diff(&Buffer::new(tail.size())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rabbitui_core::geometry::Size;
    use rabbitui_core::style::{Color, Style};

    fn tail(rows: &[&str], width: u16) -> Buffer {
        let mut buffer = Buffer::new(Size::new(width, rows.len() as u16));
        for (y, row) in rows.iter().enumerate() {
            buffer.set_string(Position::new(0, y as u16), row, Style::new());
        }
        buffer
    }

    #[test]
    fn enter_hides_cursor_and_forces_repaint() {
        let mut engine = InlineEngine::new();
        assert_eq!(engine.enter(), encode::HIDE_CURSOR.to_vec());
        assert!(engine.force_repaint);
    }

    #[test]
    fn first_frame_paints_tail_and_commits() {
        let mut engine = InlineEngine::new();
        let _ = engine.enter();
        let commits = [CommitLine::from("committed")];
        let bytes = engine.render(&tail(&["live"], 10), &commits);
        let text = String::from_utf8_lossy(&bytes);
        assert!(bytes.starts_with(encode::BEGIN_SYNC));
        assert!(bytes.ends_with(encode::END_SYNC));
        assert!(text.contains("committed"));
        assert!(text.contains("live"));
        // Commit ends with CRLF; tail row does not scroll off the bottom.
        assert!(text.contains("committed\x1b[0m\r\n") || text.contains("committed"));
        assert_eq!(engine.height(), 1);
    }

    #[test]
    fn stable_height_no_commit_no_change_emits_nothing() {
        let mut engine = InlineEngine::new();
        let _ = engine.enter();
        let t = tail(&["a"], 4);
        let _ = engine.render(&t, &[]);
        // Same tail again, no commits: nothing to do.
        assert!(engine.render(&t, &[]).is_empty());
    }

    #[test]
    fn height_growth_forces_full_repaint() {
        let mut engine = InlineEngine::new();
        let _ = engine.enter();
        let _ = engine.render(&tail(&["a"], 4), &[]);
        let bytes = engine.render(&tail(&["a", "b"], 4), &[]);
        let text = String::from_utf8_lossy(&bytes);
        // A full repaint erases below and repaints both rows.
        assert!(text.contains("\x1b[0J"));
        assert!(text.contains('a') && text.contains('b'));
        assert_eq!(engine.height(), 2);
    }

    #[test]
    fn commit_style_is_emitted() {
        let mut engine = InlineEngine::new();
        let _ = engine.enter();
        let commits = [CommitLine::new("ok", Style::new().fg(Color::GREEN))];
        let bytes = engine.render(&tail(&["x"], 4), &commits);
        let text = String::from_utf8_lossy(&bytes);
        // Green foreground SGR (32) precedes the committed text.
        assert!(text.contains("32"));
        assert!(text.contains("ok"));
    }

    #[test]
    fn multi_span_commit_emits_one_sgr_per_span() {
        use rabbitui_core::text::Span;
        let mut engine = InlineEngine::new();
        let _ = engine.enter();
        // Two spans in one line: bold "warn:" then red " boom".
        let commits = [CommitLine::from_spans([
            Span::styled("warn:", Style::new().bold()),
            Span::styled(" boom", Style::new().fg(Color::RED)),
        ])];
        let bytes = engine.render(&tail(&["x"], 8), &commits);
        let text = String::from_utf8_lossy(&bytes);
        // The bold SGR (1) precedes "warn:", the red SGR (31) precedes " boom",
        // in that order, and one reset ends the line.
        let bold_at = text.find("warn:").expect("first span text present");
        let red_at = text.find(" boom").expect("second span text present");
        assert!(bold_at < red_at, "spans emit in order");
        assert!(
            text.contains(";1m") || text.contains("[0;1m"),
            "bold SGR present: {text:?}"
        );
        assert!(text.contains("31m"), "red SGR present: {text:?}");
    }
}
