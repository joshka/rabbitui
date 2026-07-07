//! The declared frame: where an app describes one frame of UI.
//!
//! Per `docs/adr/0001-programming-model.md`, the app's view function receives
//! a [`Frame`] and declares widgets into it by key. The frame composes
//! identities ([`WidgetId`]s) from the declaration path, lends each widget its
//! retained state from the [`StateStore`], and paints into the target buffer
//! through clipped [`RenderCtx`]s. From slice 3 it also collects frame facts
//! (each widget's area, scope parent, focusability) and registers a
//! type-erased **handler thunk** per widget, so the runtime can route the next
//! event against this frame's facts (ADR 0006).
//!
//! # Focus at render time
//!
//! The frame carries a read-only focus snapshot — the [`WidgetId`] the
//! framework currently focuses. When declaring a widget whose id equals the
//! snapshot, the frame tells that widget's [`RenderCtx`] it is focused, so the
//! widget can paint a focus style. The snapshot is the *previous* frame's focus
//! verdict; the runtime advances focus between frames (ADR 0006).
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::buffer::Buffer;
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::geometry::{Position, Size};
//! use rabbitui_core::id::key;
//! use rabbitui_core::layout::Constraint;
//! use rabbitui_core::store::StateStore;
//! use rabbitui_core::style::Style;
//! use rabbitui_core::widget::{RenderCtx, Widget};
//!
//! struct Label<'a>(&'a str);
//! impl Widget for Label<'_> {
//!     type State = ();
//!     fn render(&self, _state: &mut (), ctx: &mut RenderCtx<'_>) {
//!         ctx.set_string(Position::ORIGIN, self.0, Style::new());
//!     }
//! }
//!
//! let mut buffer = Buffer::new(Size::new(20, 3));
//! let mut store = StateStore::new();
//! let mut frame = Frame::new(&mut buffer, &mut store);
//!
//! let [title, _body] = frame.rows([Constraint::Length(1), Constraint::Fill(1)]);
//! frame.widget(key("title"), title, &Label("hello"));
//! # let _facts = frame.finish();
//! assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "h");
//! ```

use std::any::Any;
use std::collections::HashMap;

use crate::buffer::Buffer;
use crate::facts::{FactEntry, FrameFacts, VisibilityRequest};
use crate::geometry::Rect;
use crate::id::{Key, WidgetId};
use crate::input::InputEvent;
use crate::layout::{Constraint, split_columns, split_rows};
use crate::store::StateStore;
use crate::theme::Theme;
use crate::widget::{HandleCtx, Handled, RenderCtx, Widget};

/// The theme a [`Frame`] carries when none is supplied — the restrained dark
/// default. Kept as a `const` so [`Frame::new`]/[`Frame::with_focus`] can borrow
/// a `'static` reference and stay signature-compatible with earlier slices.
const DEFAULT_THEME: &Theme = &Theme::dark();

/// A type-erased widget handler thunk.
///
/// One is registered per declared widget: a monomorphization of `W::handle`
/// that downcasts the erased state back to `W::State` before calling it. This is
/// how the runtime routes an event to a widget whose spec no longer exists —
/// the thunk closes over `W`, not over the spec (ADR 0006).
pub type Handler = fn(&mut dyn Any, &InputEvent, &mut HandleCtx<'_>) -> Handled;

/// The registered handlers of one frame, keyed by widget identity.
///
/// Kept alongside the [`FrameFacts`] of the same frame; the runtime routes an
/// event through the facts to select target/ancestors, then invokes their
/// handlers from this map.
pub type HandlerMap = HashMap<WidgetId, Handler>;

/// Wraps `W::handle` as a type-erased [`Handler`].
///
/// The wrapper downcasts the erased `&mut dyn Any` back to `W::State`; the state
/// was stored as `Box<dyn Any>` of exactly that type by the same-id render, so
/// the downcast cannot fail for a well-formed frame.
fn handler_thunk<W: Widget>() -> Handler {
    |state, event, ctx| {
        let state = state
            .downcast_mut::<W::State>()
            .expect("handler state type matches the widget that registered it");
        W::handle(state, event, ctx)
    }
}

/// One frame under declaration.
///
/// Created by the runtime once per frame around the app's view function. The
/// frame does not retain painted cells — identity-keyed state lives in the
/// store, cells live in the buffer — but it *does* accumulate the frame's facts
/// and handlers, surrendered with [`finish`](Self::finish) for the runtime to
/// route the next event against.
#[derive(Debug)]
pub struct Frame<'a> {
    buffer: &'a mut Buffer,
    store: &'a mut StateStore,
    /// Identity of the current declaration parent; child widgets compose
    /// their ids under it.
    parent: WidgetId,
    /// The framework's current focus verdict (previous frame's), used to tell a
    /// widget it is focused at render time.
    focus: Option<WidgetId>,
    /// The active theme, lent to every widget's [`RenderCtx`] so it can resolve
    /// roles to styles (ADR 0007).
    theme: &'a Theme,
    /// The overlay layer widgets declared into this frame land in. Base = 0;
    /// [`layer`](Self::layer) increments it for the scope it brackets (slice-7
    /// layers, ADR 0003 delta).
    layer: u8,
    /// Facts collected as widgets declare, in declaration order.
    facts: FrameFacts,
    /// Handler thunks registered as widgets declare.
    handlers: HandlerMap,
}

