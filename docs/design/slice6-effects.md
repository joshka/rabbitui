# Slice 6 design: async effects, timers, widget commands

Working design note for slice 6 (ROADMAP.md), implementing the effects half of ADR 0005 and closing
two recorded gaps: controlled inputs (slice-4 delta) and `request_focus` reveal-or-fail (ADR 0006
amendment).

## Messages

Apps that use effects define a message type. `Event` becomes `Event<M = ()>`:
`Input(InputEvent) | Resize(Size) | Message(M) | EffectFailed(EffectError)`. `EffectError` carries
the effect's group name (if any) and the panic/failure text — effect panics are contained (tokio
task boundary), never kill the app, and are surfaced honestly rather than swallowed.
`App<S, M = ()>` and `Update<'_, M>` pick up the parameter; message-less apps compile unchanged with
the default.

## Commands (ADR 0005: commands-only, no subscriptions)

`Command<M>` in the facade (it owns tokio):

- `Command::future(async move -> M)` — one message.
- `Command::stream(impl Stream<Item = M>)` — many messages; ends when the stream does (a "subscription"
  is just a long stream the app chose to start).
- `Command::timeout(Duration, impl FnOnce() -> M)` — sugar over `future`.
- `.group(&str)` — **cancel-previous**: spawning into a group aborts the group's previous task
  (Textual `@work(exclusive=True)` semantics; the debounced-search pattern). Ungrouped commands run
  to completion.

Issued via `Update::spawn(Command<M>)` (buffered like `commit`, drained by the runtime after `update`
returns). Results re-enter the loop mailbox and arrive as `Event::Message(M)` in order of
completion. The effect runtime (spawn tables, group abort, mailbox) is an `Effects<M>` struct
separable from the render loop and unit-tested directly under `#[tokio::test]` — the loop itself
still needs a tty, so async semantics get tested below the loop.

## Frame coalescing (the tui2 FrameRequester, first real need)

Streams can outpace frames. The loop gains a frame budget (~60fps): after processing an
event/message, drain everything already queued (biased `try_recv` loop — one render absorbs a
burst), then render if the budget allows, else arm a trailing deadline in the `select!` so the last
state always paints. A `flood` test proves messages ≫ frames.

## Widget commands — typed, erased at the store boundary

The facade cannot know widget state types (it does not depend on rabbitui-widgets). The mechanism
that keeps typing without the dependency:

```rust
update.widget::<TextInput>(&[key("search")], |state| state.clear());
```

`Update::widget::<W: Widget>(path, f: impl FnOnce(&mut W::State))` boxes a monomorphized closure
that downcasts via `StateStore::get_dyn_mut` when the runtime applies it **between frames** (after
update, before the next view). Missing/foreign-typed state: the command is dropped with a
`debug_assert` — commanding a widget that was never declared is an app bug. Widget state types grow
the public mutation surface they want commanded (`TextInputState::clear`, `set_value`;
`SelectionListState::select`). This unlocks **controlled-input patterns**: the todo example's
re-keying workaround is replaced by `update.widget::<TextInput>(path, |s| s.clear())` on submit; the
slice-4 delta is folded back.

`Update::focus(&[Key])` — focus-by-path from the app. Reveal-or-fail: applied when the target is
present-and-focusable in the _next_ frame's facts (covering the declare-then-focus case naturally);
if still absent after that frame, dropped with a `debug_assert` naming the path. This closes the ADR
0006 amendment's silent-ignore gap (fail loudly in debug; document release behavior).

## examples/fetch.rs

A fake search app: TextInput (group("search") command per keystroke — simulated 300ms fetch
producing items), SelectionList of results, a completed- fetch counter proving cancel-previous
(rapid typing completes far fewer fetches than keystrokes), a ticker stream ('t' toggles a clock
line), and Ctrl-L clearing the input via widget command. Inline-mode friendly but runs alt-screen by
default.

## Testing

`Effects<M>` unit tests (tokio): future delivery, stream delivery + end, group cancel-previous
(first result never arrives), panic containment → EffectFailed with group name, ordering under
concurrency (completion order, not spawn order). Loop-level: coalescing flood test at the
Effects/scheduler boundary.

**Correction to the widget-command placement**: the pending widget-command table is pure
(type-erased `Box<dyn FnOnce(&mut dyn Any)>` keyed by id, plus the deferred focus request) and needs
no tokio — it lives in **core** (`core::pending`, applied by a single
`apply(&mut StateStore, &FrameFacts, &mut Focus)` function) so the runtime and TestApp share one
implementation by construction, exactly like `routing::route`. TestApp gains
`send_message`-equivalent state mutation plus the same between-frames apply. Todo example migrates
to widget-command clear; its integration test updates accordingly.

## Implementation deltas

Recorded during implementation; decisions favor the simplest option consistent with ADR 0005/0006.

