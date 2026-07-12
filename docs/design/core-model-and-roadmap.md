# rabbitui — what it is, the core model, and the roadmap

Living doc (2026-07-08, restructured 2026-07-11 into explaining order: overview first,
sections fill in). Grounded in the research memos, both field reports, ADRs 0001–0015,
and a full inventory of the current public surface. Execution detail lives in
`docs/plans/wave-*.md`; this doc is the map.

## What rabbitui is

**rabbitui is ratatui's big framework brother.** ratatui deliberately is not a framework —
no event loop, no focus, no identity, no widget state, and its maintainer agreed with all
five structural gaps in discussion #552. rabbitui, from one of ratatui's own maintainers,
is the framework layer that refusal leaves open: the blessed loop, identity, focus,
outcomes, effects, theming, and a real catalog — while staying interoperable with the
ratatui ecosystem rather than replacing it. It runs on the qwertty terminal substrate but
is deliberately substrate-agnostic at the core (ADR 0012/0014).

**Naming (open; author's call).** Candidates: `ratatui-framework`, `qwertty-tui`, or
`rabbitui` (likely). Considerations on file: the brand-independence decision (ADR 0014
amendment — the core is substrate-free, which cuts against `qwertty-tui`); the name
`rabbitui` is now semi-public (teased on r/rust, 2026-07-10) and needs a crates.io
availability check (an abandoned `maxmindlin/rabbitui` repo exists); `ratatui-framework`
states the sibling relationship plainly and is plausible from a maintainer, at the cost
of implying an org-official blessing. Not decided here — recorded so the decision has
its inputs in one place.

### The pillars (what the framework must be)

Priority-ordered by the author (2026-07-11); each expands in its numbered section:

1. **Solid structure for apps** — a real place for state, lifecycle, and growth
   (`trait App`) — §1.
2. **An easy developer model** — events, outcomes, actions, and commands as _one_
   coherent vocabulary, not four — §2.
3. **Solid UX basics** — selection, focus, layout, keybinds, themes, scrolling: correct
   by default — §3.
4. **ratatui compatible** — the ecosystem's gravity is a feature; interop, don't
   replace — §4.
5. **Async first, sync possible** — async at the edges, synchronous app logic, a
   no-runtime driver as the simplicity path — §5.
6. **Testable by default** — the headless + PTY-level harness as a product feature
   (also the trust mechanism for AI-authored apps) — §6.
7. **Looks good by default** — the author's standing rule: examples and apps must look
   good, and achieving that must be easy — §7.
8. **Terminal-native** — inline and alt-screen as peer modes; cooperate with the
   terminal (scrollback, copy, search) rather than owning the viewport — §8.
9. **Dependably boring** — stability policy, honest versioning, and a flagship kept
   alive as the acceptance test; survive longer than one person's attention span — §9.

Pillars 6–9 are the additions this synthesis argues for: 6 because the field report names
the runnable harness a scarce good and every only-showed-on-hardware bug this project hit
proves it; 7 because it is already the repo's standing non-functional rule (ROADMAP Arc
2A); 8 because the era's sharpest demand (agent CLIs) and a whole ADR (0013) were in no
pillar; 9 because the field report's closing verdict — "the boring, correct,
well-documented middle" — is a promise adopters choose frameworks on, and the named
failures (ratatui's 74 breaking changes, Textual's 8 majors in 18 months) are exactly
what it protects against.

Considered and deliberately **folded, not added**: accessibility (a differentiation
_bet_, Wave E — not yet a property the framework has); docs/teachability (inside §2 —
the developer model _is_ the teaching surface); AI-agent legibility (inside §6 — the
harness is the trust mechanism); panic-safety/never-wreck-the-terminal (inside §1 —
already shipped: restore-on-panic, contained effect panics).

## 1. Solid structure for apps — `trait App`

### The problem with the current shape

Today the entry is `App::new(state, update, view)` — two closures plus a state value,
with config as builder methods. It works, but it **does not grow**, and this repo has
already paid for that three times:

