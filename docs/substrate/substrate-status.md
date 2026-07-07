# qwertty substrate status

Status of qwertty as rabbitui's terminal substrate. Maintained from the qwertty side; updated as
qwertty evolves.

- Reflects qwertty change `sopwxwrn` (local jj, 2026-07-07). This is **ahead of the pushed `main`**
  (still `c3554fdc` on GitHub): qwertty is deep in Phase 4 implementation, 23 sealed local changes,
  none pushed yet. The design review referenced below **landed and passed its gate**; the ADRs were
  dispositioned (12 affirmed / 4 revised / 2 superseded).
- qwertty is still **unpublished** (`version 0.0.0`, `publish = false`); first publish is milestone
  **M8**, after the input protocols, screen/styling, capabilities, and suspend/handoff milestones
  complete. Keep the git/path dependency; do not expect crates.io before then.
- **Build-stability action for rabbitui (do this):** your `Cargo.toml` pins
  `qwertty = { path = "../../../qwertty" }`, which resolves to qwertty's **dev checkout** — the
  actively-churning working tree that is often mid-slice and may not compile between seals. **Repoint
  it to `../../../qwertty/work/default`**, the jj workspace qwertty keeps pinned to the last *sealed,
  green* change. You then build against a tree that always compiles and passes the full gate, and you
  advance deliberately (qwertty moves `work/default` forward only at each seal). This is the intended
  consumption path for a downstream during active development.
- Division of responsibility (unchanged, reaffirmed at the gate): qwertty owns bytes, terminal
  protocol, session lifecycle, and input decoding. rabbitui owns cell buffers, diffing, widgets,
  layout, and the app model. **One boundary moved at the gate:** grapheme *width measurement* is now
  qwertty's (terminal-behaviour-keyed, conformance-measured) — see the churn map. rabbitui still owns
  segmentation-for-editing and layout.

## 0. Phase 4 progress and churn map (2026-07-07) — READ THIS FIRST

The sections below (1–8) were written against `c3554fdc` and are **substantially superseded**; this
section is the current truth. qwertty rebuilt its input/output stack from first principles after the
design review. What that means for rabbitui, framed as: **what is stable to build on now** vs **what
will still churn**, so you can plan.

### Landed and stable to build on now (at `work/default`)

- **Encode layer — stable, device-free, no-tokio, buildable anywhere.** `Command`/`CommandBuffer`
  plus `commands::{cursor, screen, style, osc, terminal}`: full SGR (16/256/truecolor, all
  attributes + resets, underline styles + colour), alternate screen, cursor show/hide/shape, window
  title (sanitized), OSC 8 hyperlinks, OSC 52 clipboard write, OSC 133 semantic prompts, synchronized
  output (2026), scroll regions (DECSTBM/SU/SD/IL/DL). **This is your P0 render surface — emit-ready
  today.** These types are the least likely to churn.
- **Session lifecycle — stable shape.** `TerminalSession<D: TerminalDevice>` and
  `TokioTerminalSession<D>`: mode ledger that undoes exactly what it enabled on `leave`/drop, a
  panic-safe `RestoreHandle` obtainable without `&mut` (your P0-7, delivered), re-entrant
  `enter`/`leave` for per-frame or per-prompt cycling, and every mode (alt screen, mouse, focus,
  paste, kitty, in-band-resize) enabled through the same ledger so teardown is automatic.
- **`FakeDevice` / `TerminalDevice` seam — delivered (your P1-15, highest-value ask).** A
  socketpair-backed in-memory device drives the *real* `TokioTerminalSession` in a plain unit test:
  scripted input bytes in, emitted bytes out, no PTY. Your headless integration tests can use this
  today.
- **Decode + events — landed, but see churn note.** Total lossless syntax layer (all of
  CSI/OSC/DCS/APC/PM/SOS preserved byte-exact — the old "OSC/DCS input not preserved" gap is closed),
  a semantic layer, and the `Event` vocabulary: `KeyEvent` (kitty-shaped: key, modifiers,
  press/repeat/release, multi-codepoint text, shifted/base-layout alternates), `MouseEvent`,
  `FocusEvent`, `PasteEvent` (aggregated, lossless, `\r`-normalized), `ResizeEvent` (in-band 2048 +
  SIGWINCH helper, storm-coalesced to one final-geometry event), and `Event::Syntax` passthrough for
  anything unmapped. Kitty keyboard has the full push/verify-granted/pop lifecycle.
- **Race-free queries — proven.** Cursor-position and terminal-status queries with the twelve
  documented contracts (preserved-unrelated-input, timeout, late-reply, wrong-report, cancellation,
  …), a sans-io correlator, verified against tmux and headless ghostty (betamax).

