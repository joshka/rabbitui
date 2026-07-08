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
//! This module maps a key event's [`Key`](qwertty::Key), carrying its
//! [`modifiers()`](qwertty::KeyEvent::modifiers) onto the core event:
//!
//! - [`qwertty::Key::Char`] → [`Key::Char`] — a printable scalar becomes a char
//!   key.
//! - the named controls → the matching core [`Key`]: `Enter` → [`Key::Enter`],
//!   `Tab` → [`Key::Tab`], `Backspace` → [`Key::Backspace`], `Escape` →
//!   [`Key::Escape`] (qwertty folds both `BS` and `DEL` into its Backspace key).
//! - [`qwertty::Key`] arrows → [`Key::Up`]/[`Key::Down`]/[`Key::Left`]/
//!   [`Key::Right`].
//! - the navigation/editing keys qwertty now decodes from their legacy escape
//!   forms → the matching core [`Key`]: `Home` → [`Key::Home`], `End` →
//!   [`Key::End`], `PageUp` → [`Key::PageUp`], `PageDown` → [`Key::PageDown`],
//!   `Insert` → [`Key::Insert`], `Delete` → [`Key::Delete`].
//! - **Shift-Tab**: qwertty decodes `CSI Z` as `Key::Tab` with the SHIFT
//!   modifier; this seam folds that specific chord into [`Key::BackTab`] (the
//!   dedicated back-traversal key the focus router matches), dropping the now
//!   redundant Shift modifier so `BackTab` reads as a bare key.
//!
//! # Modifiers
//!
//! qwertty's [`KeyEvent`](qwertty::KeyEvent) carries a
//! [`Modifiers`](qwertty::Modifiers) set (SHIFT/ALT/CTRL and the lock/super/…
//! bits). This seam maps SHIFT/ALT/CTRL onto the core [`Modifiers`] so chorded
//! keys (`Ctrl-arrow`, `Alt-Backspace`, …) surface with their modifiers; the
//! non-core bits (Super/Hyper/Meta/lock states) are dropped. Legacy input that
//! carries no modifier field decodes to an empty set, exactly as before.
//!
//! - A **Ctrl-letter chord** (a raw C0 byte in `0x01..=0x1A`) → the letter
//!   [`Key::Char`] with the Ctrl [`Modifiers`]
//!   set — so an app can bind `Ctrl-L` (clear the input) on terminals that send
//!   the bare C0 byte (no kitty protocol), and a widget that ignores ctrl chords
//!   (TextInput) leaves them for the app. `Ctrl-I`/`Ctrl-M` are indistinguishable
//!   from Tab/Enter at the byte level and stay Tab/Enter, as every terminal
//!   treats them.
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
//! # Associated text (`TextPayload`) — single-codepoint only, by decision
//!
//! qwertty's [`KeyEvent`](qwertty::KeyEvent) carries an optional
//! [`TextPayload`](qwertty::TextPayload): a small, *multi-codepoint-capable*
//! string for the kitty associated-text field (IME/compose runs, decomposed
//! accents, ZWJ clusters). This seam deliberately does **not** consume the
//! payload. It maps [`qwertty::Key::Char`]'s single scalar to [`Key::Char`] and
//! stops there, which is exactly right for every path rabbitui takes today:
//!
//! - The **legacy UTF-8 path** — the only one a terminal without the negotiated
//!   kitty protocol takes — emits one `Key::Char(ch)` per character, its payload
//!   holding that same single char (`event.rs::push_text_events`). The keycode
//!   already carries the character; the payload is redundant here.
//! - A **multi-codepoint payload** only arises from the kitty `CSI u`
//!   associated-text field, which requires negotiating the kitty protocol with
//!   text reporting — rabbitui does not enable it yet.
//!
//! Consuming a multi-codepoint payload would need a **new core input
//! representation** (a text/paste-like variant, or `Key::Text(String)`), because
//! core [`Key`]/[`KeyEvent`]/[`InputEvent`] are all `Copy` and a single [`char`]
//! cannot hold a cluster. That is a larger core change — non-`Copy` ripples
//! through the whole event pipeline and every widget's `match key.key`. Per Arc 4
//! item 8 step 3's "do the minimal correct thing", it is deferred, not
//! half-built: typing stays correct, `TextInput` (in `rabbitui-widgets`) keeps
//! consuming `Key::Char`, and no widget needs revision.
//!
//! TODO(arc4-item8): when rabbitui negotiates the kitty protocol with
//! associated-text reporting, add an additive core paste/text input variant and
//! route multi-codepoint [`TextPayload`](qwertty::TextPayload)s through it (TextInput inserts the whole
//! payload string at the cursor). Until then a multi-codepoint payload's extra
//! codepoints are not delivered — no such payload reaches this seam in the
//! current (legacy-only) configuration.
//!
//! Everything else is **dropped** (mapped to `None`): non-mouse CSI sequences,
//! undecoded bytes, and control bytes rabbitui has no key for yet — the function
//! keys ([`qwertty::Key::Function`]) among them, since core has no `F`-key
//! vocabulary. A press/repeat/release distinction
//! ([`KeyEventKind`](qwertty::KeyEventKind)) is not
//! surfaced: core has no release events, so a `Release` maps identically to a
//! `Press` (the app sees each mapped key once per press; auto-repeat surfaces as
//! repeated presses). Dropping unmapped input is deliberate: a half-understood
//! escape sequence must never be mistaken for a binding.

