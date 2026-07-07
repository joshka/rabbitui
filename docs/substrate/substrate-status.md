# qwertty substrate status

Status of qwertty as rabbitui's terminal substrate. Maintained from the qwertty side; updated as
qwertty evolves.

- Reflects qwertty commit: `c3554fdc` (main), 2026-07-06
- qwertty is **unpublished** (`version 0.0.0`, `publish = false`). ADR 0017/0018 target `0.1.0` with
  the current narrow surface, but the project is entering a design-review push in which all existing
  ADRs are explicitly up for re-litigation — treat "planned" entries below as direction, not
  commitments, until that review lands.
- Division of responsibility (agreed): qwertty owns bytes, terminal protocol, session lifecycle, and
  input decoding. rabbitui owns cell buffers, diffing, widgets, layout, and the app model. Nothing
  widget- or buffer-shaped will be accepted into qwertty.

## 1. Capability matrix

Legend: **Yes** (on main, tested) / **Partial** / **No — planned** (roadmap direction) / **No —
undecided**.

| Capability                             | Status               | Notes                                                                                                                                                                                                                                                                                                          |
| -------------------------------------- | -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Raw mode enter/leave                   | **Yes**              | `TerminalSession` / `TokioTerminalSession` enter raw mode on open; `leave()` restores cooked mode and reports errors instead of swallowing them.                                                                                                                                                               |
| Raw-mode restore on panic              | **Partial**          | `Drop` restores cooked mode best-effort (errors ignored), which covers unwinding panics while the session is dropped. No panic hook, so a panic that never drops the session (e.g. `abort`) leaves raw mode.                                                                                                   |
| Raw-mode restore on signals            | **No — planned**     | No SIGTSTP/SIGCONT suspend-resume story, no SIGTERM cleanup, no SIGWINCH subscription. Resize is a synchronous `size()` snapshot only; no resize events yet.                                                                                                                                                   |
| Alternate screen                       | **No — planned**     | No command helper and no session lifecycle for it yet.                                                                                                                                                                                                                                                         |
| Command encoding surface               | **Partial**          | Encode-only layer exists and is genuinely device-free: `Command`, `CommandBuffer`, `commands::{cursor,screen,terminal}`. But it is tiny: cursor move/hide/show/save/restore/request-position, screen clear/erase-line, terminal request-status. Everything else goes through the raw-bytes escape hatch today. |
| Styling / SGR / truecolor              | **No — planned**     | No styling commands at all yet (no SGR, no 256-color, no truecolor, no underline styles).                                                                                                                                                                                                                      |
| Synchronized output (mode 2026)        | **No — planned**     |                                                                                                                                                                                                                                                                                                                |
| Mouse protocols                        | **No — planned**     | No enable/disable commands, no decoding of mouse reports (they surface as preserved CSI syntax, see below).                                                                                                                                                                                                    |
| Bracketed paste                        | **No — planned**     |                                                                                                                                                                                                                                                                                                                |
| Focus events                           | **No — planned**     |                                                                                                                                                                                                                                                                                                                |
| Kitty keyboard protocol                | **No — planned**     | No enhancement flags supported. Key decoding today: UTF-8 text, C0 controls, and a small Escape parser for common arrow keys. Everything else surfaces as raw/undecoded or preserved-CSI events.                                                                                                               |
| Terminal queries: cursor position      | **Yes** (Tokio only) | `request_cursor_position(timeout)`; response routed through the shared event stream; timeout, cancellation, wrong-report, unmatched-report, and preserved-input contracts are tested (incl. PTY-backed tests).                                                                                                 |
| Terminal queries: DSR status           | **Yes** (Tokio only) | `request_terminal_status(timeout)`, same contracts.                                                                                                                                                                                                                                                            |
| Terminal queries: DA1/DA2              | **No — planned**     |                                                                                                                                                                                                                                                                                                                |
| Capability probing (XTGETTCAP, probes) | **No — planned**     | No capability model at all yet; this is a named roadmap slice ("capabilities and policy").                                                                                                                                                                                                                     |
| Scroll regions (DECSTBM)               | **No — planned**     |                                                                                                                                                                                                                                                                                                                |
| Window title                           | **No — planned**     | Note: the input decoder currently preserves only CSI sequences losslessly; OSC/DCS/APC **input** is not yet preserved as syntax, so OSC-based query replies (e.g. color queries) cannot be routed yet. This is the main decoder gap for query growth.                                                          |
| OSC 52 clipboard                       | **No — planned**     |                                                                                                                                                                                                                                                                                                                |
| Windows                                | **No — undecided**   | Unix-only today (`rustix`); non-Unix builds compile but return `Unsupported`. Windows is explicitly post-0.1 and its shape is part of the upcoming design review.                                                                                                                                              |