### Still to come (the remaining churn, in likely order)

- **Capability probe + policy (M3, in progress now).** A DA1-fenced `probe_capabilities()` returning
  a typed capability struct (your P1-10, batched single-timeout probe — the exact shape you specified)
  is landing. After it: synchronized-output emission and OSC 52 will become **capability-/policy-
  gated** — i.e. the encode commands stay, but the *session* helpers that emit 2026 wraps or clipboard
  writes will consult probed support and a `Policy` (secure-by-default: clipboard read off, etc.).
  Plan for a policy argument/handle appearing on those session paths.
- **Suspend / resume / handoff (M6, not yet started).** Your P1-14 ($EDITOR handoff, Ctrl-Z). The API
  shape (`suspend`/`resume`, a `run_detached`-style handoff) is designed but not built — treat as
  not-yet-available.
- **First publish (M8).** Version → 0.1.0, `publish=false` removed. At that point sequence-database
  IDs become citable and a crates.io dependency becomes possible.

### The one churn pivot that matters most for rabbitui

**The `Event` / `KeyEvent` / `command` vocabulary is deliberately NOT frozen yet** — it freezes at
**milestone M4-S4** (a dedicated review pass, maintainer-gated, not yet run). Until then qwertty will
break these types freely if a better shape emerges (your own contract ask: "break `InputEvent` freely
before 0.1 rather than freeze it wrong"). **Concrete guidance:**

- If you build your event-handling against `Event`/`KeyEvent` now, expect field/variant changes until
  the freeze. Keep the coupling behind your single-file seam (as you planned) so a re-pin is a
  one-file update.
- The kitty-shaped `KeyEvent` carries text as an **optional multi-codepoint payload on the key event**
  (not one-char-per-event, and not a separate composed-text event). If your composer assumed
  char-granularity typed input, this is the change to plan for — it exists specifically so IME/compose
  and ZWJ-cluster text arrive as one associated event, not split.
- **Width measurement moved to qwertty** (gate decision). A `width_of(&str, &Capabilities)`-shaped API
  is a named future design item (needs a spike); it will be terminal-behaviour-aware, not a static
  unicode-width table. If you currently measure width yourself, know that a substrate-provided,
  terminal-accurate path is coming — you can keep yours until it lands, then decide.

When M4-S4 freezes the vocabulary, this doc will say so explicitly and that becomes the safe pin for
building durable event code.

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

**Landed status (2026-07-07):** the table below records *dispositions*; §0's churn map records what
is actually *available now*. Summary: items 1, 2, 4, 5, 6, 7, 8, 9, 11 (SGR, alt screen, kitty
keyboard, resize, paste, panic-restore, OSC/DCS preservation, mouse, focus), 13 (cursor shape,
title), 15 (in-memory device), and 19 (OSC 8) are **landed and available at `work/default`**. Item 3
(sync output) and 12 (OSC 52) have their **commands landed**; their capability-/policy-gated *session
emission* lands with item 10 (the probe, in progress now). Items 14 (suspend/handoff) and 17
(Windows) are **not yet started**. Item 16 (mode 2027) and 20 (mode 2031) land with the probe/caps
work. Nothing declined.

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

  **Width API shape requirements (answering the Phase 3 ask, from our real call sites —
  `Buffer::set_string`, `Cell::width`, TextInput cursor/scroll math, future `desired_height(width)`
  wrapping):**

  1. **Sync, `&self`, allocation-free, lock-free lookup.** Width is consulted in the strictly
     synchronous render core, O(visible cells) per frame (~thousands of calls at 60fps). No `async`,
     no `&mut`, no interior locks, no syscalls at query time. If conformance data backs it, compile
     it into a lookup structure at session open; never consult it lazily.
  2. **Granularity: grapheme cluster, required** — `fn width(&self, cluster: &str) -> u8` (0 for
     zero-width/controls, 1, or 2; if kitty text-sizing ever yields >2 we need to hear about it
     loudly, not silently). A per-string sum + `(cluster, width)` iterator is a nice-to-have only:
     segmentation stays on our side, so we can compose it ourselves.
  3. **Immutable epoch handles, not a mutable oracle.** The policy handle we capture is `'static`,
     cheap to clone (Arc-like), detached from the session's borrow (lookups happen while the session
     sits in select!), and carries an **epoch/identity id** we can compare cheaply. A mid-session
     width change (2027/2031 renegotiation, mux attach) must arrive as a decoded _event carrying the
     new handle_; the old handle stays valid and self-consistent so in-flight frames finish
     coherently, then we swap and full-repaint. Never mutate width behavior under an existing
     handle.
  4. **Constructible headless.** A pure default policy (unicode-width-class, no terminal, no
     session) must be available for tests and for rendering before the probe completes — first-frame
     latency cannot wait on conformance lookup; a policy-swap event after the probe is fine.
  5. **Deterministic unknowns.** State the fallback rule for clusters the table doesn't cover (we
     suggest: widest-codepoint rule, documented), the East Asian ambiguous-width setting's home
     (policy attribute, not a global), and the VS16/ZWJ/flag behavior per epoch — we will encode
     these in escape-level tests against the handle, so underspecification becomes test flakiness on
     your doorstep.

  Shape we would wrap it in: rabbitui-core defines `trait WidthPolicy` (default impl =
  unicode-width); our facade adapts your handle to it at session open. Anything satisfying 1–5 fits.

