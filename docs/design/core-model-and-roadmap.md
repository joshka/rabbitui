# Core model + capability roadmap (2026-07-08)

A forward-looking synthesis: what belongs in rabbitui's _fundamental library_, why the
framework is worth having over raw ratatui, and the totality of the work still owed —
organized by capability and by the app archetypes each unblocks, not by what is already
done. Grounded in the research memos (`docs/research/`), the field reports, the accepted
ADRs, and a full inventory of the current public surface.

## 0. The thesis — why a framework over ratatui is the right bet

The field report's own verdict (`docs/field-report.md`, Parts V–VI) is the load-bearing
frame for everything below:

> Architecture novelty is free now. Correct interaction behavior, a real widget catalog, a
> headless test harness an AI author can run, and one serious reference app treated as an
> acceptance test — those are the scarce goods.

ratatui **deliberately refuses to be a framework**. Its maintainer agreed with "every single
point" of discussion #552's five structural gaps — no content-aware layout, unsigned coords,
no compositing, no post-render geometry, no event handling — and all five are downstream of
the one thing immediate mode structurally cannot give: **stable widget identity across
frames**. The ecosystem sprawl (rat-focus, tui-textarea, tui-realm, crokey, the template
zoo) is "a negative-space drawing of the framework Ratatui deliberately does not try to be."

rabbitui fills exactly that negative space, and — per the survival evidence — does it by
_layering on_ ratatui's 36M-download gravity (the `rabbitui-ratatui` bridge), never by
demanding wholesale replacement. The frameworks that replace ratatui forfeit the widget
ecosystem and stall (`rooibos`: a complete signals+taffy+async synthesis, five stars —
"architecture without a catalog, docs, or flagship is worth nothing").

So the differentiation is **not** the programming model. It is: the boring correct middle,
a real catalog, a runnable test harness, and being first to a credible accessibility export.

## 0.5 The size question (it will be the first question at announcement)

Evidence: r/rust, 2026-07-10 — "Looking for a simple TUI framework (smaller than
Ratatui?)". rabbitui was publicly name-dropped in that thread ("not smaller but it is
cuter"), so this exact question now has our name attached. The thread decomposes into
three different demands wearing one word, and each gets a different answer:

1. **Cognitive size (the OP's actual complaint).** The asker wanted tile layout, a few
   keybinds, and a modal with a text input — "am I using 10% of ratatui?" That is not a
   bytes problem; it is _ceremony_: they want the missing middle with less setup, which is
   this document's whole thesis. The thread recommends Cursive because it is
   batteries-included — validating that "smaller" often means "higher-level."
   **Answer: an `examples/simple.rs` that implements literally the OP's app** (grid
   tiles, a couple of chords, a modal with a `TextInput`) in as few lines as the trait
   model allows — an acceptance artifact and the announcement's opening code block.
2. **Actual weight (dependencies, binary, compile time).** Measured 2026-07-11:
   `rabbitui-core` = **3 crates** (the sans-io core is genuinely tiny);
   `rabbitui-widgets` = 4; the facade = **51 unique crates**, almost entirely the tokio
   tax (mio, signal-hook, macros, futures-core); `hello` release binary = 2.5 MB
   (~2.0 MB stripped) — comfortably inside the thread's own hosted-app calibration
   ("full app with sqlite + audio < 5 MB") and irrelevant to flash budgets. **Answer:
   publish these numbers honestly; budget them in CI** (dep-count + stripped-size
   assertions on `hello`, alongside the existing perf budgets) so they never regress
   silently; keep default features lean (`tracing` debug-profile, `themes`/`devtools`/
   `harness` opt-in). The §1.5 `run_blocking` tertiary is also the dep-weight answer:
   a sync driver would cut the facade's tree roughly in half for the simple-tool
   archetype — demand-gated, but now with a second constituency.
3. **Embedded / no_std (<64 KB RAM, <1 MB flash: ESP8266, STM32, RP Pico serial TUIs).**
   A real constituency in the thread (serial debug panels, snake-over-BLE), already
   served by tuit (no_std no_alloc), Mousefood, and ratatui-core's no_std support.
   **Answer: stays an explicit non-goal** — rabbitui targets hosted terminals; say so in
   the README so the embedded reader bounces fast to the right tool instead of slowly to
   disappointment.

