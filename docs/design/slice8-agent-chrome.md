# Slice 8 design: the agent-chrome flagship

Working design note for slice 8 (ROADMAP.md) — the acceptance test. A simulated coding-agent chat:
streaming markdown transcript, tool-call cells, prompt composer, inline by default, alt-screen
togglable. The thesis this example must demonstrate: **one app state, two viewport philosophies** —
inline mode commits finished cells to native scrollback (terminal owns history: scroll, select,
copy, reflow), alt-screen renders the same transcript as a retained scrolling view with collapsible
cells. That is the codex tui1/tui2 duality resolved as a mode choice instead of an architecture bet.

## Multi-span commit lines (lifting the slice-5 ceiling)

`core::text` gains `Span { text: String, style: Style }` and `CommitLine` becomes `Vec<Span>` (with
`From<&str>`/`From<(String, Style)>` preserved for compatibility — a single-span line). The inline
engine emits per-span SGR within the committed line. Unification with widget-side text (styled
`Text` content) is deliberately deferred to the catalog phase; commit lines are the only consumer
this slice.

## Markdown

Markdown-to-spans lives **in the example**, not the framework — rendering markdown is app-land until
the catalog grows a real widget. `pulldown-cmark` as a dev-dependency (examples only; the
framework's dep budget is untouched). Coverage: headings, bold/italic, inline code, fenced code
blocks (code role, no highlighting), bullets. The streaming case re-renders only the in-progress
message's accumulated source per frame — small, no incremental parsing needed.

## Streaming vs append-once

The simulated agent streams chunks via `Command::stream`. Rules:

- The **in-progress** message renders in the live tail from accumulated source, soft-wrapped to
  width. `Text` gains `wrap(bool)` (grapheme-correct soft wrap via the core width oracle) — a
  genuine widget addition, in scope.
- A message **commits when it completes** (or when a complete markdown block can no longer change —
  v1 simplification: commit whole messages on completion; block-level early commit is a recorded
  refinement). Commit = markdown-render the source to multi-span lines, emit unwrapped (terminal
  reflows). Append-once holds by construction: only completion commits.
- Tool calls: in inline mode a tool cell commits as a one-line summary
  (`▸ ran cargo test — 396 passed`, styled by status role) when the tool finishes; the full output
  is kept in app state and viewable in alt-screen. Committed scrollback is immutable — collapsing
  committed content is impossible by design, and the example's help line says so (that is the honest
  inline tradeoff, per the gap analysis).

## Alt-screen transcript view

App state holds `Vec<TranscriptCell>` (User / Assistant{source} / Tool{name, summary, output,
status}). Alt-screen renders the whole transcript as a scrollable column: a new `Collapsible` widget
(header + body; Enter or click on header toggles; collapsed state retained by identity in the store
— tool cells default collapsed, assistant cells expanded). Simple offset scroll
(Up/Down/PageUp/PageDown when the transcript has focus); no virtualization this slice (a few hundred
cells is fine; the ListSource seam is the recorded path when it matters).

## The chrome

Layout (both modes): transcript region (live tail in inline; scrolling view in alt) / status line
(mode, agent state, spinner while streaming) / prompt composer (TextInput; Enter sends, spawns the
simulated agent response; composer stays focused). `m` toggles mode; `q`/Ctrl-C quits; Esc cancels a
streaming response (`Command::cancel_group("agent")` — cancel-previous also covers re-prompting
mid-stream).

## Simulated agent

A `Command::stream` emitting: chunked markdown prose (realistic pacing via interval), then a tool-call
start/finish pair (with a sleep), then more prose, then completion. Deterministic content (seeded
from the prompt text) so the integration test can assert the transcript.

## Testing

Core/text: span commit-line encoding (per-span SGR, vt100-verified). Text::wrap: grapheme wrap incl.
wide chars at boundaries, snapshot. Collapsible: toggle by key and click, state persistence by
identity. Integration (TestApp): send prompt → inject stream messages → assert live tail; complete →
assert commit content via the engine bytes through vt100 (inline) and the transcript view (alt);
tool cell collapsed-by-default then expanded by Enter. The "what this slice revealed" note (feeds
ADR corrections before slice 9) is a required deliverable in the deltas section.

## Implementation deltas

What shipped, and — the real deliverable of this acceptance-test slice — where the design strained
once a full app leaned on it. These feed ADR corrections before slice 9.

### What shipped, as designed

- `core::text::Span { text, style }`; `CommitLine` is now `Vec<Span>` with the single-span
  constructors and `text()`/`style()` accessors preserved (`text()` concatenates, `style()` reports
  the first span). The inline engine emits one SGR per span per committed line; vt100-verified that
  a two-span line lands two distinct colors in scrollback (`row_cells` on `VtScreen`, a new
  per-cell-color assertion surface added to the harness).
- `Text::wrap(bool)`: grapheme-correct, width-aware soft wrap (word-preferring, hard-break for an
  over-long word, wide graphemes never split). Snapshot pinned.
