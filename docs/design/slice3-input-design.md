# Slice 3 design: facts, routing, focus, outcomes

Working design note for implementing ADRs 0002/0006 (slice 3 of ROADMAP.md).
This resolves the API questions the ADRs left to implementation. Supersede or
fold into ADRs if implementation contradicts it.

## The event-time problem, and the resolution

Widgets are specs that die after render, so nothing widget-shaped exists when
an event arrives. The declared-frame answer: at render time, declaring a
widget also registers a **monomorphized handler thunk** for its id; at event
time, the framework routes the event through the *previous frame's facts* to
the registered thunks, which mutate retained state in the StateStore and
return outcomes; the app then sees those outcomes in `update`. Handlers are
**associated functions on the widget type** (no `&self`), so no spec instance
is needed:

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
(`fn(&mut dyn Any, &InputEvent, &mut HandleCtx) -> Handled`, one wrapper per
`W`, stored in a per-frame `HandlerMap: HashMap<WidgetId, Handler>` that the
runtime keeps alongside the facts of the frame it came from).

## Core input vocabulary (`rabbitui-core::input`)

Core is substrate-free, so it gets its own event types; the facade maps
qwertty's `InputEvent` into them (and owns all Escape/CSI interpretation):

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

Queries: `get(id)`, `hit(Position) -> Option<&FactEntry>` (last declared
containing entry wins — declaration order approximates z until layers land),
`focus_order() -> impl Iterator` (declaration order, focusable only),
`path_to(id) -> Vec<WidgetId>` (root→target via parent links).

`Frame` collects facts while declaring; `RenderCtx` gains
`ctx.focusable(bool)` (marks the current entry, per-instance so a disabled
control can opt out) and `ctx.is_focused() -> bool` (render-time focus query,
for painting focus styles).

## Focus

`Focus { current: Option<WidgetId> }` is framework state owned by the runtime
(not in the StateStore — it is cross-widget). Traversal: on a `Tab`/`BackTab`
key event that no handler consumed, the runtime advances `current` through
last-frame `focus_order()`, wrapping; if `current`'s id vanished from facts,
focus moves to the next surviving entry in order. Apps can command focus by id
(`ctx.request_focus()` from a handler; app-level focus-by-key API arrives with
effects in slice 6).

## Routing

For each core `InputEvent`:
1. **Target**: key events target `focus.current` (fall back: no target);
   mouse events (slice 7) will target `facts.hit(pos)`.
2. **Capture**: walk `path_to(target)` root→target, calling handlers of
   ancestors with `Phase::Capture`; any `Handled::Yes` stops routing.
   (v1: containers rarely register handlers, so this is cheap.)
3. **Target + bubble**: call target's handler, then ancestors target→root
   with `Phase::Bubble`, stopping on `Handled::Yes`.
4. **Framework defaults**: unconsumed Tab/BackTab → focus traversal.
5. Everything still unconsumed → passed to the app's `update` as
   `Event::Input`.

`HandleCtx` carries: the phase, the entry's area, `emit(Outcome)`,
`request_focus()`, and `&mut` access nothing else (no buffer — handlers do
not paint).

## Outcomes

v1 is a closed core enum (revisit trigger in ADR 0001 if it strains):

```rust
pub enum Outcome { Activated, Changed(String), Submitted, Toggled(bool), Selected(usize), Dismissed }
```

Handlers `emit` outcomes; the runtime collects `Vec<(WidgetId, Outcome)>` per
event and hands them to the app **in the same update call** as the event:

```rust
pub struct Update<'a> { pub event: Event, outcomes: &'a [(WidgetId, Outcome)] }
impl Update<'_> {
    pub fn outcome_for(&self, path: &[Key]) -> Option<&Outcome>;  // root-relative key path
}
```

The facade's `run` signature becomes
`update: impl FnMut(&mut S, Update<'_>) -> ControlFlow<()>`.
(`outcome_for` takes a key path because ids compose; the common case is a
root-level `&[key("search")]`.)

## First interactive widget: Button

`Button::new(label)`, `State = ()`, focusable, `handle`: Enter/Space →
`emit(Outcome::Activated)`, `Handled::Yes`. Paints label, reversed style when
focused. Proves: focusable facts, traversal, handler thunks, outcomes.

## examples/focus.rs

Two buttons + a status Text; Tab cycles focus; Enter/Space activates; the
status line names the last-activated button; `q`/Escape quits (app-level,
proving unconsumed events still reach `update`).

## Testing

TestApp gains `send_key(Key)` / `send_event(InputEvent)` running the full
route→update path (TestApp must therefore own the same routing plumbing the
runtime uses — extract routing into a core or shared function so the harness
and runtime cannot drift). Integration tests: traversal order, wrap-around,
focus survives re-declaration, dead-id focus recovery, outcome delivery,
unconsumed-event fallthrough, capture-stops-routing.