## 8. rabbitui adoption plan for the Phase 3 surface (2026-07-07)

Read your recent landings (device seam, RestoreHandle, syntax tokenizer, semantic events,
correlator). Adoption order on our side:

1. **FakeDevice/FakeTerminal — adopting first.** rabbitui-testing will drive the real Tokio session
   headless the moment the tokio session is generic over TerminalDevice on main (it already appears
   to be). This completes the input side of our harness; our vt100 layer keeps the output side.
2. **RestoreHandle — adopting second.** Replaces our /dev/tty restore-of-last-resort hack in the
   panic hook (and neatly sidesteps the macOS alias issue for the restore path too).
3. **Semantic KeyEvent/TextPayload — the pre-pin migration, scheduled.** Our facade mapping moves
   from the legacy InputEvent to your event vocabulary; our core Key is non_exhaustive so
   TextPayload lands additively. We will not pin a rev until this migration is done on both sides.
   Flag when you consider the event module shape stable enough to build against.
4. **Correlator — no action yet** (we use the typed query methods only).
5. Heads-up: your in-flight correlate module currently emits dead-code warnings that show up in our
   workspace clippy runs (cosmetic; scoped runs stay clean). No action needed unless it lingers.

- **2026-07-07 (drift note):** your input-API refactor (InputEvent →
  Event/KeyEvent/Key/SyntaxToken) landed while our Arc 2B build was in flight
  and broke our facade seam mid-task; we migrated faithfully the same hour
  (behavior-identical — full TextPayload adoption is still the scheduled
  pre-pin migration). No complaint — we signed up for tracking main — but this
  is the concrete case for the stability flag we asked for in §8 item 3:
  a one-line "event module shape frozen enough to build against" signal in
  this doc when you believe it, so we can schedule the real migration once
  instead of chasing drift.

- **2026-07-07 (drift note, mouse decode):** a later main commit
  ("Decode in-band resize and route SIGWINCH as coalesced resize events",
  touching input.rs/event.rs) changed how SGR mouse events surface: our facade's
  `from_qwertty` mouse mapping now returns `None` for `CSI < b ; col ; row M/m`
  sequences, so five `rabbitui` input tests (`sgr_mouse_press/release/drag/
  right_button/wheel`) fail on `unwrap()` (input.rs:291). Our keyboard mapping is
  unaffected; this is mouse-only. We are **not** chasing it mid-flight (§8 item 3:
  we migrate once, on your stability flag) — the mouse-decode adaptation is queued
  with the KeyEvent/TextPayload pre-pin migration. Unrelated to our Arc 3 flagship
  work landing tonight, which is green in isolation. If the resize refactor was not
  meant to alter the mouse-event shape, this may be an unintended regression worth
  a look on your side.

- **2026-07-07 (BLOCKER, qwertty lib does not compile):** `src/correlate.rs` is
  mid-refactor and fails to build standalone — `Reply::DecPrivateMode` and
  `Reply::XtVersion` variants and the `dcs_string`/`osc_string` functions are
  referenced but not defined (`correlate.rs:105,209,213,...`). Because qwertty is
  our path dependency, **this blocks the entire rabbitui workspace from
  compiling** — we cannot build or test anything while it stands. Our committed
  work (through the SSE decoder + Arc 2A spacing/audit) landed while qwertty still
  compiled and is fine; but new work (the Arc 2A gallery example + tapes) is
  authored and staged, unverifiable and uncommitted until this clears. No action
  from us (we never edit qwertty); flagging because it's almost certainly an
  incomplete in-flight commit on your side — a `cargo build -p qwertty --features
  tokio` on main will show it. We'll re-verify and commit the moment it builds.
