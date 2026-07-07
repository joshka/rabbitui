# ADR 0002: Framework-owned stable widget identity and per-ID state store

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

The declared-frame architecture (ADR 0001) makes app state plain Rust owned by the app and
rebuilds the UI declaration every frame. That model buys borrow-checker sanity but reopens the
one problem the immediate-mode style cannot answer on its own: **making a widget "the same
widget" across frames.** Focus, scroll position, cursor/IME position, collapse/expand state,
"where did that widget land" (extents), and render caches all must persist between frames and be
addressable, or the framework cannot deliver tab traversal, scroll-into-view, mouse hit-testing,
or async replies routed to the right control.

The GUI literature is unanimous that this problem transfers to any declarative UI undiminished,
because it is a reactivity problem, not a pixel problem: "Focus, accessibility, event routing,
animation, and incremental update all need a widget to be the same widget across frames. Object
references are messy in Rust; the survivors are key/id **paths** (Xilem, Compose) and **arena
keys** (Masonry `tree_arena`, Dioxus, Warp's EntityId HashMap). Pure immediate mode fundamentally
struggles here" (docs/research/rust-gui-lessons.md, citing Raph Levien's "Towards principled
reactive UI" and "Advice for the next dozen Rust GUIs"). Masonry's single biggest 2024 win was
moving from parent-owned children to arena/slotmap storage where "widgets own keys into a shared
structure" (same memo, citing the Xilem 2024 post-mortem).

Every TUI that skipped this paid for it:

- **Bubble Tea** has no identity layer: "no way to know which child a reply `Msg` belongs to;
  users invent `Wrap()`/integer-tag schemes" (docs/research/bubbletea.md, discussion #751), and
  it has no focus system — the canonical `textinputs` example hand-maintains a `focusIndex int`
  the parent increments on tab, calling `Focus()`/`Blur()` on every child. Composition has been
  its #1 complaint, unresolved for 5+ years.
- **libvaxis/vxfw** used raw-pointer identity (`userdata` ptr + `drawFn` ptr) and it is "its
  worst wart": user-built ephemeral widgets "immediately dangle" and the community answer is to
  hand-maintain a stable-pointer list — "the user hand-implements retained state the framework
  should own" (docs/research/libvaxis.md, discussion #232).
- **Cursive** used string names: `NamedView<V>` wraps `Arc<Mutex<V>>` + a `String`; lookup is an
  O(tree) walk with a triple-downcast that returns `None` on a wrong name *or* a wrong type,
  silently, and `ViewRef` double-borrow panics at runtime (docs/research/cursive.md).

The one framework that got identity right is **Brick**: a single user-defined resource-name type
`n` keys everything framework-side that persists — `viewportMap` (scroll offsets), `renderCache`,
`clickableNames`, and `reportedExtents` — while the app's Elm model stays pure and serializable.
"One identity namespace… nothing else in TUI-land is this coherent" (docs/research/brick.md). Its
one flaw is enforcement: duplicate names are a runtime `error` with an apology message
(`Core.hs:1436`); the memo's explicit recommendation is to "enforce uniqueness with debug asserts
+ a `#[derive]`-friendly ID type instead of brick's runtime `error`."

## Options considered

### A. String-keyed identity (Cursive's `NamedView` / `call_on_name`)

*What it is:* widgets addressed by user-chosen strings; lookup walks the tree.

*Steelman:* zero ceremony to author; names are human-readable in logs; no path composition to
learn. Cursive shipped real apps this way for a decade.

*Why not chosen:* stringly-typed identity "is where Cursive hurts most" — O(tree) lookup, silent
`None` on typos or type mismatch, runtime double-borrow panics (docs/research/cursive.md). Strings
collide silently across independently-authored subtrees with no structural component, so a reused
component breaks the moment two instances exist — the exact failure a nesting scheme prevents.

### B. Raw-pointer / object-reference identity (libvaxis vxfw, retained-OO GUIs)

*What it is:* a widget *is* its heap object; identity is the pointer.

*Steelman:* trivially unique and O(1); no key allocation. Natural in GC'd or arena-per-frame
systems.

*Why not chosen:* it is the vxfw footgun (dangling pointers into user stack frames,
discussion #232) and "exactly what Rust borrow rules make miserable anyway"
(docs/research/libvaxis.md). It also fails the declared-frame contract outright: specs are
short-lived values rebuilt every frame, so there is no stable object to point at. Object
references are "a big wad of shared mutable state" in Rust (docs/research/rust-gui-lessons.md).

### C. Pure structural id-paths, no explicit keys (naive Xilem-lite)

*What it is:* identity is position-in-tree alone (`[1,3]` from root), derived automatically.

*Steelman:* no user annotation at all in the common case; Xilem's id-paths "triple as identity,
event route, and async waker address" (docs/research/rust-gui-lessons.md).

*Why not chosen:* position alone breaks on reordered or filtered dynamic lists — the item at
index 2 becomes a different item, silently swapping its focus and scroll state. The GUI consensus
is explicit that identity "must be structural-position-plus-explicit-keys, carried as paths"
(same memo). We adopt id-paths but require user keys where structure is unstable (option E).

### D. Retained widget tree in an arena, as the public contract (Masonry model)

*What it is:* a long-lived type-erased widget tree in an arena/slotmap; the arena key *is* the
identity; the tree is the public object model users mutate through the framework.

*Steelman:* Masonry's hardest-won lesson; arena keys solve "mutating an item's children while
keeping a live reference to the item's value," and the retained tree "buys the inspector,
serialization, and headless testing for free" (docs/research/rust-gui-lessons.md).

*Why not chosen:* ADR 0001 already rejected a retained *object* tree as the public contract on
terminal-scale evidence — full re-render of a ≤100k-cell grid is microseconds, so the
incrementality the retained tree amortizes mostly evaporates, while its cost (borrow-checker tax
in API shape, per-node locking) is permanent. Crucially, the retained tree's *load-bearing*
payload — stable identity plus a home for retained state — can be captured as **data** (an ID
store + frame facts) without a retained object graph (docs/research/prior-art.md). We keep the
arena/slotmap storage (option E) but store per-ID *state*, not user-facing widget *objects*.

### E. Framework-owned id-paths + per-ID state store in a slotmap (DECISION)

*What it is:* Brick's one-namespace insight, made typed and structural, backed by Masonry's arena
storage, with the retained data but not a retained object tree.

*Steelman:* combines the two ideas the research most strongly endorses — Brick's single
framework-side namespace keying scroll/extents/cache/focus (docs/research/brick.md) and
Masonry/Xilem's arena-keys-plus-id-paths identity (docs/research/rust-gui-lessons.md) — while
honoring ADR 0001's rejection of a retained public object tree. It gives the labs' frame-facts
model (docs/research/prior-art.md) exactly the durable state home it needs. It makes "list of
ephemeral view values" — the case that broke vxfw and Bubble Tea — the *default* that works, not
a footgun.

*Why chosen:* it is the intersection every memo points at. See Decision.

## Decision

rabbitui owns widget identity and the state keyed by it, from v0.1.

1. **Identity is an id-path.** Every widget instance is addressed by a `WidgetId` derived from an
   **id-path**: the ordered composition of a parent's id-path with a child key. A child key is
   either the structural position assigned by the framework (auto, the common case) or an
   explicit user key supplied at the spec site (`Key<T>`, `#[derive]`-friendly). Explicit keys are
   **required** wherever list structure is unstable (reordering, filtering, insertion) and
   optional elsewhere. The id-path doubles as identity, event route, and async waker address
   (Xilem shape).

2. **Collision policy is a debug assertion.** Two live widgets resolving to the same id-path in
   one frame is a programming error. rabbitui `debug_assert!`s on collision with a message naming
   the colliding path; in release builds the later writer wins deterministically (last-write) and
   the collision is recorded as a frame fact for the inspector. rabbitui does **not** panic in
   release (Brick's runtime `error` is explicitly rejected) and does **not** fail silently
   (Cursive's silent `None` is explicitly rejected).

3. **Per-ID state lives in a framework-owned slotmap store.** rabbitui keeps a store, keyed by
   `WidgetId`, backing a generational slotmap (arena keys, Masonry storage; generational to make
   stale keys detectable, not dangling). Containers and user code never hold `&mut` to another
   widget's stored state directly; the framework lends `&mut` slices to a widget only during its
   own dispatch/render scope.

4. **What lives in the store (framework state):** focus (which id holds keyboard input, plus
   per-widget focusable flag; tab order derived from frame facts), scroll offsets / viewport
   state, cursor and IME/preedit position candidates, collapsed/expanded and similar durable
   widget-local UI state, reported **extents** (post-layout `Rect` per id, for hit-testing and
   scroll-into-view), and per-id render/measurement **caches** with explicit invalidation. This
   is Brick's `viewportMap` + `reportedExtents` + `renderCache` + `clickableNames`, unified and
   typed.

5. **What does NOT live in the store (app state):** domain data, the contents a widget displays,
   business logic, and anything the app must serialize or reason about. App state is plain Rust
   owned by the app (ADR 0001); the framework "never owns, wraps, lenses, or adapts application
   state." The split is Brick's: framework state is derived UI bookkeeping keyed by id; app state
   is the model. A widget reads app state by borrow at spec time and mutates it only via returned
   outcomes.

6. **Lifecycle — per-ID state is dropped when the id goes absent.** An id is *present* in a frame
   if a spec resolved to it during that frame's declaration. Per-ID state is retained while
   present and for a bounded grace window after it goes absent; it is dropped when the id has been
   **absent for N frames** (default small, e.g. 1–2, configurable) or on **explicit disposal** by
   the app. The grace window prevents a one-frame flicker (e.g. a widget hidden for a single frame
   during a transition) from destroying scroll/focus state, while bounding store growth. Drop is
   deterministic and reported as a frame fact.

## Consequences

Positive:

- Third-party and coding-agent-authored widgets get focus, scroll-into-view, mouse hit-testing,
  and cursor placement **for free**, because the state those features need is framework-owned and
  keyed uniformly (Brick's coherence, without Brick's Haskell-only laziness).
- The "list of ephemeral widget values" pattern — the exact case that dangled in vxfw
  (discussion #232) and forced hand-rolled routing in Bubble Tea (#751, #176) — is the default
  and works.
- Identity is data (id-paths + slotmap keys), so the headless inspector, snapshot/replay harness
  (ADR 0009), and a future AccessKit-style export (stable ids on specs) come nearly for free —
  "testability & replayability… is the one place rabbitui can exceed the GUI state of the art
  cheaply" (docs/research/rust-gui-lessons.md).
- App state stays pure and serializable; async replies route to a control by id-path without the
  app maintaining a tag scheme.

Negative (honest):

- **Explicit keys are a real ergonomic tax** on dynamic lists — the one place the user must think
  about identity. Get it wrong (reused/omitted key on a reordered list) and focus/scroll state
  attaches to the wrong row. This is inherent to the problem (Compose and Xilem have the same
  tax); we mitigate with debug asserts and by requiring keys only where structure is unstable.
- **The N-frame grace window is a tunable with no perfect default.** Too short drops state across
  legitimate one-frame gaps; too long lets the store hold state for widgets the app considers
  gone. We expose explicit disposal as the escape hatch and document the tradeoff.
- **A store that outlives app state can desync from it** if the app deletes a domain entity but a
  stale id lingers within the grace window — cursor/scroll briefly point at nothing. Bounded and
  self-healing (absent → dropped), but a real transient.

Neutral:

- Store growth is bounded by live-id count plus grace-window residue, not by history; generational
  keys make the store O(live widgets), comparable to Brick's per-frame maps.
- id-paths as async waker addresses reuse the routing machinery, so effects (ADR 0005) and focus
  (ADR 0006) address widgets through the same identity — one mechanism, three uses.

## Revisit triggers

- **Explicit-key ergonomics prove too error-prone**: reference apps (proof-roadmap step 9) or
  coding-agent usage show frequent mis-keyed dynamic lists causing focus/scroll bugs. Revisit
  toward stronger structural inference or a typed `Keyed<T>` container that makes omission a
  compile error.
- **The grace-window default causes visible state loss or unbounded growth** in a real app.
  Revisit the lifecycle rule (e.g. per-widget-kind windows, or eager drop with app-driven
  persistence).
- **Accessibility becomes a forcing function** (ratatui #2190, AccessKit): if a persistent
  identified *tree* (not just a per-id store) is required for AT export, promote the id store to a
  retained node graph — the id-paths already exist, so this is additive, but it reopens ADR 0001's
  retained-tree rejection.
- **View-construction cost on large dynamic views** (rust-gui-lessons.md's "assumed, not verified"
  concern; prior-art.md roadmap steps 8–9): if profiling shows re-deriving id-paths per frame is
  material, add per-id memoize nodes keyed by the same store — a within-model optimization, not a
  new identity scheme.
- **Slotmap contention or generational-key exhaustion** under pathological widget churn: revisit
  the store's backing structure.
