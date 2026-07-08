//! Event routing: the one shared path the runtime and the test harness use.
//!
//! Per `docs/adr/0006-input-focus-events.md`, an input event routes against the
//! **previous** frame's facts through **capture → target → bubble**, widgets
//! consume events and emit outcomes, and unconsumed Tab/BackTab drive focus
//! traversal. This module is that logic, extracted into one function
//! ([`route`]) so the runtime (`rabbitui::app::run`) and the headless
//! [`TestApp`](../../rabbitui_testing/struct.TestApp.html) cannot drift — the
//! ADR's "extract routing so the harness and runtime cannot drift" requirement.
//!
//! Routing is deliberately substrate-free and runtime-free: it takes the frame's
//! facts and handlers, the framework's [`Focus`], the state store, and a core
//! [`InputEvent`], and returns the outcomes and whether the event was consumed.
//! It never paints, never awaits, and never touches a terminal.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::buffer::Buffer;
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::geometry::Size;
//! use rabbitui_core::id::{WidgetId, key};
//! use rabbitui_core::input::{InputEvent, Key};
//! use rabbitui_core::outcome::Outcome;
//! use rabbitui_core::routing::{Focus, route};
//! use rabbitui_core::store::StateStore;
//! use rabbitui_core::widget::{HandleCtx, Handled, RenderCtx, Widget};
//!
//! struct Button;
//! impl Widget for Button {
//!     type State = ();
//!     fn render(&self, _s: &mut (), ctx: &mut RenderCtx<'_>) {
//!         ctx.focusable(true);
//!     }
//!     fn handle(_s: &mut (), event: &InputEvent, ctx: &mut HandleCtx<'_>) -> Handled {
//!         if matches!(event.as_key().map(|k| k.key), Some(Key::Enter)) {
//!             ctx.emit(Outcome::Activated);
//!             return Handled::Yes;
//!         }
//!         Handled::No
//!     }
//! }
//!
//! let mut buffer = Buffer::new(Size::new(4, 1));
//! let mut store = StateStore::new();
//! store.begin_frame();
//! let mut frame = Frame::new(&mut buffer, &mut store);
//! frame.widget(key("btn"), frame.area(), &Button);
//! let (facts, handlers) = frame.into_parts();
//! store.end_frame();
//!
//! let id = WidgetId::ROOT.child(key("btn"));
//! let mut focus = Focus::new();
//! focus.set(Some(id));
//!
//! let result = route(&facts, &handlers, &mut focus, &mut store, &InputEvent::key(Key::Enter));
//! assert!(result.consumed);
//! assert_eq!(result.outcomes, vec![(id, Outcome::Activated)]);
//! ```

use crate::facts::FrameFacts;
use crate::frame::HandlerMap;
use crate::geometry::Rect;
use crate::id::WidgetId;
use crate::input::{InputEvent, Key, MouseKind};
use crate::outcome::Outcome;
use crate::store::StateStore;
use crate::widget::{HandleCtx, Handled, Phase};

/// The framework's focus state: which widget holds keyboard input.
///
/// Focus is cross-widget framework state (ADR 0006), so it lives here rather
/// than in the per-widget [`StateStore`]. The runtime and the test harness both
/// own one `Focus` across frames and pass it to [`route`], which advances it on
/// unconsumed Tab/BackTab and honors handler focus requests.
///
/// # Examples
///
/// ```
/// use rabbitui_core::id::{WidgetId, key};
/// use rabbitui_core::routing::Focus;
///
/// let mut focus = Focus::new();
/// assert!(focus.current().is_none());
/// let id = WidgetId::ROOT.child(key("a"));
/// focus.set(Some(id));
/// assert_eq!(focus.current(), Some(id));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Focus {
    current: Option<WidgetId>,
}

impl Focus {
    /// No widget focused.
    #[must_use]
    pub const fn new() -> Self {
        Self { current: None }
    }

    /// The currently focused widget, if any.
    #[must_use]
    pub const fn current(&self) -> Option<WidgetId> {
        self.current
    }

    /// Sets the focused widget directly.
    pub fn set(&mut self, id: Option<WidgetId>) {
        self.current = id;
    }

