# Slice 5 design: inline mode + the vt100 escape-level harness

Working design note for slice 5 (ROADMAP.md), implementing ADR 0013 (screen modes) and the third
testing layer of ADR 0009. The tui2 lessons govern: terminal-native scrollback, store source and let
the terminal wrap committed content, never pre-wrap history.

## The inline invariant (ADR 0013 made concrete)

Inline mode = a **bounded live tail** at the bottom of the primary screen plus an **append-once
commit channel** into native scrollback above it.

- **Committed lines are emitted unwrapped** — one logical line per line, styled, terminated `\r\n`,
  written _without_ cursor addressing so the terminal soft-wraps them and therefore owns their
  reflow on resize (the tui2 retirement lesson: the terminal keeps scrollback, selection, copy, and
  rewrap of everything committed). A committed line is immutable: committed exactly once, never
  repainted, never addressed again.
- **The live tail** is rendered from the declared frame's buffer, bounded by
  `min(content_height, max_height, viewport_height)`. It is repainted in place; its height may grow
  and shrink frame to frame.
- **Region mechanics v1** (simple, correct-first): the renderer tracks the live region's current
  height H. To render: cursor to region top (cursor-up H-1 from bottom anchor… no absolute rows —
  the region floats), clear from cursor down (ED), emit any pending commits (region content scrolls
  into history naturally), then paint the new live tail below the last commit. All wrapped in
  mode-2026 framing. No DECSTBM scroll regions in v1 — ED + repaint is simpler and correct;
  scroll-region optimization is a recorded later step. Cell-level diffing applies _within_ a
  stable-height live tail; any height change or commit flush forces a full tail repaint.
- **Resize in inline mode**: committed history is the terminal's problem (it reflows natively — the
  entire point). The live tail re-layouts to the new width and fully repaints; the tracked height is
  clamped to the new viewport. Known artifact, documented: a resize that rewraps the _live_ region's
  old cells may leave one stray line in some emulators; conservative ED-down on resize bounds it.

## API

- `Mode::{AltScreen, Inline { max_height: u16 }}` (core type). `App::mode(Mode)` on the builder;
  default stays `AltScreen`.
- **Committing**: `Update::commit(line: impl Into<CommitLine>)` — commits are an _update-time_
  action (event-driven, naturally once), never a view-time one (views re-run every frame; committing
  there would double-emit). `CommitLine` v1 = one `String` + one `Style` (`From<&str>` provided).
  Multi-span lines are deliberately deferred to the transcript work (slice 8 flagship) — recorded as
  a known ceiling, not an oversight.
- **Mode switching at runtime**: `Update::set_mode(Mode)`, buffered, applied between frames by the
  runtime (enter/leave alt screen with correct ordering relative to pending commits: commits flush
  _before_ entering alt, so nothing is lost behind the alternate screen).

## Architecture: pure byte-producing engines

The renderer split so escape-level behavior is testable without a tty (the qwertty in-memory-device
seam does not exist yet — this is our answer until it does): a `mode` engine
(`InlineEngine`/`AltEngine`) is a **pure function from (previous state, commits, frame buffer/diff)
to bytes** plus next state. `Terminal` only writes bytes. The engines live in the facade
(`rabbitui/src/engine/`), unit-tested by feeding their output to vt100.

## The vt100 harness (ADR 0009 layer 3)

`rabbitui-testing` gains a `vt` module with a real `vt100` crate dependency (regular dep of the
testing crate; it never enters core/widgets/facade):

- `VtScreen::new(cols, rows)`, `feed(bytes)`, `assert_row(y, expected)`, `row_text(y)`, `cursor()`,
  `contents()` — thin, honest wrappers over `vt100::Parser`, with trimmed-row helpers matching
  `assert_buffer_lines` conventions.
- Because engines are pure, tests compose: engine bytes → VtScreen → assert the _screen a terminal
  would show_. This is the layer that catches what buffer equality cannot (tui2/textual-rs finding):
  framing, clears, cursor discipline, commit/tail interleaving.

