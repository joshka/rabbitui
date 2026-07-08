# rabbitui log-follower — comparison exhibit

A **streaming log-follower** with a filter input and a detail modal, built on
[rabbitui]. This is the **rabbitui column** of the eventual four-framework
comparison (`docs/plans/arc5-field.md` item 3) — the same app will be written in
ratatui, Bubble Tea, and Textual so the four can be read side by side. It is also
the second real rabbitui app (beside the flagship `rabbitui-agent`), built to
**dogfood** the framework and surface API rough edges.

## What it does

A simulated log source pushes a new line every ~700ms into a bounded live window.
You filter the visible lines by typing, Tab between the filter and the list, and
press Enter on a line to open a modal showing its full detail.

It deliberately exercises the four things the field report says differentiate TUI
frameworks:

- **Streaming** — a `Cmd::stream` timer (the flagship's spinner pattern) emits
  entries over time.
- **A filter `TextInput`** — its `Changed` outcome updates the filter; the
  visible list is recomputed each frame (case-insensitive substring over level,
  target, and message).
- **Focus** — Tab / Shift-Tab cycle focus between the filter and the list (the
  runtime drives this on unconsumed Tab); the focused region's panel highlights.
- **A detail modal** — Enter (or a click) opens a `Frame::layer` modal with a
  focusable Close button; Esc or Ctrl-D closes it.

### Inline vs. alt-screen

The field report frames "inline/scrollback vs. alt-screen" as a differentiator.
This app is a **browse** app — you scroll a growing list and open modals over it —
so it runs in the default `Mode::AltScreen`, which gives it a stable full-viewport
canvas. The inline/scrollback case (a log *emitter* committing lines into native
terminal scrollback) is covered by the framework's own `stream` example; a log
*follower* you filter and inspect is the alt-screen case. This is a design axis,
answered deliberately, not a gap.

## Running it

```sh
cd comparisons/rabbitui
cargo run
```

Controls:

| Key                | Action                                        |
| ------------------ | --------------------------------------------- |
| `Tab` / `Shift-Tab`| move focus between the filter and the list    |
| type (filter)      | narrow the visible lines                      |
| `↑` / `↓`          | move the list selection (list focused)        |
| `Enter`            | open the detail modal for the selected line   |
| `Esc` / `Ctrl-D`   | close the modal                               |
| `Ctrl-P`           | pause / resume the log source                 |
| `q` (list focused) | quit                                          |
| `Ctrl-C`           | quit (works while the filter is focused)      |

> Note: the log source starts **at launch**, via the one-shot `Event::Started`
> hook the framework grew after this app first reported the gap (dogfood finding
> #1). Lines flow immediately — no key press needed.

Verification is green with:

```sh
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

Visual verification (the real TTY behavior) is the coordinator's, via betamax; a
headless harness cannot drive a real terminal for full e2e, so the automated tests
cover the pure filter/selection/state logic only.

## Files

- `Cargo.toml` — a **standalone crate** (an empty `[workspace]` table detaches it
  from the root workspace, mirroring `conformance/`). Path deps on `rabbitui`,
  `rabbitui-core`, `rabbitui-widgets`.
- `src/main.rs` — the whole app (~430 lines incl. docs and tests): domain types,
  the `App_` state, `update`, `view`, the modal, a small `CloseButton` widget, the
  `LogSource` stream, and unit tests over the filter/`visible` logic.

## Wiring into the workspace later

Kept standalone on purpose so it builds and tests on its own while the root
manifest churns under other workstreams. To fold it in:

1. delete the empty `[workspace]` table from `comparisons/rabbitui/Cargo.toml`;
2. add `"comparisons/rabbitui"` to the root `Cargo.toml`'s `workspace.members`.

The path deps already point at the workspace crates, so no dependency edits are
needed.

## Other frameworks (future work)

The ratatui, Bubble Tea (Go), and Textual (Python) implementations of this same
log-follower are future work — they become the other three columns of the
comparison. This directory only holds the rabbitui column today.

[rabbitui]: ../../rabbitui
