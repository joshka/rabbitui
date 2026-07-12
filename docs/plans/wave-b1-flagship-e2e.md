# Wave B1 — flagship e2e tests over FakeDevice (implementation spec)

Written 2026-07-11 on Fable. The harness exists and is proven
(`rabbitui/tests/e2e_headless.rs`, `docs/design/fakedevice-e2e-harness.md` — read both
first). This wave points it at the flagship: the three bug classes that motivated the
harness (help-overlay panic, tool-cell scrollback freeze, mode-toggle duplication) become
permanent CI guards. Playbook discipline applies (`00-execution-playbook.md`).

Sequencing: parallel-safe with Wave A steps 1–5 (different files); if Wave A's flagship
migration (its Step 6) is in flight, land that first to avoid churning the same app files.

## Step 1 — promote the pump harness into the facade (feature `harness`)

The `Harness` in `rabbitui/tests/e2e_headless.rs` is test-local; rabbitui-agent needs it
too. Copy-paste across crates is the wrong move — promote it:

- New module `rabbitui/src/harness.rs` behind a new cargo feature `harness`:
  `[features] harness = ["dep:rabbitui-testing"]`, with
  `rabbitui-testing = { workspace = true, optional = true }` as a **regular optional dep**
  of the facade. Direction check: facade → testing → core; no cycle; ADR 0011 holds
  (nothing depends on the facade).
- Move the struct generically, decoupled from any app shape — it drives a **future**, so
  it works for closure apps, trait apps, and the flagship identically:

  ```rust
  pub struct Harness<F: Future<Output = crate::app::Result<()>>> {
      app: Pin<Box<F>>, terminal: qwertty::FakeTerminal,
      pub screen: rabbitui_testing::vt::VtScreen,
      done: Option<crate::app::Result<()>>,
  }
  impl<F: …> Harness<F> {
      pub fn launch(app: F, cols: u16, rows: u16) -> (Self, qwertty::FakeDevice) // or take a
      // builder closure `impl FnOnce(qwertty::FakeDevice) -> F` so the device never escapes:
      pub fn launch_with(build: impl FnOnce(qwertty::FakeDevice) -> F, cols: u16, rows: u16) -> Self;
      pub async fn pump_once(&mut self);
      pub async fn wait_for(&mut self, needle: &str) -> bool;   // 500-iteration cap, 2ms tick
      pub async fn wait_while(&mut self, needle: &str) -> bool; // inverse: until needle GONE
      pub fn feed(&mut self, bytes: &[u8]);
      pub async fn join(self) -> crate::app::Result<()>;
  }
  ```

  Prefer `launch_with` (the `FakeDevice::open()` pair is created inside; the closure gets
  the device and returns the run future — for the flagship that closure is
  `|dev| my_app.run_over_device(dev)`). Keep the module doc's "why a pump, not a spawn"
  explanation — it is load-bearing knowledge.
- Rewrite `rabbitui/tests/e2e_headless.rs` on top of the promoted type (facade dev-deps
  itself with `features = ["harness"]` via `[dev-dependencies] rabbitui = { path = ".",
  features = ["harness"] }` — if cargo rejects the self-dep in this workspace setup, keep
  the harness `pub` behind the feature and have the test enable it via
  `required-features`).
- Gate + commit: `feat(harness): promote the FakeDevice pump harness behind a feature`.

## Step 2 — flagship test entry

