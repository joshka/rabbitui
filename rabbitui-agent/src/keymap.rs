//! The in-app declarative keymap: the ONE table that drives both dispatch and
//! the generated help overlay.
//!
//! This is the minimal, in-app version of Arc 4's keymap generalization (kept
//! out of the framework for now, per `docs/plans/arc3-agent-client.md`). An
//! [`Action`] names a thing the app can do; a [`Chord`] names a key press; the
//! [`Keymap`] maps chords to actions and, read the other way, actions to their
//! chords for the help overlay.
//!
//! # Standing invariants (carried from the plan)
//!
//! - **App actions are Ctrl-chords only** while the composer is focused: lone
//!   `Esc` is unreliable on the current substrate, and printable keys belong to
//!   the composer. `Esc` is bound only where it already works — dismissing the
//!   confirm modal and the help overlay — never as a base app action.
//! - **Printable chords stay `consumed()`-guarded** at the dispatch site: a
//!   binding on a bare letter (there are none in the base map today, but the
//!   modal's `y`/`n` are printable) must only fire on a key no focused widget
//!   consumed, so typing into the composer is never stolen.
//!
//! # Substrate note (help chord)
//!
//! The help overlay's decided chord is **`Ctrl-/`** (the plan's choice). On the
//! current qwertty substrate a raw `Ctrl-/` is C0 byte `0x1F`, which is outside
//! the `0x01..=0x1A` Ctrl-letter range the decoder surfaces, so it does **not**
//! decode today (`rabbitui/src/input.rs`). `Ctrl-H` is no better: it is byte
//! `0x08`, folded into `Backspace`. So both plan candidates are dead on this
//! substrate. To keep the overlay live-verifiable now, [`Action::Help`] also
//! carries a working alias, **`Ctrl-G`** (`0x07`, a free decodable Ctrl-letter).
//! Both are listed in the help table; `Ctrl-/` lights up on the substrate
//! migration, `Ctrl-G` works today.

use rabbitui_core::input::{Key, KeyEvent};

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

    /// Whether this action belongs to an overlay (modal / help) rather than the
    /// base composer view. The base help overlay lists only base actions so it
    /// stays a stable reference card; modal affordances are shown in the modal.
    #[must_use]
    pub fn is_base(self) -> bool {
        !matches!(self, Action::Allow | Action::Deny)
    }
}

/// A single key chord: a [`Key`] plus whether Ctrl is held. Alt/Shift are not
/// modelled — the app binds only Ctrl-chords and a couple of bare keys, which is
/// all the substrate decodes reliably.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    /// The key.
    pub key: Key,
    /// Whether the Ctrl modifier is held.
    pub ctrl: bool,
}

impl Chord {
    /// A Ctrl-letter chord (e.g. `Chord::ctrl('t')` for Ctrl-T).
    #[must_use]
    pub const fn ctrl(letter: char) -> Self {
        Self {
            key: Key::Char(letter),
            ctrl: true,
        }
    }

    /// A bare (no-modifier) key chord.
    #[must_use]
    pub const fn bare(key: Key) -> Self {
        Self { key, ctrl: false }
    }

    /// Whether a decoded key press matches this chord. Only Ctrl is significant;
    /// a chord that wants Ctrl must have it, and a bare chord must not.
    #[must_use]
    pub fn matches(self, press: &KeyEvent) -> bool {
        press.key == self.key && press.modifiers.ctrl == self.ctrl
    }

    /// The chord rendered for the help overlay's left column (e.g. `Ctrl-T`,
    /// `Esc`, `Enter`, `y`).
    #[must_use]
    pub fn display(self) -> String {
        let name = match self.key {
            Key::Char('/') => "/".to_string(),
            Key::Char(ch) => ch.to_uppercase().to_string(),
            Key::Enter => "Enter".to_string(),
            Key::Escape => "Esc".to_string(),
            Key::Backspace => "Backspace".to_string(),
            Key::Tab => "Tab".to_string(),
            other => format!("{other:?}"),
        };
        if self.ctrl {
            format!("Ctrl-{name}")
        } else {
            name
        }
    }
}

/// One row of the keymap: an action and every chord bound to it. The order of
/// this table is the order the help overlay lists rows in, so it is authored
/// reading top-to-bottom as a reference card.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    /// The action this row triggers.
    pub action: Action,
    /// The chords bound to it (most rows have one; Help has two — see the module
    /// note on the substrate).
    pub chords: &'static [Chord],
}

/// The app's whole key surface, as a flat table. Both dispatch and the help
/// overlay read this — change a binding here and both follow.
#[derive(Debug, Clone, Copy)]
pub struct Keymap {
    /// The bindings, in help-display order.
    pub bindings: &'static [Binding],
}

/// The Ctrl-/ + Ctrl-G help chords (see the module note): the first is the
/// decided binding, the second the works-today alias.
const HELP_CHORDS: &[Chord] = &[Chord::ctrl('/'), Chord::ctrl('g')];
const TOGGLE_CHORDS: &[Chord] = &[Chord::ctrl('t')];
const CANCEL_CHORDS: &[Chord] = &[Chord::ctrl('x')];
const QUIT_CHORDS: &[Chord] = &[Chord::ctrl('c')];
const SEND_CHORDS: &[Chord] = &[Chord::bare(Key::Enter)];
const ALLOW_CHORDS: &[Chord] = &[Chord::bare(Key::Char('y'))];
const DENY_CHORDS: &[Chord] = &[Chord::bare(Key::Char('n')), Chord::bare(Key::Escape)];
const DISMISS_CHORDS: &[Chord] = &[Chord::bare(Key::Escape)];

