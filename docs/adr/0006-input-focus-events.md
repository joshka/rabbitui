# ADR 0006: Capture/target/bubble event routing over frame facts; ID-keyed focus

- Status: accepted (2026-07-06)
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

rabbitui's core contract is the declared frame (ADR 0001): each frame renders widget specs,
rendering emits _frame facts_ (hit regions, focus order, cursor candidates, extents, visibility
requests), and input routes against the previous frame's facts. This ADR specifies the input side:
how key/mouse/paste events reach widgets, how focus is stored and traversed, and which terminal
input protocols the framework negotiates.

- **Identity is the one GUI problem that transfers to TUIs undiminished, and focus needs it.**
  Textual gets focus "nearly free" because a focused widget is a persistent object reference that
  "survives every repaint, which is precisely what immediate-mode frameworks have to fake with IDs
  and hashing" (textual.md). rabbitui has no retained object tree, so it must supply that stable
  reference — the `WidgetId` store (ADR 0001; brick.md's name system).
- **Two-phase dispatch is the settled shape.** vxfw dispatches capture root→target then bubble
  target→root (`App.zig:588-602`); Brick bubbles cursor/visibility/extent results up through
  containers (Internal.hs `Result`); Textual bubbles messages up the DOM with `stop()`/ `prevent()`.
  Nobody who built a real one regrets capture/bubble.
- **Hit-testing must be against what was actually shown.** vxfw hit-tests mouse events against the
  _last rendered_ surface tree (`App.zig:363`) — "what the user clicked is what was on screen, not
  what the next layout would produce" (libvaxis.md). Brick derives `clickable` from `reportExtent`
  (last-frame geometry) for the same reason.
- **The terminal input layer is a negotiation, not a constant.** Kitty keyboard, bracketed paste,
  mouse SGR, and focus events are opt-in protocols detected by query, not terminfo
  (terminal-substrate.md §1; libvaxis.md burst). Enabling them late is painful: Textual never fully
  enabled kitty progressive enhancement, so shift+enter/shift+space stayed indistinguishable for
  years and "retrofitting is painful and stalls" (#6074).
- **Scroll input is a per-terminal subsystem, not an event type.** tui2's scroll study
  (codex-tui2.md; `scroll_input_model.md`: 16 logs, 13,734 events, 8 terminals) found terminals emit
  1–9+ raw events per physical notch and _timing cannot distinguish wheel from trackpad_.
  Normalization needs per-terminal events-per-tick factors plus a mandatory user override.
- **The substrate is incomplete, and its input model churns.** qwertty decodes text + C0 + arrows +
  preserved CSI only; kitty keyboard, mouse, paste, focus, and OSC input preservation are gaps, and
  `InputEvent` is declared high-churn (terminal-substrate.md §gap analysis). rabbitui must consume
  qwertty's decoded events, never fork input decoding.
- **IME/preedit has no substrate support anywhere.** No memo reports a terminal-side IME solution;
  qwertty has no preedit event. This is a substrate gap, not a framework decision.
- **The labs work already decided the creed.** ratatui-labs' interaction-model-survey built the same
  control strip four ways and concluded: _"render produces visible facts / input routes through
  previous visible facts / controls return outcomes / the app owns effects"_ (prior-art.md:52). Its
  roadmap flags pointer-capture and nested-routing (drag past target, release outside app, Escape
  policy) as the hard cases to prove before stabilizing the API.

## Options considered

### A. Capture/target/bubble routing over previous-frame facts, ID-keyed focus (chosen)

_What it is._ Each event routes against the last frame's facts. Key events target the focused
`WidgetId`; pointer events target the topmost hit region under the cursor. Dispatch runs capture
(root→target) then bubble (target→root); a handler gets `&mut EventCtx`, may `consume()` and push
effect requests (focus, scroll-into-view, redraw), and returns typed _outcomes_ to the app. Focus is
framework state keyed by `WidgetId`; traversal order derives from focus-order facts each frame;
focus is addressable by ID (`ctx.focus(id)`).

_Steelman._ The union of every design that shipped: vxfw's capture/bubble + last-frame hit-test

- command effects (libvaxis.md: "the runtime shape is right even where the identity model is
  wrong"), Brick's fact bubbling and `clickable`-from-extent, Textual's bubbling. Facts already
  exist for rendering, so routing reuses them at zero extra structure; ID-keyed focus gives
  Textual's stable-reference property without a retained tree — the ID store already holds scroll
  and cursor, so focus is one more per-ID field. One-frame-stale facts are immaterial at terminal
  event rates (ADR 0001; identical to hit-testing the last paint in every GUI).

_Why chosen._ The only option that composes with ADR 0001's facts without a parallel structure, and
the design ratatui-labs converged on after building the alternatives (prior-art.md:52).

### B. Retained-DOM focus + message pump (Textual's model)

_What it is._ Persistent widget nodes; focus is a node reference; Tab walks a computed `focus_chain`
(DOM order, siblings sorted by on-screen position); messages bubble up ancestry.

_Steelman._ Proven at app scale (harlequin, posting, toolong); the traversal model itself "draws few
complaints" (textual.md). `can_focus`/`can_focus_children`, `:focus`/`:focus-within`, modal
focus-trap, and selector-filtered `focus_next/previous` are a complete package worth stealing.

_Why not chosen._ It presupposes the retained object tree rabbitui rejected (ADR 0001); in Rust that
tree costs `Rc<RefCell>`/`Arc<Mutex>` per node and deferred-callback vocabulary (brick.md,
cursive.md). rabbitui takes the _behaviors_ — descend to first focusable child, reveal off-screen
target, focus-reason on the event — as contract requirements, over the ID store and facts, not a
live DOM. Textual's own focus bugs live at seams the tree did not prevent: `focus()` on a
non-focusable container silently no-ops (#2186); focusing inside an inactive tab leaves focus on an
invisible widget (#4593); click-to-focus fires Focus before the mouse event, so widgets cannot tell
_why_ they were focused (#4364). We adopt the shape and fix these in the contract.

### C. Widget identity by raw pointer or index (vxfw)

_What it is._ vxfw keys identity on `userdata`+`drawFn` pointers (`vxfw.zig:296-299`); focus is
`path_to_focused` rebuilt from the surface tree each frame.

_Steelman._ Zero bookkeeping for long-lived stable objects; the rebuild-focus-path-per-frame idea
maps cleanly onto deriving traversal order from facts each frame.

_Why not chosen._ Pointer identity is vxfw's worst wart and what Rust makes miserable: discussion

\# 232 shows ephemeral list-item widgets that dangle, forcing a hand-maintained stable-pointer list

— "the user hand-implements retained state the framework should own" (libvaxis.md). Ephemeral view
values are rabbitui's _default_; stable `WidgetId`s from user keys (Xilem id-paths, ADR 0001) are
the fix. We keep C's per-frame focus-path derivation, discard its identity model.

### D. Semantic-only input at the adapter edge (prior-art.md:80)

_What it is._ Route kitty/paste/mouse specifics in qwertty adapters, hand widgets only "domain
input" (activate, cancel, focus-next, pointer-press).

_Steelman._ Keeps widgets backend-neutral; separates focus scopes from traversal — a real, adopted
refinement.

_Why not chosen as the whole answer._ It is the outcome layer, not routing. Widgets still need raw
key/mouse/paste to implement text editing, drag, and custom bindings — a pure-semantic API cannot
express a text field. rabbitui does both: raw events route via A, handlers emit semantic outcomes
(Submitted, SelectionChanged…). D is folded in, not chosen instead.

## Decision

rabbitui routes input through the previous frame's facts using **capture → target → bubble**, and
stores **focus as framework state keyed by `WidgetId`**. Specifically:

1. **Routing.** Every event dispatches against the last rendered frame's facts. Key/text events
   target the focused `WidgetId`; pointer events target the topmost hit region containing the
   pointer (z-order from the facts hit map). Dispatch is two-phase (capture root→target, bubble
   target→root). A handler gets `&mut EventCtx`, may `consume()` to stop propagation, and pushes
   effect _requests_ (focus, scroll-into-view, redraw, terminal query) applied next frame — never
   mutating render state mid-dispatch (Brick's queued-request discipline, brick.md). Controls return
   typed **outcomes** to the app on the next update (ADR 0001).

2. **Focus storage and traversal.** The framework keeps `Option<WidgetId>` in the per-ID store.
   **Traversal order derives from frame facts** each frame (the emitted focus-order list, visual-
   position ordered), not from a retained tree or a tab-index attribute. `focus_next`/
   `focus_previous` walk that order; containers may declare focus-scope/trap semantics on their spec
   for modals.

3. **Focus-by-ID addressing.** Focus is addressable directly (`ctx.focus(id)`). Fixing Textual's
   seams as contract requirements: focusing a non-focusable container **descends to its first
   focusable descendant**; focusing a widget not visible in the current facts **raises a
   scroll-into-view/reveal request** (or fails loudly — never silently focusing an invisible
   widget); the **focus reason (keyboard vs pointer) rides on the Focus event**.

4. **Kitty keyboard negotiation.** Negotiated at startup via the batched, DA1-fenced, timeout-
   bounded probe (libvaxis burst; terminal-substrate.md §probe bundle #10), requesting at least
   flags 1|2 (disambiguate + event types). Flags are pushed on enter (and on alt-screen transitions
   — separate stacks) and popped on leave and suspend. When absent (e.g. tmux, which swallows
   `CSI ? u`), rabbitui degrades to legacy CSI/SS3 with decoded modifiers; the DA1 fence ensures the
   probe still terminates. Results land in the `Capabilities` struct (ADR 0012).

5. **Mouse hit-testing via facts.** Pointer events hit-test the facts hit map of the last rendered
   frame (vxfw `App.zig:363`; Brick `clickable`). Pointer capture (drag past target, release outside
   app) routes subsequent pointer events to the capturing `WidgetId` regardless of current hit
   region until release; Escape/cancel policy follows the labs pointer-capture proof
   (prior-art.md:53, roadmap step 5).

6. **Wheel/trackpad normalization.** Normalized through the tui2 scroll model (codex-tui2.md;
   `scroll_input_model.md`): group raw events into streams (80 ms gaps or direction flips),
   normalize by a per-terminal events-per-tick factor from a shipped probe-derived defaults table,
   classify wheel-like (fixed lines/tick, flush immediately) vs trackpad-like (fractional,
   cadence-gated, bounded acceleration), and expose a mandatory `scroll_mode` override because
   auto-classification is best-effort. Intermediate move/drag events are coalesced before `update`
   (brick.md #178 event flood).

7. **Paste.** Bracketed paste (mode 2004) is enabled at startup and **aggregated into one paste
   event** (not per-byte), delivered as a distinct event kind so widgets never interpret pasted
   content as key bindings (terminal-substrate.md §6, P0 #6).

8. **IME/preedit.** rabbitui does **not** ship IME/preedit in v0.1: no substrate offers it and
   qwertty has no preedit event. The facts contract reserves the anchor (widgets expose cursor
   candidates and extents), so a future preedit overlay has a place to attach. The gap is recorded
   in the qwertty requirements handover (terminal-substrate.md §requirements) and tracked upstream,
   not worked around in-framework.

9. **Substrate discipline.** rabbitui consumes qwertty's decoded `InputEvent`s and never forks input
   decoding (it is high-churn). Where qwertty has not landed a protocol (mouse, paste, kitty),
   rabbitui decodes on top of `Undecoded`/`Csi` events in its own layer, deleted module- by-module
   as qwertty lands each family. Widget crates stay runtime-free; only the runtime crate touches the
   loop.

## Consequences

**Positive.**

- Routing reuses the facts already produced for rendering — one source of truth for input and
  render, no parallel structure (ADR 0001).
- Third-party widgets get focus, hit-testing, scroll-into-view, and pointer capture for free by
  emitting facts (focusability, hit regions, extents, cursor candidates) — Brick's payoff ("mouse
  support falls out of it", brick.md).
- Focus-by-ID plus fact-derived traversal gives Textual's stable-focus property without a retained
  tree, and fixes Textual's focus seams (#2186/#4593/#4364) in the contract.
- Kitty/paste/mouse enabled from day one avoids Textual's stalled retrofit (#6074).
- Handlers pushing effect _requests_ (not mutations) keep dispatch deterministic and headlessly
  testable (libvaxis.md widget-test ergonomics; ADR 0009).

**Negative (honest).**

- One-frame-stale facts: a widget that appears and is clicked in the same frame is not hittable
  until the next frame. Immaterial at terminal event rates, but real (ADR 0001 accepts this).
- Fact-derived traversal is only correct after a render: pre-first-frame focus must target a known
  ID, not "next," and focusing a not-yet-rendered widget needs the reveal path (Decision 3).
- Wheel/trackpad normalization is a genuine subsystem with per-terminal tuning — "the least
  glamorous and most under-budgeted cost" (codex-tui2.md); it needs maintenance as terminals change
  and will misclassify some setups (hence the mandatory override).
- No IME means CJK/complex-script _composed_ input is unavailable in v0.1.
- The interim mouse/paste/kitty decode layer on `Undecoded`/`Csi` is throwaway work, and qwertty's
  high-churn `InputEvent` means rework as it stabilizes.

**Neutral.**

- Capture/target/bubble is what vxfw, Brick, and Textual all use; users from any of them find it
  familiar.
- Semantic outcomes (option D) sit _above_ raw routing, serving both low-level widget authors and
  high-level app authors from one pipeline.

## Revisit triggers

- **Stale-facts routing causes an observable bug** (e.g. rapid autocomplete where same-frame
  appear-and-click matters) → add a synchronous re-layout-before-hit-test path for pointer events.
- **Fact-derived traversal proves insufficient** (e.g. a grid needing 2-D arrow nav the visual-
  position order gets wrong) → add an explicit focus-order override on specs (a bounded tab-index,
  which Textual deliberately avoided).
- **qwertty ships kitty/mouse/paste decode** → retire the interim decode layer, consume typed events
  directly (planned deletion, terminal-substrate.md).
- **A terminal-side IME/preedit protocol emerges** (or qwertty adds a preedit event) → add preedit
  overlay rendering against the reserved cursor-candidate anchors.
- **The scroll defaults table drifts** — new terminals make the shipped events-per-tick factors
  wrong for common setups → runtime calibration probe or a shipped-config update cadence.
- **Pointer-capture / nested-routing proofs (labs roadmap steps 5–6) surface a case the capturing-
  ID model cannot express** (e.g. nested overlays with conflicting capture claims) → reopen the
  capture-ownership rules before API stabilization.

## Amendments

- **2026-07-06 (slice 3):** Clarified: `update` runs exactly once per mapped event _regardless of
  consumption_, carrying the event plus any outcomes in one `Update` — "unconsumed → passed to
  update" governs how apps should treat raw keys (check `outcome_for` first), not whether `update`
  runs. Dead-id focus recovery is defined as **first surviving focusable in declaration order**
  (facts carry no cross-frame ordinal; a stable-ordinal upgrade is a revisit option, not the
  default). Known gap, deliberately deferred to the effects slice: `request_focus` for a widget
  absent from current facts is silently ignored, which does not yet meet this ADR's "reveal or fail
  loudly" clause — tracked in docs/design/slice3-input-design.md deltas.

- **2026-07-07 (betamax round):** Two additions from running the examples in a real terminal via
  betamax tapes. (1) `Focus::reconcile` now assigns focus to the first focusable when nothing is
  focused — every example was unusable without Tab first; initial focus was unspecified. (2) The
  deferred "consumed bit" is now delivered as `Update::consumed()`: update runs for every event so
  outcomes can ride along, which means raw-key app bindings also see keys a focused widget already
  handled — the todo example's `d` binding deleted a todo while the user typed "feed". App-level
  printable-key bindings must guard on `!update.consumed()`; outcomes need no guard.

- **2026-07-07 (form UX round):** `Update::is_focused(&[Key])` added — apps need focus-dependent
  decisions (the concrete case: arrow-key field navigation in a form), and mirroring focus from
  outcomes is unreliable because Tab traversal is framework-internal. Composed-identity comparison,
  so it works at any depth. Also amended by the same round: initial auto-focus interacts with
  single-focusable apps such that printable bindings guarded on consumption can never fire (the
  stream example) — the pattern for such apps is ctrl-chords, which text inputs pass through by
  contract.
