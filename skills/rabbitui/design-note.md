# rabbitui skill — design note

Companion to `SKILL.md` and `evals.md`. Records how the skill was built, why the evals are not yet
run, and where the author should verify before treating this as done.

## What this skill is

An agent-legibility artifact for the `rabbitui` TUI framework, following the ratatui-kit precedent
(ADR 0008 "Designed for agents"; arc5-field.md §4). It gives a fresh agent the mental model, the
four load-bearing invariants, idiomatic snippets for the ten commonest tasks, and a trap list drawn
from this repo's own build logs — so an agent can author correct rabbitui code on the first try
rather than rediscovering the declared-frame shape and re-shipping the bugs the build already found.

## How it was grounded

Every snippet and signature is taken from **current source**, not invented. Primary grounding:

- **Examples** (`rabbitui/examples/`): `hello`, `counter`, `todo`, `form`, `stream`, `fetch`,
  `agent`. These are the executable spec; the skeleton, key-binding, outcome, focus, `Cmd`, and
  commit snippets are lifted from them near-verbatim.
- **ADRs** (`docs/adr/`): 0001 (declared frame), 0006 (input/focus, incl. the two 2026-07-07
  amendments that added `Update::consumed()` and `Update::is_focused`), 0007 (theming/roles), 0008
  (widget contract), 0013 (screen modes).
- **Build-log design notes** (`docs/design/`): `slice3-input-design`, `slice4-widgets-theming`,
  `slice5-inline-vt100`, `slice6-effects`, `slice8-agent-chrome`, and `arc2a-role-audit`. The trap
  list and the invariants come from the "where the design strained" and "implementation deltas"
  sections of these — i.e. bugs that actually shipped, not hypotheticals.
- **Testing crate** (`rabbitui-testing/src/lib.rs`) and widget snapshot tests for the `TestApp`
  idiom.

The exact public signatures were cross-checked against `rabbitui-core/src/{widget,input,theme,
frame,id,layout,outcome}.rs`, `rabbitui/src/{app,effect}.rs`, and `rabbitui-widgets/src/*`.

## Why running the evals is deferred

This session **cannot spawn fresh agents to run the five evals** (the with-skill / without-skill A/B
that produces the pass/fail matrix). The eval prompts in `evals.md` are the deliverable; executing
them and recording the delta is future work. When run, the intended protocol is:

1. For each task, run a fresh agent twice — once with `SKILL.md` in context, once without — from a
   clean checkout.
2. Record pass/fail, which (if any) documented **fail signature** appeared, and iterate cycles.
3. Report the with-vs-without pass-rate delta per task.
4. **Feed failures back into the docs, not just the score** (the ADR 0008 / arc5-field.md acceptance
   criterion): a trap that reproduces _with_ the skill present is a legibility defect in the skill or
   the API, to be closed by editing this skill (or filing an API issue), not merely noted.

The five tasks were chosen to each isolate one part of the grammar or one real trap, so a failure
points at a specific fix: EVAL-2 targets the unguarded-printable trap (highest signal), EVAL-4 the
render→focus→render→send_key ordering gotcha, EVAL-3 the Accent-vs-Highlight distinction and the two
entry points, EVAL-1 the basic declare-a-widget path, EVAL-5 the effects-are-messages model.

## Judgment calls and things to verify (flagged for the author)

The following were inferred or simplified; verify before relying on them externally.

1. **`Cmd::cancel_group` turbofish.** The skill notes it "may need a turbofish where the message type
   cannot be inferred" (`Cmd::<Msg>::cancel_group("agent")`). This matches `examples/agent.rs`
   (which uses the turbofish) versus `examples/fetch.rs` (which does not, because the surrounding
   `update.spawn` fixes the type). The guidance is correct but the phrasing "may need" is a
   heuristic, not a compiler rule — worth a glance.

2. **`SelectionList::new` accepted argument types.** The skill states `Vec<String>` and `&[&str]`
   both work via `Into<ListSource>` / the `ListSource` trait. `examples/todo.rs` passes
   `app.todos.clone()` (a `Vec<String>`) and `examples/form.rs` passes a `&[&str]` const — both
   confirmed in source. If the `ListSource` impls change, this line needs updating.

3. **`Text::new` argument.** Stated as `impl Into<Content>`, so both a `&str` literal and a `&String`
   work. Confirmed against `text.rs`. The skill uses the simple `Text::new("...")` / `Text::new(&s)`
   forms only.

4. **Substrate key-vocabulary gap.** The "Traps → Substrate key gaps" item asserts the terminal
   substrate currently decodes only text, C0 controls, and the four arrows, so `BackTab`, `Home`,
   `End`, `PageUp`, `PageDown`, forward `Delete`, and non-`ctrl` modifiers may not arrive. This is
   from ADR 0006 §9 and the slice-3 note (dated 2026-07-07). **This is the item most likely to go
   stale** as qwertty lands more protocols — re-check against `rabbitui/src/input.rs` and the
   substrate status before publishing.

5. **Role background/foreground semantics.** The Accent (fg-only) vs Highlight (bg-carrying) vs Muted
   (`Ansi(8)`, never DIM) description is from `arc2a-role-audit.md` and the `Theme::dark()` tuning.
   Individual presets (Nord, Dracula, Catppuccin) may realize these roles with different concrete
   colors, but the _semantic_ contract (which role carries a background) is what the skill teaches
   and what widgets rely on. Verify the semantic contract still holds if the role set is retuned.

6. **No custom-widget deep-dive.** The skill points at the `SubmitButton` in `examples/form.rs` as
   the reference for implementing `rabbitui_core::widget::Widget` rather than reproducing the full
   trait surface (`render`, `handle`, `desired_height`, `RenderCtx`, `HandleCtx`, `Handled`,
   `Phase`). This keeps the skill focused on app-authoring (the common case) over widget-authoring;
   a
   dedicated widget-authoring skill or section is a reasonable future addition if agents are asked to
   write widgets often.

## Naming

Per the arc5 naming rule, `rabbitui`/`qwertty` are internal names not to leak into
**externally-facing** documents. This skill is an **in-repo developer artifact** (it teaches the
actual framework by its real crate names — it cannot do its job otherwise), so the rule does not
apply here, exactly as it does not apply to the ADRs or the rustdoc. It is not an outward-facing
publication. Flagging explicitly so the boundary is on record.

## Status

- `SKILL.md`, `evals.md`, this note: complete, markdownlint-clean, grounded in current source.
- Eval **execution**: deferred (cannot spawn fresh agents this session). Prompts + protocol ready.
- Recommended next step once agents can be spawned: run the five evals, record the matrix here, and
  close any reproduced trap with a skill/API edit.
