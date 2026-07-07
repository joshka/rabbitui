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

/// The restore-of-last-resort sequence: leave alt screen, reset styles, show
/// the cursor. Written on drop and from the panic hook; every byte here must
/// be safe to emit unconditionally on any terminal state.
pub const RESTORE: &[u8] = b"\x1b[?1049l\x1b[0m\x1b[?25h";

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
}
