//! The flagship's key surface: the ONE table that drives both dispatch and the
//! help overlay, built on the framework's [`rabbitui_core::keymap`].
//!
//! An [`Action`] names a thing the app can do; [`KEYMAP`] maps chords to actions
//! and, read the other way (via [`base_help_rows`]), actions to their chords for
//! the help overlay. This is the adopt-back of Arc 4's keymap generalization: the
//! generic `Chord`/`Binding`/`Keymap` and the help layout now live in the
//! framework; only the app-specific `Action` enum, its labels, and the binding
//! table stay here.
//!
//! # Standing invariants (carried from the plan, now enforced by the framework)
//!
//! - **App actions are Ctrl-chords only** while the composer is focused: lone
//!   `Esc` is unreliable on the current substrate, and printable keys belong to
//!   the composer. `Esc` is bound only where it already works — dismissing the
//!   confirm modal and the help overlay — never as a base app action.
//! - **Printable chords stay consumed-guarded**: `Update::action` applies the
//!   guard automatically (via `Chord::is_guarded`), so the modal's `y`/`n` fire
//!   only on a key no focused widget consumed.
//!
//! # Substrate note (help chord)
//!
//! The help overlay's decided chord is **`Ctrl-/`** (the plan's choice). On the
//! current qwertty substrate a raw `Ctrl-/` is C0 byte `0x1F`, outside the
//! `0x01..=0x1A` Ctrl-letter range the decoder surfaces, so it does **not** decode
//! today (`rabbitui/src/input.rs`); `Ctrl-H` (`0x08`) folds into `Backspace`. So
//! [`Action::Help`] also carries a working alias, **`Ctrl-G`** (`0x07`). Both are
//! listed; `Ctrl-/` lights up on the substrate migration, `Ctrl-G` works today.

use rabbitui_core::input::Key;
use rabbitui_core::keymap::{Binding, Chord, Keymap};

/// A thing the app can do in response to a key. Drives both dispatch (chord →
/// action) and the help overlay (action → chords), so there is exactly one
/// source of truth for the app's key surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Send the composer draft (owned by the composer's `Submitted` outcome, so
    /// it is documented here but dispatched by the `TextInput`, not the keymap).
    Send,
    /// Toggle inline ↔ alt-screen browse mode.
    ToggleMode,
    /// Cancel the in-flight streaming turn.
    Cancel,
    /// Open the generated help overlay.
    Help,
    /// Quit the app.
    Quit,
    /// Allow the pending tool call(s) (modal only).
    Allow,
    /// Deny the pending tool call(s) (modal only).
    Deny,
    /// Dismiss the top overlay (help or the confirm modal).
    Dismiss,
}

impl Action {
    /// A short human-readable label for the help overlay's right column.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Action::Send => "send message",
            Action::ToggleMode => "toggle inline / browse mode",
            Action::Cancel => "cancel the streaming turn",
            Action::Help => "toggle this help",
            Action::Quit => "quit",
            Action::Allow => "allow the tool call",
            Action::Deny => "deny the tool call",
            Action::Dismiss => "dismiss the overlay",
        }
    }

    /// Whether this action belongs to the base composer view rather than an
    /// overlay. The help overlay lists only base actions so it stays a stable
    /// reference card; modal affordances are shown in the modal itself.
    #[must_use]
    pub fn is_base(self) -> bool {
        !matches!(self, Action::Allow | Action::Deny)
    }
}

// The chords bound to each action. `Ctrl-/` + `Ctrl-G` for help (see the module
// note); a bare `Enter` for send; the modal's printable `y`/`n` and `Esc`.
const HELP_CHORDS: &[Chord] = &[Chord::ctrl('/'), Chord::ctrl('g')];
const TOGGLE_CHORDS: &[Chord] = &[Chord::ctrl('t')];
const CANCEL_CHORDS: &[Chord] = &[Chord::ctrl('x')];
const QUIT_CHORDS: &[Chord] = &[Chord::ctrl('c')];
const SEND_CHORDS: &[Chord] = &[Chord::bare(Key::Enter)];
const ALLOW_CHORDS: &[Chord] = &[Chord::bare(Key::Char('y'))];
const DENY_CHORDS: &[Chord] = &[Chord::bare(Key::Char('n')), Chord::bare(Key::Escape)];
const DISMISS_CHORDS: &[Chord] = &[Chord::bare(Key::Escape)];

