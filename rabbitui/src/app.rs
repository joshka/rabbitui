//! The minimal application loop.
//!
//! [`run`] is the walking-skeleton facade over the event loop (ADR 0005): it
//! owns the terminal, drives update → view → diff → render, and restores the
//! terminal on every exit path. The app supplies plain owned state, a
//! synchronous `update` that folds events and outcomes into that state, and a
//! synchronous `view` that declares the state's UI into a [`Frame`].
//!
//! # The declared frame, facts, and routing
//!
//! `view` receives a [`Frame`] (`docs/adr/0001-programming-model.md`), not a
//! bare buffer: it declares widgets by key into the frame, which composes their
//! identities, lends each its framework-retained state from the loop's
//! [`StateStore`], paints them, and — from slice 3 — collects the frame's
//! *facts* (each widget's area, scope parent, focusability) and registers a
//! handler thunk per widget.
//!
//! On the next input event, [`run`] maps it into the core input vocabulary
//! (`crate::input`) and routes it through the *previous* frame's facts and
//! handlers via the shared [`route`] function (`docs/adr/0006-input-focus-events.md`):
//! capture → target → bubble, with unconsumed Tab/BackTab driving focus
//! traversal. Handlers emit typed [`Outcome`]s; the app sees them — together
//! with the raw event — in one [`Update`] passed to `update`. Focus is framework
//! state the loop owns across frames (a [`Focus`]), not app state.
//!
//! # Examples
//!
//! A one-line app that quits on the next event:
//!
//! ```no_run
//! use std::ops::ControlFlow;
//!
//! use rabbitui::app::{self, Update};
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::id::key;
//! use rabbitui_widgets::Text;
//!
//! # async fn demo() -> rabbitui::app::Result<()> {
//! app::run(
//!     (),
//!     |_state: &mut (), _update: Update<'_>| ControlFlow::Break(()),
//!     |_state: &(), frame: &mut Frame<'_>| {
//!         frame.widget(key("greeting"), frame.area(), &Text::new("hi"));
//!     },
//! )
//! .await
//! # }
//! ```

use std::ops::ControlFlow;

use rabbitui_core::buffer::Buffer;
use rabbitui_core::facts::FrameFacts;
use rabbitui_core::frame::{Frame, HandlerMap};
use rabbitui_core::geometry::Size;
use rabbitui_core::id::{Key, WidgetId};
use rabbitui_core::input::InputEvent;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::routing::{Focus, route};
use rabbitui_core::store::StateStore;

use crate::render;
use crate::terminal::Terminal;

pub use crate::terminal::{Error, Result};

/// An event delivered to the app's `update` function, inside an [`Update`].
///
/// # Substrate gap: resize is polled, not pushed
///
/// qwertty has no resize event yet (`docs/adr/0012-terminal-substrate.md`), so
/// [`run`] polls the terminal size once per loop iteration and synthesizes
/// [`Event::Resize`] when it changes. This means a resize is only observed on
/// the next input event, not the instant the window changes; when qwertty gains
/// a resize signal this becomes push-based with no change to this enum.
///
/// [`Event::Input`] carries a *core* [`InputEvent`] — the facade has already
/// mapped qwertty's decoded event into rabbitui's substrate-free vocabulary and
/// routed it through the frame; the app sees it only if no widget consumed it.
///
/// # Examples
///
/// ```
/// use rabbitui::app::Event;
/// use rabbitui_core::geometry::Size;
///
/// let event = Event::Resize(Size::new(80, 24));
/// assert!(matches!(event, Event::Resize(_)));
/// ```
#[derive(Debug, Clone, Copy)]
pub enum Event {
    /// A decoded, unconsumed input event (a key). Consumed events (a button
    /// press, a Tab that moved focus) never reach the app as `Input`; their
    /// effect arrives as an [`Outcome`] instead.
    Input(InputEvent),
    /// The terminal was resized to this new size, detected by polling.
    Resize(Size),
}

/// One call into the app's `update`: the event that occurred, plus any typed
/// outcomes routing produced from it.
///
/// Per `docs/adr/0001-programming-model.md`, a widget handler emits outcomes
/// rather than mutating app state; the loop collects the frame's outcomes and
/// hands them to `update` *in the same call* as the event, so the app applies
/// every effect itself. Query them with [`outcome_for`](Self::outcome_for) by
/// the widget's root-relative key path.
///
/// # Examples
///
/// ```
/// use rabbitui::app::{Event, Update};
/// use rabbitui_core::id::{WidgetId, key};
/// use rabbitui_core::input::{InputEvent, Key};
/// use rabbitui_core::outcome::Outcome;
///
/// let id = WidgetId::ROOT.child(key("ok"));
/// let outcomes = [(id, Outcome::Activated)];
/// let update = Update::new(Event::Input(InputEvent::key(Key::Enter)), &outcomes);
///
/// assert_eq!(update.outcome_for(&[key("ok")]), Some(&Outcome::Activated));
/// assert_eq!(update.outcome_for(&[key("cancel")]), None);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Update<'a> {
    event: Event,
    outcomes: &'a [(WidgetId, Outcome)],
}

