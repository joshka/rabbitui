# ADR 0015: API naming — full words, std-idiomatic

- Status: accepted (2026-07-11)
- Deciders: joshka

## Context

Public API names are the one decision that is effectively forever: every rename after 0.1
is a breaking change multiplied across every consumer, doc, and tutorial. Pre-0.1 is the
only cheap moment to get them right. An audit (2026-07-11) found several public names had
imported Bubble Tea's Go abbreviation culture or rustc-internal shorthand rather than
Rust's std idiom, where words are spelled out (`std::process::Command`,
`std::task::Context`, crossterm's `Attributes`).

## Decision

**Public API names spell words out.** An abbreviation is acceptable only when the
abbreviation _is_ the domain term (vt100, SGR, IME) or the ecosystem-universal word.

Renames executed with this ADR (whole workspace + living docs, in one sweep):

- `Cmd` → **`Command`** (std `process::Command`, clap precedent; `Cmd` was Bubble Tea's).
- `RenderCtx` / `HandleCtx` → **`RenderContext`** / **`HandleContext`**
  (std `task::Context` precedent).
- `Attrs` → **`Attributes`** (crossterm precedent).
- module `a11y` → **`accessibility`** (numeronym slang out of API paths).
- App-land `Msg` → **`Message`** in the examples, flagship, and comparison app — app code
  is the teaching idiom, so it follows the same rule.

Deliberately **kept** (domain-standard, not abbreviation culture):

- `Rect` — the universal graphics term (ratatui, egui, wgpu all agree); `Rectangle` would
  fight the entire ecosystem.
- `WidgetId` — `Id` is sanctioned by the Rust API guidelines.
- `fg` / `bg` — universal styling vocabulary (crossterm, ratatui).
- `vt` / `VtScreen` — the domain is literally named vt100.
- `from_fn` / `FnApp` — std precedent (`iter::from_fn`, `iter::FromFn`).

## Consequences

- Earlier ADRs (0005 effects, 0008 widget contract, and design notes) that say `Cmd` or
  `RenderCtx` are historical records — read them with this rename applied. Per the
  supersession discipline they are not silently rewritten; this ADR is the correction.
- Research memos and field reports keep `Cmd`/`Msg` where they quote other frameworks
  (Bubble Tea's Go API really is named `Cmd`).
- New public items are reviewed against this rule; when in doubt, spell it out.
