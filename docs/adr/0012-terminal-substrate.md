# ADR 0012: qwertty as substrate behind a one-file seam, with an interim encoder

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

rabbitui does not own the terminal. Some library must hold `/dev/tty`, put it in raw mode,
decode the interleaved byte stream (keyboard *and* query replies arrive on one fd), encode
escape sequences, negotiate capabilities, and restore state on every exit path including
panic. Everything above (rendering, layout, input, styling degradation) is shaped by what
that substrate does. The choice is a load-bearing dependency decision, made against a moving
target: the substrate (qwertty) is real but young.

**The 2026 bar is query-based, not terminfo.** The non-negotiable checklist — kitty keyboard
protocol (five progressive flags, separate push/pop stacks per screen), synchronized output
(mode 2026), negotiated grapheme width (mode 2027), truecolor + OSC 10/11 color queries,
in-band resize (mode 2048) with SIGWINCH fallback, mouse/paste/focus, OSC 52, panic-safe
restore — is detected by *querying the terminal*, never terminfo
(docs/research/terminal-substrate.md §"The 2026 substrate bar"). libvaxis "does not use
terminfo; support for vt features is detected through terminal queries"; Helix ships a
hardcoded "is it WezTerm" check because terminfo lies about undercurl (helix#13224).
Terminfo is the losing strategy.

**The query/input race is the chronic wound.** crossterm's `cursor::position()` fails
under PTY forwarding (#963), misreads a lone `ESC` as a keypress because it depends on
`read()` chunking (#993), and forbids combining `EventStream` with `poll`/`read` (docs) —
all symptoms of two consumers of one byte stream with no router. Any substrate that gets
this wrong poisons every query rabbitui will ever make.

**qwertty already solved the hard 80%, and lacks the easy 20%.** Its architecture *already*
solves session ownership, race-free query routing, the encode-only device-free layer, and
honest cleanup — but implements ~20% of the protocol surface a styled TUI needs
(docs/research/terminal-substrate.md §"gap analysis", verified against qwertty commit
c3554fdc). Verified-real: an encode-only `Command`/`CommandBuffer` layer with a raw-bytes
escape hatch; a Tokio session owner that is the sole owner of `/dev/tty`, readiness-driven
with no background reader (`&mut self` serializes queries vs `next_event()` by construction);
race-free typed queries (CPR/DSR) with PTY-tested timeout/late-reply contracts; a stateful
decoder that buffers split UTF-8/CSI and never guesses on a lone ESC. Missing: all
SGR/styling, alt-screen, mouse, paste, focus, kitty keyboard, modes 2026/2027/2048,
OSC/DCS/APC input preservation, a capability model, and panic-without-drop restore. The
candid read: "the risk is not architecture, it's schedule." qwertty is `0.0.0`,
`publish = false`, with `InputEvent` explicitly flagged high-churn.

**Width is a contract, and two tables corrupt frames.** Cell width is negotiated (legacy
wcwidth vs grapheme clustering diverge on emoji/ZWJ — Hashimoto). libvaxis threads one
`width_method` value through cell writing, layout measurement, *and* the render diff
(gwidth.zig; `src/Vaxis.zig:203,459`). If the framework's width table and the substrate's
disagree, cursor positions drift and every frame corrupts — this is Hashimoto's desync trap
(docs/research/libvaxis.md §"Width is negotiated").

## Options considered

**A. Adopt crossterm (ratatui's incumbent).** *Steelman:* mature, published, semver-stable,
huge install base; a crossterm-backed rabbitui would run everywhere ratatui runs today and
inherit its device support for free. *Why not:* it couples input reading and query reading
badly — the #963/#993/#919 failures are architectural, not bugs to be patched, and rabbitui
is a query-heavy design (capability probing, color queries) that would inherit exactly that
wound (docs/research/terminal-substrate.md §"What users complain about"). Its async is a
second-class bolt-on: `EventStream` is "not allowed" to combine with `poll`/`read`, which
collides with a `select!`-based loop. rabbitui keeps crossterm as *insurance only* — the
single-file seam means the interface could be temporarily re-backed by crossterm if qwertty's
P0 slips — but building that adapter before it is needed is not warranted.

**B. Adopt termwiz (WezTerm's library).** *Steelman:* the most complete Rust substrate —
surfaces, cells, diffing, widgets. *Why not:* it bundles exactly what rabbitui intends to
*own* (cell buffers, diffing — ADR 0003), the wrong layering, and its terminfo-first
detection is the strategy every modern reference (libvaxis, termina) abandoned
(docs/research/terminal-substrate.md §"How existing backends compare").

**C. Port/adopt libvaxis, or wait to reimplement it in Rust.** *Steelman:* it is provably
the feature ceiling — the complete 2026 checklist with query-based detection; the negotiation
burst, DA1 fence, negotiated width, and mode-2048-first are all correct and worth stealing
wholesale (docs/research/libvaxis.md). *Why not:* it is Zig on a libxev event loop — not a
Rust library, and its event-loop model isn't a model for Rust. It is a source of *design*,
not a dependency; we steal its patterns into qwertty's requirements handover, not its code.

**D. Adopt qwertty behind a single-file seam, bridge P0 gaps with an interim encoder, hand
over a prioritized requirements list.** *What it is:* the decision below. *Steelman:*
qwertty already has the hard, hard-to-retrofit parts (session ownership, race-free routing,
device-free encode layer, async-first with no background reader — the concurrency answer
crossterm structurally cannot give); everything missing is "more of the same" encode/decode
work; and its alignment means rabbitui's requirements steer it. The raw-bytes escape hatch
means rabbitui is never *blocked* on a missing command family — it emits the bytes itself and
deletes that code module-by-module as qwertty lands each. *Cost:* a git dependency on
unpublished `0.0.0` code with a churning `InputEvent`; schedule risk owned, not wished away.

## Decision

rabbitui adopts **qwertty** as its terminal substrate.

1. **Git-dependency posture.** rabbitui depends on qwertty as a **git dependency pinned to
   `main`**, not a crates.io release, until qwertty publishes. rabbitui explicitly prefers
   semver honesty over stability theater: qwertty may break `InputEvent` (and any pre-0.1
   surface) freely, and rabbitui absorbs the churn. The pin is a specific commit updated
   deliberately, never a floating ref in a release build.

2. **Single-file seam on `TokioTerminalSession`.** All substrate coupling lives in **one
   file** (`terminal.rs`) targeting qwertty's concrete `TokioTerminalSession`. rabbitui does
   **not** define its own runtime-agnostic backend trait now (qwertty's own ADR 0011 defers
   theirs for the same reason). The seam's surface is exactly: open / write+flush /
   `next_event` / typed queries / `leave`. This is the sole place any substrate type name
   appears; the rest of rabbitui sees rabbitui types.

3. **Interim SGR/mode encoder via the raw-bytes escape hatch.** Until qwertty lands the P0
   command families (SGR styling, alt-screen, mode 2026, kitty keyboard enable, mode 2048,
   bracketed paste), rabbitui ships its **own SGR/mode encoding module** that emits escape
   sequences through qwertty's raw-bytes `Command`. This module is structured to be
   **deleted family-by-family** as qwertty lands each. rabbitui renders a frame to a
   `CommandBuffer` (device-free), written and flushed once per frame.

4. **Never fork input decoding.** rabbitui does **not** build a parallel input decoder. It
   consumes qwertty's decoded events and, for sequences qwertty does not yet decode, layers
   its own decoding *only on top of* qwertty's `Undecoded`/`Csi` events — never by
   duplicating the byte-level state machine. qwertty's `InputEvent` is declared high-churn;
   forking it now buys guaranteed rework.

5. **One width oracle, owned by rabbitui.** Grapheme segmentation and width measurement live
   in **one oracle module in rabbitui** (mode-2027-aware, wcwidth fallback), consumed
   identically by layout, text measurement, and the render diff, with the active width method
   selected from the negotiated capability at runtime. qwertty only *sets and queries* mode
   2027 and reports the answer. **There is never a second width table** — the cursor-desync
   trap (Hashimoto).

6. **DA1-fenced capability probing into a `Capabilities` struct.** Capabilities resolve at
   startup via a **single batched probe** (DA1 + XTVERSION + DECRQM for 2026/2027/2048/2004 +
   `CSI ? u` + OSC 10/11), fenced by the **DA1 reply as end-sentinel** under **one timeout**.
   Results populate a `Capabilities` struct (truecolor? kitty kbd? 2026? 2027?…) that styling
   and rendering consume for degradation (truecolor → 256 → 16). It carries **provenance**
   (the raw XTVERSION reply, in which a multiplexer self-identifies) and honors **user
   override env vars** (libvaxis's `VAXIS_FORCE_*` precedent). Batching is mandatory: under
   tmux ≤3.5, DECRQM and the kitty query get *no reply at all*, so N sequential timeouts stall
   once per swallowed probe, while the DA1 fence pays one timeout at most — zero on tmux, which
   answers DA1 itself (docs/research/terminal-substrate.md §"The multiplexer middle-box").

7. **Requirements-handover process.** rabbitui maintains a **prioritized requirements list**
   for qwertty (P0 blocking styled apps → P1 first-releases → P2 wanted), duplicated verbatim
   at `work/qwertty/substrate-requirements.md` for the qwertty effort to disposition in its
   `substrate-status.md §6`. Contract asks are explicit: keep the split (nothing cell-buffer-
   or widget-shaped in qwertty), keep the encode layer device-free and `tokio`-optional, keep
   "framework owns the event loop" (awaitable primitives, no spawned tasks), and keep
   multiplexer *policy* out of qwertty — qwertty reports what the innermost terminal answered
   verbatim; mux detection and overrides are rabbitui's layer. When qwertty declines an item,
   rabbitui covers it via the raw-bytes escape hatch and its decode-on-top layer.

## Consequences

**Positive.**
- Async is first-class without apology: qwertty's readiness-driven, no-background-reader
  session makes rabbitui's loop a plain `tokio::select!` (ADR 0005) — the answer crossterm
  cannot give. Rendering to a device-free `CommandBuffer` yields snapshot tests on emitted
  escape sequences for free (feeds ADR 0009).
- rabbitui is never *blocked* on a substrate gap: the escape hatch makes any missing family
  emit-it-ourselves-now, delete-later. The single-file seam localizes schedule risk and keeps
  a crossterm fallback cheap.
- Requirements handover gives rabbitui real influence over an aligned substrate — the inverse
  of consuming a frozen third-party library.

**Negative (honest).**
- rabbitui depends on unpublished `0.0.0` code with ADRs under review and a churning
  `InputEvent`; a `main`-pinned git dependency means CI can break on an upstream commit, and
  no crates.io release is possible until qwertty publishes.
- The interim encoder is real effort spent on code designed to be thrown away, plus a window
  where SGR/mode correctness is rabbitui's problem, not the substrate's.
- Two projects must stay coordinated: a qwertty P0 slip (kitty keyboard, mode 2026) delays
  rabbitui's P0; the bus-factor and velocity of a young substrate are now rabbitui's risk.
- Windows is deferred (qwertty is Unix-only), though the ConPTY-VT-only path (termina, Win10
  1809+) changes no rabbitui architecture.

**Neutral.**
- Grapheme/unicode-data dependencies live in rabbitui, not qwertty — a deliberate placement;
  libvaxis's own dependency churn now sits on rabbitui's side.
- Multiplexer degradation runs *constantly*, not rarely: a large share of real sessions are
  inside tmux ("2026/2031 yes on 3.7+, kitty keyboard / 2027 / 2048 no"), so the
  `Capabilities` struct's provenance and overrides are core paths, not edge cases.

## Revisit triggers

- **qwertty P0 slips past rabbitui's P0 window** — kitty keyboard, SGR, alt-screen, mode
  2026, or in-band resize not landing in time to unblock rabbitui's first styled release.
  Activate the crossterm insurance adapter behind the same one-file seam.
- **qwertty pivots architecturally** — abandons the encode-only device-free layer, the
  no-background-reader session, or "framework owns the loop." Any breaks the seam's premise
  and reopens substrate choice.
- **The seam stops being one file** — substrate types leaking beyond `terminal.rs` collapse
  both the fallback-adapter and handover posture; a regression to fix on sight.
- **A second width table appears anywhere** (a widget or the substrate measuring
  independently of the oracle) — the cursor-desync trap; close on sight.
- **qwertty publishes with a stable `InputEvent`** (positive) — retire the git-dependency
  posture, pin a semver range, and reassess which interim-encoder modules can be deleted.
- **A published Rust substrate reaches the 2026 bar** (a hypothetical libvaxis-in-Rust with a
  correct query router and async model) — re-price build-vs-adopt if it can match the
  alignment and influence rabbitui gets from qwertty.
