# ADR 0010: Ratatui interop as a soft goal via a buffer bridge crate

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

Ratatui is the gravity well of the Rust TUI ecosystem: 36.2M downloads, 13.5M in the last 90 days,
versus 204k for the next-most-adopted framework (tui-realm). Every `docs/research/prior-art.md`
finding points the same way: frameworks that _replace_ ratatui forfeit its widget ecosystem and
network effect and stall at niche adoption (iocraft, r3bl, zi, rooibos); frameworks that _layer on_
it survive but get squeezed as ratatui absorbs features. "Ratatui users demonstrably want to adopt
one concept at a time" (`prior-art.md`), and the single loudest real-world complaint about ratatui
is sparse first-party widgets with third-party version skew (HN 45830829). A brand-new framework
that cannot borrow from that widget zoo starts from zero coverage on day one.

But three forces pull the other way:

1. **rabbitui's buffer must not be hostage to ratatui's design.** ADR 0003 makes cells the paint
   primitive, z-ordered layers the compositor, and mode-2026 synchronized output the wire framing.
   ratatui has none of layers, z-order, signed coordinates, or intrinsic text measurement — the
   exact gaps `docs/research/ratatui.md` and discussion
   [#552](https://github.com/ratatui/ratatui/discussions/552) catalog. Coupling our internals to
   ratatui's `Buffer` would re-import those ceilings.

2. **ratatui widgets are not our widgets.** A ratatui `Widget::render(self, area, buf)` consumes
   `self` by value and emits only cells (`ratatui-core/src/widgets/widget.rs`). It carries no
   identity, no focus, no hit regions, no cursor candidates, no outcomes — the "frame facts" that
   ADR 0001/0008 make the whole point of a rabbitui widget. A ratatui `StatefulWidget` threads
   app-owned state for one frame but still emits only cells. There is nothing to _convert_: the
   value that a rabbitui widget adds lives entirely in the surface ratatui deliberately omits.

3. **The cell models are already shaped alike.** ADR 0003 chose a cell model "ratatui-compatible in
   shape (`Cell` = grapheme + style; wide-grapheme skip cells)" precisely so interop stays a copy,
   not a translation. ratatui's `Cell` stores its symbol as an `Option<CompactString>` with the same
   wide-grapheme skip-cell convention (`ratatui-core/src/buffer/cell.rs`, diff rules at
   `buffer/buffer.rs:471-506`). Style attributes (fg/bg color, bold/italic/underline/reverse
   modifiers) map field-for-field.

The reserved `ratatui-*` names and the standalone-vs-ratatui-org positioning question are out of
scope here — that is ADR 0014, deliberately deferred. This ADR decides only the _technical_ interop
mechanism, which must hold regardless of how positioning lands.

## Options considered

### A. No interop — rabbitui is a closed world

_What it is:_ ship only rabbitui-native widgets; offer no path to render ratatui widgets.

_Steelman:_ the cleanest possible story. No compatibility surface to maintain, no risk of users
mistaking ratatui widgets (identity-less, styling-divergent) for first-class rabbitui ones, no
coupling pressure on the buffer. The catalog (ADR 0008) is meant to be the product anyway; force
everyone onto it.

_Why not chosen:_ it discards the largest strategic asset available to a new entrant. Day one,
rabbitui's catalog is small; ratatui's third-party widget zoo (charts, canvas, calendars,
tui-textarea, sparklines) is enormous. `prior-art.md`'s central lesson is that "wholesale buy-in
loses to ratatui's gravity" and that an incremental adoption path is a survival requirement —
rooibos shipped the architecture without the ecosystem bridge and got 5 stars. Zero-interop is the
rooibos failure mode with extra steps.

### B. Adopt ratatui as the rendering substrate (build on ratatui-core)

_What it is:_ depend on `ratatui-core`, use its `Buffer`/`Cell`/`diff_iter` directly, and render
ratatui widgets natively into our frames.

_Steelman:_ the buffer, cell, wide-grapheme diff, and styled-text types are "done; don't reinvent
them, re-house them" (`ratatui.md`). Free interop, free battle-tested diff semantics, immediate
access to every ratatui widget. This is what tui-realm and rooibos chose.

_Why not chosen:_ it re-imports ratatui's design ceiling. A flat single-plane `Buffer` with unsigned
`Rect` and no z-order (`ratatui.md`; #552 items 2-3) cannot express ADR 0003's layer compositor or
ADR 0013's inline live-tail without the `Clear`-then-overpaint hack that causes the documented
inline-viewport flicker class. It also chains us to ratatui's release cadence and 74-entry
`BREAKING-CHANGES.md` churn history at our most foundational layer. The cell model is worth _copying
in shape_; the crate is not worth depending on structurally.

### C. Bridge crate: render into a ratatui `Buffer`, copy the cells out (CHOSEN)

_What it is:_ a separate `rabbitui-ratatui` crate that constructs a ratatui `Buffer` sized to a
target rectangle, invokes any `ratatui::Widget` (or `StatefulWidget` with caller-supplied state) to
paint into it, then copies each cell — grapheme + converted style — into a rabbitui layer at the
corresponding position.

_Steelman:_ ratatui interop is achieved at arm's length. rabbitui-core never mentions ratatui; the
bridge is opt-in and lives in its own crate (ADR 0011). Because both cell models were made
convertible by construction (ADR 0003), the copy is a per-cell field map, not a re-render or a
semantic translation. Users get day-one access to the ratatui widget zoo — a chart, a canvas, a
third-party widget — as an _escape hatch inside a rabbitui app_, without any of it constraining our
buffer, layer, or event design.

_Why not chosen — its honest cost:_ what crosses the bridge is cells and nothing else. Everything
that makes a rabbitui widget a rabbitui widget stays behind (see Consequences). We accept that: the
bridge's job is coverage of the everyday drawing cases HN users name, not to launder ratatui widgets
into first-class rabbitui citizens.

## Decision

rabbitui treats ratatui interop as a **soft goal**, delivered by a single optional bridge crate.
Normatively:

1. **rabbitui-core never depends on ratatui, and rabbitui widgets never require ratatui.** The core
   widget contract, buffer, cells, layers, and facts are defined without reference to any ratatui
   type. A rabbitui app that uses no ratatui widget pulls in no ratatui code.

2. **Interop ships as `rabbitui-ratatui`** (ADR 0011), a leaf crate depending on both
   `rabbitui-core` and `ratatui`. It is the only place the two type systems meet.

3. **The bridge mechanism is render-into-Buffer-and-copy-cells.** The bridge allocates a ratatui
   `Buffer` for a target `Rect`, calls the ratatui `Widget::render` (or `StatefulWidget::render`
   with caller-owned state) into it, then copies each ratatui `Cell` into a rabbitui layer cell at
   the same coordinate. No ratatui `Terminal`, backend, or draw loop is involved — only the widget's
   paint step and a buffer copy.

4. **Cell-model convertibility is maintained by construction, not by adaptation.** ADR 0003's cell
   model stays field-compatible with ratatui's (grapheme as compact string; wide-grapheme handled by
   skip cells; fg/bg/modifier styles field-mappable). The copy is a total per-cell function with no
   lossy negotiation. It is a standing invariant of ADR 0003 that a change breaking this
   convertibility must be justified against this ADR.

5. **The bridge carries cells only.** Identity, focus, hit regions, cursor candidates, extents,
   outcomes, and theme-role resolution are _not_ produced for bridged content. Bridged ratatui
   widgets are inert rectangles of styled cells within a rabbitui frame.

6. **Interop never becomes a hard constraint on rabbitui's own evolution.** If a future rabbitui
   buffer or style capability has no ratatui analog, the bridge degrades (ratatui cannot express it,
   so it does not round-trip) rather than the capability being withheld. The buffer design leads;
   the bridge follows.

## Consequences

### Positive

- Day-one access to the entire ratatui widget ecosystem as a drop-in drawing escape hatch, directly
  answering the "sparse built-ins + version skew" complaint (HN 45830829) while our own catalog
  (ADR 0008) grows.
- The incremental-adoption path `prior-art.md` names as a survival requirement: a plain
  ratatui/qwertty app can move to rabbitui and keep rendering its existing ratatui widgets through
  the bridge.
- Zero coupling cost on the core: buffer, layers (ADR 0003), inline mode (ADR 0013), and the widget
  contract (ADR 0008) evolve without ratatui in their dependency graph.
- Because convertibility is by-construction, the bridge is small and cheap to keep correct — a
  per-cell copy, not a compatibility engine.

### Negative (honest)

- **Stateful widgets do not fully carry over.** ratatui `StatefulWidget` state (`ListState` scroll
  offset, table selection) is threaded by the _caller_ for one frame and lives in app-owned structs,
  not in rabbitui's per-ID state store (ADR 0001). Bridged stateful widgets keep ratatui's manual
  state-threading ergonomics; they do not gain rabbitui's framework-owned identity, focus
  persistence, or scroll-into-view.
- **Event handling does not carry over at all.** ratatui widgets emit no facts, so bridged content
  has no hit regions, no focus order, no cursor candidates, and returns no outcomes. Input routing
  (ADR 0006) sees a bridged widget as an opaque cell rectangle; making it interactive means wrapping
  it in a rabbitui widget that owns the facts.
- **Styling semantics do not carry over.** ratatui styles are concrete colors/modifiers; rabbitui's
  are semantic theme roles resolved framework-side (ADR 0007). Bridged cells arrive pre-resolved to
  concrete styles and are _not_ re-themed — a bridged widget will not follow a Catppuccin/Nord theme
  switch unless the caller re-styles it. Truecolor → 256 → 16 degradation (ADR 0012) still applies
  at the encoder, but role-based theming does not reach inside a bridged widget.
- A naive user may mistake a bridged ratatui widget for a first-class rabbitui one and be surprised
  it is not focusable or themed. Documentation must frame the bridge as an escape hatch, not a peer
  to the catalog.

### Neutral

- The bridge allocates a transient ratatui `Buffer` per bridged widget per frame. At terminal cell
  counts (≤100k cells) this is negligible, consistent with ADR 0001's microsecond-scale full-render
  assumption.
- `rabbitui-ratatui` tracks ratatui's release cadence and absorbs its churn — but only in this leaf
  crate, insulated from core by the crate boundary (ADR 0011).
- The bridge is unidirectional by default (ratatui → rabbitui). A reverse adapter (expose a rabbitui
  frame as a ratatui `Buffer`) is possible under the same convertibility invariant but is not part
  of this decision.

## Revisit triggers

- **Convertibility breaks.** If ADR 0003's cell model diverges from ratatui's in a way that makes
  the copy lossy or fallible (e.g. a rabbitui-only cell attribute with no ratatui representation
  becomes common), reopen to decide whether the bridge degrades silently, errors, or the divergence
  is reconsidered.
- **Stateful/interactive bridging becomes a top user request.** If users repeatedly demand that
  bridged ratatui widgets participate in focus, hit-testing, or theming, revisit whether a richer
  adapter (wrapping ratatui widgets in fact-producing rabbitui shells for the common cases) is worth
  its complexity.
- **Positioning lands on the ratatui org (ADR 0014).** If rabbitui ships as/under the reserved
  `ratatui-*` names or merges into that org, reconsider whether the bridge should be promoted from a
  soft goal to a supported first-class integration (or whether the two buffer models should converge
  upstream).
- **ratatui's own buffer gains layers/z-order/signed coordinates.** If ratatui closes the #552 gaps
  that motivated keeping our buffer independent, re-evaluate whether Option B (build on
  `ratatui-core`) becomes viable and the bridge unnecessary.
- **Bridge maintenance cost exceeds its value.** If ratatui's churn makes `rabbitui-ratatui`
  expensive to maintain relative to how much the ecosystem is actually used through it (measured via
  reference-app and adopter usage), reopen the soft-goal premise itself.

## Amendments

- **2026-07-07 (bridge implementation):** The Context's claim that ratatui marks wide-grapheme
  continuation cells with an empty symbol was true of ratatui 0.29 but not 0.30, where the
  continuation cell reports a space. The bridge therefore detects continuation by grapheme width
  (advance two past a wide lead), not by empty symbol. The convertibility-by-construction bet
  otherwise held exactly as decided: total per-cell copy, no failure path;
  SLOW_BLINK/RAPID_BLINK/HIDDEN/underline_color narrow (dropped with doc notes). The bridge crate
  carries rust-version 1.88 (ratatui 0.30's MSRV), insulating the rest of the workspace per this
  ADR's leaf-crate rationale.
