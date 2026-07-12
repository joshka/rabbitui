---
name: rabbitui
description: >-
  Build and modify terminal UI apps with the rabbitui Rust framework. Use when adding a
  widget to a view, binding a key, theming a panel, writing a snapshot test, spawning an
  async effect (Command), or working with inline vs alt-screen mode. Covers the declared-frame
  mental model, the load-bearing invariants (consumed-guard on printable keys, never DIM,
  SGR reset before erase, Accent-vs-Highlight roles), idiomatic snippets, and the traps that
  bit real builds.
---

# rabbitui

rabbitui is a Rust TUI framework built on the **declared-frame** model: the app owns plain
state, a `view` function declares widgets by key each frame, input routes through the
previous frame's facts, widgets return typed **outcomes**, and effects are app-owned async
`Command`s. There is no retained widget tree and no `Rc<RefCell>` web — identity, focus, and
scroll live in a framework-owned per-id store, addressed by `WidgetId`.

This skill grounds every snippet in the current API (crates `rabbitui`, `rabbitui-core`,
`rabbitui-widgets`, `rabbitui-testing`) and the examples under `rabbitui/examples/`. When in
doubt, read an example — they are the executable spec.

## Mental model (read this first)

- **Declared frame.** Each frame you call `view(&State, &mut Frame)` and declare every widget
  fresh. Widgets are short-lived _specs_, not objects you hold. You never mutate a widget; you
  read its **outcome** next update and mutate your own state.
- **Identity by key.** `frame.widget(key("input"), area, &widget)` gives the widget a stable
  `WidgetId` (`key("input")` composed under the frame root, or under a layer). The framework
  keeps that widget's retained state (cursor, scroll, focus, collapsed) across frames, keyed by
  identity. Keep a widget's key **stable** across frames or its state resets.
- **Outcomes, not callbacks.** Interactive widgets emit typed `Outcome`s
  (`Changed(String)`, `Submitted`, `Activated`, `Selected(usize)`, `Toggled(bool)`,
  `Dismissed`). The app reads them in `update` via `update.outcome_for(&[key("input")])` and
  folds them into its own state. No widget ever holds `&mut App`.
- **Commands (`Command`) are the only effect primitive.** Async work is `Command::future` /
  `Command::stream`, spawned via `update.spawn(cmd)`; results re-enter the loop as
  `Event::Message(M)`. There are no subscriptions — a subscription is just a long stream you
  chose to start.
- **Two screen modes.** `Mode::AltScreen` (default, full-screen buffer) and
  `Mode::inline(tail_height)` (a bounded live tail at the bottom of the primary screen, with
  finalized lines committed once into native scrollback via `update.commit(...)`). Both are
  peer modes over the same `view`.

## The app skeleton

The idiom is a type implementing `trait App`: your struct **is** the state, `update` folds
events into it, `view` declares its UI. Defaulted hooks cover the lifecycle — `init` (the
opening `Command`, spawned before `Event::Started`), `global` (runs before `update` for every
event; the home for app-wide chords), and `config` (launch-only settings: theme, mode, mouse,
tracing).

```rust
use std::ops::ControlFlow;
use rabbitui::app::{App, Config, Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;

#[derive(Default)]
struct Counter {
    count: i64,
}

impl App for Counter {
    fn update(&mut self, update: Update<'_>) -> ControlFlow<()> {
        let Event::Input(input) = update.event() else {
            return ControlFlow::Continue(());
        };
        match input.as_key().map(|k| k.key) {
            Some(Key::Char('+' | ' ')) => self.count += 1,
            Some(Key::Char('-')) => self.count -= 1,
            Some(Key::Char('q') | Key::Escape) => return ControlFlow::Break(()),
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn view(&self, frame: &mut Frame<'_>) { /* declare widgets — see below */ }

    // Optional: launch config (drop this to take the defaults).
    fn config(&self) -> Config {
        Config::new()
            .theme(rabbitui_core::theme::Theme::catppuccin_mocha())
            .mode(rabbitui_core::mode::Mode::inline(3)) // default is AltScreen
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Counter::default().run().await?;
    Ok(())
}
```

