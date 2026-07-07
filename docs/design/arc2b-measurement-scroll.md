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
