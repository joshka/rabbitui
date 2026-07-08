# Dogfood findings — the second app (`comparisons/rabbitui`, a log-follower)

Building a second real rabbitui app (a streaming log-follower with a filter and a
detail modal) surfaced these framework rough edges — the same feedback loop the flagship
gave, from an independent app. Ranked; the top three bite any second real app immediately.

## Resolution status (2026-07-08)

| #   | Finding                       | Status | Landed as                                              |
| --- | ----------------------------- | ------ | ------------------------------------------------------ |
| 1   | startup/init `Cmd` hook       | done   | `Event::Started` (one-shot, pre-first-input)           |
| 2   | widget-state reader           | done   | `Update::widget_state::<W>(path)`                      |
| 3   | `view` can't read focus       | done   | `Frame::is_focused` / `Frame::focused`                 |
| 4   | declare-then-command panic    | done   | `Update::try_focus` / `try_command` + `apply_guarded`  |
| 5   | lazy `ListSource` over `T`    | done   | `rows_with` / `from_fn` / `FromFn`                     |
| 6   | empty↔populated focus drop    | done   | `SelectionList::empty_text`                            |
| 7   | global chords at every return | done   | pattern: check globals at top of `update` (adopted)    |
| 8   | `frame.split` naming sugar    | done   | `Frame::split_rows` / `Frame::split_columns`           |

## Findings (ranked — 1–4 are the substantive framework fixes; 5–8 are papercuts)

1. **No startup/init command hook.** `App::run` never calls `update` until the first
   input/resize (the first frame draws from `view`). A self-starting stream therefore
   can't begin at launch — the app had to spawn its `Cmd::stream` lazily on the first
   `update` behind a `started` flag and tell the user "press a key to start." **Want:**
   a builder `.init(|| Cmd)` or an `Event::Started` delivered once before first input.

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

## Also noted

- The detail modal's `message` value rendered thin in one screenshot — a minor layout
  detail in the app to revisit, not a framework issue.

Findings 1–4 are the strongest candidates to feed into framework work; 4 in particular
generalizes the declare-then-focus lesson into a policy question about the whole
declare-then-X contract.
