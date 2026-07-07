# ADR 0009: Testing

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

A TUI framework's credibility is its interaction correctness, and interaction correctness is
only provable by driving the app and inspecting output. Three forces converge on shipping a
first-class test kit *early*, before the widget catalog grows:

- **Every mature TUI framework that succeeded shipped a headless driver, and the ones that
  are pleasant to test converged on the same API.** Bubble Tea's `teatest` gives
  `NewTestModel(m, WithInitialTermSize(x,y))`, `Send`/`Type`, `WaitFor(output-condition)`,
  `FinalModel`, and `RequireEqualOutput` golden files with a `-update` flag
  (`docs/research/bubbletea.md`; pkg.go.dev teatest). Textual ships `app.run_test()` returning
  a `Pilot` (`press`, `click("#selector")`, `pause()` to drain the queue) plus
  `pytest-textual-snapshot` with an SVG diff report (`docs/research/textual.md`). Cursive had a
  `puppet` backend exposing an `ObservedScreen` stream and a scripted input channel *years*
  before most frameworks had any testing story (`docs/research/cursive.md`,
  `cursive/src/backends/puppet/mod.rs`). Three independent lineages, one shape: inject events,
  advance the loop, assert on rendered output, snapshot with an update flag.

- **Buffer-equality snapshots are necessary but demonstrably insufficient — the bugs that
  matter live below the buffer, at the escape-sequence layer.** Codex's tui2 built a test
  backend wrapping `CrosstermBackend<vt100::Parser>` so tests drive the *actual* emitted escape
  sequences through a real terminal emulator and assert on the resulting screen grid
  (`docs/research/codex-tui2.md`; `tui2/src/test_backend.rs`, `tests/suite/vt100_history.rs`).
  This is strictly stronger than ratatui's `TestBackend` buffer comparison, which validates the
  *intended* buffer, not the ANSI *emission* — and inline mode, resize/reflow, and
  suspend/resume are exactly the paths whose bugs only appear at the escape level (scroll
  regions, clears, cursor moves). The recent-rust-tui wave replicated the finding independently:
  textual-rs "documented the key failure: headless render tests passed while the live app was
  broken — buffer snapshots alone are insufficient" (`docs/research/recent-rust-tui-wave.md`).
  The bar for this workload is now PTY-level.

- **The test harness is a trust and adoption mechanism, doubly so for AI-authored code and
  coding agents.** A third of the 2025–26 TUI wave is AI-generated, and the disciplined tail
  invented verification tooling precisely to make AI-scale development trustworthy: textual-rs's
  real-PTY cell-grid parity harness, inkferro's byte-golden + differential fuzz against a live
  Node oracle, testty's "PTY e2e semantic assertions… so agents can verify TUIs they can't see"
  (`docs/research/recent-rust-tui-wave.md`). Agents are a large share of future TUI authors and
  they cannot *see* a terminal; a scriptable, assertable harness is how they close the loop.
  This makes the harness public API, not an internal test crate.

- **Owning the loop is what makes deterministic testing possible at all.** ratatui cannot ship
  a real headless driver because it does not own the event loop (`docs/research/ratatui.md`);
  rabbitui does (ADR 0005), so `update → layout → render → diff → write` can be single-stepped.
  Determinism additionally requires that *time* be injectable — Textual's suite is sensitive to
  event-loop-per-test overhead and does nothing to protect you (`docs/research/textual.md`,
  `#5068`: 16 chars took 15s), and any test that `sleep`s to wait for a timer is flaky by
  construction.

## Options considered

### A. Buffer-snapshot testing only (the ratatui baseline)

*What it is.* Ship `TestBackend` + `Buffer::with_lines` equality assertions; widgets are tested
by rendering into a `Buffer` and comparing lines (`docs/research/ratatui.md`,
`ratatui-core/src/backend/test.rs`).

*Steelman.* It is done, battle-tested across thousands of apps, trivial to build, and readable.
Most widget logic (what glyphs land in which cells under which state) is fully covered by it.

*Why not chosen.* It validates the intended buffer, never the emitted bytes. It cannot catch the
inline-mode/scroll-region/synchronized-output bugs that tui2 and textual-rs both proved are the
ones that break live apps while snapshots pass. It is a floor, not a ceiling — we keep it as the
base of the pyramid, not the whole pyramid.

### B. Headless driver + buffer snapshots, no escape-level layer

*What it is.* Ship the teatest/Pilot/puppet-style driver — inject events, drain the queue,
`wait_for(|buf| ...)`, snapshot the buffer with an update flag — and stop there.

*Steelman.* Covers the vast majority of interaction tests (focus traversal, outcome routing,
key handling) at low cost, with proven ergonomics and near-zero design risk to copy
(`docs/research/bubbletea.md`). It is what most of the wave shipped.

*Why not chosen as the whole story.* This is exactly the configuration textual-rs shipped when
its headless tests passed on a broken live app. It cannot see the ANSI layer. We adopt this
driver — it is the majority of the value — but pair it with the escape-level harness below.

### C. End-to-end PTY testing only (spawn a real terminal, assert on the screen)

*What it is.* Drive the app through an actual PTY (à la testty / Betamax tapes), assert on
semantic screen content.

*Steelman.* Maximum fidelity — it exercises the real terminal stack including the OS PTY layer;
testty exists because agents need this (`docs/research/recent-rust-tui-wave.md`).

