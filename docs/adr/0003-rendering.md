# ADR 0003: Rendering — cell buffer, z-ordered layers, double-buffer diff, mode-2026 framing

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

Rendering is where a TUI framework is proven or exposed. The workload rabbitui targets — streaming,
interaction-heavy, coding-agent CLIs — stresses exactly the failure modes that show up at the paint
layer: flicker on redraw, desync between the framework's model of the screen and the terminal's
actual state, and cost that scales badly with widget count.

Four forces bound the decision:

- **The cheap-redraw fact.** A terminal frame is at most ~100k cells. Diffing two cell buffers and
  emitting escape sequences is microseconds. The Rust-GUI literature is explicit that this dissolves
  the machinery GUIs need: "damage regions are unnecessary — the double-buffer diff _is_ the damage
  tracking, computed for free after the fact" (docs/research/rust-gui-lessons.md; Levien, Rust
  2021). This is the single most load-bearing fact for this ADR.

- **Flicker and desync are the real bugs.** OpenTUI issue #1187: "Renderer diff emitter desyncs from
  host terminal cursor and never self-corrects; only a full repaint recovers"
  (docs/research/opentui.md). Bubble Tea's whole-area re-render blink on slow links (#32), Windows
  flicker regression (#1019), line-diff renderer losing rows on resize (#1039) — all fixed only by
  moving to a cell grid plus synchronized output in v2 (docs/research/bubbletea.md). Ink "clears and
  redraws for each update," which is why Claude Code and Gemini CLI flicker
  (docs/research/opentui.md; Toad post). McGugan's stated anti-flicker recipe is "overwrite don't
  clear / single write / Synchronized Output" (docs/research/textual.md).

- **The cell model is battle-tested and interoperable.** ratatui's `Buffer` + `Cell` + `diff_iter`
  "is battle-tested across thousands of apps," including the carefully-tested wide-grapheme diff
  rules — skip trailing cells of wide chars, clear stale halves (docs/research/ratatui.md,
  `buffer/buffer.rs:471-506`). libvaxis, Bubble Tea v2's "Cursed Renderer" (ncurses-derived), and
  OpenTUI all independently converged on double-buffer cell diffing wrapped in mode 2026
  (docs/research/libvaxis.md `src/Vaxis.zig:375,424`; bubbletea.md; opentui.md `renderer.zig:1321`).
  Keeping our cell model convertible to ratatui's is a stated interop goal (ADR 0010).

- **Overlays, modals, and tooltips are where flat planes hurt.** ratatui has "no layers, no z-order:
  one flat cell plane; overlays are done by rendering `Clear` then painting over"
  (docs/research/ratatui.md). Discussion #552 lists "no masking/compositing for scrolling
  containers" as a structural gap. Both Textual's compositor and lipgloss v2's `Layer`/`Compositor`
  earn free modals/overlays from z-order, and derive mouse hit-testing "from the same structure that
  painted" (docs/research/textual.md, bubbletea.md).

## Options considered

### A. Flat single cell plane, painter's-algorithm overpaint (ratatui's model)

_What it is:_ one `Buffer` per frame; overlays via `Clear` + later `render_widget` calls; z-order is
call order.

_Steelman:_ simplest possible model, zero compositor code, and it demonstrably shipped thousands of
apps. Cell diff already handles the overpaint correctly.

_Why not:_ it makes modals/popups/scrolling-containers a manual `Clear`-then-overpaint hack
(docs/research/ratatui.md), gives no natural home for per-widget partial repaint, and forces mouse
hit-testing to be hand-maintained bookkeeping (#552 item 4). rabbitui wants overlays and hit regions
as first-class facts (ADR 0001), so it needs layers.

### B. Textual-style compositor with damage regions (cuts → chops → occlusion → merge, spatial map)

_What it is:_ per-widget paint into styled segments; per line, find cuts, slice into chops, discard
occluded chops, merge survivors; a 100×20 spatial map for O(1) culling and hit-testing; emit only
changed regions (docs/research/textual.md, algorithms post).

_Steelman:_ the best-documented damage-region design in any TUI framework. Occlusion-aware layers
give free modals/tooltips; the spatial map keeps 1000-widget scrolling smooth; partial updates mean
"changing a single button color updates only that region."

_Why not:_ it optimizes a cost class terminals do not have. The spatial map and damage tracking
exist because Python's per-cell work is expensive and Textual wants to avoid touching the whole
screen; in Rust, diffing a ≤100k-cell buffer is already microseconds, so the double-buffer diff
yields the same "only changed regions get emitted" result with none of the compositor bookkeeping
(docs/research/rust-gui-lessons.md: "do not build repaint-boundary machinery; spend the complexity
budget on skipping _view/layout_ work instead"). Damage regions are a permanent invariant surface to
keep correct (Textual's compositor is its most intricate subsystem); the diff computes the same
damage for free, after the fact, with no way to get it wrong. We take Textual's _layers_ idea and
drop its _damage_ machinery.

### C. Segment / styled-run as the paint primitive (Textual/Rich, lipgloss)

_What it is:_ widgets emit runs of `(text, style)` rather than writing per-cell; the compositor
slices and merges runs; this is how Textual keeps CJK/emoji sane and memory low
(docs/research/textual.md: "Segments (styled runs) as the paint primitive, not per-cell grids").

_Steelman:_ fewer allocations than a cell grid for text-heavy content, natural double-width
handling, lower bandwidth, and it is the proven primitive behind harlequin-scale apps.

_Why not:_ runs lose on two axes that matter more here. First, **diff simplicity**: a cell grid
diffs positionally and trivially; two run-lists require alignment (Textual's cuts/chops pass exists
precisely to realign runs before merging) before you can tell what changed. Second, **ratatui
interop**: our stated bridge (ADR 0010) constructs a ratatui `Buffer` and copies cells; a run-based
internal model would need a lossy round-trip through cells at the boundary anyway. Cells win on diff
simplicity and interop. The efficiency argument for runs is real but belongs at the _wire_ layer,
not the paint model — see the decision on run-merging in the encoder.

### D. String diffing (Bubble Tea v1) — negative proof

_What it is:_ `View()` returns one big string; the renderer splits on `\n` and skips lines identical
to the last frame; width math is done in userland (lipgloss) (docs/research/bubbletea.md
`standard_renderer.go`).

_Steelman:_ trivially simple; the model is "just a string"; no buffer type to learn.

_Why not:_ this is a documented dead end. Diff granularity is a whole line, identity granularity is
the whole frame string, and correctness depends on ANSI-aware width math scattered in userland.
Bubble Tea "spent six years on line-granularity string diffing and the bug tail to match (#32,
\#1019, #1039) before rebuilding on an ncurses-style cell grid in v2" (docs/research/bubbletea.md).
dax rejected Bubble Tea for OpenTUI because it "operates on strings so that makes a lot of things
difficult — like performance and text detection" (docs/research/opentui.md). We treat v1→v2 as
settled evidence and start at the cell grid.

## Decision

rabbitui renders through a cell buffer, ratatui-compatible in shape, composited from z-ordered
layers, double-buffer diffed, and emitted inside synchronized-output framing.

- **Cell model.** rabbitui's `Cell` is a grapheme (small-string-inlined, per ratatui's
  `CompactString` approach) plus a typed style. Multi-cell graphemes occupy a lead cell and one or
  more **wide-grapheme skip cells**; the diff skips trailing cells of wide chars and clears stale
  halves when a wide char is overwritten by narrow content, adopting ratatui's tested wide-grapheme
  diff rules wholesale (docs/research/ratatui.md `buffer/buffer.rs:471-506`). The cell model is kept
  convertible to ratatui's `Cell` by construction (ADR 0010).

- **Layers, not a flat plane.** Widgets paint into z-ordered layers. Layers composite into one
  buffer before diffing; z-order is explicit (named/ordered layers), not merely call order.
  Overlays, modals, and tooltips are layers, not `Clear`-then-overpaint. Mouse hit regions and the
  other frame facts (ADR 0001) are recorded from the same layer geometry that painted, so
  hit-testing is a facts lookup, not separate bookkeeping.

- **Double-buffer diff is the damage tracking.** rabbitui holds two composited buffers; each frame
  diffs `previous` against `current` and emits only changed cells, then swaps. There are **no damage
  regions and no compositor spatial map** — the diff _is_ the damage tracking
  (docs/research/rust-gui-lessons.md). Widgets always paint correctly into a fresh layer; there is
  no per-widget invalidation contract to get wrong.

- **Mode-2026 framing.** The emitted diff is wrapped in synchronized-output (mode 2026) begin/end
  framing so the terminal presents each frame atomically, with an `errdefer`-style guarantee that
  sync is reset if a write fails (libvaxis's shape, docs/research/libvaxis.md `src/Vaxis.zig:424`).
  Synchronized output is negotiated as a capability (ADR 0012) and degraded gracefully when absent.

- **Full-repaint escape hatch.** rabbitui keeps a force path that discards the previous-buffer model
  and repaints every cell. It is invoked on resize, on resume from suspend, on capability
  renegotiation, and as the recovery action when desync is suspected — the failure OpenTUI #1187
  could only recover via full repaint (docs/research/opentui.md). The escape hatch is also public
  API for apps that detect corruption out-of-band.

- **No-op frame suppression.** When the diff is empty, rabbitui emits zero bytes (OpenTUI's "lazy
  frame start," `renderer.zig:1327`), so an idle app writes nothing.

- **Run-merging lives in the encoder.** The paint model is cells; the _encoder_ that turns the diff
  into bytes coalesces runs of adjacent cells that share SGR state into a single styled write with
  minimal cursor moves (OpenTUI "batches same-attribute cells into runs," `renderer.zig`).
  Run-merging is a wire-level optimization keyed on the SGR/mode encoder (ADR 0012), never a
  property of the paint primitive. This is why segments were rejected as the paint model (Option C)
  yet their bandwidth benefit is still captured: the cell grid stays the simple diffable truth, and
  runs appear only at the byte boundary.

## Consequences

_Positive:_

- Correctness-first painting with zero invalidation bugs: widgets repaint into fresh layers, the
  framework computes damage. No repaint-boundary machinery to maintain
  (docs/research/rust-gui-lessons.md).
- Free modals/overlays/tooltips from z-ordered layers, and hit-testing that falls out of the same
  geometry (the #552 compositing gap closed).
- Flicker-resistant by construction: synchronized output + overwrite-don't-clear + single-write is
  the recipe Textual, libvaxis, and Bubble Tea v2 all converged on.
- ratatui interop is cheap because the cell model is convertible by construction (ADR 0010), giving
  day-one access to the existing widget ecosystem.
- Bandwidth stays low via encoder run-merging and zero-byte idle frames, without paying
  segment-realignment complexity in the diff.

_Negative (honest):_

- No sub-frame partial repaint. Every frame composites all visible layers and diffs the whole
  buffer. This is fine at ≤100k cells but means we cannot cheaply exploit "only one widget changed"
  the way Textual's damage regions can — we rely on the diff being microseconds, and on skipping
  _view/layout_ work upstream (ADR 0004, 0005) rather than paint work. If a pathological app grows
  far past terminal-scale cell counts, this ADR's central assumption weakens (see revisit triggers).
- Layer compositing adds a pass ratatui's flat plane lacks; for the common single-layer case it is
  near-free, but it is real code and a real correctness surface (wide-grapheme cells straddling
  layer boundaries need care).
- Cell grids use more memory than styled runs for very long text-heavy content; we accept this and
  mitigate via virtualization in scrollable widgets (ADR 0008), not via a run-based buffer.

_Neutral:_

- The wide-grapheme skip-cell state machine needs invariant checks and property tests, not just care
  — libvaxis's #180-class regressions show this (docs/research/libvaxis.md). PTY-level tests assert
  on emitted escape sequences (ADR 0009).
- Width measurement is centralized in one oracle module (ADR 0012), not scattered `unicode-width`
  calls, because a grapheme's cell count depends on terminal mode (mode-2027, emoji VS16).
- Screen-mode specifics (inline vs alt-screen, scrollback-commit channel, mode-2026 negotiation
  detail) are decided in ADR 0013; this ADR owns only the buffer/layer/diff/encoder pipeline.

## Revisit triggers

- **Cell counts stop being terminal-scale.** If a real target app drives buffers large enough that
  full-buffer diff time becomes a measurable frame-budget cost (profiled, not assumed), reopen for
  per-layer dirty tracking or a Textual-style damage pass — the one path that reintroduces damage
  regions.
- **Diff desync recurs despite the escape hatch.** If OpenTUI-#1187-style desync is observed in the
  wild often enough that the force-repaint recovery is user-visible, revisit toward
  periodic/heuristic resync or a cursor-position reconciliation protocol.
- **Layer compositing shows up as the hot path** in the coding-agent CLI's streaming transcript
  profile, which would argue for a run/segment internal buffer after all (reopening Option C on
  measured evidence rather than a-priori).
- **A run-based ratatui successor** changes the interop cell model such that convert-by-construction
  no longer holds, which would force a re-evaluation of the cell-vs-run tradeoff at the bridge
  boundary (ADR 0010).
