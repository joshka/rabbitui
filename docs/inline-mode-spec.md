# Inline Mode: A Specification for the Two-Region Terminal Application Surface

**Status:** specification draft

**Audience:** TUI framework authors and renderer implementers. This document extracts a single
rendering discipline — inline mode — into a contract any framework can implement from, independent
of language or widget model. It is the constructive companion to
[_The Terminal as a 2D Application Surface: What's Missing_](./terminal-gap-analysis.md), which
argues for the terminal-side protocol extensions that would let this discipline do more; this
document specifies what the discipline can do _today_, on unmodified terminals, and states precisely
where it stops.

Every claim of behavior is grounded in a working reference implementation and in public evidence
from the coding-agent CLI wave that made inline mode the sharpest unmet demand of the era. Where the
reference implementation and the ideal contract diverge, the divergence is named as a limitation,
not hidden.

---

## 1. What this specifies, and why

A terminal application renders into one of two screen regions. The **alternate screen** is a
dedicated full-screen buffer the terminal discards on exit — the classic full-app model. **Inline
mode** renders into the normal (primary) screen alongside the shell prompt, so finalized output
flows into the terminal's own scrollback. The choice decides who owns scrollback, text selection,
and copy: the terminal, or the application.

Inline mode's value is that it keeps the terminal in charge of the things users defend most. When a
production coding-agent CLI moved from inline to the alternate screen, users called it a regression;
another filed an issue titled "Don't mess with the native TUI," stating the constituency's position
flatly: "Terminal is king because anything works anywhere. Don't break scrolling, copy/paste, for
crying out loud" ([openai/codex #8344](https://github.com/openai/codex/issues/8344)).

The naive way to render live, updating content on the primary screen flickers structurally. If a
renderer erases its previous frame by line count and rewrites it, content taller than the viewport
cannot be erased — anything that has scrolled off screen is duplicated into scrollback on the next
repaint. This is
[Ink #359](https://github.com/vadimdemedes/ink/issues/359), open since 2020: the framework cannot
fix it without the application restructuring its output into a committed history plus a small live
region. Inline mode is the name for that restructuring made into an enforced invariant.

The discipline has two regions and a hard boundary between them.

---

## 2. The two-region model

An inline application's output is partitioned into exactly two regions at every instant:

- A **committed channel**: an append-only sequence of finalized lines written _once_ into the
  terminal's own scrollback, above the live region, and never addressed again.
- A **live tail**: a bounded, repaintable region at the bottom of the primary screen, below all
  committed content and above the shell prompt, holding everything still mutating (a streaming
  message, a spinner, a text composer, a status line).

The boundary between them is a high-water mark. Content crosses it in one direction only: a line
moves from the live tail into the committed channel when the application decides it is final, and it
never comes back. This mirrors the append-only history region a React-for-terminals framework
exposes as `<Static>`, generalized from a component into a renderer invariant
([Ink `<Static>`](https://github.com/vadimdemedes/ink)).

The reference implementation for the retired app-owned model established the correctness property
the high-water mark must guarantee: "each logical cell appears exactly once" across resizes,
suspends, and mode transitions
([codex tui2 design docs](https://github.com/openai/codex/tree/41e38856f6c11679d75bd63f6eef1e0ea76dffeb/codex-rs/tui2/docs)).
Address the boundary by a **stable content identity** — a count of logical items already committed —
never by a running line count, because a line count drifts out of sync the moment the terminal
re-wraps history at a new width, and that drift was the specific bug generator in the earlier
cooperative-scrollback designs.

---

## 3. The commit contract

A committed line is finalized content handed to the terminal permanently. The contract is:

1. **Emitted unwrapped.** One logical line is emitted as one logical line — never pre-wrapped to the
   current viewport width. The terminal soft-wraps it. Because the application never chose the wrap
   points, the terminal owns re-wrapping the line when the window resizes. This is the single most
   important clause: pre-wrapping committed content at the width of the moment is what made the
   earlier cooperative designs corrupt history on resize ("some lines were lost or overwritten,
   others were duplicated").

2. **Emitted styled.** A committed line is an ordered run of styled spans; each span's styling
   (color, weight, emphasis) is emitted before its text, followed by a style reset at end of line,
   so styling survives into scrollback. (The reference implementation shipped single-style commit
   lines first and generalized to multi-span; multi-span styled commits are the correct contract and
   a plain-text implementation is a degradation, not the target.)

3. **Terminated.** Each committed line ends with a carriage return plus line feed, so the terminal
   advances to the next line and the committed content scrolls naturally into native history. No
   trailing bare line feed is emitted at the tail's own bottom, so the live region never
   self-scrolls.

4. **Never re-addressed.** Once emitted, a committed line is immutable. The renderer issues no cursor
   move that targets it, no erase that reaches it, no repaint. The terminal owns its reflow,
   selection, and copy from that point forward. This is the whole point: native scrollback, native
   Find, native copy/paste all "just work" because the bytes are ordinary scrollback and nothing
   ever touches them again.

**Committing is an event-time action, not a render-time one.** A view function re-runs every frame;
committing there would emit the same line on every repaint. The contract requires that a line is
committed exactly once, at the moment the application decides it is final — which by construction
happens once.

---

## 4. The live-tail contract

The live tail is the only region the application repaints. Its contract:

1. **Bounded height.** The tail's height never exceeds the viewport height. The ideal bound is
   `min(content_height, max_height, viewport_height)` — the intrinsic height of the declared content,
   an application-chosen cap, and the terminal's row count, whichever is smallest. Overflow is not
   allowed to grow the region past the viewport; content that would overflow is either committed
   above (crossing into the append-only channel) or scrolled inside a bounded widget. This bound is
   what makes Ink #359 structurally impossible: a region that never exceeds the viewport can always
   be fully erased and repainted. (An implementation without an intrinsic-height measurement may
   honor `content_height` by having the application declare a correctly sized frame; the bound still
   holds.)

2. **Repaint discipline.** The tail is repainted in place, inside a synchronized-output frame (see
   below). A repaint anchors the cursor at the region's top, erases the old region, flushes any
   pending commits, then paints the new tail. Between frames with a stable height and no commits, the
   renderer may diff the tail cell-by-cell and repaint only changed cells; **any height change or any
   commit forces a full tail repaint.** Diffing across a commit or a height change would require
   tracking how far content scrolled — exactly the line-count bookkeeping that generated the historic
   bugs — so the discipline forbids it and pays a full repaint instead.

3. **Relative addressing.** The live region floats: it has no fixed screen rows, because committed
   content above it grows and the shell may scroll the whole primary screen. The renderer therefore
   addresses the region **relative to itself**, never by absolute row. It reaches the region top by
   moving the cursor up `H - 1` rows from a known bottom anchor and returning to column one; it moves
   between the region's own rows by relative cursor-down and cursor-up. The renderer's frame
   invariant is that it always leaves the cursor at column one of the region's bottom row, so the
   next frame's upward arithmetic is exact.

4. **ED-based shrink.** When the tail shrinks, the vacated rows below must be cleared or they leave
   orphaned content on screen. The discipline clears from the region top downward with an
   erase-to-end-of-display before repainting, so a shorter tail leaves no stray rows. This
   erase-below reaches the old tail and everything under it but, crucially, never reaches committed
   scrollback above the anchor.

5. **Synchronized-output framing.** Every frame — the erase, the commit flush, and the tail paint —
   is wrapped in a synchronized-output block (DEC private mode 2026, begin/end). The terminal buffers
   the whole update and presents it atomically, so a resize or a multi-step repaint never shows a
   half-drawn frame. This is one of the recent terminal extensions that deployed cleanly and degrades
   to a no-op on terminals that do not implement it, so emitting the framing unconditionally is safe.

Note the deliberate omission: this contract uses erase-plus-repaint, not scroll-region (DECSTBM)
manipulation. Scroll regions are a valid _optimization_ an implementation may add later without
changing the contract, but they are the primary source of per-emulator divergence in the cooperative
designs, so the correct-first baseline avoids them.

---

## 5. Resize behavior

Resize splits cleanly along the region boundary, which is the payoff of the two-region model:

- **Committed content is the terminal's problem.** Because committed lines were emitted unwrapped
  (§3.1), the terminal re-wraps them to the new width using its own reflow. The application does
  nothing, replays nothing, and keeps no per-terminal table of scrollback limits. This is the entire
  reason the commit contract insists on unwrapped emission.

- **The live tail re-layouts and fully repaints.** On resize the application re-lays-out the tail to
  the new width and forces a full repaint; the tracked height is clamped to the new viewport.

- **One known artifact.** Repainting a floating region across a width change can leave a single stray
  line in some emulators, because the region's _old_ cells were wrapped at the previous width and the
  emulator's own rewrap of those cells does not perfectly align with the fresh repaint. A
  conservative erase-below on resize bounds the artifact to at most one line. This is an inherent
  limit of repainting a floating region on an unmodified terminal, not a defect to be fixed
  application-side; it is the honest edge the model accepts in exchange for never owning committed
  reflow.

---

## 6. Mode-switch ordering

An application may switch between inline and alternate-screen modes at runtime (an overlay, a pager,
a full-screen approval dialog). The ordering constraint is:

**Pending commits flush into scrollback _before_ the alternate screen is entered.** The alternate
screen hides the primary screen and its scrollback. If a line is committed in the same update that
requests the switch, and the switch is applied first, the committed content lands behind the
alternate screen and is lost to the user. The runtime therefore drains buffered commits into the
primary screen, then emits the alternate-screen enter sequence — never the reverse.

Because the alternate screen by definition hides primary scrollback, this guarantee is observable as
**byte-emission order**: the commit bytes precede the alternate-screen-enter sequence in the output
stream. It cannot be observed by inspecting scrollback after entry, since the alternate screen has
hidden it — that is what the alternate screen _is_, not a gap in this contract.

Entering inline mode itself performs no buffer switch: it hides the cursor and resets the region
tracking, then the first frame anchors at the current cursor line and grows the region downward.
Leaving inline mode drops the cursor below the tail onto a fresh line and shows it, so the shell
prompt returns beneath the final frame; the tail stays on screen as ordinary primary-screen content,
which is exactly the terminal-native behavior inline mode exists to preserve.

---

## 7. Input caveats

Inline mode's guarantee is that the terminal keeps ownership of scrollback, selection, and copy. That
guarantee is conditional on one input decision.

To learn about scrolling — whether the user is at the tail, and to be notified when they scroll — an
application must today enable mouse capture, and enabling mouse capture makes the terminal **stop
scrolling natively** and hand the application raw wheel events it must integrate itself. This is a
mutual exclusion, documented in the gap analysis's scroll section
([§4](./terminal-gap-analysis.md)): scroll _awareness_ and native scroll _behavior_ cannot both be
had on an unmodified terminal. An inline application that captures the mouse has, at that moment,
opted out of the very native-scroll behavior that is inline mode's reason to exist.

The discipline's default is therefore to **not capture the mouse**, preserving native scroll,
selection, and copy, and to accept that the application does not know the scroll offset. Applications
that genuinely need scroll awareness must weigh it against the native behavior they surrender, and
the durable fix is terminal-side — scroll-state events decoupled from mouse capture, as sketched in
the gap analysis. Until then, this is a real tradeoff, not a solved problem, and an inline framework
should expose it as a choice with its cost stated.

---

## 8. What this model deliberately cannot do

Terminal-native inline mode caps fidelity, on purpose, and the cap is the honest cost of not owning
the viewport:

- **It cannot collapse, edit, or re-flow committed content.** Once a line is committed it is dead
  scrollback text with no application-addressable identity. There is no per-cell copy, no
  expand/collapse of a committed tool-call log, no re-styling, no reflow beyond what the terminal's
  own wrap does. Append-only is a hard invariant; reflowing already-committed scrollback runs
  straight back into the per-terminal clear-and-scroll-region swamp that every cooperative design
  fled.

- **It cannot offer faithful source copy across a width change.** Because the terminal owns
  selection, a copy yields the painted grid, not the logical source. Copying a rendered diff yields
  the decorated cells, not the patch text; copying a wrapped logical line may yield the visual rows,
  not the joined line.

An application whose workload genuinely requires collapsible committed cells or source-fidelity copy
across reflow cannot get them from this model; it must own the viewport, and inherit the combinatorial
compatibility cost that a production team priced and then retired the app-owned approach over
([codex tui2 retirement rationale](https://github.com/openai/codex/tree/41e38856f6c11679d75bd63f6eef1e0ea76dffeb/codex-rs/tui2/docs)).

The terminal-side fix is not "own the viewport." It is typed, identified regions with a
live/committed lifetime, so the terminal can select, copy, collapse, and reflow a committed region
from application-declared boundaries — the highest-leverage proposal in the gap analysis
([§2, regions](./terminal-gap-analysis.md)). Under that extension, the commit contract in §3 becomes
the protocol form of committing a `type=live` region, and the limitations in this section dissolve
without the application reimplementing scroll, selection, and copy. This specification is what an
implementer builds while that extension does not yet exist.

---

## 9. Conformance checklist

An implementation conforms if it exhibits the following observable byte-and-screen behaviors. Each is
testable by feeding the renderer's output to a terminal emulator and asserting on the resulting
screen (or, where noted, on the byte stream), which catches framing, clears, and cursor discipline
that buffer-equality testing cannot.

1. **Commit-then-tail ordering.** After a commit plus a tail paint, the emulated screen shows the
   committed line above the live tail, in that vertical order.
2. **Multi-commit ordering.** Two lines committed in one update appear in scrollback in the order
   they were committed.
3. **Unwrapped commit emission.** A committed line longer than the viewport width is emitted as one
   logical line (the terminal, not the renderer, introduces the wrap); no cursor addressing targets
   the wrapped line afterward.
4. **Committed styling reaches the terminal.** A styled committed line emits its style sequence before
   its text and a reset after; the emulated scrollback shows the styling.
5. **Bounded tail.** The live region's height never exceeds the viewport height across any frame.
6. **Tail shrink leaves no orphan rows.** When the tail shrinks, an erase-to-end-of-display precedes
   the repaint and the emulated screen shows no stray rows below the new, shorter tail.
7. **Stable-tail cell diff.** A frame with unchanged height and no commits that changes only some
   cells repaints only those cells (no erase-below, unchanged rows untouched).
8. **No-op frame is silent.** A frame with unchanged height, no commits, and no cell changes emits no
   bytes.
9. **Relative addressing / bottom-row invariant.** The renderer uses only relative cursor moves for
   the floating region and leaves the cursor at column one of the tail's bottom row every frame.
10. **Synchronized framing.** Every frame's bytes are wrapped in a synchronized-output begin/end pair.
11. **Resize repaint at new width.** After a width change, the tail fully repaints laid out to the new
    width (committed content is left to the terminal's reflow).
12. **Commits flush before alternate-screen entry.** When a commit and a switch to the alternate
    screen occur in the same update, the commit bytes precede the alternate-screen-enter sequence in
    the output stream.

---

## 10. Conformance corpus

The checklist in §9 is prose; a conforming implementation is verified against a **named, stable test
corpus** so that "conforms" means the same thing to two independent implementers and to a
regression suite over time. Each corpus entry has a stable identifier that never changes once
published, a fixed input scenario, and an assertion made either on the emulated screen (feed the
renderer's byte output to a headless terminal emulator and assert on the resulting grid) or directly
on the byte stream (for ordering and framing properties the screen cannot reveal). Identifiers are
the contract; the harness that executes them is a separate deliverable and is out of scope for this
document.

> **Status of the corpus.** The corpus itself — a runner, fixture scenarios, and a per-terminal
> results matrix — is **planned work, not yet built**. This section fixes the identifier namespace
> and the assertion each identifier stands for, so the spec, the future harness, and any
> conformance claim reference the same stable names. Treat an identifier below as a promise about
> _what_ is tested, not evidence that it is tested today.

### 10.1 Corpus identifiers

Each identifier maps to one or more checklist clauses in §9. The mapping is normative: an
implementation "passes `<ID>`" iff it exhibits the referenced §9 behavior under the identifier's
scenario.

| Corpus ID                | Scenario                                                                                         | Asserts on   | §9 clause(s) |
| ------------------------ | ------------------------------------------------------------------------------------------------ | ------------ | ------------ |
| `INLINE-APPEND-ONCE`     | A line is committed, then the tail is repainted N times without re-committing.                   | screen+bytes | 1, 2         |
| `INLINE-COMMIT-ORDER`    | Two lines committed in a single update.                                                          | screen       | 2            |
| `INLINE-WRAP-ON-RESIZE`  | A committed line wider than the viewport; the emulator width is then reduced and increased.      | screen+bytes | 3, 11        |
| `INLINE-COMMIT-STYLED`   | A committed line carrying multiple styled spans.                                                 | screen+bytes | 4            |
| `INLINE-TAIL-BOUNDED`    | Declared tail content taller than the viewport across several frames.                            | screen       | 5            |
| `INLINE-TAIL-SHRINK`     | A tail that shrinks in height between two frames.                                                | screen+bytes | 6            |
| `INLINE-TAIL-CELL-DIFF`  | A stable-height, no-commit frame that changes only some cells.                                   | bytes        | 7            |
| `INLINE-NOOP-SILENT`     | A frame with unchanged height, no commits, and no cell changes.                                  | bytes        | 8            |
| `INLINE-RELATIVE-CURSOR` | Any multi-frame sequence; assert the floating-region addressing and bottom-row cursor invariant. | bytes        | 9            |
| `MODE2026-FRAMING`       | Any frame; assert the whole update is bracketed by a synchronized-output begin/end pair.         | bytes        | 10           |
| `INLINE-ALTSCREEN-FLUSH` | A commit and an alternate-screen switch requested in the same update.                            | bytes        | 12           |

Two further identifiers name properties that this spec depends on but that live at the byte/erase
layer rather than in the §9 region-behavior list. They are called out separately because they are
the two failure modes most likely to pass a screen-equality test yet corrupt a real terminal, and
because the companion gap analysis references them as evidence:

| Corpus ID                    | Scenario                                                                                                   | Asserts on   | Spec basis |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------- | ------------ | ---------- |
| `BCE-RESET`                  | A styled tail (non-default background) is repainted and then shrinks, forcing an erase.                    | screen+bytes | §4.4       |
| `WIDE-GRAPHEME-CONTINUATION` | Committed and tail content containing a wide grapheme (ZWJ emoji, CJK, combining sequence) at a wrap edge. | screen       | §3.1, §4.1 |

`BCE-RESET` asserts that **every erase or clear the renderer emits is immediately preceded by an SGR
reset** (`CSI 0 m`). An erase inherits the terminal's current graphic rendition, so an erase issued
while a non-default background is active floods the vacated cells with that background color
(background-color-erase, "BCE bleed") — the erase-below of §4.4 would paint a colored band beneath a
shrinking tail. The assertion is on the byte stream (reset precedes erase) and confirmable on screen
(no colored orphan rows). This is the erase-side companion to the §4.4 ED-based shrink clause.

`WIDE-GRAPHEME-CONTINUATION` asserts that a wide grapheme is treated as a single indivisible unit of
its measured width in both the committed channel and the tail: it is never split across a soft-wrap
boundary such that its trailing cell is separated from its leading cell, and the renderer's own
width accounting agrees with the emulator's cursor advance (the width-agreement invariant the gap
analysis calls the contract every frame rests on). A disagreement here desynchronizes every
subsequent cell on the row, so this identifier guards the measurement assumption underneath both the
unwrapped-commit clause (§3.1) and the bounded-tail layout (§4.1).

### 10.2 Using the corpus in a conformance claim

An implementation states conformance as a pass/fail vector over the identifiers above, per terminal
emulator tested — because several identifiers (`INLINE-WRAP-ON-RESIZE`, `BCE-RESET`,
`WIDE-GRAPHEME-CONTINUATION`, `MODE2026-FRAMING`) can pass on one emulator and fail on another, and
the honest unit of a conformance claim is therefore _(identifier × emulator)_, not a single boolean.
The one artifact from §5 that is a known, accepted edge — the single stray line a floating region
can leave on some emulators across a width change — is expected to surface as an emulator-specific
soft failure of `INLINE-WRAP-ON-RESIZE`, and is documented as an inherent limit (§5), not a defect
the implementation is required to eliminate.

---

## 11. Why this discipline

The evidence base is a decade of the same lesson arriving from every direction. Ink discovered the
two-region shape as `<Static>` and documented, in #359, that the naive alternative is unfixable
without it. A production coding-agent CLI refined it with a cell-level high-water mark so width
changes never drop or duplicate history, then built the maximal alternative — an app-owned viewport
that reimplemented scroll physics, selection geometry, and source-fidelity copy — won on reflow and
copy, and **retired it**, because "making that experience feel fully native across the environment
matrix … creates a combinatorial explosion of edge cases." Its users had already told it why, in
[#8344](https://github.com/openai/codex/issues/8344).

The two-region model is the settlement between those poles: the terminal keeps everything it is good
at, the application keeps only the small mutating tail, and the boundary between them is a hard
invariant rather than a convention an application can accidentally violate. It is not the most
capable model — §8 is honest about its ceiling — but it is the one that keeps the property that makes
the terminal valuable: any application, any host, any multiplexer, one interface. That is worth
specifying as a named discipline, and worth implementing the same way everywhere.
