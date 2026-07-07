# Slice 3 design: facts, routing, focus, outcomes

Working design note for implementing ADRs 0002/0006 (slice 3 of ROADMAP.md). This resolves the API
questions the ADRs left to implementation. Supersede or fold into ADRs if implementation contradicts
it.

## The event-time problem, and the resolution

Widgets are specs that die after render, so nothing widget-shaped exists when an event arrives. The
declared-frame answer: at render time, declaring a widget also registers a **monomorphized handler
thunk** for its id; at event time, the framework routes the event through the _previous frame's
facts_ to the registered thunks, which mutate retained state in the StateStore and return outcomes;
the app then sees those outcomes in `update`. Handlers are **associated functions on the widget
type** (no `&self`), so no spec instance is needed:

```rust
pub trait Widget {
    type State: Default + 'static;
    fn render(&self, state: &mut Self::State, ctx: &mut RenderCtx<'_>);
    /// Handles an event routed to this widget. Associated fn — runs without a
    /// spec, against retained state only. Default: ignore everything.
    fn handle(state: &mut Self::State, event: &InputEvent, ctx: &mut HandleCtx<'_>) -> Handled {
        let _ = (state, event, ctx);
        Handled::No
    }
}
```

`Frame::widget` registers `W::handle` type-erased
(`fn(&mut dyn Any, &InputEvent, &mut HandleCtx) -> Handled`, one wrapper per `W`, stored in a
per-frame `HandlerMap: HashMap<WidgetId, Handler>` that the runtime keeps alongside the facts of the
frame it came from).

## Core input vocabulary (`rabbitui-core::input`)

Core is substrate-free, so it gets its own event types; the facade maps qwertty's `InputEvent` into
them (and owns all Escape/CSI interpretation):

```rust
pub enum InputEvent { Key(KeyEvent) /* Mouse, Paste arrive in later slices */ }
pub struct KeyEvent { pub key: Key, pub modifiers: Modifiers }
pub enum Key {
    Char(char), Enter, Escape, Backspace, Tab, BackTab,
    Up, Down, Left, Right, Home, End, PageUp, PageDown, Delete,
}
pub struct Modifiers { pub ctrl: bool, pub alt: bool, pub shift: bool }
```

## Facts (`rabbitui-core::facts`)

```rust
pub struct FrameFacts { entries: Vec<FactEntry> }   // declaration order = paint order
pub struct FactEntry {
    pub id: WidgetId,
    pub parent: WidgetId,     // scope parent, for capture/bubble paths
    pub area: Rect,           // absolute buffer coordinates
    pub focusable: bool,
}
```

Queries: `get(id)`, `hit(Position) -> Option<&FactEntry>` (last declared containing entry wins —
declaration order approximates z until layers land), `focus_order() -> impl Iterator` (declaration
order, focusable only), `path_to(id) -> Vec<WidgetId>` (root→target via parent links).

`Frame` collects facts while declaring; `RenderCtx` gains `ctx.focusable(bool)` (marks the current
entry, per-instance so a disabled control can opt out) and `ctx.is_focused() -> bool` (render-time
focus query, for painting focus styles).

## Focus

`Focus { current: Option<WidgetId> }` is framework state owned by the runtime (not in the StateStore
— it is cross-widget). Traversal: on a `Tab`/`BackTab` key event that no handler consumed, the
runtime advances `current` through last-frame `focus_order()`, wrapping; if `current`'s id vanished
from facts, focus moves to the next surviving entry in order. Apps can command focus by id
(`ctx.request_focus()` from a handler; app-level focus-by-key API arrives with effects in slice 6).

## Routing

For each core `InputEvent`:

1. **Target**: key events target `focus.current` (fall back: no target); mouse events (slice 7) will
   target `facts.hit(pos)`.
2. **Capture**: walk `path_to(target)` root→target, calling handlers of ancestors with
   `Phase::Capture`; any `Handled::Yes` stops routing. (v1: containers rarely register handlers, so
   this is cheap.)
3. **Target + bubble**: call target's handler, then ancestors target→root with `Phase::Bubble`,
   stopping on `Handled::Yes`.
4. **Framework defaults**: unconsumed Tab/BackTab → focus traversal.
5. Everything still unconsumed → passed to the app's `update` as `Event::Input`.

`HandleCtx` carries: the phase, the entry's area, `emit(Outcome)`, `request_focus()`, and `&mut`
access nothing else (no buffer — handlers do not paint).

## Outcomes

v1 is a closed core enum (revisit trigger in ADR 0001 if it strains):

```rust
pub enum Outcome { Activated, Changed(String), Submitted, Toggled(bool), Selected(usize), Dismissed }
```

