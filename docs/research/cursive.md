# Cursive: the retained-tree existence proof (and its scar tissue)

**Verdict:** Retained-tree TUI in Rust works, but only because Cursive invented a whole workaround
vocabulary — deferred callbacks, string-name selectors, per-node `Arc<Mutex>`, `FnMut`-in-a-`Mutex`
— and every one of those workarounds is a place rabbitui can do better with typed IDs and a mediated
mutation API.

Date: 2026-07-06

Sources:

- Source clone of <https://github.com/gyscos/cursive> at commit `c43afe7e6db2` (2026-07-01).
  File:line citations below are relative to that checkout (read locally).
- Tokio event loop request: <https://github.com/gyscos/Cursive/issues/92>
- `Send` bound complaints: <https://github.com/gyscos/cursive/issues/383>
- Full-redraw / bandwidth bug: <https://github.com/gyscos/cursive/issues/667>
- Per-view styling gap (TextArea): <https://github.com/gyscos/cursive/issues/362>
- Per-view styling gap (TextView): <https://github.com/gyscos/cursive/issues/674> — ANSI color codes
  render as escaped literals in `TextView`; no way to style individual text sections without losing
  wrapping
- Markup/styling direction: <https://github.com/gyscos/cursive/discussions/797>
- tui-rs maintainer on retained vs immediate: <https://github.com/fdehau/tui-rs/issues/654>
- CHANGELOG (Send+Sync breaking change, crossterm default): `CHANGELOG.md` in the repo

## Core architecture

**One owned tree, no references.** `Cursive` owns the entire UI as a single generic type:
`root: OnEventView<ScreensView<StackView>>` (`cursive-core/src/cursive_root.rs:23,38`). Every node
implements `trait View: Any + AnyView + Send + Sync` with `draw(&self, &Printer)`,
`layout(&mut self, Vec2)`, `required_size(&mut self, Vec2) -> Vec2`,
`on_event(&mut self, Event) -> EventResult`, `call_on_any`, `focus_view`, `take_focus`
(`cursive-core/src/view/view_trait.rs:34-153`). There are no parent pointers and no view-to-view
references — Rust's aliasing rules forbid them in a single-ownership tree, and this one fact
generates the entire rest of the API.

**Identity = strings.** A view becomes addressable by wrapping it: `NamedView<V>` stores
`Arc<Mutex<V>>` plus a `String` name (`cursive-core/src/views/named_view.rs:13-16`). The `Selector`
enum has exactly one surviving variant, `Name(&'a str)` (`cursive-core/src/view/finder.rs:83-86`) —
a positional `ViewPath` selector existed and is now vestigial
(`cursive-core/src/view/view_path.rs`). Lookup (`call_on_name`, `find_name`) is a full tree walk:
`call_on_any` recurses through every container (`cursive-core/src/views/linear_layout.rs:682-685`)
passing a type-erased `AnyCb = &mut dyn FnMut(&mut dyn View)` (`cursive-core/src/event.rs:33`), and
the `Finder` impl downcasts three ways at each hit — the type itself, `NamedView<V>`, or
`NamedView<BoxedView>` (`cursive-core/src/view/finder.rs:57-78`). O(tree) per access, `None` on name
typo _or_ type mismatch, silently.

**&mut access, three ways, all workarounds:**

1. `call_on_name(name, |v: &mut V| ...)` — scoped closure inside the tree walk.
2. `find_name::<V>(name) -> ViewRef<V>` — an `ArcMutexGuard` deliberately untied from any lifetime
   (`named_view.rs:23-25`), which is how you hold a "reference" to a view outside the tree. The
   cost: `NamedView::get_mut` **panics if another reference exists** (`named_view.rs:53-56`), and
   the internal `with_view_mut` uses `try_lock` and **silently no-ops** when the view is already
   borrowed (`named_view.rs:83-88`). Reentrancy bugs become skipped updates or panics, discovered at
   runtime.
