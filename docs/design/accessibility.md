# Accessibility groundwork — recording, not exporting

How rabbitui carries the semantic information an assistive technology needs, and why the exporter
that would consume it is deliberately deferred. Arc 4 item 5 (`docs/plans/arc4-spine.md` §5).

## Position: record into facts now, export later

A terminal UI is, to an assistive technology (AT), a grid of cells with no inherent structure — a
screen reader cannot tell a button from a label from a list by looking at glyphs. The information an
AT bridge needs is _semantic_: for each interactive element, **what kind of control is it** (its
role) and **what is it called** (its accessible label). rabbitui already produces a per-frame record
of every declared widget — its identity, area, scope parent, focusability, layer — in
[`FrameFacts`](../../rabbitui-core/src/facts.rs). The decision here is to record the two missing
semantic facts into that record, so the facts carry everything an exporter needs, and stop there.

The architectural payoff (both field reports call this the tiebreaker): adding an exporter later is
a _consumer of existing data_, not a re-plumbing of the render path. The facts are the seam.

## What was added

Two render-time declarations on [`RenderCtx`](../../rabbitui-core/src/widget.rs), mirroring the
existing `focusable` / `request_visibility` readback pattern (the frame reads them back after
`render` and records them onto the widget's fact):

- `semantic_role(SemanticRole)` — the control kind, from a small closed enum
  ([`a11y::SemanticRole`](../../rabbitui-core/src/a11y.rs)): `Button`, `TextInput`, `List`, `Dialog`,
  `Label`, `Log`, `Disclosure`, plus the `None` default for decorative widgets. Recorded as a `Copy`
  field on [`FactEntry`](../../rabbitui-core/src/facts.rs) (`FactEntry::role`).
- `label(&str)` — the accessible name a screen reader would announce. Recorded in an `id → label`
  side table on `FrameFacts` (`FrameFacts::record_label` / `FrameFacts::label`), **not** on
  `FactEntry`, so `FactEntry` stays `Copy` while labels remain owned strings.

Both are present in **every build** — accessibility is not a devtools-only concern, so unlike the
`devtools` name capture (item 6) these are not feature-gated. The catalog widgets set them:

| widget         | role         | label source                        |
| -------------- | ------------ | ----------------------------------- |
| `Button`       | `Button`     | its caption                         |
| `Text`         | `Label`      | its plain text                      |
| `TextInput`    | `TextInput`  | its placeholder (its purpose)       |
| `SelectionList`| `List`       | — (items are the content)           |
| `Collapsible`  | `Disclosure` | its header                          |
| `ErrorBanner`  | `Dialog`     | `"{title}: {message}"`              |
| `LogOverlay`   | `Log`        | —                                   |

`TextInput` is labelled by its **placeholder**, not its value: the value may be empty or sensitive
(a password), while the placeholder states the field's purpose — which is what an AT should announce.

The devtools facts dump (`facts::dump` / the `FactsInspector` overlay) surfaces `role=` and
`label="…"` on a widget's line when set, so a developer can eyeball the recorded semantics.

## The export path (deferred)

An exporter would be a new, opt-in seam in the **facade** (it needs an OS boundary; core stays
runtime-free). Shape, so the deferral is honest about where it lands:

1. After each paint, the runtime already holds the frame's `FrameFacts`. An exporter reads the
   focus-ordered, role-and-label-bearing entries from it — no new render pass.
2. It maps `SemanticRole` → the platform accessibility role vocabulary and pushes an accessibility
   tree to the OS bridge: **AT-SPI** (D-Bus) on Linux, **UI Automation** on Windows, **NSAccessibility**
   on macOS. A headless **in-process probe** (the same tree as a data structure) is the testable
   first target and the one to build first — it needs no OS integration and proves the facts carry
   enough.
3. Focus changes and value changes (a `TextInput` edit) become AT notifications; the runtime already
   tracks focus across frames and re-declares facts each frame, so the diff is available.

### What is deliberately deferred and why

- **No OS bridge.** AT-SPI/UIA/NSAccessibility are large, platform-specific, and each wants its own
  event loop integration — out of scope for groundwork, and none is verifiable in this offline
  environment.
- **No live tree/notification machinery.** Recording is per-frame and cheap; wiring change
  notifications is exporter work.
- **No richer semantics yet** — no `checked`/`expanded`/`value` states, no `describedby` relations,
  no live-region politeness levels. The enum and label are the minimum an exporter needs to start;
  richer attributes are added when an exporter demands them, not speculatively.

The gate for building the exporter is a consumer that wants it (a real AT integration or the
in-process probe test); until then the facts carrying role + label is the whole deliverable, and it
is enough that adding the exporter never touches a widget.

## Tests

- `rabbitui-core`: `RenderCtx` role/label default off and are settable (`widget.rs`); the a11y enum's
  stable identifiers (`a11y.rs`).
- `rabbitui-widgets`: `tests/a11y_facts.rs` — a facts snapshot over a one-of-each catalog gallery
  asserting every widget's recorded role and label, and that the dump surfaces them.
