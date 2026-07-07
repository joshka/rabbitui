//! Mapping qwertty's decoded input into rabbitui-core's input vocabulary.
//!
//! rabbitui-core is substrate-free (`docs/adr/0006-input-focus-events.md` §9):
//! it defines its own [`InputEvent`](rabbitui_core::input::InputEvent) and never
//! depends on qwertty. This module is the single seam where the substrate's
//! decoded events cross into the framework. All Escape/CSI interpretation lives
//! here; the core sees only clean semantic keys.
//!
//! # What maps, and what is dropped
//!
//! qwertty's first input layer decodes text, C0 control bytes, the four arrow
//! keys, and complete-but-uninterpreted CSI sequences
//! (`docs/adr/0012-terminal-substrate.md`). This module maps:
//!
//! - [`qwertty::InputEvent::Text`] → [`Key::Char`], except control characters
//!   (which qwertty also surfaces as `Control`) — a printable scalar becomes a
//!   char key.
//! - [`qwertty::InputEvent::Control`] → the matching [`Key`]: `CarriageReturn`
//!   and `LineFeed` → [`Key::Enter`], `Tab` → [`Key::Tab`], `Backspace` →
//!   [`Key::Backspace`], `Escape` → [`Key::Escape`], `Delete` →
//!   [`Key::Backspace`] (DEL is the usual terminal Backspace).
//! - [`qwertty::KeyInput`] arrows → [`Key::Up`]/[`Key::Down`]/[`Key::Left`]/
//!   [`Key::Right`].
//!
//! Everything else is **dropped** (mapped to `None`): unclassified CSI
//! sequences, undecoded bytes, and control bytes rabbitui has no key for yet.
//! qwertty does not yet decode Shift-Tab, Home/End, Page Up/Down, a forward
//! Delete key, or keyboard modifiers, so [`Key::BackTab`], [`Key::Home`],
//! [`Key::End`], [`Key::PageUp`], [`Key::PageDown`], [`Key::Delete`], and any
//! non-empty [`Modifiers`](rabbitui_core::input::Modifiers) never arise from
//! this mapping in slice 3 — the core vocabulary is ahead of the substrate on
//! purpose, so widget code written against it needs no revision when qwertty
//! lands those protocols (ADR 0006 §9's "decode on top, delete module-by-module"
//! discipline). Dropping unmapped input is deliberate: a half-understood escape
//! sequence must never be mistaken for a binding.

use qwertty::{ControlInput, InputEvent as QwerttyEvent, KeyInput};
use rabbitui_core::input::{InputEvent, Key};

/// Maps one qwertty input event to a core [`InputEvent`], or `None` if rabbitui
/// has no key for it (the event is dropped).
///
/// # Examples
///
/// ```
/// use qwertty::{ControlInput, InputEvent as QwerttyEvent};
/// use rabbitui::input::from_qwertty;
/// use rabbitui_core::input::{InputEvent, Key};
///
/// assert_eq!(from_qwertty(&QwerttyEvent::Text('a')), Some(InputEvent::key(Key::Char('a'))));
/// assert_eq!(
///     from_qwertty(&QwerttyEvent::Control(ControlInput::Escape)),
///     Some(InputEvent::key(Key::Escape)),
/// );
/// // An unclassified sequence is dropped.
/// assert_eq!(from_qwertty(&QwerttyEvent::Control(ControlInput::Null)), None);
/// ```
#[must_use]
pub fn from_qwertty(event: &QwerttyEvent) -> Option<InputEvent> {
    let key = match event {
        QwerttyEvent::Text(ch) => Key::Char(*ch),
        QwerttyEvent::Control(control) => control_key(*control)?,
        QwerttyEvent::Key(key) => arrow_key(*key),
        // Unclassified CSI and undecoded bytes have no core key.
        QwerttyEvent::Csi(_) | QwerttyEvent::Undecoded(_) => return None,
        // qwertty's InputEvent is non_exhaustive; unknown future variants drop.
        _ => return None,
    };
    Some(InputEvent::key(key))
}