/// The app's whole key surface, in help-display order. Modal-only affordances
/// (`Allow`/`Deny`/`Dismiss`) are included so the modal can dispatch and label
/// from the same table; [`base_help_rows`] filters them out of the base help card.
const BINDINGS: &[Binding<Action>] = &[
    Binding::new(Action::Send, SEND_CHORDS),
    Binding::new(Action::ToggleMode, TOGGLE_CHORDS),
    Binding::new(Action::Cancel, CANCEL_CHORDS),
    Binding::new(Action::Help, HELP_CHORDS),
    Binding::new(Action::Quit, QUIT_CHORDS),
    Binding::new(Action::Allow, ALLOW_CHORDS),
    Binding::new(Action::Deny, DENY_CHORDS),
    Binding::new(Action::Dismiss, DISMISS_CHORDS),
];

/// The app's keymap. Dispatch (`update.action(&KEYMAP)` / `KEYMAP.action_for`) and
/// the help overlay both read this one table — change a binding and both follow.
pub const KEYMAP: Keymap<'static, Action> = Keymap::new(BINDINGS);

/// The base help rows `(chord-column, label)` in table order, excluding modal-only
/// affordances — the content of the help overlay's reference card.
#[must_use]
pub fn base_help_rows() -> Vec<(String, &'static str)> {
    KEYMAP
        .bindings()
        .iter()
        .filter(|binding| binding.action.is_base())
        .map(|binding| {
            let chords = binding
                .chords
                .iter()
                .map(|chord| chord.display())
                .collect::<Vec<_>>()
                .join(" / ");
            (chords, binding.action.label())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rabbitui_core::input::KeyEvent;

    fn ctrl(letter: char) -> KeyEvent {
        KeyEvent::new(Key::Char(letter)).ctrl()
    }

    #[test]
    fn app_chords_dispatch_their_actions() {
        assert_eq!(KEYMAP.action_for(&ctrl('t')), Some(Action::ToggleMode));
        assert_eq!(KEYMAP.action_for(&ctrl('x')), Some(Action::Cancel));
        assert_eq!(KEYMAP.action_for(&ctrl('c')), Some(Action::Quit));
    }

    #[test]
    fn both_help_chords_dispatch_help() {
        // The decided Ctrl-/ (future substrate) and the works-today Ctrl-G alias.
        assert_eq!(KEYMAP.action_for(&ctrl('/')), Some(Action::Help));
        assert_eq!(KEYMAP.action_for(&ctrl('g')), Some(Action::Help));
    }

    #[test]
    fn a_bare_letter_is_not_a_base_action() {
        // A printable key with no modifier must not dispatch a base action — it
        // belongs to the composer. (Bare y/n/Esc are modal-only, guarded by
        // `Update::action` at the dispatch site.)
        assert_eq!(KEYMAP.action_for(&KeyEvent::new(Key::Char('t'))), None);
    }

    #[test]
    fn base_help_rows_exclude_modal_actions_and_keep_order() {
        let rows = base_help_rows();
        let labels: Vec<&str> = rows.iter().map(|(_, label)| *label).collect();
        assert!(labels.contains(&Action::ToggleMode.label()));
        assert!(labels.contains(&Action::Quit.label()));
        assert!(!labels.contains(&Action::Allow.label()));
        assert!(!labels.contains(&Action::Deny.label()));
        // Send is first (table order preserved).
        assert_eq!(rows.first().map(|(_, label)| *label), Some(Action::Send.label()));
    }

    #[test]
    fn help_row_shows_both_help_chords() {
        let rows = base_help_rows();
        let help = rows
            .iter()
            .find(|(_, label)| *label == Action::Help.label())
            .expect("a help row");
        assert!(help.0.contains("Ctrl-/"), "shows the decided chord: {}", help.0);
        assert!(help.0.contains("Ctrl-G"), "shows the works-today alias: {}", help.0);
    }
}