An app with a message type is `impl App<Msg> for X`; its `fn init(&mut self) -> Command<Msg>`
returns the opening command (a stream to start, a load to kick off) instead of waiting for the
first event.

For tests, demos, and one-screen tools, the closure shorthand skips the trait: free
`app::run(state, update, view)`, or `rabbitui::from_fn(state, update, view)` with
`.with_theme(...)` / `.with_mode(...)` builders when it needs config (both need
`use rabbitui::App;` in scope only for a `.run()` call on `from_fn`'s result).

`update` returns `ControlFlow<()>`: `Break(())` quits the loop, `Continue(())` keeps running.
`update.event()` returns `&Event<M>` (a reference — match by ref): `Event::Input(InputEvent)`,
`Event::Resize(Size)`, `Event::Message(M)`, `Event::EffectFailed(EffectError)`.

## The ten commonest tasks

### 1. Add a widget to a view

Declare it into an area with a stable key. Layout an area first (see task 6). Widgets live in
`rabbitui_widgets`: `Text`, `Button`, `TextInput`, `SelectionList`, `Collapsible`, `Panel`,
`ErrorBanner`, `LogOverlay`.

```rust
use rabbitui_widgets::{Panel, Text};
use rabbitui_core::theme::Role;

fn view(state: &MyState, frame: &mut Frame<'_>) {
    let area = rabbitui_core::layout::center(frame.area(), 44, 7);
    let panel = Panel::new().title("counter").padding(1);
    frame.widget(key("panel"), area, &panel);            // backdrop first

    let inner = Panel::inner(area, &panel);              // then content into inner
    frame.widget(key("label"), inner, &Text::new("hello").role(Role::Accent));
}
```

`Panel` is a _backdrop_, not a container: declare the panel, then declare content into
`Panel::inner(area, &panel)`. Widgets do not nest yet.

### 2. Add a key binding

App-level bindings live in `update`, matching on the key of an `Event::Input`. **Printable
keys must guard on `!update.consumed()`** so a focused text field's keystrokes do not double as
app commands (this is the single most common bug — see Traps).

```rust
if let Event::Input(input) = update.event()
    && !update.consumed()                                // guard printables
{
    match input.as_key().map(|k| k.key) {
        Some(Key::Char('d')) if !state.items.is_empty() => state.delete_selected(),
        Some(Key::Char('q') | Key::Escape) => return ControlFlow::Break(()),
        _ => {}
    }
}
```

Ctrl-chords do **not** need the guard — text inputs pass Ctrl chords through to the app by
contract, so `Ctrl-L`, `Ctrl-C` etc. fire even while a field is focused:

```rust
if let Some(k) = input.as_key() {
    if k.key == Key::Char('l') && k.modifiers.ctrl { /* clear, etc. */ }
}
```

### 3. Handle a widget outcome

Read outcomes by the widget's key path. Track `Changed` to mirror a value, act on `Submitted`
/ `Activated` / `Selected`.

```rust
use rabbitui_core::outcome::Outcome;

if let Some(Outcome::Changed(value)) = update.outcome_for(&[key("input")]) {
    state.draft = value.clone();
}
if update.outcome_for(&[key("input")]) == Some(&Outcome::Submitted) {
    state.items.push(std::mem::take(&mut state.draft));
    update.widget::<TextInput>(&[key("input")], |s| s.clear());   // controlled clear
}
if let Some(Outcome::Selected(index)) = update.outcome_for(&[key("list")]) {
    state.selected = *index;
}
```

Outcome vocabulary (`rabbitui_core::outcome::Outcome`): `Activated`, `Changed(String)`,
`Submitted`, `Toggled(bool)`, `Selected(usize)`, `Dismissed`.

### 4. Add a text input and a list

`TextInput` is _uncontrolled_: its value is retained state, not app state. Learn the value from
`Changed`/`Submitted`; force a value with a widget command (task 8), never by re-keying.

```rust
use rabbitui_widgets::{SelectionList, TextInput};

frame.widget(key("input"), input_row, &TextInput::new().placeholder("Add a todo…"));
frame.widget(key("list"), list_area, &SelectionList::new(state.todos.clone()));
```

`SelectionList::new` takes anything `Into<ListSource>` (a `Vec<String>` or `&[&str]` work); it
selects **by index**, emits `Selected(usize)` on move and `Activated` on Enter, and virtualizes
(renders only visible rows). A selected row paints `Highlight` when focused, `Muted` when not.

### 5. Theme a panel / widget

Widgets reference semantic **roles**, never raw colors. Set the theme once on the builder; the
whole catalog re-skins. Roles (`rabbitui_core::theme::Role`): `Surface`, `Text`, `Muted`,
`Accent`, `Success`, `Warning`, `Danger`, `Border`, `Highlight`.

```rust
frame.widget(key("title"), row, &Text::new("Ready").role(Role::Success));
let panel = Panel::new().title("form").padding(1).focused(true);   // focused border = Accent
```

Role semantics that matter (see Traps for the invariants):

- **`Accent`** is a **foreground-only** hue — use it for a focused border, a title, an emphasis
  color. It carries no background.
- **`Highlight`** carries a **background** (reverse-style) — use it for the _selected_ row or a
  _focused_ button, where the element should read as a filled block.
- **`Muted`** is `Ansi(8)` alone — recessive text (hints, unfocused rows). It is **never** the
  DIM attribute (DIM is banned framework-wide) and never used for selection styling.

Presets ship as theme data: `Theme::default()` (dark), `Theme::catppuccin_mocha()`,
`Theme::nord()`, `Theme::dracula()`. TOML theme files hot-reload in debug builds via
`Config::theme_file(path)` in your app's `fn config`.

### 6. Lay out an area

Layout is intrinsic constraint/flex, no solver. Split an area into rows (or use `frame.rows`
for the whole frame), center a box, get the panel's inner area.

```rust
use rabbitui_core::layout::{Constraint, center, split_rows};

let [header, body, footer] = split_rows(inner, [
    Constraint::Length(1),   // fixed 1 row
    Constraint::Fill(1),     // take the rest
    Constraint::Length(1),
]);
// Whole-frame convenience (used in inline mode):
let [input_row, status_row, hint_row] = frame.rows([
    Constraint::Length(1), Constraint::Length(1), Constraint::Fill(1),
]);
let boxed = center(frame.area(), 60, 16);   // width, height
```

`Constraint` variants in common use: `Length(u16)`, `Fill(u16)`. `split_rows` returns a
fixed-size array you destructure.

### 7. Manage focus

Focus is framework state keyed by `WidgetId`. Tab / Shift-Tab traverse focusables in
declaration order; the framework auto-focuses the first focusable when nothing is focused (so
apps are usable without pressing Tab first). Command focus by key from the app, and query it:

```rust
update.focus(&[key("email")]);                 // focus a widget by key path
let focused = update.is_focused(&[key("name")]); // focus-dependent decisions (e.g. arrow nav)
```

Arrow-key field navigation, verbatim shape from `examples/form.rs`:

```rust
let order = ["name", "email", "notes"];
let at = order.iter().position(|n| update.is_focused(&[key(n)]));
if let (Some(step), Some(at)) = (step, at) {        // step = -1 (Up) or +1 (Down)
    let next = (at as i32 + step).rem_euclid(order.len() as i32) as usize;
    update.focus(&[key(order[next])]);
}
```

### 8. Command a widget's state (controlled operations)

Because a widget is a spec that dies after render, you cannot mutate it directly. Between
frames, apply a typed command that downcasts to the widget's state:

```rust
update.widget::<TextInput>(&[key("input")], |state| state.clear());
```

This is how you clear an input on submit, set a value, or move a list's selection
programmatically. Commanding a widget that was never declared trips a `debug_assert` (an app
bug). This replaces any "re-key to reset" workaround — keep the key stable.

### 9. Spawn an async effect (`Command`)

Define a message type `M`, spawn a `Command<M>` with `update.spawn`, handle results as
`Event::Message(M)`. `App<M>` (`impl App<Msg> for X`) and `Update<'_, M>` carry the type;
message-less apps use the `()` default. An app's _opening_ command belongs in
`fn init(&mut self) -> Command<M>` rather than a first-event workaround.

```rust
use rabbitui::effect::Command;
use std::time::Duration;

#[derive(Debug, Clone)]
enum Msg { Results { query: String, rows: Vec<String> }, Tick(u64) }

// Spawn a one-shot future, debounced by a group (cancel-previous):
if let Some(Outcome::Changed(value)) = update.outcome_for(&[key("input")]) {
    let query = value.clone();
    update.spawn(fake_fetch(query).group("search"));   // newer keystroke aborts the old fetch
}

// Handle the result:
match update.event() {
    Event::Message(Msg::Results { query, rows }) => state.results = rows.clone(),
    Event::EffectFailed(error) => state.last_error = Some(error.to_string()),
    _ => {}
}

fn fake_fetch(query: String) -> Command<Msg> {
    Command::future(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Msg::Results { query, rows: vec![] }
    })
}
```

Effect constructors: `Command::future(async move -> M)` (one message), `Command::stream(impl
Stream<Item = M>)` (many; a "subscription"), `Command::timeout(Duration, FnOnce -> M)`,
`Command::cancel_group(&str)` (stop a group's stream for good; may need a turbofish where the
message type cannot be inferred, e.g. `Command::<Msg>::cancel_group("agent")`), and
`Command::none()` (the no-op — `init`'s default). `.group(&str)` makes a command
cancel-previous within that group. Effect panics are contained and surface as
`Event::EffectFailed` — they never crash the loop. Expected failures should be _values_ (a
`Msg` variant), not panics.

### 10. Write a snapshot test

Use the headless `TestApp` from `rabbitui-testing` — it runs the _same_ store/frame/routing
path as the runtime, with no tty or async. Drive input with `send_key`/`send_event`/`send_mouse`,
assert on the buffer with `assert_buffer_lines`, or snapshot a themed render with
`assert_snapshot!`.

```rust
use rabbitui_core::geometry::Size;
use rabbitui_core::id::{WidgetId, key};
use rabbitui_core::input::Key;
use rabbitui_core::outcome::Outcome;
use rabbitui_testing::TestApp;
use rabbitui_widgets::Button;

#[test]
fn button_activates_on_enter() {
    let mut app = TestApp::new(Size::new(4, 1), ());
    let view = |_s: &(), f: &mut rabbitui_core::frame::Frame<'_>| {
        f.widget(key("ok"), f.area(), &Button::new("OK"));
    };
    app.render(view);
    app.set_focus(Some(WidgetId::ROOT.child(key("ok"))));  // arrange focus
    app.render(view);
    let result = app.send_key(Key::Enter);                 // routes through the real router
    assert!(result.consumed);
    assert_eq!(result.outcomes[0].1, Outcome::Activated);
}
```

Notes: `send_key`/`send_event` do **not** re-render — call `render` again to observe the
resulting frame. Use `.with_theme(Theme::catppuccin_mocha())` for themed snapshots. `inject`
folds a message the way an effect result arrives; `apply_pending` runs a widget command between
frames. Regenerate golden snapshots with `UPDATE_SNAPSHOTS=1 cargo test` and review the diff.

## Invariants (each was a shipped bug — do not violate)

1. **Guard printable app bindings on `!update.consumed()`.** `update` runs for every event even
   when a widget consumed it (so outcomes can ride along), so an unguarded `d` binding fires
   while the user types "feed" into a focused input. Outcomes need no guard; Ctrl-chords need no
   guard (inputs pass them through).
2. **Never use the DIM attribute.** De-emphasis is `Role::Muted` (`Ansi(8)`) alone. DIM is
   banned framework-wide (poor terminal support, unreadable on many themes).
3. **SGR reset before every erase/clear.** Any erase inherits the current graphic rendition; an
   erase issued under a non-default background floods the vacated cells (background-color-erase
   / "BCE bleed"). The renderer emits `CSI 0 m` before every erase. If you write raw escapes,
   preserve this.
4. **Accent vs Highlight are different roles.** `Accent` is fg-only (focused _borders_, titles,
   emphasis). `Highlight` carries a bg (the _selected_ row, a _focused_ button). Do not use
   `Highlight` for a border (it paints a bg band) or `Accent` for a selected row (no fill).
   Selection styling never uses `Muted`.

## Traps (drawn from real build logs)

- **Unguarded printable keys.** The classic: `examples/todo.rs`'s `d`-to-delete deleted a todo
  while the user typed a word containing "d". Fix: `&& !update.consumed()`. (See invariant 1.)
- **Missing initial focus.** Early examples were unusable until you pressed Tab, because focus
  started as `None`. The framework now auto-focuses the first focusable — but if you build a
  custom focusable, make sure it declares `ctx.focusable(true)` so it enters the chain.
- **Single-focusable app + printable bindings never fire.** If the only focusable is a
  `TextInput`, auto-focus means it is _always_ focused, so any `!consumed()`-guarded printable
  binding can never fire (the input ate the key). The pattern for such apps is **Ctrl-chords**
  (which inputs pass through), as in `examples/stream.rs` and `examples/fetch.rs`. Do not remove
  the guard to "fix" this — you would break typing.
- **BCE bleed on erase.** A styled (non-default background) region that shrinks and erases below
  will paint a colored band into the vacated rows unless an SGR reset precedes the erase. This
  is handled by the engine; only a concern if you emit escapes directly. (Invariant 3.)
- **Top-anchored vs bottom-anchored overflow (inline mode).** In inline mode the live tail is
  _bottom-anchored_ against the shell prompt and **bounded** to `tail_height` rows; committed
  content flows up into scrollback. A streaming message taller than the tail scrolls its own top
  out of view before it commits (whole-message commit is on completion). Do not assume unbounded
  tail height — size your tail and commit finished lines with `update.commit(...)`. Also: do not
  wrap the inline tail in a bordered `Panel` — the committed scrollback above belongs to the
  terminal and a border sharing its top edge with native history reads wrong (style inside the
  tail with roles instead).
- **Re-keying to reset a widget.** Tempting but wrong now: changing a widget's key to clear it
  churns its identity (and focus). Use a widget command (`update.widget::<W>(path, |s| ...)`)
  instead and keep the key stable.
- **Substrate key gaps.** The terminal substrate currently decodes text, C0 controls, and the
  four arrows only. `Shift-Tab`/`BackTab`, `Home`/`End`, `PageUp`/`PageDown`, a forward
  `Delete`, and non-`ctrl` modifiers are defined in `Key` but may not arrive from the terminal
  yet. Forward `Tab` wraps, so all focusables stay reachable; do not rely on `BackTab` in an
  example's control scheme. Ctrl-letter chords _do_ arrive (as `Key::Char` + `ctrl`).

## Where to look

- `rabbitui/examples/` is the executable spec: `hello.rs`/`counter.rs` (skeleton),
  `todo.rs` (input + list + consumed-guard), `form.rs` (focus, arrow-nav, modal layer, custom
  widget), `stream.rs` (inline mode + commit), `fetch.rs` (`Command` future/stream, groups, widget
  command, effect failure), `agent.rs` (the flagship: streaming transcript, both modes).
- ADRs under `docs/adr/`: 0001 (declared frame), 0006 (input/focus), 0007 (theming), 0008
  (widget contract), 0013 (screen modes). Read the relevant ADR before changing behavior — they
  carry the rationale and the revisit triggers.
- Custom widgets implement `rabbitui_core::widget::Widget` (one trait: `type State`, `render`,
  optional `handle`). See the `SubmitButton` in `examples/form.rs` for a complete verbatim
  example (focusability gating, mouse + key handling, emitting an outcome).
