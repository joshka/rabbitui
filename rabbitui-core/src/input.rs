//! The substrate-free input vocabulary.
//!
//! Per `docs/adr/0006-input-focus-events.md`, rabbitui-core is substrate-free:
//! it defines its own key/modifier types rather than depending on qwertty's
//! decoded events. The facade (`rabbitui`) maps qwertty's `InputEvent` into
//! these, owning all Escape/CSI interpretation; core routes events expressed in
//! this vocabulary through frame facts to widget handlers.
//!
//! Key input landed in slice 3; **mouse** input lands in slice 7 (ADR 0006 §5).
//! Paste and focus events arrive in later slices (§6–7); [`InputEvent`] is
//! `#[non_exhaustive]` so adding them is not a breaking change.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::input::{InputEvent, Key, KeyEvent, Modifiers};
//!
//! // A bare Enter key.
//! let enter = InputEvent::key(Key::Enter);
//! assert_eq!(enter, InputEvent::Key(KeyEvent::new(Key::Enter)));
//!
//! // Ctrl-C: a character key with a modifier set.
//! let ctrl_c = InputEvent::Key(KeyEvent::new(Key::Char('c')).ctrl());
//! assert!(ctrl_c.as_key().unwrap().modifiers.ctrl);
//! ```
//!
//! A mouse press carries its position, so the runtime can hit-test it against
//! the previous frame's facts (ADR 0006 §5):
//!
//! ```
//! use rabbitui_core::geometry::Position;
//! use rabbitui_core::input::{InputEvent, MouseButton, MouseEvent, MouseKind};
//!
//! let click = InputEvent::Mouse(MouseEvent::new(
//!     MouseKind::Down,
//!     MouseButton::Left,
//!     Position::new(4, 2),
//! ));
//! assert_eq!(click.as_mouse().unwrap().position, Position::new(4, 2));
//! assert!(click.as_key().is_none());
//! ```

use crate::geometry::Position;

/// One decoded input event routed through the frame.
///
/// Slice 3 carried only [`InputEvent::Key`]; slice 7 adds [`InputEvent::Mouse`].
/// Paste and focus events join later, which is why this enum is
/// `#[non_exhaustive]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InputEvent {
    /// A key press.
    Key(KeyEvent),
    /// A mouse press, release, drag, or wheel scroll.
    Mouse(MouseEvent),
}

impl InputEvent {
    /// A key event with no modifiers.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::input::{InputEvent, Key};
    ///
    /// let tab = InputEvent::key(Key::Tab);
    /// assert!(matches!(tab, InputEvent::Key(_)));
    /// ```
    #[must_use]
    pub const fn key(key: Key) -> Self {
        Self::Key(KeyEvent::new(key))
    }

    /// The key event, if this is a key press.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::input::{InputEvent, Key};
    ///
    /// assert_eq!(InputEvent::key(Key::Enter).as_key().unwrap().key, Key::Enter);
    /// ```
    #[must_use]
    pub const fn as_key(&self) -> Option<&KeyEvent> {
        match self {
            Self::Key(event) => Some(event),
            Self::Mouse(_) => None,
        }
    }

    /// The mouse event, if this is a mouse press, drag, or scroll.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::geometry::Position;
    /// use rabbitui_core::input::{InputEvent, MouseButton, MouseEvent, MouseKind};
    ///
    /// let event = InputEvent::Mouse(MouseEvent::new(
    ///     MouseKind::Down,
    ///     MouseButton::Left,
    ///     Position::ORIGIN,
    /// ));
    /// assert_eq!(event.as_mouse().unwrap().kind, MouseKind::Down);
    /// ```
    #[must_use]
    pub const fn as_mouse(&self) -> Option<&MouseEvent> {
        match self {
            Self::Mouse(event) => Some(event),
            Self::Key(_) => None,
        }
    }
}

