//! Headless test harness for rabbitui applications.
//!
//! Per `docs/adr/0009-testing.md`, interaction correctness is only provable by
//! driving the app and inspecting output, so the test kit ships *before* the
//! widget catalog grows and is public, semver-stable API — third-party widget
//! authors and coding agents verify their own output against the same contract
//! the core uses. This crate depends only on [`rabbitui_core`]; it never touches
//! tokio or a terminal, so tests run deterministically with no I/O.
//!
//! Slice 2 landed the first two layers of the three in ADR 0009: a headless
//! [`TestApp`] driver plus buffer-snapshot assertions with an update flag. Slice
//! 5 adds the third — the [`vt`] escape-level harness, a real [`vt100`] terminal
//! model the render engines' emitted bytes are fed through, so tests assert on
//! the *screen a terminal would show*. That is the layer that catches
//! synchronized-output framing, clears, cursor discipline, and inline
//! commit/tail interleaving that buffer equality cannot see.
//!
//! # The driver mirrors the real loop
//!
//! [`TestApp`] runs the *same* [`StateStore`]/[`Frame`] path as the runtime's
//! `run` loop, minus the async edges: it owns a [`StateStore`] and a back
//! buffer across frames, and each [`TestApp::render`] clears the buffer to
//! blank, brackets the view call in [`StateStore::begin_frame`] /
//! [`StateStore::end_frame`], and constructs a [`Frame`] over the two. Because
//! rabbitui owns the loop (`docs/adr/0005-runtime.md`), this single-stepping is
//! possible at all — ratatui cannot ship a real headless driver for exactly
//! this reason.
//!
//! # Input goes through the *same* routing as the runtime
//!
//! From slice 3 the driver also owns a [`Focus`] and keeps the last rendered
//! frame's facts and handlers, so [`TestApp::send_key`] / [`TestApp::send_event`]
//! route an event through the shared [`route`] function — the exact code path
//! `rabbitui::app::run` uses. Extracting routing into core is what makes the
//! harness and the runtime provably identical (ADR 0006's "cannot drift"
//! requirement); a test that passes here is testing the real router.
//!
//! [`Focus`]: rabbitui_core::routing::Focus
//! [`route`]: rabbitui_core::routing::route
//!
//! [`StateStore`]: rabbitui_core::store::StateStore
//! [`Frame`]: rabbitui_core::frame::Frame
//!
//! # Examples
//!
//! Drive a counter view through a render / mutate / re-render cycle:
//!
//! ```
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::geometry::Size;
//! use rabbitui_core::id::key;
//! use rabbitui_core::style::Style;
//! use rabbitui_core::widget::{RenderCtx, Widget};
//! use rabbitui_testing::TestApp;
//!
//! struct Label<'a>(&'a str);
//! impl Widget for Label<'_> {
//!     type State = ();
//!     fn render(&self, _s: &mut (), ctx: &mut RenderCtx<'_>) {
//!         use rabbitui_core::geometry::Position;
//!         ctx.set_string(Position::ORIGIN, self.0, Style::new());
//!     }
//! }
//!
//! fn view(count: &u32, frame: &mut Frame<'_>) {
//!     let text = count.to_string();
//!     frame.widget(key("count"), frame.area(), &Label(&text));
//! }
//!
//! let mut app = TestApp::new(Size::new(3, 1), 0u32);
//! app.render(view);
//! app.assert_buffer_lines(&["0"]);
//!
//! // Mutate state as an update would, re-render, and observe the new frame.
//! app.send(|count| *count += 1, view);
//! app.assert_buffer_lines(&["1"]);
//! ```

pub mod snapshot;
pub mod vt;

pub use snapshot::assert_snapshot;
pub use vt::VtScreen;

use rabbitui_core::buffer::Buffer;
use rabbitui_core::facts::FrameFacts;
use rabbitui_core::frame::{Frame, HandlerMap};
use rabbitui_core::geometry::{Position, Size};
use rabbitui_core::id::WidgetId;
use rabbitui_core::input::{InputEvent, Key, MouseButton, MouseEvent, MouseKind};
use rabbitui_core::pending::Pending;
use rabbitui_core::routing::{Focus, RouteResult, route};
use rabbitui_core::store::StateStore;
use rabbitui_core::theme::Theme;