impl<'a> Update<'a> {
    /// Builds an update from an event and the outcomes routing produced.
    ///
    /// The loop constructs this; it is public so tests (and the harness) can
    /// build one directly.
    #[must_use]
    pub fn new(event: Event, outcomes: &'a [(WidgetId, Outcome)]) -> Self {
        Self { event, outcomes }
    }

    /// The event this update is delivering.
    #[must_use]
    pub fn event(&self) -> Event {
        self.event
    }

    /// Every outcome routing produced this event, keyed by the emitting widget.
    #[must_use]
    pub fn outcomes(&self) -> &'a [(WidgetId, Outcome)] {
        self.outcomes
    }

    /// The first outcome emitted by the widget at the given root-relative key
    /// path, if any.
    ///
    /// Ids compose, so a widget is addressed by the path of keys from the root:
    /// the common case is a single root-level key, `&[key("ok")]`; a widget
    /// declared inside a scope is `&[key("panel"), key("ok")]`. This mirrors the
    /// composition [`Frame`] performs when declaring the widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui::app::{Event, Update};
    /// use rabbitui_core::id::{WidgetId, key};
    /// use rabbitui_core::input::{InputEvent, Key};
    /// use rabbitui_core::outcome::Outcome;
    ///
    /// // A widget declared inside a "panel" scope.
    /// let id = WidgetId::ROOT.child(key("panel")).child(key("ok"));
    /// let outcomes = [(id, Outcome::Activated)];
    /// let update = Update::new(Event::Input(InputEvent::key(Key::Enter)), &outcomes);
    ///
    /// assert_eq!(update.outcome_for(&[key("panel"), key("ok")]), Some(&Outcome::Activated));
    /// ```
    #[must_use]
    pub fn outcome_for(&self, path: &[Key]) -> Option<&Outcome> {
        let target = path.iter().fold(WidgetId::ROOT, |id, &key| id.child(key));
        self.outcomes.iter().find(|(id, _)| *id == target).map(|(_, outcome)| outcome)
    }
}

