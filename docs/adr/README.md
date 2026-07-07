# Architecture Decision Records

One ADR per decision: context, options with steelmen, decision, honest consequences, and
concrete revisit triggers. Decisions change by supersession, never silent edits.
`DESIGN.md` at the workspace root is the narrative summary.

| ADR | Decision |
|---|---|
| [0001](0001-programming-model.md) | Declared-frame architecture as the core programming model |
| [0002](0002-widget-identity.md) | Framework-owned stable widget identity and per-ID state store |
| [0003](0003-rendering.md) | Cell buffer, z-ordered layers, double-buffer diff, mode-2026 framing |
| [0004](0004-layout.md) | Intrinsic-measurement constraint/flex layout; no solver, no flexbox in core |
| [0005](0005-concurrency-event-loop.md) | Async-first framework-owned event loop with a synchronous core |
| [0006](0006-input-focus-events.md) | Capture/target/bubble routing over frame facts; ID-keyed focus |
| [0007](0007-styling-theming.md) | Typed styles + semantic theme tokens; presets in v0.1; no cascade engine yet |
| [0008](0008-widget-contract.md) | One widget contract in core: specs, rich render output, outcomes, virtualization |
| [0009](0009-testing.md) | Headless driver + buffer snapshots + vt100 escape-level harness, shipped early |
| [0010](0010-ratatui-interop.md) | Ratatui interop as a soft goal via a buffer bridge crate |
| [0011](0011-crate-layout.md) | Workspace layout, edition 2024, MSRV stable-minus-one (carries a dissent) |
| [0012](0012-terminal-substrate.md) | qwertty as substrate behind a one-file seam, with an interim encoder |
| [0013](0013-screen-modes.md) | Inline and alt-screen as peer modes; terminal-native scrollback by default |
| [0014](0014-positioning.md) | Build standalone; defer the ratatui-* shipping decision to ~0.1 |
