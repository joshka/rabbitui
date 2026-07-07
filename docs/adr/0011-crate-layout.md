# ADR 0011: Workspace layout, edition 2024, MSRV stable-minus-one

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

A crate layout is a stability contract made physical. Where the widget contract, the buffer, and the
runtime live — and which of them a third-party author or coding agent must depend on — decides who
pays for churn and who is insulated from it. The research gives a clear map of the right and wrong
shapes.

- **ratatui's 0.30 split is the proven-good shape, and its one mistake is instructive.** Its
  workspace is `ratatui-core` (traits/text/buffer/layout/style — the stability anchor),
  `ratatui-widgets`, per-backend crates, `ratatui-macros`, and a re-export facade; the stated goal
  was "widget libraries depend only on a slow-moving core" (`docs/research/ratatui.md`, RFC
  [#1388](https://github.com/ratatui/ratatui/issues/1388)). But `WidgetRef` — a trait third-party
  widgets must implement — shipped _outside_ core, and 0.30 had to move it out of the main crate,
  reverse its blanket-impl, and gate `render_widget_ref` behind a new `FrameExt` (`ratatui.md`;
  `BREAKING-CHANGES.md:311-347`, [#1287](https://github.com/ratatui/ratatui/issues/1287)). The
  lesson: "anything a third-party widget must implement belongs in the stability-anchor crate from
  v0.1" (`ratatui.md`).

- **Monolithic frameworks cannot shed weight.** tui-realm and r3bl_tui own every layer in few crates
  and both stall — r3bl "owning every layer is a permanent full-time job; external adoption stays
  near zero" — while "ratatui 0.30's core split shows the ecosystem rewards small stable cores"
  (`docs/research/prior-art.md`).

- **The runtime is the crate that must not leak.** Every concurrency memo reaches one rule: widget
  crates must not depend on tokio; "only the optional runtime crate owns polling, workers, and
  cancellation" (`prior-art.md`; ADR 0005 confines the `select!` loop and tokio to the runtime
  layer). iocraft's `use_future` panics (`prior-art.md`, iocraft #48) are runtime concerns reaching
  where they should not; the crate boundary keeps them out.

- **The labs already drafted this layout.** `ratatui-labs`' `widget-system-vision.md` specifies
  foundation → family → optional-runtime → facade with strict dependency direction, which
  `prior-art.md` says to "adopt nearly verbatim." rabbitui collapses the labs' many foundation
  crates into one `rabbitui-core` (they split them to probe seams; a shipping framework wants one
  anchor) but keeps the direction and the optional-runtime rule.

- **Churn is the tax a layout amortizes or invites.** ratatui's `BREAKING-CHANGES.md` narrates 74
  breaking changes, "mostly signature drift on builder methods" (`ratatui.md`); maintaining that
  single migration narrative is listed under "worth stealing." Edition and MSRV policy are the other
  half of the contract: how often downstream toolchains must move.

Out of scope: the `ratatui-*` naming/positioning question (ADR 0014, deferred) and the substrate
crate seam (ADR 0012 keeps qwertty behind `terminal.rs`, not a published crate).

## Options considered

### A. Single crate with feature flags (the monolith)

_What it is:_ one `rabbitui` crate; widgets, testing, runtime, and bridge gated behind Cargo
features.

_Steelman:_ simplest to author and release; one version number; users add one dependency; feature
flags give a "pay for what you use" story.

_Why not chosen:_ feature flags give third-party widget authors no _slow-moving surface to depend
on_ — they depend on the whole crate and absorb all its churn, the version-skew pain ratatui's split
ended ([#1388](https://github.com/ratatui/ratatui/issues/1388)). Features are additive, so one
dependent enabling `runtime` drags tokio into everyone's build, breaking runtime confinement. This
is the tui-realm/r3bl monolith `prior-art.md` documents.

### B. Maximal split (a crate per foundation concern)

_What it is:_ the labs' literal `widget-system-vision.md` layout — separate `-action`, `-layout`,
`-surface`, `-interaction`, `-text`, `-theme`, `-diagnostics` foundation crates, plus family crates
per widget group.

_Steelman:_ maximally granular stability; each concern versions independently; the labs proved the
boundaries are real. A `-text` consumer need not pull `-theme`.

_Why not chosen:_ the labs split concerns to _probe_ where the seams are (`prior-art.md`); that is
exploration tooling, not a shipping contract. N foundation crates means N versions to keep in
lockstep and N points of break coordination — skew at a finer grain. The concerns (ids, facts,
buffer, style, layout, widget contract) co-evolve tightly because the widget contract references all
of them; one `rabbitui-core` gives one anchor and one SemVer promise. Family crates can still be
carved out of `rabbitui-widgets` later, non-breaking.

### C. Five-crate split: core / widgets / testing / bridge / facade, runtime in the facade (CHOSEN)

_What it is:_ a Cargo workspace of `rabbitui-core` (ids, facts, buffer, style, layout, the widget
contract — no runtime deps), `rabbitui-widgets` (the catalog), `rabbitui-testing` (headless + PTY
harness), `rabbitui-ratatui` (the ADR 0010 bridge), and `rabbitui` (the facade: the tokio runtime
and event loop plus re-exports — the crate users depend on). Optional shells like `rabbitui-tea`
come later as their own crates.

_Steelman:_ it is ratatui's proven shape with its one documented mistake corrected — the widget
contract, the surface third parties must implement, lives in `core` from v0.1, not outside it. It
keeps tokio out of `core`, `widgets`, and `testing` by construction: the runtime lives in the
facade, so a widget author or the headless test driver never compiles a runtime. It gives exactly
one stability anchor and exactly one crate for users to add.

_Why not chosen — its honest cost:_ two boundary calls are debatable. Putting the runtime _in the
facade_ rather than a separate `rabbitui-runtime` means the facade is not purely re-exports (see
Dissent). And a five-crate workspace is more release ceremony than a monolith. Both costs are judged
worth paying for the insulation they buy.

## Decision

rabbitui ships as a Cargo **workspace** with this layout and dependency direction:

1. **`rabbitui-core`** holds ids/`WidgetId`, frame facts, the cell buffer, styles and theme roles,
   layout, and — normatively — **the one public widget contract, present from v0.1.** Everything a
   third-party widget or coding agent must implement to be a rabbitui widget is defined here.
   `rabbitui-core` has **no runtime dependency** (no tokio, no async executor). This directly
   applies `ratatui.md`'s rule that the third-party trait surface belongs in the stability anchor,
   correcting the `WidgetRef`-outside-core mistake.

2. **`rabbitui-widgets`** is the first-party catalog (ADR 0008), depending only on `rabbitui-core`.
   It is **runtime-free**; a widget crate never touches tokio.

3. **`rabbitui-testing`** (ADR 0009) — the headless driver and the PTY/escape-sequence harness —
   depends on `rabbitui-core` and is public API. It is **runtime-free**: the headless driver
   advances an injectable clock and pumps frames without owning a real async runtime, so widget
   authors test without compiling one.

4. **`rabbitui-ratatui`** (ADR 0010) is the bridge leaf crate, the only place `ratatui` types
   appear. It depends on `rabbitui-core` and `ratatui`; nothing depends on it.

5. **`rabbitui`** is the **facade**: it owns the tokio runtime and the `select!` event loop
   (ADR 0005) and re-exports `rabbitui-core` and `rabbitui-widgets`. **Runtime dependencies are
   confined to this crate.** It is the crate users depend on for the batteries-included experience.
   "Bring your own loop" (ADR 0005) means depending on `rabbitui-core` + `rabbitui-widgets` directly
   and driving them yourself, without the facade's runtime.

6. **Dependency direction is strict and one-way:** `core` depends on nothing internal; `widgets`,
   `testing`, and `ratatui`-bridge depend on `core`; the facade depends on `core` + `widgets` (+
   runtime). No crate depends on the facade. Optional shells (`rabbitui-tea`, a later Xilem-style
   view-diff layer) are separate crates layered _above_ this, never inside the widget contract (ADR
   0001).

7. **Edition 2024.** All workspace crates target Rust edition 2024.

8. **MSRV = stable minus one.** The minimum supported Rust version trails current stable by one
   release. MSRV is **bumped only on minor releases** (never a patch), and each bump is a
   `BREAKING-CHANGES.md` entry.

9. **`BREAKING-CHANGES.md` discipline, copied from ratatui.** A single maintained file narrates
   every breaking change with migration guidance (`ratatui.md`, "worth stealing"). MSRV bumps,
   edition moves, and any change to the `rabbitui-core` widget contract or cell-model convertibility
   (ADR 0010 §4) are recorded there.

10. **Feature-flag posture: minimal and additive-safe.** Features gate optional _capabilities_,
    never the core contract, and never as a substitute for the crate split. Themes beyond the
    built-in presets, optional shells, and the like may be features; the widget contract, buffer,
    and facts are always present in `core`. Following `ratatui.md`'s churn guidance, anything
    unproven ships behind an explicit `unstable-*` feature (ratatui's `unstable-widget-ref`
    precedent) so it can change without a breaking release.

## Consequences

### Positive

- One stability anchor: third-party authors and coding agents depend on `rabbitui-core` alone,
  insulated from runtime, catalog, and bridge churn — the version-skew complaint ratatui's split
  addressed ([#1388](https://github.com/ratatui/ratatui/issues/1388)), with the
  `WidgetRef`-outside-core regret pre-empted.
- Runtime confinement is structural, not conventional: a widget crate or headless test cannot
  compile tokio, enforcing ADR 0005's "widget crates stay runtime-free" at the dependency graph
  rather than by discipline.
- Incremental adoption (a `prior-art.md` survival requirement): a plain ratatui/qwertty app can add
  `rabbitui-core` + `rabbitui-widgets` + the bridge without the facade, or take the facade for
  batteries included.
- Edition 2024 + stable-minus-one MSRV gives a predictable, non-bleeding-edge floor;
  `BREAKING-CHANGES.md` makes every break — MSRV bumps included — one reviewable narrative, the
  discipline `ratatui.md` credits for surviving 74 changes.

### Negative (honest)

- The facade is **not** purely re-exports: it carries the runtime, so `rabbitui` is heavier than
  ratatui's re-export-only facade. (See Dissent.)
- Five crates is more release ceremony than a monolith: coordinated bumps and inter-crate SemVer
  care. Judged worth it for insulation.
- MSRV stable-minus-one still excludes the most conservative users (LTS distros, air-gapped pins);
  we are not targeting them, and minor-release-only bumps are the concession. Edition 2024 similarly
  raises the launch floor and forecloses older toolchains.

### Neutral

- `rabbitui-ratatui` absorbs ratatui's churn, but only in that leaf crate (ADR 0010) — the boundary
  contains it.
- Family crates (`-forms`/`-collections`/… out of `rabbitui-widgets`) and moving the runtime to its
  own `rabbitui-runtime` crate both remain available later, additively, without touching the core
  contract.

## Revisit triggers

- **The facade's runtime becomes a burden to facade-less users.** If "bring your own loop" adopters
  repeatedly need runtime pieces that only live in the facade, split out `rabbitui-runtime` so the
  facade returns to pure re-exports.
- **`rabbitui-widgets` grows past comfortable single-crate size or compile time.** Then carve out
  the labs' family crates (`-forms`, `-collections`, `-overlays`, …) — the split is pre-designed to
  be non-breaking on `core`.
- **A foundation concern needs to version independently of the rest of `core`.** If, say, the
  text/width layer (ADR 0012's width oracle) or the theme layer proves to churn on a different clock
  than the widget contract, reconsider extracting it toward Option B's finer split.
- **MSRV stable-minus-one causes real adoption loss** — measured demand from users pinned to older
  toolchains — reopens the MSRV policy (e.g. stable-minus-two, or an explicit LTS target).
- **Positioning lands on the ratatui org (ADR 0014).** Merging under `ratatui-*` names would force a
  re-evaluation of crate names and whether `rabbitui-core` and `ratatui-core` converge.

## Dissent

Putting the runtime in the `rabbitui` facade rather than a dedicated `rabbitui-runtime` crate blurs
a boundary the rest of this ADR draws sharply. ratatui's facade is re-exports only; ours carries
live tokio machinery, so the one crate users are told to depend on is also the one that pins an
async runtime. The clean parallel to the runtime-free rule enforced below the facade would be `core`
/ `widgets` / `runtime` (loop + tokio) / `rabbitui` (re-exports of all three) — every crate
single-purpose, and a "bring your own loop" user depends on core + widgets and ignores `runtime`
cleanly. The decision stands: one crate for users to add is a real ergonomic win, and the
confinement that matters most (widgets, testing) is already achieved. But the first revisit trigger
exists because this is the layout's weakest seam, and the split-out is kept additive so it can be
taken later without a break.