Allocation-consciousness (the thread's `ArrayString`/`set_stringn`/`SmallVec` list) is
already partially encoded (Cow-returning sources, lazy `ListSource`, the planned
`CompactString` cell work in Arc 4); treat that list as review heuristics for hot paths,
not a rewrite mandate.

## 1. The core model — a trait `App`, not two closures (high priority)

### The problem with the current shape

Today the entry is `App::new(state, update, view)` — two closures plus a state value, with
config as builder methods (`theme`/`mode`/`mouse`/`tracing`/`log_handle`). It works, but it
**does not grow**, and this repo has already paid for that three times:

- **init hook** (dogfood finding #1) → shipped as an `Event::Started` enum variant + loop
  plumbing, because there is nowhere to put an `init` closure without changing `new`'s arity.
- **global chords** (dogfood finding #7) → resolved "by pattern" and an `App::on_global`
  hook was _explicitly deferred_ because a boxed always-runs closure was ugly.
- **suspend/resume** (Arc 4; qwertty 0.1.x shipped suspend — Wave D wires it) → wants
  `on_suspend`/`on_resume`; same shape, no home.

Each is a one-line default method on a trait. The closure API cannot take them gracefully.

### The shape (final — resolved in Fable review, 2026-07-11)

```rust
pub trait App<M = ()>: Sized
where
    M: Send + 'static,
{
    // The two required methods — the declared-frame contract, unchanged.
    fn update(&mut self, update: Update<'_, M>) -> ControlFlow<()>;
    fn view(&self, frame: &mut Frame<'_>);

    // Lifecycle hooks — defaulted. This is the extensibility win.
    fn init(&mut self) -> Command<M> { Command::none() }                    // finding #1
    fn global(&mut self, _update: &Update<'_, M>) -> ControlFlow<()> {  // finding #7
        ControlFlow::Continue(())
    }

    // Startup config — ONE method returning a struct, not N methods.
    fn config(&self) -> Config { Config::default() }

    // Provided run entries (AFIT; MSRV 1.88 ≥ 1.75). Not dyn-compatible — fine.
    async fn run(self) -> Result<()> { /* Terminal::open() → run_on */ }
    async fn run_over_device<D: qwertty::TerminalDevice>(self, device: D) -> Result<()> { /* … */ }
}
```

`Self` _is_ the state, so `&mut S` stops being threaded through two independent closures.
And the read/mutate split becomes **compiler-enforced** (`fn view(&self)` vs
`fn update(&mut self)`) instead of a convention two separate closures only happen to honor —
so the trait is _more_ faithful to the declared-frame goal, not less.

### Resolved design decisions (the holes the first sketch left open)

1. **Name collision.** The trait takes the `App` name. The existing `struct App<S,U,V,M>`
   becomes `FnApp<S,U,V,M>` — the closure adapter returned by `rabbitui::from_fn` (the std
   `iter::FromFn` naming). Its config builders become `with_theme`/`with_mode`/… so they
   cannot shadow trait methods. Free `run(state, update, view)` stays as
   `from_fn(…).run()`.
2. **Where `M` lives: generic param, not associated type.** `type Message = ();` needs
   associated-type defaults (unstable); a defaulted generic param (`App<M = ()>`) gives the
   no-effects case zero ceremony today. A type _could_ implement both `App<A>` and `App<B>`;
   harmless, documented.
3. **Config: one `fn config(&self) -> Config`, not six methods.** A `#[non_exhaustive]`
   `Config { theme, theme_file, mode, mouse, tracing, log_handle }` with builder methods
   grows without touching the trait, keeps the trait surface tight, and gives `FnApp` one
   field to store. Runtime switching (`update.set_mode`/`set_theme`) is unchanged — `Config`
   is startup-only.
4. **`init` vs `Event::Started`: both, deliberately.** `Event::Started` stays as the loop
   truth — `from_fn` apps cannot override trait hooks, so the event is their only init path.
   The loop calls `self.init()` once, spawns the returned `Command`, _then_ delivers `Started`
   through `global`/`update`. Trait apps use `init()`; closure apps match on `Started`; no
   conflict. Requires `Command::none()` (new; a `Kind::None` the spawn path skips).
5. **`global` semantics pinned** (the questions finding #7 deferred): runs before `update`
   for _every_ event (Started/Input/Resize/Message/EffectFailed), receiving `&Update` —
   all `Update` methods take `&self`, so `global` can spawn/commit/focus. Routing has
   already run, so `update.consumed()` and `update.action(&KEYMAP)` (with the printable-chord
   guard) work. `Break` exits the loop without calling `update`; pending effects still
   drain.
6. **Cut from v1: `on_error`, `on_suspend`/`on_resume`.** `Event::EffectFailed` already
   serves errors; suspend wiring is Wave D (qwertty 0.1.x shipped it). Defaulted methods
   are non-breaking to add later — dead hooks now buy nothing (YAGNI).

### Keep the one-liner (the std pattern)

The only thing the closure form genuinely buys is zero-ceremony inline apps (tests, demos,
10-line tools). Preserve it with a thin adapter, exactly as std does (`Iterator` +
`iter::from_fn`, `Future` + `poll_fn`, tower's `Service` + `service_fn`):

```rust
impl App for Counter { /* … */ }        // real apps + teaching
Counter::default().run().await?;

let app = rabbitui::from_fn(state, update, view);   // tests / demos keep the one-liner
```

`from_fn` returns an `impl App` whose `update`/`view` delegate to the closures — so the
closure form is a strict _subset_ expressible as a trait impl. Nothing is lost.

### Cost & boundary

- **Nothing is published (pre-0.1)** → no deprecation dance; `App::new` becomes `from_fn`.
- Migration is mechanical: 10 examples + the flagship + `tests/e2e_headless.rs` flip from
  `App::new(…)` to `impl App`. A day, doable non-breaking (add trait, keep `from_fn`).
- This touches only the **top-level packaging**. ADR 0001 §6 reserved exactly this: the
  declared-frame _contract_ (identity, facts, outcomes, Command-effects) is untouched; the
  Elm-style and Xilem-style shells were always meant to sit _above_ core. Low architectural
  risk; high teaching + extensibility payoff. **Recommend doing this first, as an ADR 0001
  amendment, before more catalog work accretes against the closure signature.**
- Full step-by-step implementation spec (for any-model execution):
  `docs/plans/wave-a-trait-app.md`. Wave B specs: `wave-b1-flagship-e2e.md`,
  `wave-b2-virtualization.md`. Wave C: `wave-c-forms-catalog.md`.

## 1.5 The async boundary (adjudicated 2026-07-11)

The question the trait forces: where does async live? Three candidate shapes — pervasive
async-first (`async fn update`), synchronous app logic on an async core, or no async at
all. The evidence picks the middle one decisively, and names two escape hatches.

### The evidence

- **Textual ran the pervasive-async experiment.** Its `async def` event handlers run on
  the one event loop, so an await in a handler _freezes the app_ — which is why Textual
  had to add `@work` workers as the escape hatch. An async `update` recreates exactly
  that footgun as the default path: updates are serialized (that is what kills data races
  by construction), so awaiting inside one stalls input, repaints, everything.
- **`view` is per-frame.** It re-runs at up to 60fps against the frame budget; an async
  view means unbounded frame latency. Every surveyed framework keeps views pure/sync.
  Loading UI is _state_ ("loading…"), rendered synchronously.
- **qwertty models the answer one level down** (`qwertty::docs::async_model`): async-first
  at the surface, but the parts that interpret bytes are a **sans-io pure core** — no I/O,
  no clock, no threads — with two drivers over the _identical_ core: `TokioTerminalSession`
  (Tokio readiness) and `TerminalSession` (blocking poll loop, no runtime at all). "Only
  who feeds bytes and time differs."

rabbitui already has the same layering one level up, and should keep it deliberately:

- `rabbitui-core` (update fold, layout, paint, facts, routing) is the sans-io analogue:
  pure state machines, no I/O, no clock, `!Send` (ADR 0005).
- The facade's `run_loop` is the driver (Tokio readiness today); the effects mailbox is
  the **only** `Send` surface. Async enters at exactly three edges: input (substrate),
  effects (`Command`), and the paint timer.
- App hooks (`update`/`view`/`init`/`global`) are synchronous **by design**, not by
  omission: they run inside the serialized fold where an await is a frozen UI.

### What each archetype actually needs (why sync hooks don't hurt)

Every archetype's async need is the same shape — _start work, keep the UI alive, fold the
result back in_ — which is `Command`, not `await`:

- Agent CLI: token stream in → `Command::stream` (proven in the flagship).
- Log follower: tail a file → `Command::stream` (proven in the comparison app).
- Dashboard / monitor: interval fetch → self-reissuing `Command`/stream.
- Picker: filesystem scan → `Command::future` + incremental `Message`s.
- Forms: async validation → `Command::future` per field edit (grouped, cancel-previous).
- Editor-ish load/save: `Command::future(async { fs work })`; note `tokio::fs` is
  `spawn_blocking` underneath — plain `std::fs` inside `spawn_blocking` is the honest
  pattern for heavy filesystem work.

None of these needs to await _inside_ `update`; all of them break if `update` awaits.

### The real pain, and the v1 answer

The genuine ceremony cost of commands is the `Message`-enum round trip: one variant + one
match arm per one-shot async result. The **closure-message idiom** collapses that to a
single variant, today, with zero framework machinery:

```rust
enum Message {
    Apply(Box<dyn FnOnce(&mut MyApp) + Send>),   // the one variant
    // …plus real variants where matching matters (streams, cancellation)
}
// one-shot load, no per-operation variant:
update.spawn(Command::future(async move {
    let text = tokio::task::spawn_blocking(move || std::fs::read_to_string(path)).await;
    Message::Apply(Box::new(move |app| app.file = text.ok()))
}));
```

Document this idiom in the `effect` module and one example. **Future shape (not v1):** a
first-class `Outbox::Apply(Box<dyn FnOnce(&mut dyn Any) + Send>)` variant — post-Wave-A
the loop knows the concrete `A: App` and can downcast — giving `spawn_then(future, |app,
out| …)` without the enum entirely. Add it only if the idiom proves insufficient (YAGNI).

### The "no async" tertiary shape

A `run_blocking()` driver over qwertty's **synchronous** `TerminalSession` is genuinely
buildable — the app core is already sync, and qwertty proved the dual-driver shape works
(same core, blocking poll loop, no runtime). Honest caveat: `Command` is future-based, so
a sync driver either restricts apps to no-effects or embeds a current-thread runtime for
the mailbox. Recorded as a roadmap option for the embedded/simple-tool archetype, gated on
demand; the async-core default stands.

### Trait hooks × archetypes (the forward frame)

- `init` → self-starting archetypes (agent, log follower, dashboard) spawn their opening
  stream with no first-keypress hack.
- `global` → every archetype's always-on chords (quit/help/suspend).
- `config` → mode per archetype: inline for agent CLIs, alt-screen for browse apps;
  grows fields (e.g. scroll behavior, paste mode) without trait churn.
- `update` + `Command` → all async work, serialized fold, race-free by construction.
- `view` → sync forever; per-frame.
- `run_over_device` → the testing archetype (harness, CI).
- **Now unblocked by qwertty 0.1.x** (shipped: suspend/resume around `SIGTSTP`/`SIGCONT`,
  `run_detached` $EDITOR handoff, `ResizeStream`, lone-Esc flush control, crates.io
  publish): `on_suspend`/`on_resume` hooks + `Event::Resize` becoming push-based — specced
  in `docs/plans/wave-d-qwertty-adoption.md`, added to the trait as defaulted methods
  (non-breaking) when Wave D lands.

## 2. The fundamental library — capability tiers and current status

What belongs in `rabbitui-core` (the stability anchor: contract, identity, facts, buffer,
style, layout — no runtime deps) vs the catalog (`rabbitui-widgets`) vs an optional shell.
Ranked by cross-framework consensus (from the checklist), annotated with what rabbitui has
today.

Each line: capability _(consensus, home)_ — current status.

- **Widget identity** _(unanimous core; core)_ — **Have**: `WidgetId`/id-paths, `StateStore`,
  facts.
- **Event loop / async runtime** _(unanimous core; facade)_ — **Have**: `run_loop`, tokio
  `select!`.
- **Async effects, commands-only** _(unanimous core; facade)_ — **Have**: `Command`
  future/stream/timeout/group, panic-catch.
- **Testing harness** _(unanimous core; testing)_ — **Have**: `TestApp` + `VtScreen` +
  FakeDevice e2e.
- **Overlays / layers / modals** _(unanimous core; core)_ — **Partial**: facts `layer` +
  `Frame::layer`; buffer compositing deferred (Clear+overpaint).
- **Focus system** _(strong core; core)_ — **Have**: `Focus`, Tab/BackTab, click-to-focus.
- **Layout, intrinsic (no solver)** _(strong core; core)_ — **Have**: `Constraint`,
  `split_*`, `desired_height`.
- **Mouse** _(strong core; core+facade)_ — **Have**; wheel/trackpad normalization
  **partial**.
- **Inline vs alt-screen, peer modes** _(strong core; facade)_ — **Have**: commit-scrollback
  with a bounded tail; owned-viewport deferred.
- **Theming, tokens+presets (no cascade)** _(strong core; core)_ — **Have**: roles, 4
  presets, TOML hot-reload (debug).
- **Scroll / virtualization** _(needed-but-everyone-failed; core contract)_ — **Partial**:
  `ScrollScope` + lazy `ListSource`; variable-height + columnar provider **unbuilt**.
- **Forms / input widgets** _(catalog; widgets)_ — **Partial**:
  `TextInput`/`Button`/`SelectionList`; no Form builder/validation.
- **Live reload** _(Textual-only, high value; facade)_ — **Partial**: theme hot-reload only.
- **Accessibility export** _(the open forcing-function; core+shell)_ — **Partial**:
  roles/labels recorded into facts, **not consumed**; no export.
- **Devtools** _(Textual-strong; testing)_ — **Have-ish**: `FactsInspector`, `facts::dump`.

Missing **hooks**: `global` lands with the §1 trait; `on_suspend`/`on_resume` follow as
defaulted methods in Wave D — qwertty 0.1.x shipped suspend/resume (non-breaking);
`on_error` is cut — `Event::EffectFailed` already serves it. `init` = trait hook +
`Event::Started` (§1 №4).

The boundary is already crisp in the ADRs and should stay that way:

- **core** = anything a third-party widget or coding agent must implement (fixes ratatui's
  `WidgetRef`-outside-core mistake). No tokio.
- **app-land** = state + effects. "The framework never owns, wraps, lenses, or adapts app
  state"; "no widget ever holds `&mut App`" — widgets return typed outcomes.
- **optional shell** = an Elm-style `rabbitui-tea` or a Xilem-style view-diff/memo layer,
  above core, never the widget contract.

## 3. The differentiation bets — why better than the _other_ frameworks too

Three places where the whole field failed or nobody has shipped. These are the moat, and
each is grounded in a real archetype demand.

1. **Virtualization done right** (capability #11). Textual's DataTable is ~800× slower than
   the community `fastdatatable`; Toolong _bypasses_ Textual's scrollables entirely for large
   files; Brick virtualizes only at uniform item height. The demand: a **pluggable lazy
   row/line/columnar provider** as a day-one core contract property, variable-height via
   estimate + measured cache. Unblocks the log/stream follower, the dashboard/table, and the
   agent transcript (the three heaviest archetypes). This is the single highest-value unbuilt
   piece and the clearest differentiation.
2. **PTY-level interaction correctness** (capability #4, extended). The FakeDevice harness is
   the foundation; the next step is a **conformance matrix** (the field report: "whoever
   publishes the harness sets the bar"). Widgets are cheap now; correct focus / mouse /
   resize / CJK / paste behavior under a real terminal is the scarce good — and doubles as
   the _trust mechanism for AI-authored apps_. FrankenTUI is the cautionary tale: breadth
   with no interaction correctness "looks like it's working… everything is broken subtly."
3. **Accessibility export** (capability #14). In a fifty-framework wave, nobody shipped AT
   semantics. rabbitui is uniquely positioned: it already records roles/labels into frame
   facts, so it has the substrate for an AccessKit-style export **without** a retained object
   tree. Whoever ships this first gets a genuine differentiator and may _settle the
   programming-model fork_ (if a11y demands a persistent identified tree, retained-core wins;
   rabbitui's facts model is the middle path that could pre-empt that).

## 4. The catalog is the product

Per every survivor: the catalog _is_ the product (Textual's 35 widgets + devtools won an
ecosystem; rooibos's zero-catalog synthesis got five stars). Current catalog is thin
(`Text`, `Button`, `TextInput`, `SelectionList`, `Collapsible`, `Panel`, `ErrorBanner`,
`HelpOverlay`, plus feature-gated `LogOverlay`/`FactsInspector`). The gaps, by demand rank:

- **Forms** — the sharpest catalog sub-gap. Target shape: a `huh`-style Form/Group/Field
  builder + a `#[derive(Form)]`, with field validation and (eventually) the a11y
  degrade-to-prompts fallback "no Rust TUI has." Serves the forms/wizards/CRUD archetype.
- **Table / DataTable** — virtualized over the #11 provider, not eager. Serves dashboards.
- **Agent-chrome widgets** — extract the flagship's markdown transcript, tool/diff cells,
  and composer into reusable catalog widgets. This is _the_ archetype of the era; the
  flagship already proved the shapes.
- **Text as infrastructure** — grapheme/width/wrap/cursor on one shared layer, never
  per-widget (the "one width oracle" rule). Underpins TextInput, the transcript, and IME.

## 5. Roadmap by archetype — what unblocks whom

Priority tags in brackets.

- **Agent CLI / agent chrome** _[highest]_ — virtualized transcript, soft-wrap copy,
  block-level early commit, catalog extraction.
- **Log / stream follower** _[highest]_ — virtualization (#11) with a lazy provider.
- **Data dashboard / table** _[high]_ — virtualized DataTable + columnar provider.
- **Forms / wizards / CRUD** _[high]_ — Form builder + `derive(Form)` + validation.
- **Pickers / palettes** _[medium]_ — query-filter list ergonomics (ids already durable).
- **REPLs / monitors** _[medium]_ — mostly served; a scrollback-history helper.
- **Desktop-metaphor** _[later]_ — z-order buffer compositing, menu/dialog widgets.
- **Editors, games, no_std** _[none]_ — explicit non-goals; watch WASM (Ratzilla) only.

## 6. The sequenced totality (the actual work list)

Waves, each shippable, ordered by leverage. Not "what's done" — what's next.

**Wave A — get the core model right (do first).**
Trait `App` + `from_fn` adapter + the defaulted hooks (init/global/suspend/on_error);
ADR 0001 amendment. Flip examples + flagship + e2e harness. Unblocks clean growth of
everything after and removes the `Event::Started`/pattern-#7 workarounds.

**Wave B — the differentiation core (highest external value).**
(1) Virtualization contract: lazy provider trait + variable-height measured cache; retrofit
`SelectionList`, add `Table`. (2) Flagship e2e tests over the FakeDevice harness (help
overlay, inline tool-cell settle, mode toggle) → then grow a PTY conformance matrix.

**Wave C — the catalog (the product).**
Form builder + `derive(Form)` + validation; virtualized `Table`; extract agent-chrome
widgets (transcript/tool-cell/composer) from the flagship into `rabbitui-widgets`.

**Wave D — substrate adoption (qwertty).**
Fully specced in `docs/plans/wave-d-qwertty-adoption.md` now that qwertty shipped 0.1.x:
version dep + CI, suspend/resume + $EDITOR hooks, push-based resize, lone-Esc timing,
KeyEvent pre-pin migration, `/dev/tty` backstop removal, width negotiation (mode 2027);
begin IME/preedit (the named v0.1 gap — only the facts anchor is reserved today).

**Wave E — accessibility export (the forcing-function).**
Consume the roles/labels already in facts → an AccessKit-style export behind a feature. This
is the strategic bet; start exploratory once B/C stabilize the facts shape.

**Wave F — polish to 0.1.**
Buffer-level layer compositing (retire Clear+overpaint), styled-span soft-wrap, block-level
early commit for bounded tails, cargo-semver-checks + release automation, the naming
decision (incl. a crates.io availability check — the r/rust thread surfaced an abandoned
`maxmindlin/rabbitui` repo, and the name is now publicly teased), concept docs once the
API stops moving. Plus the §0.5 size story:
`examples/simple.rs` (the r/rust asker's app — grid, chords, modal input — in minimal
lines), CI budgets for dep count and stripped `hello` size (baseline 2026-07-11: facade 51
crates, 2.0 MB stripped), and README positioning for the three size demands (ceremony,
weight, embedded non-goal).

### Known deferred (tracked, not lost)

Owned-viewport inline mode (opt-in, tui2 shapes), buffer-level compositing, per-terminal
wheel normalization, hardware-cursor via facts, `WidthPolicy` seam (waits on qwertty),
kitty-shaped KeyEvent adaptation (pre-pin blocker), macOS `/dev/tty` upstreaming.

---

_Sources: `docs/research/` memos (ratatui #552, codex-tui2, textual, bubbletea, ink, brick,
cursive, libvaxis, prior-art, recent-rust-tui-wave), `docs/field-report.md` Parts V–VI,
ADRs 0001–0014, and a full inventory of the current public surface (2026-07-08)._