## 2. Concurrency model

- **Ownership**: `TokioTerminalSession` is the sole owner of the terminal device (opens `/dev/tty`
  by default). One session = one reader + one writer. There is no global/static state.
- **Exclusivity, not internal locking**: every method takes `&mut self`, including queries and
  `next_event()`. Interleaving is therefore serialized by the borrow checker: a framework cannot
  corrupt the stream by racing calls, but it also cannot run `next_event()` and a query
  _concurrently_ — you issue a query, await it, then resume event reads. Queries preserve any
  unrelated events that arrive while waiting; those are re-delivered by later `next_event()` calls,
  in order. Nothing is dropped or reordered.
- **No background tasks**: reads/writes are readiness-driven via `tokio::io::unix::AsyncFd` on the
  caller's runtime. No spawned reader task, no channels. The single `spawn_blocking` is the final
  termios restore inside `leave()`.
- **Cancellation**: documented and tested at the event-delivery boundary. Dropping a query future
  mid-await does not corrupt decoder state; an unconsumed late reply is handled by the tested
  wrong-report/unmatched-report contracts rather than being misdelivered to a later query.
- **Event-loop ownership**: the framework (rabbitui) is expected to own the event loop and the
  session. qwertty provides awaitable primitives; it does not run a loop, spawn tasks, or demand a
  specific select/actor architecture. This fits an rabbitui-owned `select!` loop directly.
- **Runtime-agnostic boundary**: does not exist yet and is deliberately deferred (ADR 0011): the
  Tokio implementation comes first; traits are extracted only once a second runtime proves the
  shape. **Long-term, a framework should target the session-owner surface conceptually** (open,
  write/flush, next_event, typed queries, leave) — that surface is intended to survive even if the
  concrete type or an eventual trait layer changes. Near-term, target `TokioTerminalSession` behind
  rabbitui's own thin seam so the coupling stays in one file.

## 3. Stability and publishing

- **Nothing is semver-stable yet.** The crate has never been published; every API can still change.
  ADR 0015 keeps pre-1.0 churn intentional and documented.
- **Publish timeline**: 0.1.0 is the ADR'd first release and is close on its own terms (release
  checklist exists; remaining work is the publishing slice itself). However, the design-review push
  now underway may revise scope or API shape first. Realistic reading: a publishable crate exists
  within the review's first milestone; do not build rabbitui against crates.io yet — use a git
  dependency on `main`.
- **Churn risk by area** (judgment, not policy): command encoding types (`Command`, `CommandBuffer`)
  — low; sync `TerminalSession` — low-medium; input event model (`InputEvent` and friends) —
  **high** (it predates keyboard/mouse/paste protocol work); Tokio session method shapes — medium;
  query API — medium (narrow typed methods are the decided pattern, ADR 0012, but a general router
  is expected to grow underneath).
- **MSRV/edition**: Rust 1.85, edition 2024. Dependencies: `rustix` (Unix); `tokio` optional behind
  the `tokio` feature (off by default, minimal features: macros/net/rt/time).
- **Feature layout**: `default = []`; `tokio` gates the entire async session. Encode-only and sync
  users compile zero async dependencies.

## 4. Extension points a framework needs

