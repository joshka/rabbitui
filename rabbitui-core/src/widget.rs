//! The widget contract.
//!
//! Per `docs/adr/0001-programming-model.md` and `docs/adr/0008-widget-contract.md`:
//! a widget is a short-lived *spec* — a plain value describing what to show —
//! rendered against framework-retained per-identity state. Specs are built
//! fresh every frame from app data; anything that must survive the frame
//! (scroll, cursor, focus) lives in the framework's state store and is lent to
//! the widget as `&mut` during render.
//!
//! Widgets paint through a [`RenderCtx`], which owns clipping to the widget's
//! area and (from slice 3) collects frame facts: whether the widget is
//! focusable, and — for painting focus styles — whether it currently holds
//! focus.
//!
//! # Input: the handler is an associated function
//!
//! A widget spec dies after render, so nothing widget-shaped exists when an
//! event arrives (ADR 0006). rabbitui resolves this by registering a
//! **monomorphized handler thunk** at render time and routing events to it
//! against the *previous* frame's facts. The handler is
//! [`Widget::handle`] — an **associated function** (no `&self`), so it runs
//! without a spec, against retained state only. It defaults to ignoring every
//! event.
//!
//! # Examples
//!
//! A minimal stateless widget:
//!
//! ```
//! use rabbitui_core::geometry::Position;
//! use rabbitui_core::style::Style;
//! use rabbitui_core::widget::{RenderCtx, Widget};
//!
//! struct Label<'a>(&'a str);
//!
//! impl Widget for Label<'_> {
//!     type State = ();
//!
//!     fn render(&self, _state: &mut (), ctx: &mut RenderCtx<'_>) {
//!         ctx.set_string(Position::ORIGIN, self.0, Style::new());
//!     }
//! }
//! ```
//!
//! A focusable widget that reacts to Enter:
//!
//! ```
//! use rabbitui_core::input::{InputEvent, Key};
//! use rabbitui_core::outcome::Outcome;
//! use rabbitui_core::widget::{HandleCtx, Handled, RenderCtx, Widget};
//!
//! struct Trigger;
//!
//! impl Widget for Trigger {
//!     type State = ();
//!
//!     fn render(&self, _state: &mut (), ctx: &mut RenderCtx<'_>) {
//!         ctx.focusable(true);
//!     }
//!
//!     fn handle(_state: &mut (), event: &InputEvent, ctx: &mut HandleCtx<'_>) -> Handled {
//!         if matches!(event.as_key().map(|k| k.key), Some(Key::Enter)) {
//!             ctx.emit(Outcome::Activated);
//!             return Handled::Yes;
//!         }
//!         Handled::No
//!     }
//! }
//! ```

use crate::buffer::Buffer;
use crate::geometry::{Position, Rect, Size};
use crate::input::InputEvent;
use crate::outcome::Outcome;
use crate::style::Style;
use crate::theme::{Role, Theme};

/// The theme a [`RenderCtx::new`] carries when no theme is supplied — the
/// restrained dark default. A `const` so the reference is `'static` and every
/// default context borrows the same value.
const DEFAULT_THEME: &Theme = &Theme::dark();

/// A widget spec: a per-frame description of one widget, rendered against its
/// retained state.
///
/// `State` is the widget's framework-retained state — `()` for stateless
/// widgets. It must implement `Default` (the state a widget has the first
/// frame it appears) and is kept across frames by identity
/// (`docs/adr/0002-widget-identity.md`).
pub trait Widget {
    /// Framework-retained state for this widget kind.
    type State: Default + 'static;

