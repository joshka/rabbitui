# ADR 0005: Async-first framework-owned event loop with a synchronous core

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

rabbitui targets the workload of the era: streaming, interaction-heavy apps whose canonical case is
a coding-agent CLI rendering a live model stream while one background task pumps token deltas,
another awaits a tool call, and a timer drives a spinner. The concurrency model is not incidental to
that workload — it _is_ the workload. Five forces bear on it.

**The substrate is async-first and readiness-driven, with no background reader.** qwertty's
`TokioTerminalSession` is the sole owner of `/dev/tty`, readiness-driven on the caller's runtime;
`&mut self` on `next_event()` and typed queries serializes input against queries by construction
(terminal-substrate.md; qwertty ADR 0001/0011). This makes a `tokio::select!` over `next_event()`,
timers, and an app mailbox the natural loop — "the concurrency answer crossterm's exclusive
`EventStream`-vs-`poll` split can't give." The memo states the requirement directly: "rabbitui owns
a `select!` loop over `next_event()`, timers, and app messages… writes + flushes once per frame"
(terminal-substrate.md §substrate requirements).

**A single serialized update kills data races by construction — if the framework owns the loop.**
Bubble Tea's whole safety story is that all async results re-enter through one serialized `Update`;
mutating the model outside the loop is "a proper race condition" and the framework's answer is
"don't — return a Cmd" (bubbletea.md, klabb3 on HN). Cursive reaches the same shape from the
opposite tradition — a synchronous owned loop with a `cb_sink` closure mailbox as the "universal
external-world bridge" (cursive.md). Both prove the loop is the framework's to own.

**Effects want one primitive, not two.** Bubble Tea _had_ Elm-style subscriptions (`tea.Sub`,
`tea.Every`) and deleted them in 2020 — commit `ade8203c`, "we can achieve the same functionality in
a much simpler fashion with commands" — and never missed them (bubbletea.md). A recurring task is a
command that re-arms; a long-lived stream is a command that yields.

**The core must be synchronous even though the loop is async.** xi-editor's async-boundary lesson
(DESIGN.md) and tui2's shape agree: tui2's `FrameRequester`/`FrameScheduler` is an async actor, but
the flatten → measure → paint pass it drives is plain synchronous code over a `ratatui::Buffer`
(codex-tui2.md §event loop). The async boundary belongs at the _edges_ — input, effects, scheduling
— never threaded through update/layout/paint.