    /// Reconciles focus against a fresh frame's facts (ADR 0006).
    ///
    /// Focus survives re-declaration: if the focused id is still present and
    /// focusable, it is kept. If it vanished (or became non-focusable), focus
    /// moves to the next surviving focusable entry at or after its old
    /// declaration position, wrapping; if nothing is focusable, focus clears.
    /// Called by the runtime/harness after each render, before routing the next
    /// event, so traversal always runs against current facts.
    pub fn reconcile(&mut self, facts: &FrameFacts) {
        let order: Vec<WidgetId> = facts.focus_order().map(|entry| entry.id).collect();
        if order.is_empty() {
            self.current = None;
            return;
        }
        match self.current {
            Some(id) if order.contains(&id) => {}
            Some(_dead) => {
                // The focused id went absent. Facts carry no cross-frame
                // position, so the deterministic recovery is the first surviving
                // focusable entry in the new frame's declaration order.
                self.current = Some(order[0]);
            }
            // Nothing focused while focusables exist: focus the first one, the
            // universal toolkit default. Without this, an app's first
            // keystrokes silently fall through to `update` until the user
            // presses Tab — the todo/form/agent examples were unusable
            // (found by betamax tapes, 2026-07-07).
            None => {
                self.current = Some(order[0]);
            }
        }
    }
}

/// The result of routing one event.
///
/// `outcomes` are the typed outcomes handlers emitted, each paired with the
/// widget that emitted it (ADR 0001 delivers these to the app's `update` in the
/// same call as the event). `consumed` is true if any handler returned
/// [`Handled::Yes`] or a framework default (Tab traversal) claimed the event —
/// i.e. the app should *not* also see it as a raw input event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteResult {
    /// Outcomes emitted this event, keyed by the emitting widget.
    pub outcomes: Vec<(WidgetId, Outcome)>,
    /// Whether the event was consumed (by a handler or a framework default).
    pub consumed: bool,
}

/// Routes one input event through a frame's facts and handlers.
///
/// The single shared routing path (ADR 0006). Steps, in order:
///
/// 1. **Target.** Key events target `focus.current`. Mouse events target the
///    topmost hit region under the pointer (layer-aware [`FrameFacts::hit`]).
///    With no target only the framework defaults (step 4) can act.
/// 2. **Capture.** Walk `path_to(target)` root → target, calling each ancestor's
///    handler (excluding the target) with [`Phase::Capture`]; a [`Handled::Yes`]
///    stops routing.
/// 3. **Target + bubble.** Call the target's handler, then ancestors target →
///    root with [`Phase::Bubble`], stopping on [`Handled::Yes`].
/// 4. **Framework defaults.** An unconsumed [`Key::Tab`]/[`Key::BackTab`]
///    advances focus through `focus_order()`, wrapping, and consumes the event.
///    A **mouse Down** on a focusable target focuses it (consumed or not)
///    (click-to-focus, the universal expectation), but does *not* consume the
///    event — it falls through to the app's `update` too.
/// 5. Anything still unconsumed is left for the app (its `update` sees the raw
///    event); `consumed` is `false`.
///
/// A handler may also request focus; an honored request moves `focus.current` to
/// the requesting widget if it is focusable in `facts`.
pub fn route(
    facts: &FrameFacts,
    handlers: &HandlerMap,
    focus: &mut Focus,
    store: &mut StateStore,
    event: &InputEvent,
) -> RouteResult {
    let mut dispatcher = Dispatcher {
        facts,
        handlers,
        store,
        event,
    };
    let mut result = RouteResult::default();

    // Step 1: target selection. Key events target the focused widget; mouse
    // events hit-test the pointer position against the facts (layer-aware).
    let target = match event {
        InputEvent::Mouse(mouse) => facts.hit(mouse.position).map(|entry| entry.id),
        _ => focus.current.filter(|id| facts.get(*id).is_some()),
    };

    // Click-to-focus runs BEFORE dispatch (Textual's rule): a left Down on a
    // focusable target moves focus whether or not a handler consumes the
    // click, so activation happens as the focused widget and the next Tab
    // starts from where the user last clicked. It never consumes the event.
    if let InputEvent::Mouse(mouse) = event
        && matches!(mouse.kind, MouseKind::Down)
        && let Some(entry) = target.and_then(|id| facts.get(id))
        && entry.focusable
    {
        focus.current = Some(entry.id);
    }

    if let Some(target) = target {
        let path = facts.path_to(target); // root → target, inclusive.

        // Step 2: capture, ancestors only (everything before the target).
        let ancestors = path.split_last().map_or(&path[..], |(_, rest)| rest);
        for &id in ancestors {
            if dispatcher.call(id, Phase::Capture, focus, &mut result) {
                result.consumed = true;
                return result;
            }
        }

        // Step 3: target, then bubble up through ancestors target → root.
        if dispatcher.call(target, Phase::Bubble, focus, &mut result) {
            result.consumed = true;
            return result;
        }
        for &id in ancestors.iter().rev() {
            if dispatcher.call(id, Phase::Bubble, focus, &mut result) {
                result.consumed = true;
                return result;
            }
        }
    }

    // Step 4: framework defaults — unconsumed Tab/BackTab drives traversal.
    if let Some(key) = event.as_key() {
        match key.key {
            Key::Tab => {
                advance_focus(facts, focus, Direction::Forward);
                result.consumed = true;
                return result;
            }
            Key::BackTab => {
                advance_focus(facts, focus, Direction::Backward);
                result.consumed = true;
                return result;
            }
            _ => {}
        }
    }

    // Step 5: unconsumed — the app's `update` will see the raw event.
    result
}