    /// Paints the widget into its area and updates retained state.
    fn render(&self, state: &mut Self::State, ctx: &mut RenderCtx<'_>);

    /// Handles an event routed to this widget.
    ///
    /// This is an **associated function** — it takes no `&self`, so it runs
    /// without a spec instance, against retained `state` only (ADR 0006). The
    /// framework registers `W::handle` as a type-erased thunk when the widget is
    /// declared and calls it when routing an event whose target (or an ancestor
    /// on the capture/bubble path) is this widget's id.
    ///
    /// Return [`Handled::Yes`] to consume the event and stop propagation; the
    /// default ignores everything and returns [`Handled::No`].
    fn handle(state: &mut Self::State, event: &InputEvent, ctx: &mut HandleCtx<'_>) -> Handled {
        let _ = (state, event, ctx);
        Handled::No
    }
}

/// Whether a handler consumed an event.
///
/// [`Handled::Yes`] stops routing (no further capture/bubble, no framework
/// default like Tab traversal, no fallthrough to the app's `update`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handled {
    /// The event was consumed; stop propagating it.
    Yes,
    /// The event was ignored; keep propagating it.
    No,
}

/// Which leg of the two-phase dispatch a handler is being called for.
///
/// Routing runs capture (root → target) then bubble (target → root), per ADR
/// 0006. A handler can behave differently in each phase — a container might
/// swallow a shortcut on [`Phase::Capture`] before it reaches the target, while
/// most widgets only act on [`Phase::Bubble`] (which includes the target
/// itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Descending root → target (ancestors first).
    Capture,
    /// The target, then ascending target → root (ancestors last).
    Bubble,
}

/// The surface a widget paints through: its area of the buffer, pre-clipped,
/// plus the frame-facts it declares as it renders.
///
/// Positions passed to paint methods are relative to the widget's own area;
/// painting outside the area is clipped, never an error.
///
/// A widget marks itself focusable with [`focusable`](Self::focusable) and reads
/// its own focus state with [`is_focused`](Self::is_focused) to paint focus
/// styles. Both feed the frame facts the framework routes input through.
///
/// The context carries the active [`Theme`]; a widget resolves a semantic
/// [`Role`] to a concrete [`Style`] with [`style`](Self::style) rather than
/// hard-coding colors (ADR 0007). Contexts built with [`new`](Self::new) carry
/// the [`Theme::default`]; [`new_themed`](Self::new_themed) supplies a specific
/// theme (the [`Frame`](crate::frame::Frame) uses it to thread its theme in).
#[derive(Debug)]
pub struct RenderCtx<'a> {
    buffer: &'a mut Buffer,
    /// The widget's area in buffer coordinates, already clipped to the buffer.
    area: Rect,
    /// The active theme, for resolving roles to styles.
    theme: &'a Theme,
    /// Whether the framework reports this widget as currently focused.
    focused: bool,
    /// Whether the widget declared itself focusable this frame. The frame reads
    /// this back after `render` to record the focus fact.
    focusable: bool,
    /// A scroll-into-view rectangle the widget requested this frame (in
    /// area-relative coordinates), read back by the frame to record a
    /// visibility-request fact (slice-7 plumbing). At most one is kept — the
    /// last request wins.
    visibility: Option<Rect>,
}

impl<'a> RenderCtx<'a> {
    /// Creates a context painting into `area` of `buffer`, using the
    /// [`Theme::default`].
    ///
    /// `area` is clipped to the buffer's bounds; a fully out-of-bounds area
    /// yields a context whose paints are all no-ops. The context starts
    /// non-focusable; `focused` is the framework's current focus verdict for
    /// this widget. Use [`new_themed`](Self::new_themed) to supply a theme other
    /// than the default.
    #[must_use]
    pub fn new(buffer: &'a mut Buffer, area: Rect, focused: bool) -> Self {
        Self::new_themed(buffer, area, focused, DEFAULT_THEME)
    }

