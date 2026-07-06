# Research memo: Ratatui

**Verdict:** Ratatui is an excellent *rendering substrate* (buffer, cell diff, styled text, 1-D constraint layout) that deliberately refuses to be a framework — and the entire ecosystem's shape is a map of the holes that refusal left: event loop, input, focus, widget identity, and composition are all outsourced to app authors and a fragmented crate zoo.

Date: 2026-07-06

**Sources**

- Source tree at `/Users/joshka/local/ratatui` (workspace, post-0.30):
  - `ARCHITECTURE.md` (crate split, RFC [#1388](https://github.com/ratatui/ratatui/issues/1388))
  - `ratatui-core/src/widgets/widget.rs`, `widgets/stateful_widget.rs`
  - `ratatui-core/src/terminal/{render.rs,buffers.rs,frame.rs}`
  - `ratatui-core/src/buffer/buffer.rs`, `buffer/cell.rs`
  - `ratatui-core/src/layout/layout.rs`
  - `ratatui/src/widgets.rs`, `ratatui/src/widgets/{widget_ref.rs,stateful_widget_ref.rs}`
  - `BREAKING-CHANGES.md` (1164 lines, 74 `###` change entries)
- Web (fetched):
  - FAQ: <https://ratatui.rs/faq/>
  - Design-limits discussion: <https://github.com/ratatui/ratatui/discussions/552>
  - WidgetRef tracking issue: <https://github.com/ratatui/ratatui/issues/1287>
  - Component pattern: <https://ratatui.rs/concepts/application-patterns/component-architecture/>
  - Templates (simple/event-driven/component, sync+async): <https://github.com/ratatui/templates>
  - Maintainer framework experiment: <https://github.com/joshka/tui-framework-experiment>
  - HN thread: <https://news.ycombinator.com/item?id=38593638>
- Web (search-verified ecosystem crates): [crokey](https://github.com/Canop/crokey), [tui-textarea](https://github.com/rhysd/tui-textarea), [rat-focus / rat-widget / rat-salsa](https://github.com/thscharler/rat-widget), forum thread ["How to debug flickering in my app?"](https://forum.ratatui.rs/t/how-to-debug-flickering-in-my-app/106)

## Core architecture

**Immediate mode, rerender everything.** The app owns the loop: `Terminal::draw(|frame| ...)` takes a closure, you rebuild every widget every frame, widgets are consumed as render *commands* (`Widget::render(self, area, buf)` takes `self` by value — `ratatui-core/src/widgets/widget.rs:70`). The FAQ is explicit that you cannot call `terminal.draw()` twice per loop iteration and that Ratatui "does not handle input" — crossterm et al. do. Frame identity across time is just `Frame::count` (a wrapping usize, `terminal/frame.rs:37`) for animations.

**Double-buffer cell diffing.** `Terminal` holds two `Buffer`s; `draw` renders into the current one, `flush` computes `previous.diff_iter(current)` and sends only changed cells to the backend, then `swap_buffers` (`terminal/buffers.rs:97-121`, `terminal/render.rs:292-305`). The diff has carefully-tested rules for multi-width graphemes (skip trailing cells of wide chars, clear stale halves — `buffer/buffer.rs:471-506` and tests at `buffer.rs:1094-1542`). `Cell` stores its symbol as an `Option<CompactString>` for inline small-string storage (`buffer/cell.rs:37-46`). There are no damage regions, no layers, no z-order: one flat cell plane per frame; overlays are done by rendering `Clear` then painting over the same cells later in the same frame. Painter's algorithm, order = the order of your `render_widget` calls.

**Layout: cassowary, per-call, memoized.** `Layout::split` builds a fresh `kasuari` (maintained cassowary fork) `Solver` per invocation with strength-tiered constraints (`layout/layout.rs:818` ff., strengths module), then caches `(Rect, Layout) -> Rects` in a thread-local LRU of 500 entries (`layout.rs:37-56, 218`). A code comment admits the design compromise: keeping a persistent `Solver` and editing constraints would be the "proper" cassowary usage, but the stateless-split API precludes it (`layout.rs:798`). Layout is strictly 1-D (split a Rect along one axis with `Constraint`s + `Flex`); nesting splits is the only way to do 2-D. There is **no text measurement in layout** — `Paragraph::line_count` exists but nothing feeds intrinsic content size back into constraint solving.

**The widget trait zoo.** Four traits: `Widget` (consumes self), `StatefulWidget` (consumes self + `&mut State` — the framework's *only* concession to cross-frame state, e.g. `ListState` scroll offset; `widgets/stateful_widget.rs`), plus unstable `WidgetRef`/`StatefulWidgetRef` for `&self` rendering and `Box<dyn WidgetRef>` collections (tracking issue [#1287](https://github.com/ratatui/ratatui/issues/1287)). The evolution hurt: 0.26 added `impl Widget for &W` on all built-ins and introduced `WidgetRef`; 0.30 *reversed the blanket impl direction* (`WidgetRef` is now blanket-implemented for `&W: Widget`, not the other way), moved `WidgetRef` out of core into the main crate behind `unstable-widget-ref`, and gated `Frame::render_widget_ref` behind a new `FrameExt` trait (`BREAKING-CHANGES.md:311-347`). Docs now recommend three different patterns (`impl Widget for &W`, `for &mut W`, `StatefulWidget`) depending on mutability needs — a decision tree every widget author must re-derive.

**Crate layout (0.30).** Workspace: `ratatui-core` (traits/text/buffer/layout/style — the stability anchor for third-party widget authors), `ratatui-widgets` (Block, Paragraph, List, Table, Chart, BarChart, Canvas, Gauge, Sparkline, Scrollbar, Tabs, Calendar, Clear...), one crate per backend (`ratatui-crossterm`, `-termion`, `-termina`, `-termwiz`), `ratatui-macros`, and a facade `ratatui` crate that re-exports everything (`ARCHITECTURE.md`). Explicit goal: widget libraries depend only on a slow-moving core.

**What's absent by design:** event loop, input decoding, key binding, focus, mouse hit-testing, styling/theming system, reactive state. The ecosystem filled it: [crokey](https://github.com/Canop/crokey) (key-combination parsing + kitty-protocol multi-key combos), [tui-textarea](https://github.com/rhysd/tui-textarea) (text editing with its own key-event abstraction over three backends), thscharler's [rat-focus/rat-widget/rat-salsa](https://github.com/thscharler/rat-widget) (FocusFlag-in-widget-state focus system, event-queue runtime with timers/tasks), the official [templates repo](https://github.com/ratatui/templates) (event-driven and Component-trait templates, sync and tokio-async variants — descendant of kdheepak's ratatui-async-template with its action channels), and the maintainer's own [tui-framework-experiment](https://github.com/joshka/tui-framework-experiment) (buttons, event handling, stack containers; explicitly a sandbox). The [component pattern](https://ratatui.rs/concepts/application-patterns/component-architecture/) (`init/handle_events/update/render` per component) is documented on the website as an *application pattern* precisely because the library won't own it.

## What it gets right

- **The buffer is a great assembly language.** `Buffer` + `Cell` + `diff_iter` with correct wide-grapheme semantics is battle-tested across thousands of apps. Styled text (`Span`/`Line`/`Text` + `Stylize`) is genuinely pleasant.
- **Rerender-everything is cheap enough.** Diffing at the cell level after a full redraw means app authors never think about invalidation, and terminal writes stay minimal. Simplicity of mental model is the #1 reason for adoption.
- **`StatefulWidget` is a minimal, honest answer to state.** App owns the state struct, widget borrows it for one frame. No hidden framework state, trivially testable.
- **Layout caching works.** Thread-local `(Rect, Layout)` LRU makes the "recompute layout every frame" model affordable despite running a constraint solver.
- **Testing story is strong at the buffer level.** `TestBackend` (`ratatui-core/src/backend/test.rs:32`) + `Buffer::with_lines` equality gives readable snapshot-style assertions; the codebase itself tests widgets by rendering into a `Buffer` and comparing lines (e.g. `widget.rs:130-134`).
- **The 0.30 crate split is the right shape.** Core-for-stability / widgets / per-backend crates / facade is a proven layering that third-party authors asked for ([#1388](https://github.com/ratatui/ratatui/issues/1388)).

## What users complain about

- **[Discussion #552](https://github.com/ratatui/ratatui/discussions/552)** is the canonical critique. arxanas lists five structural gaps: (1) no relative/flow positioning because solved sizes ignore content size; (2) unsigned `Rect` means you can't render a widget partially off-screen at negative coordinates; (3) no masking/compositing for scrolling containers; (4) no way to know *where* a widget was rendered afterward (hit-testing/focus requires manual bookkeeping); (5) no event handling — "a combinatorial explosion of state/event pairs" as apps grow. The maintainer response (joshka): "I agree with every single point you're making. Ratatui doesn't do any of those" — with pointers to Masonry's retained `Widget` trait (`on_event/layout/paint/children`) and Taffy as directions worth exploring. That is the maintainers naming their own design ceiling.
- **"It's not a UI framework, it's a display library"** — maintainer's own framing on [HN](https://news.ycombinator.com/item?id=38593638), where commenters complain about missing containers/dialog types, no deep event support, wanting "a more react/swiftui-esque TUI library", and mouse click events being DIY.
- **Flickering** in specific scenarios (inline viewports, Gauge/BarChart-heavy redraws) — see the [forum thread](https://forum.ratatui.rs/t/how-to-debug-flickering-in-my-app/106); mitigations like the `scrolling-regions` backend feature exist but are opt-in patches on the flat-diff model.
- **Panics on out-of-bounds buffer access** — the FAQ concedes most code "was not designed around using `Result`s"; `Buffer` indexing panics (`buffer.rs:518` docs).
- **API churn.** `BREAKING-CHANGES.md` documents 74 distinct breaking changes across releases: `Spans`→`Line`, `Table::new` signature changes twice, `block::Title` removed, `Alignment` renamed, prelude reshuffles, and the `WidgetRef` blanket-impl reversal. Much of it is builder-method signature churn (`Into<Line>` generalization, one method at a time).
- **Every non-trivial app rebuilds the same scaffolding** — event loop + action channel + focus + component routing. That the official answer is a *template* (copy-paste this runtime) rather than a library is itself the complaint, and rat-salsa, tui-realm, the component template, and tui-framework-experiment are four incompatible answers to it.

## What's worth stealing

- **`Buffer`/`Cell`/`diff_iter` semantics wholesale** — including CompactString cells, the wide-grapheme diff rules, and `Buffer::with_lines` test constructors. This layer is done; don't reinvent it, re-house it.
- **`TestBackend` + buffer-equality testing** as the base of the test pyramid.
- **Styled-text types** (`Span`/`Line`/`Text`, `Stylize` shorthands) — the most-loved API surface in the crate.
- **Layout caching keyed on inputs** (pure function + memo), and kasuari's strength-tiered constraint vocabulary (`Length/Min/Max/Percentage/Ratio/Fill` + `Flex`) as the *user-facing* layout language even if the solver underneath changes.
- **The crate layering** (stable core / widgets / backends / facade) and the discipline of a maintained `BREAKING-CHANGES.md`.
- **`StatefulWidget`'s "app owns state, widget borrows it" honesty** — as a low-level escape hatch, not the primary model.
- **From the ecosystem:** crokey's key-combination parsing (incl. kitty protocol), rat-focus's insight that focus is per-widget state plus an ordered traversal built at render time, and the component template's `handle_event → action → update → render` unidirectional loop.

## Implications for rabbitui

- **Own the event loop, async-first.** The single largest ecosystem fragmentation (templates, rat-salsa, tui-realm, component pattern) exists because Ratatui won't ship a runtime. rabbitui on qwertty should ship the loop: an async event stream (input, resize, timers, subprocess/network via spawned tasks feeding a message channel), with `draw` scheduled by the runtime. Make the Ratatui-style "bring your own loop" a documented escape hatch, not the default.
- **Give widgets identity; make the tree retained even if authoring is declarative.** Every one of #552's five gaps (hit-testing, focus, event routing, compositing, content-aware layout) is downstream of "no widget identity across frames." Xilem-style view diffing onto a retained widget tree (Masonry's `on_event/layout/paint/children` — which the Ratatui maintainer himself pointed to in #552) gets you immediate-mode ergonomics *and* identity. Framework-owned per-node state keyed by tree position/ID replaces the `StatefulWidget` state-threading boilerplate.
- **One dyn-compatible render trait from day one.** The `Widget`/`StatefulWidget`/`WidgetRef`/`StatefulWidgetRef` zoo and the 0.30 blanket-impl reversal (`BREAKING-CHANGES.md:333`) happened because consuming-`self` render was baked in early and dyn-compatibility was retrofitted. rabbitui's paint method should take `&mut self` on a retained node — boxed, storable, no parallel ref-traits needed.
- **Keep cell-diff as the wire protocol; add layers above it.** Double-buffer diffing survives contact with reality — keep it at the qwertty boundary. But render into per-layer buffers (or a clip/offset-aware compositor) with z-order and *signed* logical coordinates, so scrolling containers, popups, and partially off-screen widgets are native (#552 items 2–3) instead of the `Clear`-then-overpaint hack. Damage regions fall out of the retained tree (repaint dirty subtrees), fixing the inline-viewport flicker class.
- **Layout: 2-D tree layout with text measurement, cassowary vocabulary on top.** Ratatui's solver-per-split can't see content size; that kills labels, wrapping, and intrinsic sizing. Use taffy-style tree layout with measure functions (text measurement via the same unicode-width tables the buffer uses), but keep `Constraint`/`Flex` as sugar — it's what users already know. Memoize like Ratatui does.
- **Focus and input routing are core, not ecosystem.** Ship focus traversal (ordered, derived from the tree), event bubbling/capture, and mouse hit-testing from node geometry recorded at layout time. Adopt crokey-style key-combination parsing and kitty-keyboard support in the core input layer so widgets like textareas don't each invent key abstractions (tui-textarea had to).
- **Ship batteries: containers, dialogs, text input.** The HN complaint list (containers, dialogs, higher-order widgets) plus tui-textarea's existence defines the missing built-ins. A framework with focus + identity can ship them; Ratatui structurally couldn't.
- **Copy the crate layout, but put the whole third-party trait surface in core.** `rabbitui-core` (tree, paint, layout, event traits, buffer/text), `rabbitui-widgets`, `rabbitui-qwertty` backend, facade crate. Learn from `WidgetRef` living *outside* core: anything a third-party widget must implement belongs in the stability-anchor crate from v0.1.
- **Testing: buffer snapshots plus a headless driver.** Keep `TestBackend`-style buffer assertions, and add what Ratatui can't have: a headless app driver (inject events, pump the async loop, assert on tree + buffer) — possible only because rabbitui owns the loop.
- **Design against churn.** 74 documented breaking changes, mostly signature drift on builder methods. Prefer `impl Into<...>` params, non-exhaustive props structs, and sealed extension points from the start; budget for an unstable-feature channel like Ratatui's `unstable-widget-ref` for anything unproven.
