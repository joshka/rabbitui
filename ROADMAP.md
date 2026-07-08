# rabbitui — Roadmap

Vertical slices. Each slice ends with a runnable example and green `cargo check` / `clippy` /
`test`; each proves something specific and feeds corrections back into the ADRs (supersede, never
silently edit). Ordering rationale: the testing floor arrives before the widget catalog (ADR 0009),
the inline-mode proof arrives before rendering assumptions ossify (the wave's sharpest demand), and
the flagship app is a coding-agent chrome (the workload of the era).

Date: 2026-07-06 · **Progress tracker** (updated as each slice commits):

| Slice | What                                   | Status                                            |
| ----- | -------------------------------------- | ------------------------------------------------- |
| 0     | Substrate smoke                        | ✅ done                                           |
| 1     | Walking skeleton                       | ✅ done                                           |
| 2     | Declared frame + testing floor         | ✅ done                                           |
| 3     | Identity, focus, outcomes              | ✅ done                                           |
| 4     | TextInput, SelectionList, theming      | ✅ done                                           |
| 5     | Inline mode + vt100 harness            | ✅ done                                           |
| 6     | Async effects, coalescing, widget cmds | ✅ done                                           |
| 7     | Overlays, mouse, forms                 | ✅ done                                           |
| 8     | Agent-chrome flagship                  | ✅ done                                           |
| 9     | Bridge, docs pass, 0.1                 | ✅ done — 0.1 gated on qwertty publish + ADR 0014 |

Arc 3 flagship progress (updated 2026-07-07): slice 1 (extraction, replay, persistence) ✅; slice 2
(Anthropic wire) ✅ SSE decoder + HTTP client, real API default — live-smoke-confirmed by the author;
slice 3 (markdown) ✅ headings/bold/italic/strikethrough, inline+fenced code, ordered/nested lists,
links; slice 4 (tools) ✅ `read_file`/`list_dir` (cwd-confined), confirmation modal, tool-call cells,
continuation loop (`docs/design/arc3-slice4-tools.md`) — four wire details flagged for a
live smoke test; slice 5 (chrome: keymap, help overlay, theme file, browse polish) 🔨 in progress;
slice 6 (widget extraction + e2e tapes) ⬜. Content model is now typed blocks
(text/thinking/tool_use/tool_result), wire-shaped and doubling as persistence.

## Arc 2 — make the product match the architecture (adjudicated 2026-07-07, expanded same day)

Flagship decision (author): **the agent client** — the app whose requirements are the library's
pitch. The arcs below are sized for continuous work (no stopping points); ordering is driven by the
three field documents (both field reports and the terminal gap analysis) and the middle-piece audit.
Standing non-functional rule (author): **examples and apps must look good, coherent, well laid out,
well themed — and achieving that must be easy.** Every arc carries that requirement; Arc 2A exists
to make it structural.

### Arc 2A — aesthetics as a system (make "looks good" the default)

Execution plan: `docs/plans/arc2a-aesthetics.md` (decisions pre-made; read it before starting).

| Item                                                                                  | Status |
| ------------------------------------------------------------------------------------- | ------ |
| Panel widget (bg fill, border, title, padding) + center/inset layout helpers          | ✅     |
| All nine examples restyled to README-screenshot bar (betamax PNGs as acceptance)      | ✅     |
| Nord + Dracula presets ✅; Theme::default retune + role coverage audit                | ✅     |
| Design tokens beyond color: spacing/density constants (`rabbitui_core::spacing`)      | ✅     |
| Screenshot pipeline: tapes render the README/gallery images (just target)             | ✅     |
| Gallery example: every widget, every theme, one screen (doubles as visual regression) | ✅     |

**Arc 2A complete.** Follow-on `Update::set_theme` (runtime theme switching, mirrors `set_mode`) —
surfaced by the gallery — is now also done (Arc 4 item 9): the gallery's number keys 1–4 switch
theme live.

### Arc 2B — the binding constraint (unblocks the flagship)

| Item                                                                                                                                                      | Status |
| --------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| `desired_height(width)` intrinsic measurement in the widget contract + layout                                                                             | ✅     |
| ScrollView container (consumes visibility requests; keyboard+wheel; scrollbar)                                                                            | ✅     |
| Styled-span Text (unify core::text spans with widget text; wrap included)                                                                                 | ✅     |
| Attrs::remove / BitAnd / Not                                                                                                                              | ✅     |
| Logging/tracing seam: `tracing` subscriber writing into a framework buffer + an overlay                                                                   | ✅     |
| Benchmark harness: view-construction + full-frame + diff costs on large synthetic views (tests ADR 0001's "microseconds" claim — its own revisit trigger) | ✅     |
| Block-level early commit for bounded inline tails (moved to the deferred ledger)                                                                          | ⬜     |

### Arc 3 — the flagship: a real agent client (`rabbit` — name TBD by author)

Execution plan: `docs/plans/arc3-agent-client.md` (backend trait, wire shape, slice order — all
decided there). Grows from examples/agent.rs into its own crate/binary, maintained permanently as
the acceptance
test (prior-art's survival law). Anthropic-API-backed with a fake/replay backend for tests and
demos. Items: streaming markdown over a real wire; tool-call cells with live status; session
transcript persistence + resume; keybinding help overlay; theme file support end to end;
inline-first with alt-screen browse mode; betamax tapes as its e2e suite; the audit's markdown
widget and modal/menu widgets get built HERE and extracted to the catalog once proven (toolong's
lesson inverted: extract from the app, don't guess in the library).

### Arc 4 — non-functional spine (the audit's cross-cutting list, scheduled)

Execution plan: `docs/plans/arc4-spine.md` (a design position per item; qwertty adoption order and
gates live there).

| Item                                                                                                                                                               | Status |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------ |
| Error story: update/view panic policy, EffectFailed UX pattern, ErrorBanner widget + effect-panic restore-hook guard                                               | ✅     |
| Suspend/resume + $EDITOR handoff surface (qwertty RestoreHandle now delivered; suspend API is qwertty M6, in flight — our wiring waits on it)                      | ⬜     |
| Keybinding/config layer: declarative keymap, user remapping, help overlay generated from it                                                                        | 🔨     |
| Performance: budget assertions in CI from the 2B harness; CompactString cell optimization                                                                          | 🔨     |
| Accessibility groundwork: roles/labels on specs recorded into facts (the a11y export needs them; both field reports name a11y the likely architectural tiebreaker) | ✅     |
| Key/WidgetId debuggability: recover names for devtools + a11y (interning or label capture)                                                                         | ✅     |
| Devtools: facts inspector (dump the frame facts tree, live overlay toggle)                                                                                         | ✅     |
| qwertty Phase 3 adoption: FakeDevice in testing, RestoreHandle, then the KeyEvent/TextPayload pre-pin migration                                                    | 🔨     |

Status notes (2026-07-07): **a11y**, **widget-id debuggability** (devtools-gated source-name capture),
and the **devtools inspector** (`FactsInspector` + `facts::dump()`) landed. **Keymap** 🔨 is the
flagship's in-app keymap (Arc 3 slice 5); the framework generalization is still open. **Performance**
🔨 = iai-callgrind benches + CI workflow authored but **unverified locally** (valgrind is Linux-only).
**qwertty adoption** 🔨 = typed mouse events adopted in the input bridge; the KeyEvent/TextPayload
migration and `/dev/tty` backstop removal are next — unblocked now that qwertty delivered
RestoreHandle/FakeDevice and froze the event vocabulary (ADR 0019). **Suspend** waits on qwertty M6.

### Arc 5 — field leadership (what the field reports say would move the field)

Execution plan: `docs/plans/arc5-field.md`.

| Item                                                                                                                                                           | Status |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Public conformance story: PTY-matrix harness grown from our vt100+betamax layers; publish results (both reports: "whoever publishes the harness sets the bar") | ⬜     |
| Inline mode as a named, specified discipline: extract our invariant into a standalone doc/spec others can implement (Fable report move #2)                     | 🔨     |
| Comparisons: the same app in ratatui / Bubble Tea / Textual / rabbitui, honestly written                                                                       | ⬜     |
| Agent-legibility: shipped agent skill for rabbitui + evals (ratatui-kit precedent)                                                                             | 🔨     |
| Concept docs/book (ratatui.rs-style site) once 2B/3 stop moving the API                                                                                        | ⬜     |
| Terminal-gap advocacy: file the gap-analysis shortlist as upstream issues (regions, scroll decoupling) where maintainers engage                                | 🔨     |
| CI growth: msrv job, cargo-semver-checks, release automation, tape job when qwertty publishes                                                                  | ⬜     |

Status notes (2026-07-07, all 🔨 pending author review before anything is posted/filed): the
**inline-mode spec** gained a conformance-corpus section; the **agent skill** (`skills/rabbitui/SKILL.md`)
plus eval prompts are written (eval runs deferred — needs fresh agents); three **terminal-gap issue
drafts** are in `docs/upstream-issues/`.

Sequencing (updated 2026-07-07): 2B is done; Arc 3 is unblocked and starts next, in vertical slices
like Arc 1, alongside the 2A remainder; Arc 4 items slot in whenever an Arc 3 slice needs them
(error story and keymaps are pulled early by the flagship); Arc 5 items are parallel-friendly
docs/harness work for gaps between build slices. **Session hand-off note:** the per-arc execution
plans in `docs/plans/` (start with `00-execution-playbook.md`) carry the pre-made design decisions
and working method — read the playbook first in every new session.

Known deferred items (tracked in design-note deltas): buffer-level layer compositing (ADR 0003
amendment pending), block-level early commit for streaming, virtualized transcript, per-terminal
wheel normalization, hardware-cursor via facts, WidthPolicy seam (waits on qwertty Phase 3),
kitty-shaped KeyEvent adaptation (pre-pin blocker), macOS /dev/tty workaround upstreaming. Slice-8
strain findings (slice-9 inputs): variable-height measurement + a real scroll container (the
fixed-slot Collapsible stack wastes rows), styled-span soft wrap (Text takes one style while commits
are `Vec<Span>` — styling pops at commit), Attrs::remove, block-level early commit for bounded
tails.

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
