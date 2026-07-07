# Terminal substrate: the 2026 bar, the field, and qwertty

**Verdict:** qwertty's architecture already solves the problems the incumbents get wrong (session
ownership, race-free query routing, encode-only layer, honest cleanup) but implements roughly 20% of
the protocol surface a styled TUI needs; rabbitui should adopt it behind a one-file seam, hand over
the requirements list below, and design as if the full surface arrives — because the hard 80% of a
substrate is the part qwertty already has.

Date: 2026-07-06

**Sources**

Web (fetched):

- Kitty keyboard protocol spec — <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>
- Synchronized output (mode 2026) spec —
  <https://gist.github.com/christianparpart/d8a62cc1ab659194337d73e399004036>
- In-band resize (mode 2048) spec —
  <https://gist.github.com/rockorager/e695fb2924d36b2bcf1fff4a3704bd83>
- Terminal Unicode Core (mode 2027) — <https://github.com/contour-terminal/terminal-unicode-core>
- Mitchell Hashimoto on grapheme clusters in terminals —
  <https://mitchellh.com/writing/grapheme-clusters-in-terminals>
- libvaxis README (feature matrix, "does not use terminfo") —
  <https://github.com/rockorager/libvaxis>
- crossterm README / event docs — <https://github.com/crossterm-rs/crossterm> ;
  <https://docs.rs/crossterm/latest/crossterm/event/index.html>
- termina README — <https://github.com/helix-editor/termina>
- Helix "Switch terminal backend from Crossterm to Termina" —
  <https://github.com/helix-editor/helix/pull/13307>
- Helix terminfo/undercurl workaround PR — <https://github.com/helix-editor/helix/pull/13224>
- termwiz docs — <https://docs.rs/termwiz/latest/termwiz/>
- crossterm cursor-position failures — <https://github.com/crossterm-rs/crossterm/issues/963> ,
  <https://github.com/crossterm-rs/crossterm/issues/993> ,
  <https://github.com/crossterm-rs/crossterm/issues/919>
- ratatui panic-hooks recipe — <https://ratatui.rs/recipes/apps/panic-hooks/>
- Mode 2027 adoption: wezterm issue — <https://github.com/wezterm/wezterm/issues/4320> ; foot
  implementation — <https://codeberg.org/dnkl/foot/pulls/1489>
- tmux CHANGES (versioned protocol support) —
  <https://raw.githubusercontent.com/tmux/tmux/master/CHANGES> ; tmux input.c (CSI dispatch table,
  DA1/DA2/XTVERSION/DECRQM replies) — <https://github.com/tmux/tmux/blob/master/input.c>
- tmux kitty-keyboard feature requests (open) — <https://github.com/tmux/tmux/issues/3335> ,
  <https://github.com/tmux/tmux/issues/4158> ; allow-passthrough —
  <https://man7.org/linux/man-pages/man1/tmux.1.html>
- notcurses termdesc.c (tmux DA2 id, GNU screen OSC forwarding) —
  <https://github.com/dankamongmen/notcurses/blob/master/src/lib/termdesc.c>
- zellij 0.41 kitty-keyboard adoption — <https://zellij.dev/news/colliding-keybinds-plugin-manager/>
  ; Helix-in-zellij regression — <https://github.com/helix-editor/helix/issues/14392>

Local (read):

- /Users/joshka/local/rabbitui/work/qwertty/substrate-status.md (qwertty commit c3554fdc,
  2026-07-06) — consumed; present as promised
- /Users/joshka/local/qwertty/README.md ; docs/architecture.md ; docs/roadmap.md
- /Users/joshka/local/qwertty/src/input.rs ; src/tokio_session.rs (Drop at :540) ; src/session.rs ;
  src/command.rs ; src/commands/{cursor,screen,terminal}.rs
- /Users/joshka/local/qwertty/docs/adr/ (0001 async-first, 0011 tokio boundary, 0012 query routing,
  0013 platform policy, 0017/0018 release)

## The 2026 substrate bar

What a terminal backend must handle in 2026, and why each item is non-negotiable:

