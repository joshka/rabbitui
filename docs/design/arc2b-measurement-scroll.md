# Arc 2B design: measurement, ScrollView, styled text, logging, benchmarks

Working design note for ROADMAP Arc 2B — the binding constraint before the flagship. The interesting
decision here turned out to be bigger than the features: **scoped builders are rabbitui's
composition mechanism.** `Frame::scoped` and `Frame::layer` already compose identity subtrees; the
scroll container extends the same shape to composed _layout_, and the catalog's future containers
(forms, splits, tabs) follow it. No widget-children trait machinery; composition is functions
declaring into scopes.

## Intrinsic measurement

The widget contract gains measurement (ADR 0004's deferred half, ADR 0008 addition):

```rust
pub trait Widget {
    // ...existing...
    /// The height this widget wants at `width`, given its retained state.
    /// Default: one row. Containers use this to stack; ScrollView uses it to
    /// virtualize. Must be cheap (called per frame per candidate item) and
    /// must not paint.
    fn desired_height(&self, state: &Self::State, width: u16) -> u16 {
        let _ = (state, width);
        1
    }
}
```

`Frame::measure(key, width, &spec) -> u16` lends the retained state read-only (a store peek that
does **not** mark the id seen — measuring is not declaring). Text implements it (line count; wrapped
line count when wrap is on); Collapsible implements it (1 collapsed, 1 + body height expanded) —
retiring the flagship's hand-rolled fixed-slot stack.

## ScrollView as a scoped builder

```rust
frame.scroll(key("transcript"), area, |scroll| {
    for cell in &app.cells {
        scroll.item(key("cell").index(cell.id), &CellWidget::new(cell));
    }
});
```

Semantics: items stack vertically, each at its measured height for the viewport's inner width; the
scroll scope retains `offset` (u16 rows) by identity; only items intersecting the viewport render
(virtualization by construction — items above/below are measured, never painted); the scope declares
itself as a focusable widget whose handler consumes Up/Down/ PageUp/PageDown/Home/End and wheel
events (scroll first, selection is the item's business); a scrollbar paints in the right column
(`Border` role, thumb `Muted`) when content overflows. Visibility requests from items
(`ctx.request_visibility`) are consumed here: the runtime already records them in facts; the scroll
scope adjusts offset next frame to reveal the requester — closing the loop plumbed in slice 7.
Nested scrolls: inner wins (its handler is the target; bubble gives the outer a chance on
unconsumed).

Measurement caching: none in v1 (measured per frame; the benchmark harness will tell us if that
matters — do not guess).

## Styled-span Text

`Text` accepts spans: `Text::new(impl Into<Content<'a>>)` where `Content` is `Plain(Cow<'a, str>)`
or `Spans(Cow<'a, [Span]>)` (core::text::Span, as committed lines use). One internal iterator yields
`(grapheme, Style)` for both paint and wrap paths, so wrap works styled — fixing the flagship's
monochrome live tail and the "styling pops at commit" strain. Role-based default style still applies
where a span's style is empty (`Style::new()` merges under the widget style). `Attrs` gains
`remove`, `BitAnd`, `Not`, `insert` for completeness.

## Logging seam

`tracing` integration behind a default-on `tracing` feature in the facade:
`rabbitui::log::Collector` is a `tracing_subscriber::Layer` writing formatted events into a bounded
ring buffer (`Arc<Mutex<VecDeque>>`, cheap enough at TUI event rates). `App` installs it by default
in debug builds (builder: `.tracing(bool)`); `LogOverlay` (widgets) renders the tail in a `layer` —
toggled by an app-chosen key (examples use F12-style `Ctrl+L`? no — that's taken; use `~`/config;
the keymap arc will formalize). Nothing writes to stderr while the terminal is owned; on close,
buffered `WARN+` lines optionally flush to stderr so panics and errors are not lost with the
alternate screen. RUST_LOG-style filtering via the standard EnvFilter.

## Benchmark harness

`rabbitui-core/benches` and `rabbitui/benches` on criterion (dev-dep only): buffer `set_string`
plus full diff at 240×70; layout splits; `StateStore` churn; and the honest one — a full declared
frame of a synthetic 1,000-widget view (declare → facts → paint) plus the same at 10,000, measuring
view-construction cost against ADR 0001's "microseconds" claim. Results are recorded in this note's
deltas; CI budget assertions are Arc 4 (baseline data first).

## Block-level commit (small)

`Update::commit` already appends whole lines; the flagship's need is committing a _finished block_
of a still-streaming message. No engine change required: the app commits markdown-rendered lines for
any block the parser closes and keeps only the open block in the live tail. This is app-pattern
documentation plus a flagship implementation, not framework code — recorded here so nobody builds
machinery for it.

## Implementation deltas (part 1)

Part 1 (intrinsic measurement, ScrollView, styled-span Text, Attrs ops) shipped; the logging seam
and benchmarks (part 2) are not built. What landed, and where it diverged from or refined the note
above:

- **Measurement contract.** `Widget::desired_height(&self, state, width) -> u16` (default 1) is on
  the trait. `Frame::measure(key, width, &spec)` composes the id under the current parent (exactly
  as `Frame::widget` does) and **peeks** the store. The store gained
  `StateStore::peek::<S>(&self, id) -> Option<&S>` — a pure `&self` read that does *not* touch
  `last_seen`. Shape rationale: measuring must not mark an id seen (a scroll measures thousands of
  off-screen candidates), or the later real declaration trips the duplicate-id `debug_assert!` and
  dropped-widget grace tracking leaks. It returns `Option` (not a created default) because a peek
  never inserts; `Frame::measure` falls back to `W::State::default()` when the peek is `None` (a
  first-frame item has no retained state). `Style::merge_over(base)` was added for span default
  merging (colors override-if-set, attrs union).

- **ScrollView (`rabbitui_core::scroll`).** `Frame::scroll(key, area, |scroll| …)` takes an
  `impl Fn` (not `FnOnce`) because the closure is replayed across a measure pass and a paint pass —
  the honest cost of "no measurement caching in v1" is running the item closure twice. Two passes:
  (1) sum every item's `desired_height` at the viewport width to learn content height and decide on
  the scrollbar column (re-measured at the narrower inner width when the scrollbar appears, since a
  narrower width wraps taller); (2) declare only viewport-intersecting items at clipped areas,
  advancing a content cursor; off-viewport items are measured (in `item`) but never declared —
  virtualization by construction. `ScrollScope::nest(key, height, …)` declares a nested scroll as an
  item (a scroll is a scope, not a `Widget`, so it can't go through `item`).

- **Frame scoping refactor.** `scoped`/`layer`/`scroll` now share one `with_child_scope(scope_id,
  layer_delta, body)` helper (the `mem::take`/reclaim dance was duplicated three ways). New
  crate-private `Frame` methods back the container: `container_state`/`put_container_state`
  (clone-out/write-back retained state before deciding what to declare), `register_container::<W>`
  (push a focusable fact + handler thunk for a scope's own id), `paint_absolute` (a container paints
  the scrollbar in absolute coords, unlike a widget's area-relative `RenderCtx`), and
  `visibility_len`/`visibility_since` (find the requests a scope's own children added this frame).

- **Routing / focus integration decisions.** The scroll scope registers itself as a focusable
  `Widget` (`ScrollView`) at its scope id with a handler thunk, so existing capture→target→bubble
  routing reaches it with no new machinery. Two decisions the note left implicit: (a) the handler
  acts **only on `Phase::Bubble`** — a nested (inner) scroll is the routing target and must handle
  the event before an enclosing (outer) scroll sees it; without the phase guard the outer swallows
  it on capture and "inner wins" fails. (b) The scroll consumes the wheel unconditionally (even a
  clamped no-op at an end) — the wheel over a scroll region is the scroll's, not the app's.

- **Facts integration for scroll-into-view.** The loop closes through **retained state**, not
  previous-frame facts. A child's `request_visibility` records a `VisibilityRequest` fact (absolute
  coords) this frame; after the paint pass the scope reads the requests its own children added
  (positional `visibility_since` filter — a `WidgetId` is a one-way hash, so a structural ancestor
  check is impossible, but every request pushed during the scope's child-frame body is a descendant
  by construction), maps the request's **bottom** row to a content row, and stashes it in
  `ScrollState.pending_reveal`. The *next* frame consumes the stash and adjusts the offset — the
  "adjusts offset next frame" the note specifies, with no previous-facts threading. Honest limit
  (shared with `SelectionList`): a **fully** off-screen item never renders to request, so
  scroll-into-view works for an edge-visible (partially-clipped) requester; a truly off-screen
  target is the app's to scroll to.

- **Styled-span Text.** `Content<'a>` is `Plain(Cow<'a, str>)` / `Spans(Cow<'a, [Span]>)` with
  `From` for `&str`, `String`, `&String`, `Cow`, `Vec<Span>`, `&[Span]`, `Span`. The `&String` impl
  was needed to keep `Text::new(&some_string)` source-compatible (the old `&'a str` parameter got
  deref coercion that `impl Into<Content>` does not). One `styled_lines` helper yields
  `Vec<Vec<(grapheme, Style)>>` (split on `\n`, each span's style `merge_over` the base) consumed by
  both paint and `wrap_line`, so styled text wraps identically to plain — wide graphemes stay whole
  at a span boundary. Wrap gained one refinement over the old plain wrapper: whitespace that would
  *lead* a row after a break is dropped (a word exactly filling the row no longer leaves a stray
  leading space). `Text::content()` now returns `&Content` (was `&str`); callers use
  `.to_plain_string()`.

- **Attrs ops.** `insert`, `remove` (both `const`), `BitAnd`, `BitAndAssign`, `Not` (masked to a new
  `Attrs::ALL` so a complement never yields an undefined flag). The flagship's hand-rolled `remove`
  (rebuild-from-known-flags) is retired for `Attrs::remove`.

- **Example rework.** `examples/agent.rs`'s alt-screen transcript is now `frame.scroll` +
  measured cells (`Text` for user/assistant, collapsed `Collapsible` for tool), retiring the
  fixed-slot stack (`render_transcript`/`render_cell` → `render_transcript`/`declare_cell`). The
  app's `scroll: usize` field and its Up/Down/PageUp/PageDown handling are gone — the scroll scope
  owns them. One behavior note surfaced: the scroll scope is focusable and declared before the
  composer, so it takes initial focus in alt-screen (the composer is reached via Tab, matching the
  example's existing "Tab to the composer" substrate note). Verified via the agent tape driven into
  alt-screen: the titled panel shows the measured, stacked column (wrapped prose at two rows, the
  tool cell collapsed to one row), content fitting the viewport with no scrollbar.

- **Forced facade adaptation (external, out-of-band).** The `qwertty` path dependency was refactored
  mid-session (its `InputEvent`/`ControlInput`/`CsiInput`/`KeyInput` became `Event`/`KeyEvent`/
  `Key`/`SyntaxToken`), which broke the `rabbitui` facade's `terminal::next_event` and
  `input::from_qwertty` — code Arc 2B does not otherwise touch. To keep `cargo test --workspace`
  runnable, the input seam was migrated faithfully to the new API (same behavior: text/named keys,
  ctrl-letter chords, SGR mouse bridged from a preserved CSI now inside `Event::Syntax`). This is a
  substrate-layer adaptation forced by external drift, not Arc 2B design work; it is noted here for
  the record.
