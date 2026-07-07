# Prior Art: Next-Gen Rust TUI Attempts

**Verdict:** The "next-gen Rust TUI" synthesis has been attempted at least six times; every attempt
either died with its author's flagship app or plateaued at niche adoption because it demanded
wholesale paradigm buy-in instead of layering on Ratatui's ecosystem — the winning move is
incremental adoption plus a real widget catalog, which is exactly what the local ratatui-labs work
already mapped out.

Date: 2026-07-06

> **See also:** [recent-rust-tui-wave.md](recent-rust-tui-wave.md) — a follow-up sweep of the
> 2024–2026 wave (63 frameworks, ~⅓ AI-generated) that stress-tests this memo's conclusions against
> fresh demand data and revises several (inline mode, AI-agent workloads, theming).

Sources:

- <https://github.com/veeso/tui-realm> (repo + `gh api`: 969 stars, pushed 2026-07-01, v4.1.0
  2026-05-02; issues #21, #173, #180, #185)
- <https://github.com/ccbrown/iocraft> (repo + `gh api`: 1,354 stars, pushed 2026-06-19; issues #37,
  #42, #48, #68, #202, #206)
- <https://github.com/aschey/rooibos> (repo + `gh api`: 5 stars, pushed 2026-04-04, "pre-alpha")
- <https://github.com/enricozb/intuitive> (repo + `gh api`: 216 stars; README: "This crate currently
  does not work... unmaintained")
- <https://github.com/r3bl-org/r3bl-open-core> (repo + `gh api`: 475 stars, pushed 2026-07-05, 65
  open issues)
- <https://github.com/mcobzarenco/zi> (repo + `gh api`: 156 stars, pushed 2023-02-10)
- <https://github.com/deinstapel/cursive-multiplex> (`gh api`: 59 stars, pushed 2024-08-12);
  deinstapel/cursive-tabs (31 stars, 2024-08)
- crates.io API (2026-07-06): tuirealm 204k downloads / 26.5k recent; iocraft 130k / 40.2k recent;
  r3bl_tui 368k / 28.9k recent; intuitive 18.9k / 36 recent; zi 23.2k / 537 recent; cursive 1.51M /
  245k recent; ratatui 36.2M / 13.5M recent
- Ratatui org repo list via `gh api orgs/ratatui/repos` (no rfcs repo; has kasuari, ratatui-labs,
  ratatui-reservations, async-template, templates)
- Ratatui discussions via GraphQL: #1969 "Components, widgets, whatits?!", #1933 "RFC: switch layout
  engine", #1925 "RFC: Text Wrapping Design", #2054 "Ratatui with Elm flavour", #1930 "Widgets:
  Click events", #2130 (kitty text-sizing), #2190 (accesskit)
