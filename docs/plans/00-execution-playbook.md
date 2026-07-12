# Execution playbook (read first, every session)

Written 2026-07-07 by the Fable-driven session that built Arcs 1–2B, for the sessions that execute
the remaining arcs. The per-arc plans (`arc2a`, `arc3`, `arc4`, `arc5` in this directory) carry the
design decisions; this file carries the working method. **Decisions recorded in these plans were
adjudicated with the author present — execute them; don't re-litigate them.** If a plan turns out to
be wrong on contact with reality, write the correction into the plan file (dated, with the evidence)
and proceed — same supersession discipline as the ADRs.

## Session bootstrap

1. `cd /Users/joshka/local/rabbitui/work/default` — **the shell cwd resets to the repo root every
   turn boundary; re-cd before every jj/cargo command.** The repo root's `.jj/working_copy` is
   renamed `working_copy.stale-bak` on purpose (it fought this workspace for the same record and
   caused two data-loss incidents). Never run `jj workspace update-stale` without confirming which
   directory owns the workspace.
2. Read `work/prompt.md` (the brief), `ROADMAP.md` (the tracker), and the plan file for the arc in
   flight. Skim `work/qwertty/substrate-status.md` for drift — the qwertty session edits it.
3. `cargo check --workspace --all-targets` to confirm a clean baseline before any edit.

## Verification gates (non-negotiable, author-mandated)

- `cargo check --workspace --all-targets` after **any** change — it is fast and keeps all nine
  examples compiling. A stalled agent once left two failing tests that `check` missed: run
  `cargo test --workspace` at every stopping point, not just `check`.
- `cargo clippy --workspace --all-targets` and `cargo doc --no-deps` clean before committing a
  slice.
- Golden snapshots: `UPDATE_SNAPSHOTS=1 cargo test` regenerates; review the diff before accepting.
- Betamax tapes (`just tapes`, `just tape name=<x>`) are the visual acceptance layer. Tape `Wait`
  sentinels match help-line text — when you change an example's help line, update its tape. Betamax
  quirk: `Wait+Screen "Tab"` parses Tab as a key press; pick sentinel strings that aren't key names.
- Style invariants (each one was a shipped bug): **never DIM** (Muted = Ansi(8) alone); printable
  app bindings must check `Update::consumed()`; every erase/clear emission is preceded by SGR reset
  (BCE floods otherwise); focused borders use Accent fg-only, never the bg-carrying Highlight;
  selection styling never uses Muted.

## jj discipline

- One `jj commit <paths> -m "..."` per logical change, rationale in the description. Path-scoped
  commits keep the untracked coordination files (`work/prompt.md`, `work/qwertty/*`) out of danger —
  an orphan-snapshot abandon deleted them once.
- ADR changes are supersessions/amendments, never silent edits.
- After each qwertty coordination exchange, mirror `work/qwertty/*.md` into `docs/substrate/` and
  commit the mirror (the drop-box itself stays untracked).

## Delegation protocol (Workflow / Agent tool)

- Use Workflow for parallelizable work (author mandate); choose **opus or sonnet subagents** —
  Fable-priced subagents hit session limits. Sonnet for mechanical/scoped work, opus for
  design-sensitive work.
- Subagents **never run jj** (snapshot races corrupt the working copy) and **always write a durable
  summary file** to `target/<task>-report.md` before their final message — two agent reports were
  lost to connection drops.