- **`Event<M>` / `Update<'a, M>` lost `Copy`.** `Event::Message(M)` and
  `Event::EffectFailed(EffectError)` are not `Copy` (`M` is arbitrary, `EffectError` owns a
  `String`), so `Event<M>` derives only `Clone` (bounded on `M: Clone`) and `Update` derives only
  `Debug`. Consequently `Update::event()` now returns `&Event<M>` rather than `Event` by value. All
  call sites match on the reference (`if let Event::Input(input) = update.event()` binds by ref),
  which the examples already did, so the fallout was mechanical.
- **`App<S, U, V, M = ()>` carries a `PhantomData<fn() -> M>`.** `M` appears only in the `update`
  closure's parameter, not in a field, so the struct needs a marker to name it; `fn() -> M` keeps
  `App` variance-correct and `Send`-agnostic.
- **The loop is a `select!` with a `Wake` enum.** Rather than nest the three arm bodies, each arm
  maps to a `Wake<M>` value (`Input(Box<qwertty::InputEvent>)`, `Effect(Outbox<M>)`, `Paint`,
  `Idle`) that a `match` below folds into state. The input is boxed because qwertty's event is far
  larger than the other variants. The trailing-paint arm is guarded `if deadline.is_some()`; a
  helper `sleep_until(Option<Instant>)` gives it a concrete future (the `None` branch parks forever
  and never runs under the guard).
- **Frame budget = 16_667µs (~60fps), dirty flag + `next_paint` deadline.** After any state change
  the loop marks itself dirty; if the last paint was within the budget it arms a trailing `Instant`
  deabline instead of painting, so a burst coalesces into one frame at the budget boundary. Effect
  results additionally drain in a biased `try_recv` loop (`deliver_effect` per item) before the
  paint, so a stream flood is one render. Proven by
  `effect::tests::flood_of_stream_messages_drains_in_one_burst`.
- **Widget commands apply against the _last drawn_ frame's facts, immediately in `drain_pending`,
  with a redraw following.** `core::pending::Pending::apply` couples commands (store-only) and focus
  (facts) into one call. Applying against the last frame's facts + store and then repainting (the
  loop is always dirty after an update) is equivalent to "between frames" for every case where the
  commanded/focused widget was declared in the previous frame — the flagship cases (clear-on-submit,
  focus an existing field). The one edge it does _not_ cover is declare-then-focus of a widget that
  appears only in the frame the command triggers; that would need the _next_ frame's facts, and
  merging several updates' `WidgetPending` (which has no extend API) across a paint. Deferred as a
  known limitation; the `debug_assert` in `apply` still fires loudly if such a focus target never
  materializes.
- **`Ctrl-L` required a substrate-bridge in `rabbitui::input`.** qwertty has no modifier protocol,
  so a Ctrl-letter chord arrives as a raw C0 byte (`ControlInput::Other(0x01..=0x1A)`). The mapper
  now surfaces those as the letter `Key::Char` with the Ctrl modifier set, so an app can bind
  `Ctrl-L` and `TextInput` (which already ignores ctrl chords) leaves it for the app. This is a
  behavior change: `Ctrl-C` etc. now reach the app instead of being dropped. `Ctrl-I`/`Ctrl-M` stay
  Tab/Enter (byte-identical, as in every terminal).
- **`Command::stream` bound is `futures_core::Stream`; no `tokio-stream`/`async-stream`.** The stream
  task hand-rolls a `poll_fn` forwarding loop; the fetch ticker hand-rolls a `Ticker: Stream` over
  `tokio::time::Interval::poll_tick` (~15 lines), exactly as specified.
- **Panic text extraction.** `Effects::watch` distinguishes a cancel-previous abort
  (`JoinError::is_cancelled` → report nothing) from a real panic (downcast the payload to
  `&str`/`String`, else a generic message) so `Event::EffectFailed` carries readable text.
- **TestApp gained `apply_pending(build, view)` and `inject(update, view)`.** `apply_pending`
  records into a fresh `core::pending::Pending`, applies it through the _same_ `Pending::apply` the
  runtime uses, and re-renders — the harness/runtime cannot drift. `inject` is the
  `send_message`-equivalent (a thin alias of `send`, named for folding an effect result).
- **Fixed inherited failing tests.** `pending.rs`'s two unit tests and its module doctest
  read/seeded widget state via `get_or_default` _after_ the declaring frame ended, tripping the
  store's per-frame duplicate-id assert. They were shipped compiling-but-failing; switched to
  `get_dyn_mut` (the between-frames accessor that does not touch `last_seen`), matching how the
  runtime reaches retained state.
- **The old todo re-keying is gone.** `examples/todo.rs` and `tests/todo_flow.rs` now use a stable
  input key and clear on submit via a widget command (`update.widget::<TextInput>` /
  `TestApp::apply_pending`). A consequence the test now asserts: the input _keeps focus_ across a
  submit (its identity no longer churns).
