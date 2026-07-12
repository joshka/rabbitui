# Wave B2 — variable-height virtualization + Table (implementation spec)

Written 2026-07-11 on Fable. This is the differentiation bet
(`docs/design/core-model-and-roadmap.md` §3.1): the survey's most-failed capability
(Textual's 800× DataTable; Toolong bypassing its own framework's scrollables; Brick's
uniform-height wall). The design below is adjudicated: **anchor-based scrolling with an
on-demand measure cache** — no global prefix sums, no O(n) work per frame. Corrections go
into this file, dated. Independent of Wave A (touches `rabbitui-core` + `rabbitui-widgets`
only); read `rabbitui-core/src/scroll.rs` and `rabbitui-widgets/src/selection_list.rs`
end-to-end before editing.

## Part 1 — anchor scrolling in `rabbitui-core/src/scroll.rs`

### Why anchors

An absolute row `offset` needs the summed height of everything above it — O(n) with
variable heights. An anchor `(item_index, rows_into_item)` needs only the heights of items
actually walked. Every robust virtual list (web and TUI alike) converges here. Total
height is never computed; the scrollbar uses an item-fraction approximation.

### State

```rust
pub struct ScrollState {
    anchor_item: usize,   // first (partially) visible item
    anchor_offset: u16,   // rows of that item hidden above the viewport top
    cache: MeasureCache,
}
struct MeasureCache {
    width: u16,                 // cache validity key: width changed → clear
    heights: Vec<Option<u16>>,  // len tracks source len; lazily filled near the window
    measured_sum: u64,
    measured_count: u64,
}
impl MeasureCache {
    fn estimate(&self) -> u16 { /* measured_sum/measured_count, min 1; 1 if none */ }
    fn get(&mut self, i: usize, width: u16, measure: impl FnOnce() -> u16) -> u16 {
        // clear-all if width != self.width; resize-with-None if len changed;
        // fill heights[i] on miss via `measure` and update sum/count.
    }
}
```

Public surface: `ScrollState::anchor() -> (usize, u16)`, `invalidate()` (content edits:
clears the cache, keeps the anchor), and whatever accessors the scrollbar test currently
uses — keep `offset()` only if trivially re-expressible (`anchor_item` when uniform),
otherwise delete it (pre-0.1) and fix callers.

### Algorithms (implement exactly; each is a loop over cached heights)

- **Fill window** (render pass): `y = -(anchor_offset as i32); i = anchor_item;
  while y < viewport_h && i < len { h = cache.get(i, width, measure_i); emit item i at
  rows y..y+h (clip top/bottom); y += h; i += 1 }`. Render + declare facts only for
  emitted items, plus one overscan item on each side for smooth wheel steps.
- **scroll_by(delta_rows: i32)**: positive → add to `anchor_offset`, then normalize:
  `while anchor_offset >= h(anchor_item) { anchor_offset -= h; anchor_item += 1 }`;
  negative mirrors backward. Clamp at both ends; the end clamp back-fills: walk backward
  from `len-1` summing heights until the viewport is full, and that walk's start is the
  maximum anchor.
- **ensure_visible(i)** (the `request_visibility` path): if `i < anchor_item` (or equal
  with offset > 0) → anchor = `(i, 0)`; if `i` beyond the window → walk backward from `i`
  so item `i`'s bottom row is the viewport's last row.
- **Scrollbar**: thumb position fraction = `anchor_item / len`, thumb size fraction =
  `emitted_count / len` — documented approximation, exact for uniform heights.

Rewire `ScrollScope::item`/`nest` and `Frame::scroll` onto this; preserve the existing
input vocabulary (wheel / PageUp / PageDown handling stays wherever it is today — only the
math moves). `SelectionList` keeps its own uniform-height windowing untouched.

### Tests (core, no runtime; follow the existing `scroll.rs` test style)

1. 10k uniform items: facts count per frame ≤ viewport items + 2 overscan (the
   virtualization property, asserted structurally — not by timing).
2. Variable heights (`i % 3 + 1`): window fill emits the right items at the right rows;
   partial top item clips correctly.