/// Runs the application loop until `update` returns [`ControlFlow::Break`].
///
/// The loop opens the terminal, renders an initial full frame (capturing its
/// facts and handlers), then repeats: wait for one input event; poll the
/// terminal size and, if it changed, resize the buffers (a full repaint) and
/// deliver [`Event::Resize`]; map the input into the core vocabulary and
/// [`route`] it through the previous frame's facts and handlers; deliver an
/// [`Update`] carrying the (possibly unconsumed) event and any outcomes to
/// `update`; if `update` asked to break, close the terminal and return;
/// otherwise repaint with `view`, diff, render, and keep the new facts for the
/// next event.
///
/// `update` and `view` are strictly synchronous — no `.await` — matching ADR
/// 0005's synchronous core; only the loop edges (input, render) are async.
///
/// The loop owns a [`StateStore`] and a [`Focus`] across iterations. Before each
/// event it reconciles focus against the latest facts (focus survives
/// re-declaration; a vanished target recovers to the next survivor), then routes.
///
/// # Errors
///
/// Returns an error if opening the terminal, reading input, polling the size,
/// rendering, or closing the terminal fails.
///
/// # Examples
///
/// A counter that activates on a button and quits on `q`:
///
/// ```no_run
/// use std::ops::ControlFlow;
///
/// use rabbitui::app::{self, Event, Update};
/// use rabbitui_core::frame::Frame;
/// use rabbitui_core::id::key;
/// use rabbitui_core::input::Key;
/// use rabbitui_core::outcome::Outcome;
/// use rabbitui_widgets::Button;
///
/// # async fn demo() -> rabbitui::app::Result<()> {
/// app::run(
///     0u32,
///     |count: &mut u32, update: Update<'_>| {
///         if update.outcome_for(&[key("inc")]) == Some(&Outcome::Activated) {
///             *count += 1;
///         }
///         if matches!(update.event(), Event::Input(e) if e.as_key().map(|k| k.key) == Some(Key::Char('q'))) {
///             return ControlFlow::Break(());
///         }
///         ControlFlow::Continue(())
///     },
///     |_count: &u32, frame: &mut Frame<'_>| {
///         frame.widget(key("inc"), frame.area(), &Button::new("+"));
///     },
/// )
/// .await
/// # }
/// ```
pub async fn run<S>(
    mut state: S,
    mut update: impl FnMut(&mut S, Update<'_>) -> ControlFlow<()>,
    view: impl Fn(&S, &mut Frame<'_>),
) -> Result<()> {
    let mut terminal = Terminal::open().await?;
    let mut size = terminal.size()?;

    // Front buffer: what the terminal currently shows. Back buffer: what the
    // next frame will show. The state store and focus persist across iterations.
    let mut front = Buffer::new(size);
    let mut back = Buffer::new(size);
    let mut store = StateStore::new();
    let mut focus = Focus::new();

    // The first frame: no focus yet, capture its facts and handlers.
    let (mut facts, mut handlers) = draw(&mut back, &mut store, focus, &state, &view);
    focus.reconcile(&facts);
    render::render(&mut terminal, &back.diff(&front)).await?;
    std::mem::swap(&mut front, &mut back);

    loop {
        let input = terminal.next_event().await?;

        // Poll for a resize (substrate has no resize event; see `Event`). On a
        // change, resize both buffers to blank so the next diff is a full
        // repaint, then deliver the resize to `update`.
        let new_size = terminal.size()?;
        if new_size != size {
            size = new_size;
            front.resize(size);
            back.resize(size);
            let update_ctx = Update::new(Event::Resize(size), &[]);
            if let ControlFlow::Break(()) = update(&mut state, update_ctx) {
                return terminal.close().await;
            }
        }

        // Map the substrate event into the core vocabulary; unmapped input is
        // dropped (see `crate::input`), so the loop simply repaints and waits.
        if let Some(event) = crate::input::from_qwertty(&input) {
            // Route through the previous frame's facts and handlers. The app
            // always sees the event plus any outcomes in one `Update`; a
            // consumed event still carries context, and its effect is delivered
            // as an outcome rather than a raw key the app must re-interpret.
            let result = route(&facts, &handlers, &mut focus, &mut store, &event);
            let ctx = Update::new(Event::Input(event), &result.outcomes);
            if let ControlFlow::Break(()) = update(&mut state, ctx) {
                return terminal.close().await;
            }
        }

        // Repaint from scratch, capturing this frame's facts for the next event.
        back.reset();
        let drawn = draw(&mut back, &mut store, focus, &state, &view);
        facts = drawn.0;
        handlers = drawn.1;
        focus.reconcile(&facts);
        render::render(&mut terminal, &back.diff(&front)).await?;
        std::mem::swap(&mut front, &mut back);
    }
}

/// Declares one frame: brackets `view` in the store's frame lifecycle, builds a
/// [`Frame`] over `buffer` and `store` with the current focus, and returns the
/// frame's collected facts and handlers.
///
/// The caller has already cleared (or resized) `buffer` to blank, matching the
/// declared-frame rule that widgets re-declare everything each frame.
fn draw<S>(
    buffer: &mut Buffer,
    store: &mut StateStore,
    focus: Focus,
    state: &S,
    view: &impl Fn(&S, &mut Frame<'_>),
) -> (FrameFacts, HandlerMap) {
    store.begin_frame();
    let parts = {
        let mut frame = Frame::with_focus(buffer, store, focus.current());
        view(state, &mut frame);
        frame.into_parts()
    };
    store.end_frame();
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use rabbitui_core::id::key;
    use rabbitui_core::input::Key;

    #[test]
    fn resize_event_carries_the_new_size() {
        let event = Event::Resize(Size::new(120, 40));
        match event {
            Event::Resize(size) => assert_eq!(size, Size::new(120, 40)),
            Event::Input(_) => panic!("expected a resize event"),
        }
    }

    #[test]
    fn outcome_for_matches_root_level_key_path() {
        let id = WidgetId::ROOT.child(key("ok"));
        let outcomes = [(id, Outcome::Activated)];
        let update = Update::new(Event::Input(InputEvent::key(Key::Enter)), &outcomes);
        assert_eq!(update.outcome_for(&[key("ok")]), Some(&Outcome::Activated));
        assert_eq!(update.outcome_for(&[key("nope")]), None);
    }

    #[test]
    fn outcome_for_matches_nested_key_path() {
        let id = WidgetId::ROOT.child(key("panel")).child(key("ok"));
        let outcomes = [(id, Outcome::Activated)];
        let update = Update::new(Event::Input(InputEvent::key(Key::Enter)), &outcomes);
        assert_eq!(update.outcome_for(&[key("panel"), key("ok")]), Some(&Outcome::Activated));
        // The wrong depth does not match.
        assert_eq!(update.outcome_for(&[key("ok")]), None);
    }
}