- <https://github.com/ratatui/async-template> (archived 2023-12-15, superseded by ratatui/templates)
- <https://ratatui.rs/concepts/application-patterns/> and <https://ratatui.rs/faq/>
- <https://textual.textualize.io/widget_gallery/> (35 built-in widgets listed, counted 2026-07-06)
- Companion memos in this directory: textual.md, rust-gui-lessons.md
- HN "Ratatui – App Showcase" comments via Algolia API
  (<https://news.ycombinator.com/item?id=45830829>); HN iocraft thread id=41632566;
  <https://users.rust-lang.org/t/iocraft-new-tui-cli-library-with-react-like-style/119236>
- Local: /Users/joshka/local/ratatui-labs/AGENTS.md;
  proof-text-forms/docs/prds/{interaction-model-survey.md, proof-roadmap.md,
  widget-system-vision.md, first-three-widgets.md};
  layout-action-overlap/.plans/layout-action-overlap.md;
  import-ratatui-layout/crates/ratatui-layout/README.md; local-notes/{betamax-feedback.md,
  practice-docs-feedback.md}; interaction-model-survey/crates/ratatui-labs/examples/

## The attempts

**tui-realm (veeso)** — React components + Elm update, layered on ratatui. Components own state and
receive `Cmd`s; the app matches on a global `Msg` enum; `Subscriptions` and `Ports` feed external
event sources into the loop. 969 stars, 204k downloads, actively maintained (v4.1.0, May 2026) — the
most successful _framework-on-ratatui_ to date, and it exists to serve the author's own app
(termscp). Reality check from its own tracker: the core abstraction was named `MockComponent` for
three major versions and only renamed to `Component` in 4.0 because everyone found it baffling
(issue #180); users reported the event loop feeling slower than hand-rolled ratatui (#21); async
integration is still bolted on via feature-gated ports with an open request for "syntactic sugar for
dealing with async responses" (#173). Lesson: double bookkeeping — you write both a component tree
_and_ a message enum — is tolerated, not loved, and one maintainer's app keeps it alive.

**iocraft (ccbrown)** — React/Ink in Rust, _not_ on ratatui: `element!` macro, `#[component]`, hooks
(`use_state`, `use_future`), taffy flexbox, and both fullscreen and **inline** (print-and-exit)
output. 1,354 stars and the best recent-download velocity of any framework here (40k/90d vs
tui-realm's 26k), despite being younger. Its issue tracker shows where hooks-in-Rust leaks: panics
when accessing `State::write` inside `use_future` (#48), TextInput with "multi-problems" (#206), no
overflow scroll for fullscreen (#68), nesting styled text awkward (#75), and users asking for
pluggable terminal backends (#202). The README explicitly positions against ratatui ("serves a
similar purpose with a less declarative API"). Lesson: the React model is _productive_ until the
runtime rules (hook ordering, state lifetimes) bite, and those rules can't be encoded in the type
system — they become panics. The inline-output mode is a genuinely differentiating feature nobody
else has.

**rooibos (aschey)** — the complete synthesis rabbitui is contemplating already exists: Leptos-style
fine-grained signals, `Send + Sync` reactivity, ratatui widgets for rendering, taffy for
flexbox/grid, async-first, testing-library-inspired test crate, and backends for
terminal/SSH/web/testing with Vello/egui/Bevy planned. It has **5 stars**, 12 open issues, last push
April 2026, and self-describes as "pre-alpha... should not be used for anything beyond
experimentation." Lesson: existence of the right feature list is worth nothing without docs, a
widget catalog, a flagship app, and marketing. This is the most important cautionary tale in the
survey.

**intuitive (enricozb)** — React/SwiftUI-flavored `#[component]` + `render!` + `use_state` on
tui-rs. 216 stars, dead since 2022; the README now says "This crate currently does not work. (Hooks
don't work, rendering is synchronous...) It's unmaintained." Lesson: a declarative macro veneer over
an immediate-mode core without a real reconciler produces something that demos well and cannot be
finished part-time.

**r3bl_tui (r3bl-org)** — whole-cloth stack: React/SolidJS/Elm-inspired, "totally async" on tokio,
double-buffered compositor "painting only diffs" (their SSH-optimization pitch), CSS-like styling,
JSX-like declarative layout, flexbox-like engine, plus editor component, Markdown parser, PTY mux.
475 stars, 368k downloads, actively developed (pushed 2026-07-05), 65 open issues, still v0.x after
~5 years. Its production users are the org's own apps (`giti`, `edi`). Lesson: owning every layer
(rendering, layout, styling, widgets, runtime) is a permanent full-time job; external adoption stays
near zero because there's no incremental path in from ratatui.

**zi (mcobzarenco)** — Elm-ish incremental declarative components, built to power the zee editor.
Dead: last push Feb 2023, last crates.io release Apr 2022, 537 downloads/90d. Lesson: frameworks
extracted from a single app die when the app does.

**cursive + its ecosystem** — the retained-tree, callback-driven old guard (4,823 stars, 1.5M
downloads, semi-maintained: last release Aug 2024). The deinstapel "cursive ecosystem" push
(cursive-multiplex, 59 stars; cursive-tabs, 31 stars; both last pushed 2024) shows what happens to
third-party extension ecosystems on a retained tree: they rot the moment core velocity drops. HN
commenters in the Ratatui showcase thread independently diagnose why retained trees are rare in
Rust: parent/child cyclical references and Qt-style signal/slot patterns "would be difficult to
implement in Rust" (dlivingston, HN 45830829).

**Ratatui itself (the control group)** — 36.2M downloads, 13.5M in the last 90 days; it is the
substrate, not a framework. There is **no rfcs repo** in the org (verified via repo listing);
architectural direction lives in discussions: layout-engine RFC #1933 (cassowary-rs unmaintained
since 2018, inconsistent expansion, panics under overconstraint → org now maintains the `kasuari`
fork), text-wrapping RFC #1925, "Ratatui with Elm flavour" #2054, click-events #1930, kitty
text-sizing #2130, accesskit #2190. The official async/component answer was the `async-template`
(Component trait + tokio + action channels), archived Dec 2023 into `ratatui/templates`; the website
documents Elm/Component/Flux as _user-choice patterns_ rather than shipping any of them. In the HN
showcase thread, the two loudest user complaints are precisely rabbitui's opportunity: "you need to
take on third party dependencies for each individual widget... basic things like spinners,
checkboxes, text areas" with version-skew pain across widget crates (ModernMech), and "I haven't
found a widget library and event loop that I like, so had to roll my own" (alfiedotwtf).

## The local experiments (ratatui-labs)

`/Users/joshka/local/ratatui-labs` is a jj-workspace container (AGENTS.md: one workspace per
experiment) holding the user's own answer-in-progress. It is far more decided than "exploration"
suggests.

- **interaction-model-survey/** — the core document (`docs/prds/interaction-model-survey.md`,
  mirrored in each workspace) implements the _same command strip four ways_ (manual bookkeeping,
  `FrameSnapshot` primitives, a smaller zone primitive, a component shell) and surveys nine
  architectures from plain ratatui to retained trees and runtimes. Its finding: all four produce
  identical behavior; the repeated cost is **previous-frame target bookkeeping** (visible targets,
  disabled routing, focus/hover/press convergence). Verdict, stated as a creed: _"render produces
  visible facts / input routes through previous visible facts / controls return outcomes / the app
  owns effects."_ It explicitly rejects: retained tree as foundation, React hooks as the state
  model, Yoga/flexbox as default layout, CSS/DOM as the core model, and callback-hidden mutation —
  while explicitly borrowing Xilem's id-paths, Masonry's inspectability/replay requirements, Bubble
  Tea's messages-for-timers, and Textual's product scope (screens, palette, workers, devtools). The
  workspace also contains ~25 `widget_*.rs` proof examples (button through combo box, date input,
  multi-select, tabs, toolbar) plus reference-app examples (settings_studio, json_data_explorer,
  metrics_dashboard...).
- **proof-roadmap.md** — a 10-step dependency-ordered proof plan: action zones → frame-fact
  inspector/replay harness → forms/text → collections/durable selection → pointer capture → nested
  routing/overlays → semantic theme → optional app model/runtime → reference apps → widget-author
  template + extraction review. Each step names what failure would teach.
- **widget-system-vision.md** — a full crate-layout plan: foundation crates (`ratatui-action`,
  `ratatui-layout`/`-surface`, `-interaction`, `-text`, `-theme`, `-testing`, `-diagnostics`,
  optional `-runtime`), family crates (`-controls`, `-forms`, `-collections`, `-overlays`,
  `-viewers`, `-app-widgets`), then a facade. Plus the shared widget contract: **Spec / State /
  Layout-View / Frame facts / Input / Outcome / Policy / Style / Diagnostics** — a checklist,
  deliberately not a generic `Component` trait.
- **proof-action-controls / proof-text-forms / proof-selection-list / proof-resource-picker /
  proof-theme-diagnostics** — workspaces executing the "first work packet" from
  `first-three-widgets.md`: `ActionRow`, `TextInput`, `SelectionList` proved together in a
  resource-picker reference app, then theme + diagnostics. Chosen because they force cursor
  requests, text editing, durable ids, virtualization, and roving focus — not because they're easy
  to draw.
- **import-ratatui-layout** — imported the `ratatui-layout` crate (0.0.1-alpha): frame-local
  coordination primitives — `Regions`, `FrameSnapshot`, focus/pointer targets, cursor requests,
  scroll metrics, visible selection — with ~20 runnable examples (modal_shell, focus_traversal,
  pointer_only_scroll_region, form_dialog...).
- **layout-action-overlap** — boundary probe between semantic actions and visible layout
  (`.plans/layout-action-overlap.md`). Conclusion: keep `ratatui-action` rendering-agnostic, keep
  layout generic over ids, add a thin adapter only when examples repeatedly demand it; disabled
  actions stay visible but skip focus/pointer routing; hidden actions are omitted from layout.
- **local-notes/betamax-feedback.md** — the testing story in practice: a Betamax tape driving the
  real command-palette example **caught a real bug unit tests missed** (stale selection index across
  query edits invoked the wrong action). Also: separate CI semantic-waits from presentation holds,
  checkpoint manifests mapping artifacts to the behavior they prove, parallel tape execution.
  `practice-docs-feedback.md` records the docs doctrine (crate root teaches the model; Rustdoc as
  contract; `-D warnings` doc builds).

Net: the labs already ran the synthesis question and answered it — not Elm, not React, not a
retained tree, but **frame facts + explicit state + outcomes + optional shells**, proven bottom-up
widget by widget.

## Why nothing stuck

1. **One person, one app.** zi died with zee; tui-realm lives exactly as long as termscp needs it;
   r3bl serves r3bl's apps; rooibos and intuitive never found a second user. No Rust TUI framework
   has survived its author's attention span except the non-framework (ratatui, which has an org,
   funding, and 13.5M downloads/quarter of gravity).
2. **Imported paradigms fight Rust.** Hooks become runtime panics (iocraft #48: `State::write` in
   `use_future` panics); retained trees force `Rc<RefCell>` and parent-pointer gymnastics (cursive;
   HN commentary on why Qt-in-Rust doesn't exist); Elm message enums metastasize (tui-realm's
   `Msg`/`Cmd` double bookkeeping; the labs survey's "verbose message enums can dominate small
   apps"). Every paradigm was designed for a GC'd language and pays a Rust tax that the framework
   author, not the user, must hide.
3. **Wholesale buy-in loses to Ratatui's gravity.** Frameworks that replace ratatui (iocraft, r3bl,
   zi) forfeit the widget ecosystem and 36M-download network effect; frameworks that layer on it
   (tui-realm, rooibos) get squeezed as ratatui absorbs features (0.30's stable `ratatui-core` split
   was announced by joshka in the HN thread specifically to fix widget-crate version skew). Ratatui
   users demonstrably want to adopt one concept at a time — the labs' "What Should Stay
   Ratatui-Like" section treats that as a hard constraint.
4. **The missing product was never the architecture.** HN users don't ask for signals or
   reconcilers; they ask for a batteries-included widget catalog and a blessed event loop
   (ModernMech, alfiedotwtf, tptacek's "very good TUI frameworks, maybe inspired by Python
   Textual?"). rooibos shipped the architecture without the catalog: 5 stars. Textual won its
   ecosystem with a curated catalog — 35 built-in widgets in its official gallery
   (<https://textual.textualize.io/widget_gallery/>) — plus CSS theming, devtools, and docs; the
   catalog _was_ the product, even though Textualize's own flagship apps sometimes routed around the
   stock widgets for performance (toolong's custom log ScrollView, the 800× fastdatatable gap — see
   textual.md). The lesson is about coverage of the everyday cases HN users name (spinners,
   checkboxes, text areas), not raw widget count.
5. **No testing/inspection story, no maintainability.** intuitive and zi had no way to verify
   behavior at scale; the labs' Betamax finding (tapes catch what unit tests miss) and Masonry's
   "testable, inspectable, replayable" requirement point at the same gap. Frameworks without a
   headless driver can't even test themselves, let alone let users test apps.

## Implications for rabbitui

- **Programming model: adopt the labs' frame-facts model as the core, not Elm/React/retained.**
  Every imported paradigm has a failed or stalled Rust instance (intuitive, zi, tui-realm, rooibos);
  the labs' "render produces facts → input routes through previous facts → controls return outcomes
  → app owns effects" is immediate-mode-compatible and borrow-checker-friendly. Scope the evidence
  honestly: the four-way implementation validates it for _interaction bookkeeping_
  (focus/hover/press convergence on a single command strip), not yet for large dynamic views — that
  proof is still ahead (roadmap steps 8–9). Offer an Elm-style app shell and a Xilem-style view-diff
  layer as _optional crates above_ it (proof-roadmap step 8), never as the widget contract.

  This directly contradicts the companion memo rust-gui-lessons.md, which recommends the Xilem
  view/element split (ephemeral typed view tree diffed against a retained widget core) as _the_
  programming model, on desktop-GUI evidence: widget identity for focus/accessibility, incremental
  view computation, and accessibility APIs that "eliminate pure immediate mode as viable for
  production." Three reasons the labs model still wins as the terminal core: (1) the costs the split
  amortizes are small here — TUI trees are tiny, paint is a cheap buffer diff, and there is no 60fps
  animation-driven view rebuild; (2) the split's essential payload, stable identity, is already
  captured as data — frame facts carry app-owned ids plus Xilem-style id-paths for overlays (next
  bullet); (3) the labs proofs ran against real terminal widgets, while the Xilem recommendation
  extrapolates from desktop toolkits. The desktop argument regains force exactly where its premises
  return: if the reference apps (roadmap step 9) hit view-construction cost on large dynamic views,
  or if accessibility work (ratatui #2190, accesskit) demands a persistent identified tree, promote
  the view-diff layer from optional to default. Treat this as a live disagreement to be settled by
  roadmap steps 8–9, not a settled question.

- **Widget identity: stable app-owned ids + scoped id-paths for overlays, no retained tree.** Steal
  Xilem's id-path idea exactly as the labs did (interaction-model-survey "Overlays And Dialogs"):
  local ids inside an overlay, translated to parent coordinates, z-ordered for pointer routing, with
  focus restoration — this delivers component-tree benefits (routing, devtools, replay) without
  cursive's ownership problems.
- **State ownership: Spec/State/Input/Outcome as a checklist contract, not a `Component` trait.**
  tui-realm's `MockComponent` saga and ratatui discussion #1969 ("Components, widgets, whatits?!")
  show generic component traits confuse more than they unify. Widgets expose conceptual accessors
  (`value`, `selection`, `cursor`, `is_open`), return outcome data, and never execute app effects
  via callbacks.
- **Rendering: keep double-buffer diffing; add scoped frames with z-order for overlays.** Everyone
  converged on buffer diff (ratatui, r3bl's "painting only diffs" SSH pitch); the unsolved part is
  layering — the labs identify z-order, backdrop policy, outside-click, and clipping as where "a
  flat zone list stops being enough." Design the layer/overlay primitive early; it gates modals,
  palettes, menus, tooltips.
- **Layout: don't default to cassowary _or_ flexbox; make text measurement first-class.** Cassowary
  is a documented failure (ratatui #1933: inconsistent expansion, overconstraint panics, dead
  upstream → kasuari fork); taffy is what iocraft/rooibos chose and works, but the labs deliberately
  discard "Yoga/Flexbox as the default layout dependency." Ship a simple constraint-splitting core,
  make taffy an adapter, and treat Unicode width/grapheme/wrapping/hit-testing as an explicit
  `-text` layer (labs: "editable text is not 'just a widget'"; ratatui text-wrapping RFC #1925 is
  still open after 22 comments).
- **Concurrency: async-first runtime, sync-capable widgets.** The demand is unambiguous (ratatui's
  archived async-template, tui-realm #173, iocraft's `use_future`, r3bl's "totally async", qwertty's
  async substrate) — but the labs rule holds: widget crates must not depend on tokio or any runtime;
  timers/subprocess/network arrive as events into the loop, and only the optional runtime crate owns
  polling, workers, and cancellation.
- **Input/focus: backend-neutral input commands at the adapter edge; focus scopes distinct from
  traversal.** Route kitty-keyboard/paste/mouse specifics in qwertty adapters; widgets consume
  domain input (activate, cancel, focus-next, pointer-press). The labs' pointer-capture and
  nested-routing proofs (roadmap steps 5–6) enumerate the hard cases (drag past target, release
  outside app, Escape policy) — schedule those proofs before stabilizing any event API.
- **Styling: semantic roles + per-widget style structs first; earn a CSS engine later.** The labs'
  theme proof has explicit failure criteria ("if widgets need CSS-like selectors to be usable,
  semantic roles are too weak"). Textual's CSS is its moat, but it sits on a retained DOM rabbitui
  won't have; start with a role matrix
  (focused/hovered/pressed/disabled/invalid/primary/destructive) proven across five widget families.
- **Widget set: the catalog is the product — ship 20+ first-party widgets at launch.** The single
  loudest real-world complaint about ratatui is sparse built-ins with third-party version skew (HN
  45830829). Follow the labs' proof-pack ordering (ActionRow, TextInput, SelectionList first — they
  force cursor, editing, durable selection, virtualization) and the widget-author template so widget
  #50 matches widget #1.
- **Testing: build the replay harness and frame-fact inspector _before_ the widget catalog (roadmap
  step 2).** Frame facts are already data — snapshot them, replay scripted input against them, and
  pair with tape-based rendered proof; the Betamax experience shows tapes catch cross-mode bugs unit
  tests miss. A headless driver is also the only credible CI story for third-party widget authors.
- **Crate layout: foundation crates → family crates → optional runtime → facade, with strict
  dependency direction.** Adopt the widget-system-vision layout nearly verbatim
  (action/layout/interaction/text/theme/testing/diagnostics foundations;
  controls/forms/collections/overlays/viewers families). tui-realm and r3bl show monolithic
  frameworks can't shed weight; ratatui 0.30's core split shows the ecosystem rewards small stable
  cores.
- **Survival plan: incremental adoption path + a flagship app.** rabbitui must be usable one crate
  at a time from a plain ratatui/qwertty app (rooibos' fate is the price of skipping this), and it
  needs at least one serious reference app treated as an acceptance test, not a demo (labs roadmap
  step 9: control plane, settings app, reader) — every surviving framework in this survey is kept
  alive by exactly one real app.
