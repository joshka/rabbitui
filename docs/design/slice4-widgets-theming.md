# Slice 4 design: TextInput, SelectionList, theming

Working design note for slice 4 (ROADMAP.md), implementing ADR 0007 (theming)
and deepening ADR 0008 (widget contract). Supersede or fold into ADRs if
implementation contradicts it.

## Theming (ADR 0007 made concrete)

- `rabbitui-core::theme`: a closed `Role` enum v1 — `Surface`, `Text`, `Muted`,
  `Accent`, `Success`, `Warning`, `Danger`, `Border`, `Highlight` — and
  `Theme` mapping roles to `Style`s (`Theme::style(Role) -> Style`,
  `Theme::default()` = a restrained dark default). Presets as const fns:
  `theme::catppuccin_mocha()` ships this slice (Nord/Dracula later).
- `Frame` carries `&Theme`; `RenderCtx::style(Role) -> Style` is how widgets
  ask. Widgets never hardcode colors — Button and Text migrate to roles
  (Button focused = `Highlight`, label = `Text`; Text default = `Text`).
- Capability degradation (truecolor→256→16) stays deferred until the
  capability probe exists (ADR 0012); `Theme` stores `Style` as-is.
- File loading + hot reload live in the **facade** behind a default-on
  `themes` feature using a `toml` dependency (core stays dep-free): a
  simple `[roles]` table of `role = "#rrggbb on #rrggbb, bold"` strings with a
  small documented grammar. Debug builds poll the theme file's mtime once per
  loop iteration (no file-watcher dependency); release builds load once.

## TextInput — the controlled-input decision

ADR 0001's sketch (`TextInput::new(&app.query)`) implies app-owned value, but
under the thunk model the handler cannot see the spec — so a controlled input
cannot apply an edit at event time. **Decision v1: the value is retained
state (uncontrolled).** `TextInput::new()` takes no value; `.placeholder(&str)`
optional. `State { value: String, cursor: usize /* byte offset at grapheme
boundary */, scroll: u16 }`. The app learns the value via
`Outcome::Changed(String)` on every edit and `Outcome::Submitted` on Enter
(Submitted does not clear; the app decides). Programmatic set/clear needs
commands-to-widgets, which arrives with effects (slice 6) — until then apps
that must force a value re-key the widget (new key = fresh state), documented
honestly. This is a recorded delta against the ADR 0001 sketch; fold back at
slice 6 when widget commands make controlled mode possible.

Editing (all grapheme-correct via the core width oracle): insert at cursor,
Backspace/Delete, Left/Right by grapheme, Home/End. Horizontal scroll keeps
the cursor visible in the area. Cursor painted as a reversed cell v1 (the
hardware-cursor path via facts cursor candidates is slice 5+). Consumes the
keys it uses only when focused; everything else `Handled::No`.

## SelectionList

`SelectionList::new(items: &[&str])` v1, selection **by index** with
`State { selected: usize, offset: usize }`; Up/Down move (clamped),
Home/End jump, selection kept visible by adjusting `offset` (scroll-into-view
inside the widget). Emits `Outcome::Selected(usize)` when selection moves and
`Outcome::Activated` on Enter. Renders only visible rows
(`offset..offset+height`) — virtualization by construction. The pluggable
lazy backend is declared as the seam now: a `ListSource` trait (`len()`,
`item(i) -> Cow<str>`), implemented for slices; widget is generic over it.
Durable selection by stable item key (reorder-proof) is deferred and noted —
it needs keyed items, which arrive with the catalog's data-row work.

Selected row paints `Highlight` when focused, `Muted` when not.

## examples/todo.rs

TextInput (add a todo on Enter), SelectionList of todos, status line; Tab
moves focus; `d` (app-level, when list focused… actually app-level on
unconsumed key) deletes the selected todo; `q`/Ctrl-C quits. Exercises: two
focusables of different types, outcomes driving app state, re-render from
mutated app state, theme roles end to end.

## Testing

Widget-level: grapheme editing (insert into "héllo", emoji, wide CJK cursor
moves), scroll-into-view math, list clamping, outcome emission. Snapshot
tests for both widgets themed and focused/unfocused. Integration: the todo
flow (type → Enter → appears in list → Tab → select → delete) through
TestApp::send_key. Theme file parse errors are reported, not panics.
