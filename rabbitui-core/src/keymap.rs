//! A generic, declarative keymap: the ONE table that drives both key dispatch
//! and a generated help overlay.
//!
//! This is the framework generalization of the flagship's proven in-app keymap
//! (`rabbitui-agent/src/keymap.rs`), the decided position of Arc 4 §3
//! (`docs/plans/arc4-spine.md`). An app defines its own `Action` enum; a
//! [`Chord`] names a key press (a [`Key`] plus [`Modifiers`]); a [`Keymap<A>`]
//! maps chords to actions and, read the other way, actions to their chords for a
//! help overlay. There is exactly one source of truth for an app's key surface.
//!
//! # The two reads
//!
//! - **Dispatch** (chord → action): [`Keymap::action_for`] for the raw lookup,
//!   or [`Keymap::action_for_guarded`] to fold in the consumed-guard convention
//!   below in one call.
//! - **Help** (action → chords): [`Keymap::bindings`] iterates the table in
//!   authored order, and [`Keymap::help_rows`] renders `(chord-column, label)`
//!   rows for a [`HelpOverlay`](../../rabbitui_widgets/struct.HelpOverlay.html).
//!
//! # The consumed-guard convention, encoded
//!
//! The flagship dispatches app actions only on a key **no focused widget
//! consumed** (`!update.consumed()` at the call site), and binds app actions to
//! **Ctrl-chords** while a text input is focused — a bare printable key belongs
//! to the composer, never to a global binding. Two invariants fall out:
//!
//! 1. A **printable/bare chord** (a [`Key::Char`] with no Ctrl/Alt) must only
//!    fire on an *unconsumed* key, so typing into a focused input is never
//!    stolen.
//! 2. A **modified chord** (Ctrl or Alt held) cannot be typed into an input, so
//!    it may dispatch even against a focused widget — the flagship still guards
//!    it at the site out of caution, but the guard is a no-op for those.
//!
//! [`Keymap::action_for_guarded`] encodes this so an app calls it once instead
//! of re-deriving the guard at every dispatch site. It takes the decoded
//! [`KeyEvent`] and whether the route consumed it (the boolean the facade's
//! `Update::consumed()` — core's [`RouteResult::consumed`](crate::routing::RouteResult)
//! — surfaces) and returns the action only when the convention allows it. See
//! [`Chord::is_guarded`] for the per-chord rule.
//!
//! # Substrate note (lone Esc, help chord)
//!
//! Two facts carried from the flagship's plan, unchanged by generalization:
//! lone `Esc` is unreliable on the current substrate (bind it only where it
//! already works — dismissing an overlay — never as a base action), and the
//! plan's decided help chord `Ctrl-/` does not decode today. An app models both
//! by binding an action to *several* chords (e.g. a decided chord plus a
//! works-today alias); [`Keymap`] supports many chords per action for exactly
//! this.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::input::{Key, KeyEvent};
//! use rabbitui_core::keymap::{Binding, Chord, Keymap};
//!
//! // An app brings its own Action enum.
//! #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//! enum Action {
//!     Quit,
//!     Help,
//! }
//!
//! // One table, authored in help-display order. Ctrl-chords for app actions.
//! static QUIT: &[Chord] = &[Chord::ctrl('c')];
//! static HELP: &[Chord] = &[Chord::ctrl('/'), Chord::ctrl('g')];
//! static BINDINGS: &[Binding<Action>] = &[
//!     Binding::new(Action::Quit, QUIT),
//!     Binding::new(Action::Help, HELP),
//! ];
//! let keymap = Keymap::new(BINDINGS);
//!
//! // Dispatch: Ctrl-C → Quit.
//! let ctrl_c = KeyEvent::new(Key::Char('c')).ctrl();
//! assert_eq!(keymap.action_for(&ctrl_c), Some(Action::Quit));
//!
//! // A bare printable is never a global action.
//! assert_eq!(keymap.action_for(&KeyEvent::new(Key::Char('c'))), None);
//! ```

