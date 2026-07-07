//! Between-frames widget mutations: commands to widgets and a deferred focus
//! request, applied by one shared function.
//!
//! Per the slice-6 design note (correction), the widget-command table and the
//! deferred focus request are **pure, runtime-free** — they carry no tokio and
//! touch no terminal — so they live in core, exactly like [`route`]. The runtime
//! (`rabbitui::app::run`) and the headless
//! [`TestApp`](../../rabbitui_testing/struct.TestApp.html) drain and
//! [`apply`](Pending::apply) the *same* [`Pending`] between frames, which makes
//! them identical by construction rather than by discipline (the ADR 0006 "the
//! harness and runtime cannot drift" requirement, extended to focus and widget
//! commands).
//!
//! # The two capabilities
//!
//! - **Widget commands.** The app commands a declared widget's retained state
//!   without owning its type: it hands over a monomorphized closure keyed by the
//!   widget's [`WidgetId`], and [`apply`](Pending::apply) downcasts the erased
//!   state through [`StateStore::get_dyn_mut`] and runs it **between frames**
//!   (after `update`, before the next view). Commanding a widget that was never
//!   declared (missing or foreign-typed state) is an app bug: the command is
//!   dropped with a `debug_assert`.
//! - **Deferred focus.** The app requests focus by path. The request is applied
//!   when the target is present-and-focusable in the *next* frame's facts, which
//!   covers **declare-then-focus** naturally (the widget may only appear in the
//!   frame the command triggers). If still absent after that frame, the request
//!   is dropped with a `debug_assert` naming the id — the ADR 0006 amendment's
//!   "reveal or fail loudly in debug" clause.
//!
//! # Examples
//!
//! ```
//! use std::any::Any;
//!
//! use rabbitui_core::buffer::Buffer;
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::geometry::Size;
//! use rabbitui_core::id::{WidgetId, key};
//! use rabbitui_core::pending::Pending;
//! use rabbitui_core::routing::Focus;
//! use rabbitui_core::store::StateStore;
//! use rabbitui_core::widget::{RenderCtx, Widget};
//!
//! #[derive(Default)]
//! struct Field {
//!     value: String,
//! }
//! struct Input;
//! impl Widget for Input {
//!     type State = Field;
//!     fn render(&self, state: &mut Field, ctx: &mut RenderCtx<'_>) {
//!         ctx.focusable(true);
//!         let _ = state;
//!     }
//! }
//!
//! // Declare the widget once so its state row exists and it is focusable.
//! let mut buffer = Buffer::new(Size::new(4, 1));
//! let mut store = StateStore::new();
//! store.begin_frame();
//! let mut frame = Frame::new(&mut buffer, &mut store);
//! frame.widget(key("input"), frame.area(), &Input);
//! let facts = frame.finish();
//! store.end_frame();
//! let id = WidgetId::ROOT.child(key("input"));
//! // Seed the retained value the way the runtime reaches it between frames —
//! // through `get_dyn_mut` (which does not read as a re-declaration).
//! let seed = store.get_dyn_mut(id).unwrap().downcast_mut::<Field>().unwrap();
//! seed.value = "old".into();
//!
//! // Command the widget and request focus, then apply between frames.
//! let mut pending = Pending::new();
//! pending.command::<Input>(id, |state| state.value = "new".into());
//! pending.request_focus(id);
//!
//! let mut focus = Focus::new();
//! pending.apply(&mut store, &facts, &mut focus);
//! let applied = store.get_dyn_mut(id).unwrap().downcast_ref::<Field>().unwrap();
//! assert_eq!(applied.value, "new");
//! assert_eq!(focus.current(), Some(id));
//! ```
//!
//! [`route`]: crate::routing::route

use std::any::Any;

use crate::facts::FrameFacts;
use crate::id::WidgetId;
use crate::routing::Focus;
use crate::store::StateStore;

/// A widget command: a type-erased mutation of one widget's retained state.
///
/// Boxed by [`Pending::command`], which monomorphizes the downcast to the
/// widget's `State` type so the closure the app wrote runs against the concrete
/// state even though this table is type-erased.
type WidgetCommand = Box<dyn FnOnce(&mut dyn Any)>;

