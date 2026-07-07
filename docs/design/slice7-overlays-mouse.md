# Slice 7 design: overlays, forms, mouse

Working design note for slice 7 (ROADMAP.md): z-layers, mouse routing, and a
real form. Plus two carried-forward slice-6 items.

## Layers: facts-level z first, buffer compositing only when needed

ADR 0003 describes buffer-level layer compositing. v1 realizes layers more
cheaply, and this is a recorded delta to fold back: **declaration order is
already the painter's algorithm** in a single buffer, and facts hit-testing
already prefers the last-declared entry. What a modal actually needs beyond
that is *event containment*, so layers land as an input-scoping concept:

- `Frame::layer(key, scope)` — declares an overlay scope. Fact entries inside
  it carry `layer: u8` (base = 0, incremented per nested layer declaration,
  declaration order among siblings).
- **Focus traversal is restricted to the topmost declared layer** (Tab cycles
  only the modal's focusables while a modal exists; when the layer disappears,
  focus reconciles to the surviving base per the existing rule).
- **Hit-testing prefers the highest layer**, then last-declared within it.
- Key routing: if the target's layer is below the topmost layer, the event is
  first offered to the topmost layer's… no — simpler and sufficient: focus can
  only be *in* the topmost layer, and mouse targets resolve top-first, so
  containment falls out of the two rules above. A `dim` helper on the base
  (paint a `Muted`-role overlay wash) is a widgets-crate utility, not core.
- Buffer-level compositing (true transparency, effects) is deferred until a
  widget genuinely needs it; ADR 0003's compositing language gets an
  amendment note pointing here.

## Mouse

- **Core**: `input::MouseEvent { kind: Down|Up|Drag|Scroll(i8), button, position, modifiers }`,
  `InputEvent::Mouse(MouseEvent)`.
- **Substrate reality**: qwertty emits no mouse events; SGR mouse reports
  arrive as *preserved complete CSI* (`CSI < b;x;y M/m`). Interpreting a
  complete preserved CSI in the facade is the same interim posture as the SGR
  encoder — qwertty owns framing, we bridge semantics until it grows typed
  mouse events (filed in the requirements handover). This does NOT fork the
  byte decoder.
- **Enabling**: `App::mouse(bool)`; default **on in alt-screen, off in
  inline** — mouse capture steals native scrollback scrolling, which inline
  mode exists to preserve (the gap-analysis scroll-capture tradeoff, applied
  to ourselves). Enable = mode 1000+1006 via the encoder at mode entry, disable
  at leave/restore (add to RESTORE unconditionally — disabling when never
  enabled is harmless).
- **Routing**: target = `facts.hit(position)` (layer-aware); same
  capture→target→bubble path as keys; unconsumed clicks focus the target if
  focusable (the universal expectation), then fall through to update.
- Widgets: Button consumes Down → `Activated`; SelectionList Down on a row →
  select (+`Selected`), Scroll moves selection; TextInput Down → focus (cursor
  placement by column is a recorded later refinement).

## Visibility requests (plumbing only this slice)

`ctx.request_visibility(rect)` (area-relative) records a fact. No generic
scrollable container exists yet to consume them; the fact + a
`FrameFacts::visibility_requests()` query land now so the container work
(catalog phase) has its contract. Tested at the facts level.

## Carried forward from slice 6

- `Pending` gains `extend(Pending)` and the runtime holds an *unapplied
  remainder* across one frame: commands/focus that missed this frame's facts
  retry once against the next frame's before the debug_assert — closing the
  declare-then-focus edge properly.
- `Cmd::cancel_group(name)`: a spawnable that aborts a group without starting
  a replacement (the stream-stop primitive; fetch.rs's ticker toggle uses it
  instead of ignoring a running stream).

## examples/form.rs

Name/email TextInputs + a notes field + Submit Button; inline validation
(status line per field via roles); Submit opens a `layer` modal confirm
(Ok/Cancel Buttons — Tab provably cycles only those two; Esc dismisses);
mouse: click to focus fields, click Ok/Cancel, wheel over the (small) notes
list. Proves layers, containment, mouse routing, and the retry-frame focus.

## Testing

Facts: layer assignment, top-layer hit preference, focus order restricted to
top layer, visibility-request recording. Routing: click-to-focus, click
Activated, wheel Selected, modal containment (Tab in modal never reaches base;
base widget receives nothing while modal exists), layer-dismiss focus
reconciliation. Pending: extend + one-frame retry (declare-then-focus now
passes). vt100: mouse enable/disable bytes at mode entry/leave/restore.
TestApp: send_mouse(kind, position).
