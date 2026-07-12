# Wave G — archetype showcase: rabbitui vs raw ratatui, with evals

Written 2026-07-11 on Fable (author-requested). Faked-out example apps per archetype,
each built **twice with identical behavior** — rabbitui and raw ratatui — plus a
measurement script and a judged eval protocol. The claim under test, in the author's
words: rabbitui should win on **lines of code and developer ergonomics while being
understandable and obvious — succinctness must not get in the way of readability.**

**The honesty rule is load-bearing.** The author is a ratatui maintainer; a strawman
ratatui version is worthless and embarrassing. Each ratatui pair is written as a
competent ratatui developer would (idiomatic, informed by the ratatui examples repo and
templates), reviewed against that bar before any numbers are published. **Decided
(author, 2026-07-11): the author personally reviews every ratatui version; eval grading
is done by a strong model with author spot-checks.** If ratatui wins
an axis somewhere, that is a finding, not a failure — record it in
`docs/design/dogfood-findings.md` and fix the framework, not the benchmark.

## Structure

One standalone crate `comparisons/showcase/` (detached from the root workspace like
`comparisons/rabbitui`), depending on both `rabbitui` and `ratatui`+`crossterm`, with one
binary per archetype × framework:

```text
comparisons/showcase/src/bin/
  picker_rabbitui.rs      picker_ratatui.rs
  form_rabbitui.rs        form_ratatui.rs
  dashboard_rabbitui.rs   dashboard_ratatui.rs
  streamer_rabbitui.rs    streamer_ratatui.rs
```

Shared fake data lives in `src/lib.rs` (both versions consume the same source, so LoC
deltas are pure framework plumbing). Already covered elsewhere, don't duplicate: the
log-follower (`comparisons/rabbitui`), `examples/simple.rs` (Wave F), the flagship
(full-scale agent chrome).

## The four pairs (behavior specs — both versions must match exactly)

