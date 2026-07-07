//! rabbitui — an async-first terminal user interface framework.
//!
//! rabbitui is the user-facing facade: it owns the tokio runtime integration,
//! the terminal session, and the event loop, and re-exports the runtime-free
//! building blocks from [`rabbitui_core`]. An app supplies three plain values —
//! owned `state`, an `update` that folds events into it, and a `view` that
//! declares the UI — and [`app::run`] drives the loop, restoring the terminal on
//! every exit path.
//!
//! # Five minutes to pixels
//!
//! Here is a complete, runnable counter. It holds an `i64`, folds each key into
//! it in `update`, and declares two lines of text in `view`. Save it as
//! `src/main.rs` in a binary crate that depends on `rabbitui`,
//! `rabbitui-core`, `rabbitui-widgets`, and `tokio`, then `cargo run`:
//!
//! ```no_run
//! use std::ops::ControlFlow;
//!
//! use rabbitui::app::{self, Event, Update};
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::id::key;
//! use rabbitui_core::input::Key;
//! use rabbitui_core::layout::Constraint;
//! use rabbitui_core::style::{Color, Style};
//! use rabbitui_widgets::Text;
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> rabbitui::app::Result<()> {
//!     app::run(0i64, update, view).await
//! }
//!
//! // `update` folds one event into the state, or asks to quit by returning
//! // `ControlFlow::Break`. It is synchronous — no `.await`.
//! fn update(count: &mut i64, update: Update<'_>) -> ControlFlow<()> {
//!     let Event::Input(input) = update.event() else {
//!         return ControlFlow::Continue(());
//!     };
//!     match input.as_key().map(|k| k.key) {
//!         Some(Key::Char('+' | ' ')) => *count += 1,
//!         Some(Key::Char('-')) => *count -= 1,
//!         Some(Key::Char('q') | Key::Escape) => return ControlFlow::Break(()),
//!         _ => {}
//!     }
//!     ControlFlow::Continue(())
//! }
//!
//! // `view` declares the UI for the current state into a `Frame`, every frame.
//! // It is also synchronous, and owns no state — it reads `count` and paints.
//! fn view(count: &i64, frame: &mut Frame<'_>) {
//!     let [title, value, hint] = frame.rows([
//!         Constraint::Length(1),
//!         Constraint::Length(1),
//!         Constraint::Fill(1),
//!     ]);
//!     let accent = Style::new().fg(Color::GREEN).bold();
//!     frame.widget(key("title"), title, &Text::new("Counter").style(accent));
//!     frame.widget(key("count"), value, &Text::new(&format!("count: {count}")));
//!     frame.widget(key("hint"), hint, &Text::new("+/- to count, q to quit"));
//! }
//! ```
//!
//! That is the whole contract. The rest of this page is a guided tour of what
//! each piece does and where to read more; every heading links into a module.
//!
//! # State, update, view
//!
//! rabbitui owns the loop but not your data (`docs/adr/0001-programming-model.md`).
//! Your `state` is a plain Rust value you own — an `i64` here, an enum-driven
//! async state machine in a real app. Each iteration, the loop hands one
//! [`Update`](app::Update) to your `update`, which mutates `state` and returns
//! [`ControlFlow`](std::ops::ControlFlow) — `Break` quits, `Continue` keeps
//! running. Then it calls your `view`, which declares the UI for the *current*
//! state into a [`Frame`](rabbitui_core::frame::Frame). Both are synchronous;
//! only the loop edges (reading input, writing bytes) are async. See
//! [`app`] for the loop and [`app::Update`] for the per-call context.
//!
//! # Keys and identity
//!
//! A `view` re-runs from scratch every frame, so widgets are short-lived *specs*,
//! not retained objects. What must persist across frames — focus, a text field's
//! contents, a list's scroll offset — is keyed by a stable
//! [`WidgetId`](rabbitui_core::id::WidgetId) the framework retains for you
//! (`docs/adr/0002-widget-identity.md`). You give each widget a
//! [`key`](rabbitui_core::id::key); the frame composes it with its ancestors into
//! an id-path. The same key at the same path is the same widget, frame after
//! frame. See [`rabbitui_core::id`] and the retained store,
//! [`rabbitui_core::store`].
//!
//! # Outcomes
//!
//! Interactive widgets do not mutate your state directly — a `Button` has no
//! access to your `count`. Instead a
//! widget *emits* a typed [`Outcome`](rabbitui_core::outcome::Outcome)
//! (`Activated`, `Changed`, `Submitted`, `Selected`, `Toggled`), and your
//! `update` reads it back by key path with
//! [`Update::outcome_for`](app::Update::outcome_for):
//!
//! ```no_run
//! # use std::ops::ControlFlow;
//! # use rabbitui::app::{self, Update};
//! # use rabbitui_core::frame::Frame;
//! # use rabbitui_core::id::key;
//! # use rabbitui_core::outcome::Outcome;
//! # use rabbitui_widgets::Button;
//! # async fn demo() -> rabbitui::app::Result<()> {
//! app::run(
//!     0u32,
//!     |count: &mut u32, update: Update<'_>| {
//!         if update.outcome_for(&[key("inc")]) == Some(&Outcome::Activated) {
//!             *count += 1;
//!         }
//!         ControlFlow::Continue(())
//!     },
//!     |_count: &u32, frame: &mut Frame<'_>| {
//!         frame.widget(key("inc"), frame.area(), &Button::new("+"));
//!     },
//! )
//! .await
//! # }
//! ```
//!
//! Routing runs capture → target → bubble against the previous frame's *facts*
//! (`docs/adr/0006-input-focus-events.md`); unconsumed Tab/BackTab move focus, and
//! an event no widget consumes falls through to `update` as
//! [`Event::Input`](app::Event::Input). See [`rabbitui_core::outcome`] and
//! [`rabbitui_core::routing`].
//!
//! # Theming
//!
//! Widgets reference semantic *roles* (`accent`, `surface`, `danger`, …) rather
//! than hard-coded colors (`docs/adr/0007-styling-theming.md`); a
//! [`Theme`](rabbitui_core::theme::Theme) maps each role to a concrete style, so
//! one theme swap re-skins the whole catalog. Set it on the builder form,
//! [`App`], and (in debug builds) hot-reload a TOML file:
//!
//! ```no_run
//! # use std::ops::ControlFlow;
//! # use rabbitui::App;
//! # use rabbitui::app::Update;
//! # use rabbitui_core::frame::Frame;
//! # use rabbitui_core::theme::Theme;
//! # async fn demo() -> rabbitui::app::Result<()> {
//! App::new(
//!     (),
//!     |_: &mut (), _: Update<'_>| ControlFlow::Continue(()),
//!     |_: &(), _: &mut Frame<'_>| {},
//! )
//! .theme(Theme::catppuccin_mocha())
//! .theme_file("theme.toml") // debug builds re-read this on change
//! .run()
//! .await
//! # }
//! ```
//!
//! See [`rabbitui_core::theme`] for roles and presets, and [`theme`] for the file
//! grammar.
//!
//! # Effects
//!
//! Async work is app-owned: `update` spawns a command
//! ([`Cmd`](effect::Cmd)) — a future, stream, or timer — whose results re-enter
//! the one serialized `update` as [`Event::Message`](app::Event::Message)
//! (`docs/adr/0005-runtime.md`). There is no subscription primitive; a recurring
//! timer is a command that re-arms. Grouped commands cancel their predecessor
//! (the debounced-search pattern), and a panic in an effect is contained and
//! surfaced as [`Event::EffectFailed`](app::Event::EffectFailed) rather than
//! crashing the loop. See [`effect`].
//!
//! # Inline vs alt-screen
//!
//! rabbitui renders into one of two peer [`Mode`](rabbitui_core::mode::Mode)s over
//! the *same* view (`docs/adr/0013-screen-modes.md`): the **alternate screen**
//! (the classic full-app takeover) or **inline** (a bounded live tail at the
//! bottom of the primary screen, plus an append-once commit channel into native
//! scrollback via [`Update::commit`](app::Update::commit)). Both are selectable
//! at startup ([`App::mode`]) and switchable at runtime
//! ([`Update::set_mode`](app::Update::set_mode)). See [`rabbitui_core::mode`].
//!
//! # Testing
//!
//! Every app and widget is testable headlessly, with no terminal and no async
//! runtime, through the `rabbitui-testing` crate: its `TestApp` drives the
//! *same* render and routing code the runtime uses, so a passing test exercises
//! the real loop (`docs/adr/0009-testing.md`). A second layer feeds the render
//! engines' emitted bytes through a `vt100` parser to assert on the escape-level
//! output a terminal would show.
//!
//! # Interop
//!
//! Existing ratatui widgets — `Paragraph`, `Block`, charts, third-party crates —
//! drop into a rabbitui frame through the optional `rabbitui-ratatui` bridge,
//! which paints a ratatui widget into a buffer and copies the cells across
//! (`docs/adr/0010-ratatui-interop.md`).
//!
//! # Crate map
//!
//! - [`rabbitui`](crate) — this facade: the runtime, the loop, the terminal.
//! - [`rabbitui_core`] — runtime-free foundation: ids, facts, buffer, style,
//!   layout, the widget contract.
//! - `rabbitui-widgets` — the built-in widget catalog.
//! - `rabbitui-testing` — the headless and vt100 test harnesses.
//! - `rabbitui-ratatui` — the ratatui interop bridge.
//!
//! # Low-level terminal access
//!
//! [`Terminal`] is the substrate seam under the loop. Most apps never touch it —
//! [`app::run`] owns it — but it is public for programs that draw a few styled
//! cells and restore, without the loop:
//!
//! ```no_run
//! use rabbitui::Terminal;
//! use rabbitui_core::geometry::Position;
//! use rabbitui_core::style::{Color, Style};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let mut terminal = Terminal::open().await?;
//! terminal.print_styled(Position::new(2, 1), "hello", Style::new().fg(Color::GREEN).bold()).await?;
//! terminal.flush().await?;
//! terminal.next_event().await?;
//! terminal.close().await?;
//! # Ok(())
//! # }
//! ```

pub use rabbitui_core as core;

pub mod app;
pub mod effect;
mod encode;
pub mod engine;
pub mod input;
mod terminal;
#[cfg(feature = "themes")]
pub mod theme;

pub use app::App;
pub use terminal::Terminal;