- **init hook** (dogfood finding #1) → shipped as an `Event::Started` enum variant + loop
  plumbing, because there is nowhere to put an `init` closure without changing `new`'s
  arity.
- **global chords** (dogfood finding #7) → resolved "by pattern"; an `on_global` hook was
  _explicitly deferred_ because a boxed always-runs closure was ugly.
- **suspend/resume** (Arc 4; qwertty 0.1.x shipped suspend — Wave D wires it) → wants
  `on_suspend`/`on_resume`; same shape, no home.

Each is a one-line default method on a trait. The closure API cannot take them
gracefully. A trait is also the idiomatic Rust shape — `impl App for MyApp` teaches
better than passing two closures to a constructor — and it makes the read/mutate split
compiler-enforced (`fn view(&self)` vs `fn update(&mut self)`) instead of a convention.

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
    fn init(&mut self) -> Command<M> { Command::none() }
    fn global(&mut self, _update: &Update<'_, M>) -> ControlFlow<()> {
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

### Resolved design decisions

1. **Name collision.** The trait takes the `App` name. The existing struct becomes
   `FnApp<S,U,V,M>` behind `rabbitui::from_fn` (std `iter::FromFn` naming); its builders
   become `with_*` so they cannot shadow trait methods.
2. **`M` is a defaulted generic param**, not an associated type (`type Message = ();`
   needs unstable associated-type defaults).
3. **Config is one method returning a `#[non_exhaustive]` struct** — grows without trait
   churn; startup-only (runtime switching stays on `Update`).
4. **`init` and `Event::Started` coexist deliberately** — `from_fn` apps cannot override
   hooks, so the event is their init path; the loop spawns `init()`'s `Command`, then
   delivers `Started`. Requires `Command::none()`.
5. **`global` semantics**: runs before `update` for every event, receives `&Update` (all
   `Update` methods take `&self`, so it can spawn/commit/focus); routing has already run
   so `consumed()`/`action()` work; `Break` skips `update`, pending still drains.
6. **Cut from v1**: `on_error` (`Event::EffectFailed` serves it), `on_suspend`/`on_resume`
   (Wave D — qwertty 0.1.x shipped the substrate side). Defaulted methods add
   non-breaking later.

### The one-liner survives

`from_fn(state, update, view)` keeps zero-ceremony inline apps (tests, demos, hello) —
the std pattern (`iter::from_fn`, `future::poll_fn`, tower's `service_fn`). The closure
form is a strict subset expressible as a trait impl; nothing is lost.

Cost: pre-0.1, nothing published — 15 call sites flip mechanically (~a day). Full
implementation spec: `docs/plans/wave-a-trait-app.md`. **Do this first**, before more
catalog accretes against the closure signature (ADR 0001 amendment included in the plan).

## 2. An easy developer model — one vocabulary, not four

The author's framing: "commands, actions, events, or some combination that is
meaningful." The combination is meaningful when each word owns exactly one direction of
flow, and the canonical `update` body reads as a sentence:

- **`Event`** — what the runtime delivers _to you_: `Started` once, `Input` (only if no
  widget consumed it), `Resize`, `Message(M)` (your effect results), `EffectFailed`.
- **`Outcome`** — what a widget says happened: `Activated`, `Changed(String)`,
  `Selected(usize)`, `Submitted`, `Toggled`, `Dismissed`. Read via
  `update.outcome_for(path)`. Widgets never call you back and never hold `&mut App`;
  outcomes are data.
- **`Action`** — what a chord _means in your app_: your enum, declared in a `Keymap`,
  resolved by `update.action(&KEYMAP)` with the printable-chord guard applied (a bare
  `d` is never stolen from a text input). The same table renders the help overlay.
- **`Command`** — what you want done _off the loop_: a future or stream whose results
  re-enter as `Event::Message`. The only async door (§5); `.group()` gives
  cancel-previous.

So the canonical update body is a fixed, teachable shape:

```rust
fn update(&mut self, update: Update<'_, Message>) -> ControlFlow<()> {
    match update.action(&KEYMAP) {              // chords → your Actions
        Some(Action::Quit) => return ControlFlow::Break(()),
        Some(Action::Refresh) => update.spawn(fetch()),   // consequences → Commands
        _ => {}
    }
    if update.outcome_for(&[key("list")]) == Some(&Outcome::Activated) {
        self.open_detail(update.widget_state::<SelectionList<_>>(&[key("list")]));
    }
    if let Event::Message(msg) = update.event() {         // effect results → state
        self.apply(msg);
    }
    ControlFlow::Continue(())
}
```

Meaning is extracted two ways (outcomes from widgets, actions from chords); consequences
leave two ways (mutate `self` synchronously, or spawn a `Command`). Everything else —
focus requests, widget commands, mode/theme switches, scrollback commits — rides on the
same `Update` context. Teaching order for docs and examples follows this section.

## 3. Solid UX basics — correct by default

The author's list — selection, focus, layout, keybinds, themes — plus the two the
research adds (scroll/virtualization, overlays). Status against the cross-framework
consensus (details and per-capability evidence in appendix A):

- **Focus** — have: id-keyed `Focus`, Tab/BackTab traversal from frame facts,
  click-to-focus; `Frame::is_focused` for focus-reactive chrome.
- **Selection** — have: `SelectionList` with durable state, lazy sources, built-in empty
  state; Wave B2 adds the virtualized `Table`.
- **Layout** — have: intrinsic-measurement constraints (`desired_height`), splits,
  center/inset; deliberately no solver (cassowary rejected; taffy = optional adapter).
- **Keybinds** — have: declarative `Keymap`/`Chord`/`Binding` + help overlay. The
  conventions layer (readline set as `TextInput` contract, CUA navigation, opt-in
  vim/fzf idioms, terminal-deliverability constraints) is specced in
  `docs/design/keybinding-conventions.md` — implementing its work items is the
  keybinding pillar's remaining half.
- **Themes** — have: semantic roles, four presets, TOML + debug hot-reload, runtime
  switching. No CSS cascade by design (tokens proved sufficient twice; Textual's dev
  loop, not its CSS, is the feature worth having).
- **Scroll/virtualization** — partial, and it is **the differentiation bet**: everyone
  failed it (Textual's 800× DataTable; Brick's uniform-height wall). Anchor-based
  variable-height scrolling + pluggable sources, specced in
  `docs/plans/wave-b2-virtualization.md`.
- **Overlays/modals** — have at the facts level (`Frame::layer`, input containment);
  buffer-level z-compositing deferred (Wave F).

The catalog is the product (rooibos shipped architecture without one: five stars). Gaps
by demand: forms (`FormScope`, Wave C1), virtualized `Table` (B2), agent-chrome widgets
extracted from the flagship (C3).

## 4. ratatui compatible — the family relationship

The sibling positioning makes compat a pillar, not a leaf-crate footnote. The survival
evidence is blunt: frameworks that layer on ratatui's 36M-download gravity get adopted;
frameworks that replace it forfeit the widget ecosystem and stall.

- **Today**: `rabbitui-ratatui` renders any ratatui `Widget`/`StatefulWidget` inside a
  rabbitui frame (`RatatuiWidget` adapter + imperative bridge). Honest limits: cells
  only — bridged widgets are inert rectangles (no identity, focus, outcomes; styles
  pre-resolved, not re-themed).
- **Direction**: treat the bridge as a migration ramp, not a museum piece. Worth
  exploring post-0.1: outcome/focus shims for common ratatui widgets, a reverse bridge
  (render a rabbitui widget into a ratatui buffer) so adoption can be incremental in
  either direction, and shared style conversion staying lossless. `rabbitui-core` never
  depends on ratatui (ADR 0010); the bridge stays the single meeting point.
- **Positioning rule**: never frame ratatui as the competitor — rabbitui is what a
  ratatui app graduates into when it needs the framework layer, and what hands its
  widgets back down via the bridge.

## 5. Async first, sync possible (adjudicated 2026-07-11)

Where async lives — three candidate shapes; the evidence picks one decisively:

- **Textual ran the pervasive-async experiment**: `async def` handlers on the one event
  loop freeze the app on any real await — their `@work` workers are the escape hatch.
  An `async fn update` recreates that footgun as the default path, because updates are
  serialized (which is what kills data races by construction).
- **`view` is per-frame** (60fps budget) — async views mean unbounded frame latency;
  every surveyed framework keeps views pure. Loading UI is _state_, rendered sync.
- **qwertty models the answer one level down** (`qwertty::docs::async_model`):
  async-first surface, sans-io pure core, **two drivers over the identical core** —
  Tokio readiness and a blocking poll loop. "Only who feeds bytes and time differs."

**Decision: synchronous app logic on an async core.** `update`/`view`/`init`/`global`
are sync by design; async enters at exactly three edges — input (substrate), effects
(`Command`), the paint timer. The effects mailbox is the only `Send` surface (ADR 0005).
Every archetype's async need (agent stream, log tail, dashboard poll, picker scan, form
validation, file load) is the same shape — _start work, keep UI alive, fold the result
back_ — which is `Command`, not `await`. Filesystem note: `tokio::fs` is
`spawn_blocking` underneath; `std::fs` inside `spawn_blocking` is the honest pattern.

**The conceded pain** is `Message`-enum ceremony per one-shot async op. The v1 answer is
the **closure-message idiom** — one variant total, zero framework machinery:

```rust
enum Message { Apply(Box<dyn FnOnce(&mut MyApp) + Send>) /* + real variants */ }
update.spawn(Command::future(async move {
    let text = tokio::task::spawn_blocking(move || std::fs::read_to_string(path)).await;
    Message::Apply(Box::new(move |app| app.file = text.ok()))
}));
```

Future shape (only if the idiom proves insufficient): a first-class
`Outbox::Apply(Box<dyn FnOnce(&mut dyn Any) + Send>)` — post-Wave-A the loop knows the
concrete `A: App` and can downcast — giving `spawn_then(future, |app, out| …)`.

**"Sync possible" — the simplicity path.** A `run_blocking()` driver over qwertty's
synchronous `TerminalSession` is genuinely buildable (the app core is already sync;
qwertty proved the dual-driver shape). Caveat: `Command` is future-based, so a sync
driver restricts effects or embeds a current-thread runtime. Demand-gated; also the
dependency-weight answer for simple tools (appendix B).

## 6. Testable by default

The field report's scarce good ("a headless test harness an AI author can run"), and this
project's own history proves the tiers: the help-overlay panic and the tool-cell freeze
passed every unit test and only showed on hardware. Three layers, all shipped:

- `TestApp` (headless driver over the same store/frame/route path) + buffer snapshots.
- `VtScreen` (vt100 escape-level assertions on emitted bytes).
- The FakeDevice pump harness driving the **real** `App::run` loop headlessly
  (`rabbitui/tests/e2e_headless.rs`; promotion to a facade feature specced in
  `docs/plans/wave-b1-flagship-e2e.md`, which also adds the flagship regression suite).

Direction: grow the harness into a published PTY conformance matrix ("whoever publishes
the harness sets the bar" — Arc 5), and keep every new framework capability landing with
a harness-level test, not just unit tests.

## 7. Looks good by default (the standing rule)

Author-mandated (ROADMAP Arc 2): examples and apps must look good, coherent, well laid
out, well themed — and achieving that must be easy. Structurally delivered by Arc 2A
(Panel, spacing tokens, four presets, gallery-as-regression, screenshot pipeline); the
rule remains an acceptance bar on every wave: a feature is not done if the example
showing it looks bad.

## 8. Terminal-native — inline and alt-screen as peer modes

The era's sharpest demand (agent CLIs) and the deepest lesson (codex tui2: owning the
viewport "priced in production and retired"). rabbitui's position, already decided and
shipped (ADR 0013, `docs/inline-mode-spec.md`):

- **Two peer modes**, runtime-switchable (`Update::set_mode`): inline (bounded live
  tail plus append-once scrollback commits — finalized content becomes the _terminal's_,
  so native scrollback, selection, copy, and Cmd-F all work and output survives exit) and
  alt-screen (the browse canvas).
- **Cooperate-with-the-terminal is the default**; the app-owned viewport (faithful copy
  across reflow, per-cell interaction) stays a documented, deliberately unbuilt opt-in
  with tui2's shapes on file — justified only when the workload genuinely demands it.
- Enforced invariants, not conventions: commits flush before alt-screen entry; committed
  lines are never repainted; the live tail is bounded. The inline-mode spec doubles as an
  Arc 5 field-leadership artifact (a discipline others can implement).

Remaining work rides existing waves: block-level early commit (Wave F), push-based
resize (Wave D), styled-span soft-wrap for commit fidelity (Wave F).

## 9. Dependably boring

The field report's closing verdict is a positioning statement: what is missing "is not
another architecture — it is someone willing to build the boring, correct,
well-documented, thoroughly-tested middle, and to keep a real application alive on top
of it for longer than one person's attention span." The named failures are versioning
failures as much as technical ones: ratatui's 74 breaking changes, Textual's eight
majors in eighteen months post-1.0, every dead framework dying with its author's app.

What this pillar means in practice — mostly already institutionalized:

- **The flagship is the acceptance test, permanently** (the survival law, inverted:
  the framework lives as long as a real app needs it — so keep one alive on purpose).
- **Versioning discipline**: `BREAKING-CHANGES.md` from day one, cargo-semver-checks +
  release automation (Wave F), the one-widget-contract-in-core rule so third-party
  widgets don't fragment on version skew (ratatui's `WidgetRef` lesson).
- **MSRV policy** stable-minus-one, true floor moving only when needed; clippy on
  stable + beta in CI so breakage is seen a release early.
- **Naming is forever** (ADR 0015) — the rename sweep happened pre-0.1 precisely because
  this pillar makes it impossible later.

## Roadmap — the waves

Sequenced, each shippable; full specs in `docs/plans/`. (Recently completed work lives in
ROADMAP.md's tracker; this list is forward-only.)

- **Wave A — trait `App`** (§1; `wave-a-trait-app.md`). Do first.
- **Wave B1 — flagship e2e over FakeDevice** (§6; `wave-b1-flagship-e2e.md`).
- **Wave B2 — anchor virtualization + `Table`** (§3's bet; `wave-b2-virtualization.md`).
- **Wave C — forms + catalog extraction** (§3; `wave-c-forms-catalog.md`).
- **Wave D — qwertty 0.1.x adoption** (`wave-d-qwertty-adoption.md`): version dep + CI,
  suspend/resume + $EDITOR hooks, push-based resize, lone-Esc timing, KeyEvent pre-pin
  migration, `/dev/tty` backstop removal, width negotiation; then IME/preedit
  exploration (the named v0.1 gap).
- **Wave E — accessibility export**: consume the roles/labels already recorded in frame
  facts into an AccessKit-style export behind a feature. The open forcing-function
  nobody in a fifty-framework wave shipped; rabbitui has the substrate without a
  retained tree. Start exploratory once B/C stabilize the facts shape.
- **Wave F — 0.1 polish**: buffer-level layer compositing, styled-span soft-wrap,
  block-level early commit, cargo-semver-checks + release automation, **the naming
  decision** (incl. crates.io availability check — see "What rabbitui is"), concept
  docs, `examples/simple.rs` + size budgets (appendix B).

By archetype (what unblocks whom): agent CLI + log follower want B2 most (virtualized
transcript/tail); dashboards want B2's `Table`; forms/wizards want C1; pickers and
REPLs are largely served; desktop-metaphor waits on Wave F compositing; editors, games,
and no_std stay explicit non-goals (embedded readers belong on tuit/Mousefood — say so
in the README).

### Known deferred (tracked, not lost)

Owned-viewport inline mode (opt-in, tui2 shapes), per-terminal wheel normalization,
hardware-cursor via facts, `WidthPolicy` seam, kitty-shaped KeyEvent adaptation, macOS
`/dev/tty` upstreaming, `run_blocking` sync driver (§5), reverse ratatui bridge (§4).

## Appendix A — why a framework over ratatui (the evidence)

The field report's verdict: "Architecture novelty is free now. Correct interaction
behavior, a real widget catalog, a headless test harness an AI author can run, and one
serious reference app treated as an acceptance test — those are the scarce goods."

ratatui's five structural gaps (#552, maintainer-endorsed) — no content-aware layout,
unsigned coords, no compositing, no post-render geometry, no event handling — are all
downstream of the one thing immediate mode cannot give: stable widget identity. The
ecosystem sprawl (rat-focus, tui-textarea, tui-realm, crokey) is a negative-space drawing
of the framework ratatui deliberately is not. The cautionary tales: rooibos (complete
architecture, no catalog, five stars), codex tui2 (owned viewport priced in production
and retired), FrankenTUI (breadth without interaction correctness), and the survival law
(every dead framework died with its author's app — hence the flagship-as-acceptance-test
discipline).

Cross-framework capability consensus, per-capability status, and the three
differentiation bets (virtualization done right, PTY-level correctness, a11y export) were
adjudicated 2026-07-08/11; the tier detail now lives inline in §§3/6 and the wave specs.

## Appendix B — the size question (informative, not directional)

A r/rust thread (2026-07-10, where rabbitui was teased) asked for "smaller than ratatui."
Decomposed: (1) most "smaller" demand is _ceremony_, not bytes — the asker described the
missing middle (grid + keybinds + modal input), which validates the pillars; answer:
`examples/simple.rs` implementing exactly that, minimally. (2) Actual weight, measured
2026-07-11: core = 3 crates; facade = 51 (the tokio tax); `hello` = 2.0 MB stripped —
fine for hosted apps; answer: publish honestly, budget dep-count/size in CI, keep default
features lean; `run_blocking` (§5) halves the tree if demanded. (3) Embedded/no_std is a
real constituency but an explicit non-goal — README should say so plainly. The thread's
allocation techniques (ArrayString, `set_stringn`, SmallVec, borrowed text) are review
heuristics for hot paths, several already encoded.

---

_Sources: `docs/research/` memos, `docs/field-report.md` Parts V–VI, ADRs 0001–0015,
qwertty 0.1.x CHANGELOG + `qwertty::docs::async_model`, dogfood findings, and the
current-surface inventory (2026-07-08/11)._
