//! The application loop and the [`App`] trait.
//!
//! An application is a type implementing [`App`] (ADR 0001, amended
//! 2026-07-11): `Self` is the state, a synchronous [`App::update`] folds events
//! and outcomes into it, and a synchronous [`App::view`] declares its UI into a
//! [`Frame`]. The provided [`App::run`] owns the terminal, drives
//! update → view → diff → render, and restores the terminal on every exit
//! path. Defaulted hooks cover the lifecycle: [`App::init`] (the opening
//! [`Command`]), [`App::global`] (before-`update` chords), and [`App::config`]
//! (launch configuration).
//!
//! ```no_run
//! use std::ops::ControlFlow;
//!
//! use rabbitui::app::{App, Event, Update};
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::id::key;
//! use rabbitui_core::input::Key;
//! use rabbitui_widgets::Text;
//!
//! #[derive(Default)]
//! struct Counter {
//!     count: i64,
//! }
//!
//! impl App for Counter {
//!     fn update(&mut self, update: Update<'_>) -> ControlFlow<()> {
//!         if let Event::Input(input) = update.event() {
//!             match input.as_key().map(|k| k.key) {
//!                 Some(Key::Char('+')) => self.count += 1,
//!                 Some(Key::Char('q')) => return ControlFlow::Break(()),
//!                 _ => {}
//!             }
//!         }
//!         ControlFlow::Continue(())
//!     }
//!
//!     fn view(&self, frame: &mut Frame<'_>) {
//!         let text = format!("count: {} (+ to add, q to quit)", self.count);
//!         frame.widget(key("count"), frame.area(), &Text::new(&text));
//!     }
//! }
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> rabbitui::app::Result<()> {
//!     Counter::default().run().await
//! }
//! ```
//!
//! For tests, demos, and one-screen tools the closure shorthand [`from_fn`]
//! (or the free [`run`]) skips the trait ceremony:
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
//! On the next input event, the loop maps it into the core input vocabulary
//! (`crate::input`) and routes it through the *previous* frame's facts and
//! handlers via the shared [`route`] function (`docs/adr/0006-input-focus-events.md`):
//! capture → target → bubble, with unconsumed Tab/BackTab driving focus
//! traversal. Handlers emit typed [`Outcome`]s; the app sees them — together
//! with the raw event — in one [`Update`] passed to `update`. Focus is framework
//! state the loop owns across frames (a [`Focus`]), not app state.

use std::cell::RefCell;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rabbitui_core::buffer::Buffer;
use rabbitui_core::commit::CommitLine;
use rabbitui_core::facts::FrameFacts;
use rabbitui_core::frame::{Frame, HandlerMap};
use rabbitui_core::geometry::Size;
use rabbitui_core::id::{Key, WidgetId};
use rabbitui_core::input::InputEvent;
use rabbitui_core::mode::Mode;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::pending::Pending as WidgetPending;
use rabbitui_core::routing::{Focus, route};
use rabbitui_core::store::StateStore;
use rabbitui_core::theme::Theme;
use rabbitui_core::widget::Widget;

use crate::effect::{Command, EffectError, Effects, Outbox};
use crate::engine::{AltEngine, InlineEngine};
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
/// # Messages and effects (slice 6)
///
/// An app that spawns effects (ADR 0005) defines a message type `M` and receives
/// effect results as [`Event::Message`]. A panicking effect is contained at the
/// tokio task boundary and surfaced as [`Event::EffectFailed`] rather than
/// swallowed. Message-less apps use the default `M = ()` and compile unchanged —
/// they simply never see `Message`.
///
/// # Examples
///
/// ```
/// use rabbitui::app::Event;
/// use rabbitui_core::geometry::Size;
///
/// let event: Event = Event::Resize(Size::new(80, 24));
/// assert!(matches!(event, Event::Resize(_)));
/// ```
#[derive(Debug, Clone)]
pub enum Event<M = ()> {
    /// Delivered exactly once, before the first input or effect, after the
    /// initial frame has drawn. Lets a self-starting app spawn its opening
    /// `Command` (e.g. begin a stream) or seed state at launch instead of waiting
    /// for the first keypress (dogfood finding #1). The store, focus, and facts
    /// reflect the frame already on screen; `update.spawn(cmd)` and
    /// `update.widget(...)` work as in any other update.
    Started,
    /// A decoded, unconsumed input event (a key). Consumed events (a button
    /// press, a Tab that moved focus) never reach the app as `Input`; their
    /// effect arrives as an [`Outcome`] instead.
    Input(InputEvent),
    /// The terminal was resized to this new size, detected by polling.
    Resize(Size),
    /// A message an effect the app spawned produced (ADR 0005). Arrives in
    /// completion order, re-entering the one serialized `update`.
    Message(M),
    /// An effect task panicked; contained and reported rather than crashing the
    /// loop. Carries the effect's group (if any) and the failure text.
    EffectFailed(EffectError),
}

/// Buffered side effects an `update` requested: lines to commit into scrollback,
/// a mode switch, effects to spawn, and between-frames widget commands / a focus
/// request — all applied by the runtime after `update` returns.
///
/// Per the slice-5 design note, committing and mode switching are *update-time*
/// actions (event-driven, naturally once), never view-time ones — a view re-runs
/// every frame and would double-emit. Slice 6 adds three more update-time actions
/// on the same buffering principle: [`Update::spawn`] queues a [`Command`] the runtime
/// hands to its [`Effects`] runtime, and [`Update::widget`] / [`Update::focus`]
/// record into a [`core::pending`](rabbitui_core::pending) set the runtime applies
/// between frames through the *same* function [`TestApp`] uses.
///
/// This type is opaque: construct it with [`Default::default`] (a test builds one
/// to pass to [`Update::new`]) and read it back only through the runtime. Its
/// fields are private and may change.
///
/// [`TestApp`]: https://docs.rs/rabbitui-testing/latest/rabbitui_testing/struct.TestApp.html
pub struct Pending<M = ()> {
    /// Lines committed this update, in call order.
    commits: Vec<CommitLine>,
    /// The last mode requested this update, if any (later calls win).
    set_mode: Option<Mode>,
    /// The last theme requested this update, if any (later calls win).
    set_theme: Option<Theme>,
    /// Effects to spawn after `update` returns, in call order.
    effects: Vec<Command<M>>,
    /// Between-frames widget commands and a deferred focus request, applied by the
    /// shared [`core::pending`](rabbitui_core::pending) function.
    widget: WidgetPending,
    /// Whether this update's widget/focus requests were made via the *guarded*
    /// API ([`try_command`](Update::try_command) / [`try_focus`](Update::try_focus)):
    /// applied best-effort — a missing target is a soft skip, not a `debug_assert`.
    /// Guarding is all-or-nothing per update (see
    /// [`core::pending::Pending::apply_guarded`](rabbitui_core::pending::Pending::apply_guarded)).
    guarded: bool,
}

impl<M> Default for Pending<M> {
    fn default() -> Self {
        Self {
            commits: Vec::new(),
            set_mode: None,
            set_theme: None,
            effects: Vec::new(),
            widget: WidgetPending::new(),
            guarded: false,
        }
    }
}

impl<M> std::fmt::Debug for Pending<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pending")
            .field("commits", &self.commits.len())
            .field("set_mode", &self.set_mode)
            .field("set_theme", &self.set_theme)
            .field("effects", &self.effects.len())
            .field("widget", &self.widget)
            .finish()
    }
}

/// One call into the app's `update`: the event that occurred, the typed outcomes
/// routing produced from it, and a sink for buffered side effects (commits, mode
/// switches).
///
/// Per `docs/adr/0001-programming-model.md`, a widget handler emits outcomes
/// rather than mutating app state; the loop collects the frame's outcomes and
/// hands them to `update` *in the same call* as the event, so the app applies
/// every effect itself. Query them with [`outcome_for`](Self::outcome_for) by
/// the widget's root-relative key path. Inline scrollback commits and runtime
/// mode switches are requested with [`commit`](Self::commit) and
/// [`set_mode`](Self::set_mode).
///
/// # Examples
///
/// ```
/// use std::cell::RefCell;
///
/// use rabbitui::app::{Event, Update};
/// use rabbitui_core::id::{WidgetId, key};
/// use rabbitui_core::input::{InputEvent, Key};
/// use rabbitui_core::outcome::Outcome;
///
/// let id = WidgetId::ROOT.child(key("ok"));
/// let outcomes = [(id, Outcome::Activated)];
/// let pending = RefCell::new(Default::default());
/// let update: Update<'_, ()> = Update::new(Event::Input(InputEvent::key(Key::Enter)), &outcomes, &pending);
///
/// assert_eq!(update.outcome_for(&[key("ok")]), Some(&Outcome::Activated));
/// assert_eq!(update.outcome_for(&[key("cancel")]), None);
/// ```
#[derive(Debug)]
pub struct Update<'a, M = ()> {
    event: Event<M>,
    outcomes: &'a [(WidgetId, Outcome)],
    pending: &'a RefCell<Pending<M>>,
    /// Whether routing consumed the event.
    consumed: bool,
    /// The framework's focus at event time.
    focus: Option<WidgetId>,
    /// The widget-state store as of the last frame, for read-only peeks.
    store: Option<&'a StateStore>,
}

impl<'a, M> Update<'a, M> {
    /// Builds an update from an event, the outcomes routing produced, and a
    /// pending-effects sink.
    ///
    /// The loop constructs this; it is public so tests (and the harness) can
    /// build one directly — pass a `&RefCell<Default::default()>` when the test
    /// ignores commits and mode switches.
    #[must_use]
    pub fn new(
        event: Event<M>,
        outcomes: &'a [(WidgetId, Outcome)],
        pending: &'a RefCell<Pending<M>>,
    ) -> Self {
        Self {
            event,
            outcomes,
            pending,
            consumed: false,
            focus: None,
            store: None,
        }
    }

    /// Marks whether routing consumed the event (a widget handled it).
    ///
    /// The loop sets this from the route result; tests may too.
    #[must_use]
    pub fn with_consumed(mut self, consumed: bool) -> Self {
        self.consumed = consumed;
        self
    }

