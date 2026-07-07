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

use crate::effect::{Cmd, Effects, EffectError, Outbox};
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
/// on the same buffering principle: [`Update::spawn`] queues a [`Cmd`] the runtime
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
    /// Effects to spawn after `update` returns, in call order.
    effects: Vec<Cmd<M>>,
    /// Between-frames widget commands and a deferred focus request, applied by the
    /// shared [`core::pending`](rabbitui_core::pending) function.
    widget: WidgetPending,
}

impl<M> Default for Pending<M> {
    fn default() -> Self {
        Self {
            commits: Vec::new(),
            set_mode: None,
            effects: Vec::new(),
            widget: WidgetPending::new(),
        }
    }
}

impl<M> std::fmt::Debug for Pending<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pending")
            .field("commits", &self.commits.len())
            .field("set_mode", &self.set_mode)
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
        Self { event, outcomes, pending }
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
        self.outcomes.iter().find(|(id, _)| *id == target).map(|(_, outcome)| outcome)
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
    /// use rabbitui::effect::Cmd;
    /// use rabbitui_core::input::{InputEvent, Key};
    ///
    /// let pending = RefCell::new(Default::default());
    /// let update: Update<'_, u32> =
    ///     Update::new(Event::Input(InputEvent::key(Key::Enter)), &[], &pending);
    /// update.spawn(Cmd::future(async { 42 }));
    /// ```
    pub fn spawn(&self, cmd: Cmd<M>) {
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
    App::new(state, update, view).run().await
}

/// A configurable application: state, an `update`, a `view`, and theming.
///
/// The builder form of [`run`], for apps that need more than the three-argument
/// default — specifically a [`Theme`] other than [`Theme::default`], or a theme
/// **file** with debug-build hot reload. Terse apps use [`run`]; anything
/// theming-aware constructs an `App`, chains [`theme`](Self::theme) /
/// [`theme_file`](Self::theme_file), and calls [`run`](Self::run):
///
/// ```no_run
/// use std::ops::ControlFlow;
///
/// use rabbitui::App;
/// use rabbitui::app::Update;
/// use rabbitui_core::frame::Frame;
/// use rabbitui_core::id::key;
/// use rabbitui_core::theme::Theme;
/// use rabbitui_widgets::Text;
///
/// # async fn demo() -> rabbitui::app::Result<()> {
/// App::new(
///     (),
///     |_state: &mut (), _update: Update<'_>| ControlFlow::Break(()),
///     |_state: &(), frame: &mut Frame<'_>| {
///         frame.widget(key("hi"), frame.area(), &Text::new("hi"));
///     },
/// )
/// .theme(Theme::catppuccin_mocha())
/// .theme_file("theme.toml")
/// .run()
/// .await
/// # }
/// ```
///
/// # Why a builder, not more `run` arguments
///
/// `run(state, update, view)` reads cleanly at three arguments; a fourth and
/// fifth positional argument for `theme` and an *optional* path would not.
/// Theming is also opt-in — most apps never set it — so it belongs on a builder
/// whose defaults reproduce `run` exactly. `run` stays as the terse entry point
/// and simply delegates to `App::new(...).run()`, so there is one loop, not two.
pub struct App<S, U, V, M = ()> {
    state: S,
    update: U,
    view: V,
    theme: Theme,
    theme_file: Option<PathBuf>,
    mode: Mode,
    /// Ties the app to its message type without owning one; the `fn() -> M`
    /// form keeps `App` `Send`-agnostic and variance-correct.
    _marker: std::marker::PhantomData<fn() -> M>,
}