use crate::input::{Key, KeyEvent, Modifiers};

/// A single key chord: a [`Key`] plus the [`Modifiers`] held with it.
///
/// The flagship's in-app `Chord` modelled only `ctrl: bool` — all it could
/// decode. The framework chord reuses core [`Modifiers`] (Ctrl/Alt/Shift) so an
/// app can bind whatever its substrate decodes, while the ctrl-only convention
/// (see [`is_guarded`](Self::is_guarded)) still holds for the keys apps bind
/// today.
///
/// # Examples
///
/// ```
/// use rabbitui_core::input::{Key, KeyEvent};
/// use rabbitui_core::keymap::Chord;
///
/// let toggle = Chord::ctrl('t');
/// assert!(toggle.matches(&KeyEvent::new(Key::Char('t')).ctrl()));
/// // A bare 't' does not match a Ctrl-chord.
/// assert!(!toggle.matches(&KeyEvent::new(Key::Char('t'))));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    /// The key that must be pressed.
    pub key: Key,
    /// The modifiers that must be held (an exact match — see [`matches`](Self::matches)).
    pub modifiers: Modifiers,
}

impl Chord {
    /// A chord for `key` with no modifiers held (a bare key press).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::input::Key;
    /// use rabbitui_core::keymap::Chord;
    ///
    /// let enter = Chord::bare(Key::Enter);
    /// assert!(enter.modifiers.is_empty());
    /// ```
    #[must_use]
    pub const fn bare(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers::NONE,
        }
    }

    /// A chord for `key` with the given `modifiers`.
    #[must_use]
    pub const fn new(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }

    /// A Ctrl-letter chord (e.g. `Chord::ctrl('t')` for Ctrl-T).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::keymap::Chord;
    ///
    /// assert_eq!(Chord::ctrl('t').display(), "Ctrl-T");
    /// ```
    #[must_use]
    pub const fn ctrl(letter: char) -> Self {
        Self {
            key: Key::Char(letter),
            modifiers: Modifiers::NONE.with_ctrl(),
        }
    }

    /// Whether a decoded key press matches this chord: the same key and the exact
    /// same modifier set.
    ///
    /// The match is exact on modifiers — a chord that wants Ctrl only matches a
    /// press with Ctrl (and nothing else) held, and a bare chord only matches a
    /// press with no modifiers. That keeps `Ctrl-T` and a bare `T` distinct, the
    /// distinction the whole convention rests on.
    #[must_use]
    pub fn matches(self, press: &KeyEvent) -> bool {
        press.key == self.key && press.modifiers == self.modifiers
    }

    /// Whether this chord is subject to the consumed-guard: a **printable/bare**
    /// chord (a [`Key::Char`] with neither Ctrl nor Alt) is guarded — it must
    /// only fire on a key no focused widget consumed, since it could otherwise be
    /// typed into an input. A modified chord (Ctrl/Alt) is not guarded: it cannot
    /// be typed, so it is safe to dispatch even against a focused widget.
    ///
    /// This is the per-chord half of [`Keymap::action_for_guarded`]; see the
    /// module docs for the convention it encodes.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::input::Key;
    /// use rabbitui_core::keymap::Chord;
    ///
    /// // A bare 'y' is guarded (it is typeable).
    /// assert!(Chord::bare(Key::Char('y')).is_guarded());
    /// // A Ctrl-chord is not (it is not typeable into an input).
    /// assert!(!Chord::ctrl('t').is_guarded());
    /// // A bare Enter/Esc is not printable, so it is not guarded as text — but
    /// // apps bind those only where they already work (see the module docs).
    /// assert!(!Chord::bare(Key::Enter).is_guarded());
    /// ```
    #[must_use]
    pub fn is_guarded(self) -> bool {
        matches!(self.key, Key::Char(_)) && !self.modifiers.ctrl && !self.modifiers.alt
    }

    /// The chord rendered for a help overlay's chord column (e.g. `Ctrl-T`,
    /// `Esc`, `Enter`, `Y`).
    ///
    /// A single printable letter is upper-cased so it reads as a key cap; `/` is
    /// left as-is; named keys use a short label; a Ctrl/Alt/Shift prefix is
    /// prepended in that order.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::input::Key;
    /// use rabbitui_core::keymap::Chord;
    ///
    /// assert_eq!(Chord::ctrl('t').display(), "Ctrl-T");
    /// assert_eq!(Chord::ctrl('/').display(), "Ctrl-/");
    /// assert_eq!(Chord::bare(Key::Enter).display(), "Enter");
    /// assert_eq!(Chord::bare(Key::Char('y')).display(), "Y");
    /// ```
    #[must_use]
    pub fn display(self) -> String {
        let name = match self.key {
            Key::Char('/') => "/".to_string(),
            Key::Char(ch) => ch.to_uppercase().to_string(),
            Key::Enter => "Enter".to_string(),
            Key::Escape => "Esc".to_string(),
            Key::Backspace => "Backspace".to_string(),
            Key::Tab => "Tab".to_string(),
            Key::BackTab => "Shift-Tab".to_string(),
            other => format!("{other:?}"),
        };
        let mut out = String::new();
        if self.modifiers.ctrl {
            out.push_str("Ctrl-");
        }
        if self.modifiers.alt {
            out.push_str("Alt-");
        }
        // Shift is folded into named keys where it matters (BackTab), so only
        // surface it for an explicit Shift on some other key.
        if self.modifiers.shift && !matches!(self.key, Key::BackTab) {
            out.push_str("Shift-");
        }
        out.push_str(&name);
        out
    }
}

