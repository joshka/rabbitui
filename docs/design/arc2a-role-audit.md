# Arc 2A — role-coverage audit and the default theme

A workspace-wide audit (2026-07-07) checking whether app and widget code styles itself through theme
`Role`s or leaks raw color literals, plus an assessment of whether `Theme::default` (the `dark`
preset) needs retuning for legibility. Plan item: `docs/plans/arc2a-aesthetics.md` §1.

## Audit result: role-clean

Swept `rabbitui-core/src`, `rabbitui/src`, `rabbitui/examples`, `rabbitui-widgets/src`, and
`rabbitui-agent/src` for `Color::`, `Style::new().fg/bg(...)`, and hex literals. **No conversions
needed.** Every live-frame `render()`/`view()` resolves colors exclusively via
`RenderCtx::style(Role)` / `.role(Role)`. The only literal colors in running code are the three
documented, legitimate exceptions:

1. **Preset definitions** in `rabbitui-core/src/theme.rs` — the four presets are where concrete
   colors belong.
2. **The inline-commit path** — `rabbitui-agent/src/{transcript,markdown}.rs` and the equivalent
   `commit_lines_for`/`MarkdownRender` in `rabbitui/examples/agent.rs`. These build `CommitLine`s
   written straight to native scrollback and have no `Theme`/`RenderCtx` handle, so concrete styles
   are unavoidable (documented in `transcript.rs`).
3. **The encode layer and the facade TOML theme parser** (`rabbitui/src/{encode,theme}.rs`) — the
   SGR encoder matches every `Color` variant, and the theme-file parser turns `#rrggbb` strings into
   `Color`s. Both are theme _infrastructure_, not widgets bypassing roles.

Everything else flagged was inside `#[cfg(test)]` or a doc-comment example. Conclusion: the role
system is being used as designed; there is no coverage debt to pay down.

## The default theme already meets the constraints

`Theme::dark()` (== `Theme::default()`) was tuned during the 2026-07-07 "never DIM" fix and already
satisfies every constraint the plan set: no role uses DIM; `Muted` is `Ansi(8)` alone; `Accent` is
fg-only (cyan); `Highlight` carries a bg (black on cyan); `Success`/`Warning`/`Danger` are
green/yellow/red. No color change was warranted — retuning would only have churned snapshots.

## The one real finding: `Border == Muted` (intentional)

In `dark`, `Nord`, and `Dracula`, `Border` and `Muted` resolve to the same recessive tone
(`Ansi(8)` in dark; the palette's "comment" color in Nord/Dracula). This is deliberate, now with a
code comment on `dark()`: both roles are meant to recede, a border louder than content reads as
heavy chrome, and the published Nord/Dracula palettes make the identical choice — so distinguishing
them would deviate from the source palettes. If a future design wants rules distinct from muted
text, that is a new decision, not a defect to fix.

## Status

Arc 2A "Theme::default retune + role coverage audit" and "design tokens (spacing)" are complete
(spacing tokens landed as `rabbitui_core::spacing`). Remaining 2A items: the gallery example and the
screenshot pipeline.
