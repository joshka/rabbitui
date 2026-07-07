# Textual (Python / Textualize)

**Verdict:** Textual is the most complete existence proof that a browser-grade retained DOM + CSS +
compositor works in the terminal — its algorithms are worth stealing wholesale, its Python tax and
monolithic widget set are the cautionary tale.

Date: 2026-07-06

**Sources**

- Docs (fetched): [CSS guide](https://textual.textualize.io/guide/CSS/),
  [Reactivity](https://textual.textualize.io/guide/reactivity/),
  [Events/message pump](https://textual.textualize.io/guide/events/),
  [Workers](https://textual.textualize.io/guide/workers/),
  [Testing](https://textual.textualize.io/guide/testing/),
  [Layout](https://textual.textualize.io/guide/layout/),
  [Input](https://textual.textualize.io/guide/input/),
  [Screen API](https://textual.textualize.io/api/screen/)
- Blog (fetched):
  [Algorithms for high performance terminal apps](https://textual.textualize.io/blog/2024/12/12/algorithms-for-high-performance-terminal-apps/)
  (Dec 2024),
  [The future of Textualize](https://textual.textualize.io/blog/2025/05/07/the-future-of-textualize/)
  (May 2025),
  [7 things I've learned building a modern TUI framework](https://www.textualize.io/blog/7-things-ive-learned-building-a-modern-tui-framework/),
  [Announcing Toad](https://willmcgugan.github.io/announcing-toad/)
- Issues/discussions (fetched or via search snippets):
  [textual-fastdatatable README](https://github.com/tconbeer/textual-fastdatatable),
  [discussion #5068 slow Input tests](https://github.com/Textualize/textual/discussions/5068),
  [issue #4737 DataTable slow](https://github.com/Textualize/textual/issues/4737),
  [discussion #5953 DataTable perf](https://github.com/Textualize/textual/discussions/5953),
  [issue #6074 kitty progressive enhancement](https://github.com/Textualize/textual/issues/6074)
- HN via Algolia API (fetched JSON):
  [thread 37174657](https://news.ycombinator.com/item?id=37174657) (Textual RAD framework),
  [thread 40926211](https://news.ycombinator.com/item?id=40926211) (Posting v1)
- GitHub API (fetched): `Textualize/textual` repo + releases list; `Textualize/toolong` source tree

## Core architecture

**Programming model: retained DOM, not Elm, not React.** An app subclasses `App`; widgets subclass
`Widget`; `compose()` yields children declaratively at mount time; afterwards the tree is mutated
imperatively (`mount`, `remove`, `query_one("#id")` with CSS selectors). State lives _in_ the
widgets as `reactive()` class-scope descriptors — there is no external model/store and no per-frame
view function. Reactives give four hooks: smart refresh (assignment schedules a repaint, batched;
`layout=True` also invalidates layout), `validate_<name>` (clamp/coerce), `watch_<name>` (old/new
callback), `compute_<name>` (cached derived values, recomputed when any reactive changes; order is
compute → validate → watch). `recompose=True` is the escape hatch toward view-diffing: throw away
children and re-run `compose()` on change. One-way parent→child `data_bind()` exists. Mutable values
are a footgun: you must call `self.mutate_reactive(...)` manually after `list.append`
([reactivity guide](https://textual.textualize.io/guide/reactivity/)).

**Message pump: an actor per widget.** Every App and Widget owns its own asyncio Task plus message
queue; handlers (`on_input_changed` naming convention, or `@on(Input.Changed, "#selector")` with
CSS-selector targeting) are awaited in order, so a widget processes one message at a time. Messages
with `bubble=True` propagate up the DOM ancestry; `stop()` halts bubbling; `prevent()` suppresses
message types while programmatically updating children
([events guide](https://textual.textualize.io/guide/events/)).

**Compositor: per-widget paint + occlusion, not full-buffer diff.** Widgets render Rich `Segment`s
(text + style runs, not per-cell "pixels" — this is how variable-width CJK/emoji stay sane). Per
line, the compositor: (1) finds _cuts_ (every x-offset where any widget region begins/ends), (2)
slices all segment lists into aligned _chops_, (3) discards occluded chops (anything not top-most in
z-order), (4) merges the rest. Z-order comes from the CSS `layers` style (leftmost name = bottom).
Visibility culling uses a _spatial map_: widgets hashed into a 100×20-cell grid so
hit-testing/culling stays ~O(1) as widget count grows; the map is cached and only rebuilt when
geometry changes. Output supports partial updates ("changing a single button color updates only that
region") and the trio "overwrite don't clear / single write / Synchronized Output" is McGugan's
stated anti-flicker recipe
([algorithms post](https://textual.textualize.io/blog/2024/12/12/algorithms-for-high-performance-terminal-apps/),
[7 things](https://www.textualize.io/blog/7-things-ive-learned-building-a-modern-tui-framework/)).
His [Toad post](https://willmcgugan.github.io/announcing-toad/) explicitly calls out Claude Code /
Gemini CLI flicker as the failure mode this avoids.

**Focus: a real traversal system, and it falls out of the DOM.** Exactly one widget has input focus;
key dispatch searches bindings from the focused widget up the DOM to the App
([input guide](https://textual.textualize.io/guide/input/)). Focusability is two class-level flags —
`can_focus` and `can_focus_children` (the latter lets a container remove its whole subtree from
traversal) — and Tab/Shift+Tab walk `Screen.focus_chain`, with `focus_next()`/`focus_previous()`
optionally filtered by CSS selector ([Screen API](https://textual.textualize.io/api/screen/)). There
is _no_ tab-index attribute: the chain is computed by DOM traversal with siblings sorted by
on-screen position (`_focus_sort_key` = (y, x) of the widget's virtual region), and a `_trap_focus`
marker confines the chain inside modal screens (`screen.py`/`widget.py`, Textualize/textual main).
Focus/Blur events fire on change, and TCSS exposes `:focus` _and_ `:focus-within`
(ancestor-of-focused) as styling hooks. This all works because widgets are persistent objects —
focus is just a reference to a node that survives every repaint, which is precisely what
immediate-mode frameworks have to fake with IDs and hashing. Three container algorithms (`vertical`,
`horizontal`, `grid`) plus `dock` (remove from flow, pin to an edge) plus `offset`, with `fr`
fractional units, `auto` content sizing, and row/column spans
([layout guide](https://textual.textualize.io/guide/layout/)). Fractional math uses Python's
`fractions.Fraction` so rounding never leaves 1-cell gaps
([7 things](https://www.textualize.io/blog/7-things-ive-learned-building-a-modern-tui-framework/)).
This modest vocabulary covers essentially every Textual app ever shipped.

**CSS (TCSS):** type/id/class/universal selectors, pseudo-classes (`:hover`, `:focus`,
`:focus-within`, `:disabled`), descendant/child combinators, specificity (ids > classes > types),
`!important`, `$variables`, nesting with `&`. Type selectors match _base classes_ (styling `Static`
styles all subclasses). `textual run --dev` live-reloads CSS into the running app
([CSS guide](https://textual.textualize.io/guide/CSS/)).

**Workers:** `@work` on a method spawns a managed background task without `await`; `exclusive=True`
auto-cancels the previous worker in the group (kills the out-of-order-response bug class);
`@work(thread=True)` wraps blocking APIs, with `App.call_from_thread()` as the only sanctioned UI
re-entry; lifecycle is observable via `Worker.StateChanged`
([workers guide](https://textual.textualize.io/guide/workers/)). This exists explicitly because raw
asyncio in handlers was a gotcha farm.

**Testing:** `app.run_test()` runs headless and returns a `Pilot` (`press`,
`click("#selector", control=True)`, `pause()` to drain queues). `pytest-textual-snapshot` renders
the app to SVG and diffs against a stored snapshot with an HTML side-by-side report
([testing guide](https://textual.textualize.io/guide/testing/)). Textual's own suite: 3000+ tests in
~22s under pytest ([#5068](https://github.com/Textualize/textual/discussions/5068)).

**Company status:** Textualize the company wound down in May 2025 — McGugan: the framework was
"mature and battle-tested" but they "struggled to identify a viable commercial problem"
([future of Textualize](https://textual.textualize.io/blog/2025/05/07/the-future-of-textualize/)).
Maintenance mode has _not_ meant decay: GitHub API shows v8.2.8 released 2026-06-30, commits by
McGugan that week, ~36.5k stars, 301 open issues — but it is a one-person show, and he shipped
**eight major versions between 1.0 (Dec 2024) and 8.x (Jun 2026)** (releases API), i.e. breaking
changes kept coming after "1.0".

## What it gets right

- **CSS is the most-loved feature for a reason:** it makes _third-party_ widgets themeable without
  forking them, gives designers/users a theming surface, and `--dev` live reload turns styling into
  a sub-second feedback loop. No other TUI framework has this; it's the #1 thing people cite when
  choosing Textual over ratatui/bubbletea (see HN 40926211: Textual apps as "de-facto basis for
  cross-platform gui apps").
- **The compositor is genuinely better than double-buffer diffing:** occlusion-aware layers give
  free modals/overlays/tooltips; partial updates + synchronized output give flicker-free rendering
  that McGugan credibly contrasts with Claude Code/Gemini CLI
  ([Toad post](https://willmcgugan.github.io/announcing-toad/)); the spatial map keeps 1000-widget
  scrolling smooth.
- **Workers tamed async:** the `exclusive=True` cancel-previous semantic encodes the single most
  common TUI concurrency bug (stale response wins) into one keyword.
- **The actor-per-widget message pump serializes handler execution per widget** — no data races on
  widget state, ordering is comprehensible, and bubbling + `@on(selector)` handler targeting scales
  to big apps.
- **SVG snapshot testing** made visual regression testing normal in the ecosystem; the interactive
  diff report is the killer detail.
- **Ecosystem proof:** harlequin (SQL IDE), posting (HTTP client), toolong (log viewer), memray's
  TUI all shipped on it — the retained-DOM + CSS model demonstrably scales to real applications.

## What users complain about

- **DataTable performance — the flagship complaint.** tconbeer (harlequin) rewrote it: built-in
  DataTable takes ~63s to load a 538k-row dataset vs 0.077s for
  [textual-fastdatatable](https://github.com/tconbeer/textual-fastdatatable) (~800×), because the
  built-in eagerly ingests everything into Python objects instead of using a pluggable columnar
  backend. Related: [#4737](https://github.com/Textualize/textual/issues/4737) (focus switching slow
  with big tables), [#5953](https://github.com/Textualize/textual/discussions/5953) (slowdown scales
  with _columns_; `render_cell` re-derives console options per cell). Never fixed in core through
  v8.
- **Even Textualize routed around its own widgets:** toolong doesn't use built-in scrollables for
  logs — it ships a custom `log_lines.py` ScrollView plus its own file scanner/watcher
  (`Textualize/toolong` `src/toolong/`), because gigabyte files require virtualization the stock
  widgets don't offer.
- **Release churn:** "you get a new release every other week, and stuff breaks in somewhat
  unpredictable places. So it's easy to prototype something with Textual, but hard to maintain it
  afterwards" — pudo, [HN 37174657](https://news.ycombinator.com/item?id=37174657), acknowledged
  in-thread by Textualize's davepdotorg. The 8 majors in 18 months post-1.0 (releases API) show this
  never stopped.
- **"Fake CSS" uncanny valley:** "I immediately ran into issues trying to use css that I'm familiar
  with… They should have called it something else and used different syntax" — darkstar999, same HN
  thread.
- **Perceived sluggishness / animation defaults:** "they made the terminal feel slow… The 500ms
  'lag' is annoying for power users" — ramses0, same thread. Python startup and input latency come
  up in every comparison with ratatui.
- **Input/keyboard protocol lag:** kitty keyboard protocol enhancements were never fully enabled, so
  shift+enter, shift+space etc. were indistinguishable
  ([#6074](https://github.com/Textualize/textual/issues/6074), opened Aug 2025, i.e. during
  maintenance mode).
- **Focus-chain edge cases, not the core design.** The traversal model itself draws few complaints;
  the bugs are at the seams: `focus()` on a non-focusable container is a silent no-op rather than
  descending to the first focusable child
  ([discussion #2186](https://github.com/Textualize/textual/discussions/2186)); focusing a widget
  inside a non-active tab focuses it without switching tabs, leaving focus on an invisible widget
  ([#4593](https://github.com/Textualize/textual/issues/4593)); Markdown accidentally made every
  list item focusable, polluting the chain
  ([#2380](https://github.com/Textualize/textual/issues/2380)); and click-to-focus fires Focus
  before the mouse event, so widgets can't tell _why_ they were focused, causing TextArea cursor
  glitches ([#4364](https://github.com/Textualize/textual/issues/4364)).
- **Test-speed footguns:** typing 16 chars took ~15s under `unittest.IsolatedAsyncioTestCase`; the
  fix was "use pytest-asyncio" ([#5068](https://github.com/Textualize/textual/discussions/5068)) —
  the framework is sensitive to event-loop-per-test overhead and does nothing to protect you.
- **Maintenance mode reveals:** the design is coherent enough for one person to keep shipping
  majors, but structural issues (DataTable, kitty protocol) sit unfixed for years while the
  community forks widgets instead of patching core — a monolithic `textual` package means widget
  fixes are gated on one maintainer.

## What's worth stealing

- **The compositor algorithm** (cuts → chops → occlusion → merge, per line) and the **spatial map**
  for culling/hit-testing. This is the best-documented damage-region design in any TUI framework,
  and it's what makes layers, modals, and partial repaints cheap.
- **CSS with live reload**, including: base-class-matching type selectors (theme a whole widget
  family), `$variables`, pseudo-classes driven by widget state, and specificity rules. The _feature_
  to steal is the dev loop, not web-CSS compatibility.
- **Reactive descriptors** — the `validate_/watch_/compute_` triad and `layout=True` vs content-only
  invalidation. Also steal the _lesson_: mutation-detection for collections must be designed in, not
  bolted on (`mutate_reactive` is an admission of defeat).
- **`@work(exclusive=True)`** semantics as a structured-concurrency worker API, and
  `call_from_thread` as the single blessed thread→UI channel.
- **Pilot + SVG snapshot testing** as a first-class core deliverable, including the HTML diff
  report.
- **Exact rational/fixed-point arithmetic in layout** (`Fraction`) so `1fr 1fr 1fr` never leaves a
  gap column.
- **Segments (styled runs) as the paint primitive**, not per-cell grids — it's how Textual handles
  double-width chars and keeps memory/bandwidth down.
- **Modest layout vocabulary**: dock + linear + grid + fr + offset covered the entire ecosystem.
  Nobody missed flexbox wrap or cassowary.

## Implications for rabbitui

- **Rendering: build a compositor, not (only) a diffed double buffer.** Implement Textual's
  cuts/chops/occlusion pass with CSS-style named layers for z-order, plus a spatial map for culling
  — because overlays/modals/tooltips and per-widget partial repaint are where cell-diff
  architectures (ratatui) hurt most, and qwertty's async layer can exploit region-scoped writes +
  synchronized output.
- **Programming model: retained tree + reactive attributes is proven at app scale; pure per-frame
  immediate mode is not (in this ecosystem).** Offer Xilem-style diffing as a _per-widget opt-in_
  (Textual's `recompose=True` shows the demand) rather than the global model — because Textual
  demonstrates that widget-local state + watch/compute hooks is enough for harlequin-sized apps,
  while recompose-everything is only wanted for small dynamic regions.
- **State invalidation must distinguish "repaint content" from "relayout"** (Textual's
  `layout=False` default) — because conflating them is the difference between 60fps typing and
  layout storms; make it a type-level property of the reactive field.
- **Layout: don't reach for cassowary.** Ship dock + linear + grid + `fr` with exact
  (integer/rational) division first; taffy-style flexbox is optional sugar — because Textual's
  entire ecosystem shipped on the modest vocabulary, and exact fractions eliminated an entire
  off-by-one bug class.
- **Styling: ship a hot-reloadable stylesheet language in v0, and name it honestly (not "CSS").**
  Selector matching on widget type _and its supertraits_, `$vars`, state pseudo-classes — because
  live-reload theming is Textual's single biggest adoption driver, and the "fake CSS" complaint says
  diverging syntax under the CSS name costs goodwill. Rust wrinkle: type-selector-matches-base-class
  needs an explicit widget-taxonomy mechanism (trait/marker registration), it won't fall out of
  inheritance.
- **Concurrency: async-first is vindicated; steal `@work` semantics on tokio** — `exclusive`
  cancel-previous groups, a single blessed thread→UI handle — because Textual proves the raw-async
  footguns are common enough to deserve framework-level names. Keep Textual's _serialized handler
  per widget_ guarantee, but implement it as ordered dispatch on one loop, not task-per-widget
  actors (Python needed actors for cooperative fairness; Rust doesn't need 1000 tasks for 1000
  widgets).
- **Input: enable kitty keyboard progressive enhancement from day one**
  ([#6074](https://github.com/Textualize/textual/issues/6074) shows retrofitting is painful and
  stalls); keep DOM-style bubbling plus selector-targeted handlers (`@on(Msg, "#id")`) — the
  combination is what makes large Textual apps navigable.
- **Focus: a retained tree makes focus nearly free — take the whole package.**
  `can_focus`/`can_focus_children` flags, a computed chain (DOM order with visual-position sibling
  sort, no tab-index), selector-filtered `focus_next/previous`, modal focus-trap,
  `:focus`/`:focus-within` styling. This is a concrete payoff of widget identity: focus is a stable
  node reference, not a hashed ID to reconcile per frame. But fix Textual's seams in the contract:
  `focus()` on a container should descend to the first focusable child, focusing an
  off-screen/hidden widget should reveal it (or fail loudly), and focus-reason (keyboard vs mouse)
  should ride on the Focus event ([#2186](https://github.com/Textualize/textual/discussions/2186),
  [#4593](https://github.com/Textualize/textual/issues/4593),
  [#4364](https://github.com/Textualize/textual/issues/4364)).
- **Widgets: every data widget must be virtualized with a pluggable data backend from the first
  release.** The 800× fastdatatable gap and toolong's bypassing of core widgets are the two loudest
  lessons — design the third-party widget trait so a "line/row provider" (lazy, columnar, mmap-able)
  is the contract, and keep widgets in separately-versioned crates so a solo maintainer isn't the
  bottleneck for widget fixes.
- **Testing: core must ship the headless driver (Pilot-equivalent: press/click/pause-until-idle) and
  human-diffable snapshot rendering** — this is cheap to build on a compositor and it's why the
  Textual ecosystem has real test suites; make "drain the message queue deterministically" a
  first-class API so tests never sleep.
- **Versioning: publish a stability policy Textual never had.** Eight majors in 18 months post-1.0
  is the documented cost of not having one; split crates (core protocol vs widgets vs style) so
  churn is contained.