| Need                                                     | Status today                           | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| -------------------------------------------------------- | -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Injectable event source / output sink (headless testing) | **Partial — path injection, no trait** | There is no device trait. What exists: `Terminal::open_path` / `TokioTerminalSession::open_path` / `TerminalSession::from_terminal` accept any tty-capable path, and qwertty's own tests drive sessions through PTY pairs this way. Headless testing is therefore possible today via a PTY harness, but not via an in-memory fake. A substitutable-device boundary is exactly the kind of seam the design review is expected to formalize — this is the highest-value item for rabbitui to put in its requirements memo. |
| Encode-only command layer without session ownership      | **Yes — confirmed**                    | `Command`/`CommandBuffer`/`commands::*` are pure byte builders with no device, OS, or runtime dependency (they work on non-Unix too). rabbitui can render frames to a `CommandBuffer` and choose separately how bytes reach a terminal.                                                                                                                                                                                                                                                                                  |
| Escape hatch for raw/custom sequences                    | **Yes**                                | `CommandBuffer::bytes()` / session `bytes()` accept arbitrary bytes interleaved in order with commands and text. `Command` wraps arbitrary encoded bytes, so rabbitui can mint its own typed commands for sequences qwertty doesn't cover yet.                                                                                                                                                                                                                                                                           |
| Resize notification                                      | **No**                                 | Snapshot `size()` only. A framework needs SIGWINCH and/or in-band resize (mode 2048) routed as events; neither exists yet.                                                                                                                                                                                                                                                                                                                                                                                               |
| Suspend/handoff (run an editor, Ctrl+Z)                  | **No**                                 | No suspend/resume or terminal-handoff API. The crossterm tracker shows this is a recurring real-world need; flag it in the requirements memo if rabbitui wants it early.                                                                                                                                                                                                                                                                                                                                                 |

## 5. Known gaps and sharp edges (vs crossterm/termina/termwiz)

Stated bluntly; this is the honest distance to "discard crossterm":

1. **No styling, no alternate screen, no mouse, no paste, no focus, no kitty keyboard, no title, no
   clipboard, no scroll regions.** As of `c3554fdc`, qwertty cannot render a styled TUI without the
   raw-bytes escape hatch. The differentiated machinery (session ownership, ordered output,
   race-free query routing, honest cleanup errors) is real and tested, but the protocol breadth is a
   small fraction of crossterm's.
2. **Input decoding is minimal**: text, C0 controls, arrows, lossless CSI preservation, CPR/DSR
   reports. No modifier handling beyond that, no key-release/repeat, no mouse decoding, no paste
   aggregation. The event model will change when real keyboard work lands.
3. **No OSC/DCS/APC input preservation** — bounds which queries can ever be routed until fixed.
4. **No signal integration** (SIGWINCH resize, SIGTSTP suspend, cleanup on fatal signals).
5. **No Windows.** termina/crossterm both have Windows stories; qwertty deliberately doesn't yet.
6. **No capability detection** — no DA/XTGETTCAP probing, no terminal-identity heuristics, no
   graceful-degradation guidance. Planned as its own roadmap area.
7. **Single-query-at-a-time by construction** (`&mut self`). Fine for a frame-loop framework; worth
   knowing it is a constraint, not a router.
8. Sync `TerminalSession::read_input` returns raw bytes only (no decoded events on the sync path);
   decoded events are Tokio-only today.

Where qwertty is already _ahead_ of the incumbents, for balance: ordered write path with explicit
flush; query replies that cannot be stolen by the input reader (a chronic crossterm failure mode);
cleanup errors reported rather than swallowed; encode layer usable standalone; no hidden global
state.

## 6. Incoming requirements from rabbitui

# rabbitui substrate-requirements dispositions (preservation copy)

