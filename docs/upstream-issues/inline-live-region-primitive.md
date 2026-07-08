# A live/committed region primitive for inline applications

> **DRAFT — pending author review. Not filed. See `README.md` in this directory.**

_Proposed title:_ A live/committed region primitive — let an application declare "this output is
finalized" so the terminal owns its scrollback, selection, and reflow.

## Summary

A large and growing class of applications — anything that renders live, updating output above the
shell prompt while committing finished output into scrollback — needs one primitive it cannot express
today: a way to tell the terminal "everything above this point is finalized; you own its scrollback,
selection, copy, and reflow, and I will never address it again," while a bounded region at the bottom
stays application-owned and repaintable. Today applications hand-roll this with scroll-region escapes
(DECSTBM) plus a hand-tracked high-water mark, and the line-count bookkeeping that requires is a
well-documented source of resize corruption. A small, typed region primitive would replace the
hand-rolling with a contract.

## What applications do today, and why it breaks

The hand-rolled pattern is: keep finalized content in native scrollback above an inline viewport
using scroll-region escapes, and track a high-water mark by counting lines. This inherits every
emulator's scroll-region and resize idiosyncrasies, and the line-count bookkeeping drifts out of sync
the moment the terminal re-wraps history at a new width — producing a resize corruption class where,
in one production write-up, "some lines were lost or overwritten, others were duplicated"
([openai/codex tui2 design docs, pinned tree](https://github.com/openai/codex/tree/41e38856f6c11679d75bd63f6eef1e0ea76dffeb/codex-rs/tui2/docs)).
The alternative — the application owning the whole viewport and reimplementing scrollback, selection,
and copy itself — was built to completion by the same team, won on reflow and copy fidelity, and was
then **retired**, because making it feel native "across the environment matrix … creates a
combinatorial explosion of edge cases." Users defended the native behaviour explicitly: "Terminal is
king because anything works anywhere. Don't break scrolling, copy/paste"
([openai/codex#8344](https://github.com/openai/codex/issues/8344)).

So applications are forced to choose between a fragile cooperative hack and a maintenance cliff, when
what they want is narrow: a boundary the terminal understands.

## Minimal reproduction (the corruption the hand-rolled pattern produces)

1. An application uses a scroll region plus a line-counted high-water mark to keep finalized lines in
   scrollback above a small live region.
2. It emits several finalized lines, some longer than the current window width (so the terminal
   soft-wraps them).
3. Resize the window narrower, then wider.

**Actual:** because the high-water mark was tracked by line count and the terminal re-wrapped the
committed content at the new width, the application's notion of where the boundary is no longer
matches the terminal's, and committed lines are duplicated or overwritten on the next repaint.

**Expected (with a primitive):** the application would have declared the finalized content as a
committed region; the terminal owns its reflow entirely, and the application neither replays nor
re-addresses it, so a resize cannot desynchronize a boundary the application is no longer responsible
for tracking.

## What exists, and the specific gap

[OSC 133 semantic prompts](https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md)
(FinalTerm/FTCS) are widely adopted and mark _boundaries_ — points in the stream (prompt / input /
output) — but not _regions with a lifetime_. There is no way for an application to open a region,
have it be application-owned and mutable, then **commit** it so it becomes immutable and fully
terminal-owned. The vocabulary is also shell-shaped, so non-shell applications must masquerade as
shells to get any region behaviour at all. Independent reinventions of most of the missing shape
already exist — WezTerm's `SemanticZone`, Ghostty's in-progress region rewrite
([ghostty#5932](https://github.com/ghostty-org/ghostty/issues/5932)), DomTerm's foldable command
subtrees — which suggests the primitive is close to what maintainers are already building.

## Proposed direction

A minimal typed-region extension: `BEGIN id type=<output|live|…>` … `END id`, with two states — a
region is _open_ (may receive writes, application-owned) then _committed_ (immutable; the terminal
fully owns scroll, selection, copy, and reflow-from-source). At most one `type=live` region at the
tail; committing it is the protocol form of the high-water-mark handoff every inline application
hand-rolls. This is deliberately **not** a widget toolkit — just identity and a live/committed
lifetime.

## Degradation and multiplexers

- **Degradation:** unknown region markers are ignored and content prints as ordinary scrollback, so
  the primitive is pure progressive enhancement — exactly the property that made OSC 8 hyperlinks
  deployable. `type=output` maps onto existing OSC 133 output semantics for terminals that already
  have them.
- **Multiplexers:** specify pane-scoping from day one — the multiplexer parses BEGIN/END, associates
  the region with the pane, and re-emits it into the outer terminal's namespace, clipped to pane
  bounds. This must be normative middle-box behaviour, not passthrough, because a region that is
  consumed rather than forwarded (the way OSC 133 state is consumed by some multiplexers today) does
  not survive the trip.
