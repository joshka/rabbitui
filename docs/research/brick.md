# Brick (Haskell) — research memo

**Verdict:** Brick is the most complete pure-functional TUI design in existence — its name-keyed
identity system (viewports, extents, cache, mouse) is the single best idea to steal, but its
lazy-closure widget model and Fixed/Greedy layout do not survive translation to Rust unmodified.

Date: 2026-07-06

Sources (all fetched/read for this memo; source paths are a shallow clone of jtdaugherty/brick @
`868004e` under
`/private/tmp/claude-501/-Users-joshka-local-rabbitui/66f598c5-8e02-44b9-9a24-3c25994bb82b/scratchpad/brick/`,
cited below as `brick/…`):

- <https://github.com/jtdaugherty/brick> (repo; guide at `brick/docs/guide.rst`, 2157 lines)
- `brick/src/Brick/Main.hs`, `brick/src/Brick/Types/Internal.hs`, `brick/src/Brick/Types/EventM.hs`,
  `brick/src/Brick/Widgets/Core.hs`, `brick/src/Brick/Widgets/Internal.hs`,
  `brick/src/Brick/AttrMap.hs`, `brick/src/Brick/Forms.hs`, `brick/src/Brick/Themes.hs`,
  `brick/FAQ.md`
- <https://github.com/jtdaugherty/brick/issues/534> (virtualized table perf; read via
  `gh issue view`)
- <https://github.com/jtdaugherty/brick/issues/275> (variable-height list items; read via `gh`)
- <https://github.com/jtdaugherty/brick/issues/282> (greedy layout confusion; read via `gh`)
- <https://github.com/jtdaugherty/brick/issues/178> (mouse event flooding)
- <https://news.ycombinator.com/item?id=24413445> (HN thread on brick; read via Algolia API)
- <https://github.com/jtdaugherty/brick/blob/master/docs/guide.rst> (fetched rendered copy)

## Core architecture

**Elm loop, five callbacks.** The whole app is an `App s e n` record
(`brick/src/Brick/Main.hs:102-122`): `appDraw :: s -> [Widget n]` (a list of **layers, topmost
first** — z-order is free), `appHandleEvent :: BrickEvent n e -> EventM n s ()`, `appStartEvent`
(pre-first-frame init, e.g. initial scroll requests), `appChooseCursor` (widgets _propose_ cursor
positions, the app picks the winner), and `appAttrMap :: s -> AttrMap`. Three type parameters: state
`s`, custom event `e`, and — the key move — a user-defined **resource name type `n`**.

**Widget = lazy render closure.**
`Widget n = { hSize, vSize :: Size, render :: RenderM n (Result n) }`
(`brick/src/Brick/Types/Internal.hs:133-140`). `Size` is just `Fixed | Greedy` per axis
(`Internal.hs:122`). `RenderM` is `ReaderT (Context n) (State (RenderState n))`: the `Context`
carries `availWidth/availHeight`, current attribute name, border style, attr map, scrollbar config
(`Internal.hs:434-455`). Layout is negotiation, not a solver: `renderBox` renders all `Fixed`
children first under the remaining budget, then divides leftover space evenly among `Greedy`
children, then pads all images to the max secondary dimension
(`brick/src/Brick/Widgets/Core.hs:668-720`). One pass, no constraints, no reflow.

**Result is the composition currency.** A widget's render returns
`Result { image, cursors, visibilityRequests, extents, borders }` (`Internal.hs:359-395`) — not just
pixels. Cursor candidates, scroll-into-view requests, reported geometry, and border-joining metadata
all _bubble up_ through container widgets, which translate their offsets. This is much richer than
ratatui's write-into-`Buffer` model.

**Names are the identity system.** The `n` type keys everything that persists across frames:
`RenderState` holds `viewportMap :: Map n Viewport` (scroll offsets),
`renderCache :: Map n (…, Result n)`, `clickableNames`, `reportedExtents :: Map n (Extent n)`
(`Internal.hs:142-150`). `viewport name Vertical w` (`Core.hs:1407`) gets its scroll offset from
last frame's map; `reportExtent n w` (`Core.hs:238`) records the post-layout rectangle;
`clickable n w` is literally `reportExtent` plus registration for mouse dispatch
(`Core.hs:260-264`); `cached n w` (`Core.hs:1242`) memoizes the rendered image until
`invalidateCacheEntry`. Duplicate names are a **runtime `error`** with a long apology message
(`Core.hs:1431-1444`).

**EventM is a monad-transformer stack**: `ReaderT (EventRO n) (StateT s (StateT (EventState n) IO))`
(`brick/src/Brick/Types/EventM.hs:25-27`) with `MonadState s`, so handlers mutate app state
"directly," and lens `zoom` (`EventM.hs:45`) delegates events to component sub-state (this
state-monad shape was the brick 1.0 breaking refactor, issue #379). Scroll commands (`vScrollBy`,
`makeVisible`, `invalidateCache`) don't act immediately — they queue requests applied at the next
render (`Main.hs:575-664`).

