# Arc 2A completion plan — aesthetics as a system

Status at hand-off (2026-07-07): Panel, center/inset helpers, the nine-example restyle, and the
Nord/Dracula/Catppuccin presets are done. Four items remain. They are small, independent, and
delegation-friendly (sonnet-grade, one agent each, or one agent for all four in sequence).

## 1. `Theme::default` retune + role coverage audit

**Decision — direction of the retune:** the default theme stays **terminal-native ANSI** (it must
respect the user's own palette; that is its entire reason to exist next to the hex presets). Retune
means: every `Role` maps to a distinct, legible ANSI slot on both dark and light terminals.
Constraints (all shipped-bug-derived, see playbook): no DIM anywhere; `Muted` is `Ansi(8)` with no
attribute; `Accent` is fg-only (used on focused borders); `Highlight` carries bg (used only for
selection); `Danger`/`Warning`/`Success` = red/yellow/green ANSI. Where the current default deviates
from this table, fix the table — do not introduce hex values into the default theme.

**Audit mechanics:** sweep `rabbitui-widgets`, `rabbitui/src`, and all examples for direct
`Color::`/`Ansi(`/hex construction outside `theme.rs` and the preset definitions. Every hit either
becomes a `Role` lookup or gets a `// deliberate:` comment stating why a raw color is correct (the
encode layer and tests are exempt). Add the finding list to the summary; convert recurring
legitimate needs into new roles only if two or more widgets need the same one.

**Acceptance:** gallery example (item 3) rendered under all four presets + default shows no
illegible pairings; grep audit clean; existing tapes still pass (help lines unchanged).

## 2. Spacing/density design tokens

**Decision — scope:** constants, not a system. Add a `theme::spacing` module (rabbitui-core) with
named constants the widgets and examples share: `GAP: u16 = 1` (between sibling panels/sections),
`PANEL_PADDING: u16 = 1`, `FORM_LABEL_GAP`, `OVERLAY_MARGIN: u16 = 2` (modal inset from screen
edge). Then replace magic numbers in Panel, the form/agent/todo examples, and the modal layout
helper with them. Do **not** build a density-scaling abstraction (compact/comfortable modes) — that
is speculative; note it in the deferred ledger instead.

**Acceptance:** no bare spacing literals in examples' layout code; `cargo test` + tapes green
(spacing values unchanged means tapes should not shift — if a tape shifts, the old value was
inconsistent; update the tape and say so).

## 3. Gallery example (`examples/gallery.rs`)

One screen showing every widget in the catalog (Text with wrap + styled spans, Button focused and
unfocused, TextInput filled and empty and validated-error state, SelectionList with selection,
Collapsible open+closed, Panel variants, LogOverlay toggled by key, a modal on `m`), laid out with
the spacing tokens in a scrollable column (uses ScrollView — this is also its visual regression).
Number keys `1`–`5` switch default/catppuccin/nord/dracula/(one TOML-loaded file theme, proving the
facade path). Help line at the bottom; Ctrl-C quits (Esc is dead, see playbook).

**Acceptance:** one tape per theme (five tapes, shared script with a `Set` for the theme key),
PNGs land in the screenshot pipeline; snapshot test of the default-theme first screen in
`rabbitui/tests/`.

## 4. Screenshot pipeline

**Decision — mechanics:** tapes already produce PNGs; the pipeline is a `just screenshots` target
that runs the tape set and copies/renames the final-frame PNGs into `docs/images/` with stable
names (`gallery-nord.png`, `agent.png`, …), plus a README section embedding the flagship + gallery
images. Do not add image diffing to CI (betamax rendering is not pixel-stable across environments);
the PNGs are human-reviewed acceptance artifacts, committed when they change meaningfully.

**Acceptance:** `just screenshots` from a clean checkout (with betamax installed) refreshes
`docs/images/`; README renders the images; ROADMAP Arc 2A table all ✅.
