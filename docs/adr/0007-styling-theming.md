# ADR 0007: Typed styles + semantic theme tokens; presets in v0.1; no cascade engine yet

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

Styling in a TUI framework must answer two separate questions that are easy to conflate:
*how does a widget author express color/attributes* (the style value type), and *how does an
app or end-user re-skin the whole thing* (the theming surface). The research is unusually
convergent on both.

- **"Pretty by default" is now a top-5 demand, not a nice-to-have.** The 2024–26 wave shows
  aesthetics envy of Charm/lipgloss driving adoption: ratatui-bubbletea earned 25 stars in a
  single weekend for a theme-tokens skin, and Catppuccin/Nord/Dracula presets are *expected*
  out of the box. The underlying want is escape from ratatui's "datacenter admin panel"
  default look (see docs/research/recent-rust-tui-wave.md §"What people are asking for" #7,
  and its rabbitui implications: "Ship semantic theme tokens plus Catppuccin/Nord/Dracula
  presets in v0.1").

- **Semantic tokens threaded to widgets are a *sufficient* theming substrate — proven twice
  independently.** Ink has no stylesheet, cascade, selector, or theme layer anywhere in the
  framework; styling is per-element props. Yet real theming demand (Gemini CLI users asking
  for custom color schemes, google-gemini/gemini-cli#2122) was satisfied by an ordinary
  React-context theme manager with semantic tokens (`text.primary`, `text.accent`,
  `status.*`) in settings.json — and ink-ui reached the same `extendTheme`+context shape
  independently. Five years of Ink issues contain *zero* meaningful demand for CSS selectors
  or cascade (see docs/research/ink.md §"What it gets right", §"What's worth stealing").

- **Role-based theming with partial-attr inheritance covers real apps at ~10% of the
  machinery of a CSS engine.** Brick maps *hierarchical* attribute names to partial attrs;
  lookup of `parent <> child` merges child over parent over default, so a role inherits what
  it doesn't specify. `Brick.Themes` adds named themes plus INI-file user customization
  (`loadCustomizations`). This shipped matterhorn and other real apps. The memo's explicit
  framing: "90% of Textual CSS at 10% of the machinery" (see docs/research/brick.md
  §"What's worth stealing" #5, §"Implications for rabbitui").

- **Per-widget hand-styling is a proven dead end (negative proof).** In Cursive, styling
  capability is "whatever each widget author bothered to expose": `TextArea` couldn't be
  colored because nobody hand-implemented style support the way `EditView` had it
  (gyscos/cursive#362); `TextView` renders ANSI color codes as escaped literals and
  per-section styling means giving up wrapping (#674). The flat palette can't express "style
  all buttons in dialogs," which is what forced the markup-format rethink in discussion #797.
  The memo names the lesson: "framework-resolved styling, not per-widget charity… issues
  #362/#674 show that 'each widget hand-implements its colors' leaves permanent gaps" (see
  docs/research/cursive.md).

- **Live reload is a documented #1 adoption driver — and it is separable from CSS.** Textual's
  `textual run --dev` hot-reloads stylesheets into the running app, turning styling into a
  sub-second feedback loop; it is the single most-cited reason people choose Textual (see
  docs/research/textual.md §"What it gets right", §"What's worth stealing": "the *feature* to
  steal is the dev loop, not web-CSS compatibility"). Nothing about hot reload requires a
  selector engine — it requires only a re-readable theme file and a re-resolve pass.

- **The "fake CSS" tax is real.** Textual users hit the uncanny valley: "I immediately ran
  into issues trying to use css that I'm familiar with… They should have called it something
  else" (docs/research/textual.md §"What users complain about"). Three *separate* literal-CSS
  engines were attempted in Rust in 2026 alone (textual-rs, ratatui-style, revue), which is a
  demand datum — but each one re-imports the borrow-checker and divergent-syntax costs.

- **Capability degradation is a hard requirement, not optional polish.** Truecolor is not
  universal; the substrate ADR (0012) already produces a `Capabilities` struct from a batched
  startup probe "that styling and rendering consume for degradation (truecolor → 256 → 16)"
  (DESIGN.md §Substrate). Windows/older-terminal correctness (missing colors) is called out
  as a "silent killer" in the wave (docs/research/recent-rust-tui-wave.md §13). Style
  resolution therefore cannot assume 24-bit color; it must degrade deterministically.

## Options considered

### Option A — Typed styles + semantic theme tokens, no cascade engine (CHOSEN)

*What it is.* Widget render output carries typed `Style` values, but widgets reference
semantic *role tokens* (`accent`, `surface`, `danger`, `muted`…) rather than concrete
colors. A `Theme` maps roles → concrete `Style`, framework-side, at resolve time. Presets
(Catppuccin, Nord, Dracula) ship as data in v0.1. Themes serialize as TOML and hot-reload in
debug builds. Resolution consumes the `Capabilities` struct and degrades truecolor → 256 →
16 deterministically.

*Strongest case (steelman).* This is the intersection of every memo's positive finding:
Ink's props+token result (ink.md), Brick's role tokens + partial merge + file overrides
(brick.md), Textual's hot-reload dev loop *without* its CSS engine (textual.md), and the
wave's "pretty by default + presets in v0.1" (recent-rust-tui-wave.md). It avoids Cursive's
per-widget charity by resolving roles framework-side. It composes with the declared-frame
architecture (ADR 0001) trivially: roles are just data on specs, resolved during the paint
pass — no retained object tree required, unlike a live stylesheet.

*Why not something else — this is chosen.*

### Option B — Textual-style CSS/selector cascade engine (DEFERRED)

*What it is.* A stylesheet language with type/id/class selectors, pseudo-classes
(`:hover`, `:focus`, `:focus-within`), descendant/child combinators, specificity, `!important`,
`$variables`, and nesting — resolved against the widget tree per frame, with `--dev` live
reload (docs/research/textual.md §CSS).

*Strongest case (steelman).* It is the single most-loved Textual feature and the #1 reason
people pick it: it makes *third-party* widgets themeable without forking, gives end-users a
real theming surface, and the live-reload loop is sub-second (textual.md §"What it gets
right"). Three independent Rust CSS engines in 2026 (textual-rs, ratatui-style, revue) prove
the pull is real and durable. A selector cascade expresses cross-cutting rules a flat token
map cannot ("dim every button inside a disabled dialog").

*Why not chosen.* The evidence says role tokens capture ~90% of the value (brick.md) and
that the *specific* thing users love — hot reload — is separable and delivered by Option A
without the engine. The costs are real and permanent: a Rust type-selector that "matches base
class" needs an explicit widget-taxonomy/trait-registration mechanism because it won't fall
out of inheritance (textual.md §"Implications"); per-frame selector matching against a tree
couples styling to a retained tree that ADR 0001 deliberately does not expose; and the "fake
CSS" goodwill tax (textual.md) plus Textual's own eight-majors-in-18-months churn argue
against committing the API surface early. Deferred, with an explicit revisit trigger below —
not rejected.

### Option C — Per-widget hand-styling (props only, no theme layer) (REJECTED)

*What it is.* Ink's literal model: every widget exposes its own color/attribute props; there
is no framework theming layer at all. Apps thread their own tokens if they want consistency.

*Strongest case (steelman).* Simplest possible implementation; zero framework machinery;
Ink shipped Claude Code, Gemini CLI, Copilot CLI, Gatsby on exactly this and its tracker has
no cascade demand (ink.md §"What it gets right").

*Why not chosen.* Ink survives props-only because JS callers freely thread a React-context
token map over the top — the *token layer exists*, just above the framework. Take the token
layer away entirely and you get Cursive: `TextArea` un-colorable because no author wired it,
`TextView` emitting escaped ANSI literals, no way to say "all buttons" (cursive#362, #674,
#797). rabbitui adopts Ink's *conclusion* (tokens are sufficient) but internalizes the token
layer rather than leaving it to per-widget charity — because the negative proof is
unambiguous that charity leaves permanent gaps.

## Decision

- **rabbitui styling has two layers: typed `Style` values and semantic role tokens.** A
  `Style` is a typed value (fg/bg color, attributes, modifiers), ratatui-compatible in shape
  per ADR 0003/0010. Widgets in the catalog reference **role tokens** — a fixed, documented
  set of semantic roles (`accent`, `surface`, `surface_variant`, `text`, `muted`, `danger`,
  `warning`, `success`, `info`, `selection`, `border`, `focus`…) — not hard-coded colors.
- **A `Theme` maps roles → concrete `Style`, resolved framework-side.** Resolution happens in
  the paint pass against framework-owned data (the theme + the `Capabilities` struct); it does
  not require or expose a retained widget tree. Role lookup supports partial override
  (a theme may set only the roles it cares about; unset roles fall back to the base theme),
  following Brick's partial-attr merge (brick.md).
- **rabbitui ships Catppuccin, Nord, and Dracula presets as theme data in v0.1.** "Pretty by
  default" is a shipped requirement; a sensible default theme is active with zero
  configuration.
- **Theme files are TOML; themes hot-reload in debug builds.** In a debug build, the runtime
  watches the active theme file and triggers a re-resolve + repaint on change. Release builds
  do not watch files. This delivers Textual's #1-cited dev loop without a CSS engine.
- **Style resolution is capability-driven and degrades deterministically.** The resolver reads
  the `Capabilities` struct (ADR 0012) and maps concrete colors truecolor → 256 → 16 by a
  fixed, tested quantization; a widget author writes truecolor tokens and gets correct output
  on a 16-color terminal without special-casing.
- **rabbitui does NOT ship a cascade/selector engine in v0.1.** There are no selectors,
  specificity rules, pseudo-classes, or descendant combinators. Cross-cutting styling is
  expressed by roles, per-subtree theme override (a `ThemedView`-equivalent scope, brick.md),
  and app code — not by a stylesheet matcher.

## Consequences

*Positive.*
- Captures the ~90%-value point (brick.md) at a fraction of a CSS engine's cost, and composes
  cleanly with the declared-frame model (ADR 0001) — roles are data on specs, resolved in the
  paint pass, no retained tree.
- Third-party widgets get themeable for free by referencing roles, avoiding Cursive's
  per-widget-charity gap (cursive#362/#674) by construction.
- Hot reload gives Textual's headline dev loop (textual.md) with no selector matcher and no
  "fake CSS" goodwill tax.
- Capability-driven degradation means one authored style is correct across the truecolor/256/16
  spectrum, addressing the wave's Windows/older-terminal "silent killer" (recent-rust-tui-wave.md).

*Negative (honest).*
- Cross-cutting rules a selector expresses concisely ("dim every button inside a disabled
  dialog") must be modeled as roles or handled in app code; some legitimately awkward cases
  exist and we accept them until the revisit trigger fires.
- No end-user selector surface: power users who want Textual-grade `.tcss` re-skinning of a
  *third-party* app cannot have it in v0.1. This is a real demand (three Rust CSS engines in
  2026) that we are consciously not serving yet.
- The role vocabulary is a committed API. Getting the initial role set wrong is a
  churn/compat cost; expanding it later is additive, but renaming is breaking.
- Hot reload only in debug builds means the fastest theming loop is unavailable to end-users
  of a release binary (mitigable later by an opt-in flag).

*Neutral.*
- The `Style` value type stays ratatui-compatible (ADR 0010), so the ratatui bridge and any
  future ratatui-org positioning (ADR 0014) inherit styling for free.
- A per-subtree `ThemedView`-style override scope is the escape hatch for local restyling; it
  is not a cascade, just a scoped theme swap.

## Revisit triggers

Reopen this ADR (specifically Option B) if any of the following is observed:

- **Concrete cross-cutting demand the role model can't express** appears repeatedly in issues
  or in catalog/third-party widgets — i.e. real cases where authors resort to app-code
  branching because "role + per-subtree override" is insufficient (the cursive#797 signal, but
  from *our* users on *our* model).
- **End-user re-skinning of third-party rabbitui apps becomes a stated, recurring want** — the
  exact demand the three 2026 Rust CSS engines (textual-rs, ratatui-style, revue) chase. If
  rabbitui apps ship widely enough that users want to theme apps they didn't write, a
  selector surface earns its cost.
- **The role vocabulary keeps growing to encode selector-like distinctions** (e.g. proliferating
  `button_in_dialog_disabled`-style roles), which is the smell that a flat map is being abused
  as a cascade and a real matcher would be simpler.
- If Option B is revisited, prefer the **emit-native-`Style` approach** (ratatui-style's
  design: a cascade that resolves to concrete `Style` values, composing without a retained
  DOM) over Textual's per-frame-tree matcher, and **name the language honestly** (not "CSS")
  to avoid the documented fake-CSS tax (textual.md).
