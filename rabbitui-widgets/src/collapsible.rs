//! A collapsible cell: a header that toggles a body open or closed.

use rabbitui_core::a11y::SemanticRole;
use rabbitui_core::geometry::Position;
use rabbitui_core::input::{InputEvent, Key, MouseButton, MouseKind};
use rabbitui_core::outcome::Outcome;
use rabbitui_core::theme::Role;
use rabbitui_core::widget::{HandleCtx, Handled, RenderCtx, Widget};

/// A header line with a body that expands and collapses.
///
/// `Collapsible` is the alt-screen transcript's disclosure cell
/// (`docs/design/slice8-agent-chrome.md`): a header row (with a `▸`/`▾`
/// disclosure marker) over a body that shows only when expanded. Pressing Enter
/// while it is focused, or clicking its header row, toggles it and emits
/// [`Outcome::Toggled`] carrying the new *collapsed* state. It is focusable so it
/// joins tab traversal.
///
/// # Retained, identity-keyed collapsed state
///
/// Whether a cell is collapsed is **framework-retained state**
/// ([`CollapsibleState`]), keyed by the widget's identity (ADR 0002), so a cell
/// the user opened stays open across frames while the spec is rebuilt each frame.
/// The spec carries only the header, body, and the *initial* collapsed default;
/// the store owns the live toggle.
///
/// # Initial state: the builder default
///
/// [`default_collapsed`](Self::default_collapsed) sets the state a cell has the
/// **first** frame it appears — the transcript defaults tool cells collapsed and
/// assistant cells expanded. It applies once (on first render for that identity)
/// and is thereafter ignored: user toggles win, so re-declaring the cell with a
/// different default does not clobber the user's choice.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::Collapsible;
///
/// // A tool cell, collapsed by default.
/// let tool = Collapsible::new("▸ ran cargo test — 396 passed", "…full output…")
///     .default_collapsed(true);
/// assert!(tool.get_default_collapsed());
///
/// // An assistant cell, expanded by default.
/// let assistant = Collapsible::new("assistant", "the reply body");
/// assert!(!assistant.get_default_collapsed());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Collapsible<'a> {
    header: &'a str,
    body: &'a str,
    default_collapsed: bool,
}

impl<'a> Collapsible<'a> {
    /// Creates a collapsible with `header` and `body`, expanded by default.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::Collapsible;
    ///
    /// let cell = Collapsible::new("summary", "detail");
    /// assert_eq!(cell.header(), "summary");
    /// assert_eq!(cell.body(), "detail");
    /// ```
    #[must_use]
    pub const fn new(header: &'a str, body: &'a str) -> Self {
        Self {
            header,
            body,
            default_collapsed: false,
        }
    }

    /// Sets the collapsed state the cell has the first frame it appears.
    ///
    /// Applied once per identity, then retained state takes over (see the type
    /// docs). The transcript uses `true` for tool cells and the default `false`
    /// for assistant cells.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::Collapsible;
    ///
    /// let cell = Collapsible::new("h", "b").default_collapsed(true);
    /// assert!(cell.get_default_collapsed());
    /// ```
    #[must_use]
    pub const fn default_collapsed(mut self, collapsed: bool) -> Self {
        self.default_collapsed = collapsed;
        self
    }

    /// The header text.
    #[must_use]
    pub const fn header(&self) -> &'a str {
        self.header
    }

    /// The body text (shown only when expanded).
    #[must_use]
    pub const fn body(&self) -> &'a str {
        self.body
    }

    /// The initial collapsed default set by [`default_collapsed`](Self::default_collapsed).
    #[must_use]
    pub const fn get_default_collapsed(&self) -> bool {
        self.default_collapsed
    }
}

/// The retained state of a [`Collapsible`]: whether it is collapsed, and whether
/// the builder default has been applied yet.
///
/// Framework-owned, keyed by identity (ADR 0002). `initialized` records that the
/// first render has consumed the spec's [`Collapsible::default_collapsed`], so
/// later frames keep the user's toggle rather than re-applying the default.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::collapsible::CollapsibleState;
///
/// // A fresh cell has not yet applied its builder default.
/// let state = CollapsibleState::default();
/// assert!(!state.is_collapsed());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CollapsibleState {
    collapsed: bool,
    initialized: bool,
}

impl CollapsibleState {
    /// Whether the cell is currently collapsed.
    #[must_use]
    pub const fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    /// Toggles the collapsed state, returning the new value.
    ///
    /// The programmatic companion to the Enter/click toggle: a widget command
    /// can drive it (`update.widget::<Collapsible>(path, |s| { s.toggle(); })`).
    /// Toggling also marks the state initialized, so a later re-declared default
    /// never overrides it.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::collapsible::CollapsibleState;
    ///
    /// let mut state = CollapsibleState::default();
    /// assert!(state.toggle());
    /// assert!(state.is_collapsed());
    /// ```
    pub fn toggle(&mut self) -> bool {
        self.collapsed = !self.collapsed;
        self.initialized = true;
        self.collapsed
    }

