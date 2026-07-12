# Slice 7 design: overlays, forms, mouse

Working design note for slice 7 (ROADMAP.md): z-layers, mouse routing, and a real form. Plus two
carried-forward slice-6 items.

## Layers: facts-level z first, buffer compositing only when needed

ADR 0003 describes buffer-level layer compositing. v1 realizes layers more cheaply, and this is a
recorded delta to fold back: **declaration order is already the painter's algorithm** in a single
buffer, and facts hit-testing already prefers the last-declared entry. What a modal actually needs
beyond that is _event containment_, so layers land as an input-scoping concept:

- `Frame::layer(key, scope)` — declares an overlay scope. Fact entries inside it carry `layer: u8`
  (base = 0, incremented per nested layer declaration, declaration order among siblings).
- **Focus traversal is restricted to the topmost declared layer** (Tab cycles only the modal's
  focusables while a modal exists; when the layer disappears, focus reconciles to the surviving base
  per the existing rule).
- **Hit-testing prefers the highest layer**, then last-declared within it.
- Key routing: if the target's layer is below the topmost layer, the event is first offered to the
  topmost layer's… no — simpler and sufficient: focus can only be _in_ the topmost layer, and mouse
  targets resolve top-first, so containment falls out of the two rules above. A `dim` helper on the
  base (paint a `Muted`-role overlay wash) is a widgets-crate utility, not core.
- Buffer-level compositing (true transparency, effects) is deferred until a widget genuinely needs
  it; ADR 0003's compositing language gets an amendment note pointing here.

## Mouse

- **Core**: `input::MouseEvent { kind: Down|Up|Drag|Scroll(i8), button, position, modifiers }`,
  `InputEvent::Mouse(MouseEvent)`.
- **Substrate reality**: qwertty emits no mouse events; SGR mouse reports arrive as _preserved
  complete CSI_ (`CSI < b;x;y M/m`). Interpreting a complete preserved CSI in the facade is the same
  interim posture as the SGR encoder — qwertty owns framing, we bridge semantics until it grows
  typed mouse events (filed in the requirements handover). This does NOT fork the byte decoder.
- **Enabling**: `App::mouse(bool)`; default **on in alt-screen, off in inline** — mouse capture
  steals native scrollback scrolling, which inline mode exists to preserve (the gap-analysis
  scroll-capture tradeoff, applied to ourselves). Enable = mode 1000+1006 via the encoder at mode
  entry, disable at leave/restore (add to RESTORE unconditionally — disabling when never enabled is
  harmless).
- **Routing**: target = `facts.hit(position)` (layer-aware); same capture→target→bubble path as
  keys; unconsumed clicks focus the target if focusable (the universal expectation), then fall
  through to update.
- Widgets: Button consumes Down → `Activated`; SelectionList Down on a row → select (+`Selected`),
  Scroll moves selection; TextInput Down → focus (cursor placement by column is a recorded later
  refinement).

## Visibility requests (plumbing only this slice)

`ctx.request_visibility(rect)` (area-relative) records a fact. No generic scrollable container
exists yet to consume them; the fact + a `FrameFacts::visibility_requests()` query land now so the
container work (catalog phase) has its contract. Tested at the facts level.

## Carried forward from slice 6

- `Pending` gains `extend(Pending)` and the runtime holds an _unapplied remainder_ across one frame:
  commands/focus that missed this frame's facts retry once against the next frame's before the
  debug_assert — closing the declare-then-focus edge properly.
