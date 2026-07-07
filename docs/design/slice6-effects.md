# Slice 6 design: async effects, timers, widget commands

Working design note for slice 6 (ROADMAP.md), implementing the effects half of
ADR 0005 and closing two recorded gaps: controlled inputs (slice-4 delta) and
`request_focus` reveal-or-fail (ADR 0006 amendment).

## Messages

Apps that use effects define a message type. `Event` becomes `Event<M = ()>`:
`Input(InputEvent) | Resize(Size) | Message(M) | EffectFailed(EffectError)`.
`EffectError` carries the effect's group name (if any) and the panic/failure
text — effect panics are contained (tokio task boundary), never kill the app,
and are surfaced honestly rather than swallowed. `App<S, M = ()>` and
`Update<'_, M>` pick up the parameter; message-less apps compile unchanged
with the default.

## Commands (ADR 0005: commands-only, no subscriptions)

`Cmd<M>` in the facade (it owns tokio):
- `Cmd::future(async move -> M)` — one message.
- `Cmd::stream(impl Stream<Item = M>)` — many messages; ends when the stream
  does (a "subscription" is just a long stream the app chose to start).
- `Cmd::timeout(Duration, impl FnOnce() -> M)` — sugar over `future`.
- `.group(&str)` — **cancel-previous**: spawning into a group aborts the
  group's previous task (Textual `@work(exclusive=True)` semantics; the
  debounced-search pattern). Ungrouped commands run to completion.

Issued via `Update::spawn(Cmd<M>)` (buffered like `commit`, drained by the
runtime after `update` returns). Results re-enter the loop mailbox and arrive
as `Event::Message(M)` in order of completion. The effect runtime (spawn
tables, group abort, mailbox) is an `Effects<M>` struct separable from the
render loop and unit-tested directly under `#[tokio::test]` — the loop itself
still needs a tty, so async semantics get tested below the loop.

## Frame coalescing (the tui2 FrameRequester, first real need)

Streams can outpace frames. The loop gains a frame budget (~60fps): after
processing an event/message, drain everything already queued (biased
`try_recv` loop — one render absorbs a burst), then render if the budget
allows, else arm a trailing deadline in the `select!` so the last state always
paints. A `flood` test proves messages ≫ frames.

## Widget commands — typed, erased at the store boundary

The facade cannot know widget state types (it does not depend on
rabbitui-widgets). The mechanism that keeps typing without the dependency:

```rust
update.widget::<TextInput>(&[key("search")], |state| state.clear());
```

`Update::widget::<W: Widget>(path, f: impl FnOnce(&mut W::State))` boxes a
monomorphized closure that downcasts via `StateStore::get_dyn_mut` when the
runtime applies it **between frames** (after update, before the next view).
Missing/foreign-typed state: the command is dropped with a `debug_assert` —
commanding a widget that was never declared is an app bug. Widget state types
grow the public mutation surface they want commanded (`TextInputState::clear`,
`set_value`; `SelectionListState::select`). This unlocks **controlled-input
patterns**: the todo example's re-keying workaround is replaced by
`update.widget::<TextInput>(path, |s| s.clear())` on submit; the slice-4
delta is folded back.

`Update::focus(&[Key])` — focus-by-path from the app. Reveal-or-fail:
applied when the target is present-and-focusable in the *next* frame's facts
(covering the declare-then-focus case naturally); if still absent after that
frame, dropped with a `debug_assert` naming the path. This closes the ADR
0006 amendment's silent-ignore gap (fail loudly in debug; document release
behavior).

## examples/fetch.rs

A fake search app: TextInput (group("search") command per keystroke —
simulated 300ms fetch producing items), SelectionList of results, a completed-
fetch counter proving cancel-previous (rapid typing completes far fewer
fetches than keystrokes), a ticker stream ('t' toggles a clock line), and
Ctrl-L clearing the input via widget command. Inline-mode friendly but runs
alt-screen by default.

## Testing

`Effects<M>` unit tests (tokio): future delivery, stream delivery + end,
group cancel-previous (first result never arrives), panic containment →
EffectFailed with group name, ordering under concurrency (completion order,
not spawn order). Loop-level: coalescing flood test at the Effects/scheduler
boundary.

**Correction to the widget-command placement**: the pending widget-command
table is pure (type-erased `Box<dyn FnOnce(&mut dyn Any)>` keyed by id, plus
the deferred focus request) and needs no tokio — it lives in **core**
(`core::pending`, applied by a single `apply(&mut StateStore, &FrameFacts,
&mut Focus)` function) so the runtime and TestApp share one implementation
by construction, exactly like `routing::route`. TestApp gains
`send_message`-equivalent state mutation plus the same between-frames apply.
Todo example migrates to widget-command clear; its integration test updates
accordingly.