impl<'a> Frame<'a> {
    /// Begins a frame over `buffer` with retained state in `store`, with no
    /// widget focused.
    ///
    /// The runtime is responsible for calling [`StateStore::begin_frame`]
    /// before and [`StateStore::end_frame`] after the view function. Use
    /// [`with_focus`](Self::with_focus) to supply the focus snapshot.
    #[must_use]
    pub fn new(buffer: &'a mut Buffer, store: &'a mut StateStore) -> Self {
        Self {
            buffer,
            store,
            parent: WidgetId::ROOT,
            focus: None,
            theme: DEFAULT_THEME,
            layer: 0,
            facts: FrameFacts::new(),
            handlers: HandlerMap::new(),
        }
    }

    /// Begins a frame over `buffer` and `store` with `focus` as the currently
    /// focused widget.
    ///
    /// The focus snapshot is read-only for the duration of the frame: a widget
    /// whose id equals `focus` renders as focused. Focus itself advances between
    /// frames in the runtime (ADR 0006).
    #[must_use]
    pub fn with_focus(
        buffer: &'a mut Buffer,
        store: &'a mut StateStore,
        focus: Option<WidgetId>,
    ) -> Self {
        Self {
            buffer,
            store,
            parent: WidgetId::ROOT,
            focus,
            theme: DEFAULT_THEME,
            layer: 0,
            facts: FrameFacts::new(),
            handlers: HandlerMap::new(),
        }
    }

    /// Begins a frame over `buffer` and `store` with `focus` and a specific
    /// `theme`.
    ///
    /// The theme-carrying variant of [`with_focus`](Self::with_focus): widgets
    /// declared into this frame resolve their [`Role`](crate::theme::Role)s
    /// against `theme` (ADR 0007). The runtime and the test harness build the
    /// frame this way to thread the app's active theme in; the plainer
    /// constructors default to [`Theme::default`].
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::geometry::Size;
    /// use rabbitui_core::store::StateStore;
    /// use rabbitui_core::theme::Theme;
    ///
    /// let mut buffer = Buffer::new(Size::new(8, 1));
    /// let mut store = StateStore::new();
    /// let theme = Theme::catppuccin_mocha();
    /// let frame = Frame::themed(&mut buffer, &mut store, None, &theme);
    /// # let _ = frame.finish();
    /// ```
    #[must_use]
    pub fn themed(
        buffer: &'a mut Buffer,
        store: &'a mut StateStore,
        focus: Option<WidgetId>,
        theme: &'a Theme,
    ) -> Self {
        Self {
            buffer,
            store,
            parent: WidgetId::ROOT,
            focus,
            theme,
            layer: 0,
            facts: FrameFacts::new(),
            handlers: HandlerMap::new(),
        }
    }