1. **Picker** (fuzzy palette; the fzf idiom, keybinding-conventions §3). Fake source:
   500 command names. Behavior: type-to-filter live; Up/Down + Ctrl-N/P move; Enter
   prints the pick to stdout and exits; Esc/Ctrl-C cancels; selection follows the
   filtered view without resetting on every keystroke (the labs' stale-index bug class);
   empty-filter state shows all, no-match shows a placeholder.
2. **Form** (settings wizard; the sharpest ratatui gap). Fields: name (required), email
   (validated on edit), port (numeric), a checkbox, Save/Cancel buttons. Behavior: Tab
   order top-to-bottom; per-field error line appears/disappears as validity changes;
   Save disabled-looking until valid; Enter on Save opens a confirm modal (its own
   focus); confirm prints the values and exits.
3. **Dashboard** (monitor). Fake source: 3 metric streams ticking at different rates +
   a 10k-row event table. Behavior: metrics update live without input; table scrolls
   (PageUp/Down, Home/End) and stays responsive at 10k rows; `1`–`4` switch theme live;
   `q` quits. (Table variant lands after Wave B2; ship the metrics-only version first.)
4. **Streamer** (agent-mini; the inline-mode delta — ratatui is alt-screen-centric, so
   this pair is where the architectural gap shows). Behavior: fake token stream renders
   into a live tail; completed paragraphs commit to native terminal scrollback (scroll
   back with the terminal, copy with the mouse, output survives exit); a bottom input
   submits a new prompt; Ctrl-T toggles into alt-screen browse and back without
   duplicating the tail. The ratatui version implements the closest honest equivalent
   (`Viewport::Inline` + `insert_before`) and the write-up documents where the
   equivalence ends.

## Measurement (mechanical, scripted — `just showcase-metrics`)

Emit a committed markdown table (`comparisons/showcase/METRICS.md`) per pair:

- **LoC** — nonblank, noncomment (tokei or `grep -cve '^\s*$\|^\s*//'`), lib excluded.
- **Plumbing LoC** — lines annotated `// plumbing` during authorship review: event-loop
  scaffolding, focus bookkeeping, redraw scheduling, state mirrors. The annotation is
  subjective; both versions get the annotation pass from the same reviewer in the same
  sitting, criteria written at the top of each file.
- **State fields** the app tracks that a framework could own (focus index, scroll
  offset, selection mirror, dirty flags, started flags).
- **Concept count** — distinct framework types/functions imported.
- **Behavior-parity checklist** — the spec above as checkboxes, verified per version.

No LoC target is promised in advance: measure honestly, publish whatever it is. The
_expectation_ is a ≥2× plumbing-LoC win and near-zero framework-ownable state fields on
the rabbitui side; if a pair misses that, it is a dogfood finding.

## Evals (judged — the drilling questions)

Protocol (precedent: `skills/rabbitui/evals.md`): one **fresh** agent per question per
version — never show the same judge both versions of the same question, and never let a
judge see METRICS.md. Grade with a strong model or the author. Record runs in
`comparisons/showcase/EVALS.md` (question, version, verbatim answer, grade, turns).

Question bank (ask against each version independently):

- _Comprehension_: "Where does focus go when the user presses Tab? Cite the line(s)."
  "What happens to the selection when the filter narrows?" "What repaints when a metric
  ticks — and what decides that?"
- _Modification (the ergonomics test — grade the diff size and whether the judge got it
  right first try)_: "Add a phone field with required validation to the form." "Add
  Home/End to the picker." "Make the dashboard table jump to the newest row on tick."
- _Bug-hunt_: "The selection lands on the wrong item after the filter changes — where do
  you look first?" "A modal is open and Ctrl-C does nothing — why might that be?"
- _Prediction_: "The terminal resizes while the stream is mid-paragraph — what does the
  user see?" "Two keys arrive in one read — can an update be lost?"
- _State audit_: "List every piece of UI state this app tracks by hand. Which of those
  could a framework own?"
- _Readability_: "Explain `update` (or the event loop) to a new team member in five
  sentences or fewer — do you have to mention anything you'd call boilerplate?"

Scoring per question: correct/incorrect, turns-to-answer, and a 1–5 "obviousness" grade
(5 = pointed at the exact line unprompted). The published claim is the aggregate, not
cherry-picked wins.

## Acceptance criteria (what done and good look like)

Done requires ALL of:

1. Behavior parity: every checklist item verified on both versions of a pair (betamax
   for visual/interaction items; the FakeDevice harness where scriptable).
2. Both versions idiomatic: the ratatui version reviewed against ratatui's own examples
   style; the rabbitui version uses the §2 canonical update shape and `impl App` (so
   this wave sequences **after Wave A**; dashboard's table variant after B2).
3. `METRICS.md` committed with the table and the annotation criteria; numbers not
   massaged — plumbing annotations reviewable in the diff.
4. At least one full eval run recorded in `EVALS.md` (every question, both versions,
   fresh judges), with the aggregate summarized at the top.
5. The looks-good bar (pillar 7): both versions themed and presentable — yes, the
   ratatui version too; ugliness there would be its own kind of strawman.
6. Any axis ratatui wins → a numbered entry in `docs/design/dogfood-findings.md`.
7. Workspace gates: both crates' tests/clippy/fmt green; showcase crate builds with the
   registry qwertty dep once Wave D1 lands.

Good, beyond done: a reader who knows neither framework can open a rabbitui version and
answer the comprehension questions unaided; the modification diffs are small enough to
paste in announcement material; the streamer pair's write-up is quotable on the
inline-mode difference without editorializing.

## Sequencing

After Wave A (the trait is the model being showcased). Picker + form first (biggest
gap, smallest builds); streamer next (the differentiator pair); dashboard's table
variant waits on B2. Each pair is an independent lane — parallelizable across agents,
one pair per agent, with the honesty-review pass serialized through one reviewer.
