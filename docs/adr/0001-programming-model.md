# ADR 0001: Declared-frame architecture as the core programming model

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

The load-bearing decision before all others: what is the contract between app and framework each
frame? Everything downstream (rendering, layout, input, widgets, testing) is shaped by it. The
evidence pulls three ways.

**Imported paradigms fight Rust and lose adoption.** Every "next-gen Rust TUI" that imported a whole
GC-language paradigm died with its author's app or plateaued (docs/research/prior-art.md).
React-in-Rust leaks runtime rules as panics (iocraft `State::write` in `use_future`, #48; intuitive
"Hooks don't work… unmaintained"). Elm message enums metastasize (tui-realm's `Msg`/`Cmd` double
bookkeeping is "tolerated, not loved," #180/#21/#173). Retained trees force
`Rc<RefCell>`/`Arc<Mutex>` gymnastics (cursive). rooibos shipped the _complete_ signals+taffy+async
synthesis and has 5 stars — "the most important cautionary tale in the survey": it shipped
architecture without a catalog, docs, or flagship app. Architecture novelty buys nothing; users ask
for spinners/checkboxes/text-areas and a blessed loop, not reconcilers (HN 45830829).

**Rust taxes reactivity, not pixels.** Ten years of Rust GUI work
(docs/research/rust-gui-lessons.md) reduce to five problems: tree ownership, widget identity,
incrementality, state scoping, text/rendering depth. Terminals dodge the last almost entirely (a
frame is ≤~100k cells, a buffer diff is microseconds, no damage regions, no GPU compositor, no
shaping). But identity, ownership, and state scoping "arrive in a TUI undiminished, because they are
Rust problems and reactivity problems, not pixel problems." Every survivor agrees on one
requirement: stable widget identity across frames, carried as id-paths (Xilem) or arena keys
(Masonry), for focus, scroll persistence, cursor/IME position, and any future accessibility export
(ratatui #2190). Pure immediate mode "fundamentally struggles" here.

**The labs already ran the experiment.** ratatui-labs implemented the _same command strip four ways_
(manual bookkeeping, frame-snapshot primitives, a zone primitive, a component shell); all four
produced identical behavior, and the recurring cost was **previous-frame target bookkeeping**
(prior-art.md). Its creed: _render produces visible facts → input routes through previous visible
facts → controls return outcomes → the app owns effects_ — immediate-mode-compatible,
borrow-checker-friendly, proven bottom-up against real terminal widgets.

## Options considered

**A. Xilem-style view/element split** (the retained rival from GUI evidence; rust-gui-lessons.md;
<https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html>). An ephemeral typed view
tree rebuilt each cycle, diffed against a long-lived type-erased widget tree, plus a view-state
tree; one `&mut AppState` threaded down id-paths to leaf callbacks. _Steelman:_ the most principled
answer to all five problems at once — structural identity, one `&mut` with no interior mutability,
incrementality free via diffing/memoize nodes; Masonry⇄Xilem proved the reactive layer and widget
engine are separable products; the memo recommends it as _the_ model and holds accessibility
"eliminates pure immediate mode as viable for production." _Why not:_ the payoff is incremental
_view computation_ plus identity. On a ≤100k-cell grid full re-render is microseconds, so
incrementality mostly evaporates (the memo concedes its cost argument "is calibrated to GUI-scale
trees"). Identity — the split's essential payload — is already captured as data by the id store
below, _without_ a retained element tree (prior-art.md: "the labs carry id-paths without a retained
tree"). What remains is pure cost: three parallel trees, view-state plumbing, a diffing contract
every widget must honor. The accessibility argument is _assumed, not verified_ — it presupposes an
AccessKit-style terminal export nobody has built; Levien himself hedges ("you never know until you
actually try it"). The memo flags this as "an open fork, not a decision." Memoization is addable
_within_ the declared-frame model later — we keep the option without paying upfront.

**B. Retained tree + reactive attributes** (Textual's proven model; docs/research/textual.md). A
persistent widget tree; state lives _in_ widgets as reactive descriptors (validate/watch/compute,
content-vs-layout invalidation); a compositor does partial repaint; focus falls out of the DOM.
_Steelman:_ the only model proven at real app scale here — harlequin, posting, toolong shipped on it
("the most complete existence proof that a browser-grade retained DOM works in the terminal"); focus
is nearly free (a stable node reference, not a hashed id to reconcile), and `recompose=True` shows
view-diffing is wanted only for small dynamic regions. _Why not:_ every Rust instance of a _public_
retained tree pays the borrow tax in API shape. Cursive is the proof (docs/research/cursive.md):
string-name selectors that fail silently on typo _or_ type mismatch; `Arc<Mutex>` per `NamedView`
with documented double-borrow panics (named_view.rs:53-56) and silent try_lock no-ops (:83-88); a
deferred-callback vocabulary forced because `on_event(&mut self)` can never receive `&mut Cursive`.
Retention also does _not_ buy partial redraw — Cursive redraws the whole tree and diffs a buffer
anyway (#667), exactly as ratatui does. Textual's reactive collections needed the `mutate_reactive`
escape hatch — "an admission of defeat." The declared-frame model keeps the retained _data_ (id
store + facts) this option is really after, without a retained _object tree_ as the public contract.
(Textual's superior parts — compositor, live-reload theming, virtualization — are adopted in ADRs
0003/0007/0008, decoupled from the tree decision.)

**C. Pure Elm / TEA** (Bubble Tea's teachable monolith; docs/research/bubbletea.md). One model,
`Init/Update(Msg)/View`; all input/timer/IO arrives as messages through a serialized `Update`;
effects are commands (`Cmd = func() Msg`). _Steelman:_ the most teachable TUI model in existence
(18k+ dependents), shortest tutorial-to-app distance; serialized `Update` kills data races by
construction; deleting subscriptions (commit `ade8203c`) was correct and never missed. _Why not:_
composition is its #1 complaint, unresolved 5+ years — the parent hand-routes every message to every
child and reassembles returned models (#176), with no way to know which child a reply belongs to
(#751) and no focus system (every app hand-rolls a `focusIndex` loop). "Every serious app rebuilds a
component runtime by hand." v2 concedes this by moving rendering, layers, and terminal state _out_
of the model. Lesson: keep the serialized loop as a _runtime contract_, never as the user-facing
component model. (Its good parts — `Cmd` as the sole effect primitive, panic-catch-and-restore, the
declarative view struct — are adopted below and in ADR 0005.)

**D. Pure immediate mode** (ratatui as-is). Stateless per-frame render; app owns all state; no
framework retention. _Steelman:_ zero ownership fights, 36M downloads, the substrate everyone uses;
borrow-checker-friendly by construction. _Why not:_ it cannot give stable identity — "the one thing
ratatui's immediate-mode model cannot give you." Focus, scroll persistence, cursor position, and
durable selection all require frame-surviving state, so every app reinvents target bookkeeping (the
four-way study) and widget crates fragment on version skew (HN 45830829). Immediate mode is the
_floor_ we build on, not the contract.

## Decision

rabbitui adopts the **declared-frame architecture** as its single core programming model:

1. **App state is plain Rust owned by the app.** The framework never owns, wraps, lenses, or adapts
   application state (Druid's lenses are a documented dead end). The app may be an async state
   machine consuming messages — the model the labs validated and the one that composes best with
   async Rust.
2. **Widgets have stable identity from v0.1.** Every instance is addressed by a `WidgetId` from user
   keys composed into id-paths (Xilem-style nesting). The framework keeps a per-id store across
   frames: focus, scroll offsets, cursor, collapsed/expanded, extents, caches. This is data, not a
   retained object tree.
3. **Rendering produces facts.** A render emits, besides cells: hit regions, focus order, cursor
   candidates, extents, visibility requests — a queryable "frame facts" record.
4. **Input routes through the previous frame's facts.** Events run capture → target → bubble against
   the facts tree; controls consume events and return typed **outcomes** (`Submitted`,
   `SelectionChanged`, `Dismissed`, …) to the app on the next update.
5. **Effects are app-owned, commands-only.** Async work is futures/streams whose results re-enter
   the loop as messages. There is **no** subscription primitive (Bubble Tea deleted theirs and never
   missed them). Panics in effect tasks are caught; the terminal is restored.
6. **Optional shells layer above, never below.** An Elm-style `rabbitui-tea` and a Xilem-style
   view-diff/memoization layer are optional crates _over_ the core, never the widget contract.

### Worked example (sketch)

```rust
struct App { query: String, results: Vec<Row>, loading: bool }   // plain owned state
enum Msg { Loaded(Vec<Row>) }

fn render(app: &App, f: &mut Frame) {                 // declares widgets by key
    let [top, body] = f.rows([Length(1), Fill(1)]);
    f.widget(key("search"), TextInput::new(&app.query)).area(top);   // cursor/focus
    let list = List::new(&app.results).loading(app.loading);         // persist by id,
    f.widget(key("results"), list).area(body);                       // not in app state
}

fn update(app: &mut App, ev: Event) -> Option<Cmd<Msg>> {   // outcomes from prev-frame facts
    if let Some(Outcome::Changed(q)) = ev.outcome_for(key("search")) {
        app.query = q.clone(); app.loading = true;
        return Some(Cmd::future(async move { Msg::Loaded(search(q).await) })); // app-owned effect
    }
    if let Some(Outcome::Submitted) = ev.outcome_for(key("results")) { /* open row */ }
    None
}

fn on_msg(app: &mut App, Msg::Loaded(rows): Msg) { app.results = rows; app.loading = false; }
```

No `Rc<RefCell>`, no per-node lock, no message hand-routing to children, no `focusIndex` loop:
identity and focus are framework-owned by `WidgetId`; the widget is a short-lived _spec_ rendered
against retained per-id state; effects are plain futures.

## Consequences

**Positive.** Borrow-checker-friendly with no interior-mutability web (dodges cursive's scar tissue
and iocraft's hook panics). Stable identity, focus, and scroll persistence from day one without a
retained object tree. Async composes naturally (effects are futures). Frame facts _are_ data, so the
headless driver and snapshot/replay harness (ADR 0009) come nearly free. Widgets stay small and
inferrable — serving third-party authors and coding agents. The core stays paradigm-neutral, so Elm-
and Xilem-preferring users get shells without the core owning a paradigm.

**Negative (honest).** Routing uses **one-frame-stale facts** — an event hit-tests against the
_previous_ paint. This is how every GUI hit-tests against the last rendered frame and is immaterial
at terminal event rates, but the API must document it (a widget appearing and clicked in the same
frame is not routable until the next). There is **no automatic fine-grained invalidation** — the
framework does not know which subtree changed, so the default is redraw-and-diff; on pathological
large dynamic views, re-running user view code every frame is real cost the model does not amortize
by itself. Scope honesty: the four-way study validated this for _interaction bookkeeping_ on one
command strip, **not yet** for large dynamic views (labs roadmap steps 8–9).

**Neutral.** Retained state lives in an id-keyed store, not a widget tree — equivalent power,
different shape; devtools query facts, not nodes. Widgets are specs, not objects, so "holding a
reference to a widget" is not a concept users have (they hold ids, read outcomes). Overlays use
scoped id-paths (local ids translated to parent coordinates, z-ordered), not tree insertion.

**Mitigation paths (pre-designed, opt-in).** Stale-facts cost is bounded by cheap redraws. The
invalidation cost has two escalators, both addable _within_ the model without changing the contract:
(1) **per-widget memo nodes** — `Arc::ptr_eq`/skip-empty-delta memoization on view subtrees; (2) the
**`rabbitui-tea` MVU shell**. If reference apps (step 9) hit view-construction cost, or
accessibility work (ratatui #2190) demands a persistent identified tree, the Xilem-style view-diff
layer is promoted from optional to default — the id store and facts tree are designed to make that
additive, not a rewrite.

## Revisit triggers

- **Large-dynamic-view cost:** a reference app (step 9) profiles >~2ms/frame in user view
  construction on a realistic model and per-widget memo nodes do not recover it → promote the
  Xilem-style view-diff layer to default.
- **Accessibility forcing-function:** an AccessKit-style terminal export (ratatui #2190) ships and
  demonstrably requires a persistent identified widget tree rather than id store + facts → reopen;
  the "assumed, not verified" premise would have become verified.
- **Stale-facts correctness failures:** one-frame-stale routing produces user-visible mis-routing
  (e.g. click-through on freshly-appeared overlays) unfixable at the facts layer → reopen the
  routing model (possibly same-frame layout before dispatch).
- **Outcome/spec grammar breakdown:** a generic `Component` trait keeps being demanded (the
  tui-realm `MockComponent` / ratatui #1969 failure mode) → reopen the widget contract.
- **Shell demand inverts:** if the optional shells attract the overwhelming majority of real apps
  while the raw declared-frame API goes unused → reconsider which layer is "core."

## Amendments

- **2026-07-07 (benchmarks):** The Context's "full re-render is microseconds" is now measured (Arc
  2B, Apple M2 Max, release): 0.51 ms at 1,000 widgets, 1.69 ms at 10,000, virtualized scroll at
  10,000 items 1.29 ms. The literal wording was ~10× optimistic; the architectural conclusion stands
  — all measured cases sit far inside a 60 fps frame budget, and the first revisit trigger (">~2
  ms/frame in user view construction on a realistic model") remains untripped by an order of
  magnitude at realistic sizes. Numbers and method: docs/design/arc2b-measurement-scroll.md §
  Benchmark results.