/// A headless driver for a rabbitui app: state, a state store, and a back
/// buffer, single-stepped without a terminal or async runtime.
///
/// `S` is the app's owned state — the same plain value the real `run` loop
/// folds events into. `TestApp` holds no `update` or `view` of its own; each is
/// supplied per call, so one driver can exercise different views against the
/// same persisted state (and thus the same [`StateStore`], the point of the
/// harness).
///
/// [`StateStore`]: rabbitui_core::store::StateStore
///
/// # Examples
///
/// ```
/// use rabbitui_core::geometry::Size;
/// use rabbitui_testing::TestApp;
///
/// let app = TestApp::new(Size::new(20, 3), "state");
/// assert_eq!(app.buffer().size(), Size::new(20, 3));
/// assert_eq!(app.state(), &"state");
/// ```
#[derive(Debug)]
pub struct TestApp<S> {
    state: S,
    store: StateStore,
    buffer: Buffer,
    focus: Focus,
    /// The active theme, threaded into every rendered frame just as the runtime
    /// does (ADR 0007), so themed snapshots are exact.
    theme: Theme,
    /// The most recently rendered frame's facts — routing targets these.
    facts: FrameFacts,
    /// The most recently rendered frame's handler thunks.
    handlers: HandlerMap,
}

impl<S> TestApp<S> {
    /// Creates a driver for `state` at a fixed terminal `size`.
    ///
    /// The buffer starts blank (all default cells) and the state store empty,
    /// exactly as the runtime starts. Nothing is rendered until [`render`] or
    /// [`send`] is called.
    ///
    /// [`render`]: Self::render
    /// [`send`]: Self::send
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::geometry::Size;
    /// use rabbitui_testing::TestApp;
    ///
    /// let app = TestApp::new(Size::new(10, 2), 0u32);
    /// assert!(app.store_len() == 0);
    /// ```
    #[must_use]
    pub fn new(size: Size, state: S) -> Self {
        Self {
            state,
            store: StateStore::new(),
            buffer: Buffer::new(size),
            focus: Focus::new(),
            theme: Theme::default(),
            facts: FrameFacts::new(),
            handlers: HandlerMap::new(),
        }
    }

    /// Sets the theme threaded into every rendered frame, and returns `self`.
    ///
    /// The harness defaults to [`Theme::default`], matching the runtime with no
    /// theme configured. Set a preset (or a loaded theme) to snapshot a widget's
    /// themed appearance — the frames render exactly as [`App::theme`] would make
    /// them.
    ///
    /// [`App::theme`]: https://docs.rs/rabbitui/latest/rabbitui/struct.App.html
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::geometry::Size;
    /// use rabbitui_core::theme::Theme;
    /// use rabbitui_testing::TestApp;
    ///
    /// let app = TestApp::new(Size::new(10, 1), ()).with_theme(Theme::catppuccin_mocha());
    /// let _ = app;
    /// ```
    #[must_use]
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// A mutable handle to the app's state, to set up a scenario directly.
    ///
    /// Prefer [`send`](Self::send) to model an update; this is for arranging
    /// preconditions a test needs before the first render.
    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    /// The framework's current focus target, if any.
    ///
    /// Reflects traversal driven by [`send_key`](Self::send_key) and
    /// reconciliation after each render — the same [`Focus`] the runtime keeps.
    #[must_use]
    pub fn focus(&self) -> Option<WidgetId> {
        self.focus.current()
    }

    /// The app's current state.
    #[must_use]
    pub fn state(&self) -> &S {
        &self.state
    }

    /// The back buffer holding the most recently rendered frame.
    #[must_use]
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// The number of widgets currently holding retained state in the store.
    ///
    /// A probe for state-store lifecycle tests: it reports how many identities
    /// the store is keeping alive across frames.
    #[must_use]
    pub fn store_len(&self) -> usize {
        self.store.len()
    }