**Frame scheduling is a solved subproblem worth copying wholesale.** tui2's coalescing, rate-limited
`FrameRequester` is "the one piece of tui2's runtime worth copying wholesale… orthogonal to the
viewport debate": redraws are _requested_ by many tasks, coalesced, and clamped to 60 FPS on a
broadcast channel, not run per-input (codex-tui2.md implications). The negative proof: ratatui ships
no loop, every app hand-rolls one, and the ecosystem fragments (ratatui.md; DESIGN.md); Cursive's
30ms poll-sleep loop is "its most dated part" and its 2016 async request (issue #92) was never
granted (cursive.md).

## Options considered

### A. Bring-your-own-loop by default (the ratatui model)

_What it is._ Ship a renderer, widget contract, and input decoder; each app writes its own
`loop { poll(); update(); draw() }`.

_Steelman._ Maximum flexibility; no runtime opinion imposed; embeds in any existing async or sync
program; widget crates never touch tokio. It is why ratatui composes with everything.

_Why not chosen._ The fragmentation is the negative proof (ratatui.md; DESIGN.md). Every app
re-derives event coalescing, resize handling, panic-safe restore, and rate-limiting — and most get
the subtle parts (synchronized-output framing, trailing-flush coalescing, restore) wrong. rabbitui's
differentiator is interaction correctness proven at the PTY level (DESIGN.md); that cannot be
delivered if the loop — where resize, coalescing, and restore live — is user-owned by default.
Retained as the _escape hatch_ (see Decision), not the default.

### B. Framework-owned async loop, commands-only effects, synchronous core (CHOSEN)

_What it is._ The runtime crate owns a `tokio::select!` over `next_event()`, timers, and the app
mailbox, driving a strictly synchronous update → layout → render → diff → write core. Effects are
`Cmd = Future<Msg>` / `Stream<Msg>` spawned by the runtime; no subscription primitive; a coalescing
`FrameRequester` schedules redraws.

_Steelman._ Serialized update kills races by construction (bubbletea.md); one effect primitive
instead of two (`ade8203c`); the async boundary stays at the edges where the substrate already put
it (terminal-substrate.md); scheduling reuses tui2's proven shape (codex-tui2.md). It matches the
substrate's grain exactly — no background reader, `&mut self` serialization, readiness-driven. Costs
accepted in Consequences.

### C. Actor-per-widget message pump (the Textual model)

_What it is._ Every widget owns a task and message queue; handlers await in order; messages bubble
up the DOM (textual.md).

_Steelman._ Per-widget serialized handlers give race-free widget state and comprehensible ordering;
bubbling + selector-targeted handlers scale to harlequin-sized apps (textual.md).

_Why not chosen._ "Python needed actors for cooperative fairness; Rust doesn't need 1000 tasks for
1000 widgets" (textual.md). The serialized-handler _guarantee_ is worth keeping, but as ordered
dispatch on one loop, not thousands of tasks. tui2 is the counter-evidence: a production
coding-agent TUI with one actor (`FrameScheduler`) and an ordinary `AppEvent` enum drained on a
single loop — "the message model was _not_ what tui2 changed" (codex-tui2.md).

### D. Blocking synchronous loop with a closure mailbox (the Cursive model)

_What it is._ A synchronous poll-sleep loop; background threads mutate the UI by sending
`Box<dyn FnOnce(&mut App)>` closures through a channel drained each step (cursive.md).

_Steelman._ "A great concurrency story for 90% of apps" — any thread sends a closure, zero async
ceremony, no tokio dependency (cursive.md).

_Why not chosen._ The 30ms poll-sleep is "its most dated part"; async "is strictly more powerful,"
requested in 2016 and never delivered (cursive.md, issue #92). On an async-first substrate with no
background reader, poll-sleep is strictly worse than a `select!` that wakes on readiness. We keep
the _idea_ — a `Send` mailbox as the external-world bridge — but drive it from `select!`, not a
sleep, with coalesced redraw as a throttle (cursive.md implications).

## Decision

**rabbitui is async-first on tokio, and the framework owns the event loop by default.** The facade
crate (`rabbitui`) runs a `tokio::select!` over three sources — qwertty's `next_event()`, timers,
and the app message mailbox — and each wake drives one pass of update → layout → render → diff →
write, writing and flushing once per frame inside synchronized-output (mode 2026) framing.

**The update/layout/paint core is strictly synchronous and single-threaded.** No `.await` appears in
update, layout, measurement, or paint; the async boundary lives only at the loop edges — input,
effects, and frame scheduling (xi-editor's lesson). The widget tree is `!Send`; only the mailbox is
`Send` (cursive.md).

**Effects are commands only.** The single effect primitive is `Cmd = Future<Msg>`, with
`Cmd::stream(impl Stream<Item = Msg>)` for long-lived sources; the runtime spawns them and injects
results into the loop as messages. **rabbitui ships no subscription primitive** — a recurring timer
is a command that re-arms, a stream is a command that yields (Bubble Tea `ade8203c`). Command
completion order is unspecified; rabbitui provides an explicit `Cmd::sequence` for ordered effects
(bubbletea.md, cf. `tea.Sequence`).

**Frame scheduling is a coalescing, rate-limited requester copied from tui2.** rabbitui exposes a
clonable `FrameRequester` handle (`schedule_frame()`, `schedule_frame_in(dur)`) any task may hold; a
`FrameScheduler` coalesces requests and clamps draws to a target frame rate with a **trailing
flush** (a request inside the rate-limit window still yields one final frame). Redraws are
requested, never run per-input.

**The loop is panic-safe.** Panics in effect tasks are caught and surfaced as messages; a panic
anywhere always restores the terminal (cooked mode, alt-screen exit, cursor) before unwinding
(bubbletea.md, leg100's `reset` complaint). rabbitui installs an explicit panic hook so restore does
not depend on `Drop` running, with qwertty's best-effort `Drop` restore (`tokio_session.rs:540`) as
backstop.

**"Bring your own loop" is the escape hatch, not the default.** rabbitui exposes the stepping
primitives (build a frame, feed one event, drive one update/paint) so an app that already owns a
tokio loop can drive rabbitui by hand. This path is documented as advanced and promises no
coalescing, resize, or restore handling.

**Widget crates stay runtime-free.** `rabbitui-core` (ids, facts, buffer, style, layout, widget
contract) and `rabbitui-widgets` have **no tokio dependency**; only the `rabbitui` facade touches
tokio and the loop (DESIGN.md §crate-layout). The encode-only substrate layer is importable without
the `tokio` feature (terminal-substrate.md), so renderer and widget tests never start a runtime.

## Consequences

**Positive.**

- Data races on app state are impossible by construction: all effect results re-enter the one
  serialized update (bubbletea.md).
- One effect concept to learn and teach; no `Sub`-vs-`Cmd` split (`ade8203c`).
- The loop matches the substrate's grain — readiness-driven, no background reader, `&mut self`
  serialization — so there is no impedance mismatch and no second sync code path.
- Frame scheduling is correct and cheap out of the box; the user never hand-writes coalescing
  (codex-tui2.md).
- Widget and renderer crates compile and test with no runtime — third-party authors and coding
  agents write runtime-free code (DESIGN.md).
- Terminal restore is guaranteed on panic, the single most-cited operational failure of hand-rolled
  loops (bubbletea.md).

**Negative (honest).**

- tokio is a hard dependency of the facade. An app wanting a non-tokio runtime, or none, must use
  the escape hatch and forfeit the coalescing/resize/restore machinery. Acceptable because the
  substrate is already tokio-shaped; a runtime-agnostic loop is explicitly not designed now
  (mirroring qwertty ADR 0011's deferral).
- Command completion is unordered, which surprises users expecting issue-order resolution; the
  mitigation (`Cmd::sequence`) is opt-in and must be documented prominently (bubbletea.md).
- Long-running synchronous work in update/paint stalls the loop and backs up the mailbox
  (bubbletea.md). The discipline — "all I/O in commands, only I/O in commands, no bare task spawns
  touching state" — is a contract the user must honor, not one the compiler enforces.
- Owning the loop means owning resize, coalescing, and restore correctness across the terminal
  matrix — real permanent surface area, and precisely the surface rabbitui exists to get right
  (DESIGN.md).

**Neutral.**

- The `FrameScheduler` is the _only_ actor in the system; the rest is ordinary synchronous code
  driven from one loop (codex-tui2.md) — deliberately unlike Textual's task-per-widget model.
- An optional MVU shell (`rabbitui-tea`, later) can layer an Elm-style `Cmd`/`Msg` vocabulary over
  this loop without changing it, since the loop is already message-serialized (DESIGN.md).
- The `Send` mailbox / `!Send` tree split is a deliberate choice, dodging the fight Cursive lost in
  both directions (cursive.md, issue #383 and the 0.4.0 `Send + Sync` breakage).

## Revisit triggers

- **Runtime-agnostic demand.** A material set of adopters need a non-tokio runtime (async-std, smol,
  embassy) _and_ the escape hatch proves insufficient — revisit extracting a runtime-agnostic loop
  trait, deferred now exactly as qwertty ADR 0011 defers its own.
- **Ordering complaints.** Unordered command completion causes recurring real bugs despite
  `Cmd::sequence` — reconsider ordered-by-default delivery for some effects.
- **Loop-stall reports.** Profiling real coding-agent workloads shows the synchronous core stalling
  on measure/paint at realistic transcript sizes — revisit moving specific measurement or wrapping
  off the loop thread, with the async boundary held at a clean seam.
- **Subscription pull.** After the catalog matures, users repeatedly reinvent the same
  self-re-arming command patterns — reconsider thin _sugar_ over commands (never a second
  primitive); the bar is Bubble Tea's six-years-and-never-missed-them evidence (`ade8203c`).
- **Frame-rate policy.** The fixed 60-FPS clamp proves wrong for high-refresh terminals or
  bandwidth-constrained SSH — make the `FrameScheduler` rate adaptive (the tui2 scroll study shows
  terminal behavior varies enough to warrant measured defaults — codex-tui2.md).