3. Historically `Rc<RefCell>`; cursive-core 0.4.0 made `View: Send + Sync`, which "prevents using
   `Rc`/`RefCell`, and may require using `Arc`/`Mutex` instead" (`CHANGELOG.md`, cursive-core 0.4.0)
   — a breaking tax paid by every custom view for a multi-threading future that mostly hasn't
   materialized.

**The deferred-callback trick is the load-bearing insight.** A view's `on_event(&mut self)` can
never receive `&mut Cursive` — the app owns the view, so that would be mutable aliasing. Cursive's
answer: handlers return `EventResult::Consumed(Option<Callback>)` where
`Callback(Arc<dyn Fn(&mut Cursive) + Send + Sync>)` (`event.rs:27,259`), and the root runs the
callback _after_ the tree borrow ends (`cursive_root.rs:819-823`). Even `EditView::set_content`
returns a `Callback` to run later (`views/edit_view.rs:361`). Stateful `FnMut` callbacks need
another hack: the `immut1!/immut2!/immut3!` macros wrap the closure in a `Mutex` and skip the call
if reentrantly locked (`cursive-core/src/utils/immutify.rs:76-88`). This is the borrow checker's
fingerprint pressed directly into the API.

**Event loop & concurrency.** Synchronous, owned loop: poll the backend, sleep 30ms when idle
(`INPUT_POLL_DELAY_MS`, `cursive-core/src/cursive_run.rs:7,176`), optional fps-based auto-refresh
(`cursive_root.rs:436`). The single bridge to the outside world is
`cb_sink: Sender<Box<dyn FnOnce(&mut Cursive) + Send>>` — an unbounded crossbeam channel of closures
drained each step (`cursive_root.rs:75,831-839`). Async integration was requested in 2016 (issue
\#92: "supporting async with Tokio is strictly more powerful than not") and never happened; the
polling loop survives to this day.

**Rendering.** Retained tree ≠ damage tracking. Every refresh re-layouts and re-draws the _whole_
tree into a `PrintBuffer`, then flushes only the diff against a frozen copy of the previous frame
(`cursive-core/src/buffer.rs:142,450`; `cursive_run.rs:182-199`, complete with "Do we need to redraw
every view every time?" TODOs). `needs_relayout()` exists as an opt-in optimization that defaults to
`true` (`view_trait.rs:62-64`). Z-order comes from `StackView` layers with optional shadows
(`views/stack_view.rs`), which makes modal dialogs trivial. Layout is two-phase parent-driven
negotiation — `required_size(constraint)` up, `layout(size)` down — no cassowary, no flexbox; sizing
policy lives in wrapper views (`ResizedView` via `.fixed_width(...)`).

**Theming.** A flat semantic `Palette` (`Background`, `View`, `Primary`, `Highlight`, ... plus
custom namespaces) mapping to colors and styles (`cursive-core/src/style/palette.rs:39-43,344,389`),
loadable from TOML (`cursive-core/src/theme.rs` module docs). Per-subtree overrides via
`ThemedView`. There is no cascade and no selector system; an inline markup format
(`/red+bold{text}`) is being designed in discussion #797.

**Extras worth noting.** A YAML/JSON "blueprint" builder system constructs view trees from config,
including callback resolution via `$variables` (`cursive-core/src/builder.rs` module docs) — bolted
on late, feature-gated. Testing ships as the `puppet` backend: a fake backend exposing an
`ObservedScreen` stream and an input-event channel, used for scripted assertion tests
(`cursive/src/backends/puppet/mod.rs`, `cursive/examples/select_test.rs`).

## What it gets right

- **Deferred callbacks are a principled fix, not a hack.** `EventResult::Consumed(Option<Callback>)`
  resolves the `&mut self` vs `&mut App` conflict with zero unsafe and no RefCell panics _during
  dispatch_. Ten years on, this is still the cleanest known shape for "widget event mutates the app"
  in a single-ownership tree.
- **`cb_sink` is a great concurrency story for 90% of apps.** Any thread sends a closure; the loop
  runs it. The `progress.rs`, `logs.rs`, and `tcp_server.rs` examples do background work with no
  framework ceremony.
- **Decorator composition.** `ViewWrapper` (`cursive-core/src/view/view_wrapper.rs`) makes
  `NamedView`, `ScrollView`, `OnEventView`, `ShadowView`, `ThemedView`, `ResizedView` orthogonal
  wrappers, surfaced as fluent traits: `.with_name("x").fixed_width(10).scrollable()`. Cross-cutting
  concerns (identity, scrolling, keybinds, theme) never bloat the base widget.
- **Layers as root structure.** `StackView` + `ScreensView` give modal dialogs, popups, and
  multi-screen apps for free — for years the single biggest ergonomic edge over tui-rs.
- **Child-first event bubbling with focus.** Events go to the focused child; on `Ignored` the parent
  tries Tab/arrow focus moves (`linear_layout.rs:630-665`). Focus traversal is directional
  (`Direction`), so Tab and arrow-keys compose sensibly.
- **A real headless testing backend** (puppet/ObservedScreen) existed years before most TUI
  frameworks had any testing story.
- **Buffer-diff flush.** After issue #667 (below), the `PrintBuffer` diff brought bandwidth down
  without needing per-widget damage tracking — a pragmatic middle ground.

## What users complain about

- **Async never arrived.** Issue #92 (2016) asked for a Tokio-based loop; it was closed without one,
  and cursive still runs a 30ms poll-sleep loop (`cursive_run.rs:176`). Async apps must funnel
  everything through `cb_sink`, and…
- **…the `Send` bounds bite.** Issue #383: "This restriction is only needed when the user … wants to
  use the `CbSink` in another thread. Cursive itself … is completely single-threaded." The docs even
  recommend the `send_wrapper` crate as an escape hatch (`cursive_root.rs:66-74`). Then cursive-core
  0.4.0 went the other way and forced `Send + Sync` on _all_ views, breaking every `Rc<RefCell>`
  custom view (`CHANGELOG.md`).
- **Rendering inefficiency.** Issue #667: with crossterm/termion the screen visibly blinked and a
  frame cost 174 KB vs ncurses' 11 KB over SSH — the retained tree gave no partial redraw; the
  buffer-diff flush had to be built separately.
- **Styling is per-widget charity.** Issue #362: `TextArea` couldn't be colored because, unlike
  `EditView`, nobody had hand-implemented style support for it. Issue #674 is the same failure in
  `TextView`: ANSI color codes come out as escaped literals, and per-section styling means giving up
  wrapping. Styling capability is whatever each widget author bothered to expose; the flat palette +
  `ThemedView` can't express "style all buttons in dialogs" — hence the markup-format rethink in
  discussion #797.
- **Stringly-typed identity.** `call_on_name` returns `None` on either a wrong name or a wrong type
  annotation, with no way to tell which; `ViewRef` double-borrow panics at runtime. Community code
  is littered with `.unwrap()` on these.
- **The ecosystem voted with its feet.** When tui-rs was dying, fdehau conceded retained mode's
  advantages — "It is hard to build complex UI abstractions on top of `tui`: scroll, mouse support
  and advanced layouts are … challenging" — and explicitly suggested "port the 'look and feel' of
  `tui` to a crate like Cursive and officially deprecate `tui`" (issue #654). The community instead
  built ratatui. Immediate mode's "we avoid a lot of lifetime and ownership issues common to
  retained mode UIs in Rust" (same thread) beat Cursive's workaround stack in developer mindshare.

## What's worth stealing

- **The deferred-callback queue** — event handlers return commands/closures to run against
  `&mut App` after dispatch, never during. This generalizes cleanly into an Elm-ish command channel
  or Xilem-ish message routing.
- **A closure mailbox (`cb_sink`) as the universal external-world bridge** — the simplest sound
  model for timers, subprocesses, and network tasks mutating the UI.
- **Decorator wrappers + fluent extension traits** (`with_name`, `fixed_width`, `scrollable`,
  `wrap_*` hooks in `ViewWrapper`) as the third-party widget surface.
- **StackView layers/screens as first-class root structure** — modality should not be an app-level
  hack.
- **Puppet backend + ObservedScreen** — headless driver, scripted input channel, screen snapshot
  assertions.
- **`needs_relayout` + `SizeCache`** (`view/size_cache.rs`) — cheap relayout-avoidance signals,
  though they should be automatic, not a manual trait method everyone leaves defaulted.
- **Blueprints** — declarative config→tree with callback resolution is a good idea executed too
  late; design the widget trait so it's derivable from day one.

## Implications for rabbitui

- **Retained-tree is viable, but only with mediated mutation — so mediate from day one.** Never hand
  user code `&mut Widget` outside a framework-controlled scope. Adopt Cursive's "handlers return
  commands to run against `&mut App`" as the _only_ mutation path during dispatch; it's the one part
  of Cursive with no known failure mode.
- **Replace string selectors with typed keys, because that's where Cursive hurts most.**
  `call_on_name`'s tree-walk + triple-downcast (`finder.rs:57-78`) is O(tree), stringly-typed, and
  fails silently. Use arena/slotmap IDs (optionally `WidgetId<T>` carrying the type) so identity is
  O(1), checked, and survives frames — this is the Masonry/Xilem lesson applied to Cursive's
  problem.
- **Do not scatter `Arc<Mutex>` through the tree.** Cursive's per-`NamedView` lock yields documented
  panics (`named_view.rs:53-56`) and silent try_lock no-ops (`named_view.rs:83-88`). Own widget
  state in one arena owned by the app; lend `&mut` slices during dispatch. Locks in a
  single-threaded UI tree are pure workaround debt.
- **Keep the tree `!Send`; make the mailbox `Send`.** Cursive tried both polarities and both drew
  blood (issue #383, then the 0.4.0 `Send + Sync` breakage). With qwertty's async substrate, the
  loop can be a single-threaded task; only the message/closure channel needs `Send`. This dodges the
  entire fight.
- **Async-first event loop, because Cursive's poll-sleep is its most dated part.** Replace the 30ms
  poll (`cursive_run.rs:7`) with `select!` over qwertty input, timers, and the mailbox. Keep
  `set_fps`-style coalesced redraw as a throttle, not a heartbeat.
- **Keep child-first bubbling with `Ignored`/`Consumed` and directional focus**
  (`linear_layout.rs:630-665`, `Direction`); it composes and users understand it. Add capture-phase
  hooks via an `OnEventView`-style wrapper rather than complicating the core trait.
- **Double-buffer diff is enough; damage regions are not free with a retained tree.** Cursive proves
  retention doesn't grant partial redraw (issue #667 existed _because_ people assumed it did). Ship
  buffer-diff first (qwertty-side), make dirty-flags automatic on `&mut` access, and treat damage
  regions as a later optimization. Make layers/z-order first-class in the renderer like `StackView`.
- **Two-phase `required_size`/`layout` negotiation beats a constraint solver for TUI**; it's what
  both Cursive and Flutter-style systems converge on. Taffy for flex where needed, but don't make
  cassowary the core — Cursive never missed it.
- **Framework-resolved styling, not per-widget charity.** Issues #362/#674 show that "each widget
  hand-implements its colors" leaves permanent gaps. Widgets should emit semantic style _roles_; a
  resolver (palette + optional selector cascade, Textual-style) maps roles to concrete styles.
  Cursive's semantic `Palette` (`palette.rs:344,389`) is the right floor, its per-widget plumbing is
  the wrong ceiling.
- **Ship the puppet equivalent in the first release:** headless backend, scripted event injection,
  `ObservedScreen`-style snapshot assertions. Cursive's `select_test.rs` pattern is directly
  copyable onto qwertty's buffer.
