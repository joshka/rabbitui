# rabbitui log-follower — comparison exhibit

A **streaming log-follower** with a filter input and a detail modal, built on
[rabbitui]. This is the **rabbitui column** of the eventual four-framework
comparison (`docs/plans/arc5-field.md` item 3) — the same app will be written in
ratatui, Bubble Tea, and Textual so the four can be read side by side. It is also
the second real rabbitui app (beside the flagship `rabbitui-agent`), built to
**dogfood** the framework and surface API rough edges.

## What it does

A simulated log source pushes a new line every ~700ms into a bounded live window.
The lines show in a columnar `Table` (seq / level / target / message) with a
pinned header. You filter the visible lines by typing, Tab between the filter and
the table, and press Enter on a line to open a modal showing its full detail.

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
- **A virtualized table** — the log lines render in a `Table` over a
  `table_rows_with` lazy source on the app's filtered `visible()` slice, so no
  per-frame `Vec<Vec<String>>` is built and only the painted cells are formatted.

### Inline vs. alt-screen

The field report frames "inline/scrollback vs. alt-screen" as a differentiator.
This app is a **browse** app — you scroll a growing list and open modals over it —
so it runs in the default `Mode::AltScreen`, which gives it a stable full-viewport
canvas. The inline/scrollback case (a log _emitter_ committing lines into native
terminal scrollback) is covered by the framework's own `stream` example; a log
_follower_ you filter and inspect is the alt-screen case. This is a design axis,
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
| `↑` / `↓`          | move the table selection (table focused)      |
| `PageUp`/`PageDown`| scroll the table by a page                    |
| `Home` / `End`     | jump to the first / last row                  |
| `Enter`            | open the detail modal for the selected line   |
| `Esc` / `Ctrl-D`   | close the modal                               |
| `Ctrl-P`           | pause / resume the log source                 |
| `q` (list focused) | quit                                          |
| `Ctrl-C`           | quit (works while the filter is focused)      |

> Note: the log source starts **at launch**, from the app's `init` hook — the
> `App` trait's first-class launch entry. This app used to spawn it from an
> `Event::Started` match arm inside `update` (dogfood finding #1); the trait's
> `init` deleted that workaround, so the spawn is now one line with nowhere to
> misfire. Lines flow immediately — no key press needed.

### One-million-row scale demo

The `Table` is virtualized: it asks its source for a cell only when it paints
that cell. To make that visible, a dedicated example points the same widget at a
1,000,000-row synthetic source with **zero app-side caching** — the entire "data
model" is a closure:

```sh
cd comparisons/rabbitui
cargo run --example scale
```

Scroll with ↑/↓, PageUp/PageDown, Home/End, or the wheel; `End` jumps to row
999,999 instantly because nothing between here and there is ever materialized.
The app struct is empty — selection and scroll live in framework-owned widget
state — so this is an honest proof that the app author writes no virtualization
code. (Its runtime feel is the coordinator's betamax pass; the structural O(window)
property is asserted in `rabbitui-widgets`' table tests at 10k and 1M rows.)

Verification is green with:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Visual verification (the real TTY behavior) is the coordinator's, via betamax; a
headless harness cannot drive a real terminal for full e2e, so the automated tests
cover the pure filter/selection/state logic only.

## Files

- `Cargo.toml` — a **standalone crate** (an empty `[workspace]` table detaches it
  from the root workspace, mirroring `conformance/`). Path deps on `rabbitui`,
  `rabbitui-core`, `rabbitui-widgets`.
- `src/main.rs` — the whole app (~690 lines incl. docs and tests): domain types,
  the `LogFollower` state and its `impl App` (`config` / `init` / `global` /
  `update` / `view`), the modal, a small `CloseButton` widget, the `LogSource`
  stream, and unit tests over the filter/`visible` logic. It started as an
  `App::new(state, update, view)` pair of closures; the trait folded those two
  functions plus the launch spawn and the always-on Ctrl-C into one `impl` — the
  `Event::Started` spawn moved to `init`, and the quit chord that used to sit
  hoisted at the top of `update` moved to `global`, where no early `return` can
  strand it. The log lines render in a columnar `Table` (they were a
  `SelectionList` of pre-formatted rows until the Wave B2 `Table` landed); the
  adoption friction is written up in `docs/design/dogfood-findings.md` (findings
  9–11).
- `examples/scale.rs` — the one-million-row `Table` scale demo (above): a minimal
  `impl App` over a `table_from_fn` source, no app-side row storage.

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
