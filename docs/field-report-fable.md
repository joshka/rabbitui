# The Terminal Settled Its Arguments: A Field Report on TUI Frameworks in 2026

Date: 2026-07-06

_Methodology: this piece is synthesized from a structured survey of thirteen framework studies —
Textual, Ink, Bubble Tea, Ratatui, Cursive, Brick, libvaxis, OpenTUI, Codex's tui2, the Rust GUI
literature, the terminal protocol substrate, pre-2024 Rust framework attempts, and a sweep of the
2024–2026 Rust TUI wave — sourced from public issue trackers, discussions, blog posts, release
notes, and source code. Claims carry their citations inline._

---

Something strange happened to terminal user interfaces around 2024: they became the most contested
UI surface in software. Not because anyone loves ANSI escape codes, but because the flagship
applications of the AI era — Claude Code, Codex CLI, Gemini CLI, opencode — all live in the
terminal, ship to millions of developers, and stream more rich text per second than any TUI in
history. The money and engineering hours followed. Anthropic spent roughly a year rebuilding a
renderer. OpenAI rewrote its CLI in another language, then ran and retired a full viewport
rearchitecture. Charm shipped Bubble Tea's first breaking release in six years. And in the Rust
ecosystem alone, more than sixty new TUI frameworks appeared in twenty-four months — about a third
of them substantially AI-generated.

This report is an attempt to say what all of that taught us. The short version: the field quietly
converged on a handful of load-bearing truths that every serious framework now shares, a few
questions remain genuinely open, and the thing that will decide the next five years is not
architecture at all.

## The wave is a demand survey, not a competition

Start with the strangest dataset. Between 2024 and 2026, one sweep catalogued 63 new Rust TUI
frameworks: Elm clones (at least ten, two of them — bubbletea-rs and hojicha — published in the same
week), React/Ink clones (three independent ones in six months), two Turbo Vision revivals in a
fortnight, three independent CSS-for-the-terminal engines in a single year, signals frameworks, ECS
frameworks, immediate-mode closures. Commit forensics — day-one multi-crate publishes, 963 commits
against 1 star, verbatim-duplicated README boilerplate across unrelated repos — mark roughly a third
of the 2025–2026 entries as substantially AI-generated.