use qwertty::{
    Event as QwerttyEvent, Key as QKey, KeyEvent as QKeyEvent, Modifiers as QModifiers,
    MouseButton as QMouseButton, MouseEvent as QMouseEvent, MouseEventKind as QMouseEventKind,
    ScrollDirection,
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
/// lossless [`Syntax`](qwertty::Event::Syntax) passthrough; this seam adapts to it:
/// text, named, and navigation/editing keys map to core keys carrying their
/// modifiers, a Ctrl-letter chord surfaces as the letter with the Ctrl modifier,
/// a typed mouse event bridges to a core [`MouseEvent`], and everything else is
/// dropped.
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
        QwerttyEvent::Key(key_event) => key_from_qwertty(key_event).map(InputEvent::Key),
        // qwertty's M4 layer decodes SGR mouse reports into a typed mouse event.
        QwerttyEvent::Mouse(mouse) => mouse_from_qwertty(mouse).map(InputEvent::Mouse),
        // Every other event (focus, resize, paste, preserved syntax) has no core
        // input yet and is dropped.
        _ => None,
    }
}

/// Maps a qwertty semantic [`KeyEvent`](qwertty::KeyEvent) to a core
/// [`KeyEvent`], carrying its modifiers, or `None` when there is no core key.
fn key_from_qwertty(event: &QKeyEvent) -> Option<KeyEvent> {
    let key = event.key();
    let modifiers = modifiers_from_qwertty(event.modifiers());

    // A Ctrl-letter chord arrives as a raw C0 byte in 0x01..=0x1A on terminals
    // without the kitty protocol; surface it as the letter key with the Ctrl
    // modifier so apps can bind Ctrl-L and friends (a widget like TextInput leaves
    // ctrl chords for the app — text_input.rs). Any modifiers qwertty already
    // reported union with the synthesized Ctrl.
    if let QKey::Control(byte @ 0x01..=0x1A) = key {
        let letter = (b'a' + (byte - 1)) as char;
        return Some(
            KeyEvent::new(Key::Char(letter))
                .with_modifiers(modifiers)
                .ctrl(),
        );
    }

    // Shift-Tab: qwertty decodes `CSI Z` as Tab + SHIFT. Fold that specific chord
    // into the dedicated BackTab key the focus router matches, and drop the now
    // redundant Shift so BackTab reads as a bare key. A Tab with other modifiers
    // (e.g. Ctrl-Tab) stays Tab and keeps them.
    if key == QKey::Tab && modifiers == Modifiers::NONE.with_shift() {
        return Some(KeyEvent::new(Key::BackTab));
    }

    let core = match key {
        // Single-codepoint only: the char keycode is the whole character on the
        // legacy path. A multi-codepoint `TextPayload` (kitty associated text) is
        // deliberately not consumed here — see the module docs' "Associated text"
        // section for the decision and the deferred core-variant plan.
        QKey::Char(ch) => Key::Char(ch),
        QKey::Up => Key::Up,
        QKey::Down => Key::Down,
        QKey::Left => Key::Left,
        QKey::Right => Key::Right,
        QKey::Enter => Key::Enter,
        QKey::Tab => Key::Tab,
        QKey::Backspace => Key::Backspace,
        QKey::Escape => Key::Escape,
        QKey::Home => Key::Home,
        QKey::End => Key::End,
        QKey::PageUp => Key::PageUp,
        QKey::PageDown => Key::PageDown,
        QKey::Insert => Key::Insert,
        QKey::Delete => Key::Delete,
        // Function keys have no core vocabulary; the app reads them via the
        // preserved-syntax escape hatch if it needs them.
        QKey::Function(_) => return None,
        // Any other C0 control byte (outside the Ctrl-letter range) has no core
        // key yet; the app reads it via the update fallthrough if it needs it.
        QKey::Control(_) => return None,
        // qwertty's Key is non_exhaustive; unknown future variants drop.
        _ => return None,
    };
    Some(KeyEvent::new(core).with_modifiers(modifiers))
}