**Event loop:** `customMain` takes an optional `BChan e` for app-generated events; a forked thread
pumps vty input into the same channel (`Main.hs:229-329`); every event triggers a full `appDraw` and
re-render; **vty (the backend) diffs the picture** against the previous frame. `suspendAndResume`
releases the terminal for subprocesses. Async = "spawn threads, write to the BChan."

**Theming:** `AttrMap` maps _hierarchical_ attribute names to partial attrs; lookup of
`parent <> child` merges child over parent over map default, so specific styles inherit what they
don't specify (`brick/src/Brick/AttrMap.hs:3-21`). `Brick.Themes` adds named themes with
documentation strings and **INI-file user customization** (`loadCustomizations`, `themeToAttrMap`,
`Themes.hs:69-144`). `Brick.Keybindings` does the same for keys (guide.rst:1628).

**Forms** (`brick/src/Brick/Forms.hs`): a `Form s e n` pairs a state type with lens-addressed fields
(`editTextField`, `checkboxField`, …); it auto-manages focus rings, per-field validation
(`setFieldValid`), and rendering, with `@@=` to decorate a field's widget (`Forms.hs:224-256`).
Type-safe forms from a record + lenses, ~zero boilerplate.

**Testing:** `renderWidget` is exposed as a pure widget→`Picture` function for headless/golden tests
(`brick/src/Brick/Widgets/Internal.hs:177-188`; used in `brick/tests/Render.hs`).

## What it gets right

