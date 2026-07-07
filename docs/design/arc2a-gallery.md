# Arc 2A — the widget gallery and screenshot pipeline

The last two Arc 2A items: a gallery example that shows the whole widget catalog and every theme
role on one screen, and a `just` pipeline that renders it under each theme into committed images.
Plan: `docs/plans/arc2a-aesthetics.md` §3–4.

## What landed

- **`rabbitui/examples/gallery.rs`** — a titled panel wrapping a virtualized `ScrollView` column of
  every catalog widget: `Text` (plain, muted, accent, styled spans, wrapped), `Button` ×2,
  `TextInput` (placeholder), `SelectionList`, `Collapsible` (expanded + collapsed), and a swatch per
  `Role`. Layout uses the `rabbitui_core::spacing` tokens. It is both a style guide and the
  `ScrollView`'s own visual regression (it scrolls its own showcase).
- **Four theme tapes** (`tapes/gallery-{dark,catppuccin,nord,dracula}.tape`) — each launches the
  gallery under one preset and captures a top screenshot and a scrolled "roles" screenshot.
- **`just screenshots`** — validates and runs the gallery tapes, then copies the final-frame PNGs
  into `docs/images/` under stable names. The PNGs are human-reviewed acceptance artifacts committed
  when they change meaningfully; there is deliberately **no** pixel-diffing in CI (betamax rendering
  is not pixel-stable across hosts).
- **README** gains a Gallery section embedding the dark screenshot, plus a refreshed crate table and
  run line.

## Key decision: theme chosen at startup, not switched at runtime

The plan wanted number keys to switch themes live. The runtime has no `Update::set_theme` — the
active theme is a run-loop local fed by the builder and the theme-file watcher, with no per-event
switch path. Rather than add a core-runtime API unattended, the gallery reads `GALLERY_THEME` at
startup and picks the preset via `App::theme(...)`; the four tapes vary it. This gives the same
every-widget × every-theme regression without a framework change, and cleanly surfaces the gap:

> **Deferred framework item (Arc 4):** `Update::set_theme(Theme)`, buffered and applied before the
> next paint exactly like `Update::set_mode`, would let an app offer a live theme picker. It is a
> small, well-shaped addition (mirrors `set_mode`), but it touches the core runtime + `Pending` +
> `TestApp`, so it belongs with the keybinding/config work, not a gallery example. Recorded here so
> the next session picks it up deliberately.

## What this revealed

- **The role system holds up visually.** Rendered under all four presets, every role reads
  distinctly and legibly — muted text is gray, not DIM; the selected list row and accent text take
  the accent hue; success/warning are green/yellow; the collapsible markers and panel border are
  quiet. The role-swatch row makes a theme's palette legible at a glance and is the single best
  visual-regression surface (see `docs/images/gallery-*-roles.png`).
- **Surface bg reads correctly per preset.** In `dark`, `Surface` is `bg(Reset)` so the panel fill
  matches the terminal; in the truecolor presets `Surface` carries a bg, so the panel reads as a
  distinct raised surface. Both are intentional and both look right.
- **Verified end-to-end, not just compiled.** All four tapes build the example, launch it, match the
  panel-title sentinel, scroll, screenshot, and quit on `q` — so the gallery is exercised as a real
  running program, not just a snapshot. (An in-process snapshot test would require extracting the
  example's `view` into an importable module; the tapes are the acceptance here, consistent with the
  other nine examples.)

## Arc 2A status: complete

All four remaining items are done — role audit (clean), spacing tokens, gallery, screenshot
pipeline. The one carried-forward item is the deferred `Update::set_theme` framework addition above.