    /// Creates a context painting into `area` of `buffer` against `theme`.
    ///
    /// Like [`new`](Self::new), but resolves roles through the supplied `theme`
    /// rather than the default. The [`Frame`](crate::frame::Frame) uses this to
    /// thread the frame's theme to every widget it declares.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::{Rect, Size};
    /// use rabbitui_core::theme::{Role, Theme};
    /// use rabbitui_core::widget::RenderCtx;
    ///
    /// let mut buffer = Buffer::new(Size::new(4, 1));
    /// let theme = Theme::catppuccin_mocha();
    /// let ctx = RenderCtx::new_themed(&mut buffer, Rect::from_size(Size::new(4, 1)), false, &theme);
    /// assert_eq!(ctx.style(Role::Accent), theme.style(Role::Accent));
    /// ```
    #[must_use]
    pub fn new_themed(buffer: &'a mut Buffer, area: Rect, focused: bool, theme: &'a Theme) -> Self {
        let bounds = Rect::from_size(buffer.size());
        let area = area.intersection(bounds);
        Self {
            buffer,
            area,
            theme,
            focused,
            focusable: false,
            visibility: None,
        }
    }

    /// The widget's area size (relative coordinates run from the origin to
    /// this size).
    #[must_use]
    pub fn area(&self) -> Rect {
        Rect::from_size(self.area.size)
    }

    /// The widget's area size — the shorthand for "how much room do I have".
    #[must_use]
    pub fn size(&self) -> crate::geometry::Size {
        self.area.size
    }

    /// Declares whether this widget can hold keyboard focus this frame.
    ///
    /// Per-instance (ADR 0006): the same widget kind may opt in on one call site
    /// and out on another (a disabled control passes `false`). The framework
    /// records this as a focus fact; only focusable widgets appear in tab
    /// traversal.
    pub fn focusable(&mut self, focusable: bool) {
        self.focusable = focusable;
    }

    /// Whether the framework reports this widget as currently focused.
    ///
    /// A render-time query, for painting focus styles (a reversed background, a
    /// highlighted border). Focus itself is framework state keyed by identity
    /// (ADR 0002/0006); this is the read-only view of it during render.
    #[must_use]
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Whether the widget declared itself focusable this frame.
    ///
    /// Read by [`Frame`](crate::frame::Frame) after `render` to record the focus
    /// fact; not typically called by widgets.
    #[must_use]
    pub fn is_focusable(&self) -> bool {
        self.focusable
    }

    /// Requests that `area` (relative to this widget's own area) be scrolled into
    /// view (slice-7 plumbing, ADR 0006's scroll-into-view request).
    ///
    /// The frame records this as a
    /// [`VisibilityRequest`](crate::facts::VisibilityRequest) fact keyed by the
    /// widget's identity, resolving the relative rectangle against the widget's
    /// absolute area. No scrollable container consumes it yet — this establishes
    /// the contract the container work (catalog phase) will implement. Calling
    /// more than once keeps the last request.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::{Position, Rect, Size};
    /// use rabbitui_core::widget::RenderCtx;
    ///
    /// let mut buffer = Buffer::new(Size::new(8, 4));
    /// let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(8, 4)), false);
    /// ctx.request_visibility(Rect::new(Position::new(0, 2), Size::new(8, 1)));
    /// ```
    pub fn request_visibility(&mut self, area: Rect) {
        self.visibility = Some(area);
    }

    /// The scroll-into-view rectangle the widget requested this frame, resolved
    /// to absolute buffer coordinates, or `None`.
    ///
    /// Read by [`Frame`](crate::frame::Frame) after `render` to record the
    /// visibility fact; not typically called by widgets. The request's origin is
    /// offset by the widget's area origin, so the recorded rectangle is in the
    /// same absolute coordinates as [`FactEntry`](crate::facts::FactEntry) areas.
    #[must_use]
    pub fn requested_visibility(&self) -> Option<Rect> {
        self.visibility.map(|relative| {
            let origin = Position::new(
                self.area.origin.x.saturating_add(relative.origin.x),
                self.area.origin.y.saturating_add(relative.origin.y),
            );
            Rect::new(origin, Size::new(relative.size.width, relative.size.height))
        })
    }