    /// Whether a widget consumed this event during routing.
    ///
    /// `update` runs for every event so outcomes can ride along (ADR 0006
    /// amendments) — which means a raw-key binding in `update` also sees keys
    /// a focused widget already handled. Guard app-level printable-key
    /// bindings with this, or a `d` binding will fire while the user types
    /// "feed" into an input (found by betamax tapes, 2026-07-07). Outcomes
    /// (`outcome_for`) need no guard: they only exist when a widget chose to
    /// emit them.
    #[must_use]
    pub fn consumed(&self) -> bool {
        self.consumed
    }

    /// Supplies the focus snapshot so [`is_focused`](Self::is_focused) works.
    ///
    /// The loop sets this from the framework's focus state; tests may too.
    #[must_use]
    pub fn with_focus(mut self, focus: Option<WidgetId>) -> Self {
        self.focus = focus;
        self
    }

    /// Supplies the widget-state store so [`widget_state`](Self::widget_state)
    /// can read committed state.
    ///
    /// The loop sets this from the store as of the last painted frame; tests may
    /// too. The reference is read-only — peeking never mutates the store or marks
    /// an id seen, so it is safe to hand out during `update`.
    #[must_use]
    pub fn with_store(mut self, store: &'a StateStore) -> Self {
        self.store = Some(store);
        self
    }

    /// Whether the widget at this root-relative key path currently has focus.
    ///
    /// Lets an app make focus-dependent decisions (arrow-key field
    /// navigation, contextual help lines) without mirroring framework state
    /// from outcomes. Compares composed identity, so it works at any depth:
    /// `update.is_focused(&[key("modal"), key("ok")])`.
    #[must_use]
    pub fn is_focused(&self, path: &[Key]) -> bool {
        let id = path.iter().fold(WidgetId::ROOT, |id, k| id.child(*k));
        self.focus == Some(id)
    }

