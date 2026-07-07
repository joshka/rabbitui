# rabbitui — Design

Status: living document. Each decision below is codified in an ADR under `docs/adr/`; this file is
the narrative summary and stays current as ADRs land or are superseded. Evidence citations live in
the ADRs and the research memos (`docs/research/`), not here.

Date: 2026-07-06

## The shape of the thing

rabbitui is an async-first terminal UI framework built on the qwertty substrate, designed around one
core contract (the _declared frame_) with optional programming-model shells layered above it. It
targets the workload of the era — streaming, inline-capable, interaction-heavy apps (the
coding-agent CLI being the canonical case) — while remaining a general TUI framework. Its
differentiators, in priority order: interaction correctness proven at the PTY level, a real widget
catalog under one contract, inline mode as a peer of alt-screen, and API ergonomics good enough that
both humans and coding agents get it right on the first try. Architecture novelty is explicitly not
a goal; the research shows it buys nothing (rooibos, and the 2024–26 wave generally).

## The core decision: a declared-frame architecture (ADR 0001)

Every frame, the app declares the UI by rendering widget _specs_ into a frame builder. The framework
retains what must persist; the app retains what it owns. Concretely:

- **App state is plain Rust owned by the app.** The framework never owns, wraps, lenses, or adapts
  application state. The app is free to be an async state machine consuming messages — the model the
  maintainer's own experiments (ratatui-labs) validated and the one that composes best with async
  Rust.
- **Widgets have stable identity (ADR 0002).** Every widget instance is addressed by a `WidgetId`
  derived from user keys composed into id-paths (parent/child nesting, Xilem-style). The framework
  keeps a per-ID state store across frames: focus, scroll offsets, cursor, collapsed/expanded,
  reported extents, caches. Identity is the one problem the research says transfers from GUI
  undiminished; it is framework-owned here, from v0.1. (The declared-frame contract is ADR 0001;
  framework-owned identity and the per-ID store are split out into their own decision, ADR 0002.)
- **Rendering produces facts.** A frame render emits, besides cells: hit regions, focus order,
  cursor candidates, extents, and visibility requests — a queryable record of what was actually
  shown ("frame facts").
- **Input routes through the previous frame's facts.** Events run capture → target → bubble against
  the facts tree; controls consume events and return typed _outcomes_ (Submitted, SelectionChanged,
  Dismissed…) to the app on the next update.
- **Effects are app-owned.** Async work is futures/streams whose results re-enter the loop as
  messages. Commands only — no subscription primitive (Bubble Tea deleted theirs in 2020 and never
  missed them). Panics in effect tasks are caught; the terminal is always restored.

Why this over the two rivals the research left standing:

- vs **Xilem-style view/element split**: the split's payoff is incremental view computation and
  identity. On a ≤100k-cell grid, full re-render is microseconds — the incrementality payoff mostly
  evaporates — and identity is delivered by the ID store without a retained element tree. The
  split's cost (three parallel trees, view-state plumbing, a diffing contract every widget must
  honor) is real and permanent. A memoization layer can be added _within_ this model later
  (per-widget memo nodes) if profiling ever demands it.
- vs **retained tree + reactive attributes (Textual's model)**: proven at app scale, but every Rust
  instance of a retained public tree pays the borrow-checker tax in API shape (Cursive's
  deferred-callback vocabulary, `Arc<Mutex>` per node). Retention also does not buy partial redraw
  (Cursive redraws everything anyway). The declared-frame model keeps the retained _data_ (ID
  store + facts) without a retained _object tree_ as the public contract.

Costs accepted, documented honestly: routing uses one-frame-stale facts (identical to hit-testing
against the last paint in every GUI; immaterial at terminal event rates), and there is no automatic
fine-grained invalidation (mitigated upstream by cheap redraws, and later by opt-in memoization). An
optional thin MVU shell (`rabbitui-tea`, later) serves Elm-preferring users without owning the loop
or the widget contract.

## Rendering (ADR 0003) and screen modes (ADR 0013)

Cell buffer, ratatui-compatible in shape (`Cell` = grapheme + style; wide-grapheme skip cells).
Widgets paint into z-ordered layers; layers composite into one buffer; the composited buffer is
double-buffer diffed and emitted inside synchronized-output (mode 2026) framing. No damage regions —
the diff _is_ the damage tracking. Full-repaint escape hatch for desync recovery.
Segments/styled-runs were considered as the paint primitive and rejected: cells win on diff
simplicity and ratatui interop; run-merging lives in the encoder as an optimization.

Inline and alt-screen are **peer modes, runtime-switchable**. The renderer invariant in inline mode:
an append-once scrollback-commit channel plus a bounded live tail that never exceeds viewport
height. The terminal keeps ownership of scrollback, selection, and copy — Codex tui2 priced the
app-owned-viewport alternative and we decline it as a default; an owned-viewport mode may arrive
later as an explicit opt-in with its costs documented.

## Layout (ADR 0004)

Two-dimensional tree layout with text measurement built in: widgets expose intrinsic measurement
(`desired_height(width)` and friends — the primitive tui2 had to bolt onto ratatui), containers use
a constraint/flex vocabulary (Length/Min/Max/Fill/Ratio) with exact rational arithmetic for
fractional splits (Textual's 1-cell-gap fix). No constraint solver (cassowary is rejected by every
memo that mentions it). No flexbox in core; a taffy adapter can be a separate crate if demand
appears. Layout runs per frame; input-keyed memoization if profiling demands.

## Concurrency and the event loop (ADR 0005)

Async-first on tokio. The framework owns the loop by default: a `select!` over qwertty's
`next_event()`, timers, and the app message mailbox, driving update → layout → render → diff →
write. The update/layout/paint core is strictly synchronous and single-threaded (xi-editor's
async-boundary lesson). Frame scheduling steals tui2's coalescing `FrameRequester` shape: many tasks
may request a redraw; frames are coalesced and rate-limited with a trailing flush. "Bring your own
loop" is the escape hatch, not the default (the ecosystem fragmentation around ratatui's missing
loop is the negative proof). Widget crates stay runtime-free; only the runtime crate touches tokio.

