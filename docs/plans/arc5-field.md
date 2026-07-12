# Arc 5 plan — field leadership

Parallel-friendly documentation/harness/advocacy work for the gaps between build slices. Everything
here is public-facing: the naming rule holds (no rabbitui/qwertty names in documents intended for
external audiences until the author says otherwise; the conformance harness and comparisons are
inherently named — flag those to the author before publishing anything).

## 1. Public conformance harness

**Position:** grow it out of what exists rather than designing fresh — layer 1 is the vt100
harness (escape-level assertions, headless, CI-friendly), layer 2 is betamax tapes against real
terminal emulators (visual, human-reviewed). The deliverable is a standalone repo-shaped directory
(`conformance/` here first) with: a test corpus (BCE behavior, mode 2026 framing, wide-grapheme
continuation, wrap-on-resize, inline append-once), a runner that executes the corpus against a
terminal via PTY, and a results matrix (terminal × test) published as markdown. Both field reports:
"whoever publishes the harness sets the bar" — the corpus doubles as the terminal-gap-analysis
evidence base. **Acceptance:** matrix generated for ≥3 terminals the author has locally (Ghostty,
kitty, Terminal.app are the likely set — confirm with author); gap-analysis doc updated to cite it.

## 2. Inline-mode spec (mostly done)

`docs/inline-mode-spec.md` exists and is public-clean. Remaining: a conformance section
cross-referencing item 1's corpus IDs, and a final author review before any external posting.
Do not publish anywhere yourself — deliver the ready-to-post artifact.

## 3. Honest comparisons

**Position:** one small-but-real app implemented four times — ratatui, Bubble Tea, Textual,
rabbitui. **App choice (decided):** a streaming log-follower with a filter input and a detail
modal — it forces the inline/scrollback question (where the frameworks differ most), an input
widget, focus, and an overlay, while staying under ~300 lines each. Write-up structure per
framework: lines of code, the three hardest parts, what the framework made easy, what it made
impossible; end with an honest "when you should not pick rabbitui" section (credibility is the
point — both field reports call vendor comparisons that lack this useless). Implementations live
under `comparisons/`. **Acceptance:** four runnable implementations + the write-up; author reviews
before it's treated as publishable.

## 4. Agent-legibility: a rabbitui skill + evals

**Position (ratatui-kit precedent):** ship `skills/rabbitui/SKILL.md` in-repo: the mental model
(declared frame, identity, outcomes, commands), the invariant list (consumed-guard, no DIM, SGR
reset before erase, Accent-vs-Highlight), idiomatic snippets for the ten commonest tasks, and the
trap list from our own build logs (the bugs agents actually shipped: unguarded printables, missing
initial focus, BCE). Evals: five task prompts (add a widget to an example, add a binding, theme a
panel, write a snapshot test, add a Command) run against a fresh agent with and without the skill;
record pass/fail in the skill's design note. **Acceptance:** skill file + eval results committed;
eval failures feed doc fixes, not just skill fixes.

## 5. Concept book

**Gated:** start only after Arc 3 slice 6 lands (API churn before that would rot it). Structure
mirrors what worked in the crate docs: Concepts (frame/identity/facts/routing/outcomes/commands),
Guides (inline vs alt, theming, testing, tapes), Recipes (the eval tasks from item 4 make good
recipe seeds). Tooling: mdBook, `book/` directory, CI build job (no publishing until the naming
decision). Rustdoc stays the API reference; the book never duplicates signatures.

## 6. Terminal-gap advocacy

**Position:** file the gap-analysis shortlist as upstream issues **where maintainers engage**
(check each project's recent issue responsiveness first): scroll-region decoupling, synchronized
output edge cases, and the inline/live-region primitives. Each issue: minimal repro from the
conformance corpus, no framework pitch. Track filed links in `docs/terminal-gap-analysis.md`
appendix. Author reviews issue text before filing (these are outward-facing, under his name-space).

## 7. CI growth

In order of value: (a) msrv job (stable minus one, per the brief — resolve the concrete version and
pin it in one place); (b) `cargo-semver-checks` job (advisory until 0.1, blocking after);
(c) release automation: tag-driven workflow running the full gate set + `cargo publish --dry-run`
for each crate in dependency order (blocked on qwertty being a version dep — keep dry-run until
then); (d) tape job once betamax + a published qwertty make it hermetic. **Acceptance:** each as
its own commit with the workflow green on a test branch before merging.
