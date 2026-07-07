//! Mapping qwertty's decoded input into rabbitui-core's input vocabulary.
//!
//! rabbitui-core is substrate-free (`docs/adr/0006-input-focus-events.md` §9):
//! it defines its own [`InputEvent`] and never
//! depends on qwertty. This module is the single seam where the substrate's
//! decoded events cross into the framework. All Escape/CSI interpretation lives
//! here; the core sees only clean semantic keys.
//!
//! # What maps, and what is dropped
//!
//! qwertty's semantic decoder produces an [`Event`](qwertty::Event) of a typed
//! [`KeyEvent`](qwertty::KeyEvent) or a lossless
//! [`Syntax`](qwertty::Event::Syntax) passthrough of a complete
//! [`SyntaxToken`](qwertty::SyntaxToken) (`docs/adr/0012-terminal-substrate.md`).
//! This module maps a key event's [`Key`](qwertty::Key):
//!
//! - [`qwertty::Key::Char`] → [`Key::Char`] — a printable scalar becomes a char
//!   key.
//! - the named controls → the matching core [`Key`]: `Enter` → [`Key::Enter`],
//!   `Tab` → [`Key::Tab`], `Backspace` → [`Key::Backspace`], `Escape` →
//!   [`Key::Escape`] (qwertty folds both `BS` and `DEL` into its Backspace key).
//! - [`qwertty::Key`] arrows → [`Key::Up`]/[`Key::Down`]/[`Key::Left`]/
//!   [`Key::Right`].
//!
//! - A **Ctrl-letter chord** (a raw C0 byte in `0x01..=0x1A`) → the letter
//!   [`Key::Char`] with the Ctrl [`Modifiers`]
//!   set — so an app can bind `Ctrl-L` (clear the input) even though qwertty has
//!   no modifier protocol yet, and a widget that ignores ctrl chords (TextInput)
//!   leaves them for the app. `Ctrl-I`/`Ctrl-M` are indistinguishable from
//!   Tab/Enter at the byte level and stay Tab/Enter, as every terminal treats
//!   them.
//!
//! # Mouse: an SGR bridge over preserved CSI (slice 7)
//!
//! qwertty emits no typed mouse events; an SGR mouse report arrives as a
//! **complete preserved CSI** — `CSI < b ; x ; y M/m` — inside
//! [`Event::Syntax`](qwertty::Event::Syntax) as a
//! [`SyntaxToken::Csi`](qwertty::SyntaxToken::Csi)
//! (`docs/adr/0006-input-focus-events.md` §5, slice-7 design note). This module
//! interprets that one complete CSI's already-parsed pieces (private marker,
//! parameters, final byte) into a core [`MouseEvent`] — the same interim posture
//! as the SGR *encoder*: qwertty owns byte framing, we bridge semantics until it
//! grows typed mouse events. This does **not** fork qwertty's byte decoder; it
//! reads the [`ControlSequence`](qwertty::ControlSequence)'s
//! [`ControlParams`](qwertty::ControlParams). Any CSI that is not a well-formed
//! SGR mouse report falls through to the "dropped" path.
//!
//! The `b` byte packs button + modifiers + motion/wheel flags: the low two bits
//! select the button (with wheel/no-button escapes), bit `0x04` is Shift, `0x08`
//! Alt, `0x10` Ctrl, `0x20` a motion (drag), and `0x40` a wheel event. Final byte
//! `M` is a press/motion, `m` a release. `x`/`y` are 1-based cell columns/rows,
//! converted to rabbitui's 0-based [`Position`].
//!
//! Everything else is **dropped** (mapped to `None`): non-mouse CSI sequences,
//! undecoded bytes, and control bytes rabbitui has no key for yet.
//! qwertty does not yet decode Shift-Tab, Home/End, Page Up/Down, or a forward
//! Delete key, so [`Key::BackTab`], [`Key::Home`], [`Key::End`],
//! [`Key::PageUp`], [`Key::PageDown`], and [`Key::Delete`] never arise from
//! this mapping in slice 3 — the core vocabulary is ahead of the substrate on
//! purpose, so widget code written against it needs no revision when qwertty
//! lands those protocols (ADR 0006 §9's "decode on top, delete module-by-module"
//! discipline). Dropping unmapped input is deliberate: a half-understood escape
//! sequence must never be mistaken for a binding.

