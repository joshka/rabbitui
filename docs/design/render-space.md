# Render space — widget-local coordinates, partial visibility, virtual canvases

Written 2026-07-11 (Fable pass; author-raised: "treating the buffer area not as a fixed
size — virtual buffers with viewports… and blandly being able to render to specific
portions of the screen as the only box"). Not a core item; recorded so the idea has a
home, the one real gap is pinned to the wave that needs it, and the expensive version is
deferred with explicit triggers.

## What rabbitui already does (the ratatui gap that is already closed)

In ratatui, `Widget::render(area, buf)` hands every widget the **whole frame buffer**
plus an area it is merely trusted to respect — nothing stops out-of-area writes, and
widget code is full of `area.x + …` arithmetic (#552 gaps 2–3 adjacent). rabbitui closed
this from the start: a widget paints through `RenderContext`, which is **widget-local and
hard-clipped** — `set_string(Position::ORIGIN, …)` is the widget's own top-left;
coordinates are translated and truncated against the widget's rect
(`rabbitui-core/src/widget.rs`, `set_string`: bounds-check → translate → `set_stringn`
with width clamp). A widget _cannot_ paint outside its box. This is worth saying loudly
in docs/marketing — it is a concrete correctness win over the substrate norm.

## The real gap: partial visibility renders the wrong slice

What the local-box model cannot yet express is "your logical extent is larger than your
visible window." Concretely, today's `ScrollScope::item` (`rabbitui-core/src/scroll.rs`,
`item`) clips a top-partial item by **shrinking the area** and painting at the viewport
top — so a 5-row item scrolled 2 rows above the viewport renders its rows `0..3` into
the visible slot, where rows `2..5` belong. Bottom clipping is correct (truncation cuts
the right rows); **top clipping shows the wrong slice**. The bug is invisible today
because uniform 1-row items are skipped rather than sliced — it starts biting exactly
when Wave B2's variable-height items land. B2's spec already demands "clip top/bottom";
this note defines the mechanism.

## Options

1. **Signed geometry** (`i32` `Position`/`Rect`; the "infinite canvas" model, ratatui
   #552 gap 2's full answer). A widget can be laid out at negative coordinates and the
   buffer renders the visible window. _Cost_: infects every geometry type, every widget,
   every layout call, for a capability most widgets never use; all arithmetic gains
   sign-handling. _Verdict_: **rejected for now** — revisit triggers below.
2. **Offset + mask on `RenderContext`** (the local-viewport model). The context keeps the
   widget's **full logical area** in local `u16` coordinates and adds a hidden-top offset
   (and, generally, a visible mask window): the widget renders itself completely in its
   own `0..height` space; the context translates by `−hidden_top` and drops writes
   outside the mask. Public API stays `u16`; signed arithmetic exists only inside the
   translate step (where `scroll.rs` already does `i32` math). A widget need not even
   know it is clipped. _Verdict_: **the design** — it is the cheap 90%, and B2 needs it.
3. **Scratch-buffer blit** (true virtual buffer). Render the widget into an owned buffer
   sized to its logical area, blit the visible window. Simple and always-correct; costs
   an allocation + full off-screen paint per widget per frame. _Verdict_: not the
   default, but the honest fallback for pathological cases (a widget whose logical
   height vastly exceeds the viewport and whose render cost is dominated by the visible
   part anyway should instead virtualize internally — that is what `desired_height` +
   sources are for).

## Decision and placement

- **Option 2 lands with Wave B2** (`docs/plans/wave-b2-virtualization.md`, amended):
  `RenderContext` gains a `hidden_top: u16` (internally a mask), `ScrollScope`/the
  anchor-fill pass the item's true height plus the hidden rows instead of shrinking the
  area, and facts keep carrying the **clipped visible** rect (hit-testing and focus
  operate on what is on screen). Acceptance test: a top-partial multi-row item shows its
  _bottom_ rows.
- **Option 1 (signed geometry) is deferred** to the known-deferred ledger. Revisit
  triggers: the desktop-metaphor wave (overlapping windows dragged partially off-screen),
  animation/transition work that wants widgets sliding in from off-canvas, or a bridge
  need (ratatui gaining signed coords upstream). If two of those arrive, signed geometry
  stops being speculative.
- **Option 3** needs no work now; document it as the escape hatch in the widget-contract
  docs when Option 2 lands.

One-width-oracle note: whatever mask model lands, clipping must slice by **display
columns via the shared width oracle**, never by bytes or chars — a mask that bisects a
wide grapheme drops the whole cell (the same rule the buffer diff already follows).