/// A mouse press, release, drag, or wheel scroll at a cell position.
///
/// Per `docs/adr/0006-input-focus-events.md` §5, pointer events target the
/// topmost hit region under [`position`](Self::position) (a facts lookup against
/// the last rendered frame), then dispatch capture → target → bubble exactly
/// like keys. The facade maps qwertty's SGR mouse reports into these; core never
/// sees escape bytes.
///
/// # Examples
///
/// ```
/// use rabbitui_core::geometry::Position;
/// use rabbitui_core::input::{MouseButton, MouseEvent, MouseKind, Modifiers};
///
/// let drag = MouseEvent::new(MouseKind::Drag, MouseButton::Left, Position::new(2, 3))
///     .with_modifiers(Modifiers::NONE.with_shift());
/// assert!(drag.modifiers.shift);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseEvent {
    /// What the pointer did: press, release, drag, or scroll.
    pub kind: MouseKind,
    /// Which button was involved. For a wheel scroll the button is the
    /// terminal's reported wheel "button"; most handlers match on
    /// [`kind`](Self::kind) instead.
    pub button: MouseButton,
    /// The cell the pointer is over, in absolute buffer coordinates.
    pub position: Position,
    /// The modifiers held during the event.
    pub modifiers: Modifiers,
}

impl MouseEvent {
    /// A mouse event with no modifiers.
    #[must_use]
    pub const fn new(kind: MouseKind, button: MouseButton, position: Position) -> Self {
        Self { kind, button, position, modifiers: Modifiers::NONE }
    }

    /// This event with the given modifiers.
    #[must_use]
    pub const fn with_modifiers(mut self, modifiers: Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }
}

/// What a [`MouseEvent`] did.
///
/// [`MouseKind::Scroll`] carries a signed line count — positive scrolls the
/// content down (wheel toward the user), negative up — normalized to whole lines
/// (the per-terminal wheel/trackpad normalization of ADR 0006 §6 is deferred;
/// v1 reports one line per notch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MouseKind {
    /// A button was pressed.
    Down,
    /// A button was released.
    Up,
    /// The pointer moved with a button held.
    Drag,
    /// The wheel scrolled by this many lines (positive down, negative up).
    Scroll(i8),
}

/// Which mouse button an event concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MouseButton {
    /// The primary (left) button.
    Left,
    /// The middle button (wheel click).
    Middle,
    /// The secondary (right) button.
    Right,
    /// No button (a bare move, or a wheel scroll the terminal did not attribute
    /// to a button).
    None,
}

/// A key press: which [`Key`], and which [`Modifiers`] were held.
///
/// # Examples
///
/// ```
/// use rabbitui_core::input::{Key, KeyEvent};
///
/// let shift_tab = KeyEvent::new(Key::BackTab);
/// assert_eq!(shift_tab.key, Key::BackTab);
/// assert!(shift_tab.modifiers.is_empty());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    /// The key that was pressed.
    pub key: Key,
    /// The modifiers held during the press.
    pub modifiers: Modifiers,
}

impl KeyEvent {
    /// A key press with no modifiers.
    #[must_use]
    pub const fn new(key: Key) -> Self {
        Self { key, modifiers: Modifiers::NONE }
    }

    /// This key press with the given modifiers.
    #[must_use]
    pub const fn with_modifiers(mut self, modifiers: Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// This key press with the Ctrl modifier set.
    #[must_use]
    pub const fn ctrl(mut self) -> Self {
        self.modifiers.ctrl = true;
        self
    }

    /// This key press with the Alt modifier set.
    #[must_use]
    pub const fn alt(mut self) -> Self {
        self.modifiers.alt = true;
        self
    }

    /// This key press with the Shift modifier set.
    #[must_use]
    pub const fn shift(mut self) -> Self {
        self.modifiers.shift = true;
        self
    }
}

/// A key, independent of any terminal encoding.
///
/// The facade maps the terminal's raw bytes onto these variants; a widget
/// handler matches on the [`Key`], never on escape sequences. Keys the
/// substrate cannot yet decode simply never appear (see the facade's mapping
/// notes), so a widget's `match` on `Key` is exhaustive over what it can
/// receive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Key {
    /// A printable character.
    Char(char),
    /// The Enter / Return key.
    Enter,
    /// The Escape key.
    Escape,
    /// The Backspace key.
    Backspace,
    /// The Tab key (forward focus traversal by default).
    Tab,
    /// Shift-Tab (backward focus traversal by default).
    BackTab,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Delete (forward delete).
    Delete,
}

