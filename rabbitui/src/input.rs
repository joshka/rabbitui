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
//! # Mouse: adopting qwertty's typed mouse events (slice 7; M4 adoption)
//!
//! qwertty's M4 semantic decoder decodes an SGR (DEC 1006) mouse report
//! `CSI < b ; x ; y M/m` into a typed [`Event::Mouse`](qwertty::Event::Mouse)
//! carrying a [`MouseEvent`](qwertty::MouseEvent) — button, modifiers, kind
//! (press/release/motion/scroll), and 1-based coordinates, already parsed. This
//! module only remaps that vocabulary into the core [`MouseEvent`]: the kind and
//! button, the modifier set, and the 1-based coordinates converted to rabbitui's
//! 0-based [`Position`]. (This seam once parsed the raw preserved CSI itself; that
//! interim byte-level bridge retired the moment qwertty grew typed mouse events —
//! the same "decode on top, delete module-by-module" discipline as the keys.) A
//! report with no core meaning — a horizontal wheel tick — falls through to the
//! "dropped" path.
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

use qwertty::{
    Event as QwerttyEvent, Key as QKey, Modifiers as QModifiers, MouseButton as QMouseButton,
    MouseEvent as QMouseEvent, MouseEventKind as QMouseEventKind, ScrollDirection,
};
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
/// chord surfaces as the letter with the Ctrl modifier, a typed mouse event
/// bridges to a core [`MouseEvent`], and everything else is dropped.
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
        // qwertty's M4 layer decodes SGR mouse reports into a typed mouse event.
        QwerttyEvent::Mouse(mouse) => mouse_from_qwertty(mouse).map(InputEvent::Mouse),
        // Every other event (focus, resize, paste, preserved syntax) has no core
        // input yet and is dropped.
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

/// Bridges qwertty's typed [`MouseEvent`](qwertty::MouseEvent) to a core
/// [`MouseEvent`], or `None` for a report with no core meaning (a horizontal wheel
/// tick, or a future `non_exhaustive` variant).
///
/// qwertty already parsed the SGR (DEC 1006) report — button, modifiers, 1-based
/// coordinates — so this seam only remaps vocabularies: press/release/motion/
/// scroll to a [`MouseKind`], the button, 1-based coordinates to 0-based buffer
/// cells, and the modifier set.
fn mouse_from_qwertty(mouse: &QMouseEvent) -> Option<MouseEvent> {
    let kind = match mouse.kind() {
        QMouseEventKind::Press => MouseKind::Down,
        QMouseEventKind::Release => MouseKind::Up,
        QMouseEventKind::Moved => MouseKind::Drag,
        // One notch per report (qwertty never coalesces wheel ticks): up scrolls
        // content up (negative), down scrolls it down (positive).
        QMouseEventKind::Scroll(ScrollDirection::Up) => MouseKind::Scroll(-1),
        QMouseEventKind::Scroll(ScrollDirection::Down) => MouseKind::Scroll(1),
        // Horizontal wheel/trackpad has no core (vertical-only) scroll meaning yet.
        QMouseEventKind::Scroll(_) => return None,
        // qwertty's MouseEventKind is non_exhaustive.
        _ => return None,
    };
    let button = match mouse.button() {
        QMouseButton::Left => MouseButton::Left,
        QMouseButton::Middle => MouseButton::Middle,
        QMouseButton::Right => MouseButton::Right,
        // MouseButton::None, a wheel "button", a bare motion, and any future
        // high button (back/forward) all have no core button.
        _ => MouseButton::None,
    };
    // qwertty reports 1-based protocol coordinates; core uses 0-based buffer cells.
    let position = Position::new(
        mouse.column().saturating_sub(1),
        mouse.row().saturating_sub(1),
    );
    let modifiers = mouse.modifiers();
    let modifiers = Modifiers {
        ctrl: modifiers.contains(QModifiers::CTRL),
        alt: modifiers.contains(QModifiers::ALT),
        shift: modifiers.contains(QModifiers::SHIFT),
    };
    Some(MouseEvent::new(kind, button, position).with_modifiers(modifiers))
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
