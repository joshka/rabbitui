//! Pure byte-producing render engines (ADR 0013, slice-5 design note).
//!
//! The render path is split so escape-level behavior is testable without a tty:
//! a **mode engine** — [`AltEngine`] or [`InlineEngine`] — is a pure function
//! from `(previous state, commits, frame buffers)` to bytes plus next state, and
//! [`Terminal`](crate::Terminal) only *writes* the bytes an engine produces. The
//! engines are unit-tested by feeding their output to a real `vt100` parser
//! (`rabbitui-testing`'s `vt` module), which is the layer that catches framing,
//! clears, cursor discipline, and commit/tail interleaving that buffer equality
//! cannot (the tui2/textual-rs finding, ADR 0009).
//!
//! # The two engines
//!
//! - [`AltEngine`] paints the alternate screen. It enters the alt screen and
//!   hides the cursor on its first frame, then emits a synchronized-output-framed
//!   **cell diff** each frame using absolute cursor addressing (the classic
//!   full-app model). Commits are meaningless in alt-screen mode and are dropped
//!   (the runtime flushes them into scrollback *before* switching to alt, so
//!   nothing is lost — see the app loop).
//!
//! - [`InlineEngine`] maintains a **bounded live tail** at the bottom of the
//!   primary screen plus an **append-once commit channel** into native
//!   scrollback (ADR 0013's inline invariant). Per frame it anchors at the live
//!   region's top, erases from the cursor down, emits any pending commits
//!   (unwrapped, `\r\n`-terminated, so the terminal owns their wrapping and
//!   reflow), then paints the new live tail below the last commit — all inside
//!   mode-2026 framing. Cell diffing applies *within* a stable-height tail; any
//!   height change or commit flush forces a full tail repaint.
//!
//! # Shared run coalescing
//!
//! Both engines turn changed cells into styled *runs* (one cursor move + SGR +
//! text per run) with `emit_runs`; the alt engine coalesces a whole-buffer
//! diff, the inline engine coalesces the tail's per-row changes. Run merging
//! lives here, not in the paint primitive (ADR 0003).

mod alt;
mod inline;

pub use alt::AltEngine;
pub use inline::InlineEngine;

use qwertty::{CommandBuffer, ProtocolPosition, commands};
use rabbitui_core::buffer::CellChange;
use rabbitui_core::style::Style;

use crate::encode;

/// Coalesces `changes` into styled runs and appends them to `frame`, each run a
/// single absolute cursor move + SGR + text write.
///
/// Consecutive changes on the same row that share a style and are contiguous
/// (accounting for wide graphemes) merge into one write; a row change, style
/// change, or column gap starts a new run. `changes` must be in row-major order
/// (as [`Buffer::diff`](rabbitui_core::buffer::Buffer::diff) returns them).
///
/// This is the alt-screen and stable-tail paint path. The inline engine adds a
/// row offset to each change's position before calling in, so the shared run
/// logic addresses cells at their on-screen row.
pub(crate) fn emit_runs(frame: &mut CommandBuffer, changes: &[CellChange]) {
    let mut run = Run::default();
    for change in changes {
        if run.extends_to(change) {
            run.push(change);
        } else {
            run.emit(frame);
            run = Run::start(change);
        }
    }
    run.emit(frame);
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

    /// Emits the run as an absolute cursor move + SGR + text into `frame`,
    /// unless empty.
    fn emit(&self, frame: &mut CommandBuffer) {
        let Some(start) = self.start else {
            return;
        };
        let position = ProtocolPosition::new(start.y.saturating_add(1), start.x.saturating_add(1));
        frame.command(commands::cursor::move_to(position));
        frame.bytes(encode::sgr(self.style));
        frame.text(&self.text);
    }
}