use qwertty::{ControlSequence, Event as QwerttyEvent, Key as QKey, SyntaxToken};
use rabbitui_core::geometry::Position;
use rabbitui_core::input::{
    InputEvent, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind,
};

/// Maps one qwertty semantic [`Event`](qwertty::Event) to a core [`InputEvent`],
/// or `None` if rabbitui has no key for it (the event is dropped).
///
/// qwertty's decoder redesign (M4 semantic layer) replaced the flat
/// `InputEvent` with an [`Event`](qwertty::Event) of a typed [`KeyEvent`] or a
/// lossless [`Syntax`](qwertty::Event::Syntax) passthrough; this seam adapts to it
/// unchanged in behavior: text and named keys map to core keys, a Ctrl-letter
/// chord surfaces as the letter with the Ctrl modifier, an SGR mouse report (a
/// preserved CSI in `Syntax`) bridges to a core [`MouseEvent`], and everything
/// else is dropped.
///
/// # Examples
///
/// ```
/// use qwertty::{Event, Key as QKey, KeyEvent};
/// use rabbitui::input::from_qwertty;
/// use rabbitui_core::input::{InputEvent, Key};
///
/// let a = Event::Key(KeyEvent::new(QKey::Char('a')));
/// assert_eq!(from_qwertty(&a), Some(InputEvent::key(Key::Char('a'))));
/// ```
#[must_use]
pub fn from_qwertty(event: &QwerttyEvent) -> Option<InputEvent> {
    match event {
        QwerttyEvent::Key(key_event) => key_from_qwertty(key_event.key()),
        // A preserved CSI may be an SGR mouse report; bridge those semantics here.
        // Every other complete/malformed token has no core key.
        QwerttyEvent::Syntax(SyntaxToken::Csi(csi)) => {
            mouse_from_csi(csi).map(InputEvent::Mouse)
        }
        _ => None,
    }
}

/// Maps a qwertty semantic [`Key`](qwertty::Key) to a core [`InputEvent`], or
/// `None` when there is no core key.
fn key_from_qwertty(key: QKey) -> Option<InputEvent> {
    // A Ctrl-letter chord arrives as a raw C0 byte in 0x01..=0x1A; surface it as
    // the letter key with the Ctrl modifier so apps can bind Ctrl-L and friends (a
    // widget like TextInput leaves ctrl chords for the app — text_input.rs).
    if let QKey::Control(byte @ 0x01..=0x1A) = key {
        let letter = (b'a' + (byte - 1)) as char;
        return Some(InputEvent::Key(KeyEvent::new(Key::Char(letter)).ctrl()));
    }
    let core = match key {
        QKey::Char(ch) => Key::Char(ch),
        QKey::Up => Key::Up,
        QKey::Down => Key::Down,
        QKey::Left => Key::Left,
        QKey::Right => Key::Right,
        QKey::Enter => Key::Enter,
        QKey::Tab => Key::Tab,
        QKey::Backspace => Key::Backspace,
        QKey::Escape => Key::Escape,
        // Any other C0 control byte (outside the Ctrl-letter range) has no core
        // key yet; the app reads it via the update fallthrough if it needs it.
        QKey::Control(_) => return None,
        // qwertty's Key is non_exhaustive; unknown future variants drop.
        _ => return None,
    };
    Some(InputEvent::key(core))
}