Required escape-level tests: alt-screen frame renders to expected grid; diff-only second frame
changes exactly the changed cells; every frame wrapped in 2026 framing (assert the bytes, vt100
ignores the mode); inline: commit then tail repaint yields commit above tail; two commits in one
update stay in order; tail shrink leaves no orphan rows (ED verified); resize repaints tail at new
width; mode switch alt→inline restores and repaints; commits issued just before alt-screen entry
appear in scrollback (feed and inspect `contents_between`/scrollback if vt100 exposes it — otherwise
assert emission order in bytes, documented).

## examples/stream.rs

A fake streaming transcript: a timer-free demo (keypress-driven v1 — real timers are slice 6): each
press of `n` commits a numbered "log line" and the live tail shows a TextInput + a status line; `m`
toggles AltScreen/Inline live; `q` quits. Demonstrates: scrollback accumulates natively (scroll up
in your terminal!), live tail stays bounded, mode switching works.

## Implementation deltas

Deviations and clarifications recorded during the slice-5 build; the design above is otherwise
implemented as written.

- **Live-tail height is realized by buffer sizing, not a separate `content_height` measurement.**
  The runtime sizes the inline back buffer to `min(max_height, viewport_height)` rows at full width,
  and the engine renders the _whole_ buffer as the tail. The `min(content_height, …)` bound is
  honored by the app declaring a frame it sizes; a dedicated intrinsic-height measurement is
  deferred (layout's `desired_height` is not in the slice-5 scope). Growth and shrink still work:
  the buffer re-sizes on max-height or viewport change, and a height change forces a full tail
  repaint.

- **Relative cursor addressing in the inline diff path.** Because the live region floats (no
  absolute rows), the stable-height cell-diff path addresses columns with `CR` + `CSI n C`
  (cursor-right) and rows with `CSI n B` (cursor-down), climbing to the region top with `CSI n A`
  (cursor-up). These plus `CSI 0 J` (erase-below) were added to `rabbitui/src/encode.rs`. Full
  repaints paint each row from column 1 with `\r\n` between rows (no trailing `\n`, so the region
  never self-scrolls). The engine leaves the cursor at column 1 of the tail's bottom row as its
  frame invariant, which the next frame's cursor-up arithmetic depends on.

- **`Update` gained a buffered-effects sink.** `Update::commit` and `Update::set_mode` record into a
  `RefCell<Pending>` the runtime drains between frames, so `Update::new` is now three-argument
  (event, outcomes, pending). Test and doc call sites pass `&RefCell::new(Default::default())`.

- **`Terminal` only writes bytes.** Alt-screen enter/leave moved out of `Terminal::open`/`close`
  into the engines (`AltEngine::enter`/`leave`); `Terminal::open` now only enters raw mode, and the
  first loop iteration writes the active engine's mode-entry bytes. `Terminal::write_bytes` is
  public (the engine-driven write path; `smoke.rs` uses it to enter/leave the alt screen). The
  panic/drop `RESTORE` still leaves the alt screen **unconditionally**, and `Terminal::close` keeps
  an unconditional leave-alt-screen backstop (a no-op when inline), so the restore guarantee is
  intact regardless of mode.

- **The "commits before alt entry" escape-level test asserts byte emission order, not post-entry
  scrollback.** vt100 faithfully models the alternate screen _hiding_ the primary screen's
  scrollback — so once the alt screen is entered, `all_lines()` cannot see a line committed to
  primary scrollback (correct emulator behavior, the whole point of the alt screen). The test
  therefore (a) asserts the commit lands in primary scrollback _before_ the alt entry, and (b)
  asserts the commit bytes precede the `CSI ? 1049 h` alt-entry byte in the stream — the design
  note's documented fallback. This is a property of what the alt screen _is_, not an
  inline-invariant gap.

- **vt100 finding: `all_lines` must walk scrollback, not just max the offset.** vt100's
  `rows()`/`contents()` report a single viewport-height window at the current scrollback offset.
  Reading committed history deeper than the screen height requires stepping the offset from the top
  down and collecting the row that scrolls off at each step (implemented in `VtScreen::all_lines`).
  This is the one non-obvious wrapper over the parser; everything else is a thin passthrough. No
  inline-invariant bug surfaced — the region mechanics (ED-down + repaint, unwrapped `\r\n` commits,
  bottom-row cursor anchor) produced the expected screen in every escape-level test, including tail
  shrink (no orphan rows), stable-tail cell diffs (no erase-below, unchanged rows untouched), and
  resize repaint at a new width.
