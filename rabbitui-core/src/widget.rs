//! The widget contract.
//!
//! Per `docs/adr/0001-programming-model.md` and `docs/adr/0008-widget-contract.md`:
//! a widget is a short-lived *spec* — a plain value describing what to show —
//! rendered against framework-retained per-identity state. Specs are built
//! fresh every frame from app data; anything that must survive the frame
//! (scroll, cursor, focus) lives in the framework's state store and is lent to
//! the widget as `&mut` during render.
//!
//! Widgets paint through a [`RenderContext`], which owns clipping to the widget's
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
//! use rabbitui_core::widget::{RenderContext, Widget};
//!
//! struct Label<'a>(&'a str);
//!
//! impl Widget for Label<'_> {
//!     type State = ();
//!
//!     fn render(&self, _state: &mut (), ctx: &mut RenderContext<'_>) {
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
//! use rabbitui_core::widget::{HandleContext, Handled, RenderContext, Widget};
//!
//! struct Trigger;
//!
//! impl Widget for Trigger {
//!     type State = ();
//!
//!     fn render(&self, _state: &mut (), ctx: &mut RenderContext<'_>) {
//!         ctx.focusable(true);
//!     }
//!
//!     fn handle(_state: &mut (), event: &InputEvent, ctx: &mut HandleContext<'_>) -> Handled {
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

