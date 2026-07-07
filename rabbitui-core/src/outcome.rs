//! Typed outcomes: what a widget tells the app happened to it.
//!
//! Per `docs/adr/0001-programming-model.md` and
//! `docs/adr/0006-input-focus-events.md`: a widget handler consumes raw input
//! and, rather than mutating app state directly, *emits* a typed [`Outcome`].
//! The framework collects the frame's outcomes and hands them to the app's
//! `update` so the app owns every effect (ADR 0001's app-owned-effects rule).
//!
//! This is a **closed** enum in v1 — the small vocabulary the built-in widgets
//! need. ADR 0001's revisit trigger for the outcome grammar applies: if a
//! `Component`-style open outcome type keeps being demanded, reopen the widget
//! contract rather than growing this enum without bound.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::outcome::Outcome;
//!
//! let activated = Outcome::Activated;
//! assert!(matches!(activated, Outcome::Activated));
//!
//! let changed = Outcome::Changed("hello".to_string());
//! assert!(matches!(changed, Outcome::Changed(_)));
//! ```

/// What a widget reports to the app after handling input.
///
/// A handler `emit`s one (or more) of these; the framework delivers them to the
/// app's `update` in the same call as the event that produced them, keyed by the
/// widget's [`WidgetId`](crate::id::WidgetId).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// The widget was activated (a button pressed, a menu item chosen).
    Activated,
    /// The widget's value changed to this new content (a text field edit).
    Changed(String),
    /// The widget's value was submitted (Enter in a single-line input).
    Submitted,
    /// The widget toggled to this on/off state (a checkbox).
    Toggled(bool),
    /// The widget's selection moved to this index (a list, a set of tabs).
    Selected(usize),
    /// The widget was dismissed (a modal closed, a prompt cancelled).
    Dismissed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_compare_by_value() {
        assert_eq!(Outcome::Activated, Outcome::Activated);
        assert_eq!(Outcome::Toggled(true), Outcome::Toggled(true));
        assert_ne!(Outcome::Toggled(true), Outcome::Toggled(false));
        assert_eq!(Outcome::Changed("a".into()), Outcome::Changed("a".into()));
        assert_ne!(Outcome::Changed("a".into()), Outcome::Changed("b".into()));
    }
}