    /// Renders one frame from the current state through `view`.
    ///
    /// This runs the runtime's per-frame path exactly: the buffer is cleared to
    /// blank (widgets declare everything each frame), the view call is bracketed
    /// in [`StateStore::begin_frame`] / [`StateStore::end_frame`], and a
    /// [`Frame`] is constructed over the buffer and store so identity-keyed
    /// state persists across calls.
    ///
    /// [`StateStore::begin_frame`]: rabbitui_core::store::StateStore::begin_frame
    /// [`StateStore::end_frame`]: rabbitui_core::store::StateStore::end_frame
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::id::key;
    /// use rabbitui_core::style::Style;
    /// use rabbitui_core::widget::{RenderCtx, Widget};
    /// use rabbitui_testing::TestApp;
    ///
    /// struct Dot;
    /// impl Widget for Dot {
    ///     type State = ();
    ///     fn render(&self, _s: &mut (), ctx: &mut RenderCtx<'_>) {
    ///         ctx.set_string(Position::ORIGIN, "x", Style::new());
    ///     }
    /// }
    ///
    /// let mut app = TestApp::new(Size::new(1, 1), ());
    /// app.render(|_state, frame| frame.widget(key("dot"), frame.area(), &Dot));
    /// app.assert_buffer_lines(&["x"]);
    /// ```
    pub fn render(&mut self, view: impl FnOnce(&S, &mut Frame<'_>)) {
        clear(&mut self.buffer);
        self.store.begin_frame();
        let (facts, handlers) = {
            let mut frame = Frame::themed(
                &mut self.buffer,
                &mut self.store,
                self.focus.current(),
                &self.theme,
            );
            view(&self.state, &mut frame);
            frame.into_parts()
        };
        self.store.end_frame();
        // Keep this frame's facts/handlers for routing, then reconcile focus so
        // traversal and dispatch run against current facts — exactly as the
        // runtime does after each paint.
        self.facts = facts;
        self.handlers = handlers;
        self.focus.reconcile(&self.facts);
    }

    /// Folds an update into the state, then renders one frame through `view`.
    ///
    /// This is the driver's step primitive: `update` mutates the state the way
    /// an app's `update` folds an event into it, and the frame that follows is
    /// rendered from the new state. Injecting a specific event is done by
    /// closing over it in `update` (the app decides how events map to state);
    /// the harness stays event-type-agnostic so it can drive any app.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::id::key;
    /// use rabbitui_core::style::Style;
    /// use rabbitui_core::widget::{RenderCtx, Widget};
    /// use rabbitui_testing::TestApp;
    ///
    /// struct Label<'a>(&'a str);
    /// impl Widget for Label<'_> {
    ///     type State = ();
    ///     fn render(&self, _s: &mut (), ctx: &mut RenderCtx<'_>) {
    ///         ctx.set_string(Position::ORIGIN, self.0, Style::new());
    ///     }
    /// }
    ///
    /// fn view(count: &u32, frame: &mut Frame<'_>) {
    ///     let text = count.to_string();
    ///     frame.widget(key("n"), frame.area(), &Label(&text));
    /// }
    ///
    /// let mut app = TestApp::new(Size::new(2, 1), 0u32);
    /// app.send(|count| *count += 2, view);
    /// app.assert_buffer_lines(&["2"]);
    /// ```
    pub fn send(
        &mut self,
        update: impl FnOnce(&mut S),
        view: impl FnOnce(&S, &mut Frame<'_>),
    ) {
        update(&mut self.state);
        self.render(view);
    }

    /// Sets the focused widget directly, to arrange a routing scenario.
    ///
    /// The runtime never exposes this — focus is framework state — but a test
    /// often needs to start from "button B is focused" without pressing Tab
    /// first. A later [`render`](Self::render) reconciles this against the facts,
    /// so the id must be focusable in the rendered frame to survive.
    pub fn set_focus(&mut self, id: Option<WidgetId>) {
        self.focus.set(id);
    }

    /// Routes one input event through the last rendered frame, returning the
    /// routing result (emitted outcomes and whether the event was consumed).
    ///
    /// This is the harness's input primitive: it runs the shared [`route`]
    /// function against the facts and handlers captured by the most recent
    /// [`render`](Self::render) — the exact path `rabbitui::app::run` takes — so
    /// a test drives the real router. It does **not** re-render; call
    /// [`render`](Self::render) afterward to observe the resulting frame (focus
    /// styles, updated status lines), and fold any returned outcomes into state
    /// the way the app's `update` would.
    ///
    /// [`route`]: rabbitui_core::routing::route
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::geometry::Size;
    /// use rabbitui_core::id::{WidgetId, key};
    /// use rabbitui_core::input::Key;
    /// use rabbitui_core::outcome::Outcome;
    /// use rabbitui_testing::TestApp;
    /// use rabbitui_widgets::Button;
    ///
    /// let mut app = TestApp::new(Size::new(4, 1), ());
    /// app.render(|_s, frame| frame.widget(key("ok"), frame.area(), &Button::new("OK")));
    /// app.set_focus(Some(WidgetId::ROOT.child(key("ok"))));
    /// app.render(|_s, frame| frame.widget(key("ok"), frame.area(), &Button::new("OK")));
    ///
    /// let result = app.send_key(Key::Enter);
    /// assert!(result.consumed);
    /// assert_eq!(result.outcomes[0].1, Outcome::Activated);
    /// ```
    pub fn send_event(&mut self, event: InputEvent) -> RouteResult {
        route(&self.facts, &self.handlers, &mut self.focus, &mut self.store, &event)
    }

    /// Routes a bare key press (no modifiers) through the last rendered frame.
    ///
    /// A convenience over [`send_event`](Self::send_event) for the common case;
    /// see that method for the routing contract and re-render note.
    pub fn send_key(&mut self, key: Key) -> RouteResult {
        self.send_event(InputEvent::key(key))
    }

    /// Routes a left-button mouse event of `kind` at `position` through the last
    /// rendered frame (slice-7 mouse routing).
    ///
    /// A convenience over [`send_event`](Self::send_event) for the common pointer
    /// case: it hit-tests `position` against the last frame's facts (layer-aware),
    /// dispatches capture → target → bubble, and applies click-to-focus for an
    /// unconsumed press — the exact path `rabbitui::app::run` takes. Like
    /// [`send_key`](Self::send_key), it does not re-render; call
    /// [`render`](Self::render) afterward to observe the resulting frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::id::{WidgetId, key};
    /// use rabbitui_core::input::MouseKind;
    /// use rabbitui_core::outcome::Outcome;
    /// use rabbitui_testing::TestApp;
    /// use rabbitui_widgets::Button;
    ///
    /// let mut app = TestApp::new(Size::new(4, 1), ());
    /// app.render(|_s, frame| frame.widget(key("ok"), frame.area(), &Button::new("OK")));
    /// // A left click over the button activates it.
    /// let result = app.send_mouse(MouseKind::Down, Position::ORIGIN);
    /// assert_eq!(result.outcomes[0].1, Outcome::Activated);
    /// ```
    pub fn send_mouse(&mut self, kind: MouseKind, position: Position) -> RouteResult {
        let button = match kind {
            MouseKind::Scroll(_) => MouseButton::None,
            _ => MouseButton::Left,
        };
        self.send_event(InputEvent::Mouse(MouseEvent::new(kind, button, position)))
    }

    /// Injects a message the way the runtime delivers an effect result, folding it
    /// into state and re-rendering.
    ///
    /// The runtime turns an effect's [`Event::Message`] into a `update` call; a
    /// headless test has no async runtime, so it drives the message-fold directly:
    /// `update` mutates the state as the app's `update` would on
    /// `Event::Message(m)`, and the frame that follows renders the new state. This
    /// is the `send_message`-equivalent the effects slice needs — the same shape
    /// as [`send`](Self::send), named for injecting effect results rather than
    /// arranging preconditions.
    ///
    /// [`Event::Message`]: https://docs.rs/rabbitui/latest/rabbitui/app/enum.Event.html
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::id::key;
    /// use rabbitui_core::style::Style;
    /// use rabbitui_core::widget::{RenderCtx, Widget};
    /// use rabbitui_testing::TestApp;
    ///
    /// struct Label<'a>(&'a str);
    /// impl Widget for Label<'_> {
    ///     type State = ();
    ///     fn render(&self, _s: &mut (), ctx: &mut RenderCtx<'_>) {
    ///         ctx.set_string(Position::ORIGIN, self.0, Style::new());
    ///     }
    /// }
    ///
    /// fn view(results: &Vec<String>, frame: &mut Frame<'_>) {
    ///     let text = results.join(",");
    ///     frame.widget(key("out"), frame.area(), &Label(&text));
    /// }
    ///
    /// // A fetch "message" arrives and the app folds it into state.
    /// let mut app = TestApp::new(Size::new(8, 1), Vec::<String>::new());
    /// app.inject(|results| results.push("hit".to_string()), view);
    /// app.assert_buffer_lines(&["hit"]);
    /// ```
    pub fn inject(
        &mut self,
        update: impl FnOnce(&mut S),
        view: impl FnOnce(&S, &mut Frame<'_>),
    ) {
        self.send(update, view);
    }

    /// Records between-frames widget commands and a focus request, applies them
    /// against the last rendered frame through the shared [`Pending::apply`], then
    /// re-renders through `view`.
    ///
    /// This is the *same* [`core::pending`](rabbitui_core::pending) apply the
    /// runtime runs — the harness and runtime cannot drift by construction (the
    /// ADR 0006 requirement extended to focus and widget commands). `build`
    /// records commands (`p.command::<W>(id, f)`) and at most one focus request
    /// (`p.request_focus(id)`) into a fresh [`Pending`]; they apply against the
    /// facts and store from the most recent [`render`](Self::render) (commanding a
    /// widget that was never declared, or focusing a non-focusable one, trips a
    /// `debug_assert`, exactly as in the runtime), then the frame re-renders so the
    /// mutation is visible.
    ///
    /// [`Pending::apply`]: rabbitui_core::pending::Pending::apply
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::id::{WidgetId, key};
    /// use rabbitui_core::style::Style;
    /// use rabbitui_core::widget::{RenderCtx, Widget};
    /// use rabbitui_testing::TestApp;
    ///
    /// #[derive(Default)]
    /// struct Field {
    ///     value: String,
    /// }
    /// struct Input;
    /// impl Widget for Input {
    ///     type State = Field;
    ///     fn render(&self, state: &mut Field, ctx: &mut RenderCtx<'_>) {
    ///         ctx.set_string(Position::ORIGIN, &state.value, Style::new());
    ///     }
    /// }
    ///
    /// fn view(_s: &(), frame: &mut Frame<'_>) {
    ///     frame.widget(key("field"), frame.area(), &Input);
    /// }
    ///
    /// let mut app = TestApp::new(Size::new(8, 1), ());
    /// app.render(view);
    /// // Force the field's value via a widget command, applied between frames.
    /// let id = WidgetId::ROOT.child(key("field"));
    /// app.apply_pending(|p| p.command::<Input>(id, |s| s.value = "hi".into()), view);
    /// app.assert_buffer_lines(&["hi"]);
    /// ```
    pub fn apply_pending(
        &mut self,
        build: impl FnOnce(&mut Pending),
        view: impl FnOnce(&S, &mut Frame<'_>),
    ) {
        let mut pending = Pending::new();
        build(&mut pending);
        pending.apply(&mut self.store, &self.facts, &mut self.focus);
        self.render(view);
    }

    /// The rendered buffer as text: rows joined by `'\n'`, each row's trailing
    /// spaces trimmed.
    ///
    /// Continuation cells (the empty right half of a wide grapheme) contribute
    /// nothing, so a wide grapheme reads as its single cluster. This is the
    /// readable form used by [`assert_buffer_lines`] and the snapshot helpers.
    ///
    /// [`assert_buffer_lines`]: Self::assert_buffer_lines
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::id::key;
    /// use rabbitui_core::style::Style;
    /// use rabbitui_core::widget::{RenderCtx, Widget};
    /// use rabbitui_testing::TestApp;
    ///
    /// struct Hi;
    /// impl Widget for Hi {
    ///     type State = ();
    ///     fn render(&self, _s: &mut (), ctx: &mut RenderCtx<'_>) {
    ///         ctx.set_string(Position::ORIGIN, "hi", Style::new());
    ///     }
    /// }
    ///
    /// let mut app = TestApp::new(Size::new(5, 2), ());
    /// app.render(|_s, frame| frame.widget(key("hi"), frame.area(), &Hi));
    /// assert_eq!(app.buffer_text(), "hi\n");
    /// ```
    #[must_use]
    pub fn buffer_text(&self) -> String {
        buffer_text(&self.buffer)
    }

    /// Asserts the rendered buffer equals `expected`, one string per row.
    ///
    /// Each row is compared trailing-space-trimmed (as [`buffer_text`] renders
    /// it), so tests need not pad lines to the buffer width. On a mismatch the
    /// panic message shows expected and actual side by side, row by row, with a
    /// marker on the differing rows.
    ///
    /// [`buffer_text`]: Self::buffer_text
    ///
    /// # Panics
    ///
    /// Panics if the rendered rows differ from `expected` (in count or content).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::id::key;
    /// use rabbitui_core::style::Style;
    /// use rabbitui_core::widget::{RenderCtx, Widget};
    /// use rabbitui_testing::TestApp;
    ///
    /// struct Two;
    /// impl Widget for Two {
    ///     type State = ();
    ///     fn render(&self, _s: &mut (), ctx: &mut RenderCtx<'_>) {
    ///         ctx.set_string(Position::new(0, 0), "a", Style::new());
    ///         ctx.set_string(Position::new(0, 1), "b", Style::new());
    ///     }
    /// }
    ///
    /// let mut app = TestApp::new(Size::new(3, 2), ());
    /// app.render(|_s, frame| frame.widget(key("t"), frame.area(), &Two));
    /// app.assert_buffer_lines(&["a", "b"]);
    /// ```
    pub fn assert_buffer_lines(&self, expected: &[&str]) {
        let actual: Vec<String> = buffer_lines(&self.buffer);
        let matches = actual.len() == expected.len()
            && actual.iter().zip(expected).all(|(a, e)| a == e);
        assert!(matches, "{}", diff_message(expected, &actual));
    }
}

/// Clears `buffer` to blank (all default cells) in place.
fn clear(buffer: &mut Buffer) {
    buffer.reset();
}

/// Renders one row of `buffer` as a trailing-trimmed string.
fn row_text(buffer: &Buffer, y: u16) -> String {
    let mut line = String::new();
    for x in 0..buffer.size().width {
        // `get` never fails within `size`; a missing cell would be a bug.
        if let Some(cell) = buffer.get(Position::new(x, y)) {
            line.push_str(&cell.symbol);
        }
    }
    line.trim_end().to_string()
}

/// Every row of `buffer` as trailing-trimmed strings, top to bottom.
fn buffer_lines(buffer: &Buffer) -> Vec<String> {
    (0..buffer.size().height).map(|y| row_text(buffer, y)).collect()
}

/// `buffer_lines` joined with `'\n'` — the public [`TestApp::buffer_text`] form.
///
/// Exposed at crate level so the snapshot helpers render a buffer the same way
/// the assertions do.
#[must_use]
pub fn buffer_text(buffer: &Buffer) -> String {
    buffer_lines(buffer).join("\n")
}

/// Builds the side-by-side diff message for [`TestApp::assert_buffer_lines`].
fn diff_message(expected: &[&str], actual: &[String]) -> String {
    let mut message = String::from("buffer did not match expected lines:\n");
    let rows = expected.len().max(actual.len());
    for i in 0..rows {
        let want = expected.get(i).copied();
        let have = actual.get(i).map(String::as_str);
        let marker = if want == have { "  " } else { "! " };
        message.push_str(&format!(
            "{marker}row {i}: expected {:?}  actual {:?}\n",
            want.unwrap_or("<none>"),
            have.unwrap_or("<none>"),
        ));
    }
    message
}

#[cfg(test)]
mod tests {
    use rabbitui_core::frame::Frame;
    use rabbitui_core::geometry::{Position, Size};
    use rabbitui_core::id::key;
    use rabbitui_core::style::Style;
    use rabbitui_core::widget::{RenderCtx, Widget};

    use super::TestApp;

    /// A stateless label used across the driver tests.
    struct Label<'a>(&'a str);
    impl Widget for Label<'_> {
        type State = ();
        fn render(&self, (): &mut (), ctx: &mut RenderCtx<'_>) {
            ctx.set_string(Position::ORIGIN, self.0, Style::new());
        }
    }

    /// A stateful widget that counts its own renders — a probe for state-store
    /// persistence across driver frames.
    #[derive(Default)]
    struct RenderCount {
        renders: u32,
    }
    struct Probe;
    impl Widget for Probe {
        type State = RenderCount;
        fn render(&self, state: &mut RenderCount, ctx: &mut RenderCtx<'_>) {
            state.renders += 1;
            ctx.set_string(Position::ORIGIN, &state.renders.to_string(), Style::new());
        }
    }

    fn label_view<'a>(text: &'a str) -> impl FnOnce(&(), &mut Frame<'_>) + 'a {
        move |(), frame: &mut Frame<'_>| {
            frame.widget(key("label"), frame.area(), &Label(text));
        }
    }

    #[test]
    fn render_paints_the_current_state() {
        let mut app = TestApp::new(Size::new(5, 1), ());
        app.render(label_view("hi"));
        app.assert_buffer_lines(&["hi"]);
    }

    #[test]
    fn buffer_text_joins_and_trims_rows() {
        let mut app = TestApp::new(Size::new(5, 2), ());
        app.render(|(), frame| {
            frame.widget(key("label"), frame.area(), &Label("ab"));
        });
        assert_eq!(app.buffer_text(), "ab\n");
    }

    #[test]
    fn send_folds_state_then_rerenders() {
        let mut app = TestApp::new(Size::new(3, 1), 0u32);
        app.send(
            |count| *count += 1,
            |count, frame| {
                let text = count.to_string();
                frame.widget(key("n"), frame.area(), &Label(&text));
            },
        );
        assert_eq!(app.state(), &1);
        app.assert_buffer_lines(&["1"]);
    }

    #[test]
    fn buffer_clears_between_frames() {
        let mut app = TestApp::new(Size::new(5, 1), ());
        app.render(label_view("wide"));
        app.assert_buffer_lines(&["wide"]);
        // A shorter label in the next frame must not leave the old tail behind.
        app.render(label_view("ok"));
        app.assert_buffer_lines(&["ok"]);
    }

    #[test]
    fn stateful_widget_state_persists_across_frames() {
        let mut app = TestApp::new(Size::new(2, 1), ());
        for _ in 0..3 {
            app.render(|(), frame| frame.widget(key("probe"), frame.area(), &Probe));
        }
        // The probe counted three renders against one persisted identity.
        app.assert_buffer_lines(&["3"]);
        assert_eq!(app.store_len(), 1);
    }

    #[test]
    #[should_panic(expected = "buffer did not match")]
    fn assert_buffer_lines_reports_a_mismatch() {
        let mut app = TestApp::new(Size::new(3, 1), ());
        app.render(label_view("no"));
        app.assert_buffer_lines(&["yes"]);
    }

    /// A focusable widget owning a mutable string, to exercise the between-frames
    /// widget-command and focus apply through the shared `core::pending` path.
    #[derive(Default)]
    struct FieldState {
        value: String,
    }
    struct Field;
    impl Widget for Field {
        type State = FieldState;
        fn render(&self, state: &mut FieldState, ctx: &mut RenderCtx<'_>) {
            ctx.focusable(true);
            ctx.set_string(Position::ORIGIN, &state.value, Style::new());
        }
    }

    fn field_view((): &(), frame: &mut Frame<'_>) {
        frame.widget(key("field"), frame.area(), &Field);
    }

    #[test]
    fn apply_pending_runs_a_widget_command_between_frames() {
        let mut app = TestApp::new(Size::new(6, 1), ());
        app.render(field_view);
        let id = rabbitui_core::id::WidgetId::ROOT.child(key("field"));
        app.apply_pending(|p| p.command::<Field>(id, |s| s.value = "set".into()), field_view);
        app.assert_buffer_lines(&["set"]);
    }

    #[test]
    fn apply_pending_honors_a_focus_request() {
        let mut app = TestApp::new(Size::new(6, 1), ());
        app.render(field_view);
        let id = rabbitui_core::id::WidgetId::ROOT.child(key("field"));
        app.apply_pending(|p| p.request_focus(id), field_view);
        assert_eq!(app.focus(), Some(id));
    }
}
