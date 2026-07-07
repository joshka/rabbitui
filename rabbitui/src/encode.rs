//! Escape-sequence encoding for capabilities the substrate does not cover yet.
//!
//! qwertty's command layer is deliberately narrow; per
//! `docs/adr/0012-terminal-substrate.md`, rabbitui bridges the gap with an
//! internal encoder that emits raw sequences through the substrate's escape
//! hatch. This module never decodes input (that stays the substrate's job) and
//! shrinks as qwertty grows equivalent typed commands.

use rabbitui_core::style::{Attrs, Color, Style};

/// Enter the alternate screen buffer (`CSI ? 1049 h`).
pub const ENTER_ALT_SCREEN: &[u8] = b"\x1b[?1049h";
/// Leave the alternate screen buffer (`CSI ? 1049 l`).
pub const LEAVE_ALT_SCREEN: &[u8] = b"\x1b[?1049l";
/// Reset all SGR attributes (`CSI 0 m`).
pub const SGR_RESET: &[u8] = b"\x1b[0m";

/// Begin synchronized output (`CSI ? 2026 h`): the terminal buffers everything
/// until the matching end so a frame is presented atomically, without tearing
/// (ADR 0003's mode-2026 framing).
pub const BEGIN_SYNC: &[u8] = b"\x1b[?2026h";
/// End synchronized output (`CSI ? 2026 l`): the terminal presents the buffered
/// frame in one update.
pub const END_SYNC: &[u8] = b"\x1b[?2026l";

/// Hide the cursor (`CSI ? 25 l`).
pub const HIDE_CURSOR: &[u8] = b"\x1b[?25l";
/// Show the cursor (`CSI ? 25 h`).
pub const SHOW_CURSOR: &[u8] = b"\x1b[?25h";
/// Clear the whole screen (`CSI 2 J`).
pub const CLEAR_SCREEN: &[u8] = b"\x1b[2J";
/// Erase from the cursor to the end of the display (`CSI 0 J`), leaving
/// everything above and to the left untouched. The inline engine clears its
/// live region this way — anchored at the region's top, it wipes the old tail
/// and everything below without disturbing committed scrollback (ADR 0013's
/// ED + repaint region mechanic).
pub const ERASE_BELOW: &[u8] = b"\x1b[0J";
/// Move the cursor to column 1 of the current row (`CR`).
pub const CARRIAGE_RETURN: &[u8] = b"\r";

/// The restore-of-last-resort sequence: leave alt screen, reset styles, show
/// the cursor. Written on drop and from the panic hook; every byte here must
/// be safe to emit unconditionally on any terminal state.
///
/// The leave-alt-screen byte is unconditional by design (ADR 0013): whichever
/// mode was active, RESTORE must always leave the alternate screen so a panic
/// mid-alt-screen never strands the user there.
pub const RESTORE: &[u8] = b"\x1b[?1049l\x1b[0m\x1b[?25h";

/// Encodes "move the cursor up `n` rows" (`CSI n A`). A zero count emits
/// nothing (the cursor is already on the target row), matching terminals that
/// treat `CSI 0 A` as a one-row move.
#[must_use]
pub fn cursor_up(n: u16) -> Vec<u8> {
    if n == 0 {
        return Vec::new();
    }
    format!("\x1b[{n}A").into_bytes()
}

/// Encodes "move the cursor down `n` rows" (`CSI n B`). A zero count emits
/// nothing.
#[must_use]
pub fn cursor_down(n: u16) -> Vec<u8> {
    if n == 0 {
        return Vec::new();
    }
    format!("\x1b[{n}B").into_bytes()
}

/// Encodes "move the cursor right `n` columns" (`CSI n C`). A zero count emits
/// nothing (the cursor is already at the target column). Used by the inline
/// engine to reach a changed run's start column from column 1 without absolute
/// row addressing.
#[must_use]
pub fn cursor_right(n: u16) -> Vec<u8> {
    if n == 0 {
        return Vec::new();
    }
    format!("\x1b[{n}C").into_bytes()
}

