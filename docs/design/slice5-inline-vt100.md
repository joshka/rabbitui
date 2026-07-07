# Slice 5 design: inline mode + the vt100 escape-level harness

Working design note for slice 5 (ROADMAP.md), implementing ADR 0013 (screen
modes) and the third testing layer of ADR 0009. The tui2 lessons govern:
terminal-native scrollback, store source and let the terminal wrap committed
content, never pre-wrap history.

## The inline invariant (ADR 0013 made concrete)

Inline mode = a **bounded live tail** at the bottom of the primary screen plus
an **append-once commit channel** into native scrollback above it.

- **Committed lines are emitted unwrapped** — one logical line per line,
  styled, terminated `\r\n`, written *without* cursor addressing so the
  terminal soft-wraps them and therefore owns their reflow on resize (the tui2
  retirement lesson: the terminal keeps scrollback, selection, copy, and
  rewrap of everything committed). A committed line is immutable: committed
  exactly once, never repainted, never addressed again.
- **The live tail** is rendered from the declared frame's buffer, bounded by
  `min(content_height, max_height, viewport_height)`. It is repainted in
  place; its height may grow and shrink frame to frame.
- **Region mechanics v1** (simple, correct-first): the renderer tracks the
  live region's current height H. To render: cursor to region top (cursor-up
  H-1 from bottom anchor… no absolute rows — the region floats), clear from
  cursor down (ED), emit any pending commits (region content scrolls into
  history naturally), then paint the new live tail below the last commit.
  All wrapped in mode-2026 framing. No DECSTBM scroll regions in v1 — ED +
  repaint is simpler and correct; scroll-region optimization is a recorded
  later step. Cell-level diffing applies *within* a stable-height live tail;
  any height change or commit flush forces a full tail repaint.
- **Resize in inline mode**: committed history is the terminal's problem (it
  reflows natively — the entire point). The live tail re-layouts to the new
  width and fully repaints; the tracked height is clamped to the new viewport.
  Known artifact, documented: a resize that rewraps the *live* region's old
  cells may leave one stray line in some emulators; conservative ED-down on
  resize bounds it.

## API

- `Mode::{AltScreen, Inline { max_height: u16 }}` (core type). `App::mode(Mode)`
  on the builder; default stays `AltScreen`.
- **Committing**: `Update::commit(line: impl Into<CommitLine>)` — commits are
  an *update-time* action (event-driven, naturally once), never a view-time
  one (views re-run every frame; committing there would double-emit).
  `CommitLine` v1 = one `String` + one `Style` (`From<&str>` provided).
  Multi-span lines are deliberately deferred to the transcript work (slice 8
  flagship) — recorded as a known ceiling, not an oversight.
- **Mode switching at runtime**: `Update::set_mode(Mode)`, buffered, applied
  between frames by the runtime (enter/leave alt screen with correct ordering
  relative to pending commits: commits flush *before* entering alt, so
  nothing is lost behind the alternate screen).

## Architecture: pure byte-producing engines

The renderer split so escape-level behavior is testable without a tty (the
qwertty in-memory-device seam does not exist yet — this is our answer until
it does): a `mode` engine (`InlineEngine`/`AltEngine`) is a **pure function
from (previous state, commits, frame buffer/diff) to bytes** plus next state.
`Terminal` only writes bytes. The engines live in the facade
(`rabbitui/src/engine/`), unit-tested by feeding their output to vt100.

## The vt100 harness (ADR 0009 layer 3)

`rabbitui-testing` gains a `vt` module with a real `vt100` crate dependency
(regular dep of the testing crate; it never enters core/widgets/facade):

- `VtScreen::new(cols, rows)`, `feed(bytes)`, `assert_row(y, expected)`,
  `row_text(y)`, `cursor()`, `contents()` — thin, honest wrappers over
  `vt100::Parser`, with trimmed-row helpers matching `assert_buffer_lines`
  conventions.
- Because engines are pure, tests compose: engine bytes → VtScreen → assert
  the *screen a terminal would show*. This is the layer that catches what
  buffer equality cannot (tui2/textual-rs finding): framing, clears,
  cursor discipline, commit/tail interleaving.

Required escape-level tests: alt-screen frame renders to expected grid;
diff-only second frame changes exactly the changed cells; every frame wrapped
in 2026 framing (assert the bytes, vt100 ignores the mode); inline: commit
then tail repaint yields commit above tail; two commits in one update stay in
order; tail shrink leaves no orphan rows (ED verified); resize repaints tail
at new width; mode switch alt→inline restores and repaints; commits issued
just before alt-screen entry appear in scrollback (feed and inspect
`contents_between`/scrollback if vt100 exposes it — otherwise assert emission
order in bytes, documented).

## examples/stream.rs

A fake streaming transcript: a timer-free demo (keypress-driven v1 — real
timers are slice 6): each press of `n` commits a numbered "log line" and the
live tail shows a TextInput + a status line; `m` toggles AltScreen/Inline
live; `q` quits. Demonstrates: scrollback accumulates natively (scroll up in
your terminal!), live tail stays bounded, mode switching works.