    /// The active theme this frame resolves roles against.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        self.theme
    }

    /// The full drawable area of this frame.
    #[must_use]
    pub fn area(&self) -> Rect {
        Rect::from_size(self.buffer.size())
    }

    /// Splits the frame's full area into horizontal bands
    /// (see [`split_rows`]).
    #[must_use]
    pub fn rows<const N: usize>(&self, constraints: [Constraint; N]) -> [Rect; N] {
        split_rows(self.area(), constraints)
    }

    /// Splits the frame's full area into vertical bands
    /// (see [`split_columns`]).
    #[must_use]
    pub fn columns<const N: usize>(&self, constraints: [Constraint; N]) -> [Rect; N] {
        split_columns(self.area(), constraints)
    }

    /// Declares a widget: composes its identity from `key` under the current
    /// parent, lends it its retained state, renders it into `area`, and records
    /// its facts and handler.
    pub fn widget<W: Widget>(&mut self, key: Key, area: Rect, widget: &W) {
        let id = self.parent.child(key);
        let focused = self.focus == Some(id);
        let bounds = Rect::from_size(self.buffer.size());
        let clipped = area.intersection(bounds);

        let (focusable, visibility) = {
            let state = self.store.get_or_default::<W::State>(id);
            let mut ctx = RenderCtx::new_themed(self.buffer, area, focused, self.theme);
            widget.render(state, &mut ctx);
            (ctx.is_focusable(), ctx.requested_visibility())
        };

        self.facts.push(FactEntry {
            id,
            parent: self.parent,
            area: clipped,
            focusable,
            layer: self.layer,
        });
        if let Some(area) = visibility {
            self.facts.push_visibility(VisibilityRequest { id, area });
        }
        self.handlers.insert(id, handler_thunk::<W>());
    }

    /// Declares a container scope: widgets declared inside `scope` compose
    /// their identities under `key`, so a reusable view function gets a
    /// distinct identity subtree per call site.
    ///
    /// Facts and handlers declared inside the scope flow into the same frame,
    /// with the scope id as their parent, preserving the routing path.
    pub fn scoped(&mut self, key: Key, scope: impl FnOnce(&mut Frame<'_>)) {
        let scope_id = self.parent.child(key);
        let mut child = Frame {
            buffer: self.buffer,
            store: self.store,
            parent: scope_id,
            focus: self.focus,
            theme: self.theme,
            layer: self.layer,
            facts: std::mem::take(&mut self.facts),
            handlers: std::mem::take(&mut self.handlers),
        };
        scope(&mut child);
        // Reclaim the accumulated facts and handlers from the child scope.
        self.facts = child.facts;
        self.handlers = child.handlers;
    }

    /// Declares an **overlay layer**: widgets declared inside `scope` land in a
    /// layer one above this frame's, and compose their identities under `key`
    /// (a scope, exactly like [`scoped`](Self::scoped)) so a modal gets its own
    /// identity subtree.
    ///
    /// Layers are the slice-7 overlay primitive (ADR 0003 delta). Declaration
    /// order is already the painter's algorithm in one buffer, so a layer needs
    /// no separate compositing pass; what it adds is **input containment**:
    ///
    /// - Hit-testing prefers the highest layer, so a click lands on the modal,
    ///   not the base beneath it ([`FrameFacts::hit`]).
    /// - Focus traversal is restricted to the topmost layer, so Tab cycles only
    ///   the modal's focusables while it exists, and reconciles back to the base
    ///   when the layer disappears ([`FrameFacts::focus_order`]).
    ///
    /// A modal is therefore a `layer(key, |f| …)` that declares its backdrop and
    /// controls; while it is declared, the base is inert to keyboard and its
    /// clicks are swallowed by the overlay's hit region. Painting a dimming wash
    /// over the base is a widgets-crate utility, not core.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::geometry::Size;
    /// use rabbitui_core::id::key;
    /// use rabbitui_core::store::StateStore;
    ///
    /// let mut buffer = Buffer::new(Size::new(20, 5));
    /// let mut store = StateStore::new();
    /// store.begin_frame();
    /// let mut frame = Frame::new(&mut buffer, &mut store);
    /// frame.layer(key("confirm"), |modal| {
    ///     // declare the modal's widgets here; they land in layer 1.
    ///     let _ = modal;
    /// });
    /// # let _ = frame.finish();
    /// store.end_frame();
    /// ```
    pub fn layer(&mut self, key: Key, scope: impl FnOnce(&mut Frame<'_>)) {
        let scope_id = self.parent.child(key);
        let mut child = Frame {
            buffer: self.buffer,
            store: self.store,
            parent: scope_id,
            focus: self.focus,
            theme: self.theme,
            layer: self.layer.saturating_add(1),
            facts: std::mem::take(&mut self.facts),
            handlers: std::mem::take(&mut self.handlers),
        };
        scope(&mut child);
        // Reclaim the accumulated facts and handlers; the base frame's own layer
        // is unchanged, so widgets declared after this call stay on the base.
        self.facts = child.facts;
        self.handlers = child.handlers;
    }

    /// Ends the declaration and returns the frame's collected facts.
    ///
    /// The runtime keeps the returned facts (and the handlers, via
    /// [`into_parts`](Self::into_parts)) to route the next event. The
    /// buffer and store were mutated in place during declaration.
    #[must_use]
    pub fn finish(self) -> FrameFacts {
        self.facts
    }

    /// Ends the declaration and returns both the collected facts and the
    /// registered handler thunks.
    ///
    /// This is the runtime/harness entry point: it needs both halves to route an
    /// event (facts to find the target and its ancestors, handlers to invoke
    /// them).
    #[must_use]
    pub fn into_parts(self) -> (FrameFacts, HandlerMap) {
        (self.facts, self.handlers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Position, Size};
    use crate::id::key;
    use crate::input::{Key as InputKey, KeyEvent};
    use crate::outcome::Outcome;
    use crate::style::Style;

    #[derive(Default)]
    struct CountState {
        renders: u32,
    }

    struct Probe;

    impl Widget for Probe {
        type State = CountState;
        fn render(&self, state: &mut CountState, ctx: &mut RenderCtx<'_>) {
            state.renders += 1;
            ctx.set_string(Position::ORIGIN, &state.renders.to_string(), Style::new());
        }
    }

    #[test]
    fn state_persists_across_frames_by_key() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut store = StateStore::new();
        for _ in 0..3 {
            store.begin_frame();
            let mut frame = Frame::new(&mut buffer, &mut store);
            frame.widget(key("probe"), frame.area(), &Probe);
            let _ = frame.finish();
            store.end_frame();
        }
        assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "3");
    }

    #[test]
    fn scoped_keys_are_distinct_identities() {
        let mut buffer = Buffer::new(Size::new(8, 2));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        let [top, bottom] = frame.rows([Constraint::Length(1), Constraint::Length(1)]);
        // The same inner key under two scopes: two widgets, two states —
        // no duplicate-id panic.
        frame.scoped(key("left"), |f| f.widget(key("probe"), top, &Probe));
        frame.scoped(key("right"), |f| f.widget(key("probe"), bottom, &Probe));
        let _ = frame.finish();
        store.end_frame();
        assert_eq!(store.len(), 2);
    }

    struct Focusable;
    impl Widget for Focusable {
        type State = ();
        fn render(&self, _state: &mut (), ctx: &mut RenderCtx<'_>) {
            ctx.focusable(true);
        }
    }

    #[test]
    fn facts_record_area_parent_and_focusable() {
        let mut buffer = Buffer::new(Size::new(8, 2));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        let area = Rect::new(Position::new(1, 0), Size::new(3, 1));
        frame.widget(key("btn"), area, &Focusable);
        let facts = frame.finish();
        store.end_frame();

        let id = WidgetId::ROOT.child(key("btn"));
        let entry = facts.get(id).unwrap();
        assert_eq!(entry.area, area);
        assert_eq!(entry.parent, WidgetId::ROOT);
        assert!(entry.focusable);
    }

    #[test]
    fn scoped_facts_have_scope_parent() {
        let mut buffer = Buffer::new(Size::new(8, 2));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.scoped(key("panel"), |f| {
            f.widget(key("btn"), Rect::from_size(Size::new(3, 1)), &Focusable);
        });
        let facts = frame.finish();
        store.end_frame();

        let panel = WidgetId::ROOT.child(key("panel"));
        let btn = panel.child(key("btn"));
        assert_eq!(facts.get(btn).unwrap().parent, panel);
    }

    #[test]
    fn layer_tags_facts_and_restricts_focus_to_top() {
        let mut buffer = Buffer::new(Size::new(8, 4));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        // A base focusable, then a modal layer with two focusables.
        frame.widget(key("base"), Rect::from_size(Size::new(8, 1)), &Focusable);
        frame.layer(key("modal"), |modal| {
            modal.widget(key("ok"), Rect::from_size(Size::new(4, 1)), &Focusable);
            modal.widget(key("cancel"), Rect::from_size(Size::new(4, 1)), &Focusable);
        });
        let facts = frame.finish();
        store.end_frame();

        let base = WidgetId::ROOT.child(key("base"));
        let modal = WidgetId::ROOT.child(key("modal"));
        let ok = modal.child(key("ok"));
        let cancel = modal.child(key("cancel"));

        // The base sits on layer 0; the modal's widgets on layer 1.
        assert_eq!(facts.get(base).unwrap().layer, 0);
        assert_eq!(facts.get(ok).unwrap().layer, 1);
        assert_eq!(facts.get(ok).unwrap().parent, modal);
        assert_eq!(facts.top_layer(), 1);

        // Focus traversal is restricted to the top layer — the base is unreachable.
        let order: Vec<_> = facts.focus_order().map(|e| e.id).collect();
        assert_eq!(order, vec![ok, cancel]);
    }

    #[test]
    fn layer_hit_prefers_the_overlay() {
        let mut buffer = Buffer::new(Size::new(8, 4));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        let full = Rect::from_size(Size::new(8, 4));
        frame.widget(key("base"), full, &Focusable);
        frame.layer(key("modal"), |modal| {
            modal.widget(key("backdrop"), full, &Focusable);
        });
        let facts = frame.finish();
        store.end_frame();

        let backdrop = WidgetId::ROOT.child(key("modal")).child(key("backdrop"));
        // A click anywhere over both lands on the higher-layer backdrop.
        assert_eq!(facts.hit(Position::new(2, 2)).unwrap().id, backdrop);
    }

    struct VisibilityRequester;
    impl Widget for VisibilityRequester {
        type State = ();
        fn render(&self, _state: &mut (), ctx: &mut RenderCtx<'_>) {
            ctx.request_visibility(Rect::from_size(Size::new(2, 1)));
        }
    }

    #[test]
    fn widget_visibility_request_is_recorded_as_a_fact() {
        let mut buffer = Buffer::new(Size::new(8, 2));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        let area = Rect::new(Position::new(1, 1), Size::new(4, 1));
        frame.widget(key("scroll"), area, &VisibilityRequester);
        let facts = frame.finish();
        store.end_frame();

        let id = WidgetId::ROOT.child(key("scroll"));
        let request = facts.visibility_requests().next().unwrap();
        assert_eq!(request.id, id);
        // The area-relative request is resolved to the widget's absolute origin.
        assert_eq!(request.area.origin, Position::new(1, 1));
    }

    struct FocusPainter;
    impl Widget for FocusPainter {
        type State = ();
        fn render(&self, _state: &mut (), ctx: &mut RenderCtx<'_>) {
            ctx.focusable(true);
            let mark = if ctx.is_focused() { "F" } else { "." };
            ctx.set_string(Position::ORIGIN, mark, Style::new());
        }
    }

    #[test]
    fn focus_snapshot_reaches_render_ctx() {
        let mut buffer = Buffer::new(Size::new(1, 1));
        let mut store = StateStore::new();
        let id = WidgetId::ROOT.child(key("w"));
        store.begin_frame();
        let mut frame = Frame::with_focus(&mut buffer, &mut store, Some(id));
        frame.widget(key("w"), frame.area(), &FocusPainter);
        let _ = frame.finish();
        store.end_frame();
        assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "F");
    }

    struct RoleProbe;
    impl Widget for RoleProbe {
        type State = ();
        fn render(&self, _state: &mut (), ctx: &mut RenderCtx<'_>) {
            let accent = ctx.style(crate::theme::Role::Accent);
            ctx.set_string(Position::ORIGIN, "x", accent);
        }
    }

    #[test]
    fn themed_frame_threads_theme_to_render_ctx() {
        use crate::theme::{Role, Theme};
        let mut buffer = Buffer::new(Size::new(1, 1));
        let mut store = StateStore::new();
        let theme = Theme::catppuccin_mocha();
        store.begin_frame();
        let mut frame = Frame::themed(&mut buffer, &mut store, None, &theme);
        frame.widget(key("probe"), frame.area(), &RoleProbe);
        let _ = frame.finish();
        store.end_frame();
        // The painted cell carries the theme's accent style, not the default's.
        assert_eq!(
            buffer.get(Position::ORIGIN).unwrap().style,
            theme.style(Role::Accent)
        );
    }

    struct Activator;
    impl Widget for Activator {
        type State = ();
        fn render(&self, _state: &mut (), ctx: &mut RenderCtx<'_>) {
            ctx.focusable(true);
        }
        fn handle(_state: &mut (), event: &InputEvent, ctx: &mut HandleCtx<'_>) -> Handled {
            if matches!(
                event.as_key(),
                Some(KeyEvent {
                    key: InputKey::Enter,
                    ..
                })
            ) {
                ctx.emit(Outcome::Activated);
                return Handled::Yes;
            }
            Handled::No
        }
    }

    #[test]
    fn handler_thunk_dispatches_to_widget_handle() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.widget(key("act"), frame.area(), &Activator);
        let (_facts, handlers) = frame.into_parts();
        store.end_frame();

        let id = WidgetId::ROOT.child(key("act"));
        let handler = handlers.get(&id).unwrap();

        // Feed the thunk the same-typed state the store holds and dispatch Enter.
        store.begin_frame();
        let state = store.get_or_default::<()>(id);
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let mut ctx = HandleCtx::new(
            crate::widget::Phase::Bubble,
            Rect::default(),
            &mut outcomes,
            &mut request_focus,
        );
        let handled = handler(state, &InputEvent::key(InputKey::Enter), &mut ctx);
        store.end_frame();

        assert_eq!(handled, Handled::Yes);
        assert_eq!(outcomes, vec![Outcome::Activated]);
    }
}
