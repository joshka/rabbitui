# FakeDevice e2e harness — design (Arc 4 item 8 step 1)

**Why.** `TestApp` drives the _reducer_ (`apply_message`/`on_submit`) but **not the real
`update` closure or the run loop** — which is exactly why the help-overlay panic
(a declare-then-focus request in `update`) and the tool-cell scrollback freeze (an
inline-commit-timing bug) passed every existing test and only showed on real hardware.
This harness runs the **actual `App::run` loop** headlessly so those bug classes become
CI-catchable.

## The seam (confirmed present, both sync and tokio)

- qwertty exposes `TokioTerminalSession::<D>::from_device(device)` (`tokio_session.rs`,
  `TokioTerminalSession<D: TerminalDevice = Terminal>`), the async analogue of
  `TerminalSession::from_device`. So a session can run over any `TerminalDevice`.
- `qwertty::FakeDevice::open() -> (FakeDevice, FakeTerminal)` is a `UnixStream` socketpair
  (`terminal/fake.rs`). `FakeDevice` is the app side (impls `TerminalDevice`, backed by a
  real fd so tokio's `AsyncFd` registers it). `FakeTerminal` is the test side:
  `feed_input(&[u8])` → bytes the app _reads_; read the stream → bytes the app _wrote_.
- rabbitui already has `VtScreen` (`rabbitui-testing/src/vt.rs`) to parse emitted bytes
  into an assertable screen+scrollback.

## Facade change (small, mechanical, backward-compatible)

Today `rabbitui::terminal::Terminal` is concrete: `session: Option<TokioTerminalSession>`
(i.e. `<Terminal>`). Make it generic with a **default type param** so every existing
caller is unchanged:

- `Terminal<D: TerminalDevice = qwertty::Terminal>`, holding
  `session: Option<TokioTerminalSession<D>>`
- `Terminal::open()` stays (returns `Terminal<qwertty::Terminal>`); add
  `Terminal::from_device(device: D) -> Result<Terminal<D>>` (→ `TokioTerminalSession::from_device`).
- The only two loop helpers that take it — `leave(mut terminal: Terminal, …)` and
  `apply_mode_switch(terminal: &mut Terminal, …)` (in `app.rs`) — gain `<D: TerminalDevice>`.
  Their bodies call `Terminal` methods only, so no logic changes.
- Extract the run-loop body (from `Terminal::open()` onward, ~app.rs 912–end of `run`)
  into `async fn run_loop<S, M, D: TerminalDevice>(terminal: Terminal<D>, …) -> Result<()>`.
  `App::run()` opens the real terminal and calls it (`D = qwertty::Terminal`, inferred);
  a new `pub(crate)`/test entry constructs `Terminal::from_device(fake)` and calls the
  same `run_loop`. **This extraction is the careful part** — the loop captures many locals
  (state/update/view/theme/watcher/engine/store/focus/effects/viewport); move them in as
  params or keep them local to `run_loop`. Verify with the full workspace suite + a betamax
  smoke that the flagship still opens/runs over the _real_ device (behavior-preserving).

## Harness (`rabbitui-testing`)

`HeadlessApp` (or a fn): `FakeDevice::open()`, spawn `run_loop(Terminal::from_device(dev), …)`
as a tokio task, keep the `FakeTerminal`. Provide:

- `feed(&[u8])` / `key(...)` / `text(...)` → `FakeTerminal::feed_input`.
- `settle()` → read available output until it quiesces (drain with a short idle timeout),
  feeding it into a `VtScreen`. **Determinism is the hard part**: don't assert on a fixed
  sleep — read-until-idle, or have the app emit a synchronizing marker. A single-thread
  tokio runtime with controlled time (`tokio::time::pause`) is the cleanest.
- `screen() -> &VtScreen` for assertions (incl. scrollback for inline commits).
- `quit()` feeds the quit chord and joins the task.

## First tests to write (the bugs that motivated this)

1. **Help overlay open+close** over the real `update` loop — would have panicked on the
   non-focusable-Panel focus request. Guards it forever.
2. **Inline tool flow** — replay a `tool_use` turn, Allow, and assert the committed Tool
   cell settles to `✓` in vt scrollback (the freeze bug), and that the modal renders.
3. **Mode toggle** inline↔alt with no duplicated tail (the earlier bug).

## Status

Seam confirmed, blast radius scoped, approach spec'd. The `run_loop` extraction + the
deterministic async settle are the two spots that want fresh, unhurried focus — do them
as their own green steps, not in a marathon tail.
