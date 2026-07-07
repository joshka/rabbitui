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

The simulated agent streams chunks via `Cmd::stream`. Rules:

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
streaming response (`Cmd::cancel_group("agent")` — cancel-previous also covers re-prompting
mid-stream).

## Simulated agent

A `Cmd::stream` emitting: chunked markdown prose (realistic pacing via interval), then a tool-call
start/finish pair (with a sleep), then more prose, then completion. Deterministic content (seeded
from the prompt text) so the integration test can assert the transcript.

## Testing

Core/text: span commit-line encoding (per-span SGR, vt100-verified). Text::wrap: grapheme wrap incl.
wide chars at boundaries, snapshot. Collapsible: toggle by key and click, state persistence by
identity. Integration (TestApp): send prompt → inject stream messages → assert live tail; complete →
assert commit content via the engine bytes through vt100 (inline) and the transcript view (alt);
tool cell collapsed-by-default then expanded by Enter. The "what this slice revealed" note (feeds
ADR corrections before slice 9) is a required deliverable in the deltas section.