*Why not chosen as the *primary* layer.* Full PTY spawning is slow, harder to make deterministic
(real clocks, real terminal timing), and OS-dependent. tui2's insight is that you get 95% of the
escape-level fidelity by running the app's *own* output through an in-process `vt100::Parser`
without spawning anything — deterministic, fast, and CI-portable. We take the vt100-parser
backend as the primary escape-level layer; a true out-of-process PTY recorder (VHS/tape-style)
is a valuable *later* addition for GIF regression and full-stack e2e, not the v0.1 core.

### D. Ship the test kit late, after the widget catalog

*What it is.* Grow widgets first; add testing once the API settles.

*Steelman.* Less to stabilize early; the harness API can bake against real widgets.

*Why not chosen.* Cursive's lesson is the opposite — puppet existed early and every widget could
lean on it. Retrofitting is painful: kitty-protocol and DataTable perf sat unfixed in Textual
partly because nothing forced the seam early. Every widget we ship *without* both kinds of tests
is debt, and third-party authors need the harness on day one to build against a stable contract.

## Decision

rabbitui ships `rabbitui-testing` **before the widget catalog grows**, as **public API**, with
three layers:

1. **A headless driver.** `rabbitui-testing` exposes a `TestApp`-style harness that constructs an
   app at a fixed terminal size, injects input events and messages (`send`, `type_text`), and
   advances the runtime loop deterministically. It provides `wait_for(|frame| …)` predicates that
   drain the message queue to quiescence rather than sleeping, and exposes the rendered buffer,
   the frame facts (hit regions, focus order, cursor, extents — ADR 0001), and returned outcomes
   for assertion. This is possible because rabbitui owns the loop (ADR 0005).

2. **Buffer snapshots with an update flag.** Buffer-equality assertions (ratatui-compatible
   `Buffer::with_lines` shape, ADR 0003) and stored snapshots are compared against rendered
   output; an environment/CLI **update flag** (`teatest`/`pytest-textual-snapshot` `-update`
   semantics) regenerates accepted snapshots. Snapshots also cover the frame-facts record, not
   only cells.

3. **A vt100-parser escape-level harness.** A backend that routes the app's *emitted escape
   sequences* through an in-process `vt100::Parser` and asserts on the resulting emulated screen
   grid and cursor state. This is the authoritative layer for inline mode, resize/reflow,
   synchronized-output framing (mode 2026), and suspend/resume — the paths whose correctness is
   defined at the ANSI layer, below buffer equality.

Additionally:

- **The clock is injectable.** The runtime's time source is a trait; tests supply a manual clock
  and advance it explicitly (`advance(Duration)`), so timers, animations, coalesced frame
  scheduling (ADR 0005), and debounce logic are deterministic and never require `sleep`. Timers
  re-arm as effects (ADR 0001), so advancing the clock deterministically drives them.

- **Every widget in `rabbitui-widgets` carries both a buffer-snapshot test and a vt100
  escape-level test.** This is a catalog contribution requirement, not a suggestion.

- **The harness is public, documented, semver-stable API** so third-party widget authors and
  coding agents verify their own output against the same contract the core uses. A shipped agent
  skill (ADR 0008) references it as the verification loop.

## Consequences

*Positive.*

- Interaction correctness — the advertised moat — is provable in CI at the escape level (CJK
  width, resize, focus, mouse-in-overlays, inline scrollback commit), the exact class of bug
  reviewers probe and that broke FrankenTUI/textual-rs (`docs/research/recent-rust-tui-wave.md`).
- Third parties and agents get a stable verification surface from v0.1; agents can close the
  see-nothing loop (testty's raison d'être).
- Deterministic time kills the flakiest test class (timer/animation sleeps) that Textual never
  protected against.
- The three layers form an honest pyramid: cheap buffer snapshots for the common case, vt100 for
  the ANSI-critical paths, driver underneath both.

*Negative (honestly).*

- The vt100-parser backend is a real dependency and maintenance surface; if its emulation
  diverges from real terminals, escape-level tests can pass while a real emulator misbehaves —
  vt100 is a model of a terminal, not every terminal. Mitigation: a later out-of-process
  PTY/VHS layer for true e2e, and the full-repaint desync escape hatch (ADR 0003) as a runtime
  backstop.
- Making the harness public, semver-stable API means its surface is now a compatibility
  commitment; a bad early API shape is expensive to change. Mitigated by copying proven shapes
  (teatest/Pilot/puppet) rather than inventing.
- Requiring two test kinds per widget raises the cost of every catalog contribution and slows
  the catalog's growth — a deliberate trade of breadth for trust.

*Neutral.*

- The injectable clock imposes a time-source trait through the runtime that non-test code must
  route through; a thin cost the async loop pays anyway.
- Snapshot files become reviewable artifacts in the repo; update-flag discipline (review the
  diff, don't blind-accept) becomes a contributor norm, as it is in the ratatui and Textual
  ecosystems.

## Revisit triggers

- **A live-app bug ships that both a buffer snapshot and the vt100 harness passed** — the
  escape-level layer is insufficient and a true out-of-process PTY/VHS recorder must be
  promoted from "later" to core.
- **vt100 emulation is observed to diverge from a real terminal** on a path we test — reassess
  the parser (swap library, or add a PTY conformance layer).
- **Third-party or agent authors report the harness API is too small/awkward to verify their
  widgets** — the public contract needs expansion (e.g. richer facts assertions, semantic
  screen queries à la testty).
- **Test-suite wall-clock time becomes an adoption complaint** (Textual's `#5068` failure mode)
  — invest in parallelism / a faster driver before the suite is load-bearing.
- **A widget class emerges whose correctness the vt100 grid cannot express** (e.g. images via
  Kitty/Sixel graphics protocol) — extend the harness to assert on those escape families.