- `Collapsible`: header + body, Enter/header-click toggle emitting `Outcome::Toggled(collapsed)`,
  collapsed state retained by identity, `default_collapsed(bool)` applied once on first render then
  never re-clobbered (an `initialized` flag in the retained state).
- `examples/agent.rs`: the full chrome, both modes off one `cells: Vec<TranscriptCell>`, markdown →
  spans over `pulldown-cmark` (dev-dep), a deterministic `Command::stream` agent,
  `cancel_group("agent")` on Esc / re-prompt, a spinner ticker stream.

### Where the design strained (the honest findings)

1. **`Attributes` is a write-only bitset — nested inline markdown needs removal.** The markdown renderer
   must clear `BOLD` when a `Strong` span ends while a surrounding heading keeps its own `BOLD`. But
   `Attributes` exposes only `|`/`|=` and `contains`; there is no `remove`/`&`/`!`. The example and
   the integration test had to reconstruct the complement by iterating the known flags — a smell
   that recurs for any nested-styling consumer. **Correction for slice 9:** give `Attributes` `remove`,
   `BitAnd`, and `Not` (or a `toggle`), the minimal closure of a real bitset. Low-risk, additive.

2. **Variable-height cells have no measurement, so the alt transcript is a hand-rolled fixed-slot
   stack.** This is the sharpest strain. A `Collapsible` is 1 row collapsed and N rows expanded, but
   the frame offers only `split_rows` with pre-decided constraints — no `desired_height(width)`
   intrinsic measurement (ADR 0004 deferred it) and no scrollable viewport widget. The example gives
   every cell a fixed 4-row slot: collapsed cells waste three rows, a large tool output is clipped,
   and "scroll" is a raw `skip(offset)` over whole cells rather than a smooth row scroll. The design
   said virtualization is "the recorded `ListSource` seam when it matters" — but the flagship shows
   that _even non-virtualized_ variable-height stacking needs (a) intrinsic height and (b) a scroll
   container, neither of which exists. **Correction:** slice 9 (catalog/containers) must land
   `desired_height(width)` measurement and a `ScrollView`/`Column` that lays children by their
   measured heights; the `Collapsible` cell is unusable in a real transcript without it.

3. **The live tail and the committed line are two different renderers, so styling _pops_ at
   commit.** The in-progress message renders as plain wrapped `Text` (no markdown); on completion it
   commits as markdown spans. The user watches plain prose turn into a bold heading the instant it
   commits. The design accepted "re-render accumulated source per frame as plain," but the
   discontinuity is visible and slightly jarring. The root cause is #4: there is no way to _wrap a
   styled multi-span line_, so the live tail cannot show markdown without a styled-wrap primitive.

4. **`Text::wrap` wraps one `&str` in one style; there is no styled-span wrap.**
   `Span`↔widget-`Text` unification was deliberately deferred, and it bit exactly here:
   soft-wrapping a `Vec<Span>` (for a styled live tail, or for wrapping a committed markdown line to
   a narrow alt column) has no path. `Text::wrap` produces `Vec<String>`; commit lines are
   `Vec<Span>`; the two wrap oracles are unshared. **Correction:** when the catalog grows the real
   text/markdown widget, wrapping must operate on spans, and `Text` should render `Vec<Span>` rather
   than `&str` so there is one wrap implementation, not two.

5. **Whole-message commit means an over-tall streaming message overflows the bounded tail
   invisibly.** v1 commits on completion only (append-once by construction — the clean part). But
   the live tail is bounded (`TAIL_HEIGHT`) while the in-progress _content_ is not: a long streaming
   answer scrolls its own top out of the tail before it commits, and that content is nowhere until
   completion. Block-level early commit (a recorded refinement) is the fix; the flagship confirms it
   is a real need, not a nicety, for any answer taller than the tail.

6. **Two coupled streams with no self-termination.** The spinner is a second stream (`"spinner"`
   group) the app must manually stop when the _agent_ stream completes — the completion signal
   arrives on the agent stream, not the spinner. There is no "derive a subscription from state," so
   start/stop is bookkeeping the app carries (`ticking` flag + `cancel_group` on every exit path,
   including Esc and error). ADR 0005's "a subscription is just a stream you chose to start" is true
   but understates the manual lifecycle coupling when one stream's end must stop another. Not a
   correction so much as a documented cost; a `Command` that ends when a predicate over state flips
   would remove it.

7. **The inline/alt asymmetry is correct but stark.** Committed scrollback is immutable, so the same
   tool cell is a collapsible disclosure in alt and a frozen one-line summary in inline — the honest
   tradeoff the design named, and the example's help line says so. Working as intended; noted
   because the flagship makes the asymmetry the first thing a user notices when they press `m`,
   which is exactly the thesis (one state, two philosophies) landing as designed.

Net: the multi-span commit lift (item 1's `Attributes` gap aside) and `Collapsible` are sound. The design
under-provisioned **layout for variable-height retained content** (#2) and **styled text wrapping**
(#3/#4); both are catalog-phase work the transcript cannot be production-quality without. Those two
are the corrections slice 9 should absorb before the widget catalog is called done.
