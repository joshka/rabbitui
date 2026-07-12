# Dogfood findings — the second app (`comparisons/rabbitui`, a log-follower)

Building a second real rabbitui app (a streaming log-follower with a filter and a
detail modal) surfaced these framework rough edges — the same feedback loop the flagship
gave, from an independent app. Ranked; the top three bite any second real app immediately.

## Resolution status (2026-07-08)

| #   | Finding                       | Status | Landed as                                              |
| --- | ----------------------------- | ------ | ------------------------------------------------------ |
| 1   | startup/init `Command` hook   | done   | `Event::Started` (one-shot, pre-first-input)           |
| 2   | widget-state reader           | done   | `Update::widget_state::<W>(path)`                      |
| 3   | `view` can't read focus       | done   | `Frame::is_focused` / `Frame::focused`                 |
| 4   | declare-then-command panic    | done   | `Update::try_focus` / `try_command` + `apply_guarded`  |
| 5   | lazy `ListSource` over `T`    | done   | `rows_with` / `from_fn` / `FromFn`                     |
| 6   | empty↔populated focus drop    | done   | `SelectionList::empty_text`                            |
| 7   | global chords at every return | done   | pattern: check globals at top of `update` (adopted)    |
| 8   | `frame.split` naming sugar    | done   | `Frame::split_rows` / `Frame::split_columns`           |

Findings 9–11 (the `Table` adoption, 2026-07-11) are open; see the section below.

## Findings (ranked — 1–4 are the substantive framework fixes; 5–8 are papercuts)

1. **No startup/init command hook.** `App::run` never calls `update` until the first
   input/resize (the first frame draws from `view`). A self-starting stream therefore
   can't begin at launch — the app had to spawn its `Command::stream` lazily on the first
   `update` behind a `started` flag and tell the user "press a key to start." **Want:**
   a builder `.init(|| Command)` or an `Event::Started` delivered once before first input.

