//! The alternate-screen engine.
//!
//! [`AltEngine`] is the classic full-app render target: it takes over the
//! alternate screen buffer, hides the cursor, and paints a synchronized-output-
//! framed **cell diff** each frame with absolute cursor addressing. On exit the
//! terminal restores the prior screen, discarding everything the app drew — so
//! alt-screen output never enters scrollback (ADR 0013).
//!
//! # Purity and state
//!
//! The engine is a pure byte producer: given the current and previous frame
//! buffers it returns the bytes a terminal must receive, mutating only its own
//! tiny state (whether it has entered the alt screen yet). [`Terminal`] writes
//! the bytes; the encoder and cursor discipline stay here where a `vt100` test
//! can pin them.
//!
//! [`Terminal`]: crate::Terminal

use qwertty::CommandBuffer;
use rabbitui_core::buffer::Buffer;

use super::emit_runs;
use crate::encode;

/// A stateful alternate-screen render engine.
///
/// Construct one per app run (or per transition *into* alt-screen mode);
/// [`enter`](Self::enter) emits the mode-entry bytes, [`render`](Self::render)
/// emits each frame, and [`leave`](Self::leave) emits the teardown when the app
/// switches away or quits.
///
/// # Examples
///
/// ```
/// use rabbitui::engine::AltEngine;
/// use rabbitui_core::buffer::Buffer;
/// use rabbitui_core::geometry::{Position, Size};
/// use rabbitui_core::style::Style;
///
/// let mut engine = AltEngine::new();
/// let enter = engine.enter();
/// // Entering emits the alt-screen escape.
/// assert!(enter.windows(8).any(|w| w == b"\x1b[?1049h"));
///
/// let previous = Buffer::new(Size::new(5, 1));
/// let mut current = previous.clone();
/// current.set_string(Position::ORIGIN, "hi", Style::new());
/// let bytes = engine.render(&current, &previous);
/// assert!(String::from_utf8_lossy(&bytes).contains("hi"));
/// ```
#[derive(Debug, Default)]
pub struct AltEngine {
    /// Whether the alt screen has been entered. The first render after entering
    /// diffs against a blank buffer (the cleared alt screen), so the whole frame
    /// paints.
    entered: bool,
}

impl AltEngine {
    /// Creates an alt-screen engine that has not entered the alt screen yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Emits the bytes that enter the alternate screen: switch to the alt buffer,
    /// hide the cursor, and clear it.
    ///
    /// Call once when the mode becomes active. The following [`render`] diffs
    /// against a blank front buffer so the first frame paints in full onto the
    /// freshly cleared screen.
    ///
    /// [`render`]: Self::render
    pub fn enter(&mut self) -> Vec<u8> {
        self.entered = true;
        let mut out = Vec::new();
        out.extend_from_slice(encode::ENTER_ALT_SCREEN);
        out.extend_from_slice(encode::HIDE_CURSOR);
        out.extend_from_slice(encode::CLEAR_SCREEN);
        out
    }

    /// Emits one frame: the [`Buffer::diff`] of `current` against `previous`,
    /// coalesced into styled runs, wrapped in synchronized-output (mode 2026)
    /// framing.
    ///
    /// An empty diff emits nothing at all — an idle alt-screen app is silent
    /// (no-op frame suppression, ADR 0003). The caller keeps `current` as the
    /// next frame's `previous`.
    ///
    /// [`Buffer::diff`]: rabbitui_core::buffer::Buffer::diff
    pub fn render(&mut self, current: &Buffer, previous: &Buffer) -> Vec<u8> {
        let diff = current.diff(previous);
        if diff.is_empty() {
            return Vec::new();
        }
        let mut frame = CommandBuffer::new();
        frame.bytes(encode::BEGIN_SYNC);
        emit_runs(&mut frame, &diff);
        frame.bytes(encode::END_SYNC);
        frame.into_bytes()
    }

    /// Emits the teardown bytes: reset styles, show the cursor, leave the
    /// alternate screen.
    ///
    /// Call when switching away from alt-screen mode or quitting. This mirrors
    /// the direct restore sequence, but through the ordinary output path; the
    /// panic/drop restore in [`Terminal`](crate::Terminal) remains the
    /// unconditional backstop.
    pub fn leave(&mut self) -> Vec<u8> {
        self.entered = false;
        let mut out = Vec::new();
        out.extend_from_slice(encode::SGR_RESET);
        out.extend_from_slice(encode::SHOW_CURSOR);
        out.extend_from_slice(encode::LEAVE_ALT_SCREEN);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rabbitui_core::geometry::{Position, Size};
    use rabbitui_core::style::{Color, Style};

    #[test]
    fn enter_switches_and_clears() {
        let mut engine = AltEngine::new();
        let bytes = engine.enter();
        assert!(bytes.starts_with(encode::ENTER_ALT_SCREEN));
        assert!(bytes.windows(6).any(|w| w == encode::HIDE_CURSOR));
        assert!(bytes.windows(4).any(|w| w == encode::CLEAR_SCREEN));
    }

    #[test]
    fn render_is_sync_framed() {
        let mut engine = AltEngine::new();
        let previous = Buffer::new(Size::new(4, 1));
        let mut current = previous.clone();
        current.set_string(Position::ORIGIN, "ab", Style::new().fg(Color::GREEN));
        let bytes = engine.render(&current, &previous);
        assert!(bytes.starts_with(encode::BEGIN_SYNC));
        assert!(bytes.ends_with(encode::END_SYNC));
        assert!(String::from_utf8_lossy(&bytes).contains("ab"));
    }

    #[test]
    fn empty_diff_emits_nothing() {
        let mut engine = AltEngine::new();
        let buffer = Buffer::new(Size::new(4, 1));
        assert!(engine.render(&buffer, &buffer).is_empty());
    }

    #[test]
    fn leave_restores_and_exits_alt_screen() {
        let mut engine = AltEngine::new();
        let bytes = engine.leave();
        let leave = encode::LEAVE_ALT_SCREEN;
        assert!(bytes.windows(leave.len()).any(|w| w == leave));
        let show = encode::SHOW_CURSOR;
        assert!(bytes.windows(show.len()).any(|w| w == show));
    }
}
