# The Terminal as a 2D Application Surface: What's Missing

**Date:** 2026-07-06

**Methodology:** This document synthesizes a 13-study survey of TUI frameworks and production
terminal applications (ratatui, libvaxis, notcurses, Textual, Bubble Tea, the Codex CLI's two TUI
architectures, Claude Code, Warp, DomTerm, and others) with protocol-level research into shipping
and proposed terminal extensions. All citations are public sources. It is written for terminal
emulator and multiplexer maintainers and for protocol designers; it is a constructive engineering
document, not a complaint. Every proposal sketch addresses graceful degradation and multiplexer
passthrough, because a mechanism that lacks either is not deployable.

---

## 1. The mismatch, stated precisely

The terminal implements a model with four load-bearing assumptions:

1. **The application emits an append-only byte stream**; the terminal interprets it into a grid of
   character cells.
2. **Painted cells have no identity.** Once bytes are interpreted, the terminal knows _where_ things
   are but not _what_ they are. Content that scrolls out of the active area becomes dead text.
3. **The terminal exclusively owns the presentation affordances** — scrollback, viewport position,
   selection, copy, and reflow-on-resize — using only the grid and its private per-line bookkeeping.
4. **Input is a byte stream in the other direction**, encoded with conventions dating to hardware
   terminals, where many distinct keys collide onto the same bytes.

Every serious full-screen or inline application built in the last decade is trying to construct
something the grid cannot express: a **transcript of typed, addressable units** (commands, diffs,
tool calls, messages), each with a logical source form that differs from its painted form, some
still live and mutating, most finalized; **logical lines** that the terminal soft-wraps but the
application knows how to join; **interactive regions** that respond to clicks and hovers; **text
input** that includes composition (IME); and a **semantic structure** that a screen reader or a test
harness could consume.