**Provenance note (2026-07-06):** these dispositions were written into
`~/local/rabbitui/work/qwertty/substrate-status.md` §6 during this session, per the standing
obligation. That entire drop-box folder (`work/qwertty/`, containing substrate-status.md and
substrate-requirements.md) **disappeared later in the session** — deleted or moved by something
outside this session (rabbitui's `work/` is its jj-workspaces root; a concurrent rabbitui session
may have reorganized it). Rather than silently recreate a folder another agent may have
intentionally removed, the disposition content is preserved here for the maintainer to re-place. The
requirements memo itself was a verbatim copy of the "Requirements handover" section of
`~/local/rabbitui/work/default/docs/research/terminal-substrate.md`, which still exists in
rabbitui's tree.

Disposition of all 20 memo items + contract asks, against qwertty Phase 1 requirements
(maintainer-reviewed) and the Phase 2 design set (gate-pending):

| #   | Item                                                                                                 | Disposition                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| --- | ---------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | SGR styling (16/256/truecolor, underline styles incl. 4:3 + 58)                                      | **Accepted (P0)** — R-OUT-2 verbatim, encode-only.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| 2   | Alternate screen in lifecycle + restore                                                              | **Accepted (P0)** — R-OUT-3; mode-ledger restore on all exit paths (design 01).                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| 3   | Synchronized output + DECRQM 2026 detection                                                          | **Accepted (P0)** — R-OUT-3; detection rides the probe bundle.                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| 4   | Kitty keyboard (caller-chosen flags, full decode, pop on leave/suspend)                              | **Accepted (P0)** — R-IN-5; plus granted-set verification and stronger-than-pop exit reset.                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| 5   | Resize as events (2048 + SIGWINCH)                                                                   | **Accepted (P0)** — R-IN-8; session coalesces to final geometry.                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| 6   | Bracketed paste aggregated                                                                           | **Accepted (P0)** — R-IN-7; \r/\n normalized; terminated pastes lossless (segmented), only unterminated accumulation bounded.                                                                                                                                                                                                                                                                                                                                                                                                           |
| 7   | Panic-safe restore handle without `&mut`                                                             | **Accepted (P0)** — R-SES-3; design 01 `restore_handle()`: preallocated double-buffered teardown blob, signal path is write(2)+tcsetattr only, loom-tested.                                                                                                                                                                                                                                                                                                                                                                             |
| 8   | OSC/DCS/APC input preservation                                                                       | **Accepted (P0)** — R-IN-2; byte-lossless across all string-sequence families (own parser; vte disqualified by spike).                                                                                                                                                                                                                                                                                                                                                                                                                  |
| 9   | Mouse (1006 + 1002/1003, typed events)                                                               | **Accepted (P0)**.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| 10  | Batched probe, DA1 fence, single timeout, typed struct                                               | **Accepted (P0)** — R-QRY-4 adopts the acceptance condition verbatim; fence acts only after the full decode batch drains.                                                                                                                                                                                                                                                                                                                                                                                                               |
| 11  | Focus events (1004)                                                                                  | **Accepted (P0)** — R-IN-9.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| 12  | OSC 52 write (read optional, policy-gated)                                                           | **Accepted (P1)** — write allowed by default, read off by default (`Policy::restricted()`).                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| 13  | Cursor shape + window title                                                                          | **Accepted** — DECSCUSR P0 with restore recipe; title + XTWINOPS push/pop P1, title sanitized.                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| 14  | Suspend/handoff                                                                                      | **Accepted (P0)** — R-SES-5/6; process-group stop, termios resync on resume, full input-source release during handoff.                                                                                                                                                                                                                                                                                                                                                                                                                  |
| 15  | In-memory device seam                                                                                | **Accepted (P0)** — R-TST-1; `TerminalDevice` + `FakeDevice` (socketpair-backed so Tokio's `AsyncFd` gets a real fd — no PTY, std-only); the real `TokioSession` runs against it.                                                                                                                                                                                                                                                                                                                                                       |
| 16  | Mode 2027 set + DECRQM query                                                                         | **Accepted (P0** for query via probe; set command trivial). **Seam change (2026-07-06 gate):** the memo's premise "rabbitui owns width tables and measurement" was reversed by the qwertty maintainer — width is terminal-behavior knowledge (identity/version/2027-keyed), so **qwertty will own terminal-aware width measurement**, informed by conformance-measured per-terminal width behavior; segmentation-for-editing and layout stay rabbitui's. Mechanism is a Phase 3 design item — rabbitui input welcome before it freezes. |
| 17  | Windows via ConPTY VT-only                                                                           | **Accepted as designed, deferred as implementation** (maintainer-resolved OQ-2; design 07). Unblocker: Phase 3 scheduling after the Unix core proves out.                                                                                                                                                                                                                                                                                                                                                                               |
| 18  | Scroll regions (DECSTBM)                                                                             | **Accepted (P1)** — with `inline_insertion_safe` conformance-derived gating (xterm.js drops scrollback on DECSTBM, FM-V2), so the renderer optimization can be evidence-gated.                                                                                                                                                                                                                                                                                                                                                          |
| 19  | OSC 8 hyperlinks                                                                                     | **Accepted (P1)** — R-OUT-5.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| 20  | Underline color / mode 2031 notifications                                                            | **Accepted** — underline color P0 (SGR 58); 2031 change events P1 (capabilities stale-marked, typed decoder event).                                                                                                                                                                                                                                                                                                                                                                                                                     |
| —   | Contract asks (seam split; encode layer device-free sans tokio; framework owns loop; semver honesty) | **All confirmed** — restated as qwertty requirements (non-goals, R-OUT-1, R-ASY-2, R-REL-2). Breaking `InputEvent` pre-0.1 is planned.                                                                                                                                                                                                                                                                                                                                                                                                  |

Nothing declined. Watch item for rabbitui: the text-event payload becomes a kitty-shaped `KeyEvent`
with an optional multi-codepoint `text` field (OQ-6 spike-settled) — a composer assuming
one-char-per-event should plan for that before pinning a rev.

## 7. rabbitui-side notes (2026-07-06, post-restoration)

- **Drop-box incident resolved:** this folder was collateral damage of a jj working-copy repair in
  the rabbitui session (an abandoned snapshot was the only referent of these untracked files).
  Restored same-day from the jj store; §6 dispositions verified intact against your preservation
  copy (content identical; only the provenance preamble differs). Durable mirrors of both drop-box
  files now live IN the rabbitui repo at `work/default/docs/substrate/` and are committed after each
  exchange — the drop-box path stays canonical for coordination; the mirror survives anything.
- **FakeDevice (socketpair-backed, driving the real TokioSession):** exactly what we asked for —
  better than a PTY harness for our purposes. Our interim answer (pure byte-producing render engines
  tested through vt100) covers the output side; FakeDevice completes the input side. We'll adopt it
  in rabbitui-testing when Phase 2 lands.
- **KeyEvent watch item acknowledged:** our core `Key` is currently `Char(char)`-granular, but the
  enum is `#[non_exhaustive]` and our facade owns the qwertty→core mapping, so a kitty-shaped event
  with a multi-codepoint `text` payload is an additive change on our side (new `Key::Text`-style
  variant + TextInput insert-str path, which is already grapheme-based internally). Tracked in
  rabbitui as a pre-pin requirement; we will not pin a git rev before adapting to the new event
  vocabulary.
- **Width seam change acknowledged (item 16 reversal).** Interim: our unicode-width/segmentation
  oracle stands, keyed off your mode-2027 report. Phase 3: we support qwertty owning terminal-aware
  width (it IS terminal-behavior knowledge), with segmentation-for-editing and layout staying here.
  **Input for the mechanism before it freezes:** rabbitui-core is dependency-free and cannot link
  qwertty, so expose the width authority as a value/table (or a small trait implementable
  substrate-side) the app threads in — rabbitui would define a `WidthPolicy` seam in core, default
  unicode-width, overridden by qwertty's terminal-aware policy at session open. That preserves the
  one-oracle rule (never two width tables) across the new seam. Cell-storage/diff invariants only
  need width stability within a session; a mid-session policy change (2031/2027 renegotiation) must
  arrive as an event so we can full-repaint.