/// The one base binding table (help-display order). Modal-only affordances
/// (`Allow`/`Deny`/`Dismiss`) are included so the modal can source its labels
/// from the same place, but [`Action::is_base`] filters them out of the base
/// help card.
const BINDINGS: &[Binding] = &[
    Binding {
        action: Action::Send,
        chords: SEND_CHORDS,
    },
    Binding {
        action: Action::ToggleMode,
        chords: TOGGLE_CHORDS,
    },
    Binding {
        action: Action::Cancel,
        chords: CANCEL_CHORDS,
    },
    Binding {
        action: Action::Help,
        chords: HELP_CHORDS,
    },
    Binding {
        action: Action::Quit,
        chords: QUIT_CHORDS,
    },
    Binding {
        action: Action::Allow,
        chords: ALLOW_CHORDS,
    },
    Binding {
        action: Action::Deny,
        chords: DENY_CHORDS,
    },
    Binding {
        action: Action::Dismiss,
        chords: DISMISS_CHORDS,
    },
];

impl Keymap {
    /// The app's keymap.
    #[must_use]
    pub const fn app() -> Self {
        Self { bindings: BINDINGS }
    }

    /// The action a decoded key press dispatches, or `None` if unbound. Scans
    /// the table in order; the base app never binds two actions to one chord, so
    /// first-match is unambiguous.
    #[must_use]
    pub fn action_for(&self, press: &KeyEvent) -> Option<Action> {
        self.bindings
            .iter()
            .find(|binding| binding.chords.iter().any(|chord| chord.matches(press)))
            .map(|binding| binding.action)
    }

    /// The chords bound to `action`, for rendering an affordance or the help row.
    #[must_use]
    pub fn chords_for(&self, action: Action) -> &'static [Chord] {
        self.bindings
            .iter()
            .find(|binding| binding.action == action)
            .map_or(&[], |binding| binding.chords)
    }

    /// The base bindings, in display order, as `(chord-column, action-label)`
    /// rows for the help overlay. Modal-only actions are excluded so the overlay
    /// stays a stable reference card.
    #[must_use]
    pub fn help_rows(&self) -> Vec<(String, &'static str)> {
        self.bindings
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use rabbitui_core::input::KeyEvent;

    fn ctrl(letter: char) -> KeyEvent {
        KeyEvent::new(Key::Char(letter)).ctrl()
    }

    #[test]
    fn ctrl_t_dispatches_toggle_mode() {
        assert_eq!(
            Keymap::app().action_for(&ctrl('t')),
            Some(Action::ToggleMode)
        );
    }

    #[test]
    fn ctrl_x_dispatches_cancel() {
        assert_eq!(Keymap::app().action_for(&ctrl('x')), Some(Action::Cancel));
    }

    #[test]
    fn ctrl_c_dispatches_quit() {
        assert_eq!(Keymap::app().action_for(&ctrl('c')), Some(Action::Quit));
    }

    #[test]
    fn both_help_chords_dispatch_help() {
        // The decided Ctrl-/ (future substrate) and the works-today Ctrl-G alias.
        assert_eq!(Keymap::app().action_for(&ctrl('/')), Some(Action::Help));
        assert_eq!(Keymap::app().action_for(&ctrl('g')), Some(Action::Help));
    }

    #[test]
    fn a_bare_letter_is_not_a_base_action() {
        // A printable key with no modifier must not dispatch a base action — it
        // belongs to the composer. (Bare y/n/Esc are modal-only, consumed-guarded
        // at the dispatch site.)
        let bare_t = KeyEvent::new(Key::Char('t'));
        assert_eq!(Keymap::app().action_for(&bare_t), None);
    }

    #[test]
    fn help_rows_are_generated_from_the_table_and_exclude_modal_actions() {
        let rows = Keymap::app().help_rows();
        // Every base action appears; Allow/Deny do not.
        let labels: Vec<&str> = rows.iter().map(|(_, label)| *label).collect();
        assert!(labels.contains(&Action::ToggleMode.label()));
        assert!(labels.contains(&Action::Help.label()));
        assert!(labels.contains(&Action::Quit.label()));
        assert!(!labels.contains(&Action::Allow.label()));
        assert!(!labels.contains(&Action::Deny.label()));
    }

    #[test]
    fn help_row_shows_both_help_chords() {
        let rows = Keymap::app().help_rows();
        let help = rows
            .iter()
            .find(|(_, label)| *label == Action::Help.label())
            .expect("a help row");
        assert!(help.0.contains("Ctrl-/"), "shows the decided chord: {}", help.0);
        assert!(help.0.contains("Ctrl-G"), "shows the works-today alias: {}", help.0);
    }

    #[test]
    fn chord_display_names_are_readable() {
        assert_eq!(Chord::ctrl('t').display(), "Ctrl-T");
        assert_eq!(Chord::ctrl('/').display(), "Ctrl-/");
        assert_eq!(Chord::bare(Key::Enter).display(), "Enter");
        assert_eq!(Chord::bare(Key::Escape).display(), "Esc");
        assert_eq!(Chord::bare(Key::Char('y')).display(), "Y");
    }
}