3. `scroll_by` forward/backward across multi-row items normalizes the anchor.
4. Scroll past the end clamps to the back-filled max anchor (last item fully visible).
5. `ensure_visible` both directions; already-visible is a no-op.
6. Width change invalidates the cache (heights re-measured; estimate recomputed).
7. Existing scrollbar tests updated to the approximation semantics.

Bench (extend `rabbitui-core/benches/core.rs`): 1M-item source, one frame render + one
`scroll_by` — assert structurally (measure-callback invocation count is O(window), e.g.
≤ 64 for a 24-row viewport) rather than by wall-clock.

## Part 2 — `Table` in `rabbitui-widgets/src/table.rs`

Uniform row height (v1; tables are rows of cells — variable-height rows are out of scope).
Reuse `SelectionList`'s windowing/selection math — read it first; the shape mirrors it.

```rust
pub struct Column {
    header: Cow<'static, str>,
    constraint: Constraint,       // width via split_columns on the body area
}
impl Column { pub fn new(header: impl Into<Cow<'static, str>>, constraint: Constraint) -> Self }

pub trait TableSource {
    fn len(&self) -> usize;                                    // rows
    fn cell(&self, row: usize, col: usize) -> Cow<'_, str>;    // called only for painted cells
}
// adapters, mirroring the list's: impl for Vec<Vec<String>> / &[Vec<String>], plus
pub fn table_from_fn(len: usize, f: impl Fn(usize, usize) -> String) -> impl TableSource;
pub fn table_rows_with<T>(rows: &[T], f: impl Fn(&T, usize) -> String) -> impl TableSource;

pub struct Table<S: TableSource> { source: S, columns: Vec<Column>, /* empty_text, … */ }
pub struct TableState { selected: usize, offset: usize }       // SelectionListState shape
```

Behavior (all mirrored from `SelectionList`, cite its tests as the template):

- Header row: row 0 of the area, `Role::Muted` + bold, never scrolls.
- Body: virtualized window over `offset`; `cell()` called only for visible (row, col).
- Keys: Up/Down/PageUp/PageDown/Home/End move selection; selection clamped at render.
- Outcomes: `Selected(usize)` on move, `Activated` on Enter. Click-to-select via the same
  hit-testing path the list uses.
- `empty_text(…)` built-in empty state (stays declared; the finding-#6 lesson).
- Column widths recomputed each frame via `split_columns(inner, constraints)`; cells
  truncated to the column (grapheme-safe — use whatever truncation `SelectionList` rows
  use; if none exists, add one helper in ONE place, per the one-width-oracle rule).

Tests: mirror `selection_list.rs`'s suite (render, selection movement + clamp, window
virtualization with a counting source proving `cell()` calls are O(window), empty state,
outcomes, header stays pinned while body scrolls, column truncation on narrow widths).

## Part 3 — adoption proof

Switch the log-follower (`comparisons/rabbitui/src/main.rs`) detail pane or add a columns
view using `Table` with `table_rows_with` — the dogfood check that the API survives
contact with a real app. Note any friction in `docs/design/dogfood-findings.md` (new
numbered findings, same format).

## Acceptance

1. `cargo test --workspace` (+ comparisons suite) green; clippy zero; nightly fmt clean.
2. The O(window) structural assertions (core test 1, table cell-count test) pass with
   sources of 10k and 1M.
3. Existing `SelectionList`/scroll callers unaffected (`gallery` example still renders —
   coordinator betamax note).
4. Commits (path-scoped, in order): core anchor+cache; widgets Table; comparisons
   adoption + findings note.

## Known traps

- `desired_height` must stay cheap and paint-free (widget contract) — the measure closure
  passed to `MeasureCache::get` wraps it; never call it for items far from the window.
- `heights: Vec<Option<u16>>` at 1M items is ~2–4 MB — acceptable; do NOT try to be clever
  with maps (cache locality is the point). Document the tradeoff in the module doc.
- Facts for clipped partial items must still carry correct (clipped) areas or hit-testing
  on the top/bottom rows misroutes — test 2 covers this; don't weaken it.
- `Cow<'_, str>` on `cell()` keeps zero-copy for stored strings while allowing formatted
  cells; resist `String` (allocates per painted cell per frame).