/// The theme a [`RenderContext::new`] carries when no theme is supplied — the
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
    fn render(&self, state: &mut Self::State, ctx: &mut RenderContext<'_>);

    /// The height this widget wants at `width`, given its retained `state`.
    ///
    /// The intrinsic-measurement half of the contract (ADR 0004, deferred there
    /// and landed in Arc 2B). Containers stack children at their measured heights;
    /// [`Frame::scroll`](crate::frame::Frame::scroll) virtualizes on it — an item
    /// outside the viewport is measured (to advance the stacking cursor and size
    /// the scrollbar) but never painted. It must be **cheap** (called per frame
    /// per candidate item, including off-screen ones) and must **not paint** — it
    /// has no [`RenderContext`], only `state` and `width`.
    ///
    /// The default is one row: a label, a button, a single-line field. Widgets
    /// whose height depends on content or state override it — the catalog's `Text`
    /// returns its line count (wrapped, when wrap is on), a disclosure cell returns
    /// 1 collapsed and 1 + body when expanded.
    ///
    /// `state` is lent read-only through [`StateStore::peek`](crate::store::StateStore::peek),
    /// so measuring never marks the widget declared. When the widget has no
    /// retained state yet (its first frame), the framework measures against
    /// `State::default()`.
    fn desired_height(&self, state: &Self::State, width: u16) -> u16 {
        let _ = (state, width);
        1
    }

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
    fn handle(state: &mut Self::State, event: &InputEvent, ctx: &mut HandleContext<'_>) -> Handled {
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
/// # Partial visibility (the offset+mask model)
///
/// A scroll container can show only the *bottom* rows of an item whose top has
/// scrolled above the viewport. The context expresses this with a hidden-top
/// mask ([`with_hidden_top`](Self::with_hidden_top), set by the container —
/// `docs/design/render-space.md`): the widget's logical space stays
/// `0..size().height` and it renders its whole self; the context drops writes
/// to the hidden rows and translates the rest onto the visible area. A widget
/// never needs to know it is clipped, and coordinates stay local `u16`s.
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
pub struct RenderContext<'a> {
    buffer: &'a mut Buffer,
    /// The widget's *visible* area in buffer coordinates, already clipped to
    /// the buffer.
    area: Rect,
    /// Rows of the widget's logical extent scrolled above the visible area
    /// (`docs/design/render-space.md`, the offset+mask model). The widget's
    /// logical height is `hidden_top + area.size.height`; writes to logical
    /// rows below `hidden_top` are dropped, the rest translate down by
    /// `hidden_top` onto `area`. Zero for a fully-visible widget.
    hidden_top: u16,
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
    /// The accessibility role the widget declared this frame (ADR arc4 §5),
    /// read back by the frame onto the widget's fact. Defaults to
    /// [`SemanticRole::None`](crate::accessibility::SemanticRole::None).
    role: crate::accessibility::SemanticRole,
    /// The accessible label the widget declared this frame, read back by the frame
    /// into the facts' label side table. `None` until a widget sets one.
    label: Option<String>,
}

impl<'a> RenderContext<'a> {
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
    /// use rabbitui_core::widget::RenderContext;
    ///
    /// let mut buffer = Buffer::new(Size::new(4, 1));
    /// let theme = Theme::catppuccin_mocha();
    /// let ctx = RenderContext::new_themed(&mut buffer, Rect::from_size(Size::new(4, 1)), false, &theme);
    /// assert_eq!(ctx.style(Role::Accent), theme.style(Role::Accent));
    /// ```
    #[must_use]
    pub fn new_themed(buffer: &'a mut Buffer, area: Rect, focused: bool, theme: &'a Theme) -> Self {
        let bounds = Rect::from_size(buffer.size());
        let area = area.intersection(bounds);
        Self {
            buffer,
            area,
            hidden_top: 0,
            theme,
            focused,
            focusable: false,
            visibility: None,
            role: crate::accessibility::SemanticRole::None,
            label: None,
        }
    }

    /// Masks the top `rows` of the widget's logical extent as scrolled out of
    /// view (the offset+mask model, `docs/design/render-space.md`).
    ///
    /// The context's logical height grows to `rows + visible height`: the
    /// widget renders its whole self in `0..size().height` local coordinates,
    /// writes to the first `rows` logical rows are dropped, and the remainder
    /// translate onto the visible area. Scroll containers use this to show the
    /// *bottom* slice of a top-clipped item — shrinking the area instead would
    /// show the wrong (top) slice. Widgets themselves never call this.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::{Position, Rect, Size};
    /// use rabbitui_core::style::Style;
    /// use rabbitui_core::widget::RenderContext;
    ///
    /// let mut buffer = Buffer::new(Size::new(4, 1));
    /// let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(4, 1)), false)
    ///     .with_hidden_top(2);
    /// // Logical height 3, but rows 0 and 1 are hidden; only row 2 paints.
    /// assert_eq!(ctx.size().height, 3);
    /// ctx.set_string(Position::new(0, 0), "hidden", Style::new());
    /// ctx.set_string(Position::new(0, 2), "shown", Style::new());
    /// assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "s");
    /// ```
    #[must_use]
    pub fn with_hidden_top(mut self, rows: u16) -> Self {
        self.hidden_top = rows;
        self
    }

    /// The widget's logical area (relative coordinates run from the origin to
    /// this size). Includes any hidden-top rows, so a partially-scrolled
    /// widget still sees its full extent.
    #[must_use]
    pub fn area(&self) -> Rect {
        Rect::from_size(self.size())
    }

    /// The widget's logical size — the shorthand for "how much room do I
    /// have". Includes any hidden-top rows (see
    /// [`with_hidden_top`](Self::with_hidden_top)).
    #[must_use]
    pub fn size(&self) -> crate::geometry::Size {
        Size::new(
            self.area.size.width,
            self.area.size.height.saturating_add(self.hidden_top),
        )
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
    /// use rabbitui_core::widget::RenderContext;
    ///
    /// let mut buffer = Buffer::new(Size::new(8, 4));
    /// let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(8, 4)), false);
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
            // Logical rows above the hidden-top mask clamp to the visible top;
            // the rect's *bottom* edge stays correct either way, which is what
            // the scroll container's reveal math consumes.
            let (y, height) = if relative.origin.y >= self.hidden_top {
                (relative.origin.y - self.hidden_top, relative.size.height)
            } else {
                let hidden = self.hidden_top - relative.origin.y;
                (0, relative.size.height.saturating_sub(hidden))
            };
            let origin = Position::new(
                self.area.origin.x.saturating_add(relative.origin.x),
                self.area.origin.y.saturating_add(y),
            );
            Rect::new(origin, Size::new(relative.size.width, height))
        })
    }

    /// Declares this widget's **accessibility role** (ADR arc4 §5) — what kind of
    /// control it is (button, text field, list, …).
    ///
    /// Recorded onto the widget's [`FactEntry`](crate::facts::FactEntry) for a
    /// future assistive-technology exporter; nothing consumes it yet. Calling more
    /// than once keeps the last role. The catalog widgets set an appropriate role;
    /// a purely-decorative widget leaves it at the
    /// [`SemanticRole::None`](crate::accessibility::SemanticRole::None) default.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::accessibility::SemanticRole;
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::geometry::{Rect, Size};
    /// use rabbitui_core::widget::RenderContext;
    ///
    /// let mut buffer = Buffer::new(Size::new(6, 1));
    /// let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(6, 1)), false);
    /// ctx.semantic_role(SemanticRole::Button);
    /// ctx.label("Save");
    /// ```
    pub fn semantic_role(&mut self, role: crate::accessibility::SemanticRole) {
        self.role = role;
    }

    /// Declares this widget's **accessible label** (ADR arc4 §5) — the human name
    /// an assistive technology would announce (a button's text, a field's purpose).
    ///
    /// Recorded into the frame's label side table
    /// ([`FrameFacts::label`](crate::facts::FrameFacts::label)) keyed by the
    /// widget's identity; nothing consumes it yet. Calling more than once keeps the
    /// last label. See [`semantic_role`](Self::semantic_role) for an example.
    pub fn label(&mut self, label: impl Into<String>) {
        self.label = Some(label.into());
    }

    /// The accessibility role the widget declared this frame, read back by
    /// [`Frame`](crate::frame::Frame) onto the fact. Not typically called by
    /// widgets.
    #[must_use]
    pub fn declared_role(&self) -> crate::accessibility::SemanticRole {
        self.role
    }

    /// The accessible label the widget declared this frame, read back by
    /// [`Frame`](crate::frame::Frame) into the label table. Not typically called by
    /// widgets.
    #[must_use]
    pub fn declared_label(&self) -> Option<&str> {
        self.label.as_deref()
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
    /// use rabbitui_core::widget::RenderContext;
    ///
    /// let mut buffer = Buffer::new(Size::new(5, 1));
    /// let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(5, 1)), false);
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

    /// Writes `text` at `position` (relative to the widget's logical area) in
    /// `style`, clipped to the area's right edge. Rows masked by
    /// [`with_hidden_top`](Self::with_hidden_top) are dropped whole — the mask
    /// slices rows, never columns, so it can never bisect a wide grapheme (the
    /// right-edge clip goes through the buffer's shared width oracle).
    pub fn set_string(&mut self, position: Position, text: &str, style: Style) {
        // Logical row → visible row: rows above the mask are dropped.
        let Some(y) = position.y.checked_sub(self.hidden_top) else {
            return;
        };
        if y >= self.area.size.height || position.x >= self.area.size.width {
            return;
        }
        let absolute = Position::new(self.area.origin.x + position.x, self.area.origin.y + y);
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
pub struct HandleContext<'a> {
    phase: Phase,
    area: Rect,
    outcomes: &'a mut Vec<Outcome>,
    request_focus: &'a mut bool,
}

impl<'a> HandleContext<'a> {
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
        let mut ctx = RenderContext::new(&mut buffer, area, false);
        ctx.set_string(Position::new(1, 0), "hi", Style::new());
        assert_eq!(buffer.get(Position::new(3, 1)).unwrap().symbol, "h");
        assert_eq!(buffer.get(Position::new(4, 1)).unwrap().symbol, "i");
    }

    #[test]
    fn clips_to_area_not_buffer() {
        let mut buffer = Buffer::new(Size::new(10, 3));
        let area = Rect::new(Position::new(2, 1), Size::new(3, 1));
        let mut ctx = RenderContext::new(&mut buffer, area, false);
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
        let mut ctx = RenderContext::new(&mut buffer, area, false);
        ctx.set_string(Position::new(0, 5), "nope", Style::new());
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, " ");
    }

    #[test]
    fn hidden_top_drops_masked_rows_and_translates_the_rest() {
        // A 5-row-tall widget with its top 2 rows scrolled out, 2 rows visible:
        // logical rows 0..2 drop, logical rows 2..4 land on visible rows 0..1,
        // and logical row 4 falls past the visible bottom (bottom truncation).
        let mut buffer = Buffer::new(Size::new(6, 3));
        let area = Rect::new(Position::new(0, 1), Size::new(6, 2));
        let mut ctx = RenderContext::new(&mut buffer, area, false).with_hidden_top(2);
        assert_eq!(ctx.size(), Size::new(6, 4));
        assert_eq!(ctx.area(), Rect::from_size(Size::new(6, 4)));
        for (row, label) in ["r0", "r1", "r2", "r3", "r4"].iter().enumerate() {
            ctx.set_string(Position::new(0, row as u16), label, Style::new());
        }
        // The buffer row above the area is untouched; the visible rows show the
        // widget's *bottom* slice (rows 2 and 3), not its top.
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, " ");
        assert_eq!(buffer.get(Position::new(1, 1)).unwrap().symbol, "2");
        assert_eq!(buffer.get(Position::new(1, 2)).unwrap().symbol, "3");
    }

    #[test]
    fn hidden_top_zero_is_the_plain_context() {
        let mut buffer = Buffer::new(Size::new(4, 2));
        let area = Rect::from_size(Size::new(4, 2));
        let mut ctx = RenderContext::new(&mut buffer, area, false).with_hidden_top(0);
        assert_eq!(ctx.size(), Size::new(4, 2));
        ctx.set_string(Position::ORIGIN, "x", Style::new());
        assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "x");
    }

    #[test]
    fn hidden_top_visibility_request_keeps_the_bottom_edge() {
        // A widget 4 rows tall with 2 hidden asks for its whole logical extent:
        // the resolved rect clamps its top to the visible area but its bottom
        // stays at the true bottom row — the edge the reveal math consumes.
        let mut buffer = Buffer::new(Size::new(4, 4));
        let area = Rect::new(Position::new(0, 1), Size::new(4, 2));
        let mut ctx = RenderContext::new(&mut buffer, area, false).with_hidden_top(2);
        ctx.request_visibility(Rect::new(Position::ORIGIN, Size::new(4, 4)));
        let resolved = ctx.requested_visibility().unwrap();
        assert_eq!(resolved.origin, Position::new(0, 1));
        assert_eq!(resolved.size.height, 2);
        assert_eq!(resolved.bottom(), 3);
        // A request wholly below the mask translates without clamping.
        ctx.request_visibility(Rect::new(Position::new(0, 3), Size::new(4, 1)));
        let resolved = ctx.requested_visibility().unwrap();
        assert_eq!(resolved.origin, Position::new(0, 2));
        assert_eq!(resolved.size.height, 1);
    }

    #[test]
    fn area_outside_buffer_is_empty() {
        let mut buffer = Buffer::new(Size::new(4, 2));
        let area = Rect::new(Position::new(10, 10), Size::new(5, 5));
        let ctx = RenderContext::new(&mut buffer, area, false);
        assert!(ctx.area().is_empty());
    }

    #[test]
    fn new_uses_default_theme_and_new_themed_overrides_it() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let area = Rect::from_size(Size::new(4, 1));
        // A default context resolves against Theme::default().
        let ctx = RenderContext::new(&mut buffer, area, false);
        assert_eq!(
            ctx.style(Role::Accent),
            Theme::default().style(Role::Accent)
        );
        let _ = ctx;
        // A themed context resolves against the supplied theme.
        let theme = Theme::catppuccin_mocha();
        let ctx = RenderContext::new_themed(&mut buffer, area, false, &theme);
        assert_eq!(ctx.style(Role::Accent), theme.style(Role::Accent));
    }

    #[test]
    fn focus_flags_default_off_and_are_settable() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let area = Rect::from_size(Size::new(4, 1));
        let mut ctx = RenderContext::new(&mut buffer, area, true);
        assert!(ctx.is_focused());
        assert!(!ctx.is_focusable());
        ctx.focusable(true);
        assert!(ctx.is_focusable());
    }

    #[test]
    fn request_visibility_records_area_relative_rect_in_absolute_coords() {
        let mut buffer = Buffer::new(Size::new(10, 5));
        let area = Rect::new(Position::new(2, 1), Size::new(6, 3));
        let mut ctx = RenderContext::new(&mut buffer, area, false);
        assert!(ctx.requested_visibility().is_none());
        // Request row 1 (relative) of the widget; the frame resolves it to
        // absolute row 2 (area origin y=1 + relative y=1).
        ctx.request_visibility(Rect::new(Position::new(0, 1), Size::new(6, 1)));
        let resolved = ctx.requested_visibility().unwrap();
        assert_eq!(resolved.origin, Position::new(2, 2));
        assert_eq!(resolved.size, Size::new(6, 1));
    }

    #[test]
    fn a11y_role_and_label_default_off_and_are_settable() {
        use crate::accessibility::SemanticRole;
        let mut buffer = Buffer::new(Size::new(6, 1));
        let area = Rect::from_size(Size::new(6, 1));
        let mut ctx = RenderContext::new(&mut buffer, area, false);
        // Defaults: no role, no label.
        assert_eq!(ctx.declared_role(), SemanticRole::None);
        assert_eq!(ctx.declared_label(), None);
        // Set both; the last write wins (label set twice).
        ctx.semantic_role(SemanticRole::Button);
        ctx.label("first");
        ctx.label("Save");
        assert_eq!(ctx.declared_role(), SemanticRole::Button);
        assert_eq!(ctx.declared_label(), Some("Save"));
    }

    #[test]
    fn handle_ctx_collects_outcomes_and_focus_request() {
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        {
            let mut ctx = HandleContext::new(
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
            fn render(&self, _state: &mut (), _ctx: &mut RenderContext<'_>) {}
        }

        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let mut ctx = HandleContext::new(
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
