# rabbitui → qwertty: substrate requirements

*(Verbatim copy of the "Requirements handover" section of rabbitui's terminal-substrate research memo — `/Users/joshka/local/rabbitui/work/default/docs/research/terminal-substrate.md` — for disposition per substrate-status.md §6.)*

rabbitui's substrate requirements on qwertty, by priority. "Frame loop" below means: rabbitui owns a `select!` loop over `next_event()`, timers, and app messages; renders a full frame to a `CommandBuffer`; writes + flushes once per frame.

**P0 — blocking any styled rabbitui app:**
1. **SGR styling commands**: 16/256/truecolor fg+bg, bold/dim/italic/reverse/strikethrough, underline styles (incl. curly + colored underline, SGR 4:3 / 58), reset. Encode-only, same shape as `commands::cursor`.
2. **Alternate screen** enter/leave (mode 1049) integrated into session lifecycle and `leave()`/Drop restore.
3. **Synchronized output**: `commands::screen::{begin_sync, end_sync}` (mode 2026) — rabbitui wraps every frame; plus DECRQM 2026 detection (see 10).
4. **Kitty keyboard**: push flags on enter (rabbitui wants at least 1|2 = disambiguate + event types; make flags caller-chosen), pop on leave and on suspend, `CSI ? u` detection, full `CSI ... u` decode (keycode, modifiers, event type, text), and decoded modifiers for legacy CSI/SS3 sequences (F-keys, Home/End, `CSI 1;5A`-style). Key release/repeat surfaced in the event type.
5. **Resize as events**: mode 2048 in-band resize decoded (`CSI 48;h;w;hp;wp t`), SIGWINCH fallback routed through `next_event()`. Snapshot `size()` is not enough for a frame loop.
6. **Bracketed paste** (mode 2004): enable/disable commands + paste aggregated into one event (not per-byte).
7. **Panic-safe restore**: a cheap handle (fd + saved termios + "modes to undo" list) obtainable from the session, usable from a panic hook without `&mut` session access, that restores cooked mode, main screen, and pops kitty flags/mouse/paste modes. Best-effort, signal-safe-ish, documented.
8. **OSC/DCS/APC input preservation** in the decoder (substrate-status.md names this the main decoder gap) — prerequisite for 10 and 12.

**P1 — needed within rabbitui's first releases:**
9. **Mouse**: SGR encoding (1006) + button/any-event tracking (1002/1003) enable/disable, decoded to typed events (kind, button, position, modifiers).
10. **Capability probe bundle**: one call that sends DA1 + XTVERSION + DECRQM(2026, 2027, 2048, 2004) + `CSI ? u` + OSC 10/11 (fg/bg color), uses the DA1 reply as end-sentinel with a single timeout, and returns a typed capability struct. Single-query-at-a-time (`&mut self`) is acceptable *only if* probing is batched like this; N sequential timeouts at startup is not.
11. **Focus events** (mode 1004) decoded.
12. **OSC 52 clipboard** write (read optional, policy-gated).
13. **Cursor shape** (DECSCUSR) and **window title** (OSC 2, ideally XTWINOPS 22/23 push/pop).
14. **Suspend/handoff**: SIGTSTP/SIGCONT raw-mode dance and an API to temporarily release the terminal (run `$EDITOR`, then re-enter) — recurring real-world need per crossterm's tracker.
15. **In-memory device seam**: a substitutable read/write device (trait or equivalent) so rabbitui can run headless integration tests with an in-memory fake — scripted input bytes in, emitted bytes out — without PTY plumbing. Highest-value seam for rabbitui's testing story; PTY-path injection (`open_path`) stays as the integration-level option.
16. **Mode 2027 set + DECRQM query** (grapheme clustering). rabbitui owns width tables and measurement; qwertty only negotiates the mode and reports the answer in the capability struct.

**P2 — wanted, not gating:**
17. **Windows via ConPTY VT-only** (termina precedent: Win10 1809+, one VT code path, no legacy Console API surface in the API).
18. **Scroll regions** (DECSTBM) — potential renderer optimization for scrolling panes.
19. **OSC 8 hyperlinks** in the styling surface.
20. **Underline color / mode 2031 color-scheme change notifications** — nice-to-have once OSC routing (8) exists.

**Contract asks (not features):**
- Keep the agreed split: nothing cell-buffer- or widget-shaped in qwertty; rabbitui will not ask for diffing.
- Keep the encode-only layer device-free and importable without the `tokio` feature — rabbitui's renderer tests depend on it.
- Keep "framework owns the event loop": awaitable primitives only, no spawned tasks, no forced actor shape (current model is exactly right).
- Semver honesty over stability theater: rabbitui will track a git dependency on `main` behind a single-file seam; breaking `InputEvent` freely *before* 0.1.0 is preferred to freezing it wrong.
- When an item here is declined/deferred, say so in substrate-status.md §6 — rabbitui will cover declined items via the raw-bytes escape hatch and its own decoder layer on top of `Undecoded`/`Csi` events.

## Field findings from rabbitui implementation (2026-07-06, slices 0-3)

Concrete, encountered-in-code findings, in priority order for the framework:

1. **Ctrl-C arrives as `ControlInput::Other(0x03)`** — an unnamed variant every
   app must know the byte for. Naming the common C0 controls (ETX at minimum;
   also SUB/0x1a, EOT/0x04) would remove a papercut every consumer hits in
   their first hour. (rabbitui maps it in its facade today.)
2. **No resize events** — rabbitui polls `size()` before every frame. SIGWINCH
   or mode 2048 routing is the single most-wanted event. (Was already P0 in
   the original list; implementation confirms it's felt immediately.)
3. **Key vocabulary ceiling**: rabbitui's core defines BackTab, Home/End,
   PageUp/Down, Delete, and modifier-carrying keys, but qwertty's decoder
   never produces them (arrows only). These arrive as preserved CSI that the
   framework must not parse (per the no-forking-input-decoding rule), so the
   decoder's key coverage directly caps framework capability. Kitty-protocol
   negotiation would subsume most of this.
4. **Mode 2026 emitted blind**: rabbitui brackets every frame in synchronized
   output with no way to know if the terminal honors it (harmless if not, but
   capability probing would let the renderer report degradation honestly).
5. **Confirmed good**: the encode-only `CommandBuffer` + raw-bytes escape hatch
   works exactly as promised — the whole interim SGR/alt-screen/2026 encoder
   lives on it with no friction; ordered write + explicit flush semantics are
   pleasant to build a frame renderer on; `&mut self` exclusivity has cost
   nothing at the framework's event-loop shape (select! on next_event).