/// Encodes a style as an SGR sequence, starting from a reset so the result is
/// self-contained (no dependency on the terminal's current attribute state).
pub fn sgr(style: Style) -> Vec<u8> {
    let mut params = String::from("0");
    if style.attrs.contains(Attrs::BOLD) {
        params.push_str(";1");
    }
    if style.attrs.contains(Attrs::DIM) {
        params.push_str(";2");
    }
    if style.attrs.contains(Attrs::ITALIC) {
        params.push_str(";3");
    }
    if style.attrs.contains(Attrs::UNDERLINE) {
        params.push_str(";4");
    }
    if style.attrs.contains(Attrs::REVERSED) {
        params.push_str(";7");
    }
    if style.attrs.contains(Attrs::STRIKETHROUGH) {
        params.push_str(";9");
    }
    if let Some(fg) = style.fg {
        push_color(&mut params, fg, ColorLayer::Foreground);
    }
    if let Some(bg) = style.bg {
        push_color(&mut params, bg, ColorLayer::Background);
    }
    let mut out = Vec::with_capacity(params.len() + 3);
    out.extend_from_slice(b"\x1b[");
    out.extend_from_slice(params.as_bytes());
    out.push(b'm');
    out
}

enum ColorLayer {
    Foreground,
    Background,
}

fn push_color(params: &mut String, color: Color, layer: ColorLayer) {
    use std::fmt::Write;

    let base = match layer {
        ColorLayer::Foreground => 30,
        ColorLayer::Background => 40,
    };
    match color {
        Color::Reset => {
            let _ = write!(params, ";{}", base + 9);
        }
        Color::Ansi(index @ 0..=7) => {
            let _ = write!(params, ";{}", base + u16::from(index));
        }
        Color::Ansi(index @ 8..=15) => {
            let _ = write!(params, ";{}", base + 60 + u16::from(index - 8));
        }
        // Out-of-range ANSI values fall back to the 256-color form.
        Color::Ansi(index) | Color::Indexed(index) => {
            let _ = write!(params, ";{};5;{index}", base + 8);
        }
        Color::Rgb(r, g, b) => {
            let _ = write!(params, ";{};2;{r};{g};{b}", base + 8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_plain_style_is_reset_only() {
        assert_eq!(sgr(Style::new()), b"\x1b[0m");
    }

    #[test]
    fn sgr_encodes_attrs_and_colors() {
        let style = Style::new().fg(Color::RED).bg(Color::Rgb(1, 2, 3)).bold();
        assert_eq!(sgr(style), b"\x1b[0;1;31;48;2;1;2;3m".as_slice());
    }

    #[test]
    fn sgr_encodes_bright_ansi_colors() {
        let style = Style::new().fg(Color::Ansi(9));
        assert_eq!(sgr(style), b"\x1b[0;91m".as_slice());
    }

    #[test]
    fn sgr_encodes_indexed_colors() {
        let style = Style::new().bg(Color::Indexed(200));
        assert_eq!(sgr(style), b"\x1b[0;48;5;200m".as_slice());
    }

    #[test]
    fn cursor_up_encodes_csi_a() {
        assert_eq!(cursor_up(3), b"\x1b[3A".as_slice());
        // A zero move emits nothing, so it never becomes a stray one-row move.
        assert!(cursor_up(0).is_empty());
    }

    #[test]
    fn cursor_down_encodes_csi_b() {
        assert_eq!(cursor_down(2), b"\x1b[2B".as_slice());
        assert!(cursor_down(0).is_empty());
    }

    #[test]
    fn cursor_right_encodes_csi_c() {
        assert_eq!(cursor_right(5), b"\x1b[5C".as_slice());
        assert!(cursor_right(0).is_empty());
    }
}
