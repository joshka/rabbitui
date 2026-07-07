# ADR 0004: Intrinsic-measurement constraint/flex layout, no solver, no flexbox in core

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

- **The measurement gap is the real hole, not the vocabulary.** Ratatui layout is strictly
  1-D constraint splitting with *no text measurement*: solved sizes ignore content size, so
  labels, wrapping, and intrinsic sizing fall outside the model (`docs/research/ratatui.md`;
  ratatui discussion [#552](https://github.com/ratatui/ratatui/discussions/552) item 1). A
  *production* coding-agent TUI proved what fills it: codex tui2 bolted `desired_height(width)`
  onto ratatui because it could not do anything real without it — trait
  `fn render(&self,area,buf); fn desired_height(&self,width:u16)->u16; fn cursor_pos(...)`,
  with `ColumnRenderable` (stacks by desired height) and `FlexRenderable` ("loosely inspired
  by Flutter's Flex," flex factors, last-child-gets-remainder) on top. `desired_height(width)`
  was tui2's single most reused new primitive (`docs/research/codex-tui2.md`).
- **The constraint *vocabulary* is loved; the *solver* under it is a documented failure.**
  Users know and like `Length/Min/Max/Percentage/Ratio/Fill + Flex` (`docs/research/ratatui.md`),
  but cassowary is the clearest cautionary tale: RFC
  [#1933](https://github.com/ratatui/ratatui/discussions/1933) records cassowary-rs
  unmaintained since 2018, inconsistent expansion, and **panics under overconstraint**,
  forcing the `kasuari` fork (`docs/research/prior-art.md`). Rejected by every memo that
  mentions it.
- **Flexbox works, but nobody in this ecosystem needs it in core.** Ink's five years of
  issues have layout-semantics complaints "approaching zero" — pain is 100% paint/erase,
  "constraint solvers never came up as a want" (`docs/research/ink.md`). taffy works (iocraft,
  rooibos), but rooibos with the full taffy/signals stack has 5 stars — the engine was never
  the product; ratatui-labs *deliberately discard* "Yoga/Flexbox as the default layout
  dependency" (`docs/research/prior-art.md`). Textual's whole shipped ecosystem (harlequin,
  posting, toolong) ran on dock + linear + grid + `fr`; "nobody missed flexbox wrap or
  cassowary" (`docs/research/textual.md`).
- **Fractional splits leave gaps unless the arithmetic is exact.** Textual uses Python's
  `fractions.Fraction` so `1fr 1fr 1fr` never leaves a 1-cell gap column
  (`docs/research/textual.md`, "7 things I've learned"). A correctness property, not a nicety.
- **Layout must feed input, focus, and scrolling.** Brick records a post-layout `Rect` per
  name (`reportExtent`); mouse/focus/scroll-into-view derive from it (`docs/research/brick.md`).
  rabbitui's declared-frame model (ADR 0001) makes extents part of "frame facts," so layout
  output must be geometry the facts tree carries, not just a paint side effect.

## Options considered

**A. Cassowary / constraint solver as the engine (rejected unanimously).** *What:* linear
constraints with strength tiers, solved per split (ratatui's `kasuari`-backed
`Layout::split`, `docs/research/ratatui.md`). *Steelman:* expressive, familiar, handles
arbitrary mixed constraints uniformly; ratatui shipped thousands of apps on it. *Why not:*
upstream is dead and the fork exists only because cassowary-rs was unmaintained, expands
inconsistently, and **panics under overconstraint** (RFC
[#1933](https://github.com/ratatui/ratatui/discussions/1933)); it also cannot see content
size, the exact gap tui2 worked around (discussion
[#552](https://github.com/ratatui/ratatui/discussions/552)). The labs call it "a documented
failure" (`docs/research/prior-art.md`). Keep the *vocabulary*, discard the *solver*.

**B. taffy / flexbox in core.** *What:* CSS flexbox (taffy, or Yoga as Ink does) as the core
2-D engine with text measure functions. *Steelman:* battle-tested, chosen by iocraft/rooibos/
Ink, gives wrap/grow/shrink/alignment free; Ink shows layout semantics essentially never
complained about (`docs/research/ink.md`). *Why not:* it buys expressiveness the observed
workloads do not spend — Textual's ecosystem shipped without it, ratatui-labs discarded it as
default, and rooibos (5 stars, full taffy stack) shows the engine is not the product
(`docs/research/prior-art.md`, `docs/research/textual.md`). A flexbox dependency in
`rabbitui-core` is a heavy, permanent commitment in the one crate that must stay small and
stable for third-party authors (ADR 0011). We route around it with an adapter, not a bake-in.

**C. Textual dock + `fr` + grid, ported wholesale.** *What:* Textual's `vertical`/
`horizontal`/`grid` + `dock` (pin to an edge) + `offset`, `fr` units, `auto` sizing
(`docs/research/textual.md`). *Steelman:* this modest vocabulary covered "essentially every
Textual app ever shipped"; dock gives free status bars/sidebars; `fr` + exact fractions solve
the gap bug. *Why not as-is:* it is coupled to a retained DOM + CSS layer rabbitui will not
have (ADR 0001, ADR 0007). We take the *lessons* — exact rational `fr`, a small vocabulary,
dock/linear/grid suffice — in a constraint/flex vocabulary users know from ratatui.
`dock`-style edge pinning is expressible as `Length` on the leading/trailing child; dedicated
dock sugar is a later addition if demanded.

**D. Ratatui 1-D constraints, unchanged.** *What:* split one Rect along one axis, nest for
2-D, no content measurement (`docs/research/ratatui.md`). *Steelman:* dead simple,
cache-friendly, its `(Rect, Layout) -> Rects` memoization is proven affordable even with a
solver. *Why not:* it is the status quo whose gaps this project exists to close — no intrinsic
sizing (tui2's proof), no content-aware height, 2-D only by manual nesting. We keep its
memoization posture and constraint names but make measurement first-class and layout 2-D.

**Chosen:** a 2-D tree layout where widgets expose intrinsic measurement and containers use a
constraint/flex vocabulary resolved by direct arithmetic (not a solver), with exact rational
division for fractional splits — tui2's `desired_height(width)` + `Column`/`Flex` generalized
to 2-D, wearing ratatui's constraint names, with Textual's exact-fraction fix.

## Decision

1. **rabbitui layout is a two-dimensional tree walk with text measurement built in.** Widgets
   expose intrinsic measurement — `desired_height(width)` and its axis-dual
   `desired_width(height)` / min-content and max-content queries — in the widget contract
   (`rabbitui-core`, ADR 0008). This is the primitive tui2 bolted onto ratatui; rabbitui
   ships it from v0.1. Measurement uses the single width/grapheme oracle shared with the
   substrate (ADR 0012) — never a second width table.
2. **Containers use a constraint/flex vocabulary:** `Length`, `Min`, `Max`, `Fill` (weighted
   flex factor), `Ratio` — the names ratatui users know (`docs/research/ratatui.md`).
   `Fill(n)` is the `fr`/flex-factor unit; the remainder after fixed and content-measured
   children is divided among `Fill` children by weight.
3. **rabbitui does not use a constraint solver.** Layout resolves by direct arithmetic in a
   bounded number of passes (measure intrinsic/`Length`/`Ratio`/`Max`/`Min` children, then
   distribute remaining space to `Fill` children) — Brick's one-pass negotiation and tui2's
   `FlexRenderable`, not cassowary/kasuari. No solver means no overconstraint panic class
   (RFC #1933) and no strength-tier reasoning.
4. **Fractional splits use exact rational arithmetic.** `Fill`/`Ratio` division is computed
   over integers/rationals with remainder distributed deterministically (largest-remainder /
   last-child), so `Fill(1) Fill(1) Fill(1)` across a width never leaves a 1-cell gap —
   Textual's `Fraction` fix (`docs/research/textual.md`), matching tui2's
   "last-child-gets-remainder rounding" (`docs/research/codex-tui2.md`).
5. **No flexbox and no taffy dependency in `rabbitui-core`.** Full CSS flexbox
   (wrap/align/justify/grow/shrink) is not in the core layout model.
6. **A taffy adapter may ship as a separate crate (`rabbitui-taffy`) if demand appears.** An
   escape valve, not a default: it adapts the contract's measure functions to taffy's
   `MeasureFunc` and produces geometry the facts tree consumes. Core never depends on it.
7. **Layout runs once per frame**, after `update` and before paint (ADR 0005 sequences
   update → layout → render → diff → write), producing extents per `WidgetId` recorded into
   frame facts (ADR 0001) that input/focus/scroll-into-view route against (ADR 0006).
8. **Memoization is deferred, not designed out.** Layout is a pure function of
   `(available area, tree of specs+constraints, measured content)` and may be memoized on
   those keys — ratatui's `(Rect, Layout) -> Rects` LRU posture (`docs/research/ratatui.md`)
   — *only if* profiling on a real workload demands it. Default is recompute-per-frame,
   because on a ≤100k-cell grid a full layout pass is microseconds (ADR 0001's cost model).

## Consequences

**Positive**
- Content-aware layout from v0.1: labels size to text, prose measures wrapped height,
  streaming markdown cells report `desired_height(width)` — tui2's validated workload with
  nothing bolted on later (`docs/research/codex-tui2.md`). No overconstraint panic class and
  no unmaintained-solver dependency (RFC #1933). Exact `fr` division kills an off-by-one bug
  class (`docs/research/textual.md`). `rabbitui-core` stays small (ADR 0011), and the familiar
  vocabulary lowers the ratatui adoption cliff while staying a small, agent-inferrable grammar
  (ADR 0008).

**Negative (honest)**
- No flexbox wrap, `justify`/`align`, or shrink in core; apps that want them must pull
  `rabbitui-taffy` (once it exists) or express layout in the constraint vocabulary — if that
  demand turns out common, this is a gap we chose.
- Two-axis intrinsic measurement is more contract surface than tui2's height-only
  `Renderable`, and a wrong measure function yields wrong layout silently (mitigated by
  testing, ADR 0009).
- Hand-rolled distribution must be tested as carefully as cassowary's edge cases were; wrong
  remainder distribution reintroduces the gap bug we claim to kill. Deferring memoization
  means a pathological deep tree could cost more than a memoized solver would — accepted until
  profiling says otherwise.

**Neutral**
- Constraint names match ratatui but the engine differs, so exotic mixed-constraint behavior
  may not be bit-identical; the ratatui *bridge* (ADR 0010) copies cells, not layout, so
  interop is unaffected. `dock`/`offset` are expressible via `Length` + layers today;
  dedicated dock sugar is left open.

## Revisit triggers

- **Flexbox demand becomes concrete:** many real apps/widget authors request
  wrap/align/justify/shrink. Ship `rabbitui-taffy` (the escape valve); reconsider a core
  dependency only if most users choose it.
- **Profiling shows layout is hot** on a real workload (large virtualized table, deep tree,
  high frame rate). Add input-keyed memoization (the ratatui LRU posture), still no solver.
- **The no-solver arithmetic can't express a needed layout** (baseline alignment, cross-axis
  stretch) reported by real widgets — reevaluate B/D scoped to that container, not a rewrite.
- **Measurement contract proves too narrow or too wide:** two-axis measurement consistently
  mis-implemented, or a single-axis (height-only) contract turns out sufficient — revise the
  widget contract (ADR 0008).
- **Exact-fraction distribution still leaves gaps in production** — the arithmetic is wrong
  and must be fixed first.