    /// The action this event dispatches under `keymap`, with the printable-chord
    /// consumed-guard applied.
    ///
    /// Returns `None` when the event is not a key, the chord is unbound, or a
    /// focused widget consumed a *guarded* (typeable) chord — so a bare letter is
    /// never stolen from an input. One call replaces the
    /// `!consumed() && event().as_key() && keymap.action_for(...)` dance at every
    /// dispatch site (see [`rabbitui_core::keymap`]).
    #[must_use]
    pub fn action<A>(&self, keymap: &rabbitui_core::keymap::Keymap<'_, A>) -> Option<A>
    where
        A: Copy + PartialEq,
    {
        let Event::Input(input) = self.event() else {
            return None;
        };
        keymap.action_for_guarded(input.as_key()?, self.consumed())
    }

    /// Commits `line` into the terminal's native scrollback (inline mode).
    ///
    /// The line is appended once, above the live tail, and thereafter owned by
    /// the terminal — never repainted, reflowed by the terminal on resize (ADR
    /// 0013's append-once channel). Multiple calls in one update stay in order.
    /// In alt-screen mode a commit is still buffered and is flushed into
    /// scrollback if/when the app switches to inline (or on quit for a pending
    /// alt→inline order); the runtime flushes buffered commits *before* entering
    /// the alt screen so nothing is lost behind it.
    ///
    /// Committing is an update-time action: a view re-runs every frame, so
    /// committing there would double-emit. This is the event-driven path.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::cell::RefCell;
    ///
    /// use rabbitui::app::{Event, Update};
    /// use rabbitui_core::input::{InputEvent, Key};
    ///
    /// let pending = RefCell::new(Default::default());
    /// let update: Update<'_, ()> = Update::new(Event::Input(InputEvent::key(Key::Enter)), &[], &pending);
    /// update.commit("build finished");
    /// ```
    pub fn commit(&self, line: impl Into<CommitLine>) {
        self.pending.borrow_mut().commits.push(line.into());
    }

    /// Requests a switch to `mode`, applied by the runtime before the next frame.
    ///
    /// The switch is buffered and ordered against any commits made in the same
    /// update: commits flush into scrollback *before* an alt-screen entry, so
    /// content committed just before switching to alt is not lost behind the
    /// alternate screen (slice-5 design note). Calling twice keeps the last mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::cell::RefCell;
    ///
    /// use rabbitui::app::{Event, Update};
    /// use rabbitui_core::input::{InputEvent, Key};
    /// use rabbitui_core::mode::Mode;
    ///
    /// let pending = RefCell::new(Default::default());
    /// let update: Update<'_, ()> = Update::new(Event::Input(InputEvent::key(Key::Char('m'))), &[], &pending);
    /// update.set_mode(Mode::inline(4));
    /// ```
    pub fn set_mode(&self, mode: Mode) {
        self.pending.borrow_mut().set_mode = Some(mode);
    }

    /// Requests a switch to `theme`, applied by the runtime before the next frame.
    ///
    /// Buffered like [`set_mode`](Self::set_mode) — calling twice keeps the last
    /// theme — and applied by replacing the active theme threaded into every
    /// widget's [`RenderContext`](rabbitui_core::widget::RenderContext). This is how an app
    /// offers a live theme picker. If a theme file is also configured
    /// ([`Config::theme_file`]), it is last-writer-wins: a later file
    /// change (debug hot-reload) overrides a runtime switch, and vice versa.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::cell::RefCell;
    ///
    /// use rabbitui::app::{Event, Update};
    /// use rabbitui_core::input::{InputEvent, Key};
    /// use rabbitui_core::theme::Theme;
    ///
    /// let pending = RefCell::new(Default::default());
    /// let update: Update<'_, ()> = Update::new(Event::Input(InputEvent::key(Key::Char('2'))), &[], &pending);
    /// update.set_theme(Theme::nord());
    /// ```
    pub fn set_theme(&self, theme: Theme) {
        self.pending.borrow_mut().set_theme = Some(theme);
    }

    /// Commands the declared widget of type `W` at the given root-relative key
    /// path, applied between frames (slice 6).
    ///
    /// The app mutates a widget's retained state without owning its type: `f`
    /// runs against the concrete `W::State` when the runtime applies the pending
    /// set after the *next* frame is declared. This is the controlled-input path
    /// — `update.widget::<TextInput>(&[key("search")], |s| s.clear())` clears a
    /// field on submit, replacing the slice-4 re-keying workaround. Commanding a
    /// widget that was never declared is an app bug: the command is dropped with a
    /// `debug_assert` (see [`core::pending`](rabbitui_core::pending)).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::cell::RefCell;
    ///
    /// use rabbitui::app::{Event, Update};
    /// use rabbitui_core::id::key;
    /// use rabbitui_core::input::{InputEvent, Key};
    /// use rabbitui_widgets::TextInput;
    ///
    /// let pending = RefCell::new(Default::default());
    /// let update: Update<'_, ()> = Update::new(Event::Input(InputEvent::key(Key::Enter)), &[], &pending);
    /// update.widget::<TextInput>(&[key("search")], |state| state.clear());
    /// ```
    pub fn widget<W>(&self, path: &[Key], f: impl FnOnce(&mut W::State) + 'static)
    where
        W: Widget,
    {
        let id = path.iter().fold(WidgetId::ROOT, |id, &key| id.child(key));
        self.pending.borrow_mut().widget.command::<W>(id, f);
    }

    /// Reads the widget at the given root-relative key path's committed state, if
    /// it exists in the store as of the last painted frame.
    ///
    /// The write-side sibling of [`widget`](Self::widget): where `widget` queues a
    /// mutation applied before the next frame, this peeks the *current* value so an
    /// app can branch on what a widget holds (a list's selection, an input's text)
    /// without mirroring it from outcomes (dogfood finding #2). Returns `None` when
    /// no store was supplied (`with_store` was not called — e.g. a bare test
    /// `Update`), the id was never declared, or the stored type does not match
    /// `W::State`.
    ///
    /// Peeking is read-only: it does not mark the id seen, so a widget the app
    /// peeks but does not re-declare still ages out at frame end.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::cell::RefCell;
    ///
    /// use rabbitui::app::{Event, Update};
    /// use rabbitui_core::id::key;
    /// use rabbitui_core::input::{InputEvent, Key};
    /// use rabbitui_widgets::TextInput;
    ///
    /// let pending = RefCell::new(Default::default());
    /// let update: Update<'_, ()> = Update::new(Event::Input(InputEvent::key(Key::Enter)), &[], &pending);
    /// // No store supplied on a bare `Update`, so the read is `None`.
    /// assert!(update.widget_state::<TextInput>(&[key("search")]).is_none());
    /// ```
    #[must_use]
    pub fn widget_state<W>(&self, path: &[Key]) -> Option<&W::State>
    where
        W: Widget,
    {
        let id = path.iter().fold(WidgetId::ROOT, |id, &key| id.child(key));
        self.store?.peek::<W::State>(id)
    }

    /// Requests focus move to the widget at the given root-relative key path,
    /// applied against the *next* frame's facts (slice 6).
    ///
    /// Reveal-or-fail (ADR 0006 amendment): the request is honored if the target
    /// is present-and-focusable in the frame the command triggers (covering the
    /// declare-then-focus case), and dropped with a `debug_assert` naming the path
    /// otherwise. Later calls in one update win.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::cell::RefCell;
    ///
    /// use rabbitui::app::{Event, Update};
    /// use rabbitui_core::id::key;
    /// use rabbitui_core::input::{InputEvent, Key};
    ///
    /// let pending = RefCell::new(Default::default());
    /// let update: Update<'_, ()> = Update::new(Event::Input(InputEvent::key(Key::Tab)), &[], &pending);
    /// update.focus(&[key("search")]);
    /// ```
    pub fn focus(&self, path: &[Key]) {
        let id = path.iter().fold(WidgetId::ROOT, |id, &key| id.child(key));
        self.pending.borrow_mut().widget.request_focus(id);
    }

    /// Like [`focus`](Self::focus) but **best-effort**: if the target is not
    /// present-and-focusable in the frame this update triggers, the request is a
    /// soft skip (no `debug_assert`) rather than a contract violation.
    ///
    /// Use it when the target is *conditionally* declared — e.g. a list that is
    /// sometimes replaced by an empty-state placeholder — where [`focus`](Self::focus)
    /// would panic (the declare-then-focus footgun, dogfood finding #4). Marks the
    /// whole update's pending set guarded (all-or-nothing; a soft skip is logged
    /// via `tracing` when that feature is on, never silently in a way you can't see).
    pub fn try_focus(&self, path: &[Key]) {
        let id = path.iter().fold(WidgetId::ROOT, |id, &key| id.child(key));
        let mut pending = self.pending.borrow_mut();
        pending.widget.request_focus(id);
        pending.guarded = true;
    }

    /// Like [`widget`](Self::widget) but **best-effort**: commanding a widget not
    /// declared this frame is a soft skip instead of a `debug_assert`.
    ///
    /// Use it for a conditionally-declared widget (the declare-then-command sibling
    /// of the focus footgun, dogfood finding #4) — e.g. resetting a list's
    /// selection when the same update may replace the list with an empty state.
    /// Marks the update's pending set guarded (all-or-nothing).
    pub fn try_command<W>(&self, path: &[Key], f: impl FnOnce(&mut W::State) + 'static)
    where
        W: Widget,
    {
        let id = path.iter().fold(WidgetId::ROOT, |id, &key| id.child(key));
        let mut pending = self.pending.borrow_mut();
        pending.widget.command::<W>(id, f);
        pending.guarded = true;
    }

    /// The event this update is delivering.
    ///
    /// Returned by reference: the event may carry a message payload (`M`) which
    /// need not be `Copy`. Match on it in place — `if let Event::Input(input) =
    /// update.event()` binds `input` by reference.
    #[must_use]
    pub fn event(&self) -> &Event<M> {
        &self.event
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
    /// use std::cell::RefCell;
    ///
    /// use rabbitui::app::{Event, Update};
    /// use rabbitui_core::id::{WidgetId, key};
    /// use rabbitui_core::input::{InputEvent, Key};
    /// use rabbitui_core::outcome::Outcome;
    ///
    /// // A widget declared inside a "panel" scope.
    /// let id = WidgetId::ROOT.child(key("panel")).child(key("ok"));
    /// let outcomes = [(id, Outcome::Activated)];
    /// let pending = RefCell::new(Default::default());
    /// let update: Update<'_, ()> = Update::new(Event::Input(InputEvent::key(Key::Enter)), &outcomes, &pending);
    ///
    /// assert_eq!(update.outcome_for(&[key("panel"), key("ok")]), Some(&Outcome::Activated));
    /// ```
    #[must_use]
    pub fn outcome_for(&self, path: &[Key]) -> Option<&Outcome> {
        let target = path.iter().fold(WidgetId::ROOT, |id, &key| id.child(key));
        self.outcomes
            .iter()
            .find(|(id, _)| *id == target)
            .map(|(_, outcome)| outcome)
    }
}

impl<M: Send + 'static> Update<'_, M> {
    /// Spawns an async effect (ADR 0005), buffered like a commit and handed to the
    /// runtime's [`Effects`] after `update` returns.
    ///
    /// The command's messages re-enter the loop as [`Event::Message`] in
    /// completion order; a grouped command applies cancel-previous against the
    /// runtime's group table (the debounced-search pattern). Multiple `spawn`
    /// calls in one update are queued in order.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::cell::RefCell;
    ///
    /// use rabbitui::app::{Event, Update};
    /// use rabbitui::effect::Command;
    /// use rabbitui_core::input::{InputEvent, Key};
    ///
    /// let pending = RefCell::new(Default::default());
    /// let update: Update<'_, u32> =
    ///     Update::new(Event::Input(InputEvent::key(Key::Enter)), &[], &pending);
    /// update.spawn(Command::future(async { 42 }));
    /// ```
    pub fn spawn(&self, cmd: Command<M>) {
        self.pending.borrow_mut().effects.push(cmd);
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
pub async fn run<S, M>(
    state: S,
    update: impl FnMut(&mut S, Update<'_, M>) -> ControlFlow<()>,
    view: impl Fn(&S, &mut Frame<'_>),
) -> Result<()>
where
    M: Send + 'static,
{
    from_fn(state, update, view).run().await
}

/// Startup configuration for an [`App`], read once by the runtime before the
/// loop starts.
///
/// Returned by [`App::config`]; the runtime reads it exactly once, before the
/// terminal opens. Runtime switching stays on [`Update`]
/// ([`set_mode`](Update::set_mode) / [`set_theme`](Update::set_theme)) — this
/// struct is *launch* state only. One method returning one struct keeps the
/// trait surface flat: new knobs are new fields with defaults, not new trait
/// methods (`docs/design/core-model-and-roadmap.md` §1).
///
/// The struct is `#[non_exhaustive]` so it can grow without breaking
/// `App::config` implementations downstream; construct it with
/// [`new`](Self::new) (or [`Default::default`]) and chain the builders:
///
/// ```
/// use rabbitui::app::Config;
/// use rabbitui_core::mode::Mode;
/// use rabbitui_core::theme::Theme;
///
/// let config = Config::new()
///     .theme(Theme::catppuccin_mocha())
///     .theme_file("theme.toml") // debug builds re-read this on change
///     .mode(Mode::inline(4))
///     .mouse(false);
/// let _ = config;
/// ```
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Config {
    /// The active [`Theme`] the loop threads into every frame. See
    /// [`theme`](Self::theme).
    pub theme: Theme,
    /// A TOML theme file layered over the base theme, hot-reloaded in debug
    /// builds. See [`theme_file`](Self::theme_file).
    pub theme_file: Option<PathBuf>,
    /// The startup screen [`Mode`]. See [`mode`](Self::mode).
    pub mode: Mode,
    /// Whether to capture the mouse, or `None` to default by mode (on in
    /// alt-screen, off in inline). See [`mouse`](Self::mouse).
    pub mouse: Option<bool>,
    /// Whether to install the tracing collector, or `None` to default by build
    /// profile (on in debug, off in release). See [`tracing`](Self::tracing).
    #[cfg(feature = "tracing")]
    pub tracing: Option<bool>,
    /// The ring the collector writes into, if the app supplied one to share
    /// with a `LogOverlay`. When `None`, the runtime makes its own so the
    /// close-flush still works. See [`log_handle`](Self::log_handle).
    #[cfg(feature = "tracing")]
    pub log_handle: Option<rabbitui_core::log::LogHandle>,
}

impl Config {
    /// The default configuration: the default theme, no theme file, alt-screen
    /// mode, by-mode mouse capture, and by-profile tracing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the active [`Theme`] the loop threads into every frame.
    ///
    /// If a [`theme_file`](Self::theme_file) is also set, this is the *base* the
    /// file's roles layer over (a file names only the roles it changes; the rest
    /// stay as this theme).
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Loads the active theme from a TOML file at `path`, layered over the base
    /// [`theme`](Self::theme).
    ///
    /// The file is loaded once at startup. In **debug builds** the loop then
    /// polls the file's modification time once per loop iteration and reloads it
    /// on change — Textual's dev loop without a file-watcher dependency (ADR
    /// 0007). Release builds load once and never re-stat. A load or parse error
    /// at startup fails [`App::run`]; a reload error mid-run is ignored so a
    /// half-saved edit never crashes the app (the previous theme stays).
    ///
    /// # Cost of hot reload
    ///
    /// The debug-build poll is **one `stat(2)` per loop iteration** — a metadata
    /// read, no file contents unless the mtime changed. The loop iterates once
    /// per input event, so at terminal event rates this is negligible; it is
    /// compiled out entirely in release builds via `cfg!(debug_assertions)`.
    #[must_use]
    pub fn theme_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.theme_file = Some(path.into());
        self
    }

    /// Sets the initial screen [`Mode`] — [`Mode::AltScreen`] (the default) or
    /// [`Mode::Inline`] with a bounded live tail.
    ///
    /// The mode can also change at runtime via
    /// [`Update::set_mode`](Update::set_mode); this sets the startup mode. In
    /// inline mode the app declares a frame sized to the live tail (the runtime
    /// caps it at `min(max_height, viewport_height)`), commits finalized lines
    /// with [`Update::commit`], and the terminal keeps native scrollback,
    /// selection, and copy above the tail (ADR 0013).
    #[must_use]
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets whether the app captures the mouse, overriding the by-mode default.
    ///
    /// Mouse capture is **on by default in alt-screen** and **off by default in
    /// inline** mode (slice-7 design note): capture steals the terminal's native
    /// scrollback scrolling, which inline mode exists to preserve, so enabling it
    /// there would defeat the mode's purpose. Alt-screen has no native scrollback
    /// to lose, so it captures by default. Call this to force the choice either
    /// way — `mouse(false)` in alt-screen for a keyboard-only app, or
    /// `mouse(true)` in inline if the app deliberately wants wheel events over
    /// scrollback.
    ///
    /// When on, the runtime enables mouse reporting (modes 1000 + 1006) at mode
    /// entry and disables it at leave; the panic/restore path disables it
    /// unconditionally regardless of this setting.
    #[must_use]
    pub fn mouse(mut self, mouse: bool) -> Self {
        self.mouse = Some(mouse);
        self
    }

    /// Sets whether the app installs rabbitui's [`tracing`](crate::log) collector
    /// as the global-default subscriber, overriding the by-profile default.
    ///
    /// The collector formats `tracing` events into a bounded ring the runtime
    /// owns; the `LogOverlay` widget renders that ring's tail. By default it is
    /// installed in **debug builds** and skipped in **release** builds (a dev
    /// affordance, off in shipped binaries) — pass `true` to force it on in
    /// release, or `false` to opt out in debug.
    ///
    /// # Install-once, never panic
    ///
    /// Installation is **only** attempted if no global-default subscriber is
    /// already set (`docs/design/arc2b-measurement-scroll.md`): if a host app, a
    /// test harness, or a prior app already installed one, this is a silent
    /// no-op, never a panic. So a program that sets up its own `tracing` stack and
    /// then runs a rabbitui [`App`] keeps its subscriber — but then the
    /// `LogOverlay` shows nothing, since rabbitui's ring never receives events.
    ///
    /// The filter honors `RABBITUI_LOG`, falling back to `RUST_LOG`
    /// ([`log::env_filter`](crate::log::env_filter)). On close, buffered `WARN` and
    /// above flush to stderr after the terminal is restored, so errors survive the
    /// alternate screen.
    #[cfg(feature = "tracing")]
    #[must_use]
    pub fn tracing(mut self, tracing: bool) -> Self {
        self.tracing = Some(tracing);
        self
    }

    /// Supplies the [`LogHandle`](rabbitui_core::log::LogHandle) the tracing
    /// collector writes into, so the app can render its tail with a `LogOverlay`.
    ///
    /// The runtime owns and shares the log ring (`docs/design/arc2b-measurement-scroll.md`):
    /// pass a clone here and keep another clone in your state, and the collector's
    /// events land in the same ring your `view` reads. Without this, the runtime
    /// still makes an internal ring so the close-flush works — an app just cannot
    /// display the overlay, since it never sees the handle.
    ///
    /// Only meaningful alongside [`tracing`](Self::tracing) (or its debug-build
    /// default); a supplied handle with tracing off is simply never written.
    #[cfg(feature = "tracing")]
    #[must_use]
    pub fn log_handle(mut self, handle: rabbitui_core::log::LogHandle) -> Self {
        self.log_handle = Some(handle);
        self
    }
}

/// An application: `Self` is the state, [`update`](Self::update) folds events
/// into it, [`view`](Self::view) declares its UI — plus defaulted lifecycle
/// hooks ([`init`](Self::init), [`global`](Self::global)), startup
/// [`config`](Self::config), and provided run entries.
///
/// This is the app-facing shape of the framework
/// (`docs/design/core-model-and-roadmap.md` §1; ADR 0001 amendment): the
/// declared-frame contract is the two required methods, everything else has a
/// default. Implement it on your state type and call [`run`](Self::run):
///
/// ```no_run
/// use std::ops::ControlFlow;
///
/// use rabbitui::app::{App, Event, Update};
/// use rabbitui_core::frame::Frame;
/// use rabbitui_core::id::key;
/// use rabbitui_core::input::Key;
/// use rabbitui_widgets::Text;
///
/// #[derive(Default)]
/// struct Counter {
///     count: i64,
/// }
///
/// impl App for Counter {
///     fn update(&mut self, update: Update<'_>) -> ControlFlow<()> {
///         if let Event::Input(input) = update.event() {
///             match input.as_key().map(|k| k.key) {
///                 Some(Key::Char('+')) => self.count += 1,
///                 Some(Key::Char('q')) => return ControlFlow::Break(()),
///                 _ => {}
///             }
///         }
///         ControlFlow::Continue(())
///     }
///
///     fn view(&self, frame: &mut Frame<'_>) {
///         let text = format!("count: {} (+ to add, q to quit)", self.count);
///         frame.widget(key("count"), frame.area(), &Text::new(&text));
///     }
/// }
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> rabbitui::app::Result<()> {
///     Counter::default().run().await
/// }
/// ```
///
/// Closure-shaped apps (tests, demos, one-screen tools) use [`from_fn`]
/// instead of implementing the trait — see [`FnApp`].
///
/// # The message type `M`
///
/// `M` is the app's effect-message type, delivered back as
/// [`Event::Message`] — see [`Command`] and [`Update::spawn`]. It is a
/// **defaulted generic parameter**, not an associated type (associated-type
/// defaults are unstable): message-less apps write `impl App for X` and never
/// see it. A type implementing two different `App<M>`s is legal but call
/// sites then need a turbofish to pick one.
///
/// # Not dyn-compatible, deliberately
///
/// The provided run entries are `async fn` (AFIT), so `dyn App` does not
/// compile. The loop is generic over the concrete app type; nothing in the
/// framework needs to box one.
#[allow(async_fn_in_trait)] // single-runtime facade: callers never re-spawn the returned futures
pub trait App<M = ()>: Sized
where
    M: Send + 'static,
{
    /// Folds one event — with its routed outcomes — into `self`, returning
    /// [`ControlFlow::Break`] to quit.
    ///
    /// Strictly synchronous (ADR 0005): async work leaves through
    /// [`Update::spawn`] and re-enters as [`Event::Message`]. Runs for every
    /// event *after* [`global`](Self::global) declined to break.
    fn update(&mut self, update: Update<'_, M>) -> ControlFlow<()>;

    /// Declares the UI for the current state into `frame`, every frame.
    ///
    /// Pure and synchronous: reads `&self`, paints, owns no state. The
    /// read/mutate split is compiler-enforced — `view` cannot touch what
    /// [`update`](Self::update) mutates mid-frame.
    fn view(&self, frame: &mut Frame<'_>);

    /// The app's opening [`Command`], spawned once at startup before
    /// [`Event::Started`] is delivered.
    ///
    /// Override it to start work at launch — begin a stream, kick off a load —
    /// without waiting for the first keypress. The default is
    /// [`Command::none()`], a true no-op. `init` and [`Event::Started`]
    /// coexist deliberately: closure apps ([`from_fn`]) cannot override hooks,
    /// so the event is their init path; trait apps may use either.
    fn init(&mut self) -> Command<M> {
        Command::none()
    }

    /// Runs before [`update`](Self::update) for **every** event; returning
    /// [`ControlFlow::Break`] quits without calling `update`.
    ///
    /// The home for app-wide chords — Ctrl-C quit, a global help toggle —
    /// that must fire even when `update` early-returns (a modal open, a
    /// wizard step). Routing has already run, so [`Update::consumed`] and
    /// [`Update::action`] work; the borrow is shared (`&Update`) but all
    /// `Update` methods take `&self`, so it can spawn, commit, and focus.
    /// On `Break`, the update's pending set still drains (effects spawn,
    /// commits flush).
    fn global(&mut self, _update: &Update<'_, M>) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    /// The app's startup [`Config`], read once by the runtime before the loop
    /// starts.
    ///
    /// The default is [`Config::default()`]. Runtime switching stays on
    /// [`Update`] ([`set_mode`](Update::set_mode) /
    /// [`set_theme`](Update::set_theme)) — this is launch state only.
    fn config(&self) -> Config {
        Config::default()
    }

    /// Runs the application loop over the controlling terminal until
    /// [`update`](Self::update) (or [`global`](Self::global)) returns
    /// [`ControlFlow::Break`].
    ///
    /// Opens the terminal, reads [`config`](Self::config) once, spawns
    /// [`init`](Self::init)'s command, delivers [`Event::Started`], then
    /// drives update → view → diff → render, restoring the terminal on every
    /// exit path (including panics).
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal, input, size polling, or rendering
    /// fails, or if a configured theme file cannot be loaded or parsed at
    /// startup.
    async fn run(self) -> Result<()> {
        let terminal = Terminal::open().await?;
        run_on(self, terminal).await
    }

    /// Runs the app over a caller-supplied [`TerminalDevice`](qwertty::TerminalDevice)
    /// instead of the real controlling terminal.
    ///
    /// The headless-testing / embedding seam: pass a [`qwertty::FakeDevice`] (a
    /// socketpair) and the *whole* [`run`](Self::run) loop executes with no pty,
    /// so tests can drive the real `update` path and assert on emitted bytes
    /// (`docs/design/fakedevice-e2e-harness.md`). Unlike [`run`](Self::run) this
    /// installs no panic-restore hook — a fake device has nothing to strand, and
    /// a test wants panics to surface.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be built over the device, the theme
    /// file cannot be loaded, or the loop hits a terminal I/O error.
    async fn run_over_device<D: qwertty::TerminalDevice>(self, device: D) -> Result<()> {
        let terminal = Terminal::from_device(device)?;
        run_on(self, terminal).await
    }
}

/// Shared setup for [`App::run`] and [`App::run_over_device`]: reads the app's
/// [`Config`] once, installs tracing, resolves the initial theme, then enters
/// [`run_loop`] over the given terminal.
async fn run_on<M, A, D>(app: A, terminal: Terminal<D>) -> Result<()>
where
    M: Send + 'static,
    A: App<M>,
    D: qwertty::TerminalDevice,
{
    let config = app.config();

    // Install the tracing collector before the loop starts, so startup events
    // are captured. The default is by build profile (debug on, release off); an
    // explicit `Config::tracing` overrides it. Installation is a no-op if a
    // global default is already set — never a panic. The returned handle (if we
    // installed and hold one) is flushed on close, after the terminal is
    // restored, so WARN+ survives the alternate screen.
    #[cfg(feature = "tracing")]
    let flush_handle = install_tracing(config.tracing, config.log_handle.clone());

    // Load the initial theme from the file (if any), layered over the base.
    // A startup error is fatal; a mid-run reload error is not (see ThemeWatcher).
    let watcher = ThemeWatcher::new(config.theme_file, config.theme)?;

    run_loop(
        terminal,
        app,
        watcher,
        config.mode,
        config.mouse,
        #[cfg(feature = "tracing")]
        flush_handle,
    )
    .await
}

/// A closure-shaped [`App`]: owned state plus an `update` and a `view`
/// function, built by [`from_fn`].
///
/// The zero-ceremony adapter for tests, demos, and one-screen tools — the std
/// pattern (`iter::from_fn`, `future::poll_fn`). Anything that outgrows two
/// closures implements [`App`] directly; the closure form is a strict subset.
/// Configuration chains through the `with_*` builders (prefixed so they cannot
/// shadow the trait's method names):
///
/// ```no_run
/// use std::ops::ControlFlow;
///
/// use rabbitui::app::{App as _, Update};
/// use rabbitui::from_fn;
/// use rabbitui_core::frame::Frame;
/// use rabbitui_core::id::key;
/// use rabbitui_core::theme::Theme;
/// use rabbitui_widgets::Text;
///
/// # async fn demo() -> rabbitui::app::Result<()> {
/// from_fn(
///     (),
///     |_state: &mut (), _update: Update<'_>| ControlFlow::Break(()),
///     |_state: &(), frame: &mut Frame<'_>| {
///         frame.widget(key("hi"), frame.area(), &Text::new("hi"));
///     },
/// )
/// .with_theme(Theme::catppuccin_mocha())
/// .run()
/// .await
/// # }
/// ```
pub struct FnApp<S, U, V, M = ()> {
    state: S,
    update: U,
    view: V,
    config: Config,
    /// Ties the app to its message type without owning one; the `fn() -> M`
    /// form keeps `FnApp` `Send`-agnostic and variance-correct.
    _marker: std::marker::PhantomData<fn() -> M>,
}

/// Creates a closure-shaped [`App`] from owned `state`, an `update`, and a
/// `view` — see [`FnApp`].
///
/// Behaves exactly like implementing [`App`] with those two bodies and default
/// hooks; closure apps take startup effects via [`Event::Started`] (they cannot
/// override [`App::init`]). The free [`run`] is `from_fn(...).run()`.
///
/// # Examples
///
/// ```no_run
/// use std::ops::ControlFlow;
///
/// use rabbitui::app::{App as _, Update};
/// use rabbitui::from_fn;
/// use rabbitui_core::frame::Frame;
/// use rabbitui_core::id::key;
/// use rabbitui_widgets::Text;
///
/// # async fn demo() -> rabbitui::app::Result<()> {
/// from_fn(
///     (),
///     |_state: &mut (), _update: Update<'_>| ControlFlow::Break(()),
///     |_state: &(), frame: &mut Frame<'_>| {
///         frame.widget(key("greeting"), frame.area(), &Text::new("hi"));
///     },
/// )
/// .run()
/// .await
/// # }
/// ```
pub fn from_fn<S, U, V, M>(state: S, update: U, view: V) -> FnApp<S, U, V, M>
where
    U: FnMut(&mut S, Update<'_, M>) -> ControlFlow<()>,
    V: Fn(&S, &mut Frame<'_>),
    M: Send + 'static,
{
    FnApp {
        state,
        update,
        view,
        config: Config::default(),
        _marker: std::marker::PhantomData,
    }
}

impl<S, U, V, M> FnApp<S, U, V, M>
where
    U: FnMut(&mut S, Update<'_, M>) -> ControlFlow<()>,
    V: Fn(&S, &mut Frame<'_>),
    M: Send + 'static,
{
    /// Sets [`Config::theme`] — the active theme the loop threads into every
    /// frame, and the base a [`with_theme_file`](Self::with_theme_file) layers
    /// over.
    #[must_use]
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.config = self.config.theme(theme);
        self
    }

    /// Sets [`Config::theme_file`] — a TOML theme file layered over the base
    /// theme, hot-reloaded in debug builds.
    #[must_use]
    pub fn with_theme_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.config = self.config.theme_file(path);
        self
    }

    /// Sets [`Config::mode`] — the startup screen [`Mode`].
    #[must_use]
    pub fn with_mode(mut self, mode: Mode) -> Self {
        self.config = self.config.mode(mode);
        self
    }

    /// Sets [`Config::mouse`] — whether to capture the mouse, overriding the
    /// by-mode default.
    #[must_use]
    pub fn with_mouse(mut self, mouse: bool) -> Self {
        self.config = self.config.mouse(mouse);
        self
    }

    /// Sets [`Config::tracing`] — whether to install the tracing collector,
    /// overriding the by-profile default.
    #[cfg(feature = "tracing")]
    #[must_use]
    pub fn with_tracing(mut self, tracing: bool) -> Self {
        self.config = self.config.tracing(tracing);
        self
    }

    /// Sets [`Config::log_handle`] — the ring the tracing collector writes
    /// into, shared with the app for a `LogOverlay`.
    #[cfg(feature = "tracing")]
    #[must_use]
    pub fn with_log_handle(mut self, handle: rabbitui_core::log::LogHandle) -> Self {
        self.config = self.config.log_handle(handle);
        self
    }
}

impl<S, U, V, M> App<M> for FnApp<S, U, V, M>
where
    U: FnMut(&mut S, Update<'_, M>) -> ControlFlow<()>,
    V: Fn(&S, &mut Frame<'_>),
    M: Send + 'static,
{
    fn update(&mut self, update: Update<'_, M>) -> ControlFlow<()> {
        (self.update)(&mut self.state, update)
    }

    fn view(&self, frame: &mut Frame<'_>) {
        (self.view)(&self.state, frame)
    }

    fn config(&self) -> Config {
        self.config.clone()
    }
}

/// The headless-testable core of [`App::run`]: drives the real event loop over
/// any [`TerminalDevice`](qwertty::TerminalDevice).
///
/// Production [`App::run`] passes a real terminal ([`Terminal::open`]); the
/// FakeDevice harness passes [`Terminal::from_device`] over a socketpair, so the
/// *same* loop — the real `update` path, routing, effects, mode switches, and
/// paint scheduling — runs headlessly in CI. That is the whole point: the bug
/// classes that only surfaced on real hardware (a declare-then-focus panic in
/// `update`, inline-commit timing) become CI-catchable
/// (`docs/design/fakedevice-e2e-harness.md`).
///
/// Every event delivery is the **global-then-update** sequence: one `Update` is
/// constructed, lent to [`App::global`], and — unless `global` broke — moved
/// into [`App::update`]. On a `global` break, `update` is not called but the
/// pending set still drains (effects spawn, commits flush).
async fn run_loop<A, M, D>(
    mut terminal: Terminal<D>,
    mut app: A,
    mut watcher: ThemeWatcher,
    mode: Mode,
    mouse: Option<bool>,
    #[cfg(feature = "tracing")] flush_handle: Option<rabbitui_core::log::LogHandle>,
) -> Result<()>
where
    A: App<M>,
    M: Send + 'static,
    D: qwertty::TerminalDevice,
{
    let mut theme = watcher.theme();
    let mut viewport = terminal.size()?;

    // The render engine for the active mode; `enter` produces the mode-entry
    // bytes (alt-screen switch, or inline cursor-hide). The state store,
    // focus, and theme persist across iterations.
    let mut engine = ModeEngine::new(mode, mouse);
    let mut store = StateStore::new();
    let mut focus = Focus::new();

    // The effect runtime (ADR 0005): spawn tables, group abort, and the
    // mailbox. It touches no terminal, so it is one `select!` arm below and is
    // unit-tested headless in `crate::effect`.
    let mut effects: Effects<M> = Effects::new();

    // Buffers are sized for the mode: the full viewport in alt-screen, or the
    // bounded live-tail height in inline. `front` is what the terminal shows,
    // `back` the frame being built. `apply_mode_switch` and the resize branch
    // re-size both when the mode or viewport changes.
    let initial_size = engine.buffer_size(viewport);
    let mut front = Buffer::new(initial_size);
    let mut back = Buffer::new(initial_size);

    // Enter the mode, then render the first frame.
    terminal.write_bytes(&engine.enter()).await?;
    let (mut facts, mut handlers) = draw(&mut back, &mut store, focus, &theme, |f| app.view(f));
    focus.reconcile(&facts);
    terminal
        .write_bytes(&engine.render(&back, &front, &[]))
        .await?;
    std::mem::swap(&mut front, &mut back);

    // The frame budget (~60fps): the earliest instant a redraw may paint. A
    // burst of stream messages is absorbed into one frame — after handling an
    // event the loop drains everything already queued before painting, and if
    // the budget has not elapsed it arms a trailing deadline so the last state
    // always paints (ADR 0005 / tui2's coalescing requester).
    let frame_budget = Duration::from_micros(16_667);
    // When `Some`, a paint is pending and must land at this instant at the
    // latest; the `select!` arms a sleep to it.
    let mut next_paint: Option<tokio::time::Instant> = None;
    // Whether state changed since the last paint (a redraw is owed).
    let mut dirty = false;
    let mut last_paint = tokio::time::Instant::now();

    // Commits and a requested mode switch accumulate across every `update`
    // handled since the last paint (an input, plus a coalesced burst of effect
    // messages), then flush together when the frame lands.
    let mut commits_buf: Vec<CommitLine> = Vec::new();
    let mut set_mode_buf: Option<Mode> = None;
    let mut set_theme_buf: Option<Theme> = None;

    // The unapplied remainder of the *previous* update's pending set — a focus
    // request that could not be honored against the frame it was made on (the
    // declare-then-focus case). It is carried across exactly one frame: after
    // the next redraw, the remainder retries against the fresh facts, and only
    // then does it fail loudly if still unhonored (slice-7 carry-forward).
    let mut widget_remainder = WidgetPending::new();

    // The one-shot startup tick has not been delivered yet; the first loop
    // iteration synthesizes a `Wake::Started` instead of blocking on input.
    let mut started = false;

    loop {
        // Debug-build hot reload: one stat per iteration. On a changed mtime,
        // re-read and re-parse; keep the old theme on any reload error.
        if watcher.poll_changed() {
            theme = watcher.theme();
            dirty = true;
        }

        // Wait for the next wake: an input event, an effect result, or the
        // trailing paint deadline. Biased so input and effects are preferred
        // over the timer, and effects are drained ahead of blocking on input.
        let wake = if !started {
            // First iteration: deliver the startup tick before waiting on
            // any real wake source, so init runs before the first keypress.
            started = true;
            Wake::Started
        } else {
            let deadline = next_paint;
            tokio::select! {
                biased;
                // An effect produced a message or failed.
                item = effects.recv() => match item {
                    Some(outbox) => Wake::Effect(outbox),
                    // The mailbox can only close if every sender is gone, which
                    // cannot happen while `effects` holds its own `tx`; treat a
                    // close defensively as "nothing to do, keep waiting on input".
                    None => Wake::Idle,
                },
                // A decoded input event (or a resize the size-poll synthesizes).
                input = terminal.next_event() => Wake::Input(Box::new(input?)),
                // The trailing frame deadline elapsed; time to paint.
                () = sleep_until(deadline), if deadline.is_some() => Wake::Paint,
            }
        };

        // Fold this wake into state through `update`. Each event source builds
        // one `Update`; the pending sink buffers commits, mode switches,
        // effects, and between-frames widget commands / focus.
        let mut broke = false;
        match wake {
            Wake::Started => {
                // Spawn the app's opening command first (`App::init`; a
                // `Command::none()` default is a no-op), then deliver the
                // one-shot startup event against the frame already on
                // screen and let the pending set (spawned effects, widget
                // commands, a mode switch) drain like any other update.
                // `dirty` forces a repaint if init changed state without
                // spawning an effect to wake the loop.
                effects.spawn(app.init());
                let pending = RefCell::new(Pending::default());
                let ctx = Update::new(Event::Started, &[], &pending).with_store(&store);
                broke = if app.global(&ctx).is_break() {
                    true
                } else {
                    app.update(ctx).is_break()
                };
                drain_pending(
                    pending.into_inner(),
                    &mut effects,
                    &mut store,
                    &facts,
                    &mut focus,
                    &mut widget_remainder,
                    &mut commits_buf,
                    &mut set_mode_buf,
                    &mut set_theme_buf,
                );
                dirty = true;
            }
            Wake::Idle => {}
            Wake::Paint => {
                // The deadline fired; fall through to the paint below.
                next_paint = None;
            }
            Wake::Input(input) => {
                // Poll for a resize (substrate has no resize event; see
                // `Event`). On a change, re-lay-out for the new viewport and
                // deliver the resize before the input.
                let new_viewport = terminal.size()?;
                if new_viewport != viewport {
                    viewport = new_viewport;
                    let size = engine.buffer_size(viewport);
                    front.resize(size);
                    back.resize(size);
                    engine.force_repaint();
                    let pending = RefCell::new(Pending::default());
                    let ctx =
                        Update::new(Event::Resize(viewport), &[], &pending).with_store(&store);
                    broke = if app.global(&ctx).is_break() {
                        true
                    } else {
                        app.update(ctx).is_break()
                    };
                    drain_pending(
                        pending.into_inner(),
                        &mut effects,
                        &mut store,
                        &facts,
                        &mut focus,
                        &mut widget_remainder,
                        &mut commits_buf,
                        &mut set_mode_buf,
                        &mut set_theme_buf,
                    );
                    dirty = true;
                }

                if !broke && let Some(event) = crate::input::from_qwertty(&input) {
                    let result = route(&facts, &handlers, &mut focus, &mut store, &event);
                    let pending = RefCell::new(Pending::default());
                    let ctx = Update::new(Event::Input(event), &result.outcomes, &pending)
                        .with_consumed(result.consumed)
                        .with_focus(focus.current())
                        .with_store(&store);
                    broke = if app.global(&ctx).is_break() {
                        true
                    } else {
                        app.update(ctx).is_break()
                    };
                    drain_pending(
                        pending.into_inner(),
                        &mut effects,
                        &mut store,
                        &facts,
                        &mut focus,
                        &mut widget_remainder,
                        &mut commits_buf,
                        &mut set_mode_buf,
                        &mut set_theme_buf,
                    );
                    dirty = true;
                }
            }
            Wake::Effect(outbox) => {
                broke = deliver_effect(
                    outbox,
                    &mut app,
                    &mut effects,
                    &mut store,
                    &facts,
                    &mut focus,
                    &mut widget_remainder,
                    &mut commits_buf,
                    &mut set_mode_buf,
                    &mut set_theme_buf,
                );
                dirty = true;

                // Coalescing drain: absorb every already-queued effect result
                // into this one frame with a biased `try_recv` loop, so a flood
                // of stream messages is one render, not one render per message.
                while !broke {
                    let Some(next) = effects.try_recv() else {
                        break;
                    };
                    broke = deliver_effect(
                        next,
                        &mut app,
                        &mut effects,
                        &mut store,
                        &facts,
                        &mut focus,
                        &mut widget_remainder,
                        &mut commits_buf,
                        &mut set_mode_buf,
                        &mut set_theme_buf,
                    );
                }
            }
        }

        if broke {
            // Flush any commits made in this final update into scrollback
            // before leaving (they belong in history), then tear down.
            let commits = std::mem::take(&mut commits_buf);
            let set_mode = set_mode_buf.take();
            let remaining = apply_mode_switch(
                &mut terminal,
                &mut engine,
                set_mode,
                commits,
                viewport,
                &mut front,
                &mut back,
            )
            .await?;
            if !remaining.is_empty() {
                let empty = Buffer::new(Size::new(viewport.width, 0));
                terminal
                    .write_bytes(&engine.render(&empty, &front, &remaining))
                    .await?;
            }
            let result = leave(terminal, &mut engine).await;
            // Now that the terminal is restored, flush buffered WARN+ to
            // stderr so errors and warnings survive the alternate screen.
            #[cfg(feature = "tracing")]
            if let Some(handle) = &flush_handle {
                crate::log::flush_warnings(handle);
            }
            return result;
        }

        if !dirty {
            continue;
        }

        // Respect the frame budget: if the last paint was recent, arm a
        // trailing deadline instead of painting now, so a burst coalesces into
        // one frame at the budget boundary. When the deadline (or the next
        // event) arrives, `dirty` is still set and we paint.
        let now = tokio::time::Instant::now();
        if now.duration_since(last_paint) < frame_budget {
            next_paint = Some(last_paint + frame_budget);
            continue;
        }

        // Apply any buffered mode switch, flushing pending commits into
        // scrollback *before* an alt-screen entry. Returns the commits the
        // frame render should still flush (inline target; empty otherwise).
        let commits = std::mem::take(&mut commits_buf);
        let set_mode = set_mode_buf.take();
        // Apply a buffered runtime theme switch before this frame's draw.
        if let Some(new_theme) = set_theme_buf.take() {
            theme = new_theme;
        }
        let frame_commits = apply_mode_switch(
            &mut terminal,
            &mut engine,
            set_mode,
            commits,
            viewport,
            &mut front,
            &mut back,
        )
        .await?;

        // Draw the next frame, apply between-frames widget commands and the
        // focus request against its facts (the shared `core::pending` path,
        // identical to `TestApp`), then paint.
        back.reset();
        let drawn = draw(&mut back, &mut store, focus, &theme, |f| app.view(f));
        facts = drawn.0;
        handlers = drawn.1;
        // Retry the carried-forward remainder (a declare-then-focus request
        // that missed its own frame) against this fresh frame's facts. This is
        // the second and final attempt: `apply` fails loudly if the target is
        // still not present-and-focusable.
        if !widget_remainder.is_empty() {
            std::mem::take(&mut widget_remainder).apply(&mut store, &facts, &mut focus);
        }
        focus.reconcile(&facts);
        terminal
            .write_bytes(&engine.render(&back, &front, &frame_commits))
            .await?;
        std::mem::swap(&mut front, &mut back);

        dirty = false;
        next_paint = None;
        last_paint = tokio::time::Instant::now();
    }
}

/// Installs the tracing collector per the app's setting, returning the ring
/// handle to flush on close (or `None` when tracing is off, or when a global
/// default was already installed and we hold no shared handle).
///
/// The on/off default is by build profile: debug on, release off. When on, the
/// collector writes into the app's supplied [`LogHandle`] if it gave one
/// ([`Config::log_handle`]), else an internal ring made here so the close-flush
/// still works. Installation is attempted only if no global default is set; a loss is
/// silent (`docs/design/arc2b-measurement-scroll.md`).
///
/// The returned handle is flushed for WARN+ on close only when *this* call
/// installed the collector — if a global default already existed, our ring never
/// received events, so there is nothing of ours to flush.
#[cfg(feature = "tracing")]
fn install_tracing(
    setting: Option<bool>,
    supplied: Option<rabbitui_core::log::LogHandle>,
) -> Option<rabbitui_core::log::LogHandle> {
    let on = setting.unwrap_or(cfg!(debug_assertions));
    if !on {
        return None;
    }
    let handle = supplied.unwrap_or_default();
    if crate::log::try_install(handle.clone()) {
        // We won the install: our ring receives events, so flush it on close.
        Some(handle)
    } else {
        // A global default already exists; our ring stays empty. Nothing to flush.
        None
    }
}

/// What woke the loop's `select!`: an input, an effect result, the paint
/// deadline, or a spurious idle (a defensively-handled closed mailbox).
enum Wake<M> {
    /// The one-shot startup tick: deliver [`Event::Started`] before the first
    /// real wake so a self-starting app can spawn its initial `Command`.
    Started,
    /// A raw substrate input event from the terminal (boxed: qwertty's event is
    /// larger than the other small variants, so this keeps `Wake` compact).
    Input(Box<qwertty::Event>),
    /// An effect result (a message or a contained failure).
    Effect(Outbox<M>),
    /// The trailing frame deadline elapsed.
    Paint,
    /// Nothing to do (the mailbox closed, which cannot happen in practice).
    Idle,
}

/// Sleeps until `deadline`, or parks forever if there is none.
///
/// The trailing-paint arm of the loop's `select!` is guarded by
/// `if deadline.is_some()`, so the `None` branch here never actually runs; it
/// exists only so the arm has a concrete future to name.
async fn sleep_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Drains one update's [`Pending`] into the running loop: queues effects onto the
/// runtime, applies between-frames widget commands and the focus request through
/// the shared [`core::pending`](rabbitui_core::pending) path, and accumulates
/// commits / a mode switch for the next paint.
///
/// Widget commands and focus apply against the *last drawn* frame's `facts` and
/// the store immediately (a redraw follows because the loop marks itself dirty),
/// so a cleared field or a moved focus shows on the next frame — the between-frames
/// semantics, using the exact function `TestApp` uses.
///
/// A focus request that cannot be honored against the last-drawn facts (the
/// declare-then-focus case, where the target only appears in the frame this
/// update triggers) is **not** dropped: [`Pending::apply_deferred`] returns it as
/// an unapplied remainder, which is folded into `remainder` for the loop to retry
/// once against the *next* frame's facts before asserting (slice-7 carry-forward).
#[allow(clippy::too_many_arguments)]
fn drain_pending<M: Send + 'static>(
    pending: Pending<M>,
    effects: &mut Effects<M>,
    store: &mut StateStore,
    facts: &FrameFacts,
    focus: &mut Focus,
    remainder: &mut WidgetPending,
    commits_buf: &mut Vec<CommitLine>,
    set_mode_buf: &mut Option<Mode>,
    set_theme_buf: &mut Option<Theme>,
) {
    let Pending {
        commits,
        set_mode,
        set_theme,
        effects: cmds,
        widget,
        guarded,
    } = pending;
    for cmd in cmds {
        effects.spawn(cmd);
    }
    if guarded {
        // Best-effort apply (try_focus/try_command): a request for an absent widget
        // is a soft skip, not a panic. No carry-forward retry — a guarded request
        // is terminal. Core is tracing-free, so warn here off a non-clean report.
        let _report = widget.apply_guarded(store, facts, focus);
        #[cfg(feature = "tracing")]
        if !_report.is_clean() {
            tracing::warn!(
                skipped_commands = _report.skipped_commands.len(),
                skipped_focus = ?_report.skipped_focus,
                "guarded update: dropped requests for widgets not declared this frame"
            );
        }
    } else {
        let unapplied = widget.apply_deferred(store, facts, focus);
        remainder.extend(unapplied);
    }
    commits_buf.extend(commits);
    if set_mode.is_some() {
        *set_mode_buf = set_mode;
    }
    if set_theme.is_some() {
        *set_theme_buf = set_theme;
    }
}

/// Delivers one effect result to the app and drains its pending, returning
/// whether the app asked to break.
///
/// A [`Outbox::Message`] becomes [`Event::Message`]; a [`Outbox::Failed`] becomes
/// [`Event::EffectFailed`]. Either way the app sees it in the one serialized
/// loop — the same global-then-update sequence as an input event — and may
/// itself spawn more effects or command widgets.
#[allow(clippy::too_many_arguments)]
fn deliver_effect<A, M>(
    outbox: Outbox<M>,
    app: &mut A,
    effects: &mut Effects<M>,
    store: &mut StateStore,
    facts: &FrameFacts,
    focus: &mut Focus,
    remainder: &mut WidgetPending,
    commits_buf: &mut Vec<CommitLine>,
    set_mode_buf: &mut Option<Mode>,
    set_theme_buf: &mut Option<Theme>,
) -> bool
where
    A: App<M>,
    M: Send + 'static,
{
    let event = match outbox {
        Outbox::Message(message) => Event::Message(message),
        Outbox::Failed(error) => Event::EffectFailed(error),
    };
    let pending = RefCell::new(Pending::default());
    let ctx = Update::new(event, &[], &pending).with_store(&*store);
    let broke = if app.global(&ctx).is_break() {
        true
    } else {
        app.update(ctx).is_break()
    };
    drain_pending(
        pending.into_inner(),
        effects,
        store,
        facts,
        focus,
        remainder,
        commits_buf,
        set_mode_buf,
        set_theme_buf,
    );
    broke
}

/// Writes the active engine's teardown bytes, then closes the terminal.
///
/// The engine's `leave` frame does the mode-specific restore (leave alt screen,
/// or drop below the inline tail); [`Terminal::close`] then leaves raw mode with
/// the unconditional alt-screen-leave backstop.
async fn leave<D: qwertty::TerminalDevice>(
    mut terminal: Terminal<D>,
    engine: &mut ModeEngine,
) -> Result<()> {
    terminal.write_bytes(&engine.leave()).await?;
    terminal.close().await
}

/// Applies a requested mode switch (if any), consuming `commits`, and returns
/// the commits the *current* frame render should still flush.
///
/// Ordering matters (slice-5 design note): when switching to alt-screen, commits
/// flush into native scrollback through the inline engine *before* the alt-screen
/// entry, so content committed just before the switch is not lost behind the
/// alternate screen. When switching to (or staying in) inline, the commits are
/// returned so the caller's frame render lands them above the fresh live tail.
/// Alt-screen has no scrollback, so with no switch to inline the commits are
/// simply dropped.
async fn apply_mode_switch<D: qwertty::TerminalDevice>(
    terminal: &mut Terminal<D>,
    engine: &mut ModeEngine,
    set_mode: Option<Mode>,
    commits: Vec<CommitLine>,
    viewport: Size,
    front: &mut Buffer,
    back: &mut Buffer,
) -> Result<Vec<CommitLine>> {
    let switching = match set_mode {
        Some(target) if target != engine.mode() => Some(target),
        _ => None,
    };

    let Some(target) = switching else {
        // No switch: an inline engine flushes the commits this frame; an
        // alt-screen engine has no scrollback, so they are dropped.
        return Ok(if engine.is_inline() {
            commits
        } else {
            Vec::new()
        });
    };

    match (engine.mode(), target) {
        // Leaving inline for alt: flush pending commits into scrollback through
        // the inline engine first (an empty tail, so only history is written),
        // then tear the inline region down.
        (Mode::Inline { .. }, Mode::AltScreen) => {
            if !commits.is_empty() {
                let empty = Buffer::new(Size::new(viewport.width, 0));
                terminal
                    .write_bytes(&engine.render(&empty, front, &commits))
                    .await?;
            }
            terminal.write_bytes(&engine.leave_for_switch()).await?;
        }
        // Any other switch tears the current mode down first.
        _ => {
            terminal.write_bytes(&engine.leave_for_switch()).await?;
        }
    }

    *engine = ModeEngine::new(target, engine.mouse_override);
    let size = engine.buffer_size(viewport);
    front.resize(size);
    back.resize(size);
    terminal.write_bytes(&engine.enter()).await?;

    // Entering inline: hand the commits back so the caller's frame render lands
    // them above the new live tail. Entering alt: the commits (if any) were
    // flushed above, so none remain.
    Ok(if engine.is_inline() {
        commits
    } else {
        Vec::new()
    })
}

/// The active render engine, dispatched by [`Mode`].
///
/// Wraps [`AltEngine`] or [`InlineEngine`] behind one uniform interface the loop
/// drives: [`buffer_size`](Self::buffer_size) sizes the frame buffers for the
/// mode, [`enter`](Self::enter)/[`leave`](Self::leave) produce mode-transition
/// bytes, and [`render`](Self::render) produces one frame's bytes. Commits are
/// meaningful only to the inline engine; the alt engine ignores them (the loop
/// flushes them before entering alt-screen).
#[derive(Debug)]
struct ModeEngine {
    kind: ModeEngineKind,
    /// The app's mouse-capture override (`None` = default by mode). Carried
    /// across mode switches so a runtime `set_mode` keeps the app's choice.
    mouse_override: Option<bool>,
}

/// The active render engine, dispatched by [`Mode`].
#[derive(Debug)]
enum ModeEngineKind {
    /// The alternate-screen engine and its declared mode.
    Alt(AltEngine),
    /// The inline engine and its `max_height`.
    Inline {
        engine: InlineEngine,
        max_height: u16,
    },
}

impl ModeEngine {
    /// Builds the engine for `mode`, carrying the app's mouse-capture override.
    fn new(mode: Mode, mouse_override: Option<bool>) -> Self {
        let kind = match mode {
            Mode::AltScreen => ModeEngineKind::Alt(AltEngine::new()),
            Mode::Inline { max_height } => ModeEngineKind::Inline {
                engine: InlineEngine::new(),
                max_height,
            },
        };
        Self {
            kind,
            mouse_override,
        }
    }

    /// Whether this engine captures the mouse: the app override, or the by-mode
    /// default (on in alt-screen, off in inline — the slice-7 design note).
    fn mouse_capture(&self) -> bool {
        self.mouse_override.unwrap_or_else(|| !self.is_inline())
    }

    /// The mode this engine renders.
    fn mode(&self) -> Mode {
        match &self.kind {
            ModeEngineKind::Alt(_) => Mode::AltScreen,
            ModeEngineKind::Inline { max_height, .. } => Mode::Inline {
                max_height: *max_height,
            },
        }
    }

    /// Whether this is the inline engine.
    fn is_inline(&self) -> bool {
        matches!(self.kind, ModeEngineKind::Inline { .. })
    }

    /// The frame-buffer size for this mode at `viewport`: the full viewport in
    /// alt-screen, or the bounded live-tail height (`min(max_height, viewport
    /// height)`) at full width in inline.
    fn buffer_size(&self, viewport: Size) -> Size {
        match &self.kind {
            ModeEngineKind::Alt(_) => viewport,
            ModeEngineKind::Inline { max_height, .. } => {
                Size::new(viewport.width, (*max_height).min(viewport.height))
            }
        }
    }

    /// The mode-entry bytes, prefixed with mouse-enable when capture is on.
    fn enter(&mut self) -> Vec<u8> {
        let mouse = self.mouse_capture();
        let mut bytes = match &mut self.kind {
            ModeEngineKind::Alt(engine) => engine.enter(),
            ModeEngineKind::Inline { engine, .. } => engine.enter(),
        };
        if mouse {
            bytes.extend_from_slice(crate::encode::ENABLE_MOUSE);
        }
        bytes
    }

    /// The mode-teardown bytes, suffixed with mouse-disable when capture was on.
    fn leave(&mut self) -> Vec<u8> {
        let mouse = self.mouse_capture();
        let mut bytes = match &mut self.kind {
            ModeEngineKind::Alt(engine) => engine.leave(),
            ModeEngineKind::Inline { engine, .. } => engine.leave(),
        };
        if mouse {
            bytes.extend_from_slice(crate::encode::DISABLE_MOUSE);
        }
        bytes
    }

    /// Tears the current mode down for a **switch** to another mode (not program
    /// exit): an inline engine reclaims its tail region instead of leaving it as
    /// scrollback (which would duplicate the tail on switch-back); an alt engine's
    /// teardown is the same as [`leave`](Self::leave).
    fn leave_for_switch(&mut self) -> Vec<u8> {
        let mouse = self.mouse_capture();
        let mut bytes = match &mut self.kind {
            ModeEngineKind::Alt(engine) => engine.leave(),
            ModeEngineKind::Inline { engine, .. } => engine.leave_for_switch(),
        };
        if mouse {
            bytes.extend_from_slice(crate::encode::DISABLE_MOUSE);
        }
        bytes
    }

    /// Forces the next render to fully repaint (resize / desync recovery).
    fn force_repaint(&mut self) {
        if let ModeEngineKind::Inline { engine, .. } = &mut self.kind {
            engine.force_repaint();
        }
    }

    /// One frame's bytes: the alt engine diffs `current` against `previous`; the
    /// inline engine flushes `commits` then paints `current` as the live tail.
    fn render(&mut self, current: &Buffer, previous: &Buffer, commits: &[CommitLine]) -> Vec<u8> {
        match &mut self.kind {
            ModeEngineKind::Alt(engine) => engine.render(current, previous),
            ModeEngineKind::Inline { engine, .. } => engine.render(current, commits),
        }
    }
}

/// Owns a theme file's path and last-seen mtime, reloading on change.
///
/// Encapsulates the hot-reload policy so [`App::run`]'s loop stays readable:
/// startup load is validated (a bad file fails `run`), and reloads are
/// best-effort (a bad reload keeps the last good theme). With no file the watcher
/// is inert — [`poll_changed`](ThemeWatcher::poll_changed) always returns false —
/// and holds the base theme.
struct ThemeWatcher {
    file: Option<PathBuf>,
    base: Theme,
    theme: Theme,
    last_modified: Option<std::time::SystemTime>,
}

impl ThemeWatcher {
    /// Loads the initial theme (fatal on error) and records the file's mtime.
    fn new(file: Option<PathBuf>, base: Theme) -> Result<Self> {
        let (theme, last_modified) = match &file {
            Some(path) => (load(path, base)?, modified(path)),
            None => (base, None),
        };
        Ok(Self {
            file,
            base,
            theme,
            last_modified,
        })
    }

    /// The current theme.
    fn theme(&self) -> Theme {
        self.theme
    }

    /// In debug builds, stats the file once and reloads if its mtime changed,
    /// returning whether the theme was replaced. Always false with no file or in
    /// release builds (where the theme loads once and never re-stats).
    fn poll_changed(&mut self) -> bool {
        if !cfg!(debug_assertions) {
            return false;
        }
        let Some(path) = &self.file else { return false };
        let now = modified(path);
        if now == self.last_modified {
            return false;
        }
        self.last_modified = now;
        // Best-effort: a parse error mid-edit keeps the last good theme.
        match load(path, self.base) {
            Ok(theme) => {
                self.theme = theme;
                true
            }
            Err(_) => false,
        }
    }
}

/// Loads a theme file, layered over `base`. With the `themes` feature off there
/// is no loader, so a configured file is an error rather than a silent ignore.
#[cfg(feature = "themes")]
fn load(path: &Path, base: Theme) -> Result<Theme> {
    crate::theme::load_theme(path, base).map_err(|error| Error::Theme(error.to_string()))
}

#[cfg(not(feature = "themes"))]
fn load(_path: &Path, _base: Theme) -> Result<Theme> {
    Err(Error::Theme(
        "theme files require the `themes` feature (it is on by default)".to_string(),
    ))
}

/// The file's modification time, or `None` if it cannot be stat'd.
fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

/// Declares one frame: brackets `view` in the store's frame lifecycle, builds a
/// themed [`Frame`] over `buffer` and `store` with the current focus, and returns
/// the frame's collected facts and handlers.
///
/// The caller has already cleared (or resized) `buffer` to blank, matching the
/// declared-frame rule that widgets re-declare everything each frame.
fn draw(
    buffer: &mut Buffer,
    store: &mut StateStore,
    focus: Focus,
    theme: &Theme,
    view: impl FnOnce(&mut Frame<'_>),
) -> (FrameFacts, HandlerMap) {
    store.begin_frame();
    let parts = {
        let mut frame = Frame::themed(buffer, store, focus.current(), theme);
        view(&mut frame);
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
    use rabbitui_core::style::Style;

    #[test]
    fn resize_event_carries_the_new_size() {
        let event: Event = Event::Resize(Size::new(120, 40));
        match event {
            Event::Resize(size) => assert_eq!(size, Size::new(120, 40)),
            _ => panic!("expected a resize event"),
        }
    }

    #[test]
    fn outcome_for_matches_root_level_key_path() {
        let id = WidgetId::ROOT.child(key("ok"));
        let outcomes = [(id, Outcome::Activated)];
        let pending = RefCell::new(Pending::<()>::default());
        let update = Update::new(
            Event::Input(InputEvent::key(Key::Enter)),
            &outcomes,
            &pending,
        );
        assert_eq!(update.outcome_for(&[key("ok")]), Some(&Outcome::Activated));
        assert_eq!(update.outcome_for(&[key("nope")]), None);
    }

    #[test]
    fn outcome_for_matches_nested_key_path() {
        let id = WidgetId::ROOT.child(key("panel")).child(key("ok"));
        let outcomes = [(id, Outcome::Activated)];
        let pending = RefCell::new(Pending::<()>::default());
        let update = Update::new(
            Event::Input(InputEvent::key(Key::Enter)),
            &outcomes,
            &pending,
        );
        assert_eq!(
            update.outcome_for(&[key("panel"), key("ok")]),
            Some(&Outcome::Activated)
        );
        // The wrong depth does not match.
        assert_eq!(update.outcome_for(&[key("ok")]), None);
    }

    #[test]
    fn commit_and_set_mode_are_buffered() {
        let pending = RefCell::new(Pending::<()>::default());
        let update = Update::new(Event::Input(InputEvent::key(Key::Enter)), &[], &pending);
        update.commit("first");
        update.commit("second");
        update.set_mode(Mode::inline(3));
        let drained = pending.into_inner();
        assert_eq!(drained.commits.len(), 2);
        assert_eq!(drained.commits[0].text(), "first");
        assert_eq!(drained.set_mode, Some(Mode::inline(3)));
    }

    #[test]
    fn alt_engine_buffer_is_full_viewport() {
        let engine = ModeEngine::new(Mode::AltScreen, None);
        assert_eq!(engine.buffer_size(Size::new(80, 24)), Size::new(80, 24));
    }

    #[test]
    fn inline_engine_buffer_is_bounded_tail() {
        let engine = ModeEngine::new(Mode::inline(3), None);
        // Capped by max_height when the viewport is taller…
        assert_eq!(engine.buffer_size(Size::new(80, 24)), Size::new(80, 3));
        // …and by the viewport when it is shorter.
        assert_eq!(engine.buffer_size(Size::new(80, 2)), Size::new(80, 2));
    }

    #[test]
    fn mouse_default_is_on_in_alt_and_off_in_inline() {
        // No override: alt-screen captures, inline does not (the scrollback
        // tradeoff applied to ourselves).
        assert!(ModeEngine::new(Mode::AltScreen, None).mouse_capture());
        assert!(!ModeEngine::new(Mode::inline(3), None).mouse_capture());
        // An explicit override wins either way.
        assert!(!ModeEngine::new(Mode::AltScreen, Some(false)).mouse_capture());
        assert!(ModeEngine::new(Mode::inline(3), Some(true)).mouse_capture());
    }

    #[test]
    fn alt_enter_leave_toggle_mouse_by_default() {
        let mut engine = ModeEngine::new(Mode::AltScreen, None);
        let enter = engine.enter();
        // Alt captures by default: entry ends with the mouse-enable bytes.
        assert!(
            enter
                .windows(crate::encode::ENABLE_MOUSE.len())
                .any(|w| w == crate::encode::ENABLE_MOUSE)
        );
        let leave = engine.leave();
        assert!(
            leave
                .windows(crate::encode::DISABLE_MOUSE.len())
                .any(|w| w == crate::encode::DISABLE_MOUSE)
        );
    }

    #[test]
    fn inline_enter_omits_mouse_by_default() {
        let mut engine = ModeEngine::new(Mode::inline(3), None);
        let enter = engine.enter();
        // Inline does not capture by default, so no mouse-enable is emitted.
        assert!(
            !enter
                .windows(crate::encode::ENABLE_MOUSE.len())
                .any(|w| w == crate::encode::ENABLE_MOUSE)
        );
    }

    /// A vt100 model processes the full alt-screen mouse-capture transition —
    /// entry (with mouse enable), a frame, and leave (with mouse disable) — into a
    /// clean screen, proving the mouse control sequences are well-formed and do
    /// not corrupt output at the escape level (ADR 0009 layer 3).
    #[test]
    fn vt_processes_mouse_enable_frame_and_disable_at_transitions() {
        use rabbitui_testing::vt::VtScreen;

        let mut engine = ModeEngine::new(Mode::AltScreen, None);
        let mut screen = VtScreen::new(10, 3);

        // Entry carries the mouse-enable sequence at its tail.
        let enter = engine.enter();
        assert!(
            enter
                .windows(crate::encode::ENABLE_MOUSE.len())
                .any(|w| w == crate::encode::ENABLE_MOUSE),
            "alt-screen entry enables mouse (modes 1000+1006)"
        );
        screen.feed(&enter);

        // A frame renders normally alongside the enabled mouse mode.
        let previous = Buffer::new(Size::new(10, 3));
        let mut current = previous.clone();
        current.set_string(
            rabbitui_core::geometry::Position::ORIGIN,
            "hi",
            Style::new(),
        );
        screen.feed(&engine.render(&current, &previous, &[]));
        screen.assert_row(0, "hi");

        // Leave carries the mouse-disable sequence, and vt100 processes it cleanly.
        let leave = engine.leave();
        assert!(
            leave
                .windows(crate::encode::DISABLE_MOUSE.len())
                .any(|w| w == crate::encode::DISABLE_MOUSE),
            "alt-screen leave disables mouse in reverse order"
        );
        screen.feed(&leave);
    }
}
