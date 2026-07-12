# Terminal conformance harness

A public conformance harness for the **two-region inline-mode rendering
discipline** specified in [`../docs/inline-mode-spec.md`](../docs/inline-mode-spec.md).
It answers one question, honestly and per-terminal: _does a renderer's emitted
byte stream produce the observable behavior the inline-mode contract requires?_

The corpus doubles as the evidence base for the terminal-gap analysis — the
same fixtures that a renderer is checked against are the minimal repros a
terminal bug is filed with.

> Naming note: this directory is deliberately kept internal-safe. It names no
> products. Whoever publishes it sets the conformance bar, so it is not published
> from here — the coordinator/author decides that. See
> [`../docs/plans/arc5-field.md`](../docs/plans/arc5-field.md) item 1.

## The two layers

Conformance is judged on two channels because neither alone is sufficient, and a
buffer-equality snapshot sees neither:

1. **Headless, escape-level (this crate).** Each corpus case carries a raw byte
   sequence and a declarative expectation. The runner feeds the bytes to a real
   [`vt100`](https://crates.io/crates/vt100) terminal model and checks the
   expectation against the emulated **screen** (the grid a terminal would show,
   including scrollback) and/or the **byte stream** (ordering, framing, cursor
   and erase discipline the screen cannot reveal). Fast, deterministic,
   CI-friendly — this is what `cargo test` runs. It is the same emulator model
   the in-repo escape-level harness uses (`rabbitui-testing`'s `VtScreen`, ADR
   0009 layer 3); this crate grows from it in spirit and is kept self-contained.

2. **Visual, on real hardware (author-driven, out of scope for this crate).**
   The _same_ corpus bytes are driven through real terminal emulators under
   [betamax](https://crates.io/crates/betamax) tapes, and a human reviews the
   result. This is the only channel that can judge behaviors an emulator model
   abstracts away — e.g. the accepted one-stray-line artifact a floating region
   can leave across a width change (spec §5), or a renderer's tail-height
   clamping decision.

A case that can only be judged on channel 2 is marked
`Expectation::VisualOnly { reason }`. The headless runner **never silently
passes** such a case — it reports it as an explicit skip (`na` in the matrix),
carrying the reason. That distinction is the point: `na` is not `pass`.

## Running the headless suite

```sh
cd conformance
cargo test
```

The suite passes when every corpus case resolves to either a **pass** or an
**explicit visual-only skip** (printed as `SKIP (visual-only) <ID>: <reason>`). A
hard failure fails the suite. Three tests run:

- `every_case_passes_or_is_an_explicit_visual_skip` — the green gate.
- `corpus_covers_every_spec_identifier` — every spec §10.1 id is present, once.
- `visual_only_cases_carry_a_reason` — no silent visual skips.

## Generating the results matrix

```sh
cd conformance
cargo run --example gen_matrix > results-matrix.md
```

This computes the `headless (vt100)` column live from the corpus and emits one
**blank** column per real terminal for the author to fill in later. A checked-in
snapshot lives at [`results-matrix.md`](./results-matrix.md).

### Matrix format

The honest unit of a conformance claim is _(corpus-id × terminal) → status_
(spec §10.2): several identifiers can pass on one emulator and fail on another,
so a single boolean would lie. The matrix is a markdown table, one row per
corpus id, one column per terminal, plus an `Asserts on` column echoing the
spec's channel:

| value  | meaning                                                                  |
| ------ | ------------------------------------------------------------------------ |
| `pass` | the behavior was observed on that terminal                               |
| `fail` | the behavior was expected but not observed (a footnote says how)         |
| `na`   | not decidable on this channel (e.g. a visual-only case in the headless column) — never conflated with `pass` |
| blank  | not yet run on that terminal — the author fills it in                    |

Every non-`pass` headless cell gets a footnote explaining itself, so the matrix
is a self-contained artifact.

## Adding a terminal later (betamax, author-driven)

The headless column is generated; real-terminal columns are filled in by the
author, tape by tape. The workflow is:

1. Add the terminal's name to `REAL_TERMINALS` in
   [`examples/gen_matrix.rs`](./examples/gen_matrix.rs) (the plan's likely set is
   Ghostty, kitty, Terminal.app — confirm the local set with the author) and
   regenerate the matrix to get its blank column.
2. For each corpus id, drive the case's **same** `input` bytes
   (`corpus::corpus()` exposes them) through the terminal under betamax, record
   the tape, and review it against the expectation the id stands for (spec §9 /
   §10.1). The corpus is designed so the bytes a headless case feeds are exactly
   the bytes a betamax tape drives — one fixture, two channels.
3. Fill the terminal's column cell-by-cell with `pass` / `fail` / `na`. The
   accepted width-change artifact (spec §5) is expected to surface as an
   emulator-specific soft `fail` of `INLINE-WRAP-ON-RESIZE` on some terminals and
   is documented as an inherent limit, not a defect to fix application-side.

The betamax runner itself is a separate, author-driven deliverable; this crate
provides the corpus and the matrix shape it plugs into.

## Corpus coverage

Every identifier the spec §10.1 fixes is covered. `id → what it asserts →
channel`:

| Corpus ID                    | Channel      | Headless status                        |
| ---------------------------- | ------------ | -------------------------------------- |
| `INLINE-APPEND-ONCE`         | screen+bytes | checked (commit-once, above tail)      |
| `INLINE-COMMIT-ORDER`        | screen       | checked                                |
| `INLINE-WRAP-ON-RESIZE`      | screen+bytes | unwrapped-emit + reflow checked; resize repaint is visual |
| `INLINE-COMMIT-STYLED`       | screen+bytes | checked (per-span color)               |
| `INLINE-TAIL-BOUNDED`        | screen       | **visual-only** (renderer clamp decision) |
| `INLINE-TAIL-SHRINK`         | screen+bytes | checked (no orphan rows)               |
| `INLINE-TAIL-CELL-DIFF`      | bytes        | checked (no erase-below on stable diff) |
| `INLINE-NOOP-SILENT`         | bytes        | checked (empty frame)                  |
| `INLINE-RELATIVE-CURSOR`     | bytes        | checked (bottom-row cursor invariant)  |
| `MODE2026-FRAMING`           | bytes        | checked (sync begin/end brackets)      |
| `INLINE-ALTSCREEN-FLUSH`     | bytes        | checked (commit precedes alt-enter)    |
| `BCE-RESET`                  | screen+bytes | checked (reset immediately precedes erase) |
| `WIDE-GRAPHEME-CONTINUATION` | screen       | checked (width agreement / cursor advance) |

`INLINE-TAIL-BOUNDED` is the one identifier a data-only fixture cannot decide:
boundedness is a property of the _renderer's_ clamping decision when handed
over-tall content, not of a fixed byte sequence. It is carried as an explicit
visual-only skip; the in-repo `rabbitui/tests/inline_vt.rs` layer-3 tests
exercise that decision against the real `InlineEngine`.

## Crate layout

- `src/corpus.rs` — the corpus as data: `Case`, `Expectation`, `Channel`, and the
  `corpus()` builder. The stable IDs and the escape/byte fixtures live here.
- `src/escape.rs` — named escape sequences the fixtures are built from
  (sync framing, alt-enter, SGR reset, erase-to-end).
- `src/model.rs` — the `vt100` wrapper (screen, scrollback, cursor, cells).
- `src/runner.rs` — executes a case → `Outcome` (`Pass` / `Fail(msg)` /
  `Skipped(reason)`).
- `src/matrix.rs` — the results-matrix markdown format and its generator.
- `tests/corpus.rs` — the `cargo test` green gate.
- `examples/gen_matrix.rs` — emits the matrix; `results-matrix.md` is a snapshot.

## Wiring into the workspace

This crate is currently **standalone**: its `Cargo.toml` has an empty
`[workspace]` table so it detaches from the root workspace and builds on its own
(`cd conformance && cargo test`), without the root manifest being edited while
other workstreams churn it. To fold it into the root workspace:

1. Delete the `[workspace]` table from `conformance/Cargo.toml`.
2. Add `"conformance"` to the `members` array in the root `Cargo.toml`.
3. Optionally switch the `vt100` dependency to a `workspace = true` form once the
   root declares it as a workspace dependency (it is currently a plain `"0.16"`,
   matching `rabbitui-testing`).
