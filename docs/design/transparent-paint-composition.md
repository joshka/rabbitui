# Transparent paint composition (a `None` fg/bg shows the backdrop through)

A rendering-semantics fix (2026-07-07): when a widget paints a cell, a `None` foreground or
background is **transparent** — it falls through to whatever the cell already holds — rather than
resetting the cell to the terminal default. Amends the buffer-writer behavior under ADR 0003.

## The bug it fixes

`Buffer::set_stringn` replaced each written cell outright (`cells[i] = Cell::new(grapheme, style)`).
A container like `Panel` fills its area with the `Surface` background, then declares content into its
inner rect; a `Text` widget paints graphemes whose resolved style is foreground-only (most roles
carry no bg). Under replace semantics, each text cell's background became `None` (the terminal
default), punching a hole through the panel's surface fill — while the untouched cells (the gaps
between and after words) kept the fill. The result was a ragged per-line background mismatch: text
sat on the default background, the block around it on the surface color.

It was invisible in the `dark` preset (where `Surface` is `bg(Reset)` — the same terminal default),
and obvious in the truecolor presets (`catppuccin`, `nord`, `dracula`), where `Surface` carries a
real background. The widget gallery surfaced it under those themes.

## The rule

`paint_over(top, under)` composes a paint over the existing cell:

- **foreground / background:** `top`'s value wins if `Some`; otherwise `under`'s shows through
  (transparency). So a fg-only `Text` keeps the container's background fill.
- **attributes:** the painted character's own — they replace, they do **not** union with the
  backdrop's (plain text over a bold fill is not bold).

This is the composable model (and what ratatui does with its patch-style cell writes). It is
strictly a superset of the old behavior: on a fresh, default-cleared cell — which is where nearly
every buffer unit test writes — `paint_over` yields exactly `top`, so painting onto a cleared buffer
is unchanged. Only _layered_ rendering (a fill, then transparent content on top) differs, and that
difference is the fix.

## Why it's safe

The back buffer is cleared to default at the start of every frame, so within a frame the composition
is deterministic (container fills, then content composes over the fill), and the double-buffer diff
compares final cells as before. The full suite stayed green (419 passing; the only failures are the
unrelated, documented qwertty mouse-decode drift). Regression tests in `buffer.rs` pin all three
rules: background shows through `None`, an explicit background overrides, and attributes replace.

## Consequence for widget authors

You no longer reset a background to the terminal default by painting `None` over a filled cell —
paint `Color::Reset` explicitly if you truly want the terminal default. In practice this is what you
want: fg-only text composes onto whatever surface it is placed on, so theming a container's `Surface`
background now actually reaches the text drawn inside it.