/// The immutable + store borrow needed to invoke handlers for one event.
///
/// Bundling these keeps [`route`]'s per-widget dispatch a two-argument method
/// call ([`Dispatcher::call`]) instead of threading facts, handlers, store, and
/// event through every hop of the capture/bubble walk.
struct Dispatcher<'a> {
    facts: &'a FrameFacts,
    handlers: &'a HandlerMap,
    store: &'a mut StateStore,
    event: &'a InputEvent,
}

impl Dispatcher<'_> {
    /// Invokes one widget's handler for `id` in `phase`, folding emitted
    /// outcomes and any focus request into `result`/`focus`. Returns true if the
    /// handler consumed the event ([`Handled::Yes`]).
    fn call(
        &mut self,
        id: WidgetId,
        phase: Phase,
        focus: &mut Focus,
        result: &mut RouteResult,
    ) -> bool {
        let Some(handler) = self.handlers.get(&id) else {
            return false;
        };
        let area = self
            .facts
            .get(id)
            .map_or(Rect::default(), |entry| entry.area);
        // A handler with no retained state row cannot run; treat as not-handled.
        let Some(state) = self.store.get_dyn_mut(id) else {
            return false;
        };

        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let handled = {
            let mut ctx = HandleCtx::new(phase, area, &mut outcomes, &mut request_focus);
            handler(state, self.event, &mut ctx)
        };

        for outcome in outcomes {
            result.outcomes.push((id, outcome));
        }
        if request_focus && self.facts.get(id).is_some_and(|entry| entry.focusable) {
            focus.current = Some(id);
        }

        matches!(handled, Handled::Yes)
    }
}

/// Traversal direction for Tab / BackTab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Forward,
    Backward,
}