The temptation is to dismiss the vibe-coded third as noise. That's exactly wrong, and it's the most
useful reframing the survey produced: **a vibe-coded framework is a compressed replay of every "I
wish ratatui had X" thread the model was trained on.** The AI-generated missing-feature lists
(ratada, weavetui) match the lists a human author (thscharler's
[rat-salsa](https://github.com/thscharler/rat-widget) family) had been shipping crate-by-crate
since 2024. The AI Elm clones match the human Elm clones. When sixty projects of mixed provenance
independently converge on the same feature list — event loop, focus management, forms and text
input, modals and overlays, theming, an MVU-or-hooks authoring layer, inline rendering — that
convergence _is_ the survey result. The wave is not sixty competitors. It is the largest demand
study ever run on the gap between ratatui (a superb rendering substrate that
[describes itself as not a framework](https://news.ycombinator.com/item?id=38593638)) and what
application authors actually need.

The demand, ranked by independent recurrence:

| Want                                                              | Evidence                                                                                                                                                                                              |
| ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The "missing middle": event loop, focus, forms, overlays, theming | Identical crate lists from rat-salsa (2024, human) and ratada (2026, AI); [ratatui-interact](https://github.com/Brainwires/ratatui-interact) at 22.6k downloads in 6 months for exactly focus + mouse |
| Batteries-included widget catalog                                 | "You take on third party dependencies for each individual widget... spinners, checkboxes, text areas" ([HN](https://news.ycombinator.com/item?id=45830829))                                           |
| Bubble Tea envy (MVU runtime)                                     | 10+ TEA clones in 18 months; "Go developers get Charm's elegant ecosystem... while Rust developers suffer" ([charmed_rust](https://github.com/Dicklesworthstone/charmed_rust))                        |
| React/Ink-style components with hooks                             | ratatui-kit, rnk, [eye-declare](https://github.com/atuinsh/eye-declare) (126 stars in 3 months, from the Atuin team), ntui, reratui                                                                   |
| Inline, scrollback-preserving rendering                           | The sharpest _new_ demand — see below                                                                                                                                                                 |
| Pretty by default                                                 | Catppuccin/Nord/Dracula presets expected out of the box; a lipgloss-flavored theme layer ([ratatui-bubbletea](https://github.com/akitaonrails/ratatui-bubbletea)) got 25 stars in a weekend           |

Notice what is _absent_ from that list: nobody is asking for a novel reactive architecture. Fifty
frameworks now ship signals, hooks, TEA, ECS, and immediate-mode closures, and none of it moves
adoption. Hold that thought.

## Three expensive lessons, paid in production

The most instructive evidence of the era doesn't come from the wave — it comes from three teams with
millions of users who ran multi-year experiments at full price.

### Ink and the year of renderer surgery

Ink — React for CLIs — won the JavaScript terminal by DX: real React components, hooks, Yoga
flexbox. Claude Code, Gemini CLI, GitHub Copilot CLI, and Shopify's CLI all shipped on it. But Ink's
rendering substrate is "render the whole tree to a string, then diff at the end": every state change
re-renders React, re-runs layout, repaints every cell, builds a full frame string, and only _then_
decides what to write. Its historical writer erased the previous frame's lines and rewrote
everything — and content taller than the viewport
[has flickered badly since 2020](https://github.com/vadimdemedes/ink/issues/359), an issue that is
still open, because erase-and-rewrite structurally cannot touch what has already scrolled into
scrollback.

At Claude Code's scale this became a saga. Per
[public teardown accounts](https://steipete.me/posts/2025/signature-flicker), Anthropic replaced
Ink's renderer with a custom one, added cell-level differential rendering, used packed TypedArrays
to dodge GC-pause artifacts, and pushed synchronized-output (DEC mode 2026) patches upstream to VS
Code's terminal and tmux. After a year of that, the fix that actually worked —
`CLAUDE_CODE_NO_FLICKER=1` — was the alternate screen buffer, "like every other TUI." Gemini CLI
accumulated its own [flicker bug tax](https://github.com/google-gemini/gemini-cli/issues/10673) and
asked, in its rendering epic, for exactly what buffer-diffing TUI libraries do natively.

Two details keep this from being a simple "Ink bad" story. First, when Anthropic ripped out the
renderer, they _kept React as the component model_ — the DX layer was worth preserving; only the
paint layer was broken. Second, when OpenAI's Codex CLI left the stack entirely for Rust, the
[maintainer-stated reasons](https://github.com/openai/codex/discussions/1174) were about the Node
runtime — the Node v22+ install requirement, garbage collection and memory, native sandboxing
bindings — not about rendering. The component model won; the string-diffing substrate lost; and the
runtime is a separate axis from both.

### Bubble Tea concedes the cell grid

Bubble Tea is the most teachable TUI framework in existence — three methods, 43.6k stars, 18k+
dependents — and for six years its `View()` returned one big string, diffed line-by-line at 60fps.
In February 2026,
[v2 shipped the first breaking change in the framework's history](https://charm.land/blog/v2/), and
it is a concession on both of Bubble Tea's founding simplicities. Rendering moved to the "Cursed
Renderer," a real cell grid "based on the ncurses rendering algorithm" with damage-based updates —
string diffing, retired by its own team. And `View()` no longer returns a string: it returns a
`tea.View` struct carrying content _plus declarative terminal state_ — cursor position and shape,
alt-screen, mouse mode, keyboard enhancement level, window title. The imperative escape-command soup
(`tea.EnterAltScreen` and friends) is gone; the view is the single source of truth.

Bubble Tea's history also contains the field's best evidence _against_ a piece of Elm orthodoxy: it
had Elm-style subscriptions in early 2020 and
[deleted them](https://github.com/charmbracelet/bubbletea) (commit `ade8203c`: "we can achieve the
same functionality in a much simpler fashion with commands"). Six years later nobody misses them — a
recurring effect is a command that re-issues itself; a stream is a command pumping a channel. One
effect primitive turned out to be enough. What Bubble Tea never fixed is composition:
[#176](https://github.com/charmbracelet/bubbletea/discussions/176),
[#751](https://github.com/charmbracelet/bubbletea/discussions/751), and
[#707](https://github.com/charmbracelet/bubbletea/discussions/707) — the last, on sharing state
between sibling components, received zero replies — document five-plus years of every serious app
hand-rolling a message router, a focus index, and a component runtime the framework declines to own.

### Codex tui2: the era's most instructive negative result

The deepest lesson comes from an experiment that _worked_ and was killed anyway. Codex CLI's TUI
keeps finalized history in the terminal's own scrollback — native scrolling, native selection,
native copy — with a live inline viewport below. That design inherited every terminal's quirks:
history printed pre-wrapped at the width-of-the-moment went out of sync on resize ("some lines were
lost or overwritten, others were duplicated"), and clear-and-rewrite founders on terminals that
treat "clear" as a suggestion.

So the team built tui2
([pinned tree here](https://github.com/openai/codex/tree/41e38856f6c11679d75bd63f6eef1e0ea76dffeb/codex-rs/tui2/docs)):
the app owns the viewport; the in-memory transcript of width-agnostic logical cells is the single
source of truth; wrapping happens at render time per current width; selection lives in
content-relative coordinates; copy reconstructs markdown source — backticks and fences — even where
the UI renders them away, using explicit soft-wrap-vs-hard-break metadata plumbed from the wrapping
algorithm to the clipboard. It won decisively on resize rewrap and copy fidelity, especially for
code.

And then they retired it. The retirement commit (PR
[#9640](https://github.com/openai/codex/pull/9640), January 2026) is worth quoting because
production teams rarely write this honestly: _"What worked: a transcript-owned viewport delivered
excellent resize rewrap and high-fidelity copy... Why stop: making that experience feel fully native
across the environment matrix (terminal emulator, OS, input modality, multiplexer, font/theme,
alt-screen behavior) creates a combinatorial explosion of edge cases."_

The single best artifact of that combinatorial explosion is tui2's scroll study: 16 probe logs,
**13,734 scroll events across 8 terminals**, establishing that terminals emit anywhere from 1 to 9+
raw events per physical wheel notch (Apple Terminal 3, Warp 9, WezTerm 1) and that _wheel and
trackpad input are timing-inseparable_ — no heuristic can reliably tell them apart, so the shipped
design required a user-facing override. That is what "own the viewport" actually costs: you
reimplement the mouse wheel, per terminal, forever. Meanwhile users were filing issues like
[#8344](https://github.com/openai/codex/issues/8344): "Terminal is king because anything works
anywhere. Don't break scrolling, copy/paste, for crying out loud." tui2 solved the rendering problem
and lost the compatibility problem, and for a CLI shipping to every terminal on earth, the second
one is the product. Native terminal behavior is a feature users will defend with pitchforks.

## What everyone learned independently

Lay the histories side by side and the convergences are almost embarrassing — a dozen teams, in five
languages, over fifteen years, arriving at the same place by different roads.

| Convergence                                                                                | Independent arrivals                                                                                                                                    |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Render to a cell grid; diff double buffers; wrap frames in synchronized output (mode 2026) | ratatui (native), Bubble Tea v2, Claude Code's custom renderer, OpenTUI's Zig core, libvaxis, Textual's compositor                                      |
| Widget identity must be framework-owned, stable data                                       | Brick's name type, Xilem's id-paths, Masonry's arenas, vxfw's pointer regret, Cursive's string selectors, Bubble Tea's unanswered #707, tui2's cell IDs |
| Terminal state is declarative render output, not imperative side effects                   | Bubble Tea v2's `tea.View`, Brick's cursor-candidate bubbling, vxfw's command lists                                                                     |
| Capabilities are negotiated by query, not looked up in terminfo                            | libvaxis, vaxis (Go), termina/Helix, Ink's kitty handshake                                                                                              |
| Inline (scrollback-preserving) rendering is a first-class mode                             | Ink's `<Static>`, Codex tui1, eye-declare, rnk, FrankenTUI, OpenTUI's split mode                                                                        |
| A headless test driver ships with the framework                                            | Textual's Pilot + SVG snapshots, teatest, Cursive's puppet backend, Masonry's TestHarness, OpenTUI's test renderer, tui2's vt100-emulator backend       |

Two of these deserve unpacking, because they invert popular intuitions.

**Retained mode does not buy you partial redraw — and damage regions are worthless on a cell grid.**
Cursive, Rust's venerable retained-tree framework, re-layouts and redraws its _entire_ tree every
refresh; users discovered this when
[a frame cost 174 KB over SSH versus ncurses' 11 KB](https://github.com/gyscos/cursive/issues/667),
and the fix was a buffer diff bolted on afterward — the supposed structural advantage of retention
was simply absent. From the other direction, the Rust GUI literature (Druid's damage-region work,
Flutter's repaint boundaries) turns out not to transfer: a terminal frame is at most ~100k cells,
diffing two buffers takes microseconds, and **the diff _is_ the damage tracking**, computed for free
after the fact. Every framework that built explicit invalidation machinery for the paint layer was
solving a GPU-era cost class terminals don't have. The place incrementality actually matters is
upstream — skipping the _view construction and layout_ work — which is exactly where Ink,
re-rendering React and Yoga before its too-late string diff, couldn't cut costs.

There's a poignant coda: when tui-rs was dying, its own maintainer
[conceded retained mode's advantages and suggested the community adopt Cursive](https://github.com/fdehau/tui-rs/issues/654).
The community built ratatui instead. Immediate mode won Rust's mindshare anyway — not because it was
architecturally superior, but because it dodged the borrow-checker fight that makes retained trees
in Rust a wasteland of `Arc<Mutex>`, deferred callbacks, and runtime panics.

**Widget identity is the one GUI problem that arrives in the terminal undiminished.** Focus, scroll
persistence, mouse routing, async-reply addressing, accessibility — all of them need a widget to be
"the same widget" across frames. Seven independent framework histories converge here. Brick
(Haskell) got it right earliest: a single user-defined name type keys scroll offsets, reported
geometry, hit regions, caching, and focus. Xilem arrived at id-paths; Masonry's hardest-won 2024
lesson was moving to arena keys. vxfw (Zig) used raw pointer identity and its users
[immediately dangled stack pointers](https://github.com/rockorager/libvaxis/discussions/232).
Cursive used string names with silent-failure lookup. Bubble Tea has no answer at all — that's what
\#707's silence means. And tui2's `HistoryCell` indices are the same idea rediscovered for
transcripts. The GUI literature adds the sharpest version of the argument: accessibility APIs assume
a retained, stably-identified tree, which "eliminates pure immediate mode as viable for production"
in Raph Levien's [analysis](https://raphlinus.github.io/rust/gui/2022/07/15/next-dozen-guis.html) —
though the terminal version of that claim remains untested, since no one has built an AccessKit
export for a TUI ([ratatui #2190](https://github.com/ratatui/ratatui/discussions/2190) is
exploratory). Performance never kills immediate mode. Identity does.

## The substrate is being renegotiated

Underneath the frameworks, the terminal protocol layer is going through its biggest shift since
xterm: **terminfo is dying, replaced by asking the terminal directly.**
[libvaxis](https://github.com/rockorager/libvaxis) — written by the author of the in-band-resize
spec — states it flatly: it "does not use terminfo"; features are detected through queries, fenced
by a trailing DA1 (every terminal answers DA1, so its arrival means all prior answers are in), with
behavioral probes for features that have no query form: emit the sequence, ask where the cursor is,
see if it moved. The old database earned this distrust — Helix ships a
[hardcoded "is it WezTerm" check](https://github.com/helix-editor/helix/pull/13224) because terminfo
lies about undercurl, and moved to the low-level [termina](https://github.com/helix-editor/termina)
backend specifically to adopt new protocols without waiting on a library.

The query-based world has one hard engineering requirement the incumbent Rust backend never met:
query replies arrive interleaved with keystrokes on the same file descriptor, so the input decoder
and the query router must be one component. Crossterm's chronic races —
[cursor position queries failing under PTY forwarding](https://github.com/crossterm-rs/crossterm/issues/963),
[a lone ESC byte misread as a keypress depending on syscall chunking](https://github.com/crossterm-rs/crossterm/issues/993)
— are all the same missing router. Add the multiplexer middle-box (tmux answers DA1 itself but
[silently swallows kitty-keyboard queries](https://github.com/tmux/tmux/issues/3335); tmux ≤3.5
doesn't answer DECRQM at all, so unbatched probing stalls once per swallowed query) and "probe
honestly, fence with DA1, let users override" is the only strategy left standing.

The unfinished business is width. What a grapheme cluster occupies is a _contract_ between program
and terminal, and today it's negotiated badly or not at all: mode 2027 (grapheme clustering) is
implemented by foot and contour,
[tracked but unimplemented in WezTerm](https://github.com/wezterm/wezterm/issues/4320), superseded
in kitty by a different explicit-width protocol, and invisible to tmux. libvaxis treats width method
itself as a negotiated capability with env-var overrides for known liars; almost nobody else does,
and every framework that hardcodes one width table is one ZWJ emoji away from cursor desync. This is
the least glamorous open problem in the field and arguably the most important.

## What's still genuinely contested

**The programming model.** This is the honest fork. Textual proves
retained-DOM-plus-reactive-attributes at real application scale (harlequin, posting, memray). The
Xilem school argues for an ephemeral view tree diffed against a retained core. Immediate mode plus
framework-owned per-ID state — Brick's model, and the direction of unpublished experiments by a
ratatui maintainer that route input through the previous frame's rendered facts — holds that on a
cell grid, the retained tree is optional as long as identity is data. The strongest empirical
statement available: every option needs stable identity and a home for retained state, pure
per-frame immediate mode is unviable past a certain app size, and pure hand-routed Elm dies of
composition. Between the three survivors, no terminal-domain evidence yet adjudicates.

**Styling.** Textual's CSS — with live reload — is its most-loved feature and its ecosystem moat.
But Ink's five-year tracker contains essentially zero demand for selectors or cascade; its ecosystem
converged on semantic tokens passed through context, and Gemini CLI shipped user themes that way.
Brick covers matterhorn-scale apps with hierarchical attribute names and INI overrides — 90% of
Textual CSS at 10% of the machinery. Meanwhile three independent CSS-for-terminal engines appeared
in 2026 alone. The demand for _theming_ is unambiguous; whether it needs a cascade engine is not.

**Layout is quietly settled, actually — just not the way anyone predicted.** Nobody wants a
constraint solver: ratatui's cassowary inheritance is a
[documented source of pain](https://github.com/ratatui/ratatui/discussions/1933) (unmaintained
upstream, overconstraint panics, an org-maintained fork). But nobody strictly needs flexbox either:
Textual's entire ecosystem shipped on dock + linear + grid + fractional units — computed with exact
`Fraction` arithmetic so `1fr 1fr 1fr` never leaves a one-cell gap — and Ink's five years of issues
contain approximately zero layout-semantics complaints against plain Yoga. The real gap everyone hit
is _content measurement_: ratatui's layout cannot see text size, which is why Codex's tui2 had to
invent `desired_height(width)` on top of it. Any layout vocabulary works; layout that can't measure
text doesn't.

**Viewport ownership.** tui2's retirement doesn't settle this; it prices it. App-owned scrollback is
the only route to faithful copy, rewrap, and per-cell interaction — and it costs you the terminal
matrix. The unclaimed design is a seam that lets an app start terminal-native and escalate to owned
per-feature, instead of the all-or-nothing choice tui2 was forced into.

## What changed in the last two years: AI, three ways

AI entered the field in three distinct roles, and conflating them muddles the analysis.

**As workload.** The defining application of 2025–26 is the coding-agent CLI: streaming markdown
transcripts, diff views, tool logs, approval modals, a pinned composer. This workload made _inline
rendering_ — a live UI region that cooperates with terminal scrollback instead of hiding in the alt
screen — the sharpest new demand in the field. It's what `<Static>` approximates in Ink (whose
overflow flicker has been [open since 2020](https://github.com/vadimdemedes/ink/issues/359)), what
alt-screen-centric designs like ratatui structurally underserve, what eye-declare and rnk now make
the _default_, and what agent vendors keep rebuilding in-house (a3s-tui, capo-tui) because nothing
off-the-shelf serves it. The era's users want their scrollback, their Cmd-F, and their copy/paste;
the framework that treats inline mode as a peer of alt-screen, rather than a footgun with a warning
label, is serving the actual workload.

**As author.** A third of the wave is AI-generated, and the reception pattern is instructive.
[FrankenTUI](https://github.com/Dicklesworthstone/frankentui) — an AI mega-framework with 20 crates
and 80+ widgets, pitched on exactly the right thesis (correct, flicker-free inline UIs) — got this
from an HN tester:
["if you look visually it looks like it's working. Once I tried interacting with it, everything is broken in a subtle way."](https://news.ycombinator.com/item?id=46986644)
That sentence is the economics of the era in miniature. **AI made widget breadth free; verification
is the new scarce good.** The disciplined tail of the wave understood this and built the most
interesting tooling in the survey: textual-rs verifies its Textual port with a real-PTY cell-grid
parity harness against the Python original; [inkferro](https://github.com/metaphorics/inkferro)
validates its Rust Ink engine with byte-golden conformance tests plus a 22,500-comparison
differential fuzz against a live Node oracle. Correctness — CJK width, focus, resize,
mouse-in-overlays, Windows key handling — is the moat now, and the frameworks that can _prove_ it at
the PTY level are the ones reviewers stopped being able to break.

**As author-population.** Frameworks have begun designing for AI agents as their users:
SuperLightTUI advertises a "small public grammar... easily inferrable from documentation";
ratatui-kit ships (and evals) an agent skill for itself; testty exists so agents can verify TUIs
they cannot see. This is genuinely new. The framework that coding agents can use correctly by
default will win a compounding share of new projects — LLMs already recommend ratatui unprompted,
which is itself a network effect nobody designed.

## Adoption physics

Against all this engineering, the adoption data is almost insultingly simple.

[rooibos](https://github.com/aschey/rooibos) is the complete synthesis on paper — fine-grained
signals, ratatui rendering, flexbox, async-first, a testing library, terminal/SSH/web backends. It
has **five stars**. Meanwhile the loudest requests in ratatui's HN threads are for spinners,
checkboxes, text areas, and "a widget library and event loop that I like"
([HN 45830829](https://news.ycombinator.com/item?id=45830829)). Textual's real moat was never its
compositor; it was a curated gallery of 35 widgets, CSS theming, devtools, and documentation. The
catalog is the product. Architecture is table stakes that users cannot see.

The wave adds forty more data points to two older laws. **Layering wins:** the entries with real
traction sit on ratatui (Ratzilla at 1,401 stars, rat-salsa, eye-declare, ratatui-interact) and
preserve incremental adoption; from-scratch stacks cap at roughly 400 stars regardless of quality.
**One person, one app kills:** zi died with the zee editor; tui-realm lives exactly as long as
termscp needs it; bubbletea-rs paused after four months with a single open issue begging for
co-maintainers. Even Textual — 36.5k stars, the category's biggest success — wound down as a company
in [May 2025](https://textual.textualize.io/blog/2025/05/07/the-future-of-textualize/) ("struggled
to identify a viable commercial problem") and survives as one person's maintenance project, still
shipping (v8.2.8, June 2026) but with structural issues like the
[800× DataTable gap](https://github.com/tconbeer/textual-fastdatatable) unfixed for years while the
community forks widgets around core. Notably, even Textualize routed around its own widgets when
performance mattered: toolong ships a custom log ScrollView because gigabyte files need
virtualization the stock widgets never got.

## What would actually move the field

Not another architecture. The 2024–2026 wave falsified that hypothesis sixty times.

Five things would.

**1. A conformance suite for the terminal matrix.** tui2's 13,734-event scroll study, textual-rs's
PTY parity harness, and inkferro's differential fuzzing are three ad-hoc versions of the same
missing public good: an executable definition of "behaves correctly in a terminal" — width and
graphemes, resize and reflow, scroll physics, mux degradation, cleanup on crash. Whoever publishes
the harness sets the bar every framework gets measured against, and gives AI-authored code the
verification substrate it needs to be trustworthy.

**2. Inline mode as a specified discipline, not a hack.** The field knows the shape now: an
append-only region committed to scrollback exactly once, a bounded live tail that never exceeds
viewport height, synchronized-output framing, strict cleanup. Ink discovered it (`<Static>`), Codex
refined it (cell-level high-water marks so width changes never drop or duplicate history), and every
agent CLI needs it. It deserves to be a renderer invariant with a name, and possibly a terminal-side
protocol extension.

**3. Finishing the width negotiation.** Mode 2027, kitty's explicit width, tmux support, and one
honest answer to "how wide is this grapheme _here_." Every framework carries scar tissue from this;
only terminal authors and framework authors together can retire it.

**4. An identity layer over the incumbent substrate.** The convergence evidence says framework-owned
stable IDs — keying focus, geometry, scroll state, hit-testing, and eventually accessibility — are
the one thing every architecture needs and the one thing ratatui's 36-million-download gravity well
doesn't provide. Shipped incrementally, one crate at a time, with the widget catalog HN keeps asking
for on top: that is the framework-shaped hole in the ecosystem, and the adoption physics say it must
be layered, not greenfield.

**5. Accessibility as the forcing function.** In a sixty-framework wave, not one project addressed
assistive-technology semantics; the sole gesture in the entire survey is Bubble Tea's huh, which
degrades forms to sequential prompts. A retained, identified tree is precisely what a screen-reader
export requires — meaning the first framework to take accessibility seriously will also,
incidentally, settle the programming-model argument. It is both the field's largest ethical gap and
its most likely architectural tiebreaker.

The terminal spent forty years as the UI layer nobody would invest in, kept alive by muscle memory
and ssh. It took language models — software that types faster than we read — to make it a
first-class product surface again, and the result is that problems which sat unexamined since curses
are finally getting the engineering they deserved all along. The frameworks of 2026 mostly agree on
how to paint. The next ones will be judged on whether they can prove, to a harness and to a screen
reader and to an agent that cannot see the screen at all, that what they painted is true.