/// The between-frames mutations an `update` requested: widget commands keyed by
/// identity, and at most one deferred focus request.
///
/// Buffered like commits (slice-5): the app records into a `Pending` during
/// `update`, and the loop drains and [`apply`](Self::apply)s it between frames.
/// Kept out of the facade so the runtime and the test harness share one
/// implementation.
///
/// The type is intentionally not `Debug`-transparent: the boxed closures are
/// opaque, so the derived `Debug` reports counts, not contents.
#[derive(Default)]
pub struct Pending {
    /// Widget commands in call order, each keyed by the target widget's id.
    commands: Vec<(WidgetId, WidgetCommand)>,
    /// The last focus request made this update (later calls win), applied
    /// against the next frame's facts.
    focus: Option<WidgetId>,
}

impl std::fmt::Debug for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pending")
            .field("commands", &self.commands.len())
            .field("focus", &self.focus)
            .finish()
    }
}

impl Pending {
    /// Creates an empty pending set.
    #[must_use]
    pub fn new() -> Self {
        Self { commands: Vec::new(), focus: None }
    }

    /// True if no widget command and no focus request are buffered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.focus.is_none()
    }

    /// Records a command against the widget at `id`, monomorphized for `W`.
    ///
    /// `f` is the mutation the app wrote against the concrete `W::State`; this
    /// wraps it in a closure that downcasts the erased state before calling it.
    /// The command runs when [`apply`](Self::apply) reaches this id and finds a
    /// state row of the right type; a missing or foreign-typed row means the
    /// widget was never declared (an app bug) and the command is dropped with a
    /// `debug_assert`.
    pub fn command<W>(&mut self, id: WidgetId, f: impl FnOnce(&mut W::State) + 'static)
    where
        W: crate::widget::Widget,
    {
        let erased: WidgetCommand = Box::new(move |any: &mut dyn Any| {
            // A well-formed frame stored `W::State` at this id; the runtime only
            // reaches this closure after confirming the row exists, and `apply`
            // checks the downcast so a foreign type is a dropped command, not a
            // panic.
            if let Some(state) = any.downcast_mut::<W::State>() {
                f(state);
            } else {
                debug_assert!(
                    false,
                    "widget command for {id:?}: state is not the commanded widget's type \
                     (the key path was declared with a different widget)"
                );
            }
        });
        self.commands.push((id, erased));
    }

    /// Requests focus move to `id`, applied against the *next* frame's facts.
    ///
    /// Later calls overwrite earlier ones (one focus verdict per update). See
    /// [`apply`](Self::apply) for the reveal-or-fail semantics.
    pub fn request_focus(&mut self, id: WidgetId) {
        self.focus = Some(id);
    }

    /// Merges `other` into this set: appends its commands after this set's (call
    /// order preserved) and takes its focus request if it made one (later wins).
    ///
    /// This is the **unapplied-remainder** primitive (slice-7 carry-forward): a
    /// runtime that could not fully apply a pending set against this frame's facts
    /// — a focus request whose target only appears in the frame the request
    /// *triggers* — carries the remainder forward and `extend`s it onto the next
    /// update's pending set, so declare-then-focus is retried once against the
    /// next frame's facts before the `debug_assert` fires.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::id::{WidgetId, key};
    /// use rabbitui_core::pending::Pending;
    ///
    /// let a = WidgetId::ROOT.child(key("a"));
    /// let b = WidgetId::ROOT.child(key("b"));
    /// let mut first = Pending::new();
    /// first.request_focus(a);
    /// let mut second = Pending::new();
    /// second.request_focus(b);
    /// first.extend(second);
    /// // The later focus request wins.
    /// assert_eq!(first.focus_request(), Some(b));
    /// ```
    pub fn extend(&mut self, other: Pending) {
        self.commands.extend(other.commands);
        if other.focus.is_some() {
            self.focus = other.focus;
        }
    }

    /// The deferred focus request, if any (for a runtime that carries it across
    /// the reconcile boundary).
    #[must_use]
    pub fn focus_request(&self) -> Option<WidgetId> {
        self.focus
    }

    /// Applies every buffered widget command, then the focus request, against a
    /// freshly rendered frame's `facts`.
    ///
    /// Commands run first (a command may make its widget focusable, or set the
    /// value focus then reveals). Each command downcasts the id's state row: a
    /// missing row (never declared, or dropped) or a foreign type drops the
    /// command with a `debug_assert`. The focus request is honored only if the
    /// target is present-and-focusable in `facts`; otherwise it is dropped with a
    /// `debug_assert` naming the id (the ADR 0006 "reveal or fail loudly" clause).
    ///
    /// This is the direct, single-shot apply (the test harness's
    /// `apply_pending`). A runtime that wants the **one-frame retry** for
    /// declare-then-focus uses [`apply_deferred`](Self::apply_deferred) instead,
    /// which returns the unhonored focus request as a remainder rather than
    /// asserting on the first miss.
    ///
    /// This consumes the pending set (the commands are `FnOnce`).
    pub fn apply(self, store: &mut StateStore, facts: &FrameFacts, focus: &mut Focus) {
        let remainder = self.apply_deferred(store, facts, focus);
        if let Some(id) = remainder.focus {
            debug_assert!(
                false,
                "focus request for {id:?}: not present-and-focusable in the frame after \
                 the request (declare-then-focus failed — check the path)"
            );
        }
    }

    /// Applies every buffered widget command and attempts the focus request,
    /// returning the **unapplied remainder** — a [`Pending`] carrying only the
    /// focus request when it could not be honored this frame.
    ///
    /// This is the retry-aware apply (slice-7 carry-forward). Commands always run
    /// (and `debug_assert` on a missing state row, an unambiguous app bug). The
    /// focus request is honored when the target is present-and-focusable; when it
    /// is not, it is returned in the remainder **without** asserting, so a runtime
    /// can [`extend`](Self::extend) it onto the next frame's pending set and retry
    /// once — closing the declare-then-focus edge where the target only appears in
    /// the frame the request triggers. Only after that second miss does
    /// [`apply`](Self::apply) fire the `debug_assert`.
    ///
    /// This consumes the pending set (the commands are `FnOnce`).
    #[must_use]
    pub fn apply_deferred(
        self,
        store: &mut StateStore,
        facts: &FrameFacts,
        focus: &mut Focus,
    ) -> Pending {
        for (id, command) in self.commands {
            match store.get_dyn_mut(id) {
                Some(state) => command(state),
                None => debug_assert!(
                    false,
                    "widget command for {id:?}: no retained state (the widget was never \
                     declared, so it cannot be commanded)"
                ),
            }
        }

        let mut remainder = Pending::new();
        if let Some(id) = self.focus {
            if facts.get(id).is_some_and(|entry| entry.focusable) {
                focus.set(Some(id));
            } else {
                // Not honorable against this frame — carry it forward for one
                // retry rather than dropping or asserting now.
                remainder.focus = Some(id);
            }
        }
        remainder
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::frame::Frame;
    use crate::geometry::Size;
    use crate::id::key;
    use crate::widget::{RenderCtx, Widget};

    #[derive(Default)]
    struct Field {
        value: String,
    }

    struct Input;
    impl Widget for Input {
        type State = Field;
        fn render(&self, _state: &mut Field, ctx: &mut RenderCtx<'_>) {
            ctx.focusable(true);
        }
    }

    /// A non-focusable widget, to exercise the focus reveal-or-fail path.
    struct Label;
    impl Widget for Label {
        type State = ();
        fn render(&self, _state: &mut (), _ctx: &mut RenderCtx<'_>) {}
    }

    /// Declares `input` (focusable) and returns (facts, store) with its state row.
    fn declared() -> (FrameFacts, StateStore) {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.widget(key("input"), frame.area(), &Input);
        let facts = frame.finish();
        store.end_frame();
        (facts, store)
    }

    fn input_id() -> WidgetId {
        WidgetId::ROOT.child(key("input"))
    }

    #[test]
    fn command_mutates_declared_widget_state() {
        let (facts, mut store) = declared();
        // Seed and read the widget's state the way the runtime does between frames
        // — through `get_dyn_mut`, which does not touch `last_seen` (a between-frames
        // access must not read as a re-declaration).
        value_mut(&mut store).clear();
        value_mut(&mut store).push_str("old");

        let mut pending = Pending::new();
        pending.command::<Input>(input_id(), |state| state.value = "new".into());
        let mut focus = Focus::new();
        pending.apply(&mut store, &facts, &mut focus);

        assert_eq!(value(&mut store), "new");
    }

    #[test]
    fn commands_apply_in_call_order() {
        let (facts, mut store) = declared();
        let mut pending = Pending::new();
        pending.command::<Input>(input_id(), |state| state.value.push('a'));
        pending.command::<Input>(input_id(), |state| state.value.push('b'));
        let mut focus = Focus::new();
        pending.apply(&mut store, &facts, &mut focus);
        assert_eq!(value(&mut store), "ab");
    }

    /// The `input` widget's value, read between frames (via `get_dyn_mut`).
    fn value(store: &mut StateStore) -> String {
        value_mut(store).clone()
    }

    /// A between-frames `&mut` to the `input` widget's value.
    fn value_mut(store: &mut StateStore) -> &mut String {
        &mut store
            .get_dyn_mut(input_id())
            .and_then(|state| state.downcast_mut::<Field>())
            .expect("input state was declared")
            .value
    }

    #[test]
    fn focus_request_honored_when_present_and_focusable() {
        let (facts, mut store) = declared();
        let mut pending = Pending::new();
        pending.request_focus(input_id());
        let mut focus = Focus::new();
        pending.apply(&mut store, &facts, &mut focus);
        assert_eq!(focus.current(), Some(input_id()));
    }

    #[test]
    fn empty_pending_is_empty() {
        let pending = Pending::new();
        assert!(pending.is_empty());
    }

    #[test]
    fn last_focus_request_wins() {
        let mut pending = Pending::new();
        pending.request_focus(input_id());
        pending.request_focus(WidgetId::ROOT.child(key("other")));
        assert_eq!(pending.focus_request(), Some(WidgetId::ROOT.child(key("other"))));
    }

    #[test]
    fn extend_merges_commands_in_order_and_takes_later_focus() {
        let (facts, mut store) = declared();
        // First set: one command + a focus request.
        let mut first = Pending::new();
        first.command::<Input>(input_id(), |state| state.value.push('a'));
        first.request_focus(WidgetId::ROOT.child(key("stale")));
        // Second set: another command + a newer focus request.
        let mut second = Pending::new();
        second.command::<Input>(input_id(), |state| state.value.push('b'));
        second.request_focus(input_id());
        first.extend(second);

        // The later focus request won.
        assert_eq!(first.focus_request(), Some(input_id()));
        let mut focus = Focus::new();
        first.apply(&mut store, &facts, &mut focus);
        // Commands applied in call order across both sets.
        assert_eq!(value(&mut store), "ab");
        assert_eq!(focus.current(), Some(input_id()));
    }

    #[test]
    fn declare_then_focus_retries_against_the_next_frame() {
        // Frame 1: the target is NOT declared, so its focus request cannot be
        // honored. `apply_deferred` returns it as a remainder without asserting.
        let mut store = StateStore::new();
        let empty = FrameFacts::new();
        let mut pending = Pending::new();
        pending.request_focus(input_id());
        let mut focus = Focus::new();
        let remainder = pending.apply_deferred(&mut store, &empty, &mut focus);
        assert_eq!(remainder.focus_request(), Some(input_id()));
        assert!(focus.current().is_none(), "nothing focusable yet");

        // Frame 2: the widget is now declared. The carried-forward remainder,
        // extended onto the next (empty) update, applies cleanly.
        let (facts, mut store2) = declared();
        let mut next = Pending::new();
        next.extend(remainder);
        next.apply(&mut store2, &facts, &mut focus);
        assert_eq!(focus.current(), Some(input_id()), "the retry frame honored the focus");
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "no retained state")]
    fn commanding_undeclared_widget_panics_in_debug() {
        let (facts, mut store) = declared();
        let mut pending = Pending::new();
        pending.command::<Input>(WidgetId::ROOT.child(key("ghost")), |s| s.value = "x".into());
        let mut focus = Focus::new();
        pending.apply(&mut store, &facts, &mut focus);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "declare-then-focus failed")]
    fn focusing_non_focusable_panics_in_debug() {
        // A frame where the target is present but not focusable.
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.widget(key("label"), frame.area(), &Label);
        let facts = frame.finish();
        store.end_frame();

        let mut pending = Pending::new();
        pending.request_focus(WidgetId::ROOT.child(key("label")));
        let mut focus = Focus::new();
        pending.apply(&mut store, &facts, &mut focus);
    }
}