/// Advances `focus.current` through the focusable entries, wrapping.
///
/// With no current focus, forward focuses the first entry and backward the last.
/// If the current focus is absent from the order (a race the runtime normally
/// prevents by reconciling first), traversal restarts from the appropriate end.
fn advance_focus(facts: &FrameFacts, focus: &mut Focus, direction: Direction) {
    let order: Vec<WidgetId> = facts.focus_order().map(|entry| entry.id).collect();
    if order.is_empty() {
        focus.current = None;
        return;
    }
    let next = match focus
        .current
        .and_then(|id| order.iter().position(|&o| o == id))
    {
        Some(index) => match direction {
            Direction::Forward => (index + 1) % order.len(),
            Direction::Backward => (index + order.len() - 1) % order.len(),
        },
        None => match direction {
            Direction::Forward => 0,
            Direction::Backward => order.len() - 1,
        },
    };
    focus.current = Some(order[next]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::frame::Frame;
    use crate::geometry::Size;
    use crate::id::key;
    use crate::widget::{RenderCtx, Widget};

    /// A focusable widget that activates on Enter or a left-button press.
    struct Button;
    impl Widget for Button {
        type State = ();
        fn render(&self, _s: &mut (), ctx: &mut RenderCtx<'_>) {
            ctx.focusable(true);
        }
        fn handle(_s: &mut (), event: &InputEvent, ctx: &mut HandleCtx<'_>) -> Handled {
            if matches!(event.as_key().map(|k| k.key), Some(Key::Enter)) {
                ctx.emit(Outcome::Activated);
                return Handled::Yes;
            }
            if let Some(mouse) = event.as_mouse()
                && mouse.kind == crate::input::MouseKind::Down
            {
                ctx.emit(Outcome::Activated);
                return Handled::Yes;
            }
            Handled::No
        }
    }

    /// Declares two focusable buttons and returns (facts, handlers, store, ids).
    #[allow(clippy::type_complexity)]
    fn two_buttons() -> (FrameFacts, HandlerMap, StateStore, WidgetId, WidgetId) {
        let mut buffer = Buffer::new(Size::new(4, 2));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        let [a_area, b_area] = frame.rows([
            crate::layout::Constraint::Length(1),
            crate::layout::Constraint::Length(1),
        ]);
        frame.widget(key("a"), a_area, &Button);
        frame.widget(key("b"), b_area, &Button);
        let (facts, handlers) = frame.into_parts();
        store.end_frame();
        let a = WidgetId::ROOT.child(key("a"));
        let b = WidgetId::ROOT.child(key("b"));
        (facts, handlers, store, a, b)
    }

    #[test]
    fn enter_on_focused_button_activates_and_consumes() {
        let (facts, handlers, mut store, a, _b) = two_buttons();
        let mut focus = Focus::new();
        focus.set(Some(a));
        let result = route(
            &facts,
            &handlers,
            &mut focus,
            &mut store,
            &InputEvent::key(Key::Enter),
        );
        assert!(result.consumed);
        assert_eq!(result.outcomes, vec![(a, Outcome::Activated)]);
    }

    #[test]
    fn tab_traverses_forward_and_wraps() {
        let (facts, handlers, mut store, a, b) = two_buttons();
        let mut focus = Focus::new();
        focus.set(Some(a));
        let r1 = route(
            &facts,
            &handlers,
            &mut focus,
            &mut store,
            &InputEvent::key(Key::Tab),
        );
        assert!(r1.consumed);
        assert_eq!(focus.current(), Some(b));
        let r2 = route(
            &facts,
            &handlers,
            &mut focus,
            &mut store,
            &InputEvent::key(Key::Tab),
        );
        assert!(r2.consumed);
        assert_eq!(focus.current(), Some(a));
    }

    #[test]
    fn backtab_traverses_backward_and_wraps() {
        let (facts, handlers, mut store, a, b) = two_buttons();
        let mut focus = Focus::new();
        focus.set(Some(a));
        route(
            &facts,
            &handlers,
            &mut focus,
            &mut store,
            &InputEvent::key(Key::BackTab),
        );
        assert_eq!(focus.current(), Some(b));
        route(
            &facts,
            &handlers,
            &mut focus,
            &mut store,
            &InputEvent::key(Key::BackTab),
        );
        assert_eq!(focus.current(), Some(a));
    }

    #[test]
    fn tab_with_no_focus_selects_first() {
        let (facts, handlers, mut store, a, _b) = two_buttons();
        let mut focus = Focus::new();
        route(
            &facts,
            &handlers,
            &mut focus,
            &mut store,
            &InputEvent::key(Key::Tab),
        );
        assert_eq!(focus.current(), Some(a));
    }

    #[test]
    fn unconsumed_event_is_not_consumed() {
        let (facts, handlers, mut store, a, _b) = two_buttons();
        let mut focus = Focus::new();
        focus.set(Some(a));
        // 'x' is not a binding of Button and not a framework default.
        let result = route(
            &facts,
            &handlers,
            &mut focus,
            &mut store,
            &InputEvent::key(Key::Char('x')),
        );
        assert!(!result.consumed);
        assert!(result.outcomes.is_empty());
    }

    #[test]
    fn reconcile_keeps_present_focus() {
        let (facts, _handlers, _store, a, _b) = two_buttons();
        let mut focus = Focus::new();
        focus.set(Some(a));
        focus.reconcile(&facts);
        assert_eq!(focus.current(), Some(a));
    }

    /// A container that swallows Escape on the capture phase, before it reaches
    /// its focused child — the classic capture-stops-routing case (ADR 0006).
    struct Trap;
    impl Widget for Trap {
        type State = ();
        fn render(&self, _s: &mut (), _ctx: &mut RenderCtx<'_>) {}
        fn handle(_s: &mut (), event: &InputEvent, ctx: &mut HandleCtx<'_>) -> Handled {
            if ctx.phase() == Phase::Capture
                && matches!(event.as_key().map(|k| k.key), Some(Key::Escape))
            {
                ctx.emit(Outcome::Dismissed);
                return Handled::Yes;
            }
            Handled::No
        }
    }

    #[test]
    fn capture_phase_stops_routing_before_the_target() {
        // A `Trap` container scoping a focusable `Button`: Escape is caught on the
        // way down (capture), so the button never sees it.
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        // The scope id must itself have a handler registered; declare the trap as
        // a widget at the scope id by using the same key for scope and a widget.
        let trap_key = key("trap");
        frame.widget(trap_key, Rect::default(), &Trap);
        frame.scoped(trap_key, |f| {
            f.widget(key("btn"), Rect::from_size(Size::new(4, 1)), &Button);
        });
        let (facts, handlers) = frame.into_parts();
        store.end_frame();

        let trap = WidgetId::ROOT.child(trap_key);
        let btn = trap.child(key("btn"));
        let mut focus = Focus::new();
        focus.set(Some(btn));

        let result = route(
            &facts,
            &handlers,
            &mut focus,
            &mut store,
            &InputEvent::key(Key::Escape),
        );
        assert!(result.consumed, "the trap consumed Escape on capture");
        assert_eq!(result.outcomes, vec![(trap, Outcome::Dismissed)]);
    }

    use crate::geometry::Position;
    use crate::input::{MouseButton, MouseEvent, MouseKind};

    #[test]
    fn mouse_down_activates_the_button_under_the_pointer() {
        // Two stacked one-row buttons: `a` at row 0, `b` at row 1.
        let (facts, handlers, mut store, a, b) = two_buttons();
        let mut focus = Focus::new();
        // Click on row 1 → button `b` activates (Button consumes Down).
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::new(0, 1),
        ));
        let result = route(&facts, &handlers, &mut focus, &mut store, &click);
        assert!(result.consumed);
        assert_eq!(result.outcomes, vec![(b, Outcome::Activated)]);
        let _ = a;
    }

    #[test]
    fn unconsumed_click_focuses_a_focusable_target() {
        // A widget that does not consume mouse events but is focusable.
        struct ClickTarget;
        impl Widget for ClickTarget {
            type State = ();
            fn render(&self, _s: &mut (), ctx: &mut RenderCtx<'_>) {
                ctx.focusable(true);
            }
            // Default handle: ignores everything, including the mouse.
        }

        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.widget(key("hit"), Rect::from_size(Size::new(4, 1)), &ClickTarget);
        let (facts, handlers) = frame.into_parts();
        store.end_frame();

        let hit = WidgetId::ROOT.child(key("hit"));
        let mut focus = Focus::new();
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::new(1, 0),
        ));
        let result = route(&facts, &handlers, &mut focus, &mut store, &click);
        // The handler ignored it, so it is unconsumed — but click-to-focus moved
        // focus to the target so the app still sees a raw click.
        assert!(!result.consumed);
        assert_eq!(focus.current(), Some(hit));
    }

    #[test]
    fn consumed_click_also_focuses_the_target() {
        // Textual's rule: focus follows the click even when the widget consumes
        // it — activation happens as the focused widget, and the next Tab
        // starts from where the user last clicked.
        let (facts, handlers, mut store, _a, b) = two_buttons();
        let mut focus = Focus::new();
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::new(0, 1),
        ));
        let result = route(&facts, &handlers, &mut focus, &mut store, &click);
        assert!(result.consumed);
        assert_eq!(result.outcomes, vec![(b, Outcome::Activated)]);
        assert_eq!(focus.current(), Some(b));
    }

    #[test]
    fn click_outside_any_hit_region_does_nothing() {
        let (facts, handlers, mut store, _a, _b) = two_buttons();
        let mut focus = Focus::new();
        // Row 5 is past both buttons; no hit region contains it.
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::new(0, 5),
        ));
        let result = route(&facts, &handlers, &mut focus, &mut store, &click);
        assert!(!result.consumed);
        assert!(result.outcomes.is_empty());
        assert!(focus.current().is_none());
    }

    #[test]
    fn reconcile_recovers_dead_focus_to_first_survivor() {
        let (facts, _handlers, _store, _a, _b) = two_buttons();
        let mut focus = Focus::new();
        focus.set(Some(WidgetId::ROOT.child(key("gone"))));
        focus.reconcile(&facts);
        // The dead id is gone; focus lands on the first surviving focusable.
        assert_eq!(focus.current(), Some(WidgetId::ROOT.child(key("a"))));
    }
}