- **The user guide.** 2157 lines of _narrative_, ordered by mental model, not API: it teaches space
  negotiation and Fixed/Greedy _before_ showing widgets, states each design's rationale (e.g. why
  the app, not widgets, picks the cursor — `Main.hs:106-113`), and pairs every feature with a demo
  program (34 under `brick/programs/`). HN users specifically call out "Docs are very good too"
  (<https://news.ycombinator.com/item?id=24413445>).
- **One identity namespace.** A single `n` type unifies scroll state, geometry reporting,
  hit-testing, caching, and focus. Nothing else in TUI-land is this coherent; ratatui users reinvent
  each of these ad hoc.
- **Extents answer "where did that widget end up?"** — the classic immediate-mode blind spot.
  Handlers query last frame's geometry (`lookupReportedExtent`, `Internal.hs:461-464`); mouse
  support falls out of it for free.
- **Visibility requests**: `visible w` at render time bubbles a scroll-into-view request to the
  nearest enclosing viewport (guide.rst:1210+). Declarative scroll-into-view beats manual offset
  math.
- **Framework state vs app state split.** Scroll offsets, extents, and cache live in `RenderState`,
  keyed by name — the app's Elm-model stays pure and serializable.
- **Border joining** via `BorderMap DynBorder` in `Result` (`Internal.hs:376-390`): adjacent
  widgets' borders rewrite each other's edges into proper T-junctions. No other framework does this
  compositionally.
- **Escape hatches are honest**: `cached` + explicit invalidation, `Widget … $ do getContext` for
  custom size-aware widgets, `suspendAndResume`, `customMainWithVty` for owning the backend.

## What users complain about

- **Performance on large UIs.** Issue #534: a naive 500-row table — "rendering performance
  absolutely plummets even with moderately large (±500-row) tables." Maintainer's answer: the
  built-in `List` virtualizes only by _requiring uniform item height_, and getting the viewport
  height to virtualize anything else requires writing a custom `Widget` against `RenderM` (his words
  in #534: "it isn't possible to do the former without the latter"). Issue #275 confirms
  variable-height list items are officially outside the efficient path. Root cause: `appDraw`
  rebuilds and renders the whole tree every event; laziness hides the tree-construction cost but not
  the render cost inside viewports.
- **Event-loop latency under mouse load.** Issue #178: full redraw per event took 10–15ms while drag
  events arrived faster, so "the event loop gets clogged and events continue to arrive for some time
  after I unpress mouse button."
- **Layout expressiveness.** `Fixed`/`Greedy` is the entire vocabulary. Issue #282 ("Confused about
  how greedy layout works") shows even the two-policy model confuses users; grids (#371) and
  navigable/scrollable tables (#417, #422) took years of ad-hoc widget work; `hLimitPercent`-style
  combinators are patches over the missing middle (no weights, no min/max, no baseline).
- **Learning curve.** The `EventM` transformer stack plus lens `zoom` for component state is
  idiomatic Haskell and a wall for everyone else; brick 1.0 had to do a breaking `EventM`
  state-monad refactor (issue #379) to make handlers tolerable. The `n` discipline is powerful but
  duplicate names crash at **runtime** (`Core.hs:1436`).
- **Wide chars/emoji are officially "avoid"**: brick's own FAQ says the "current recommendation is
  to avoid use of wide characters" because vty's width table and the emulator's disagree
  (`brick/FAQ.md`).
- **Unix-only.** vty was historically POSIX-only ("This doesn't support windows" — HN 24413445;
  ghcup's TUI was blocked on it, <https://github.com/haskell/ghcup-hs/issues/208>).

## What's worth stealing

1. **The name/resource-ID system** — one ID type keying scroll state, extents, hit regions, cache,
   and focus, stored framework-side across frames.
2. **`reportExtent` / post-layout geometry map** queryable in event handlers; `clickable` as a
   one-line derivative.
3. **Render-time visibility requests** that bubble to the enclosing viewport, plus event-time scroll
   _requests_ queued and applied at next render (never mid-frame mutation).
4. **Cursor candidates bubbling up + app-level chooser** (`appChooseCursor`).
5. **Hierarchical style names with partial-attr inheritance** + a `Theme` layer with user-editable
   override files — 90% of Textual CSS at 10% of the machinery. Same INI pattern for keybindings.
6. **A `Result`-like composition currency** (cells + cursors + visibility reqs + extents +
   border-join metadata), which is what lets third-party widgets participate in border joining and
   scroll-into-view.
7. **The guide's structure**: mental model first, rationale stated, one runnable demo per feature.
8. **Pure `renderWidget` for headless snapshot tests.**

**What only works because of Haskell — do not port literally:**

- `Widget` as a lazy closure is free to _construct_ in Haskell (thunks); the equivalent in Rust is a
  `Box<dyn FnOnce>` per node per frame — allocation and virtual-call cost with no laziness payoff.
  Rust wants either eager lightweight view structs (Xilem-style, diffable) or retained nodes.
- `appDraw` purity is enforced by types; in Rust it's a convention you must design for (e.g.
  `&AppState` not `&mut`).
- Lens `zoom` for component state delegation — Rust gets this _better_ for free via
  `&mut state.field`.
- The `EventM` transformer stack exists to smuggle three kinds of state through a pure language; in
  Rust it's just a context struct with `&mut` fields. Brick's own #379 refactor shows even
  Haskellers wanted it flatter.

## Implications for rabbitui

- **Build virtualization into scrollable containers from day one.** Brick's #1 real-world perf
  complaint (#534, #275) is retrofitting it. Concretely: a viewport must hand its child the allotted
  `Size` _during build_ (two-phase: measure/allot, then build visible children only), and lazy lists
  must support variable-height items via a height-estimate + measured-cache, not brick's
  uniform-height requirement.
- **Adopt a `WidgetId` namespace keying framework-owned per-ID state** (scroll offsets, extents,
  focus, cache, hit regions) in a frame-persistent map, separate from app state. Enforce uniqueness
  with debug asserts + a `#[derive]`-friendly ID type instead of brick's runtime `error`
  (`Core.hs:1436`). This is _the_ answer to "widget identity & state ownership across frames" for an
  Elm/immediate-mode loop.
- **Ship `report_extent` semantics in the core render contract**, not as a widget add-on: after
  layout, the framework records `Rect` per ID; event handlers read last-frame geometry; mouse
  dispatch is derived. This costs one HashMap and eliminates the whole "how do I know where my
  widget is" class of issues.
- **Make widget render output a struct, not just buffer writes**:
  `{cells, cursor_candidates, visibility_requests, extents, border_joins}`. That's the trait surface
  third-party widgets need to compose properly (border joining, scroll-into-view, IME cursor) —
  richer than ratatui's `Widget::render(area, buf)`.
- **Don't copy Fixed/Greedy as the layout vocabulary** — it's brick's most-confused feature (#282)
  and its ceiling (#371, #417). Use taffy/flexbox with content measurement, but _keep_ brick's
  defaults-are-simple property: `vbox![a, b, c]` must work with zero layout annotations.
- **Full rebuild + backend diff is an acceptable baseline** (brick + vty proves it), but budget for
  input coalescing: brick's 10–15ms frames caused mouse-drag event floods (#178). qwertty being
  async-first, rabbitui should drop/merge intermediate frames and coalesce mouse-move events before
  they reach `update`.
- **Queue side-effectful UI commands (scroll, invalidate, focus) and apply at next render** rather
  than mutating render state from event handlers — brick's request queues (`Main.hs:575-664`) keep
  the loop deterministic and are trivially testable.
- **Theming: hierarchical style keys with partial-style merge + TOML theme overrides** before
  considering full CSS. Brick demonstrates this covers real apps (matterhorn) with a fraction of
  Textual's engine.
- **Forms as a derive macro** over a struct (field widgets + validation + focus ring) — brick's
  lens-based `Forms` shows the shape; Rust's `#[derive(Form)]` can beat it ergonomically.
- **Expose a pure `render_to_buffer(widget, area) -> Buffer` for snapshot tests** (brick's
  `renderWidget`, `Widgets/Internal.hs:177`), independent of qwertty.
- **Invest in the guide like brick did**: a narrative doc that teaches space negotiation, identity,
  and the event loop with a runnable example per concept. Brick's adoption despite Haskell's
  audience ceiling is substantially a docs phenomenon (HN 24413445).