/// Maps qwertty's [`Modifiers`](qwertty::Modifiers) to the core [`Modifiers`],
/// keeping the three qwertty and core share (Ctrl/Alt/Shift) and dropping the
/// bits core has no vocabulary for (Super/Hyper/Meta and the lock states).
fn modifiers_from_qwertty(modifiers: QModifiers) -> Modifiers {
    Modifiers {
        ctrl: modifiers.contains(QModifiers::CTRL),
        alt: modifiers.contains(QModifiers::ALT),
        shift: modifiers.contains(QModifiers::SHIFT),
    }
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
    let modifiers = modifiers_from_qwertty(mouse.modifiers());
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
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[A")),
            Some(InputEvent::key(Key::Up))
        );
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[B")),
            Some(InputEvent::key(Key::Down))
        );
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[D")),
            Some(InputEvent::key(Key::Left))
        );
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[C")),
            Some(InputEvent::key(Key::Right))
        );
    }

    #[test]
    fn navigation_keys_map_from_legacy_forms() {
        // qwertty decodes these legacy escape forms; the seam now maps them to the
        // core navigation/editing keys (previously dropped as preserved CSI).
        // Home/End via the SS3-in-CSI letter forms `CSI H` / `CSI F`.
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[H")),
            Some(InputEvent::key(Key::Home))
        );
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[F")),
            Some(InputEvent::key(Key::End))
        );
        // Home/End also have the alternate tilde forms `CSI 1 ~` / `CSI 4 ~`.
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[1~")),
            Some(InputEvent::key(Key::Home))
        );
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[4~")),
            Some(InputEvent::key(Key::End))
        );
        // The editing tilde block: Insert (2~), Delete (3~), PageUp (5~), PageDown (6~).
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[2~")),
            Some(InputEvent::key(Key::Insert))
        );
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[3~")),
            Some(InputEvent::key(Key::Delete))
        );
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[5~")),
            Some(InputEvent::key(Key::PageUp))
        );
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[6~")),
            Some(InputEvent::key(Key::PageDown))
        );
    }

    #[test]
    fn shift_tab_maps_to_backtab() {
        // qwertty decodes `CSI Z` as Tab + SHIFT; the seam folds that chord into
        // the dedicated BackTab key, dropping the redundant Shift.
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[Z")),
            Some(InputEvent::key(Key::BackTab)),
        );
    }

    #[test]
    fn modifiers_are_carried_onto_keys() {
        use rabbitui_core::input::{KeyEvent, Modifiers};
        // A modified arrow `CSI 1 ; 5 A` is Ctrl+Up in the kitty modifier encoding
        // (modifier field 5 = 1 + CTRL bit); the Ctrl modifier surfaces on the core
        // key so an app can bind Ctrl-Up.
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[1;5A")),
            Some(InputEvent::Key(
                KeyEvent::new(Key::Up).with_modifiers(Modifiers::NONE.with_ctrl())
            )),
        );
        // A modified Home `CSI 1 ; 2 H` is Shift+Home (modifier field 2 = 1 + SHIFT).
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[1;2H")),
            Some(InputEvent::Key(
                KeyEvent::new(Key::Home).with_modifiers(Modifiers::NONE.with_shift())
            )),
        );
    }

    #[test]
    fn kitty_ctrl_char_carries_modifier() {
        use rabbitui_core::input::{KeyEvent, Modifiers};
        // Under the kitty protocol a Ctrl chord arrives as a typed KeyEvent with the
        // CTRL modifier (not a bare C0 byte): `CSI 99 ; 5 u` is Ctrl+c. The seam maps
        // the modifier straight through, so bindings work on kitty-enabled terminals
        // the same as on the C0-byte path.
        assert_eq!(
            from_qwertty(&decode_one(b"\x1b[99;5u")),
            Some(InputEvent::Key(
                KeyEvent::new(Key::Char('c')).with_modifiers(Modifiers::NONE.with_ctrl())
            )),
        );
    }

    #[test]
    fn char_key_maps_to_single_codepoint_ignoring_payload() {
        // Decision (Arc 4 item 8 step 3): map the char keycode, not the associated
        // TextPayload. A qwertty KeyEvent whose payload is multi-codepoint (an IME
        // cluster) still surfaces only its single-scalar keycode — core has no
        // multi-codepoint key type, and adding one is deferred. This test pins the
        // single-codepoint contract so a later payload-consuming change is a
        // deliberate, visible edit here.
        use qwertty::TextPayload;
        let event =
            QKeyEvent::new(QKey::Char('e')).with_text_payload(TextPayload::from_text("e\u{0301}"));
        assert_eq!(
            from_qwertty(&QwerttyEvent::Key(event)),
            Some(InputEvent::key(Key::Char('e'))),
        );
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
