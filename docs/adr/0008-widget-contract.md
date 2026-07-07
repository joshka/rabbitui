# ADR 0008: One widget contract in core — specs, rich render output, outcomes, virtualization

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

A widget contract is the framework's single most consequential public surface. It is what
third-party crate authors implement, what the built-in catalog is built against, and — new
in this era — what coding agents must infer and produce correctly. Getting its *shape* and
its *home crate* wrong is expensive and near-unrepairable once an ecosystem depends on it.

Four forces bear on the design:

1. **The trait-zoo negative proof.** Ratatui's render surface accreted into four traits —
   `Widget` (consumes `self`), `StatefulWidget` (`self` + `&mut State`), and unstable
   `WidgetRef`/`StatefulWidgetRef` for `&self` rendering and `Box<dyn WidgetRef>` collections
   (tracking issue [#1287](https://github.com/ratatui/ratatui/issues/1287)). The evolution
   hurt: 0.26 added `impl Widget for &W` and introduced `WidgetRef`; 0.30 *reversed* the
   blanket-impl direction, **moved `WidgetRef` out of `ratatui-core`** into the main crate
   behind `unstable-widget-ref`, and gated `Frame::render_widget_ref` behind a new `FrameExt`
   trait (`BREAKING-CHANGES.md:311-347`). Docs now recommend three patterns depending on
   mutability. Root cause: consuming-`self` render was baked in early, so dyn-compatibility
   was retrofitted, and the third-party trait surface ended up *outside* the stability-anchor
   crate (`docs/research/ratatui.md`). Both mistakes are ours to not repeat.

2. **Render-as-facts, not render-as-pixels.** Ratatui's `Widget::render(self, area, buf)`
   writes cells and returns nothing; discussion [#552](https://github.com/ratatui/ratatui/discussions/552)
   enumerates the five gaps that fall out of this (no post-render geometry, no hit-testing, no
   focus, no compositing, no event handling), and the maintainer concedes every one. Brick
   solved this two decades ago in Haskell: a widget's render returns a `Result { image, cursors,
   visibilityRequests, extents, borders }` — cursor candidates, scroll-into-view requests, and
   reported geometry all *bubble up* through containers, which translate offsets
   (`docs/research/brick.md`, `Internal.hs:359-395`). This richer currency is precisely what
   lets a *third-party* widget get focus, scroll-into-view, mouse hit-testing, and border-joining
   for free instead of reimplementing each.

3. **Virtualization is a day-one contract property, not a widget feature.** The two loudest
   real-world perf complaints in the whole survey are both retrofit failures. Brick issue #534:
   a naive 500-row table's "rendering performance absolutely plummets," and the built-in `List`
   virtualizes only by *requiring uniform item height*; anything else means writing a custom
   `Widget` against `RenderM` (`docs/research/brick.md`). Textual's flagship complaint is the
   built-in DataTable at ~63s to load 538k rows vs 0.077s for `textual-fastdatatable` (~**800×**)
   because the built-in eagerly ingests into Python objects instead of a pluggable columnar
   backend; even Textualize's own `toolong` bypasses the stock scrollables for a custom
   ScrollView (`docs/research/textual.md`). If the *contract* doesn't make lazy row/line
   provisioning the natural path, every data widget re-hits this wall.

4. **Agent-legibility is a design constraint, not a nicety.** ~A third of the 2024–26 wave is
   AI-generated, and agent-CLI vendors keep extracting in-house frameworks; frameworks now design
   for AI *authors* (SuperLightTUI's "small public grammar… easily inferrable from documentation,"
   ratatui-kit ships and evals an agent skill). The wave's sharpest failure mode is "breadth
   without interaction correctness" — FrankenTUI "looks like it's working… once I tried
   interacting with it, everything is broken in a subtle way" (`docs/research/recent-rust-tui-wave.md`).
   A contract an agent can infer *and get right on the first try* is a competitive moat.

## Options considered

### A. Multi-trait zoo (ratatui's shape): separate consuming/stateful/ref traits

*What it is:* a base `Widget` consuming `self`, a `StatefulWidget` taking `&mut State`, plus
`Ref` variants for `&self` / boxed collections, as need is discovered.

*Steelman:* each trait is minimal and honest in isolation; `StatefulWidget`'s "app owns the
state struct, widget borrows it for one frame" is the cleanest low-level state story anywhere,
trivially testable with no hidden framework state (`docs/research/ratatui.md`). Consuming `self`
lets a widget move its data into the render with zero clones.

*Why not chosen:* it is the documented negative proof (Force 1). Consuming-`self` render is
structurally dyn-*in*compatible, so the moment you want `Box<dyn Widget>` (catalogs, layered
children, containers over heterogeneous widgets) you retrofit ref-traits, reverse blanket impls,
and split the surface across crates — 74 documented breaking changes, the `WidgetRef` saga among
the worst. Every widget author re-derives a mutability decision tree. We pay this cost once, at
design time, by choosing a single dyn-compatible shape.

### B. Retained public widget tree (Cursive/Textual/Masonry shape)

*What it is:* widgets are persistent objects implementing `draw/layout/on_event/children`; focus,
scroll, and identity are node references that survive repaints.

*Steelman:* focus and identity become "nearly free" — a stable reference, not a hashed ID to
reconcile (`docs/research/textual.md`); Textual proves the model scales to harlequin-sized apps.

*Why not chosen:* this is ADR 0001's decision (declared-frame, not retained public tree). Cursive
is the Rust existence proof *and* the scar tissue: a single owned tree forbids view-to-view
references, which "generates the entire rest of the API" — string-name selectors, per-node
`Arc<Mutex>` (documented double-borrow panics and silent `try_lock` no-ops), deferred-callback
vocabulary (`docs/research/cursive.md`). We keep retained *data* (the per-ID state store and frame
facts of ADR 0001) without a retained public *object tree* as the contract. The widget contract
therefore describes a short-lived spec, not a long-lived node.

### C. One spec-shaped contract in core with rich render output *(chosen)*

*What it is:* a single dyn-compatible trait in `rabbitui-core`. A widget is a **spec**
(declarative, short-lived, built each frame) that renders against **framework-owned per-ID state**
and returns **typed outcomes**; its render emits a **rich output struct**, not bare cell writes.
Scrollable containers are **virtualized from day one** behind a pluggable lazy backend.

*Steelman + why chosen:* it is the only option that satisfies all four forces at once. It inherits
ADR 0001's declared-frame model (spec, not node); it steals Brick's `Result` (the single best idea
in TUI-land) as the composition currency so third-party widgets compose properly; it is
dyn-compatible by construction so there is no ref-trait future; and it makes lazy provisioning the
default path so the DataTable/500-row walls never form. It is small enough to be agent-inferrable.

## Decision — precise, normative statements

**One trait, in core, dyn-compatible.** `rabbitui-core` defines exactly one public widget trait
from v0.1. It renders through `&self` (or `&mut self` where a widget legitimately mutates its own
transient spec fields) — never by consuming `self`. It is object-safe: `Box<dyn Widget>` collections,
heterogeneous container children, and stored specs are first-class, requiring **no** parallel
`Ref`/`Stateful*` traits. Anything a third-party widget must implement lives in `rabbitui-core`, the
stability anchor — never behind an `unstable-*` gate in a downstream crate (the `WidgetRef`-outside-core
mistake is not repeated).

**Spec / state / outcome are three separated concerns.** (1) A widget is a *spec*: a short-lived,
declaratively-constructed value describing what to show this frame, holding borrowed or owned app data,
carrying an optional user *key* that composes into the `WidgetId` id-path of ADR 0001. (2) Cross-frame
*state* — scroll offset, cursor, collapsed/expanded, focus, reported extents, caches — is **owned by the
framework** in the per-ID store (ADR 0001), never by the widget object and never wrapped/lensed out of app
state. The render method receives a `&mut` handle to *its own* ID-keyed state slice. (3) A widget returns
typed *outcomes* (`Submitted`, `SelectionChanged`, `Dismissed`, …) that reach the app on the next update;
outcomes are the sanctioned mutation path (Cursive's deferred-callback lesson, `docs/research/cursive.md`),
so no widget ever holds `&mut App`.

**Render output is rich, not cells-only.** A render produces a structured value carrying, besides styled
**cells**: **cursor candidates** (position + shape; the app/framework picks the winner, Brick's
`appChooseCursor` split), **extents** (the post-layout `Rect` recorded per `WidgetId`), **focusability**
(whether this node enters the focus chain, plus a can-focus-children flag — Textual's two-flag model,
`docs/research/textual.md`), **hit regions** (id-tagged rectangles the input router in ADR 0006 tests
against), and **visibility requests** (scroll-into-view, bubbling to the nearest enclosing viewport). These
fields *are* the "frame facts" of ADR 0001; emitting them is the contract, so focus, scroll-into-view, and
mouse support are inherited by every conforming third-party widget rather than reimplemented. Border-join
metadata is a candidate extension, tracked but not required in v0.1.

**Virtualization from day one, pluggable backend.** Scrollable containers are virtualized in the core
contract, not as a catalog widget feature. A viewport allots its child a size *during build* and the child
provisions only visible items through a lazy **item/row/line provider** trait (lazy, index-addressable,
columnar- and mmap-friendly). Variable-height items are supported via a height-estimate plus a
measured-height cache (never Brick's uniform-height requirement). The built-in provider is eager for small
data; third parties (and the catalog's `DataTable`) supply columnar/streaming providers without touching
the widget.

**Designed for agents.** The contract is a small, inferrable grammar: one trait, one spec pattern, one
outcome enum vocabulary, one render-output struct. A shipped, evaluated agent skill follows once the API
settles (per ADR 0009's public headless + PTY harness, agents can verify their own widget output).

**Home.** The contract, `WidgetId`, frame facts, buffer, style, and layout live in `rabbitui-core` (no
runtime deps). The catalog lives in `rabbitui-widgets`, versioned with the workspace (ADR 0011).

## Consequences

*Positive.* Third-party widgets get focus, scroll-into-view, hit-testing, and cursor handling for free by
emitting facts — the #552 gap-list closes at the contract layer. One dyn-compatible trait eliminates the
ratatui trait-zoo and its blanket-impl-reversal class of breaking changes before it can start. Virtualized
data widgets never hit the Brick-500-row or Textual-800× walls. Framework-owned per-ID state removes
ratatui's `StatefulWidget` state-threading boilerplate and Cursive's `Arc<Mutex>`/string-selector debt. The
grammar is small enough for agents to author correctly.

*Negative (honest).* The rich render-output struct is a wider, heavier return type than ratatui's
write-into-`Buffer` — more per-widget bookkeeping and allocation than bare cell writes, justified only
because the facts are load-bearing for interaction. `&self`/`&mut self` render forbids the zero-clone
"move data into the render" that consuming-`self` allowed; widgets borrow instead. A single trait must
serve both a one-line label and a virtualized table, so the trait carries surface (default-implemented
methods) that trivial widgets ignore — breadth that must be taught well to stay legible. The
provider-trait indirection adds a hop that eager, small-data widgets do not need.

*Neutral.* The contract assumes the declared-frame model of ADR 0001; it is not portable to a retained-tree
framework. Virtualization is *available* everywhere but obligatory nowhere — a non-scrolling widget ignores
it. Border joining is deferred to a later extension of the same struct, not a redesign.

## Revisit triggers

- **Object-safety forces a split anyway.** If a compelling widget genuinely cannot be expressed
  dyn-compatibly (e.g. a return-position generic the trait can't erase), and the workaround is worse than a
  second trait, reopen — but treat ratatui's history as the prior.
- **The render-output struct proves too heavy.** If profiling shows the rich-facts return dominating frame
  cost for facts-light widgets, consider a split "cells-only fast path + opt-in facts" — measured, not
  assumed.
- **The single provider abstraction can't span use cases.** If columnar (DataTable), line-oriented (log
  viewer), and tree (outline) virtualization cannot share one provider trait without contortion, split the
  provider — but only the provider, not the widget contract.
- **Agents systematically misuse the contract.** If eval results on the shipped agent skill show a specific
  part of the grammar is reliably gotten wrong, that part is a legibility defect to redesign, not document
  around.
- **Border joining or a11y roles need to be mandatory.** If compositional border joining
  (`docs/research/brick.md`) or AccessKit-style role export (the survey's most likely forcing-function,
  `docs/research/recent-rust-tui-wave.md`) must become required render-output fields, revisit the struct's
  required-vs-optional boundary.