1. **Kitty keyboard protocol.** Five progressive-enhancement flags (1 disambiguate, 2 event types, 4
   alternate keys, 8 report-all-as-escape-codes, 16 associated text), queried with `CSI ? u`,
   managed as a push/pop stack with _separate stacks for main and alternate screens_. Implemented by
   alacritty, foot, ghostty, iTerm2, Windows Terminal, WezTerm
   ([spec](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)). Without it you cannot distinguish
   `Esc` from `Alt+key`, get key release/repeat, or bind `Ctrl+i` separately from `Tab`.
2. **Synchronized output (mode 2026).** `CSI ? 2026 h/l` around each frame; terminal buffers and
   applies atomically. Detect via DECRQM `CSI ? 2026 $ p` → reply `CSI ? 2026 ; <1|2> $ y` means
   supported ([spec](https://gist.github.com/christianparpart/d8a62cc1ab659194337d73e399004036)).
   This is the modern answer to tearing; a diffing renderer without it flickers on slow links.
3. **Unicode width / mode 2027 / graphemes.** The width of a cell's contents is _negotiated_, not
   fixed: legacy wcwidth vs grapheme clustering diverge on emoji/ZWJ
   ([Hashimoto](https://mitchellh.com/writing/grapheme-clusters-in-terminals)). Mode 2027
   (DECSET/DECRQM 2027) lets the app and terminal agree on grapheme clustering; foot, contour,
   wezterm-tracked, ghostty implement or force it. A substrate must be able to set/query the mode;
   the _framework_ must own width tables and measure per the negotiated mode, or cursor desync
   corrupts every frame.
4. **Truecolor + color queries.** 24-bit SGR is table stakes; OSC 10/11 fg/bg queries are how you
   detect dark/light mode, and mode 2031 gives change notifications (libvaxis supports all of
   these).
5. **Queries and response routing as a first-class problem.** Modern capability detection is
   query-based, not terminfo: send DA1/XTVERSION/DECRQM/`CSI ? u`/OSC probes at startup, use the DA1
   reply as the "end of probes" sentinel. libvaxis states flatly it "does not use terminfo; support
   for vt features is detected through terminal queries." Terminfo is stale in practice — Helix
   ships a hardcoded "is it WezTerm" check because terminfo lies about undercurl
   ([helix#13224](https://github.com/helix-editor/helix/pull/13224)). The killer design constraint:
   query replies arrive _interleaved with keyboard input on the same fd_, so the input decoder and
   query router must be one component.
6. **Mouse (SGR 1006 + 1002/1003), bracketed paste (2004, aggregated into one event), focus events
   (1004), OSC 52 clipboard, OSC 8 hyperlinks, cursor shape (DECSCUSR), title (OSC 0/2).** All table
   stakes; libvaxis has every one.
7. **Resize: in-band (mode 2048) preferred, SIGWINCH fallback.** Signals are "inherently racy" and
   don't traverse serial/ssh reliably; mode 2048 delivers `CSI 48;h;w;hp;wp t` in the data stream
   ([spec](https://gist.github.com/rockorager/e695fb2924d36b2bcf1fff4a3704bd83)).
8. **Panic-safe restore.** Raw mode + alt screen + kitty flags + mouse must be undone on panic or
   the user's shell is wrecked; ratatui documents that every app must install panic hooks itself
   ([recipe](https://ratatui.rs/recipes/apps/panic-hooks/)). A 2026 substrate should make correct
   restore the default, including a restore path that works from a panic hook without owning the
   session.
9. **Windows reality check.** The old Console API is dead weight. termina's answer is the right one:
   require Windows 10 1809+ ConPTY and make Windows "speak VT" so one code path serves all platforms
   ([termina](https://github.com/helix-editor/termina),
   [helix#13307](https://github.com/helix-editor/helix/pull/13307)). OSC 52 and bracketed paste then
   work on Windows for free.

### The multiplexer middle-box

tmux, screen, and zellij are terminal _emulators_ sitting in the middle: they parse everything a
pane writes, act on what they recognize, and drop what they don't — unrecognized sequences are not
forwarded to the outer terminal. Every P0/P1 probe above changes meaning under a mux:

- **Queries tmux answers itself:** DA1 (`CSI ?1;2c`), DA2 (`CSI >84;0;0c`), XTVERSION
  (`DCS >|tmux <ver> ST`) — all replied from tmux's own dispatch table
  ([input.c](https://github.com/tmux/tmux/blob/master/input.c)). Two consequences: a DA1-fenced
  probe bundle still terminates promptly under tmux, and the XTVERSION reply _positively identifies_
  the mux.
- **DECRQM is tmux-version-dependent:** tmux ≤3.5 doesn't answer DECRQM at all — each probe just
  times out; 3.6 added replies for ?12/?1004/?1006/?2004; 3.7 answers 2026 "and various others" and
  replies "not recognized" (0) for unknown modes
  ([CHANGES](https://raw.githubusercontent.com/tmux/tmux/master/CHANGES), input.c
  `INPUT_CSI_QUERY_PRIVATE`). tmux 3.7 genuinely supports modes 2026 and 2031; 2027 and 2048
  correctly report unsupported.
- **Kitty keyboard is swallowed:** `CSI ? u` and the push/pop sequences aren't in tmux's CSI table
  (bare `u` is restore-cursor), so they're dropped with no reply; support is a long-open feature
  request ([#3335](https://github.com/tmux/tmux/issues/3335),
  [#4158](https://github.com/tmux/tmux/issues/4158)) — tmux 3.5 offers xterm modifyOtherKeys /
  `extended-keys-format csi-u` instead. A DA1-fenced probe degrades correctly to "unsupported"; a
  per-query-timeout design stalls once per swallowed probe.
- **Stale outer-terminal answers are real:** GNU screen forwards OSC 10/11 to the underlying
  terminal instead of answering itself — notcurses sends those queries _first_ because the reply
  round-trips through the outer terminal
  ([termdesc.c](https://github.com/dankamongmen/notcurses/blob/master/src/lib/termdesc.c)). tmux
  ≥3.4 answers OSC 10/11 itself, returning the first attached client's colors (CHANGES).
- **Passthrough doesn't rescue probing:** `allow-passthrough` (default off since it was gated in
  tmux 3.3) lets a pane tunnel bytes to the outer terminal via `DCS tmux; … ST` with doubled ESCs
  ([tmux(1)](https://man7.org/linux/man-pages/man1/tmux.1.html)). It is outbound-only — nothing
  routes the outer terminal's reply back to the querying pane — so it suits OSC 52 writes and
  images, not capability queries.
- **Muxes diverge per capability:** zellij implements the kitty keyboard protocol itself
  ([0.41 announcement](https://zellij.dev/news/colliding-keybinds-plugin-manager/)), so "under a
  mux" ≠ "no modern features"; and Helix's termina migration broke Cmd-modifier keys inside zellij
  ([helix#14392](https://github.com/helix-editor/helix/issues/14392)) — mux keyboard interop is
  where backends regress in practice.
- **What the reference libraries do:** libvaxis has _no_ mux-specific code — its whole answer is the
  DA1 fence plus env overrides for known liars (`VHS_RECORD`, `TERM_PROGRAM=vscode`,
  `VAXIS_FORCE_WCWIDTH`/`_UNICODE` in `Vaxis.zig` `enableDetectedFeatures`). notcurses identifies
  tmux via DA2 84, and its source admits `// FIXME what, oh what to do with tmux?` (termdesc.c).
  Nobody has a better story than: probe honestly, fence with DA1, trust the mux's answers as the
  innermost terminal's truth, let users override.

## How existing backends compare

| Capability            | crossterm                                       | termwiz           | termina                       | libvaxis (Zig)            |
| --------------------- | ----------------------------------------------- | ----------------- | ----------------------------- | ------------------------- |
| Kitty keyboard        | Yes (`PushKeyboardEnhancementFlags`)            | Partial           | Yes                           | Yes                       |
| Mode 2026             | Command exists; detection DIY                   | No first-class    | Exposed, app detects          | Yes, auto-detected        |
| Mode 2027 / graphemes | No                                              | Own width tables  | App-level                     | Yes                       |
| Capability detection  | Ad-hoc per-feature                              | terminfo          | Queries, app-routed           | Queries only, no terminfo |
| Query/input routing   | Broken (see below)                              | Internal          | poll/read exposed, app routes | Internal, correct         |
| Mouse/paste/focus     | Yes                                             | Yes               | Yes                           | Yes                       |
| OSC 52                | No (apps DIY)                                   | Yes               | Yes (incl. Windows)           | Yes                       |
| Windows               | Win7+, dual code paths                          | Yes (heavy)       | ConPTY VT-only, Win10 1809+   | No                        |
| Panic restore         | DIY panic hooks                                 | DIY               | DIY                           | App-owned                 |
| Async story           | `EventStream` bolt-on, exclusive with poll/read | Own executor bits | Sync poll/read core           | libxev event loop         |

- **crossterm** is the incumbent (ratatui's default) with the broadest legacy-Windows support, but
  its architecture couples input reading and query reading badly.
- **termwiz** (WezTerm's library) is the maximalist: surfaces, cells, diffing, widgets,
  terminfo-driven caps. It bundles what rabbitui wants to own (cell buffers, diffing) — wrong
  layering for us, and terminfo-first detection is the losing strategy.
- **termina** (Helix, by the-mikedavis) is "a cross between Crossterm and TermWiz with a lower level
  API which exposes escape codes." Its thesis — expose the escape layer, let the app route queries
  and adopt new protocols without waiting on the library — is the closest existing philosophy to
  qwertty's, and Helix adopted it precisely to detect kitty keyboard and 2026 "simultaneously" with
  reading input ([#13307](https://github.com/helix-editor/helix/pull/13307)).
- **libvaxis** is the feature ceiling: the complete 2026 checklist (2026/2027/2031/2048, OSC
  8/9/22/52/777, kitty graphics, explicit width) with query-based detection. It's the reference for
  _what_ to support; its Zig/libxev event loop isn't a model for a Rust library.

## What users complain about

- **The query/input race is crossterm's chronic wound.** `cursor::position()` fails "within a normal
  duration" under PTY forwarding ([#963](https://github.com/crossterm-rs/crossterm/issues/963)); the
  parser misreads a lone `ESC` byte as a keypress because it depends on `read()` syscall chunking
  ([#993](https://github.com/crossterm-rs/crossterm/issues/993)); position queries fail when stdout
  is piped ([#919](https://github.com/crossterm-rs/crossterm/issues/919)). Root cause: two consumers
  of one byte stream without a router.
- **Async is a second-class bolt-on.** crossterm docs: it is "not allowed" to combine `EventStream`
  with `poll`/`read` or call them from different threads
  ([docs](https://docs.rs/crossterm/latest/crossterm/event/index.html)) — so a query API that
  internally does poll/read silently conflicts with an app's event stream.
- **Panic cleanup is everyone's problem but the library's.** The ratatui book has a whole recipe
  because a panicking app "leaves the terminal in a modified, unusable state"
  ([panic hooks](https://ratatui.rs/recipes/apps/panic-hooks/)).
- **terminfo is distrusted.** Helix hardcodes a WezTerm check for undercurl because terminfo entries
  are wrong/stale ([#13224](https://github.com/helix-editor/helix/pull/13224)); libvaxis abandoned
  terminfo entirely.
- **Protocol lag pushes serious apps off crossterm.** Helix's stated reason for termina: crossterm's
  high-level events hide the escape layer, so adopting kitty/2026/OSC 52 semantics meant forking
  behavior; termina "pushes all of that handling to the application"
  ([#13307](https://github.com/helix-editor/helix/pull/13307)).

## qwertty gap analysis

Source of truth: the qwertty effort's own status memo at
`/Users/joshka/local/rabbitui/work/qwertty/substrate-status.md` (present, dated today, commit
c3554fdc), cross-checked against source. It is candid and accurate.

**What's real and verified in source:**

- Encode-only, device-free command layer (`Command`, `CommandBuffer`,
  `commands::{cursor,screen,terminal}` — src/command.rs, src/commands/) with a raw-bytes escape
  hatch. rabbitui can render a frame to bytes with zero device coupling.
- A Tokio session owner (src/tokio_session.rs) that is the _sole_ owner of `/dev/tty`,
  readiness-driven on the caller's runtime — no spawned reader task, no channels, no globals.
  `&mut self` on everything serializes queries vs. `next_event()` by construction.
- Race-free typed queries (cursor position, DSR status) with tested
  timeout/cancellation/wrong-report/late-reply/preserved-input contracts — this is exactly the
  crossterm #963/#993 failure mode, solved and PTY-tested.
- Stateful input decoder (src/input.rs) that buffers split UTF-8 and CSI across chunks and never
  guesses on a lone ESC (`InputDecoder::finish`, src/input.rs:105-119) — the #993 bug is
  structurally impossible.
- `leave()` reports cleanup errors; `Drop` best-effort restores cooked mode
  (src/tokio_session.rs:540).

**The gaps (substrate-status.md §1, §5, confirmed against src/):**

- **No styling at all.** No SGR, no truecolor, no underline styles. Today a styled TUI is only
  possible via the raw-bytes escape hatch.
- **No alternate screen, mouse, paste, focus, kitty keyboard, title, OSC 52, scroll regions, mode
  2026/2027/2048.** Input decoding is text + C0 + arrows + preserved CSI; the `InputEvent` model is
  flagged high-churn.
- **No OSC/DCS/APC input preservation** — the decoder gap that _bounds which queries can ever exist_
  (color queries reply via OSC). This is the single most leveraged decoder fix.
- **No signal integration** (SIGWINCH/SIGTSTP/SIGCONT), no resize events, no suspend/handoff.
- **No capability model**, no DA1/XTGETTCAP, no Windows (Unix-only via rustix; ConPTY undecided).
- **Drop restore doesn't cover panic-without-drop** (e.g. abort), and there's no panic-hook-friendly
  restore handle.
- **Headless testing is PTY-only**: `open_path` accepts any tty path, but there's no in-memory
  device trait.

**Read on trajectory:** the division of responsibility is already agreed (qwertty owns
bytes/protocol/session/input decode; rabbitui owns buffers/diff/widgets/layout), the query-routing
and session architecture are the hard-to-retrofit parts and they're done, and everything missing is
"more of the same" encoding/decoding work. The risk is not architecture, it's schedule: unpublished
(0.0.0, `publish = false`), all ADRs under design review, `InputEvent` explicitly expected to
change.

## Requirements handover

_(This section is duplicated verbatim at
`/Users/joshka/local/rabbitui/work/qwertty/substrate-requirements.md` for the qwertty effort to
disposition per substrate-status.md §6.)_

rabbitui's substrate requirements on qwertty, by priority. "Frame loop" below means: rabbitui owns a
`select!` loop over `next_event()`, timers, and app messages; renders a full frame to a
`CommandBuffer`; writes + flushes once per frame.

**P0 — blocking any styled rabbitui app:**

1. **SGR styling commands**: 16/256/truecolor fg+bg, bold/dim/italic/reverse/strikethrough,
   underline styles (incl. curly + colored underline, SGR 4:3 / 58), reset. Encode-only, same shape
   as `commands::cursor`.
2. **Alternate screen** enter/leave (mode 1049) integrated into session lifecycle and `leave()`/Drop
   restore.
3. **Synchronized output**: `commands::screen::{begin_sync, end_sync}` (mode 2026) — rabbitui wraps
   every frame; plus DECRQM 2026 detection (see 10).
4. **Kitty keyboard**: push flags on enter (rabbitui wants at least 1|2 = disambiguate + event
   types; make flags caller-chosen), pop on leave and on suspend, `CSI ? u` detection, full
   `CSI ... u` decode (keycode, modifiers, event type, text), and decoded modifiers for legacy
   CSI/SS3 sequences (F-keys, Home/End, `CSI 1;5A`-style). Key release/repeat surfaced in the event
   type.
5. **Resize as events**: mode 2048 in-band resize decoded (`CSI 48;h;w;hp;wp t`), SIGWINCH fallback
   routed through `next_event()`. Snapshot `size()` is not enough for a frame loop.
6. **Bracketed paste** (mode 2004): enable/disable commands + paste aggregated into one event (not
   per-byte).
7. **Panic-safe restore**: a cheap handle (fd + saved termios + "modes to undo" list) obtainable
   from the session, usable from a panic hook without `&mut` session access, that restores cooked
   mode, main screen, and pops kitty flags/mouse/paste modes. Best-effort, signal-safe-ish,
   documented.
8. **OSC/DCS/APC input preservation** in the decoder (substrate-status.md names this the main
   decoder gap) — prerequisite for 10 and 12.

**P1 — needed within rabbitui's first releases:** 9. **Mouse**: SGR encoding (1006) +
button/any-event tracking (1002/1003) enable/disable, decoded to typed events (kind, button,
position, modifiers). 10. **Capability probe bundle**: one call that sends DA1 + XTVERSION +
DECRQM(2026, 2027, 2048, 2004) + `CSI ? u` + OSC 10/11 (fg/bg color), uses the DA1 reply as
end-sentinel with a single timeout, and returns a typed capability struct **that includes the raw
XTVERSION reply string** (a mux self-identifies there: `tmux <ver>`). Single-query-at-a-time
(`&mut self`) is acceptable _only if_ probing is batched like this; N sequential timeouts at startup
is not — under tmux ≤3.5 DECRQM and the kitty query get _no reply at all_ (see "The multiplexer
middle-box"), so unbatched probing stalls once per swallowed query while the DA1 fence pays one
timeout at most, and zero on tmux (which answers DA1 itself). 11. **Focus events** (mode 1004)
decoded. 12. **OSC 52 clipboard** write (read optional, policy-gated). 13. **Cursor shape**
(DECSCUSR) and **window title** (OSC 2, ideally XTWINOPS 22/23 push/pop). 14. **Suspend/handoff**:
SIGTSTP/SIGCONT raw-mode dance and an API to temporarily release the terminal (run `$EDITOR`, then
re-enter) — recurring real-world need per crossterm's tracker. 15. **In-memory device seam**: a
substitutable read/write device (trait or equivalent) so rabbitui can run headless integration tests
with an in-memory fake — scripted input bytes in, emitted bytes out — without PTY plumbing.
Highest-value seam for rabbitui's testing story; PTY-path injection (`open_path`) stays as the
integration-level option. 16. **Mode 2027 set + DECRQM query** (grapheme clustering). rabbitui owns
width tables and measurement; qwertty only negotiates the mode and reports the answer in the
capability struct.

**P2 — wanted, not gating:** 17. **Windows via ConPTY VT-only** (termina precedent: Win10 1809+, one
VT code path, no legacy Console API surface in the API). 18. **Scroll regions** (DECSTBM) —
potential renderer optimization for scrolling panes. 19. **OSC 8 hyperlinks** in the styling
surface. 20. **Underline color / mode 2031 color-scheme change notifications** — nice-to-have once
OSC routing (8) exists.

**Contract asks (not features):**

- Keep the agreed split: nothing cell-buffer- or widget-shaped in qwertty; rabbitui will not ask for
  diffing.
- Keep the encode-only layer device-free and importable without the `tokio` feature — rabbitui's
  renderer tests depend on it.
- Keep "framework owns the event loop": awaitable primitives only, no spawned tasks, no forced actor
  shape (current model is exactly right).
- Semver honesty over stability theater: rabbitui will track a git dependency on `main` behind a
  single-file seam; breaking `InputEvent` freely _before_ 0.1.0 is preferred to freezing it wrong.
- When an item here is declined/deferred, say so in substrate-status.md §6 — rabbitui will cover
  declined items via the raw-bytes escape hatch and its own decoder layer on top of
  `Undecoded`/`Csi` events.
- **Multiplexer policy stays out of qwertty (explicit deferral).** qwertty reports what the
  innermost terminal answered, verbatim — if that terminal is tmux, tmux's answers _are_ the truth
  about the terminal the app is drawing to. Mux _detection_ (`$TMUX`/`$ZELLIJ`, XTVERSION string)
  and user/env capability overrides (libvaxis's `VAXIS_FORCE_*` precedent) are rabbitui's layer on
  top of the capability struct. rabbitui does not request `DCS tmux;` passthrough support:
  passthrough is outbound-only and cannot carry probes.

## Implications for rabbitui

- **Adopt qwertty as the substrate, but behind one file.** Define `rabbitui-backend`'s internal seam
  as substrate-status.md §2 recommends: open / write+flush / next_event / typed queries / leave.
  Target `TokioTerminalSession` concretely; do not design our own runtime-agnostic trait now
  (qwertty's ADR 0011 defers theirs for the same reason).
- **Be async-first without apology.** qwertty is readiness-driven on the caller's runtime with no
  background reader; that makes rabbitui's event loop a plain `tokio::select!` over terminal events,
  timers, and app messages — the concurrency answer crossterm's exclusive `EventStream`-vs-`poll`
  split can't give. Sync/blocking rabbitui is not worth a second code path.
- **Render to `CommandBuffer`, not to a Writer.** The encode-only layer is confirmed device-free, so
  rabbitui's renderer output type is "bytes for a frame". This gives snapshot tests on emitted
  escape sequences for free and keeps the renderer testable with zero PTY.
- **Bridge the gap with an interim style/mode encoder inside rabbitui.** P0 items 1-6 don't exist
  yet; rabbitui can ship against the raw-bytes escape hatch (`Command` wraps arbitrary bytes) with
  its own SGR/mode encoding module, structured to delete module-by-module as qwertty lands each
  command family. Do not fork input decoding the same way — wait for qwertty's keyboard work (its
  `InputEvent` is declared high-churn; building on it now buys rework).
- **Own unicode width in rabbitui, negotiate it via qwertty.** Layout and text measurement need
  grapheme segmentation + width tables (mode-2027-aware, wcwidth fallback) in the framework; the
  substrate only sets/queries mode 2027. Never let two width tables exist (this is Hashimoto's
  cursor-desync trap).
- **Design the frame protocol around mode 2026 + damage-agnostic full-frame diffing.** With sync
  output wrapping each flush, the diff-vs-damage-regions debate loses urgency: correctness comes
  from atomic frames, bandwidth from cell diffing. Decide diffing on bandwidth numbers, not tearing.
- **Make headless testing a stated requirement now.** The in-memory device seam (P1 #15) is
  explicitly the kind of item qwertty's design review wants from us; until it lands, rabbitui's
  integration harness uses `open_path` + PTY pairs (qwertty's own tests prove the pattern) and unit
  tests feed `InputDecoder` directly.
- **Plan capability-driven degradation from day one.** Widgets and styling must consume a
  `Capabilities` struct (truecolor? kitty kbd? 2026? 2027?) resolved at startup by the probe bundle
  — the libvaxis model, not terminfo. This decision leaks into the styling system design (theme
  colors need 256/16 downsampling paths).
- **Design for muxes as first-class terminals, not edge cases.** A large share of real sessions run
  inside tmux, where the honest capability answer today is "2026 and 2031 yes (3.7+), kitty keyboard
  / 2027 / 2048 no" — degradation paths will be exercised constantly, not rarely. The `Capabilities`
  struct needs provenance (the XTVERSION reply names the mux) and user-facing override env vars
  (libvaxis's `VAXIS_FORCE_*` precedent), because mux answers change per tmux version and zellij ≠
  tmux (zellij does kitty keyboard). Never probe the outer terminal through passthrough; trust the
  middle-box.
- **Windows is post-first-release, and that's acceptable.** qwertty is Unix-only and undecided on
  Windows; the credible path is termina's ConPTY-VT-only approach, which changes nothing about
  rabbitui's architecture (same VT bytes). State the floor (Win10 1809+) in rabbitui's docs now so
  no design decision assumes the legacy Console API.
- **Schedule risk is real: keep a crossterm adapter as insurance only if the seam stays one file.**
  qwertty is 0.0.0 with ADRs under re-litigation. The single-file seam means rabbitui could
  temporarily back the same interface with crossterm if qwertty's P0 slips — but don't invest in
  that adapter until it's actually needed.
