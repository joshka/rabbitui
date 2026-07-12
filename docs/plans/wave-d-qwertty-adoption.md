# Wave D — qwertty 0.1.x adoption (implementation spec)

Written 2026-07-11 on Fable, on the news that qwertty shipped 0.1.0/0.1.1 to crates.io.
The release contains everything Arc 4 was parked on. Verify each claim against the
installed release before implementing (`qwertty::docs` pages are authoritative); the
qwertty tree lives at `../../../qwertty/work/default`, but the **published crate** is now
the integration target. Playbook discipline applies. Items are independent lanes unless
noted; ordering below is by leverage.

## D1 — version dependency + CI

- Switch the workspace `qwertty` dep from the path to the published version
  (`qwertty = { version = "0.1", features = ["tokio"] }`), keeping a
  `[patch.crates-io]` entry pointing at the local path while both trees co-develop —
  ask the author which mode CI should build (pure-registry proves the release; patched
  proves HEAD). Default: registry in CI, patch locally.
- Unblock the Arc 5 CI items gated "when qwertty publishes": the betamax tape job and a
  plain `cargo build` job with **no** path deps (proves the release is sufficient).

## D2 — suspend/resume + $EDITOR handoff (the trait hooks, now unblocked)

qwertty shipped suspend/resume around `SIGTSTP`/`SIGCONT` and `run_detached` on
`TokioTerminalSession`. Wire it:

- Facade: on suspend (Ctrl-Z reaching the loop / `SIGTSTP` via qwertty's signal surface),
  write the engine's leave frame, let qwertty suspend, and on `SIGCONT` re-enter the mode,
  force a full repaint (`engine.force_repaint()`), and deliver a resize (the terminal may
  have changed while stopped).
- Trait: add the defaulted hooks `fn on_suspend(&mut self)` / `fn on_resume(&mut self)`
  (non-breaking, per the §1 №6 deferral — this wave is the "later"). Call them around the
  suspend seam.
- `Update::detach(f)` or a `Command`-shaped equivalent for $EDITOR handoff via
  `run_detached`: leave raw mode + alt screen, run the closure/child, restore, repaint.
  Design the exact surface against qwertty's `run_detached` signature when implementing.
- e2e: FakeDevice cannot deliver signals — cover the leave/re-enter byte sequence with
  `VtScreen` assertions and unit-test the hook ordering; note real-signal verification is
  a coordinator betamax item.

## D3 — push-based resize

qwertty ships a `SignalStream` and a `ResizeStream` (`SIGWINCH` fallback) plus in-band
resize (mode 2048) in `Capabilities`. Replace the per-input-event size poll in `run_loop`
with a `select!` arm on the resize source; delete the "polled, not pushed" caveat from the
`Event` docs (`rabbitui/src/app.rs` module doc — the doc itself promises this change).
Keep the poll as a fallback only if the stream is absent on some platform; verify against
the release.

## D4 — lone-Esc flush timing

qwertty exposes lone-Escape flush timing control. Adopt: configure the session's Esc
timing at open (surface a `Config` field only if apps genuinely need to tune it — default
to qwertty's default), then revisit `docs/design/keybinding-conventions.md` §Esc and the
Wave B1 note that avoided Esc in e2e tests — with deterministic flush timing, Esc-to-close
becomes testable; add an Esc-close assertion to the flagship help-overlay e2e then.

## D5 — the pre-existing adoption backlog (unchanged, now schedulable)

- KeyEvent/TextPayload pre-pin migration (ADR 0019 vocabulary froze; the interim decode
  layer in `rabbitui/src/input.rs` shrinks family-by-family).
- Drop the `/dev/tty` restore backstop where qwertty's `RestoreHandle` covers it
  (`rabbitui/src/terminal.rs` — keep the panic-hook path until verified equivalent).
- Width/grapheme negotiation (mode 2027) → the one-width-oracle seam when qwertty
  exposes it.

## Acceptance

Workspace + comparisons suites, clippy, nightly fmt; e2e 5×; a registry-only build job
green; suspend/resume + $EDITOR verified by the coordinator on a real TTY (betamax).
Update ROADMAP Arc 4 rows (suspend ⬜ → status) and this file with dated corrections.
