# Upstream terminal-gap issue drafts

**Status: DRAFTS — pending author review. Do not file.**

These are ready-to-file GitHub issue drafts for the terminal-gap shortlist
(`docs/terminal-gap-analysis.md` §10 top three; arc5-field.md §6): scroll-region decoupling,
synchronized-output edge cases, and inline/live-region primitives. Each is a minimal, constructive
issue — a title, a minimal reproduction, expected vs. actual, and a short note on degradation and
multiplexer behaviour. **No framework pitch** appears in any of them: they are written as a terminal
user reporting a protocol gap, not as advocacy for any particular application or framework.

They are outward-facing artifacts under the author's name, so:

- **The author reviews the text before anything is filed.** Wording, scope, and tone are the author's
  call.
- **Target the right project, and check responsiveness first** (arc5-field.md §6: file "where
  maintainers engage"). Each draft is written to be retargetable — it names the mechanism, not a
  specific tracker. Pick the project (a terminal emulator, a multiplexer, or the freedesktop
  terminal-wg specifications repo) whose recent issue activity shows maintainers engaging on this
  class of change, and adjust the framing to that project's conventions.
- **File one at a time, not as a batch**, so each gets a focused discussion.
- The naming rule holds: no internal project names appear (these are public).

Track filed links, once filed, in the `docs/terminal-gap-analysis.md` appendix (arc5-field.md §6).

| Draft                                                                    | Gap-analysis section | One-line summary                                                                                 |
| ------------------------------------------------------------------------ | -------------------- | ------------------------------------------------------------------------------------------------ |
| [`scroll-region-decoupling.md`](scroll-region-decoupling.md)             | §4                   | Learning the scroll position requires mouse capture, which kills native scroll.                  |
| [`synchronized-output-edge-cases.md`](synchronized-output-edge-cases.md) | §9, §11              | Mode 2026 is undetectable/unforwarded under a multiplexer, so apps flicker.                      |
| [`inline-live-region-primitive.md`](inline-live-region-primitive.md)     | §2, §10              | No primitive for a committed/live region boundary; apps hand-roll it with scroll-region escapes. |