Handlers `emit` outcomes; the runtime collects `Vec<(WidgetId, Outcome)>` per event and hands them
to the app **in the same update call** as the event:

```rust
pub struct Update<'a> { pub event: Event, outcomes: &'a [(WidgetId, Outcome)] }
impl Update<'_> {
    pub fn outcome_for(&self, path: &[Key]) -> Option<&Outcome>;  // root-relative key path
}
```

The facade's `run` signature becomes `update: impl FnMut(&mut S, Update<'_>) -> ControlFlow<()>`.
(`outcome_for` takes a key path because ids compose; the common case is a root-level
`&[key("search")]`.)

## First interactive widget: Button

`Button::new(label)`, `State = ()`, focusable, `handle`: Enter/Space → `emit(Outcome::Activated)`,
`Handled::Yes`. Paints label, reversed style when focused. Proves: focusable facts, traversal,
handler thunks, outcomes.

## examples/focus.rs

Two buttons + a status Text; Tab cycles focus; Enter/Space activates; the status line names the
last-activated button; `q`/Escape quits (app-level, proving unconsumed events still reach `update`).

## Testing

TestApp gains `send_key(Key)` / `send_event(InputEvent)` running the full route→update path (TestApp
must therefore own the same routing plumbing the runtime uses — extract routing into a core or
shared function so the harness and runtime cannot drift). Integration tests: traversal order,
wrap-around, focus survives re-declaration, dead-id focus recovery, outcome delivery,
unconsumed-event fallthrough, capture-stops-routing.

## Implementation deltas

Deviations made during the slice-3 implementation, with rationale:

- **`update` is always called with the event, even when a handler consumed it.** The spec's routing
  step 5 reads "everything still unconsumed → passed to the app's `update` as `Event::Input`", which
  could be read as _skipping_ the `update` call for consumed events. Instead, `run` calls `update`
  exactly once per mapped event with an `Update { event, outcomes }` in every case: outcomes must be
  delivered "in the same update call as the event" (the Outcomes section), so `update` has to run
  when an event was consumed-and-produced-an- outcome. Apps follow the ADR 0001 worked-example
  pattern — check `outcome_for` first, then handle raw keys — so a consumed key that yielded an
  outcome does not also misfire as a raw binding. Net effect matches the intent (unconsumed keys
  reach the app; consumed keys arrive as outcomes) with a simpler, single `update` call.
  `Event::Input` does not expose a `consumed` bit; if an app ever needs it, that is an additive
  change.

- **`RenderCtx::new` gained a `focused: bool` parameter** (was `(buffer, area)`, now
  `(buffer, area, focused)`). The spec says "Frame gains access to a focus snapshot so
  `RenderCtx::is_focused` works" but fixes no signature; threading the verdict in at construction is
  the least-surprising shape and keeps `RenderCtx` self-contained. `Frame::widget` computes
  `focus == Some(id)` and passes it.

- **Focus reconciliation is a named step (`Focus::reconcile`) run after each render, before routing
  the next event**, rather than folded into traversal. The spec describes dead-id recovery as part
  of traversal; splitting it out makes "focus survives re-declaration" and "dead-id recovery"
  testable in isolation and keeps `route` about the current event. Dead-id recovery targets the
  **first surviving focusable in declaration order** (facts carry no cross-frame position, so "next
  survivor after the dead slot" is not recoverable from data on hand); documented on
  `Focus::reconcile`.

- **`StateStore::get_dyn_mut`** was added so the router can lend a widget's retained state to its
  type-erased handler thunk without knowing the concrete type. It does not touch `last_seen`
  (dispatch happens between frames and must not read as a re-declaration).

- **`Frame` grew `with_focus`, `finish`, and `into_parts`.** `new` stays focus-less for existing
  call sites; `with_focus` supplies the snapshot; `finish`/`into_parts` surrender the collected
  facts (and handlers) to the runtime/harness. `TestApp::send_event`/`send_key` return the core
  `RouteResult` (outcomes + consumed) rather than an `rabbitui::app::Update`, because the testing
  crate depends only on core; this keeps the harness runtime-free while exercising the identical
  `route` function.

- **Substrate coverage is narrower than the core `Key` vocabulary.** qwertty decodes text, C0
  controls, and four arrows only, so `Key::BackTab`, `Home`/`End`, `PageUp`/`PageDown`, a forward
  `Delete`, and any non-empty `Modifiers` are defined in core but never produced by the facade's
  mapping in slice 3 (documented in `rabbitui::input`). Ctrl-C (`0x03`) maps to nothing and is
  dropped, so the examples quit on `q`/Escape rather than Ctrl-C. The core vocabulary is
  intentionally ahead of the substrate so widget code needs no revision when qwertty lands those
  protocols (ADR 0006 §9).
