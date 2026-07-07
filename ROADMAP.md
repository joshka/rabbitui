# rabbitui — Roadmap

Vertical slices. Each slice ends with a runnable example and green `cargo check` / `clippy` /
`test`; each proves something specific and feeds corrections back into the ADRs (supersede, never
silently edit). Ordering rationale: the testing floor arrives before the widget catalog (ADR 0009),
the inline-mode proof arrives before rendering assumptions ossify (the wave's sharpest demand), and
the flagship app is a coding-agent chrome (the workload of the era).

Date: 2026-07-06 · **Progress tracker** (updated as each slice commits):

| Slice | What                                   | Status                                                            |
| ----- | -------------------------------------- | ----------------------------------------------------------------- |
| 0     | Substrate smoke                        | ✅ done                                                           |
| 1     | Walking skeleton                       | ✅ done                                                           |
| 2     | Declared frame + testing floor         | ✅ done                                                           |
| 3     | Identity, focus, outcomes              | ✅ done                                                           |
| 4     | TextInput, SelectionList, theming      | ✅ done                                                           |
| 5     | Inline mode + vt100 harness            | ✅ done                                                           |
| 6     | Async effects, coalescing, widget cmds | ✅ done                                                           |
| 7     | Overlays, mouse, forms                 | ✅ done                                                           |
| 8     | Agent-chrome flagship                  | ✅ done                                                           |
| 9     | Bridge, docs pass, 0.1                 | 🔨 bridge ✅ · docs + fold-backs remaining (positioning → author) |

Known deferred items (tracked in design-note deltas): buffer-level layer compositing (ADR 0003
amendment pending), block-level early commit for streaming, virtualized transcript, per-terminal
wheel normalization, hardware-cursor via facts, WidthPolicy seam (waits on qwertty Phase 3),
kitty-shaped KeyEvent adaptation (pre-pin blocker), macOS /dev/tty workaround upstreaming. Slice-8
strain findings (slice-9 inputs): variable-height measurement + a real scroll container (the
fixed-slot Collapsible stack wastes rows), styled-span soft wrap (Text takes one style while commits
are `Vec<Span>` — styling pops at commit), Attrs::remove, block-level early commit for bounded tails.

## Slice 0 — Substrate smoke (`examples/smoke.rs`)

Workspace conversion (`rabbitui-core`, `rabbitui`, `rabbitui-testing` stubs; widgets and bridge
crates come when they have content). qwertty git dependency behind the one-file seam; interim
SGR/mode encoder (styles, alt-screen, mode 2026 brackets) over the raw-bytes escape hatch. Example:
enter alt-screen, draw styled text at a position, quit on any key, restore terminal on Drop, panic,
and ctrl-c.

**Proves:** the substrate seam, the encoder, panic-safe restore. **Deferred:** everything else.

## Slice 1 — Walking skeleton (`examples/hello.rs`)

The full loop end-to-end: `select!` over session events + mailbox → update → layout → render
(declared frame, one `Text` widget) → composite → double-buffer diff → mode-2026 framed write. Frame
scheduler with coalescing. Quit on `q`.

**Proves:** ADR 0001's loop shape, 0003's diff pipeline, 0005's scheduler. **API sketch:**
`App::run(state, update_fn, view_fn)` facade over the loop; `Frame::widget(key, spec)`.

## Slice 2 — Counter + testing floor (`examples/counter.rs`)

State + events through the declared frame; keys/IDs in anger; first snapshot tests via the headless
driver (inject key events, injectable clock, assert buffer, snapshot with update flag).
`rabbitui-testing` becomes real.

**Proves:** the declared-frame contract is testable and ergonomic at hello-world scale.

## Slice 3 — Identity, focus, outcomes (`examples/focus.rs`)

Two buttons and a list; tab/shift-tab traversal from frame facts; capture→target→bubble routing;
controls return outcomes consumed by the app. Per-ID state store with lifecycle (state dropped after
N absent frames).

**Proves:** ADR 0002 and 0006 — the parts every prior framework got wrong first.

## Slice 4 — Real widgets + theming (`examples/todo.rs`)

`TextInput` (grapheme-correct cursor and editing) and `SelectionList` (durable selection,
virtualized with pluggable backend) — deliberately the two widgets that force the hard problems.
Semantic theme tokens; one preset (Catppuccin); TOML hot-reload in debug.

**Proves:** ADR 0007/0008 at the depth where Textual's and Brick's failures live.

## Slice 5 — Inline mode + PTY harness (`examples/stream.rs`)

The renderer invariant: append-once scrollback commit + bounded live tail; runtime switch inline ↔
alt-screen; resize without history corruption (store source, wrap at render). The vt100-parser PTY
harness lands here and pins escape-level behavior.

**Proves:** ADR 0013 before the catalog bakes in alt-screen assumptions; ANSI-level testing catches
what buffer tests miss (tui2/textual-rs finding).

## Slice 6 — Async effects (`examples/fetch.rs`)

Commands as futures/streams re-entering as messages; a timer; a simulated slow fetch with
cancel-previous semantics; frame coalescing under stream load; effect-task panic containment with
terminal restore.

**Proves:** ADR 0005 under real concurrency, the user's async-state-machine model.

## Slice 7 — Overlays, forms, mouse (`examples/form.rs`)

Multi-field form with validation; modal dialog on a z-layer; mouse hit-testing and click routing
through facts; scroll-into-view via visibility requests.

**Proves:** ADR 0003's layers and 0006's mouse path — where flat-region models break.

## Slice 8 — Flagship: agent chrome (`examples/agent.rs`, grows into its own crate)

Streaming markdown transcript (source-stored, render-wrapped), collapsible diff cell, tool-call log,
prompt composer; inline by default, alt-screen togglable. This is the acceptance test of the whole
design and the living reference app — every vendor rebuilds exactly this, and survival requires a
flagship (prior-art's law).

## Slice 9 — Bridge, docs, 0.1

`rabbitui-ratatui` bridge crate; rustdoc pass to std quality with runnable examples on every module;
crate-root mini-tutorial; BREAKING-CHANGES.md; release checklist; the positioning decision
(ADR 0014) goes to the author.

## Standing rules

- `cargo check` after nearly every edit; clippy + tests at each stopping point.
- Each slice ends with an honest "what this revealed" note; ADR corrections by supersession.
- Widget crates stay runtime-free; only the facade touches tokio.
- Substrate gaps discovered here are filed into `work/qwertty/substrate-requirements.md` rather than
  worked around silently.