/// Interprets a complete preserved CSI as an SGR mouse report, or `None` if it is
/// not one.
///
/// An SGR mouse report is `CSI < b ; x ; y M/m`: private marker `<`, three
/// decimal parameters, and final byte `M` (press/motion) or `m` (release). This
/// reads the [`ControlSequence`]'s already-parsed pieces — it does not re-parse
/// bytes. Any deviation (wrong marker, wrong final byte, non-decimal or missing
/// fields) returns `None`, leaving the CSI to the "dropped" path.
fn mouse_from_csi(csi: &ControlSequence) -> Option<MouseEvent> {
    let params = csi.params();
    // The report must carry the `<` private marker and end in `M` or `m`.
    if params.private_markers() != b"<" {
        return None;
    }
    let release = match params.final_byte() {
        b'M' => false,
        b'm' => true,
        _ => return None,
    };
    if !params.intermediates().is_empty() {
        return None;
    }

    // Parameter bytes exclude the leading private marker: they are `b;x;y`.
    let numeric = params.param_bytes();
    let mut fields = numeric.split(|&byte| byte == b';');
    let b = parse_u32(fields.next()?)?;
    let x = parse_u16(fields.next()?)?;
    let y = parse_u16(fields.next()?)?;
    if fields.next().is_some() {
        return None; // more than three fields is not a mouse report.
    }

    // 1-based protocol coordinates → 0-based cell position (a zero coordinate is
    // out of the protocol's range but is clamped rather than rejected).
    let position = Position::new(x.saturating_sub(1), y.saturating_sub(1));

    let modifiers = Modifiers {
        shift: b & 0x04 != 0,
        alt: b & 0x08 != 0,
        ctrl: b & 0x10 != 0,
    };
    let wheel = b & 0x40 != 0;
    let motion = b & 0x20 != 0;
    let low = b & 0x03;

    let (kind, button) = if wheel {
        // Wheel: low bit 0 = up (scroll content up), 1 = down.
        let lines = if low == 0 { -1 } else { 1 };
        (MouseKind::Scroll(lines), MouseButton::None)
    } else {
        let button = match low {
            0 => MouseButton::Left,
            1 => MouseButton::Middle,
            2 => MouseButton::Right,
            _ => MouseButton::None,
        };
        let kind = if release {
            MouseKind::Up
        } else if motion {
            MouseKind::Drag
        } else {
            MouseKind::Down
        };
        (kind, button)
    };

    Some(MouseEvent {
        kind,
        button,
        position,
        modifiers,
    })
}