`rabbitui-agent` needs a headless constructor. Read `rabbitui-agent/src/main.rs` (the
`ReplayBackend::from_path` wiring, ~line 66) and `src/app.rs`, then factor a
`pub fn build_app(backend: Box<dyn Backend>) -> <the App value>` (exact return type
follows whatever shape Wave A left: the `FnApp` value or the flagship's `impl App` type)
so `main` and tests share one construction path. Add
`rabbitui = { workspace = true, features = ["harness"] }` to rabbitui-agent's
dev-dependencies. Check `rabbitui-agent/tests/slice*.rs` and `src/backend/replay.rs` for
the existing replay fixture format — reuse a fixture with a `tool_use` turn if one exists;
otherwise author `tests/fixtures/tool_turn.replay` per the replay format (keep it minimal:
one assistant text delta, one tool_use requiring approval, one final text).

## Step 3 — the three tests (`rabbitui-agent/tests/e2e.rs`)

Chords verified against `src/keymap.rs`: Help = Ctrl-G (`0x07`; the Ctrl-/ alias is
`0x1f` — send Ctrl-G, it is unambiguous), ToggleMode = Ctrl-T (`0x14`), Allow = `y`,
Quit = Ctrl-C (`0x03`). **Close help with a second Ctrl-G, not Esc** — lone-ESC decoding
is a live qwertty coordination item; don't couple this suite to it.

1. `help_overlay_opens_closes_and_loop_survives` — the declare-then-focus panic guard.
   Launch with an idle replay backend; `wait_for` the composer's key-hint footer (grep
   `view` for its literal text and cite it in the test); feed `0x07`; `wait_for("keys")`
   (the overlay title); feed `0x07` again; `wait_while("keys")`; then feed a printable
   into the composer and `wait_for` it echoed — proves the loop did not die.
2. `tool_turn_settles_to_terminal_glyph_in_scrollback` — the scrollback-freeze guard.
   Launch with the tool_use fixture; `wait_for` the approval modal (assert on its
   Allow/Deny text); feed `y`; then wait until `screen.all_lines()` (scrollback included)
   contains the tool cell's terminal `✓` glyph AND `wait_while` the pending spinner glyph.
   This is exactly the committable-end bug's shape: a cell committed at the Pending glyph
   never settles.
3. `mode_toggle_leaves_one_tail` — the duplication guard. Launch idle; settle; feed
   `0x14`; settle (wait_for a marker that only the alt/browse chrome shows); feed `0x14`
   again; settle; assert the key-hint footer appears **exactly once** in
   `screen.contents()` (count occurrences), and the transcript content appears exactly
   once in the visible region.

Determinism rules from the existing suite apply: never a bare sleep — always
`wait_for`/`wait_while` a rendered marker; run the suite 5× before calling it done.

## Acceptance

1. `cargo test -p rabbitui-agent --test e2e` green 5× consecutively.
2. `cargo test --workspace` + clippy + `cargo +nightly fmt --all --check` clean.
3. Each test fails when its bug is reintroduced — spot-check at least №1 by locally
   reverting the help-overlay display-only fix (`jj` restore it afterward) and observing
   the test catch it. Record the result in this file.
4. Commit: `test(agent): flagship e2e over FakeDevice — help overlay, tool settle, mode toggle`.

## Known traps

- The flagship's `M = Msg` is not `()` — the promoted harness is M-agnostic because it
  holds only the future; don't re-introduce a type parameter for M.
- `VtScreen::new(cols, rows)` must match the FakeDevice's reported size (80×24 default;
  use `FakeTerminal::set_size` + a matching `VtScreen` if a test needs a different size).
- Inline mode writes into scrollback: assert history via `all_lines()`, visible tail via
  `contents()` — mixing them up makes test 2 flaky.
- If the replay backend needs real time to emit turns, prefer a fixture that emits
  immediately; the pump's 2ms tick is wall-clock.

## What good looks like (beyond the acceptance gates)

- Each test reads as a user story (open help, approve a tool, toggle modes) — a reader
  can tell what regressed from the test name + failure message alone, without opening
  the harness.
- No sleeps, no magic byte strings without a named const citing the keymap chord.
- The bug-reintroduction spot-check (step 3 of acceptance) is recorded IN this plan with
  the revert tried and the failure message observed.
- The promoted harness module doc keeps the "why a pump, not a spawn" explanation and a
  copy-pasteable example — the next crate to adopt it (rabbitui-agent was first) should
  need zero archaeology.