    /// The concrete [`Style`] the active theme maps `role` to.
    ///
    /// The sanctioned way a widget styles itself (ADR 0007): it names a semantic
    /// [`Role`] and the framework resolves it against the frame's theme, so the
    /// widget never hard-codes a color and re-skinning is a theme swap.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::{Position, Rect, Size};
    /// use rabbitui_core::theme::Role;
    /// use rabbitui_core::widget::RenderCtx;
    ///
    /// let mut buffer = Buffer::new(Size::new(5, 1));
    /// let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(5, 1)), false);
    /// let text_style = ctx.style(Role::Text);
    /// ctx.set_string(Position::ORIGIN, "hi", text_style);
    /// ```
    #[must_use]
    pub fn style(&self, role: Role) -> Style {
        self.theme.style(role)
    }

    /// The active theme, for a widget that needs several roles at once.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        self.theme
    }

    /// Writes `text` at `position` (relative to the widget's area) in
    /// `style`, clipped to the area's right edge.
    pub fn set_string(&mut self, position: Position, text: &str, style: Style) {
        if position.y >= self.area.size.height || position.x >= self.area.size.width {
            return;
        }
        let absolute = Position::new(
            self.area.origin.x + position.x,
            self.area.origin.y + position.y,
        );
        let max_width = usize::from(self.area.size.width - position.x);
        self.buffer.set_stringn(absolute, text, style, max_width);
    }
}

/// The surface a handler acts through during event routing.
///
/// A handler does **not** paint (it has no buffer) and does **not** mutate
/// render state mid-dispatch (Brick's queued-request discipline, ADR 0006 §1).
/// It carries the current dispatch [`Phase`], the widget's area (in absolute
/// buffer coordinates, from last frame's facts), and two request channels:
/// [`emit`](Self::emit) to report a typed [`Outcome`] to the app, and
/// [`request_focus`](Self::request_focus) to ask the framework to focus this
/// widget next.
#[derive(Debug)]
pub struct HandleCtx<'a> {
    phase: Phase,
    area: Rect,
    outcomes: &'a mut Vec<Outcome>,
    request_focus: &'a mut bool,
}

impl<'a> HandleCtx<'a> {
    /// Creates a handler context for one handler invocation.
    ///
    /// `outcomes` collects everything this handler emits, and `request_focus` is
    /// set if the handler asks for focus. The framework owns both and drains
    /// them after the handler returns.
    #[must_use]
    pub fn new(
        phase: Phase,
        area: Rect,
        outcomes: &'a mut Vec<Outcome>,
        request_focus: &'a mut bool,
    ) -> Self {
        Self {
            phase,
            area,
            outcomes,
            request_focus,
        }
    }

    /// The dispatch phase this call is part of (capture or bubble).
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The widget's area in absolute buffer coordinates (from last frame's
    /// facts).
    #[must_use]
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Reports a typed outcome to the app.
    ///
    /// The framework collects the frame's outcomes and delivers them to the
    /// app's `update` in the same call as the event (ADR 0001). A handler may
    /// emit more than once.
    pub fn emit(&mut self, outcome: Outcome) {
        self.outcomes.push(outcome);
    }

    /// Asks the framework to move focus to this widget.
    ///
    /// Applied after routing; the widget must be focusable in the current facts
    /// or the request is ignored.
    pub fn request_focus(&mut self) {
        *self.request_focus = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Size;

    #[test]
    fn paints_relative_to_area() {
        let mut buffer = Buffer::new(Size::new(10, 3));
        let area = Rect::new(Position::new(2, 1), Size::new(5, 1));
        let mut ctx = RenderCtx::new(&mut buffer, area, false);
        ctx.set_string(Position::new(1, 0), "hi", Style::new());
        assert_eq!(buffer.get(Position::new(3, 1)).unwrap().symbol, "h");
        assert_eq!(buffer.get(Position::new(4, 1)).unwrap().symbol, "i");
    }

    #[test]
    fn clips_to_area_not_buffer() {
        let mut buffer = Buffer::new(Size::new(10, 3));
        let area = Rect::new(Position::new(2, 1), Size::new(3, 1));
        let mut ctx = RenderCtx::new(&mut buffer, area, false);
        ctx.set_string(Position::ORIGIN, "abcdef", Style::new());
        // "abc" fits the 3-wide area; "def" is clipped even though the buffer
        // continues.
        assert_eq!(buffer.get(Position::new(4, 1)).unwrap().symbol, "c");
        assert_eq!(buffer.get(Position::new(5, 1)).unwrap().symbol, " ");
    }

    #[test]
    fn out_of_area_positions_are_no_ops() {
        let mut buffer = Buffer::new(Size::new(4, 2));
        let area = Rect::new(Position::ORIGIN, Size::new(4, 1));
        let mut ctx = RenderCtx::new(&mut buffer, area, false);
        ctx.set_string(Position::new(0, 5), "nope", Style::new());
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, " ");
    }

