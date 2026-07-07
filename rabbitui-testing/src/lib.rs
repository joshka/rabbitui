//! Headless test harness for rabbitui applications.
//!
//! Per `docs/adr/0009-testing.md`, interaction correctness is only provable by
//! driving the app and inspecting output, so the test kit ships *before* the
//! widget catalog grows and is public, semver-stable API — third-party widget
//! authors and coding agents verify their own output against the same contract
//! the core uses. This crate depends only on [`rabbitui_core`]; it never touches
//! tokio or a terminal, so tests run deterministically with no I/O.
//!
//! Slice 2 lands the first layer of the three in ADR 0009: a headless
//! [`TestApp`] driver plus buffer-snapshot assertions with an update flag. The
//! vt100 escape-level harness follows in slice 5.
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

pub use snapshot::assert_snapshot;

use rabbitui_core::buffer::Buffer;
use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::{Position, Size};
use rabbitui_core::store::StateStore;

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
        Self { state, store: StateStore::new(), buffer: Buffer::new(size) }
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
        {
            let mut frame = Frame::new(&mut self.buffer, &mut self.store);
            view(&self.state, &mut frame);
        }
        self.store.end_frame();
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
}