## Input, focus, events (ADR 0006)

Capture → target → bubble over the facts tree. Focus is framework state keyed by WidgetId, with
traversal order derived from frame facts; focus is addressable by ID. Kitty keyboard protocol
negotiated at startup (query + timeout, libvaxis-style burst), mouse hit-testing against the facts
hit map, bracketed paste aggregated, wheel/trackpad normalization informed by the tui2 scroll study.
IME/preedit is acknowledged as a substrate gap; tracked in the qwertty requirements handover.

## Styling and theming (ADR 0007)

Typed styles plus semantic theme tokens, resolved framework-side. Widgets reference roles (`accent`,
`surface`, `danger`…), themes map roles to concrete styles; Catppuccin Mocha ships today, Nord /
Dracula land with Arc 2.2 presets ship in v0.1 — "pretty by default" is a requirement, per the wave.
Theme files are TOML and hot-reloadable in debug builds (live reload is Textual's #1 adoption
driver; we get it without a CSS engine). A cascade/selector engine is deliberately deferred —
Brick's evidence says role-based theming covers ~90% at ~10% of the machinery; the ADR records what
demand signal would justify revisiting.

## Widgets and the extension surface (ADR 0008)

One public widget contract in `rabbitui-core` from v0.1 (the WidgetRef-outside-core pain is the
negative proof). A widget is a _spec_ (declarative, short-lived) rendered against framework-owned
per-ID state, returning outcomes; render output is rich (cells + cursor candidates + extents +
focusability + hit regions — Brick's insight, so third-party widgets get focus, scroll-into-view,
and mouse support for free). Scrollable containers are virtualized from day one with a pluggable
lazy backend (Textual's 800× DataTable gap and Brick's #1 perf complaint are the proofs). The
catalog lives in `rabbitui-widgets`, versioned with the workspace; the contract is designed for
third-party crates and for coding agents (small, inferrable grammar; a shipped agent skill once the
API settles).

## Testing (ADR 0009)

`rabbitui-testing` ships before the catalog grows: a headless driver (inject events, advance an
injectable clock, run frames, assert on buffers; snapshot tests with an update flag) plus a
PTY-level harness asserting on _emitted escape sequences_ through a vt100 parser (tui2's finding:
the bugs that matter live at the ANSI layer, below buffer equality). Every widget in the catalog
carries both kinds of tests; the harness is public API so third-party widget authors and coding
agents can verify their own output.

## Ratatui interop (ADR 0010)

Soft goal, bridge-based: construct a ratatui `Buffer`, render any ratatui `Widget` into it, copy
cells into a rabbitui layer (cell models kept convertible by construction). rabbitui widgets never
require ratatui. The bridge ships as `rabbitui-ratatui`, giving day-one access to the existing
widget ecosystem without constraining our buffer design.

## Substrate (ADR 0012)

qwertty, as a git dependency behind a single-file seam (`terminal.rs`) targeting
`TokioTerminalSession`. Gaps (styling/SGR, alt-screen, mouse, paste, kitty keyboard, mode 2026) are
bridged short-term by an SGR/mode encoder inside rabbitui using qwertty's raw-bytes escape hatch —
but input decoding is never forked (qwertty's InputEvent is declared high-churn; we consume, not
duplicate). Width/grapheme measurement is owned by rabbitui in one oracle module shared conceptually
with the substrate (never two width tables). Capability negotiation: batched startup probe
(DA1-fenced, timeout-bounded) into a `Capabilities` struct that styling and rendering consume for
degradation (truecolor → 256 → 16).

## Crate layout (ADR 0011)

Cargo workspace: `rabbitui-core` (ids, facts, buffer, style, layout, widget contract — no runtime
deps), `rabbitui-widgets`, `rabbitui-testing`, `rabbitui-ratatui` (bridge), and `rabbitui` (facade:
the tokio runtime/loop + re-exports; the crate users depend on). Optional shells (`rabbitui-tea`)
come later. Edition 2024; MSRV = stable minus one, bumped only on minor releases;
BREAKING-CHANGES.md discipline copied from ratatui.

## Positioning (ADR 0014 — decision deferred by design)

rabbitui builds standalone. Whether it ultimately ships as/under the reserved `ratatui-*` names (the
maintainer's own protective reservations) is a positioning decision deferred until ~0.1: the
adoption evidence (org gravity vs unaffiliated caps) is recorded in the ADR, and the architecture
keeps the option open (ratatui-compatible cell model, bridge crate, no naming assumptions in the
API).

## Non-goals (unchanged from the brief, plus wave additions)

Cross-platform GUI rendering, web/SSH serving, animation framework, games/pixel engines,
editor-internals toolkits, embedded pixel displays, TUI-in-TUI embedding. Windows support is
deferred (ConPTY VT-only makes it nearly free architecturally; it waits on the substrate).
Accessibility export is _tracked, not shipped_: the identity layer and facts tree are designed so an
AccessKit-style export is possible (stable IDs, roles on specs), because the research marks a11y as
the most likely future forcing-function — but no AT integration is promised before the core proves
out.