/// Parses an ASCII decimal byte slice as a `u32`, or `None` if empty or
/// non-decimal.
fn parse_u32(bytes: &[u8]) -> Option<u32> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// Parses an ASCII decimal byte slice as a `u16`, or `None` if empty or
/// non-decimal.
fn parse_u16(bytes: &[u8]) -> Option<u16> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use qwertty::{KeyEvent, SemanticDecoder};
    use rabbitui_core::input::Key;

    /// Decodes `bytes` through qwertty's semantic decoder and returns the single
    /// event it produces (the test inputs each decode to exactly one).
    fn decode_one(bytes: &[u8]) -> QwerttyEvent {
        let mut decoder = SemanticDecoder::new();
        let mut events = decoder.feed(bytes);
        events.extend(decoder.finish());
        assert_eq!(events.len(), 1, "expected one event from {bytes:?}");
        events.into_iter().next().unwrap()
    }

    #[test]
    fn text_maps_to_char() {
        assert_eq!(
            from_qwertty(&QwerttyEvent::Key(KeyEvent::new(QKey::Char('z')))),
            Some(InputEvent::key(Key::Char('z')))
        );
    }

    #[test]
    fn carriage_return_maps_to_enter() {
        // CR (0x0d) decodes to the Enter key.
        assert_eq!(
            from_qwertty(&decode_one(b"\r")),
            Some(InputEvent::key(Key::Enter)),
        );
    }

    #[test]
    fn tab_and_escape_and_backspace_map() {
        assert_eq!(
            from_qwertty(&decode_one(b"\t")),
            Some(InputEvent::key(Key::Tab)),
        );
        // A lone ESC decodes to Escape once the decoder is flushed.
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b")),
            Some(InputEvent::key(Key::Escape)),
        );
        // Both BS (0x08) and DEL (0x7f) fold into the Backspace key.
        assert_eq!(
            from_qwertty(&decode_one(b"\x08")),
            Some(InputEvent::key(Key::Backspace)),
        );
        assert_eq!(
            from_qwertty(&decode_one(b"\x7f")),
            Some(InputEvent::key(Key::Backspace)),
        );
    }

    #[test]
    fn arrows_map() {
        assert_eq!(from_qwertty(&decode_one(b"\x1b[A")), Some(InputEvent::key(Key::Up)));
        assert_eq!(from_qwertty(&decode_one(b"\x1b[B")), Some(InputEvent::key(Key::Down)));
        assert_eq!(from_qwertty(&decode_one(b"\x1b[D")), Some(InputEvent::key(Key::Left)));
        assert_eq!(from_qwertty(&decode_one(b"\x1b[C")), Some(InputEvent::key(Key::Right)));
    }

    fn csi(bytes: &[u8]) -> QwerttyEvent {
        decode_one(bytes)
    }

    #[test]
    fn sgr_mouse_press_maps_to_mouse_down() {
        // CSI < 0 ; 5 ; 3 M — left button press at column 5, row 3 (1-based).
        let event = from_qwertty(&csi(b"\x1b[<0;5;3M")).unwrap();
        let mouse = event.as_mouse().unwrap();
        assert_eq!(mouse.kind, MouseKind::Down);
        assert_eq!(mouse.button, MouseButton::Left);
        // 1-based (5,3) → 0-based (4,2).
        assert_eq!(mouse.position, Position::new(4, 2));
        assert!(mouse.modifiers.is_empty());
    }

    #[test]
    fn sgr_mouse_release_maps_to_mouse_up() {
        // Final byte `m` is a release.
        let event = from_qwertty(&csi(b"\x1b[<0;5;3m")).unwrap();
        let mouse = event.as_mouse().unwrap();
        assert_eq!(mouse.kind, MouseKind::Up);
        assert_eq!(mouse.button, MouseButton::Left);
    }

    #[test]
    fn sgr_mouse_right_button_and_modifiers() {
        // b = 2 (right) | 0x04 (shift) | 0x10 (ctrl) = 22.
        let event = from_qwertty(&csi(b"\x1b[<22;1;1M")).unwrap();
        let mouse = event.as_mouse().unwrap();
        assert_eq!(mouse.button, MouseButton::Right);
        assert!(mouse.modifiers.shift);
        assert!(mouse.modifiers.ctrl);
        assert!(!mouse.modifiers.alt);
    }

    #[test]
    fn sgr_mouse_drag_sets_motion_kind() {
        // b = 0x20 (motion) | 0 (left) = 32 → a left-button drag.
        let event = from_qwertty(&csi(b"\x1b[<32;2;2M")).unwrap();
        assert_eq!(event.as_mouse().unwrap().kind, MouseKind::Drag);
    }

    #[test]
    fn sgr_wheel_up_and_down_map_to_scroll() {
        // b = 64 (0x40) → wheel up; b = 65 → wheel down.
        let up = from_qwertty(&csi(b"\x1b[<64;1;1M")).unwrap();
        assert_eq!(up.as_mouse().unwrap().kind, MouseKind::Scroll(-1));
        assert_eq!(up.as_mouse().unwrap().button, MouseButton::None);
        let down = from_qwertty(&csi(b"\x1b[<65;1;1M")).unwrap();
        assert_eq!(down.as_mouse().unwrap().kind, MouseKind::Scroll(1));
    }

    #[test]
    fn non_mouse_csi_is_dropped() {
        // A cursor-position report is a CSI but not a mouse report.
        assert_eq!(from_qwertty(&csi(b"\x1b[12;34R")), None);
        // A CSI with the private marker but the wrong final byte is not a mouse
        // report either.
        assert_eq!(from_qwertty(&csi(b"\x1b[<0;1;1H")), None);
    }

    #[test]
    fn ctrl_letter_c0_bytes_map_to_ctrl_char() {
        use qwertty::KeyEvent as QKeyEvent;
        use rabbitui_core::input::KeyEvent;
        // Ctrl-L is C0 byte 0x0c; it surfaces as the letter with the Ctrl modifier
        // so apps can bind it (and TextInput, which ignores ctrl chords, leaves it).
        assert_eq!(
            from_qwertty(&QwerttyEvent::Key(QKeyEvent::new(QKey::Control(0x0c)))),
            Some(InputEvent::Key(KeyEvent::new(Key::Char('l')).ctrl())),
        );
        // Ctrl-A is 0x01 (the low end of the range).
        assert_eq!(
            from_qwertty(&QwerttyEvent::Key(QKeyEvent::new(QKey::Control(0x01)))),
            Some(InputEvent::Key(KeyEvent::new(Key::Char('a')).ctrl())),
        );
        // A C0 byte outside the Ctrl-letter range has no core key.
        assert_eq!(
            from_qwertty(&QwerttyEvent::Key(QKeyEvent::new(QKey::Control(0x1c)))),
            None,
        );
    }
}