impl<S, U, V, M> App<S, U, V, M>
where
    U: FnMut(&mut S, Update<'_, M>) -> ControlFlow<()>,
    V: Fn(&S, &mut Frame<'_>),
    M: Send + 'static,
{
    /// Creates an app from owned `state`, an `update`, and a `view`, using the
    /// default theme, no theme file, and the default screen [`Mode`]
    /// ([`Mode::AltScreen`]).
    ///
    /// The result behaves exactly like [`run`] until [`theme`](Self::theme),
    /// [`theme_file`](Self::theme_file), or [`mode`](Self::mode) is called.
    #[must_use]
    pub fn new(state: S, update: U, view: V) -> Self {
        Self {
            state,
            update,
            view,
            theme: Theme::default(),
            theme_file: None,
            mode: Mode::default(),
            _marker: std::marker::PhantomData,
        }
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
    ///
    /// # Examples
    ///
    /// ```
    /// use std::ops::ControlFlow;
    ///
    /// use rabbitui::App;
    /// use rabbitui::app::Update;
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::mode::Mode;
    ///
    /// let app = App::new(
    ///     (),
    ///     |_: &mut (), _: Update<'_>| ControlFlow::Break(()),
    ///     |_: &(), _: &mut Frame<'_>| {},
    /// )
    /// .mode(Mode::inline(3));
    /// let _ = app;
    /// ```
    #[must_use]
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets the active [`Theme`] the loop threads into every frame.
    ///
    /// If a [`theme_file`](Self::theme_file) is also set, this is the *base* the
    /// file's roles layer over (a file names only the roles it changes; the rest
    /// stay as this theme).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::ops::ControlFlow;
    ///
    /// use rabbitui::App;
    /// use rabbitui::app::Update;
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::theme::Theme;
    ///
    /// let app = App::new(
    ///     (),
    ///     |_: &mut (), _: Update<'_>| ControlFlow::Break(()),
    ///     |_: &(), _: &mut Frame<'_>| {},
    /// )
    /// .theme(Theme::catppuccin_mocha());
    /// let _ = app;
    /// ```
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
    /// at startup fails [`run`](Self::run); a reload error mid-run is ignored so
    /// a half-saved edit never crashes the app (the previous theme stays).
    ///
    /// # Cost of hot reload
    ///
    /// The debug-build poll is **one `stat(2)` per loop iteration** — a metadata
    /// read, no file contents unless the mtime changed. The loop iterates once
    /// per input event, so at terminal event rates this is negligible; it is
    /// compiled out entirely in release builds via `cfg!(debug_assertions)`.
    ///
    /// [`theme_file`]: Self::theme_file
    #[must_use]
    pub fn theme_file(mut self, path: impl AsRef<Path>) -> Self {
        self.theme_file = Some(path.as_ref().to_path_buf());
        self
    }

    /// Runs the application loop until `update` returns [`ControlFlow::Break`].
    ///
    /// Identical to [`run`]'s loop, plus theming: the active theme is threaded
    /// into every frame, and (debug builds only) a theme file is polled for
    /// changes once per iteration and hot-reloaded.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal, input, size polling, or rendering fails,
    /// or if a configured theme file cannot be loaded or parsed at startup.
    pub async fn run(self) -> Result<()> {
        let App { mut state, mut update, view, theme: base_theme, theme_file, mode, .. } = self;

        // Load the initial theme from the file (if any), layered over the base.
        // A startup error is fatal; a mid-run reload error is not (see below).
        let mut watcher = ThemeWatcher::new(theme_file, base_theme)?;
        let mut theme = watcher.theme();

        let mut terminal = Terminal::open().await?;
        let mut viewport = terminal.size()?;

        // The render engine for the active mode; `enter` produces the mode-entry
        // bytes (alt-screen switch, or inline cursor-hide). The state store,
        // focus, and theme persist across iterations.
        let mut engine = ModeEngine::new(mode);
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
        let (mut facts, mut handlers) = draw(&mut back, &mut store, focus, &theme, &state, &view);
        focus.reconcile(&facts);
        terminal.write_bytes(&engine.render(&back, &front, &[])).await?;
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
            let wake = {
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
                        let ctx = Update::new(Event::Resize(viewport), &[], &pending);
                        broke = update(&mut state, ctx).is_break();
                        drain_pending(
                            pending.into_inner(),
                            &mut effects,
                            &mut store,
                            &facts,
                            &mut focus,
                            &mut commits_buf,
                            &mut set_mode_buf,
                        );
                        dirty = true;
                    }

                    if !broke {
                        if let Some(event) = crate::input::from_qwertty(&input) {
                            let result =
                                route(&facts, &handlers, &mut focus, &mut store, &event);
                            let pending = RefCell::new(Pending::default());
                            let ctx =
                                Update::new(Event::Input(event), &result.outcomes, &pending);
                            broke = update(&mut state, ctx).is_break();
                            drain_pending(
                                pending.into_inner(),
                                &mut effects,
                                &mut store,
                                &facts,
                                &mut focus,
                                &mut commits_buf,
                                &mut set_mode_buf,
                            );
                            dirty = true;
                        }
                    }
                }
                Wake::Effect(outbox) => {
                    broke = deliver_effect(
                        outbox,
                        &mut state,
                        &mut update,
                        &mut effects,
                        &mut store,
                        &facts,
                        &mut focus,
                        &mut commits_buf,
                        &mut set_mode_buf,
                    );
                    dirty = true;

                    // Coalescing drain: absorb every already-queued effect result
                    // into this one frame with a biased `try_recv` loop, so a flood
                    // of stream messages is one render, not one render per message.
                    while !broke {
                        let Some(next) = effects.try_recv() else { break };
                        broke = deliver_effect(
                            next,
                            &mut state,
                            &mut update,
                            &mut effects,
                            &mut store,
                            &facts,
                            &mut focus,
                            &mut commits_buf,
                            &mut set_mode_buf,
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
                    terminal.write_bytes(&engine.render(&empty, &front, &remaining)).await?;
                }
                return leave(terminal, &mut engine).await;
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
            let drawn = draw(&mut back, &mut store, focus, &theme, &state, &view);
            facts = drawn.0;
            handlers = drawn.1;
            focus.reconcile(&facts);
            terminal.write_bytes(&engine.render(&back, &front, &frame_commits)).await?;
            std::mem::swap(&mut front, &mut back);

            dirty = false;
            next_paint = None;
            last_paint = tokio::time::Instant::now();
        }
    }
}

/// What woke the loop's `select!`: an input, an effect result, the paint
/// deadline, or a spurious idle (a defensively-handled closed mailbox).
enum Wake<M> {
    /// A raw substrate input event from the terminal (boxed: qwertty's event is
    /// larger than the other small variants, so this keeps `Wake` compact).
    Input(Box<qwertty::InputEvent>),
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
#[allow(clippy::too_many_arguments)]
fn drain_pending<M: Send + 'static>(
    pending: Pending<M>,
    effects: &mut Effects<M>,
    store: &mut StateStore,
    facts: &FrameFacts,
    focus: &mut Focus,
    commits_buf: &mut Vec<CommitLine>,
    set_mode_buf: &mut Option<Mode>,
) {
    let Pending { commits, set_mode, effects: cmds, widget } = pending;
    for cmd in cmds {
        effects.spawn(cmd);
    }
    widget.apply(store, facts, focus);
    commits_buf.extend(commits);
    if set_mode.is_some() {
        *set_mode_buf = set_mode;
    }
}

/// Delivers one effect result to `update` and drains its pending, returning
/// whether the app asked to break.
///
/// A [`Outbox::Message`] becomes [`Event::Message`]; a [`Outbox::Failed`] becomes
/// [`Event::EffectFailed`]. Either way the app's `update` sees it in the one
/// serialized loop, exactly like an input event, and may itself spawn more
/// effects or command widgets.
#[allow(clippy::too_many_arguments)]
fn deliver_effect<S, M, U>(
    outbox: Outbox<M>,
    state: &mut S,
    update: &mut U,
    effects: &mut Effects<M>,
    store: &mut StateStore,
    facts: &FrameFacts,
    focus: &mut Focus,
    commits_buf: &mut Vec<CommitLine>,
    set_mode_buf: &mut Option<Mode>,
) -> bool
where
    M: Send + 'static,
    U: FnMut(&mut S, Update<'_, M>) -> ControlFlow<()>,
{
    let event = match outbox {
        Outbox::Message(message) => Event::Message(message),
        Outbox::Failed(error) => Event::EffectFailed(error),
    };
    let pending = RefCell::new(Pending::default());
    let ctx = Update::new(event, &[], &pending);
    let broke = update(state, ctx).is_break();
    drain_pending(pending.into_inner(), effects, store, facts, focus, commits_buf, set_mode_buf);
    broke
}

/// Writes the active engine's teardown bytes, then closes the terminal.
///
/// The engine's `leave` frame does the mode-specific restore (leave alt screen,
/// or drop below the inline tail); [`Terminal::close`] then leaves raw mode with
/// the unconditional alt-screen-leave backstop.
async fn leave(mut terminal: Terminal, engine: &mut ModeEngine) -> Result<()> {
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
async fn apply_mode_switch(
    terminal: &mut Terminal,
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
        return Ok(if engine.is_inline() { commits } else { Vec::new() });
    };

    match (engine.mode(), target) {
        // Leaving inline for alt: flush pending commits into scrollback through
        // the inline engine first (an empty tail, so only history is written),
        // then tear the inline region down.
        (Mode::Inline { .. }, Mode::AltScreen) => {
            if !commits.is_empty() {
                let empty = Buffer::new(Size::new(viewport.width, 0));
                terminal.write_bytes(&engine.render(&empty, front, &commits)).await?;
            }
            terminal.write_bytes(&engine.leave()).await?;
        }
        // Any other switch tears the current mode down first.
        _ => {
            terminal.write_bytes(&engine.leave()).await?;
        }
    }

    *engine = ModeEngine::new(target);
    let size = engine.buffer_size(viewport);
    front.resize(size);
    back.resize(size);
    terminal.write_bytes(&engine.enter()).await?;

    // Entering inline: hand the commits back so the caller's frame render lands
    // them above the new live tail. Entering alt: the commits (if any) were
    // flushed above, so none remain.
    Ok(if engine.is_inline() { commits } else { Vec::new() })
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
enum ModeEngine {
    /// The alternate-screen engine and its declared mode.
    Alt(AltEngine),
    /// The inline engine and its `max_height`.
    Inline { engine: InlineEngine, max_height: u16 },
}

impl ModeEngine {
    /// Builds the engine for `mode`.
    fn new(mode: Mode) -> Self {
        match mode {
            Mode::AltScreen => Self::Alt(AltEngine::new()),
            Mode::Inline { max_height } => {
                Self::Inline { engine: InlineEngine::new(), max_height }
            }
        }
    }

    /// The mode this engine renders.
    fn mode(&self) -> Mode {
        match self {
            Self::Alt(_) => Mode::AltScreen,
            Self::Inline { max_height, .. } => Mode::Inline { max_height: *max_height },
        }
    }

    /// Whether this is the inline engine.
    fn is_inline(&self) -> bool {
        matches!(self, Self::Inline { .. })
    }

    /// The frame-buffer size for this mode at `viewport`: the full viewport in
    /// alt-screen, or the bounded live-tail height (`min(max_height, viewport
    /// height)`) at full width in inline.
    fn buffer_size(&self, viewport: Size) -> Size {
        match self {
            Self::Alt(_) => viewport,
            Self::Inline { max_height, .. } => {
                Size::new(viewport.width, (*max_height).min(viewport.height))
            }
        }
    }

    /// The mode-entry bytes.
    fn enter(&mut self) -> Vec<u8> {
        match self {
            Self::Alt(engine) => engine.enter(),
            Self::Inline { engine, .. } => engine.enter(),
        }
    }

    /// The mode-teardown bytes.
    fn leave(&mut self) -> Vec<u8> {
        match self {
            Self::Alt(engine) => engine.leave(),
            Self::Inline { engine, .. } => engine.leave(),
        }
    }

    /// Forces the next render to fully repaint (resize / desync recovery).
    fn force_repaint(&mut self) {
        if let Self::Inline { engine, .. } = self {
            engine.force_repaint();
        }
    }

    /// One frame's bytes: the alt engine diffs `current` against `previous`; the
    /// inline engine flushes `commits` then paints `current` as the live tail.
    fn render(&mut self, current: &Buffer, previous: &Buffer, commits: &[CommitLine]) -> Vec<u8> {
        match self {
            Self::Alt(engine) => engine.render(current, previous),
            Self::Inline { engine, .. } => engine.render(current, commits),
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
        Ok(Self { file, base, theme, last_modified })
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
    std::fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

/// Declares one frame: brackets `view` in the store's frame lifecycle, builds a
/// themed [`Frame`] over `buffer` and `store` with the current focus, and returns
/// the frame's collected facts and handlers.
///
/// The caller has already cleared (or resized) `buffer` to blank, matching the
/// declared-frame rule that widgets re-declare everything each frame.
fn draw<S>(
    buffer: &mut Buffer,
    store: &mut StateStore,
    focus: Focus,
    theme: &Theme,
    state: &S,
    view: &impl Fn(&S, &mut Frame<'_>),
) -> (FrameFacts, HandlerMap) {
    store.begin_frame();
    let parts = {
        let mut frame = Frame::themed(buffer, store, focus.current(), theme);
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
        let update = Update::new(Event::Input(InputEvent::key(Key::Enter)), &outcomes, &pending);
        assert_eq!(update.outcome_for(&[key("ok")]), Some(&Outcome::Activated));
        assert_eq!(update.outcome_for(&[key("nope")]), None);
    }

    #[test]
    fn outcome_for_matches_nested_key_path() {
        let id = WidgetId::ROOT.child(key("panel")).child(key("ok"));
        let outcomes = [(id, Outcome::Activated)];
        let pending = RefCell::new(Pending::<()>::default());
        let update = Update::new(Event::Input(InputEvent::key(Key::Enter)), &outcomes, &pending);
        assert_eq!(update.outcome_for(&[key("panel"), key("ok")]), Some(&Outcome::Activated));
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
        let engine = ModeEngine::new(Mode::AltScreen);
        assert_eq!(engine.buffer_size(Size::new(80, 24)), Size::new(80, 24));
    }

    #[test]
    fn inline_engine_buffer_is_bounded_tail() {
        let engine = ModeEngine::new(Mode::inline(3));
        // Capped by max_height when the viewport is taller…
        assert_eq!(engine.buffer_size(Size::new(80, 24)), Size::new(80, 3));
        // …and by the viewport when it is shorter.
        assert_eq!(engine.buffer_size(Size::new(80, 2)), Size::new(80, 2));
    }
}