/// The modifier keys held during a key press.
///
/// # Examples
///
/// ```
/// use rabbitui_core::input::Modifiers;
///
/// let ctrl = Modifiers::NONE.with_ctrl();
/// assert!(ctrl.ctrl);
/// assert!(!ctrl.is_empty());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    /// The Control key was held.
    pub ctrl: bool,
    /// The Alt / Option key was held.
    pub alt: bool,
    /// The Shift key was held.
    pub shift: bool,
}

impl Modifiers {
    /// No modifiers held.
    pub const NONE: Self = Self { ctrl: false, alt: false, shift: false };

    /// Returns true if no modifier is held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !self.ctrl && !self.alt && !self.shift
    }

    /// This set with Ctrl added.
    #[must_use]
    pub const fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    /// This set with Alt added.
    #[must_use]
    pub const fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// This set with Shift added.
    #[must_use]
    pub const fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_helper_builds_unmodified_key_event() {
        assert_eq!(InputEvent::key(Key::Enter), InputEvent::Key(KeyEvent::new(Key::Enter)));
    }

    #[test]
    fn as_key_extracts_the_event() {
        let event = InputEvent::key(Key::Char('x'));
        assert_eq!(event.as_key().unwrap().key, Key::Char('x'));
    }

    #[test]
    fn modifier_builders_compose() {
        let mods = Modifiers::NONE.with_ctrl().with_shift();
        assert!(mods.ctrl);
        assert!(mods.shift);
        assert!(!mods.alt);
        assert!(!mods.is_empty());
    }

    #[test]
    fn key_event_builders_set_modifiers() {
        let event = KeyEvent::new(Key::Char('a')).ctrl().alt();
        assert!(event.modifiers.ctrl);
        assert!(event.modifiers.alt);
        assert!(!event.modifiers.shift);
    }

    #[test]
    fn empty_modifiers_is_default() {
        assert_eq!(Modifiers::default(), Modifiers::NONE);
        assert!(Modifiers::NONE.is_empty());
    }

    #[test]
    fn mouse_event_carries_position_and_kind() {
        let event = MouseEvent::new(MouseKind::Down, MouseButton::Left, Position::new(3, 4));
        let input = InputEvent::Mouse(event);
        assert_eq!(input.as_mouse().unwrap().position, Position::new(3, 4));
        assert_eq!(input.as_mouse().unwrap().kind, MouseKind::Down);
        // A mouse event is not a key event.
        assert!(input.as_key().is_none());
    }

    #[test]
    fn scroll_kind_carries_a_signed_line_count() {
        let up = MouseEvent::new(MouseKind::Scroll(-1), MouseButton::None, Position::ORIGIN);
        let down = MouseEvent::new(MouseKind::Scroll(2), MouseButton::None, Position::ORIGIN);
        assert_eq!(up.kind, MouseKind::Scroll(-1));
        assert_eq!(down.kind, MouseKind::Scroll(2));
    }

    #[test]
    fn mouse_modifiers_compose() {
        let event = MouseEvent::new(MouseKind::Drag, MouseButton::Left, Position::new(1, 1))
            .with_modifiers(Modifiers::NONE.with_ctrl());
        assert!(event.modifiers.ctrl);
        assert!(!event.modifiers.shift);
    }
}