    /// Sets the collapsed state directly, marking the cell initialized.
    pub fn set_collapsed(&mut self, collapsed: bool) {
        self.collapsed = collapsed;
        self.initialized = true;
    }

    /// Applies the spec's initial default the first time the cell renders.
    fn apply_default(&mut self, default_collapsed: bool) {
        if !self.initialized {
            self.collapsed = default_collapsed;
            self.initialized = true;
        }
    }
}

impl Collapsible<'_> {
    /// The collapsed state to measure against: the retained state once it has
    /// been initialized, otherwise the builder default (which the first render
    /// will apply). This keeps [`desired_height`](Widget::desired_height) honest
    /// on the first frame, before any render has consumed the default.
    fn effective_collapsed(&self, state: &CollapsibleState) -> bool {
        if state.initialized {
            state.collapsed
        } else {
            self.default_collapsed
        }
    }
}

impl Widget for Collapsible<'_> {
    type State = CollapsibleState;

    fn render(&self, state: &mut CollapsibleState, ctx: &mut RenderCtx<'_>) {
        ctx.focusable(true);
        // A11y groundwork (ADR arc4 §5): a disclosure header, labelled by its title.
        ctx.semantic_role(SemanticRole::Disclosure);
        ctx.label(self.header);
        state.apply_default(self.default_collapsed);

        let header_style = if ctx.is_focused() {
            ctx.style(Role::Highlight)
        } else {
            ctx.style(Role::Text)
        };
        // A disclosure marker leads the header: ▸ collapsed, ▾ expanded.
        let marker = if state.collapsed { "▸ " } else { "▾ " };
        ctx.set_string(Position::ORIGIN, marker, header_style);
        ctx.set_string(Position::new(2, 0), self.header, header_style);

        // The body paints below the header only when expanded, one row per line,
        // clipped to the area's height (the transcript view sizes the area to the
        // cell's declared height).
        if !state.collapsed {
            let body_style = ctx.style(Role::Muted);
            let height = ctx.area().size.height;
            for (index, line) in self.body.split('\n').enumerate() {
                let Ok(row) = u16::try_from(index + 1) else {
                    break;
                };
                if row >= height {
                    break;
                }
                ctx.set_string(Position::new(0, row), line, body_style);
            }
        }
    }

    fn desired_height(&self, state: &CollapsibleState, _width: u16) -> u16 {
        // Collapsed: the header row alone. Expanded: the header plus one row per
        // body line (the transcript view sizes the cell to this so a scroll can
        // stack and virtualize on it).
        if self.effective_collapsed(state) {
            1
        } else {
            let body_lines = u16::try_from(self.body.split('\n').count()).unwrap_or(u16::MAX);
            body_lines.saturating_add(1)
        }
    }

    fn handle(
        state: &mut CollapsibleState,
        event: &InputEvent,
        ctx: &mut HandleCtx<'_>,
    ) -> Handled {
        // A left click on the header row toggles the cell (row 0 of the area).
        if let Some(mouse) = event.as_mouse() {
            let on_header = mouse.button == MouseButton::Left
                && mouse.kind == MouseKind::Down
                && mouse.position.y == ctx.area().origin.y;
            if on_header {
                let collapsed = state.toggle();
                ctx.emit(Outcome::Toggled(collapsed));
                return Handled::Yes;
            }
            return Handled::No;
        }
        let Some(key) = event.as_key() else {
            return Handled::No;
        };
        if key.modifiers.ctrl || key.modifiers.alt {
            return Handled::No;
        }
        match key.key {
            Key::Enter => {
                let collapsed = state.toggle();
                ctx.emit(Outcome::Toggled(collapsed));
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::input::{InputEvent, Key, MouseButton, MouseEvent, MouseKind};
    use rabbitui_core::outcome::Outcome;
    use rabbitui_core::widget::{HandleCtx, Handled, Phase, RenderCtx, Widget};

    use super::{Collapsible, CollapsibleState};

    fn dispatch(
        state: &mut CollapsibleState,
        event: InputEvent,
        area: Rect,
    ) -> (Handled, Vec<Outcome>) {
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let handled = {
            let mut ctx = HandleCtx::new(Phase::Bubble, area, &mut outcomes, &mut request_focus);
            Collapsible::handle(state, &event, &mut ctx)
        };
        (handled, outcomes)
    }

    fn render(
        cell: &Collapsible<'_>,
        state: &mut CollapsibleState,
        size: Size,
        focused: bool,
    ) -> Buffer {
        let mut buffer = Buffer::new(size);
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(size), focused);
        cell.render(state, &mut ctx);
        buffer
    }

    fn row(buffer: &Buffer, y: u16) -> String {
        let mut line = String::new();
        for x in 0..buffer.size().width {
            line.push_str(&buffer.get(Position::new(x, y)).unwrap().symbol);
        }
        line.trim_end().to_string()
    }

    #[test]
    fn builder_sets_header_body_and_default() {
        let cell = Collapsible::new("h", "b").default_collapsed(true);
        assert_eq!(cell.header(), "h");
        assert_eq!(cell.body(), "b");
        assert!(cell.get_default_collapsed());
    }

    #[test]
    fn default_collapsed_applies_on_first_render_only() {
        let cell = Collapsible::new("head", "body").default_collapsed(true);
        let mut state = CollapsibleState::default();
        // First render applies the collapsed default.
        render(&cell, &mut state, Size::new(20, 3), false);
        assert!(state.is_collapsed());
        // The user expands it; a later render with the same default keeps it open.
        state.toggle();
        assert!(!state.is_collapsed());
        render(&cell, &mut state, Size::new(20, 3), false);
        assert!(!state.is_collapsed());
    }

    #[test]
    fn expanded_shows_header_and_body() {
        let cell = Collapsible::new("summary", "line one\nline two");
        let mut state = CollapsibleState::default();
        let buffer = render(&cell, &mut state, Size::new(20, 3), false);
        // The disclosure marker is ▾ when expanded, followed by the header.
        assert_eq!(row(&buffer, 0), "▾ summary");
        assert_eq!(row(&buffer, 1), "line one");
        assert_eq!(row(&buffer, 2), "line two");
    }

    #[test]
    fn collapsed_hides_the_body() {
        let cell = Collapsible::new("summary", "hidden body").default_collapsed(true);
        let mut state = CollapsibleState::default();
        let buffer = render(&cell, &mut state, Size::new(20, 3), false);
        assert_eq!(row(&buffer, 0), "▸ summary");
        // The body is not painted while collapsed.
        assert_eq!(row(&buffer, 1), "");
    }

    #[test]
    fn enter_toggles_and_emits_toggled() {
        let mut state = CollapsibleState::default();
        let area = Rect::from_size(Size::new(20, 3));
        // Enter collapses an expanded cell, emitting Toggled(true).
        let (handled, outcomes) = dispatch(&mut state, InputEvent::key(Key::Enter), area);
        assert_eq!(handled, Handled::Yes);
        assert!(state.is_collapsed());
        assert_eq!(outcomes, vec![Outcome::Toggled(true)]);
        // Enter again expands it, emitting Toggled(false).
        let (_h, outcomes) = dispatch(&mut state, InputEvent::key(Key::Enter), area);
        assert!(!state.is_collapsed());
        assert_eq!(outcomes, vec![Outcome::Toggled(false)]);
    }

    #[test]
    fn click_on_header_row_toggles() {
        let mut state = CollapsibleState::default();
        // The cell occupies rows 2..5; a click on the header row (absolute y=2)
        // toggles it.
        let area = Rect::new(Position::new(0, 2), Size::new(20, 3));
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::new(3, 2),
        ));
        let (handled, outcomes) = dispatch(&mut state, click, area);
        assert_eq!(handled, Handled::Yes);
        assert!(state.is_collapsed());
        assert_eq!(outcomes, vec![Outcome::Toggled(true)]);
    }

    #[test]
    fn click_below_the_header_does_not_toggle() {
        let mut state = CollapsibleState::default();
        let area = Rect::new(Position::new(0, 2), Size::new(20, 3));
        // A click on a body row (absolute y=3) is not a header toggle.
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::new(3, 3),
        ));
        let (handled, outcomes) = dispatch(&mut state, click, area);
        assert_eq!(handled, Handled::No);
        assert!(!state.is_collapsed());
        assert!(outcomes.is_empty());
    }

    #[test]
    fn desired_height_is_one_collapsed_and_header_plus_body_expanded() {
        let cell = Collapsible::new("head", "line one\nline two\nline three");
        // Expanded (default): 1 header + 3 body rows.
        let expanded = CollapsibleState::default();
        assert_eq!(cell.desired_height(&expanded, 40), 4);
        // Collapsed: just the header row.
        let mut collapsed = CollapsibleState::default();
        collapsed.set_collapsed(true);
        assert_eq!(cell.desired_height(&collapsed, 40), 1);
    }

    #[test]
    fn desired_height_honors_builder_default_before_first_render() {
        // A tool cell defaults collapsed; measured before any render (state not yet
        // initialized), it must report the collapsed height, not the expanded one.
        let tool = Collapsible::new("h", "a\nb\nc").default_collapsed(true);
        assert_eq!(tool.desired_height(&CollapsibleState::default(), 40), 1);
        // An assistant cell defaults expanded.
        let assistant = Collapsible::new("h", "a\nb\nc");
        assert_eq!(
            assistant.desired_height(&CollapsibleState::default(), 40),
            4
        );
    }

    #[test]
    fn other_keys_are_ignored() {
        let mut state = CollapsibleState::default();
        let area = Rect::from_size(Size::new(20, 3));
        let (handled, outcomes) = dispatch(&mut state, InputEvent::key(Key::Char('x')), area);
        assert_eq!(handled, Handled::No);
        assert!(outcomes.is_empty());
    }
}