/// One row of a [`Keymap`]: an action and every [`Chord`] bound to it.
///
/// The order of the bindings slice is the order a help overlay lists rows in, so
/// the table is authored reading top-to-bottom as a reference card. Most rows
/// have one chord; a row may carry several (a decided chord plus a works-today
/// alias — see the module's substrate note).
///
/// `A` is the app's action type. It is stored by value and returned by
/// [`Keymap::action_for`], so a `Copy` enum is the natural fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding<A> {
    /// The action this row triggers.
    pub action: A,
    /// The chords bound to it, in display order.
    pub chords: &'static [Chord],
}

impl<A> Binding<A> {
    /// A binding of `action` to `chords`.
    ///
    /// `const` so a whole binding table can be a `static`.
    #[must_use]
    pub const fn new(action: A, chords: &'static [Chord]) -> Self {
        Self { action, chords }
    }
}

/// An app's whole key surface, as a flat borrowed table over its action type
/// `A`. Both dispatch and a help overlay read this one table — change a binding
/// and both follow.
///
/// The table is borrowed (`&'a [Binding<A>]`), so it is typically a `static`
/// authored once and wrapped in a `const` constructor, exactly like the
/// flagship's `Keymap::app()`.
///
/// # Examples
///
/// ```
/// use rabbitui_core::input::{Key, KeyEvent};
/// use rabbitui_core::keymap::{Binding, Chord, Keymap};
///
/// #[derive(Clone, Copy, PartialEq, Eq, Debug)]
/// enum Action { Quit }
///
/// static QUIT: &[Chord] = &[Chord::ctrl('c')];
/// static BINDINGS: &[Binding<Action>] = &[Binding::new(Action::Quit, QUIT)];
/// let keymap = Keymap::new(BINDINGS);
///
/// assert_eq!(
///     keymap.action_for(&KeyEvent::new(Key::Char('c')).ctrl()),
///     Some(Action::Quit),
/// );
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Keymap<'a, A> {
    bindings: &'a [Binding<A>],
}

impl<'a, A> Keymap<'a, A> {
    /// A keymap over `bindings`, in help-display order.
    ///
    /// `const` so an app can wrap a `static` table in a `const fn`.
    #[must_use]
    pub const fn new(bindings: &'a [Binding<A>]) -> Self {
        Self { bindings }
    }