2. **Widget state is write-only from the app.** `Update::widget::<W>(path, |s| …)` mutates
   between frames but there is no _reader_. To know a `SelectionList`'s selected row the
   app must mirror it from every `Outcome::Selected(i)` — exactly the duplicated state the
   framework should own. (`SelectionList`'s own doc says "the app reads the authoritative
   selection from the widget state," but no API does.) **Want:**
   `update.widget_state::<W>(path) -> Option<&W::State>`.

3. **`view` can't read focus.** Only `Update::is_focused` exists; `Frame` has no
   `is_focused(path)`. Focus-reactive chrome (highlight the focused panel) forces the app
   to mirror focus into its own state each event. The flagship dodges this by hardcoding
   `.focused(true)` on its single composer; a two-region app can't. **Want:**
   `frame.is_focused(path)` (or `Panel` reading framework focus directly).

4. **`declare-then-command` panics — the widget sibling of declare-then-focus.**
   _(Found by the coordinator's betamax run; the agent couldn't hit it without a TTY.)_
   The app renders `key("list")` only when the filter matches something, else a
   `key("empty")` placeholder. A deferred `update.widget::<SelectionList>(key("list"), …)`
   issued when the next frame shows the placeholder hits an **undeclared** widget →
   `pending.rs` panic ("no retained state … cannot be commanded"). This is the exact
   family as the flagship's help-overlay declare-then-_focus_ panic. **Two independent
   apps hit the declare-then-X footgun.** **Want:** a command/focus request to an
   undeclared id should be a soft no-op (or warn), not a `debug_assert` panic — or a
   guarded API (`try_focus`/`try_command`). Worked around here by guarding on
   `!visible().is_empty()`.

5. **`ListSource` only for `Vec<String>`/`&[String]`/`&[&str]`.** A filtered view of a
   custom type must be materialized into a fresh `Vec<String>` _every frame_, so the
   app-side allocation defeats the virtualization the list works to provide (fine at 500
   rows, not at the "million-row source" it advertises). **Want:** `ListSource` over a
   borrowed slice of `T` + a row-formatting closure, or `ListSource for Vec<T: Display>`.

6. **Empty↔populated key swap drops focus.** Rendering `key("empty")` instead of
   `key("list")` makes the focusable list vanish and focus silently falls back. No
   "focusable placeholder"; re-appearing widgets reset focus. A `SelectionList` that
   renders its own empty state would avoid the swap (and #4).

7. **Global chords must be repeated at every early `return`.** Because app-level bindings
   live at the end of `update`, the modal branch's early `return` made the bottom Ctrl-C
   unreachable — it had to be re-checked inside the modal branch. (The flagship has the
   same shape.) **Want:** an always-checked "global chords" hook, or route quit before the
   overlay branches.

   **Resolved by pattern, not new API (2026-07-08).** The framework already delivers
   every event to `update` and exposes `update.action(&keymap)` / `update.event()`; the
   fix is discipline, not surface — check global chords at the **top** of `update`, before
   any early-return branch. The log-follower now hoists its Ctrl-C quit into one top-level
   block and drops the two duplicated copies. A dedicated `App::on_global(hook)` would add
   a boxed always-runs closure (and questions: does it see consumed events? effect
   messages?) — deferred until the pattern proves insufficient, since a heavyweight hook is
   not worth a papercut a three-line hoist removes.

8. **`rows()`/`columns()` split the whole frame only; sub-areas use
   `split_rows`/`split_columns(area, …)`.** Minor naming friction (rows = horizontal
   bands). A `frame.split(area, …)` sugar would read more consistently.

## Table adoption (2026-07-11) — findings 9–11

Swapping the log-follower's `SelectionList` of pre-formatted rows for a columnar
`Table` (seq / level / target / message over `table_rows_with` on the app's
`visible()` slice) surfaced three more edges. The adoption itself was clean — the
API reads exactly like `SelectionList` (same `empty_text`, same `select`, same
`Outcome::Activated`, same `widget_state().selected()`), so a user who knew one
knew the other. These are the gaps that remained.

**9. A widget command needs a _nameable_ source type, but the lazy sources are
unnameable.** Reading the selection and resetting it after a filter go through
`update.widget_state::<W>(path)` / `update.widget::<W>(path, …)`, which are generic
over the whole widget `W`. But the source the view actually declares is
`Table<TableFromFn<{closure}>>` — the closure type is unnameable, so `W` cannot be
written down. The app names a _phantom_ source it never uses —
`Table<Vec<Vec<String>>>` — and it works only because every `Table<S>` shares one
`TableState` and the lookup keys on `W::State` (`peek::<W::State>` /
`downcast::<W::State>`), not on `W`. This is inherited from `SelectionList` (the
old code named `SelectionList<Vec<String>>` over a `FromFn` source for the same
reason), so it is not new — but the `Table` adoption makes it sharper, because the
"obvious" `W` to write (`Table<_>` inference, or the real source type) is exactly
the one that won't compile. Nothing in the type system tells a reader the source
parameter is a phantom, and a wrong choice fails with a `TableSource`-bound error
that points nowhere near the real cause. **Want:** a state-typed accessor that
skips the widget's generic source — `update.widget_state_as::<TableState>(path)` /
`widget_as::<TableState>(path, …)` — or a documented `Table` type alias for "any
source, for command typing." Cheapest honest fix: a one-line rustdoc on
`widget`/`widget_state` naming this pattern (pick any concrete source; the state is
what is keyed).

**10. No per-row or per-cell styling — semantic color can't ride the lazy seam.**
`Table` paints every body row in one uniform role: `Role::Text`, or
`Role::Highlight`/`Role::Accent` for the selection. A log viewer's primary
scannability cue is _level color_ — ERROR in danger red, WARN in warning yellow —
and the app already owns `Level::role()`, but there is no way to hand it to the
widget. The `level` column renders in the same grey as everything else.
`SelectionList` has the identical limitation (the old app never colored its rows
either, so this is _not a regression_), but the columnar layout makes the gap
conspicuous: a reader expects a typed `level` column to carry its level's color.
The fix must preserve virtualization — the style has to be pulled lazily per
painted cell, exactly like the text. **Want:** an optional per-row style on
`TableSource` (`fn row_role(&self, row: usize) -> Option<Role> { None }`, called
only for painted rows) or a per-cell variant, so semantic coloring flows through
the same on-demand seam as `cell()` and a million-row source still costs one
screenful. This is the strongest candidate of the three to feed into framework
work — level color is the single feature a real log table is judged on.

**11. `SemanticRole::Table` is still absent (B2's flag, confirmed).** `Table`
declares `SemanticRole::List` because `rabbitui-core` has no `Table` variant (B2
completion note flagged this). It did not block the adoption — a table _is_ a
selectable set of rows for a11y purposes — but a screen reader announcing a
columnar grid as a flat list loses the column structure. **Want:** a
`SemanticRole::Table` (rows × columns) in `rabbitui-core`; `Table` adopts it. Out
of this lane; noted for the coordinator. (B2 also flagged two private
`truncate_to_width` twins and a slice-based `split_lengths` — both internal to
`rabbitui-widgets`; neither bit the app.)

## Also noted

- The detail modal's `message` value rendered thin in one screenshot — a minor layout
  detail in the app to revisit, not a framework issue.

Findings 1–4 are the strongest candidates to feed into framework work; 4 in particular
generalizes the declare-then-focus lesson into a policy question about the whole
declare-then-X contract.