    #[test]
    fn area_outside_buffer_is_empty() {
        let mut buffer = Buffer::new(Size::new(4, 2));
        let area = Rect::new(Position::new(10, 10), Size::new(5, 5));
        let ctx = RenderCtx::new(&mut buffer, area, false);
        assert!(ctx.area().is_empty());
    }

    #[test]
    fn new_uses_default_theme_and_new_themed_overrides_it() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let area = Rect::from_size(Size::new(4, 1));
        // A default context resolves against Theme::default().
        let ctx = RenderCtx::new(&mut buffer, area, false);
        assert_eq!(
            ctx.style(Role::Accent),
            Theme::default().style(Role::Accent)
        );
        let _ = ctx;
        // A themed context resolves against the supplied theme.
        let theme = Theme::catppuccin_mocha();
        let ctx = RenderCtx::new_themed(&mut buffer, area, false, &theme);
        assert_eq!(ctx.style(Role::Accent), theme.style(Role::Accent));
    }

    #[test]
    fn focus_flags_default_off_and_are_settable() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let area = Rect::from_size(Size::new(4, 1));
        let mut ctx = RenderCtx::new(&mut buffer, area, true);
        assert!(ctx.is_focused());
        assert!(!ctx.is_focusable());
        ctx.focusable(true);
        assert!(ctx.is_focusable());
    }

    #[test]
    fn request_visibility_records_area_relative_rect_in_absolute_coords() {
        let mut buffer = Buffer::new(Size::new(10, 5));
        let area = Rect::new(Position::new(2, 1), Size::new(6, 3));
        let mut ctx = RenderCtx::new(&mut buffer, area, false);
        assert!(ctx.requested_visibility().is_none());
        // Request row 1 (relative) of the widget; the frame resolves it to
        // absolute row 2 (area origin y=1 + relative y=1).
        ctx.request_visibility(Rect::new(Position::new(0, 1), Size::new(6, 1)));
        let resolved = ctx.requested_visibility().unwrap();
        assert_eq!(resolved.origin, Position::new(2, 2));
        assert_eq!(resolved.size, Size::new(6, 1));
    }

    #[test]
    fn handle_ctx_collects_outcomes_and_focus_request() {
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        {
            let mut ctx = HandleCtx::new(
                Phase::Bubble,
                Rect::from_size(Size::new(4, 1)),
                &mut outcomes,
                &mut request_focus,
            );
            assert_eq!(ctx.phase(), Phase::Bubble);
            ctx.emit(Outcome::Activated);
            ctx.request_focus();
        }
        assert_eq!(outcomes, vec![Outcome::Activated]);
        assert!(request_focus);
    }

    #[test]
    fn default_handle_ignores_events() {
        use crate::input::{InputEvent, Key};

        struct Ignorer;
        impl Widget for Ignorer {
            type State = ();
            fn render(&self, _state: &mut (), _ctx: &mut RenderCtx<'_>) {}
        }

        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let mut ctx = HandleCtx::new(
            Phase::Bubble,
            Rect::default(),
            &mut outcomes,
            &mut request_focus,
        );
        let handled = Ignorer::handle(&mut (), &InputEvent::key(Key::Enter), &mut ctx);
        assert_eq!(handled, Handled::No);
        assert!(outcomes.is_empty());
    }
}
