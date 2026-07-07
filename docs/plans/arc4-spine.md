# Arc 4 plan — non-functional spine

Eight items from the middle-piece audit, each with its design position decided. They are
independent; the flagship pulls "error story" and "keymap" early (the keymap's minimal form is
built inside Arc 3 slice 5 — the item here is its generalization). Ordering among the rest is
free; qwertty adoption is gated on their side (check `work/qwertty/substrate-status.md`).

## 1. Error story

**Positions:** (a) Panics in `view`/`update` are bugs — let them crash; the panic-restore hook
already guarantees terminal restoration, which is the whole contract. Do not `catch_unwind` app
code (it hides bugs and corrupts state assumptions). (b) Effect-task panics are already contained
per ADR 0005 — standardize the surfacing: a `Cmd` whose future panics or errors delivers a typed
`EffectFailed { group, error }` message to `update`, and the docs show the match-arm pattern.
(c) Ship an `ErrorBanner` widget (Danger role, dismissible, top layer) as the recommended UX for
recoverable failures, used by the flagship's error cells. **Acceptance:** a doc page (when things
fail) with runnable examples; fetch example gains a deliberate-failure key demonstrating the
pattern; test that an effect panic restores the terminal and delivers EffectFailed.

## 2. Suspend / $EDITOR handoff

**Position:** API is `Cmd::suspend(f)` where `f: FnOnce() -> T` runs with the terminal restored to
cooked mode + primary screen (engine writes its leave frame, raw mode off), then re-enters and
schedules a full repaint, delivering `T` as a message. Implementation **waits on qwertty
R-SES-5/6 (RestoreHandle)** — write the API sketch and a `docs/design/` note now; wire it when
RestoreHandle is adopted (item 8). Ctrl-Z/SIGTSTP handling rides the same machinery. **Acceptance
(sketch phase):** design note committed; requirements doc cross-references it.

## 3. Keybinding/config layer

**Position:** generalize the flagship's `Keymap`: a declarative table mapping `Action` (app-defined
enum) → chords, consulted via `update.action(&keymap)`; TOML remapping section loaded by the same
facade config path as themes; `HelpOverlay` widget generated from the table (chord + action label
columns). Constraints carried in: printable chords must be `consumed()`-guarded automatically
(the keymap helper checks it, not each app); lone Esc unusable until upstream fixes timing.
**Acceptance:** flagship and two examples ported to it; remap-via-TOML integration test; help
overlay in the gallery.

## 4. Performance budgets in CI

**Position (from the 2B benchmark verification):** wall-clock is load-sensitive (scroll-10k
measured 1.13ms quiet vs 2.4ms under load), so CI budgets must be instruction-count-based —
**iai-callgrind** job on ubuntu (valgrind available), covering the three frame benches + diff, with
thresholds set ~30% above first-run baselines. Keep criterion for local trend work; do not gate CI
on it. CompactString cell optimization: measure first with the existing benches — adopt only if
frame-build improves ≥10%; otherwise record the negative result in the bench design note.
**Acceptance:** CI job green with committed baselines; a doc line telling contributors how to
update baselines intentionally.

## 5. Accessibility groundwork

**Position:** minimal recording, no exporter yet. `RenderCtx` gains `semantic_role(SemanticRole)`
(Button/TextInput/List/Dialog/Log/…) and `label(&str)`; both are recorded into frame facts next to
areas/focus. Widgets in the catalog set them. The a11y _exporter_ (AT-SPI etc.) is out of scope —
the point (per both field reports) is that the facts already carry what an exporter needs, which
is the architectural tiebreaker. **Acceptance:** facts snapshot tests show roles/labels for the
gallery; a design note stating the export path and what's deliberately deferred.

## 6. Key/WidgetId debuggability

**Position:** debug-capture, not interning. Behind `feature = "devtools"` (default-on in dev via
examples), `Key` construction records its source string into a per-frame side table in facts
(id → path-of-names). Release builds keep the current zero-cost FNV-only behavior. This feeds the
inspector (item 7) and the a11y labels default. **Acceptance:** inspector shows human paths;
`cargo check --release` unaffected; no public API change to `Key`.

## 7. Devtools facts inspector

**Position:** a `FactsInspector` overlay widget (toggled by an app-chosen chord; gallery and
flagship wire it to Ctrl-D) rendering the current facts tree: id path (via item 6), area, layer,
focusable, visibility requests, focus marker — plus a `facts::dump()` that writes the same to the
log seam. Read-only in v1 (no pick-to-highlight). **Acceptance:** tape of the gallery with the
inspector open; snapshot test of the dump format.

## 8. qwertty Phase 3 adoption (ordered, gated)

Adopt in this order, each its own commit + drift note in `docs/substrate/`:

1. **FakeDevice → rabbitui-testing:** drive a real `TokioTerminalSession` against the socketpair
   FakeDevice inside the vt100 harness, replacing byte-level fakery where it exists; keeps our
   coverage on qwertty's real decode path.
2. **RestoreHandle → terminal.rs:** replace `restore_directly()`'s hand-rolled `/dev/tty` write
   with the substrate's RestoreHandle; keep our unconditional leave-alt-screen byte backstop until
   qwertty guarantees equivalent behavior (verify against their doc, then drop).
3. **KeyEvent/TextPayload pre-pin migration:** only after qwertty posts the stability flag in
   `substrate-status.md`. Update `rabbitui/src/input.rs` mapping to the semantic KeyEvent +
   multi-codepoint TextPayload; TextInput consumes payloads not chars; re-run the full tape suite
   (this is exactly where key regressions surface). This is the pre-pin blocker for 0.1.

**Acceptance:** each step lands with the tape suite green and a substrate-status note confirming
versions; open P0s (lone-ESC, resize events) re-checked at each step — if lone-ESC has landed,
file the follow-up task to restore Esc bindings app-wide and retire the ctrl-chord workaround note
in ADR 0006.

## 9. `Update::set_theme` (runtime theme switching) — ✅ done 2026-07-07

Landed: `Update::set_theme(Theme)` buffered in the facade `Pending` and applied before the next
paint (mirroring `set_mode`), last-writer-wins with the theme-file watcher; `TestApp::set_theme` for
the harness; the gallery's number keys 1–4 switch theme live (verified end-to-end through the run
loop). Test: `rabbitui/tests/set_theme.rs`.

**Surfaced by the Arc 2A gallery (2026-07-07).** The runtime has no way to change theme mid-run —
the active theme is a run-loop local fed by the builder and the theme-file watcher. **Position:**
add `Update::set_theme(Theme)`, buffered in `Pending` and applied before the next paint, mirroring
`Update::set_mode` exactly (same buffering, same last-call-wins, same `TestApp::apply_pending`
support). Interaction with an active `theme_file`: last-writer-wins (a later mtime poll re-overrides);
document it. Small and well-shaped, but it touches core runtime + `Pending` + `TestApp`, so it lands
here with the keybinding/config work, not in an example. **Acceptance:** the gallery's number keys
switch theme live; a `TestApp` test asserts a role's resolved color changes after `set_theme`.
