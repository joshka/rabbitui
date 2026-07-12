# Wave C — forms + catalog extraction (implementation spec)

Written 2026-07-11 on Fable. "The catalog is the product"; forms are the sharpest catalog
sub-gap (`core-model-and-roadmap.md` §4, `recent-rust-tui-wave.md` §2). C1 is fully
specced; C2/C3 are scoped with their design constraints pinned so a later pass can spec
them without re-research. Depends on Wave A (examples idiom) but not B2.

## C1 — `FormScope`: declared-frame forms (fully specced)

**Design constraint that shapes everything:** rabbitui widgets do not nest (`Panel` is a
backdrop; children are declared through `Frame`, not owned by widgets). So a Form is NOT a
container widget — it is a **declaration helper**, exactly like `ScrollScope`:

```rust
// rabbitui-widgets/src/form.rs
pub struct FieldSpec<'a> {
    label: &'a str,
    error: Option<&'a str>,     // validation is app-land; the form only displays it
    required: bool,
}
impl<'a> FieldSpec<'a> {
    pub fn new(label: &'a str) -> Self;
    pub fn error(self, err: Option<&'a str>) -> Self;
    pub fn required(self) -> Self;
}

pub struct FormScope<'a, 'f> { /* frame, area, label_width, cursor_y */ }
impl FormScope<'_, '_> {
    /// Declares one field row: right-aligned label column, the input widget, and an
    /// error line below when `error` is set (Role::Error). Key scopes the input.
    pub fn field<W: Widget>(&mut self, key: Key, spec: FieldSpec<'_>, widget: &W);
    /// Vertical gap row.
    pub fn gap(&mut self, rows: u16);
    /// A trailing button row (e.g. Submit / Cancel), right-aligned.
    pub fn buttons(&mut self, f: impl FnOnce(&mut Frame<'_>, Rect));
}

// entry — a free fn or a Frame extension in widgets (NOT core; it is catalog policy):
pub fn form(frame: &mut Frame<'_>, key: Key, area: Rect, f: impl FnOnce(&mut FormScope));
```

Behavior:

- Label column width = max label width across the calls this frame (two-pass or
  caller-supplied `label_width` — implement caller-supplied with a
  `measure`-style helper first; it is simpler and explicit).
- Each `field` consumes `1 + error.is_some() as u16` rows plus the widget's
  `desired_height`; the scope tracks `cursor_y` down the area.
- Focus traversal needs nothing new — fields are ordinary declared widgets; Tab order
  falls out of frame facts (declaration order). Required-marker `*` renders in
  `Role::Error` next to the label.
- Validation is app-land by contract (ADR 0001: framework never owns app state): the app
  validates on `Changed`/`Submitted` outcomes and passes `error: Option<&str>` back in.
  The form displays; it never judges.

Deliverables: `form.rs` + re-exports; rewrite `examples/form.rs` on it (it currently
hand-rolls this layout — the diff is the proof of value); widget tests in the
`selection_list.rs` style (label column alignment, error line appears/disappears, focus
order matches declaration order, `desired_height` accounting); dogfood-findings entry for
any friction.

Acceptance: workspace suites + clippy + fmt green; `form` example visually verified
(coordinator betamax); commit `feat(widgets): FormScope — declared-frame forms`.

## C2 — `#[derive(Form)]` (scoped, needs its own spec pass)

Target: `#[derive(Form)] struct Login { #[form(label = "User")] user: String, … }`
generating (a) the `FormScope` declarations, (b) a typed extraction from widget state via
`Update::widget_state`, (c) per-field validator hook points. Constraints pinned now:

- New proc-macro crate `rabbitui-form-derive`, re-exported through `rabbitui-widgets`
  behind a `derive` feature. Nothing in core.
- Generates _declarations_, not a retained form object — the derive writes the same
  `form(frame, …)` calls a human would (keep the expansion readable; it is a teaching
  artifact too).
- Do not start until C1 has survived one real consumer (the example + one dogfood app).

## C3 — extract agent-chrome widgets from the flagship (scoped)

The era's flagship archetype; vendors keep re-extracting these in-house. Extraction list,
in dependency order (all from `rabbitui-agent/src/`):

1. **Markdown cell** (`markdown.rs` → `rabbitui-widgets` behind a `markdown` feature with
   the `pulldown-cmark` dep) — styled-span rendering of a markdown block.
2. **Tool/status cell** — the collapsible call cell with spinner→`✓`/`✗` lifecycle and
   `committable_end` semantics (the scrollback-freeze lesson encoded as API).
3. **Composer** — multi-line input + key-hint footer + submit/cancel outcomes.

Constraints pinned: each must keep the flagship compiling against the extracted version in
the same commit (the flagship is the acceptance test — ADR discipline); `commit`-related
semantics (append-once, terminal-status gating) move with the tool cell, documented; the
transcript container itself WAITS for Wave B2 (it wants variable-height virtualization).

Do C3 after B2 lands; item 1 (markdown) is independent and can go anytime.

## Sequencing summary

- C1: after Wave A. One lane, small.
- C2: after C1 + one real consumer. Own spec pass first.
- C3.1 (markdown): anytime. C3.2/C3.3: after B2.