- `Command::cancel_group(name)`: a spawnable that aborts a group without starting a replacement (the
  stream-stop primitive; fetch.rs's ticker toggle uses it instead of ignoring a running stream).

## examples/form.rs

Name/email TextInputs + a notes field + Submit Button; inline validation (status line per field via
roles); Submit opens a `layer` modal confirm (Ok/Cancel Buttons — Tab provably cycles only those
two; Esc dismisses); mouse: click to focus fields, click Ok/Cancel, wheel over the (small) notes
list. Proves layers, containment, mouse routing, and the retry-frame focus.

## Testing

Facts: layer assignment, top-layer hit preference, focus order restricted to top layer,
visibility-request recording. Routing: click-to-focus, click Activated, wheel Selected, modal
containment (Tab in modal never reaches base; base widget receives nothing while modal exists),
layer-dismiss focus reconciliation. Pending: extend + one-frame retry (declare-then-focus now
passes). vt100: mouse enable/disable bytes at mode entry/leave/restore. TestApp: send_mouse(kind,
position).

## Implementation deltas

Landed as specified. Decisions where the note was silent, and deviations:

- **Layers held up at the facts level without buffer compositing.** `FactEntry` gained a
  `layer: u8`; `Frame::layer(key, scope)` is a `scoped`-with-`layer+1` that reuses the
  identity-subtree machinery verbatim. `hit()` became `max_by_key(layer)` over containing entries —
  `max_by_key` returns the _last_ maximum on ties, which is exactly "highest layer, then
  last-declared within it", so the two z-order rules collapse into one line. `focus_order()` filters
  to `top_layer()`. No compositing pass, no buffer changes, no `Clear`-then-overpaint — declaration
  order already paints correctly in the single buffer, and containment is purely an input-scoping
  property of the two facts queries. Feedback: this is the right cut for v1; the only thing a modal
  wants beyond painter's-order is event containment, and layer-tagged facts deliver it at near-zero
  cost. A `dim`/`Muted`-wash helper on the base was **not** built — the note calls it a
  widgets-crate utility and nothing in scope needs it; deferred.

- **Preserved-CSI mouse parsing: no surprises, one clean seam.** qwertty's `CsiInput` already
  exposes `private_marker_bytes()` (`<`), `parameter_bytes()` (`<b;x;y`, marker included),
  `final_byte()` (`M`/`m`), and `intermediate_bytes()`. The bridge reads those fields — it does
  **not** re-parse bytes or fork the decoder — matching the SGR _encoder_'s interim posture. The one
  wrinkle worth noting: `parameter_bytes()` includes the leading `<`, so the bridge strips it before
  splitting on `;`. A non-mouse CSI (cursor report, etc.) falls through to the slice-3 "dropped"
  path unchanged. Wheel normalization is the v1 one-line-per-notch stub (ADR 0006 §6 per-terminal
  tuning deferred); `MouseKind::Scroll(i8)` carries a signed line count so the richer model slots in
  without an API change.

- **Routing: mouse targets the hit region, click-to-focus is unconsumed-only.** `route()` selects
  the target by `facts.hit(position)` for mouse events (else `focus.current` for keys); everything
  after (capture→target→bubble) is shared. Click-to-focus was implemented unconsumed-only per the
  note's literal wording, then **revised in review to Textual's rule**: any left `Down` on a
  focusable target moves focus, before dispatch, consumed or not — a click that activates a Button
  but leaves focus elsewhere makes the next Tab start from a stale position. Focus moves first, then
  the handler runs (activation happens as the focused widget). The event is still never consumed by
  focusing itself. Pinned by `routing::tests::consumed_click_also_focuses_the_target`.

- **One-frame retry wired into the runtime, not just core.** `Pending::extend` +
  `Pending::apply_deferred` (returns the unapplied focus request as a remainder instead of asserting
  on the first miss). The runtime holds a `widget_remainder` across exactly one frame:
  `drain_pending` applies via `apply_deferred` and folds the remainder forward; after the next
  redraw the remainder retries against the fresh facts via the asserting `apply`. This is what makes
  the form's submit→open-modal→focus-OK declare-then-focus work without a debug panic.
  `TestApp::apply_pending` keeps the single-shot `apply` (a test controls its own frames), so its
  contract is unchanged.

- **Mouse enable/disable lives in the `ModeEngine` wrapper, not the pure engines.**
  `AltEngine`/`InlineEngine` stay mouse-agnostic; `ModeEngine` (app.rs) resolves capture
  (`App::mouse` override, else on-in-alt/off-in-inline) and prepends `ENABLE_MOUSE` to `enter()` /
  appends `DISABLE_MOUSE` to `leave()`. `RESTORE` and `Terminal::close` disable unconditionally. The
  vt100 assertion feeds the full `ModeEngine` entry→frame→leave stream through `VtScreen` (a
  dev-dependency of the facade) and checks the enable/disable sequences at the transitions; the
  byte-exact sequences are also unit-tested in `encode`.

- **ADR 0003 amendment (not our job, noted here):** ADR 0003 still describes buffer-level layer
  compositing as the layers mechanism. This slice realized layers as facts-level input scoping
  instead (see the note's opening section). ADR 0003 wants an amendment pointing here; `docs/adr/`
  was not touched per the task rules.