- Give agents pre-made decisions, not open questions. A slice-6 agent stalled twice deliberating
  dependencies; the fix was a fresh agent with the decision handed to it ("use futures-core,
  hand-roll the IntervalStream, note it, move on"). Every delegation brief should include: the
  decision list, the acceptance criteria, the style invariants above, and "pick the simplest
  option, note it in the summary, and keep moving."
- Scope concurrent cargo runs: when another agent (or the qwertty session) is mid-edit, `cargo
check -p <crate>` the unaffected crates, or use the Monitor tool with an until-compiles loop.
  Never sleep-poll.

## qwertty coordination

qwertty (`~/local/qwertty`) is the assumed substrate — adopt, don't re-litigate. The seam is
`rabbitui/src/terminal.rs` (one file); input decode is **never forked** (interpreting preserved CSI
is fine). Substrate gaps get filed in `work/qwertty/substrate-requirements.md`, not worked around
silently — except where the requirements doc already records an accepted workaround (macOS
ttyname/open_path; interim SGR encoder). Open P0s upstream: lone-ESC timing (Esc keybindings are
dead app-wide — ctrl-chords are the interim pattern, ADR 0006 amendment), resize events, named C0s,
key-vocabulary ceiling. The Phase 3 adoption plan (FakeDevice → RestoreHandle → KeyEvent/TextPayload
pre-pin migration) is in `arc4-spine.md`; the KeyEvent migration **waits for qwertty's stability
flag** — check substrate-status.md before starting it.

## Documentation discipline

- Markdown: `markdownlint-cli2` clean (config at repo `.markdownlint-cli2.yaml`: 100 cols, tables
  and code exempt from wrap, aligned table separators). Prettier traps: mid-sentence `+` becomes a
  list bullet; line-leading `#123` becomes a heading (escape as `\#123`).
- Docs are deliverables here, same bar as code. Public-facing docs (field reports, gap analysis,
  inline-mode spec) never mention rabbitui or qwertty by name. Never read
  `docs/research/tui-framework-field-report-2026.md` (the author's GPT5.5 report — context poison).
- Each slice ends with an honest "what this revealed" design note in `docs/design/`.
- **Progressive disclosure** (author-mandated 2026-07-11): write documents in _explaining
  order_, not discovery order — a clear overview up top (what it is, the priorities), then
  sections that fill in depth, with detail pushed to appendices or plan files. A doc that
  grew by accretion gets restructured, not appended to.

## Multi-session parallel execution (added 2026-07-11)

For running several Claude threads concurrently on the wave plans. Two safe modes; never
mix them on the same files.

**Mode 1 — one session, parallel subagents (the proven default).** The coordinator
session owns `work/default` and the jj working copy; subagents edit **disjoint file
sets** (assigned from the wave's file list); only the coordinator runs jj commands and
commits path-scoped. This is how Arcs 1–3 were built, including crash recovery. Use it
whenever the lanes fit one session's attention.

**Mode 2 — one session per wave, one jj workspace per session.** For genuinely
concurrent threads: each session runs `jj workspace add ../wave-<x>` (sibling of
`work/default`) and works only there. Rules:

- **Never** run `jj workspace update-stale` without confirming which directory owns the
  record (the two data-loss incidents were workspace contention; the repo root's renamed
  `working_copy.stale-bak` is deliberate).
- A session claims its lane by writing its workspace name + wave + date into the wave
  plan's header before starting; the lane matrix in `core-model-and-roadmap.md` (and the
  file lists in each plan) define what it may touch. Anything outside the list needs the
  coordinator.
- **Landing is serialized through the coordinator**: the wave session leaves its work as
  committed changes in its workspace; the coordinator rebases lanes onto trunk in
  dependency order (A before B1/C/D2-D3; B2 parts 1–2 anytime; B2 part 3 after A),
  reruns the full gates after each rebase, and only then advances trunk. No wave session
  ever rebases another's work.
- Merge conflicts are expected only where the lane matrix predicted overlap; a conflict
  outside the matrix means a session left its lane — investigate before resolving.

**Integration gates at every landing** (Mode 1 or 2): full workspace + standalone-crate
suites, clippy, nightly fmt, e2e 5×, markdownlint on touched docs; betamax visual pass
flagged for the coordinator when anything visible changed. CI is the backstop, not the
integration mechanism — the coordinator's rebase-and-regate is.

## Definition of done (global; wave plans add specifics)

"Done" is not "compiles and the listed tests pass." Every wave item meets ALL of:

1. **Gates**: `cargo test --workspace` (+ standalone crates), clippy zero warnings,
   `cargo +nightly fmt --all --check`, markdownlint on touched docs. Non-negotiable.
2. **Proven at the right layer**: framework behavior gets a harness-level test (TestApp /
   VtScreen / FakeDevice pump), not only unit tests — this repo's bug history (help-panic,
   tool-freeze) is the reason. Anything visual/interactive is flagged for coordinator
   betamax; say so explicitly in the handoff rather than claiming it verified.
3. **Consumed once**: new API is not done until one real consumer uses it (an example,
   the dogfood app, or the flagship) and any friction found is written into
   `docs/design/dogfood-findings.md` as a numbered entry.
4. **Documented where users look**: rustdoc on every public item (with an example for
   entry points), and the relevant design/plan doc updated with dated corrections —
   never silently diverging from the plan.
5. **Reads well**: the code a user would imitate (examples, doc examples) follows the
   §2 canonical shapes, wins on succinctness WITHOUT losing obviousness — if a fresh
   reader can't answer "where does focus go on Tab?" from the example, it is not done.
6. **Looks good** (standing rule): if the change touches anything visible, the example
   showing it meets the Arc 2A bar.

"Good," beyond done, is each plan's "What good looks like" block — read it before
starting, self-review against it before committing.

## Author decision queue (do not decide these yourself)

In deadline order (updated 2026-07-11; briefing with tradeoffs delivered in-session):

1. **Before Wave A starts**: trait config shape — one `fn config() -> Config` struct (as
   specced) vs individual `fn mode()/theme()/…` methods. Small; specced as Config.
2. **Before spinning threads**: execution mode — serial / Mode 1 (one session,
   subagents) / Mode 2 (workspace per session), and how many lanes (token-budget aware).
3. **Before Wave D1**: qwertty dependency mode in CI — registry-only vs patched-HEAD vs
   both jobs.
4. **Before Wave G publishes numbers**: who performs the ratatui-version honesty review
   and eval grading (author vs strong-model + author spot-check).
5. **Before 0.1, cheaper sooner**: ADR 0014 name (`rabbitui` / `ratatui-framework` /
   `qwertty-tui`) + publish cadence rider (reservation-only vs rolling 0.0.x pre-releases).
   Flagship binary name (`rabbit` placeholder) rides with it.

Standing author-only chores:

- Deleting `work/stale-root-checkout/` and `.jj/working_copy.stale-bak` (author cleanup).
- Publishing cadence with qwertty (now decision 3 above; path dep → version dep is Wave D1).