    /// The bindings, in authored (help-display) order.
    ///
    /// This is the action → chords read: iterate it to build a help overlay or
    /// to render an affordance. Prefer [`help_rows`](Self::help_rows) when you
    /// just want display strings.
    #[must_use]
    pub const fn bindings(&self) -> &'a [Binding<A>] {
        self.bindings
    }
}

impl<A: Copy + PartialEq> Keymap<'_, A> {
    /// The action a decoded key press dispatches, or `None` if unbound.
    ///
    /// The **raw** lookup: it scans the table in order and returns the first
    /// action whose chords contain a match, ignoring the consumed-guard. Use it
    /// when you have already established the key is unconsumed, or in a context
    /// (a modal) that owns the key regardless. For the guarded convention in one
    /// call, use [`action_for_guarded`](Self::action_for_guarded).
    #[must_use]
    pub fn action_for(&self, press: &KeyEvent) -> Option<A> {
        self.bindings
            .iter()
            .find(|binding| binding.chords.iter().any(|chord| chord.matches(press)))
            .map(|binding| binding.action)
    }

    /// The action a key press dispatches under the **consumed-guard
    /// convention**, or `None`.
    ///
    /// `consumed` is whether the route already consumed this key (core's
    /// [`RouteResult::consumed`](crate::routing::RouteResult), which the facade
    /// surfaces as `Update::consumed()`). The rule, encoded once so an app need
    /// not re-derive it at every dispatch site:
    ///
    /// - A match on a **guarded** ([`Chord::is_guarded`]) printable/bare chord
    ///   dispatches only when `!consumed` — a focused input's key is never
    ///   re-interpreted as a global action.
    /// - A match on a **modified** (Ctrl/Alt) chord dispatches regardless of
    ///   `consumed`: it cannot be typed into an input, so nothing is stolen.
    ///
    /// This mirrors the flagship's `!update.consumed() && input.as_key()` site
    /// guard, but makes it a property of the binding rather than the caller.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::input::{Key, KeyEvent};
    /// use rabbitui_core::keymap::{Binding, Chord, Keymap};
    ///
    /// #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    /// enum Action { Quit, Allow }
    ///
    /// static QUIT: &[Chord] = &[Chord::ctrl('c')];
    /// static ALLOW: &[Chord] = &[Chord::bare(Key::Char('y'))];
    /// static BINDINGS: &[Binding<Action>] =
    ///     &[Binding::new(Action::Quit, QUIT), Binding::new(Action::Allow, ALLOW)];
    /// let keymap = Keymap::new(BINDINGS);
    ///
    /// let ctrl_c = KeyEvent::new(Key::Char('c')).ctrl();
    /// // A Ctrl-chord dispatches even if a widget consumed the key.
    /// assert_eq!(keymap.action_for_guarded(&ctrl_c, true), Some(Action::Quit));
    ///
    /// let y = KeyEvent::new(Key::Char('y'));
    /// // A bare 'y' dispatches only when unconsumed…
    /// assert_eq!(keymap.action_for_guarded(&y, false), Some(Action::Allow));
    /// // …and is suppressed when a focused widget took it.
    /// assert_eq!(keymap.action_for_guarded(&y, true), None);
    /// ```
    #[must_use]
    pub fn action_for_guarded(&self, press: &KeyEvent, consumed: bool) -> Option<A> {
        self.bindings.iter().find_map(|binding| {
            let matched = binding.chords.iter().find(|chord| chord.matches(press))?;
            if matched.is_guarded() && consumed {
                None
            } else {
                Some(binding.action)
            }
        })
    }

    /// The chords bound to `action`, for rendering an affordance or a help row.
    ///
    /// Returns an empty slice if the action is unbound.
    #[must_use]
    pub fn chords_for(&self, action: A) -> &'static [Chord] {
        self.bindings
            .iter()
            .find(|binding| binding.action == action)
            .map_or(&[], |binding| binding.chords)
    }
}

