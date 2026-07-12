# rabbitui skill — eval task prompts

Five task prompts for evaluating the `rabbitui` skill (arc5-field.md §4, ratatui-kit precedent).
Each is a self-contained task a fresh agent is given **with** the skill and **without** it; the
delta measures the skill's value. Each carries an objective pass condition so a grader (human or
harness) can score pass/fail without judgement calls, and each targets a distinct load-bearing part
of the grammar or a known trap.

**Running these is deferred** — see `design-note.md` for why and for the intended protocol. The
prompts below are the deliverable; the pass/fail matrix is future work.

Each task starts from a clean checkout of the workspace at
`/Users/joshka/local/rabbitui/work/default`, targets `rabbitui/examples/` or `*/tests/`, and must
end with `cargo check --workspace --all-targets` (and, where the task adds a test,
`cargo test -p <crate>`) passing. The agent may read the codebase but must not be handed the answer.

---

## EVAL-1 — Add a widget to an example

> In `rabbitui/examples/counter.rs`, add a second line under the count that shows the count's
> parity as the word `even` or `odd`, styled with `Role::Success` when even and `Role::Warning`
> when odd. Do not change the existing count line or the key bindings. Keep the example compiling
> and its layout sensible.

**Pass condition.** `cargo run --example counter` compiles; a new `Text` widget is declared with a
**stable, unique key** into its own laid-out row (not overlapping the count); the role switches on
parity. **Fail signatures:** reusing an existing widget's key (state/paint collision), hard-coding a
color instead of a `Role`, or writing into an area that overlaps the count row.

**What it probes.** The core declare-a-widget grammar: `frame.widget(key(...), area, &Text::new(...)
.role(...))`, laying out a new row with `split_rows`, and the roles-not-colors rule.

---

## EVAL-2 — Add a key binding

> In `rabbitui/examples/todo.rs`, add a key binding: pressing `c` (lowercase) clears **all** todos
> at once (empties the list and resets the selection). It must behave like the existing `d` binding
> with respect to focus — that is, typing a word containing `c` into the input field must **not**
> clear the list.

**Pass condition.** The `c` binding lives in the app-level `Event::Input` block **guarded by
`!update.consumed()`**, alongside the existing `d`/`q` arms; typing "cat" into the focused input adds
a todo without wiping the list. **Fail signatures:** adding the `c` arm without the consumed-guard
(the canonical bug), or putting it in an unguarded branch that also runs for consumed keys.

**What it probes.** The single most common real bug: unguarded printable app bindings fighting a
focused text input (the `d`-in-"feed" trap, ADR 0006 amendment). This is the highest-signal eval.

---

## EVAL-3 — Theme a panel

> In `rabbitui/examples/hello.rs`, make the panel render as a **focused** panel (its border should
> read as focused), switch the app to the Nord theme, and change the greeting text to use the
> highlight role rather than the accent role. Do not otherwise change the layout or the quit
> binding.

**Pass condition.** `Panel::new()...focused(true)` is set; the app is built via the `App::new(...)
.theme(Theme::nord()).run()` builder (not bare `app::run`, which takes no theme); the greeting uses
`Role::Highlight`. **Fail signatures:** trying to pass a theme to `app::run` (wrong entry point);
using `Role::Accent` for a filled/selected look or `Role::Highlight` where only a foreground accent
was asked (Accent-vs-Highlight confusion); reaching for a DIM attribute or a raw color.

**What it probes.** The theming surface — the two entry points (`app::run` vs the `App` builder),
the preset names, `Panel::focused`, and the Accent-vs-Highlight role distinction.

---

## EVAL-4 — Write a snapshot test

> Add a test to `rabbitui-widgets/tests/` that renders a `Button` labelled `Go`, focuses it, sends
> an `Enter` key, and asserts the button emits `Outcome::Activated`. Use the headless `TestApp`
> harness. The test must pass under `cargo test -p rabbitui-widgets`.

**Pass condition.** The test uses `TestApp::new(Size, ())`, renders the button in a view, sets focus
via `set_focus(Some(WidgetId::ROOT.child(key("..."))))`, **re-renders after setting focus** (so the
focus reconciles against the facts), then `send_key(Key::Enter)` and asserts on the returned
`RouteResult`'s `outcomes`/`consumed`. `cargo test -p rabbitui-widgets` is green. **Fail signatures:**
asserting on the outcome without re-rendering after `set_focus` (focus not reconciled, key routes
nowhere); expecting `send_key` to re-render on its own; hand-rolling a store/buffer instead of using
`TestApp`.

**What it probes.** The testing idiom and one of its sharpest gotchas (render → set_focus → render →
send_key), plus that `send_key` returns a `RouteResult` and does not paint.

---

## EVAL-5 — Add a Command (async effect)

> In `rabbitui/examples/fetch.rs`, add a `Command` that runs when the user presses `Ctrl-R`: after a
> simulated 500ms delay it should deliver a message that sets the results list to a single row,
> `"reloaded"`. Reuse the existing `Msg` enum (add a variant if needed) and the existing message
> handling in `update`. The debounced-search behaviour must keep working.

**Pass condition.** A `Command::future(async move { sleep(...).await; Msg::... })` is spawned via
`update.spawn(...)` from a `Ctrl-R` arm; a matching `Event::Message(...)` arm in `update` sets
`app.results`; the example compiles and the search still works. **Fail signatures:** doing the sleep
synchronously in `update` (blocking the loop) instead of inside the future; mutating state from
inside the async block (it cannot see `&mut app` — results must arrive as a message); forgetting the
`Event::Message` handler so the result is dropped.

**What it probes.** The commands-only effect model: futures re-enter as messages, effects never touch
app state directly, and `update.spawn` is the issue point.

---

## Scoring

For each task, record for the with-skill and without-skill runs: **pass/fail**, whether any **fail
signature** above appeared, and the number of compile/iterate cycles. The intended headline metric is
the with-vs-without pass-rate delta per task, with the fail-signature column showing _which_ trap the
skill prevented. Per the acceptance criterion, an eval failure that reproduces with the skill present
feeds a **doc/skill fix**, not just a score — the trap it exposes is a legibility defect to close.
