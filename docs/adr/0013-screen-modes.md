# ADR 0013: Screen modes — inline and alt-screen as peer modes; terminal-native scrollback by default

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

A TUI framework renders into one of two screen regions. **Alt-screen** takes over a
dedicated full-screen buffer that the terminal discards on exit — the classic full-app
model ratatui defaults to. **Inline** renders into the normal (primary) screen alongside
the shell prompt, so finalized output flows into the terminal's own scrollback. The choice
is not cosmetic: it decides who owns scrollback, text selection, and copy — the terminal,
or the app.

Four forces bound the decision:

- **Inline mode is the sharpest unmet demand of the era, driven by coding-agent CLIs.**
  The wave survey promotes inline "from 'genuinely differentiating feature nobody else has'
  to the single sharpest demand of the era" (docs/research/recent-rust-tui-wave.md §5):
  eye-declare targets "CLI tools, AI assistants, and interactive prompts where output
  accumulates"; rnk and FrankenTUI make inline the *default*; the whole Ink-flicker genre
  chases it. "Alt-screen-centric ratatui makes this awkward; native scrollback/text-
  selection/Cmd+F is the emerging bar." rabbitui's own top implication is to "design inline
  mode … as a first-class render target alongside alt-screen, from the buffer layer up."

- **Native terminal behavior is a feature users actively defend.** Ink's single best idea
  is `<Static>`: "append-only history committed to terminal scrollback + small repaintable
  live tail" so "users keep native scrollback, Cmd-F, and copy/paste" (docs/research/ink.md
  §What it gets right). Codex's move to alt-screen was called "a regression" by users
  precisely because it broke this (ink.md §What's worth stealing #2). And Codex issue #8344,
  "Don't mess with the native TUI" — a *user* filing — states it flatly: "Terminal is king
  because anything works anywhere. Don't break scrolling, copy/paste, for crying out loud"
  (docs/research/codex-tui2.md).

- **Owning the viewport is a cliff, not a slope — a production team priced it and retired
  it.** Codex tui2 rearchitected so the *app*, not the terminal, owned scrollback +
  selection + copy. It won decisively on resize-rewrap and copy fidelity — and was then
  retired. The retirement commit (#9640, authored by joshka@openai.com) is the verdict:
  "What worked: a transcript-owned viewport delivered excellent resize rewrap and high-
  fidelity copy (especially for code). Why stop: making that experience feel fully native
  across the environment matrix (terminal emulator, OS, input modality, multiplexer,
  font/theme, alt-screen behavior) creates a combinatorial explosion of edge cases"
  (docs/research/codex-tui2.md §What it deliberately avoided). Owning the viewport obligates
  you to reimplement, per terminal: wheel physics (`scroll_input_model.md`: 16 probe logs,
  13,734 events, 8 terminals, a per-terminal events-per-tick table, a wheel-vs-trackpad
  heuristic that "must be overridable"), selection geometry (content-relative coordinates,
  gutter exclusion), and copy fidelity (a `joiner_before: Vec<Option<String>>` soft-wrap-vs-
  hard-break metadata channel plumbed from wrap to clipboard, emitting markdown source).
  "You reimplement the wheel."

- **The naive inline renderer flickers structurally, and the fix is an enforced invariant.**
  Ink issue #359 (open since 2020): content taller than the viewport "flickers badly on
  updates," because `eraseLines` can only erase what is still on screen; anything scrolled
  off gets duplicated into scrollback. "The framework can't fix it without the app
  restructuring around `<Static>`" (docs/research/ink.md §What users complain about). Gemini
  shipped this flicker tax to millions; Claude Code spent a year of renderer surgery ending
  at `CLAUDE_CODE_NO_FLICKER=1` (alt-screen). rabbitui's stated conclusion: "Make 'scrollback
  commit + live tail' a renderer invariant, because #359 is unfixable otherwise. The live
  region must never exceed viewport height" (ink.md §Implications). tui2's own bug generator
  was the same class — line-count bookkeeping drifting out of sync — fixed by a cell-level
  high-water mark so "each logical cell appears exactly once" (codex-tui2.md §What it avoided).

## Options considered

### A. Alt-screen only (ratatui's effective default)

*What it is:* the framework always takes the alternate screen buffer; the app owns the whole
frame; on exit the terminal restores the prior screen. Scrollback of app output does not
exist — the app buffer is discarded.

*Steelman:* simplest, most robust model. Sidesteps every native-terminal fight at once
(scroll regions, per-emulator clear quirks, resize races) because nothing the app draws ever
touches scrollback. Claude Code's endgame flicker fix was literally switching to alt-screen
"like every other TUI" (docs/research/ink.md); it is a known-good floor.

*Why not:* it structurally underserves the flagship workload. Coding-agent CLIs *want*
finalized transcript in native scrollback with working Cmd-F, selection, and copy — the exact
thing alt-screen throws away, and the exact thing users filed #8344 to protect
(docs/research/codex-tui2.md). The wave marks inline as the sharpest demand ratatui
"structurally underserves" (recent-rust-tui-wave.md §5). Shipping alt-screen only concedes
the era's defining use case.

### B. App-owned viewport as the default (Codex tui2's model)

*What it is:* the framework owns an in-memory transcript as source of truth, flattens cells to
visual lines each frame, and paints the visible slice; scrollback becomes an *output target*,
not a data structure. Selection, copy, and scroll are all app-implemented over content-
relative coordinates.

*Steelman:* it is the only model that delivers perfect resize-rewrap and high-fidelity copy
(reflowing old content, emitting markdown source for code) — tui2 "won decisively" on exactly
these, and they are real user goods (codex-tui2.md). It also unlocks per-cell interaction
(expand/collapse a tool call, per-cell copy) that dead scrollback text cannot address.

*Why not:* tui2 priced it in production and OpenAI retired it. The cost is a "combinatorial
explosion of edge cases" across the environment matrix, and the specific bills are itemized:
you must reimplement wheel physics (`scroll_input_model.md`'s per-terminal normalization + a
mandatory `scroll_mode` override), selection geometry, copy fidelity (the `joiner_before`
soft-wrap channel), and per-terminal escape quirks — for a product shipping to "every terminal
on earth" (codex-tui2.md §Implications). "tui2 chose (b) and it was the right *engineering* and
wrong *product* call." Defaulting to this makes rabbitui inherit that maintenance surface for
every app, most of which do not need faithful code-copy-across-reflow.

### C. Inline and alt-screen as peer modes, terminal-native by default, owned viewport as future opt-in (CHOSEN)

*What it is:* both modes are first-class render targets over the same widget tree, switchable
at runtime. Inline mode is terminal-native: the terminal keeps scrollback, selection, and
copy; the renderer commits finalized content append-once to scrollback and paints only a
bounded live tail. An app-owned viewport is a documented future opt-in, not the default.

*Steelman:* it serves the flagship workload (inline, native scrollback) without paying tui2's
compatibility bill by default, and it keeps the escalation path open per-feature rather than
all-or-nothing. tui2's own implication for rabbitui is exactly this shape: "Give app authors
the choice with eyes open… Design the seam so a team can start native and escalate to owned
per-feature, not all-or-nothing as tui2 was forced to be" (codex-tui2.md §Implications). Ink,
Gemini's rendering epic (#10673 asks for a runtime toggle), and Claude Code (ended up shipping
both) all independently want the peer-mode toggle (ink.md §Implications).

*Why not — honestly:* terminal-native inline caps the copy/reflow fidelity you can offer;
apps that genuinely need faithful code copy across reflow must take the future owned-viewport
opt-in and its documented costs. Accepted as a deliberate default-for-the-common-case tradeoff.

## Decision

1. **rabbitui ships inline and alt-screen as peer render modes over the same widget tree,
   selectable at startup and switchable at runtime.** Neither is privileged in the API; the
   same declared frame targets either (ADR 0001, ADR 0003).

2. **Inline mode is terminal-native by default.** The terminal retains ownership of
   scrollback, text selection, and copy. rabbitui does not, by default, own the viewport,
   reimplement wheel physics, or drive selection/copy.

3. **The inline renderer enforces two invariants, not conventions.** (a) An **append-once
   scrollback-commit channel**: finalized content is written to the terminal's own scrollback
   exactly once and never rewritten (Ink `<Static>`, generalized to a first-class renderer
   concept with proper item identity — ink.md §What's worth stealing #1). Committed content
   is addressed by the widget-identity high-water mark (ADR 0002), not by line counts, so
   width changes never drop or duplicate — "each logical cell appears exactly once"
   (codex-tui2.md). (b) A **bounded live tail** that never exceeds viewport height; overflow
   is committed above via the scrollback channel or scrolled inside a widget. This closes
   Ink #359 in the renderer rather than documenting it as a footgun (ink.md §Implications).

4. **rabbitui declines an app-owned viewport as the default**, on the tui2 pricing: wheel
   physics, selection geometry, copy fidelity, and the per-terminal escape matrix are a
   combinatorial cost most apps should not pay (codex-tui2.md).

5. **An owned-viewport mode is a future explicit opt-in**, layered on the same seam, with its
   costs documented at the point of opt-in. When built, it adopts tui2's proven shapes:
   width-agnostic logical lines wrapped at render time (never persisted `Vec<Line>` for
   reflowable prose), soft-wrap-vs-hard-break as data (`joiner_before`), the probe-derived
   per-terminal scroll table with a mandatory `scroll_mode` override, and content-relative
   selection coordinates (codex-tui2.md §Implications).

6. **Both modes wrap every frame write in synchronized output (DEC mode 2026) and support a
   full-repaint escape hatch** for desync recovery. The renderer/encoder emits the mode-2026
   begin/end framing (ADR 0003); the substrate negotiates the capability (ADR 0012). Until
   qwertty lands mode 2026, the framing is emitted by rabbitui's interim encoder.

## Consequences

**Positive.** The flagship coding-agent workload is served first-class with native
scrollback, Cmd-F, selection, and copy intact — the thing users file bugs to protect
(#8344). Ink #359 flicker is structurally impossible in inline mode. rabbitui avoids
tui2's retirement fate by not defaulting into the compatibility swamp. The runtime toggle
matches Gemini #10673 and Claude Code's shipped-both endgame. The escalation seam keeps the
owned-viewport option genuinely open, per-feature, without an architecture rewrite.

**Negative (honest).** Terminal-native inline caps fidelity: no reflow of already-committed
scrollback (append-only is a hard invariant — reflowing old scrollback runs straight back into
the per-terminal clear/scroll-region swamp tui2 fled, codex-tui2.md §What it avoided), and no
faithful markdown-source code copy across width changes. Apps needing those must wait for and
adopt the owned-viewport opt-in. Committing correctly to inline scrollback still requires
careful escape handling (scroll regions, per-emulator quirks like tui1's `ZellijRaw` case);
we push this into the substrate/encoder (ADR 0012) but it is real work, and inline mode carries
a heavier PTY-level test burden (ADR 0009). Two modes is more surface than one.

**Neutral.** Alt-screen remains the robust floor and the right default for full-screen apps
that do not want inline scrollback. The owned-viewport path, if demand justifies it, is
additive — a superset mode, not a redesign. Overlays and pagers use alt-screen within an
otherwise-inline app (tui2 straddled inline + alt-screen for exactly this — codex-tui2.md),
so the two modes coexist within one app, not just across apps.

## Revisit triggers

- **Faithful copy demand crosses the bar.** If real apps repeatedly need markdown-source /
  code-fidelity copy across reflow that terminal-native selection cannot give, prioritize and
  ship the owned-viewport opt-in (mode b), on tui2's documented shapes.
- **The append-once + bounded-tail invariant proves too restrictive.** If a genuine workload
  cannot be expressed as commit-once history plus a viewport-bounded live tail (e.g. content
  that must mutate after commit), reopen the inline renderer model.
- **Terminal-native selection degrades below usability** in a major emulator/multiplexer such
  that inline mode's copy/scroll story breaks for a meaningful user base — reassess whether
  owned-viewport must be promoted from opt-in toward default for that environment.
- **Per-cell interaction becomes a core requirement** (expand/collapse tool calls, drill-into
  overlays addressing committed cells) — this needs owned-viewport cell identity beyond dead
  scrollback text, and would pull mode (b) forward.
- **Accessibility export forces a persistent addressable transcript.** If an AccessKit-style
  export (tracked, ADR 0001 non-goals) requires the whole transcript to remain app-addressable,
  terminal-owned scrollback is insufficient and the owned-viewport model gains a second
  justification.