The mismatch is not that terminals lack the underlying machinery. They have almost all of it,
privately: every emulator tracks a soft-wrap flag per line
([xterm.js #609](https://github.com/xtermjs/xterm.js/pull/609), DEC's Last Column Flag per
[wraptest](https://github.com/mattiase/wraptest)); WezTerm tracks semantic zones with a
`StableRowIndex` that survives reflow
([docs](https://wezterm.org/config/lua/pane/get_semantic_zone_at.html)); the terminal receives
high-resolution scroll deltas from the OS and pixel-precise pointer positions. The mismatch is that
**the semantics live in the application and the mechanisms live in the terminal, and there is almost
no channel between them.** The app knows which rows form one logical line; the terminal owns the
copy operation. The terminal knows the user scrolled; the app owns the transcript. The app knows a
region is a collapsible tool log; the terminal owns the viewport.

Faced with this, applications choose between two poles, and the best-documented experiment of the
era ran both to completion. The Codex CLI's first TUI ("tui1") **cooperated with the terminal**: it
wrote finalized history into native scrollback above an inline viewport using scroll-region escapes,
so native scrolling, selection, and copy kept working — and it inherited every emulator's
scroll-region and resize idiosyncrasies, with per-terminal special cases, and a class of resize
corruption where "some lines were lost or overwritten, others were duplicated"
([tui2 design docs, pinned tree](https://github.com/openai/codex/tree/41e38856f6c11679d75bd63f6eef1e0ea76dffeb/codex-rs/tui2/docs)).
Its successor ("tui2") **owned the viewport**: an in-memory list of history cells became the source
of truth, and the app reimplemented scrolling, selection, and copy. It won decisively on
resize-reflow and copy fidelity — and was retired, in the maintainers' own words, because "making
that experience feel fully native across the environment matrix (terminal emulator, OS, input
modality, multiplexer, font/theme, alt-screen behavior) creates a combinatorial explosion of edge
cases." Users defended the native pole explicitly: "Terminal is king because anything works
anywhere. Don't break scrolling, copy/paste"
([openai/codex #8344](https://github.com/openai/codex/issues/8344)).

The third answer is Warp's: stop being a terminal. Each command, agent message, and diff becomes a
native UI block outside the character grid entirely
([Warp's block model](https://www.warp.dev/blog/block-model-behind-warps-agentic-development-environment)).
That buys per-block copy, search, and collapse — at the cost of interoperability, which is the
terminal's entire value.

Three architectures, three failure modes, one cause. The rest of this document walks the gap areas
one at a time. Each follows the same shape: what applications need, how they approximate it today,
what mechanisms already exist, what is missing, and a proposal sketch.

---

## 2. Regions: identity, lifetime, and the live/committed boundary

**What applications need.** An agent-CLI transcript is a sequence of discrete cells: a user prompt,
an assistant message, a diff, a tool-call log. Applications want to declare "this run of output is
one unit" so the terminal can select it as a unit, copy it as a unit, jump between units, fold a
500-line test log to a summary line, and attach an exit status or duration. They need a **stable
handle** to a region — "the diff I printed 200 lines ago" — addressable by ID, not by fragile screen
row. And they need a **live-tail vs. committed distinction**: the bottom region (streaming tokens, a
spinner) is mutating and app-owned; everything above a high-water mark is finalized and should be
fully terminal-owned.

**How it's approximated.** Agent CLIs hand-roll the live-region-plus-committed-scrollback pattern
with DECSTBM scroll-region escapes and a hand-tracked high-water mark; the line-count bookkeeping
was Codex tui1's "specific bug generator" on resize. Apps that need addressable identity rebuild the
entire transcript app-side (tui2's cell list — retired). Apps reimplement reflow of committed
scrollback by replaying their transcript from source on resize, with a hardcoded per-terminal table
of scrollback caps — VS Code 1000 rows, WezTerm 3500, Alacritty 10000, Windows Terminal 9001
([codex #18575](https://github.com/openai/codex/pull/18575)) — because "the terminal cannot provide
this functionality natively since it operates on already-wrapped content." Shells re-emit prompt
marks in PS1 on every redraw because marks don't survive reflow
([kitty shell integration](https://sw.kovidgoyal.net/kitty/shell-integration/)).

**What exists.**
[OSC 133 semantic prompts](https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md)
(FinalTerm/FTCS) are the de-facto standard, adopted by iTerm2, kitty, WezTerm, Ghostty, VS Code,
Windows Terminal, foot, Contour, and DomTerm. They power prompt navigation, exit-status decorations,
click-to-move-cursor, and kitty's `mouse_select_command_output`. WezTerm's `SemanticZone` is the
closest first-class region abstraction. Ghostty is actively redesigning its OSC 133 storage from
row-based marks to multi-row regions, explicitly to enable styling, collapsing, and better selection
([ghostty #5932](https://github.com/ghostty-org/ghostty/issues/5932)). DomTerm — built by the OSC
133 author — proves the full model: each command is a foldable DOM subtree with stable identity
([DomTerm DOM structure](https://domterm.org/DOM-structure.html)). iTerm2 ships proprietary marks
and annotations ([escape codes](https://iterm2.com/documentation-escape-codes.html)).

**What's missing.** OSC 133 marks _boundaries_ — points in the stream — not regions with identity.
Nothing lets an app open a region, receive a stable handle, append to it, close it, and later
address it. The vocabulary is shell-shaped (prompt/input/output only), so agent CLIs must masquerade
as shells to get any region behavior — which is literally what users are requesting of Claude Code
([anthropics/claude-code #22528](https://github.com/anthropics/claude-code/issues/22528),
[#32635](https://github.com/anthropics/claude-code/issues/32635)). There is no live-vs-committed
contract, no app-driven reflow of a declared region, no portable fold affordance (WezTerm has an
open request, [#4253](https://github.com/wezterm/wezterm/issues/4253)), and no callback path — the
app that created a region is never told the user folded or clicked it. And OSC 133 already breaks
under tmux, which consumes marks as pane-local state and does not forward them
([tmux #5237](https://github.com/tmux/tmux/issues/5237)).

**Proposal sketch.** Promote boundary marks to explicit, identified, typed spans:
`BEGIN id=<handle> type=<prompt|input|output|cell|live|app-defined> [collapsible] [source-hint]` …
`END id [exit] [title]`. Semantics: a region is _open_ (may receive writes) then _committed_
(immutable; terminal fully owns scroll/select/copy). At most one `type=live` region at the tail;
committing it is the protocol form of the high-water-mark handoff every agent CLI hand-rolls.
Regions carrying a source hint are re-wrapped by the terminal from source on resize; regions without
one reflow as today. Collapsible regions get a native fold affordance, and folds/clicks round-trip
to the owning app in-band, keyed by region id. **Degradation:** unknown OSCs are ignored; content
prints as plain scrollback — regions are pure progressive enhancement, and
`type=prompt/input/output` maps onto OSC 133 A/B/C/D for existing terminals. **Multiplexer:**
specify pane-scoping from day one — the mux parses BEGIN/END, associates regions with the pane (as
tmux already does for its own OSC 133 state), re-emits them into the outer terminal's namespace,
clips to pane bounds, and routes interaction callbacks back to the correct pane. Ghostty's #5932
region rewrite is a large fraction of this already.

---

## 3. Wrap, reflow, selection, and copy: whose line is it?

**What applications need.** When a user copies a wrapped logical line — a long shell command, a line
of code — the clipboard must contain _one_ logical line, joined at soft-wrap points, not the visual
rows. When a rendered diff is copied, the payload should be the patch text, not the decorated grid.
When the window resizes, committed content should re-wrap from source without the app replaying its
transcript.

**How it's approximated.** Codex tui2 modeled soft-wrap-vs-hard-break as first-class data — per
wrapped line, an optional "joiner" carrying the exact whitespace skipped at the break — and emitted
markdown _source_ on copy, precisely because none of this survives when the terminal owns selection.
Terminal-owned copy guesses from its private wrap flag and guesses wrong at boundaries: tmux yanks
the wrap newline inside a long command so pasting executes half of it
([tmux #530](https://github.com/tmux/tmux/issues/530)); rectangle selection trims characters
([tmux #2709](https://github.com/tmux/tmux/issues/2709)); Windows Terminal historically split
wrapped lines on copy ([#3367](https://github.com/microsoft/terminal/issues/3367)); Wave's copy
"returns the visual grid view rather than the underlying logical content"
([waveterm #3288](https://github.com/wavetermdev/waveterm/issues/3288)).

**What exists.** Every emulator already stores a per-line soft-wrap flag and uses it to reflow its
own grid ([xterm.js #609](https://github.com/xtermjs/xterm.js/pull/609)) — but the flag is never
exposed, and it records where _the terminal's_ autowrap fired, not where the _app's_ logical line
ends. OSC 52 writes the clipboard across ssh and (with configuration) tmux, but it is write-only in
practice (read is blocked as an exfiltration risk almost everywhere), capped near 74,994 bytes
([st commit](https://git.suckless.org/st/commit/2e54a21b5ae249a6bcedab9db611ea86037a018b.html)), and
the app must have already chosen the bytes — it solves transport, not _which bytes_. kitty's
[OSC 5522](https://sw.kovidgoyal.net/kitty/clipboard/) shows the richer direction (MIME types,
chunking, permission prompts) but is single-terminal. One adoption signal deserves prominence: kitty
**rejected** a proposal for per-line continuation prefixes on the grounds that unbounded per-line
app metadata is "unacceptable complexity" for reflow
([kitty discussion #9134](https://github.com/kovidgoyal/kitty/discussions/9134)). Any wrap-metadata
proposal must respect that ceiling: bounded, per-break, evictable.

**What's missing.** A wire representation of soft-vs-hard breaks _as app-authored data_; sub-command
semantic granularity (a diff cell, a code block) so selection can snap to it; and any form of
**semantic copy** — a mechanism by which the terminal, resolving a user selection, obtains the
logical payload for the covered region instead of scraping the grid.

**Proposal sketch.** Two composable pieces. (1) _Logical-line markup:_ regions (from §2) carry
per-break wrap intent — a soft/hard bit plus a short joiner string — turning the wrap flag terminals
already keep into app-authored truth, and enabling terminal-side re-wrap of committed inline content
from the app's break points. The metadata is bounded per break and discarded with scrollback
eviction, honoring the #9134 ceiling. (2) _Semantic copy:_ on copy of a selection covering a
registered region, the terminal issues an in-band copy-request to the app ("logical payload for
region R, offsets a..b"); the app replies with an OSC-5522-shaped, MIME-typed payload. Note the
trust inversion: the _terminal_ initiates, triggered by a real user gesture, so this sidesteps the
exfiltration risk that keeps OSC 52 read disabled — an app can only supply data for a region the
user actively selected. **Degradation:** no markers → today's geometric copy; unsupported terminal →
the app falls back to OSC 52 writes. **Multiplexer:** the markers ride the stream like OSC 8; the
mux either answers semantic copy itself from recorded region metadata (preferred — it already owns
copy-mode and reflows panes) or routes the request to the focused pane, the same routing proposed
for OSC 52 in [tmux #4275](https://github.com/tmux/tmux/issues/4275).

---

## 4. Scroll: the viewport, the wheel, and the mutual-exclusion trap

**What applications need.** Inline applications need to know whether the user is at the tail (to pin
a composer, to offer "jump to latest"), to be _notified_ when the user scrolls, and occasionally to
move the viewport programmatically ("jump to this hunk"). They need wheel and trackpad input that
behaves consistently. Test harnesses and agents need permissioned readback of the rendered buffer.

**How it's approximated.** The core defect is a mutual exclusion: to learn about scrolling at all,
an app must enable mouse mode — which makes the terminal stop scrolling natively and hand the app
raw wheel events it must integrate itself. Those events carry no magnitude and no device type:
terminals emit 1–9+ events per physical notch, differently per emulator. Codex tui2's scroll model
was built from a 13,734-event study across 8 terminals and needed per-terminal normalization tables,
stream grouping, a wheel-vs-trackpad heuristic documented as non-separable by timing, and a
mandatory user override
([tui2 docs](https://github.com/openai/codex/tree/41e38856f6c11679d75bd63f6eef1e0ea76dffeb/codex-rs/tui2/docs)).
The terminal _has_ the OS-level fractional delta and discards it
([wezterm #7645](https://github.com/wezterm/wezterm/issues/7645): "discards the magnitude of the
delta and forwards only a single tick";
[ghostty discussion #4259](https://github.com/ghostty-org/ghostty/discussions/4259)). Meanwhile
alternate-scroll mode (?1007) translates the wheel to arrow keys — lossy and OS-divergent
([claude-code #64214](https://github.com/anthropics/claude-code/issues/64214)) — and even SU/SD
semantics split between implementations
([microsoft/terminal #11078](https://github.com/microsoft/terminal/issues/11078)). Buffer readback
exists only as emulator-specific side channels: kitty's
[remote control get-text](https://sw.kovidgoyal.net/kitty/remote-control/), tmux `capture-pane`,
each with its own coarse permission model, against a real threat class
([weaponizing ANSI escape sequences](https://www.packetlabs.net/posts/weaponizing-ansi-escape-sequences/)).

**What exists in proposal space.** The freedesktop terminal-wg has an open proposal for apps
reporting buffer size and offset so the terminal can render a real scrollbar (specifications issue
\#5, from [xterm.js #1875](https://github.com/xtermjs/xterm.js/issues/1875)); nothing has been
ratified. Textual demonstrated that cell+pixel size reporting enables sub-cell smoothness
([Textual smooth scrolling](https://textual.textualize.io/blog/2025/02/16/smoother-scrolling-in-the-terminal-mdash-a-feature-decades-in-the-making/)).
The GUI stack solved high-resolution scroll a decade ago (WHEEL_DELTA=120, libinput high-res
wheels); terminals sit downstream and flatten it.

**What's missing, and the sketch.** Four DECRQM-gated mechanisms, each independently useful: (1)
**Scroll reporting without capture** — a mode under which the terminal keeps scrolling natively but
emits an event (`offset; total; height; source`) on user scroll, plus a query for current state.
This decouples scroll _awareness_ from mouse _capture_, the structural defect of modes 1000/1006.
(2) **Magnitude-carrying wheel events** — extend SGR mouse with a signed v120-style fractional delta
and a discrete/continuous device bit. This single field obsoletes the entire per-terminal
normalization industry. (3) **A viewport-scroll command** with pinned semantics (moves the viewport
through scrollback, resolving the SU divergence). (4) **Permissioned buffer readback** standardizing
kitty get-text / capture-pane, default-deny, per-request consent — not a global allow-all flag.
**Degradation:** each mode answers "unsupported" via DECRQM and the app falls back to today's
behavior. **Multiplexer:** the mux is the real scrollback owner in a huge share of sessions, so it
must answer scroll state for _its_ scrollback and mediate readback of its own panes — these must be
mux-native features, as the kitty keyboard precedent shows, not passthrough.

---

## 5. Input: one keypress, one unambiguous event — and the IME hole

**What applications need.** `Tab` distinct from `Ctrl+I`, `Enter` from `Shift+Enter` (now table
stakes for every agent composer: "insert newline, don't submit"), `Esc` delivered immediately with
no ambiguity against `Alt+key` or a sequence prefix, key release/repeat, full modifiers, and the
same logical event for the same keypress on every OS, terminal, and mux. And — the largest genuinely
unsolved hole — **IME composition**: a preedit string a TUI text widget can render inline, commit
events, and candidate-window coordination, so CJK users can type in a terminal composer at all.

**How it's approximated.** Legacy encoding cannot express Shift+Enter, so Anthropic ships a
`/terminal-setup` command that writes per-terminal configuration just to make one keybinding work
([Claude Code terminal config](https://code.claude.com/docs/en/terminal-config)); the general
analysis is laid out in
["Your terminal can't tell Shift+Enter from Enter"](https://blog.fsck.com/agent-blog/2026/02/26/terminal-keyboard-protocol/).
Esc is disambiguated by a 50–100 ms timing heuristic that is wrong by construction — decode behavior
depends on `read()` syscall chunking
([crossterm #993](https://github.com/crossterm-rs/crossterm/issues/993)). Editors hardcode terminal
allowlists because terminfo lies
([Ghostty devlog 004](https://mitchellh.com/writing/ghostty-devlog-004)). IME is worked around
entirely outside the protocol — external input-method switchers, GUI frontends — because apps
reacting to keydown "react to the composition steps prematurely" and corrupt input
([neovim TUI docs](https://neovim.io/doc/user/tui.html);
[kitty #469](https://github.com/kovidgoyal/kitty/issues/469)).

**What exists.** The [kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)
is the modern answer and a genuine success: five progressive-enhancement flags, a non-destructive
query, a push/pop stack with separate main/alt-screen stacks. Adoption as of early 2026 spans kitty,
foot, WezTerm, Alacritty, Ghostty, iTerm2, Warp, and Windows Terminal Preview
([1.25 release](https://devblogs.microsoft.com/commandline/windows-terminal-preview-1-25-release/));
absent from VTE-based terminals, Terminal.app, and PuTTY. SGR mouse (1006) is the de-facto floor;
SGR-Pixels (1016) is spreading. Bracketed paste (2004) and focus events (1004) are widespread.
Windows carries win32-input-mode as a parallel, Windows-shaped fix
([microsoft/terminal PR #6309](https://github.com/microsoft/terminal/pull/6309)).

**What's missing.** (1) A _floor_, not a ceiling: kitty keyboard is opt-in and fragmented, so every
app still ships negotiation plus a legacy decoder plus quirk tables. (2) tmux passthrough: `CSI ? u`
and the push/pop sequences are silently swallowed — bare `u` is restore-cursor in tmux's dispatch
table — with the feature request open since 2022
([tmux #3335](https://github.com/tmux/tmux/issues/3335)); zellij implemented the protocol natively,
so behavior under a mux is inconsistent by mux. (3) IME: no protocol at all.

**Proposal sketch.** Ratify kitty keyboard as a mandatory baseline (the spec exists; this is an
adoption problem, and Windows Terminal shipping it removes the last platform excuse). Specify mux
behavior normatively: a mux MUST either implement the protocol and re-encode inward, or forward the
query/push/pop verbatim and answer the capability query truthfully — a swallowed query becomes a
spec violation. Then add the missing piece: an **IME composition channel** — `preedit-begin`,
`preedit-update(text, cursor)`, `commit(text)` events in a namespaced encoding legacy apps ignore,
with the app reporting a cell anchor for candidate-window placement. **Degradation:** unaware apps
still receive the final committed text, so CJK "just types" everywhere; terminals without the
baseline fall back to legacy decode with the timing heuristic as the exception rather than the norm.

---

## 6. Width, graphemes, and rich text: the contract every frame rests on

**What applications need.** Width agreement is not a nicety; it is _the_ correctness invariant of
the cell grid. If the app measures a string at N cells and the terminal advances M ≠ N, every
subsequent glyph on the row is misplaced and the frame corrupts
([Hashimoto, grapheme clusters in terminals](https://mitchellh.com/writing/grapheme-clusters-in-terminals)).
Modern content — ZWJ emoji, skin-tone modifiers, flags, combining marks — makes codepoint-summing
wcwidth wrong by construction. Above the floor, applications want hyperlinks (OSC 8), styled
underlines with color, and multi-size text for headings and super/subscript.

**How it's approximated.** Every framework ships its own width table and hopes it matches.
Empirically it cannot: a survey of ~35 terminals found **23 distinct implementations** of which
codepoints are Wide and **19 distinct implementations** of ZWJ-emoji width
([jquast, correction tables](https://www.jeffquast.com/post/perfecting-terminal-character-width-using-correction-tables/)).
The Python wcwidth library now ships _per-terminal correction tables_ keyed on `TERM_PROGRAM` — a
reverse-engineered compatibility shim where a contract should be. Unicode-version drift is baked in:
most wcwidth implementations descend from a 2007 table pinned to Unicode 5.0, while East Asian Width
has changed since ([jquast/wcwidth](https://github.com/jquast/wcwidth)). Multiplexers compute their
own widths independently, so a pane can corrupt when zellij says 1 and the outer terminal renders 2
([ghostty #10333](https://github.com/ghostty-org/ghostty/issues/10333)).

**What exists.** Three overlapping, non-composable answers.
[Mode 2027](https://github.com/contour-terminal/terminal-unicode-core) negotiates grapheme
clustering but explicitly punts on which Unicode version's tables apply, so two compliant terminals
can still disagree per codepoint. iTerm2's proprietary `UnicodeVersion` pins the table
([escape codes](https://iterm2.com/documentation-escape-codes.html)). And kitty's
[text-sizing protocol (OSC 66)](https://sw.kovidgoyal.net/kitty/text-sizing-protocol/) takes the
structurally different path: the app declares the width in cells (`w=`), putting "only one actor in
charge of determining string width" — plus integer scale and fractional sizing for headings and
super/subscript. OSC 8 hyperlinks are the degradation success story: broad adoption
([adoption matrix](https://github.com/Alhadis/OSC8-Adoption)) because ECMA-48-correct parsers
silently drop the URI and show the text. Extended underlines (SGR 4:3, 58/59) are widespread but
detection relies on terminfo, which lies.

**What's missing, and the sketch.** The survey's lesson is that mutual-agreement models cannot
converge across 23 divergent tables; the only stable fix is to make one actor authoritative. So: (1)
a **queryable width regime** — a typed answer to "explicit-width supported? grapheme clustering?
Unicode version of your tables?" — with no cursor-probe round-trip; (2) **explicit width (OSC 66
`w=`) as the interop standard**, currently kitty plus partial foot/Ghostty; (3) rich text as
orthogonal capabilities on the same negotiation (OSC 8 with id-grouping by default, underline
detection by query rather than terminfo, OSC 66 scaling). **Degradation is naturally monotonic:**
explicit-width → 2027 clustering → version-pinned wcwidth → legacy wcwidth; scaled text renders
unscaled; links render as text. **Multiplexer:** explicit width is what makes muxes _tractable_ — a
mux that stores the app-declared width per cell never needs its own width table at all, eliminating
the mismatch class outright. That inversion (mux trusts declared width instead of re-deriving it)
should be specified, because today OSC 66 has no mux story.

---

## 7. Interactive and rich content cells

**What applications need.** Inline approvals as real buttons; collapsible tool-call cells; hover
affordances; a progress bar that can still finalize after it scrolls out of the live region; images
and graphs that reflow with text; sub-cell pointer coordinates for small targets.

**How it's approximated.** Owning the whole viewport (tui2's path — retired). iTerm2's proprietary
buttons and annotations, which are macOS-only and, worse, are swallowed by any TUI running in the
alt screen — writes to `/dev/tty` don't reach the host terminal
([claude-code #33686](https://github.com/anthropics/claude-code/issues/33686)). OSC 8 links as the
only portable "interactive" primitive — but the click is handled by the terminal (opens a URL),
never reported to the app, so it cannot drive app logic
([claude-code #13008](https://github.com/anthropics/claude-code/issues/13008) shows the demand).
Full mouse mode seizes selection. Images work via the
[kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/) and sixel, with the
Unicode-placeholder mechanism (U+10EEEE) the only variant that reflows with text and survives tmux —
but all graphics are raster-only with zero event callbacks: no click or hover on an image, ever.

**What exists in adjacent space.** Contour's
[passive mouse tracking (mode 2029)](https://github.com/contour-terminal/vt-extensions/blob/master/passive-mouse-tracking.md)
is the right idea in embryo: mouse events _without_ disabling native text selection, explicitly
motivated by hover tooltips — but single-vendor and unscoped. kitty's pointer-shape OSC gives the
"this is clickable" cursor affordance. The terminal-wg has an image-protocol proposal and
screen-reader-hint discussions, but no interactive-region proposal exists at all.

**What's missing, and the sketch.** Identity + events + lifecycle — deliberately _not_ a widget
toolkit. (1) The app registers a region (ideally anchored the way Unicode-placeholder images are, so
it reflows and rides through muxes) with an app-chosen id, a role
(button/collapsible/progress/canvas), and an accessible label; the app keeps drawing the region's
content as ordinary cells. (2) Clicks and hover-enter/leave on registered regions are reported to
the app as typed events with sub-cell coordinates, without global mouse mode — native selection
stays live outside regions (generalizing Contour 2029). (3) Because regions have stable ids, the app
can repaint region N after it scrolls — in-place progress finalization and collapse/expand fall out.
(4) **Security:** registered interactivity is an injection surface, so specify a sandbox tier —
untrusted output (a `cat`-ed file, remote bytes) can register nothing action-bearing; interactive
tiers require the foreground app on the primary fd, mirroring iTerm2's user-interaction gating and
kitty's resource quotas. **Degradation:** registration is a no-op on unsupported terminals and the
fallback bytes ("Approve? [y/n]") remain readable — the exact contract image fallbacks (chafa,
notcurses blitters) already use. **Multiplexer:** placeholder anchoring gives position/reflow for
free; the registry sequences and the event route back to the owning pane must be specified mux
behavior, not passthrough.

---

## 8. Accessibility and observability: the missing semantic channel

**What applications need.** A screen-reader user in front of a coding-agent CLI needs new assistant
tokens announced as a polite live region, a diff read as added/removed semantics rather than `+`/`-`
glyphs, and the approval dialog exposed as a focusable button group. The CHI 2021 best paper on CLI
accessibility documents blind users defeated by curses redraws, ASCII tables, and spinners
([ACM](https://dl.acm.org/doi/10.1145/3411764.3445544)). The same structured view — roles, labels,
focus, values — is exactly what AI agents and test harnesses need to read a TUI they cannot see.

**How it's approximated.** Terminals expose the grid as flat text to OS accessibility APIs — Windows
Terminal's UIA text provider is the most serious attempt and still exposes _text_, not roles
([microsoft/terminal PR #14097](https://github.com/microsoft/terminal/pull/14097)). xterm.js/VS Code
maintain an off-screen ARIA buffer with live-region announcement and keystroke dedup — a heroic
app-level workaround that still announces character streams
([design doc](https://github.com/xtermjs/xterm.js/wiki/Design-Document:-Screen-Reader-Mode)).
GitHub's CLI team did accessibility purely at the content level (static text instead of spinners,
linear prompts) and noted there is no WCAG-for-terminals
([GitHub blog](https://github.blog/engineering/user-experience/building-a-more-accessible-github-cli/)).
Agent-observability tools re-emulate the terminal headlessly to reconstruct pseudo-semantics from
the grid ([coder/agent-tty](https://github.com/coder/agent-tty)). kitty's maintainer states the
terminal-side objection precisely: the terminal "cannot know what to read" when a TUI redraws the
whole screen ([kitty discussion #9202](https://github.com/kovidgoyal/kitty/discussions/9202)) —
which is an argument _for_ an app-provided channel, not against accessibility.

**What exists.** One draft: terminal-wg's "control screen reader from applications" proposal
(announce/stop/resume plus announce-only alt-text —
[specifications #18](https://gitlab.freedesktop.org/terminal-wg/specifications/-/issues/18)),
unratified and unimplemented. [AccessKit](https://accesskit.dev/how-it-works/) already provides the
cross-platform schema and OS adapters (UIA/AT-SPI/NSAccessibility) — a push model of incremental
tree updates with roles, states, and actions. What it needs is a window handle, which the _terminal_
owns, not the app ([ratatui #2190](https://github.com/ratatui/ratatui/issues/2190) documents the
dead end from the app side).

**What's missing, and the sketch.** A semantic sidechannel: the app declares an AccessKit-shaped
tree — node id, role, label, value, state — bound to region ids from §2, updated incrementally; the
terminal, which owns the window and the AT connection, forwards it into the OS adapters it can
already embed. Politeness levels and announce-only nodes come from the terminal-wg draft,
generalized. AT action requests (focus, invoke) return to the app as input events. The terminal
never guesses what to read; the app declares it, exactly as GUI toolkits declare to AccessKit.
**Degradation:** the channel is inert framing to terminals that ignore it; a screen reader without
the bridge still gets today's grid-as-text; an agent without it still gets a PTY snapshot — nothing
regresses. **Multiplexer:** forward by default (learn from tmux/OSC 133), and route action requests
inward; a mux that only forwards outbound yields read-only accessibility — an honest, documentable
degradation. One channel, two consumers: the same tree serves Orca and the CI harness.

---

## 9. Capability negotiation and the multiplexer middle-box

Everything above depends on this section. A capability that cannot be discovered reliably, or that
dies inside tmux, does not exist for application authors.

**What applications need.** At startup — fast, every invocation, possibly over ssh to a host with no
terminfo entry — an app must learn which of the modern extensions it may use, _who_ it is talking to
(real terminal, tmux, zellij, an embedded editor terminal, a recorder), and be told when the answer
changes (a differently-capable client attaches).

**How it's approximated.** The state of the art is the DA1-fenced query burst (libvaxis: "does not
use terminfo; support for vt features is detected through terminal queries" —
[libvaxis](https://github.com/rockorager/libvaxis)): batch DECRQM/XTVERSION/`CSI ? u`/OSC probes and
use the universally-answered DA1 reply as the "all answers are in" sentinel, paying at most one
timeout. Beyond that: hardcoded quirk tables (Vim hardcodes which terminals support kitty keyboard,
ignoring terminfo — [Ghostty devlog 004](https://mitchellh.com/writing/ghostty-devlog-004)); env-var
overrides for known liars; and per-app mux detection followed by guessing. The failure modes are
concrete and current: an OSC 11 background query sent inside tmux is neither forwarded nor answered,
times out, and a UI element silently disappears
([codex #19741](https://github.com/openai/codex/issues/19741)); an app that doesn't write the kitty
activation sequence never receives Shift+Enter through tmux, and the fix is manual per-user tmux
config ([claude-code #26629](https://github.com/anthropics/claude-code/issues/26629)); missing
synchronized-output detection under tmux produces flicker even though tmux 3.7 supports mode 2026
([claude-code #37283](https://github.com/anthropics/claude-code/issues/37283)).

**What exists.** DA1 as the fence; DECRQM (tmux only began answering it broadly in 3.6/3.7);
[XTGETTCAP](https://sigwait.org/~alex/blog/2025/03/25/XTGETTCAP.html) as terminfo-over-the-wire,
solving the remote-host problem for the capabilities terminfo names; XTVERSION as the one honest
provenance primitive (tmux self-identifies in its reply); and tmux's `allow-passthrough` — which is
**outbound-only and provably cannot carry query round-trips**: a DCS reply passing back through gets
re-encoded into garbage, closed "not planned"
([tmux #4386](https://github.com/tmux/tmux/issues/4386)).

**What's missing, and the sketch.** There is no single capability namespace (apps fan out six query
syntaxes), no structured provenance (the full stack: ghostty ← tmux 3.7 ← mosh, and _who answered
which capability_), no composed answer (the only number an app needs is the minimum of what the mux
forwards and what the outer terminal supports), no activation handshake muxes gate on, and no
"capabilities changed, re-negotiate" event. Sketch: one fenced query returning (name, version,
state) triples over a registered namespace, as a single DCS blob, always terminated by DA1; each
middle-box **interprets and re-emits** the query, prepends its identity and forwarding policy to a
provenance list, and reports the composite capability it will actually deliver; activation is
request/confirm/NACK so silence is never ambiguous; a change event (on the 2048 pattern) triggers
re-query on client attach/detach. **Degradation:** a legacy terminal answers only DA1 and the app
gets the legacy floor after one timeout; user override env vars remain the escape hatch.
**Multiplexer:** the mux is not bypassed — it _is_ the terminal the app draws to, so it answers for
the composite it presents. Apps should never probe the outer terminal blind through passthrough.

---

## 10. If terminals shipped only three things

Ranked by leverage — how much per-app hack code each retires, weighted by how many of the other gaps
it unlocks:

**1. Typed, identified regions with a live/committed lifetime (§2), including reflow-from-source and
fold.** This is the highest-leverage primitive in the document. It retires the
DECSTBM-plus-high-water-mark pattern (the single largest source of inline-mode resize corruption),
the per-terminal scrollback-replay tables, and shell-masquerading OSC 133 emission by agent CLIs; it
is the anchor that semantic copy (§3), interactive cells (§7), and the accessibility tree (§8) all
key off. It is also the proposal closest to what maintainers are already building — Ghostty's region
rewrite, WezTerm's SemanticZone, DomTerm's command groups are independent reinventions of most of
it.

**2. Composed capability negotiation with provenance, specified for multiplexers (§9).** Without it,
every other extension arrives as another decade of quirk tables. One fenced query, one structured
reply, middle-boxes answer for the composite they deliver, activation is acknowledged, changes are
events. This is the enabling infrastructure for everything else on the list, and it turns "does
feature X work under tmux?" from folklore into data.

**3. Scroll decoupled from capture: scroll-state events without mouse mode, plus magnitude-carrying
wheel events (§4).** Small spec, immediate payoff. The notification mode fixes the
pinned-composer/"jump to latest" problem for every inline app without stealing native scrolling; the
v120-style delta field — information the terminal already receives and discards — deletes the entire
per-terminal wheel-normalization genre exemplified by the 13,734-event Codex study.

Honorable mention, deliberately excluded because it is an adoption problem rather than a design
problem: making the kitty keyboard protocol a floor rather than an option, with normative mux
forwarding (§5). The spec exists and works; what it needs is tmux support and default-on adoption,
not new engineering.

---

## 11. Design principles from the extensions that worked

Synchronized output (mode 2026), in-band resize (mode 2048), the kitty keyboard protocol, and OSC 8
hyperlinks are the recent extensions that actually deployed. They share a discipline worth writing
down, because it is the template every proposal above follows:

1. **Queryable, in-band, before use.** Every success is discoverable at runtime from the terminal
   itself (DECRQM, `CSI ? u`) — never from terminfo, which is a stale local file that lies about
   undercurl and doesn't travel over ssh. Mode 2048 is the cleanest template: query, enable, event,
   all on one fd.
2. **Event-based, on the input stream.** In-band resize exists because SIGWINCH is racy and doesn't
   traverse ssh. Deliver state changes as data on the fd the app is already reading, in order with
   input.
3. **Degradable to a no-op.** OSC 8's genius is that a conforming ECMA-48 parser that has never
   heard of it silently drops the URI and renders the text. Unknown OSC/DCS must be inert; a
   feature's absence must leave the app with exactly today's behavior, so emitting the extension
   unconditionally is always safe once probing says yes — and merely wasteful, never corrupting,
   when probing was impossible.
4. **Stack-disciplined and nestable.** kitty keyboard's push/pop with separate main/alt-screen
   stacks means an app, its child editor, and its pager can each enable what they need without
   trampling each other. Any mode an app enables, a nested app will also enable.
5. **Mux-forwardable by design, not by afterthought.** This is where even the successes stumbled:
   kitty keyboard is still swallowed by tmux four years after the request was filed, and OSC 133 is
   consumed rather than forwarded. The lesson: a proposal must specify middle-box behavior
   normatively — parse, scope to the pane, re-emit outward, route replies inward — and treat "the
   mux answers for the composite it delivers" as part of conformance. A mechanism whose mux story is
   `allow-passthrough` has no mux story: passthrough is outbound-only and cannot carry a reply.
6. **Bounded state.** kitty's rejection of per-line continuation prefixes
   ([discussion #9134](https://github.com/kovidgoyal/kitty/discussions/9134)) is the clearest
   statement of the maintainers' cost ceiling: unbounded per-line metadata is unacceptable. Region
   ids, one soft/hard bit per break, a short joiner — bounded, evictable with scrollback — is the
   budget proposals must live within.
7. **One actor authoritative, not two actors agreeing.** The width lesson generalizes: wherever a
   protocol requires the app and the terminal to independently compute the same answer from drifting
   tables (wcwidth, wheel ticks, wrap points), they will diverge. Protocols that let one side
   _declare_ (explicit width, declared wrap intent, declared regions) and the other side _honor_ are
   the ones that stay correct.

---

## 12. Why now: the agent-CLI workload

Terminals have absorbed application waves before — curses full-screen apps, then editors and
multiplexers, then the modern TUI frameworks — and the grid model stretched to fit each one. The
current wave is different in a specific, technical way: the agent CLI is simultaneously an **inline
transcript application** (finalized cells belong in native scrollback, where users rightly demand
native scrolling, selection, and copy) and a **rich interactive application** (streaming markdown,
collapsible tool calls, diff cells, approval buttons, a composer with IME). It needs both halves of
the split that the grid model forces apps to choose between.

The evidence that this workload is hitting the ceiling is not speculative; it is in the issue
trackers cited throughout this document, filed within the last two years by the largest CLI
deployments in the ecosystem: an agent CLI asking terminals for OSC 133 semantics it has to fake,
shipping a setup command to hack in one keybinding per terminal, losing clipboard writes and
background-color queries inside tmux, replaying its transcript on every resize against a hardcoded
table of per-terminal scrollback limits, and — in the most instructive case — building a full
app-owned viewport that fixed reflow and copy, then retiring it because native terminal behavior
across the environment matrix is the product. These teams have engineering budgets most TUI authors
never had, and they still could not buy their way out of the gap; they could only choose which half
of it to fall into. Warp's answer — leave the grid entirely — shows what the ecosystem loses if the
gap stays open: the properties that make the terminal valuable (any app, any host, any mux, one
interface) are exactly the properties the escape hatches abandon.

The opportunity is symmetrical. The pieces the applications need are mostly things terminals already
have privately — wrap flags, zone tracking, scroll deltas, the AT connection — and the extension
discipline that ships (query, event, degrade, forward, bounded state, one-actor authority) is proven
and recent. A small number of primitives, specified with multiplexer behavior from day one, would
replace a decade of per-app compensation code with a contract. The alternative is already visible:
every major CLI carrying its own quirk tables, its own scroll physics, its own region bookkeeping —
each a private, partial, mutually incompatible reimplementation of the same missing protocol. That
is the outcome this document is written to avoid.