impl<A: Copy> Keymap<'_, A> {
    /// The bindings, in display order, as `(chord-column, action)` rows for a
    /// help overlay. `label` maps each action to its right-column string.
    ///
    /// The chord column joins an action's chords with ` / ` (so a `Ctrl-/`
    /// primary and a `Ctrl-G` alias read as `Ctrl-/ / Ctrl-G`). Filter the table
    /// first (e.g. exclude modal-only actions) by constructing a [`Keymap`] over
    /// a narrower slice, or filter the returned rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::keymap::{Binding, Chord, Keymap};
    ///
    /// #[derive(Clone, Copy, PartialEq, Eq)]
    /// enum Action { Quit }
    /// fn label(a: Action) -> &'static str {
    ///     match a { Action::Quit => "quit" }
    /// }
    ///
    /// static QUIT: &[Chord] = &[Chord::ctrl('c')];
    /// static BINDINGS: &[Binding<Action>] = &[Binding::new(Action::Quit, QUIT)];
    /// let rows = Keymap::new(BINDINGS).help_rows(label);
    /// assert_eq!(rows, vec![("Ctrl-C".to_string(), "quit")]);
    /// ```
    #[must_use]
    pub fn help_rows(
        &self,
        label: impl Fn(A) -> &'static str,
    ) -> Vec<(String, &'static str)> {
        self.bindings
            .iter()
            .map(|binding| {
                let chords = binding
                    .chords
                    .iter()
                    .map(|chord| chord.display())
                    .collect::<Vec<_>>()
                    .join(" / ");
                (chords, label(binding.action))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Key, KeyEvent};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Action {
        ToggleMode,
        Cancel,
        Quit,
        Help,
        Allow,
        Deny,
    }

    fn label(action: Action) -> &'static str {
        match action {
            Action::ToggleMode => "toggle mode",
            Action::Cancel => "cancel",
            Action::Quit => "quit",
            Action::Help => "help",
            Action::Allow => "allow",
            Action::Deny => "deny",
        }
    }

    static TOGGLE: &[Chord] = &[Chord::ctrl('t')];
    static CANCEL: &[Chord] = &[Chord::ctrl('x')];
    static QUIT: &[Chord] = &[Chord::ctrl('c')];
    static HELP: &[Chord] = &[Chord::ctrl('/'), Chord::ctrl('g')];
    static ALLOW: &[Chord] = &[Chord::bare(Key::Char('y'))];
    static DENY: &[Chord] = &[Chord::bare(Key::Char('n')), Chord::bare(Key::Escape)];

    static BINDINGS: &[Binding<Action>] = &[
        Binding::new(Action::ToggleMode, TOGGLE),
        Binding::new(Action::Cancel, CANCEL),
        Binding::new(Action::Quit, QUIT),
        Binding::new(Action::Help, HELP),
        Binding::new(Action::Allow, ALLOW),
        Binding::new(Action::Deny, DENY),
    ];

    fn keymap() -> Keymap<'static, Action> {
        Keymap::new(BINDINGS)
    }

    fn ctrl(letter: char) -> KeyEvent {
        KeyEvent::new(Key::Char(letter)).ctrl()
    }

    #[test]
    fn ctrl_chord_dispatches_its_action() {
        assert_eq!(keymap().action_for(&ctrl('t')), Some(Action::ToggleMode));
        assert_eq!(keymap().action_for(&ctrl('x')), Some(Action::Cancel));
        assert_eq!(keymap().action_for(&ctrl('c')), Some(Action::Quit));
    }

    #[test]
    fn multiple_chords_all_dispatch_the_same_action() {
        // The decided Ctrl-/ (future substrate) and the works-today Ctrl-G alias.
        assert_eq!(keymap().action_for(&ctrl('/')), Some(Action::Help));
        assert_eq!(keymap().action_for(&ctrl('g')), Some(Action::Help));
    }

    #[test]
    fn a_bare_printable_is_not_a_ctrl_action() {
        // A printable key with no modifier does not match a Ctrl-chord.
        let bare_t = KeyEvent::new(Key::Char('t'));
        assert_eq!(keymap().action_for(&bare_t), None);
    }

    #[test]
    fn exact_modifier_match_distinguishes_ctrl_from_bare() {
        // Ctrl-Y is unbound; only bare 'y' (Allow) is.
        assert_eq!(keymap().action_for(&ctrl('y')), None);
        assert_eq!(
            keymap().action_for(&KeyEvent::new(Key::Char('y'))),
            Some(Action::Allow)
        );
    }

    #[test]
    fn guarded_bare_chord_is_suppressed_when_consumed() {
        let y = KeyEvent::new(Key::Char('y'));
        // Unconsumed → fires; consumed (a focused input took it) → suppressed.
        assert_eq!(keymap().action_for_guarded(&y, false), Some(Action::Allow));
        assert_eq!(keymap().action_for_guarded(&y, true), None);
    }

    #[test]
    fn unguarded_ctrl_chord_fires_even_when_consumed() {
        // A Ctrl-chord cannot be typed into an input, so the guard is a no-op.
        assert_eq!(
            keymap().action_for_guarded(&ctrl('c'), true),
            Some(Action::Quit)
        );
        assert_eq!(
            keymap().action_for_guarded(&ctrl('c'), false),
            Some(Action::Quit)
        );
    }

    #[test]
    fn bare_esc_is_guarded_as_non_text_but_still_matches() {
        // Esc is not a printable char, so `is_guarded` is false — it dispatches
        // regardless of `consumed`. Apps bind it only where it already works.
        assert!(!Chord::bare(Key::Escape).is_guarded());
        let esc = KeyEvent::new(Key::Escape);
        assert_eq!(keymap().action_for_guarded(&esc, true), Some(Action::Deny));
    }

    #[test]
    fn chords_for_returns_the_bound_chords() {
        assert_eq!(keymap().chords_for(Action::Help), HELP);
        assert_eq!(keymap().chords_for(Action::ToggleMode), TOGGLE);
    }

    #[test]
    fn help_rows_are_generated_in_table_order() {
        let rows = keymap().help_rows(label);
        let labels: Vec<&str> = rows.iter().map(|(_, l)| *l).collect();
        assert_eq!(
            labels,
            vec!["toggle mode", "cancel", "quit", "help", "allow", "deny"]
        );
    }

    #[test]
    fn help_row_joins_multiple_chords() {
        let rows = keymap().help_rows(label);
        let help = rows.iter().find(|(_, l)| *l == "help").expect("a help row");
        assert_eq!(help.0, "Ctrl-/ / Ctrl-G");
    }

    #[test]
    fn help_rows_can_be_filtered_by_a_narrower_keymap() {
        // Exclude the modal-only actions by mapping over a narrower slice: build a
        // keymap over just the base bindings.
        static BASE: &[Binding<Action>] = &[
            Binding::new(Action::ToggleMode, TOGGLE),
            Binding::new(Action::Help, HELP),
            Binding::new(Action::Quit, QUIT),
        ];
        let rows = Keymap::new(BASE).help_rows(label);
        let labels: Vec<&str> = rows.iter().map(|(_, l)| *l).collect();
        assert!(labels.contains(&"toggle mode"));
        assert!(!labels.contains(&"allow"));
        assert!(!labels.contains(&"deny"));
    }

    #[test]
    fn chord_display_names_are_readable() {
        assert_eq!(Chord::ctrl('t').display(), "Ctrl-T");
        assert_eq!(Chord::ctrl('/').display(), "Ctrl-/");
        assert_eq!(Chord::bare(Key::Enter).display(), "Enter");
        assert_eq!(Chord::bare(Key::Escape).display(), "Esc");
        assert_eq!(Chord::bare(Key::Char('y')).display(), "Y");
        assert_eq!(Chord::bare(Key::BackTab).display(), "Shift-Tab");
    }

    #[test]
    fn keymap_is_const_constructible() {
        const KM: Keymap<'static, Action> = Keymap::new(BINDINGS);
        assert_eq!(KM.bindings().len(), 6);
    }
}