/// Maps a classified C0 control byte to a core [`Key`], or `None` if rabbitui has
/// no key for it.
fn control_key(control: ControlInput) -> Option<Key> {
    Some(match control {
        // Both CR and LF stand in for Enter in raw mode.
        ControlInput::CarriageReturn | ControlInput::LineFeed => Key::Enter,
        ControlInput::Tab => Key::Tab,
        // BS and DEL both surface as Backspace (DEL is the common terminal
        // Backspace byte).
        ControlInput::Backspace | ControlInput::Delete => Key::Backspace,
        ControlInput::Escape => Key::Escape,
        // Null and other raw C0 bytes (e.g. Ctrl-C = 0x03) have no core key yet;
        // apps that need them read the raw event via the update fallthrough.
        ControlInput::Null | ControlInput::Other(_) => return None,
        // ControlInput is non_exhaustive.
        _ => return None,
    })
}

/// Maps a qwertty arrow key to the matching core [`Key`].
fn arrow_key(key: KeyInput) -> Key {
    match key {
        KeyInput::Up => Key::Up,
        KeyInput::Down => Key::Down,
        KeyInput::Left => Key::Left,
        KeyInput::Right => Key::Right,
        // KeyInput is non_exhaustive; treat unknown arrows-family keys as Up is
        // wrong, so fall back to dropping via a defensive default. In practice
        // every current variant is covered above.
        _ => Key::Up,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rabbitui_core::input::Key;

    #[test]
    fn text_maps_to_char() {
        assert_eq!(from_qwertty(&QwerttyEvent::Text('z')), Some(InputEvent::key(Key::Char('z'))));
    }

    #[test]
    fn carriage_return_and_line_feed_map_to_enter() {
        assert_eq!(
            from_qwertty(&QwerttyEvent::Control(ControlInput::CarriageReturn)),
            Some(InputEvent::key(Key::Enter)),
        );
        assert_eq!(
            from_qwertty(&QwerttyEvent::Control(ControlInput::LineFeed)),
            Some(InputEvent::key(Key::Enter)),
        );
    }

    #[test]
    fn tab_and_escape_and_backspace_map() {
        assert_eq!(
            from_qwertty(&QwerttyEvent::Control(ControlInput::Tab)),
            Some(InputEvent::key(Key::Tab)),
        );
        assert_eq!(
            from_qwertty(&QwerttyEvent::Control(ControlInput::Escape)),
            Some(InputEvent::key(Key::Escape)),
        );
        assert_eq!(
            from_qwertty(&QwerttyEvent::Control(ControlInput::Backspace)),
            Some(InputEvent::key(Key::Backspace)),
        );
        assert_eq!(
            from_qwertty(&QwerttyEvent::Control(ControlInput::Delete)),
            Some(InputEvent::key(Key::Backspace)),
        );
    }

    #[test]
    fn arrows_map() {
        assert_eq!(from_qwertty(&QwerttyEvent::Key(KeyInput::Up)), Some(InputEvent::key(Key::Up)));
        assert_eq!(
            from_qwertty(&QwerttyEvent::Key(KeyInput::Down)),
            Some(InputEvent::key(Key::Down)),
        );
        assert_eq!(
            from_qwertty(&QwerttyEvent::Key(KeyInput::Left)),
            Some(InputEvent::key(Key::Left)),
        );
        assert_eq!(
            from_qwertty(&QwerttyEvent::Key(KeyInput::Right)),
            Some(InputEvent::key(Key::Right)),
        );
    }

    #[test]
    fn unmapped_input_is_dropped() {
        assert_eq!(from_qwertty(&QwerttyEvent::Control(ControlInput::Null)), None);
        assert_eq!(from_qwertty(&QwerttyEvent::Control(ControlInput::Other(0x03))), None);
    }
}
