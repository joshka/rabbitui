//! A virtualized, index-selected list over a pluggable [`ListSource`].

use std::borrow::Cow;

use rabbitui_core::geometry::Position;
use rabbitui_core::input::{InputEvent, Key, MouseButton, MouseKind};
use rabbitui_core::outcome::Outcome;
use rabbitui_core::theme::Role;
use rabbitui_core::widget::{HandleCtx, Handled, RenderCtx, Widget};

/// A lazy, index-addressable source of list rows.
///
/// The seam for virtualization (ADR 0008): [`SelectionList`] is generic over
/// this trait and only ever asks for the rows it paints, so a large or streaming
/// backend never materializes every row. Slices and `Vec<String>` implement it
/// eagerly out of the box; a columnar or mmap-backed source implements the same
/// two methods without touching the widget. Durable selection by a stable item
/// key (reorder-proof) is deferred until keyed rows land (design note); v1
/// selects by index.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::ListSource;
///
/// let items: &[&str] = &["alpha", "beta"];
/// assert_eq!(items.len(), 2);
/// assert_eq!(&*items.item(1), "beta");
/// ```
pub trait ListSource {
    /// The number of rows available.
    fn len(&self) -> usize;

    /// The text of row `i`.
    ///
    /// Returns a [`Cow`] so an eager source can borrow (`&str` slices) while a
    /// computed source can own (a formatted row). `i` is always `< len()` when
    /// the widget calls it; out-of-range indices may return an empty row.
    fn item(&self, i: usize) -> Cow<'_, str>;

    /// Whether the source has no rows. Provided; override only if cheaper.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ListSource for &[&str] {
    fn len(&self) -> usize {
        <[&str]>::len(self)
    }

    fn item(&self, i: usize) -> Cow<'_, str> {
        Cow::Borrowed(self.get(i).copied().unwrap_or(""))
    }
}

impl ListSource for Vec<String> {
    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn item(&self, i: usize) -> Cow<'_, str> {
        self.get(i)
            .map_or(Cow::Borrowed(""), |s| Cow::Borrowed(s.as_str()))
    }
}

impl ListSource for &[String] {
    fn len(&self) -> usize {
        <[String]>::len(self)
    }

    fn item(&self, i: usize) -> Cow<'_, str> {
        self.get(i)
            .map_or(Cow::Borrowed(""), |s| Cow::Borrowed(s.as_str()))
    }
}

/// A single-column list the user moves a selection through, one row per item.
///
/// Selection is **by index** (ADR 0008 / design note): [`SelectionListState`]
/// holds the selected index and the scroll offset. Bindings (only while focused):
/// Up/Down move the selection one row, clamped at the ends; Home/End jump to the
/// first/last row; Enter activates the selected row. The list keeps the selection
/// visible by adjusting its offset (scroll-into-view inside the widget) and paints
/// **only the visible rows** (`offset .. offset + height`), so a million-row
/// source costs one screenful — virtualization by construction.
///
/// Outcomes: [`Outcome::Selected`] carrying the new index whenever the selection
/// moves, and [`Outcome::Activated`] on Enter.
///
/// The selected row paints in [`Role::Highlight`] when the list is focused and
/// [`Role::Muted`] when it is not; other rows use [`Role::Text`].
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::SelectionList;
///
/// let items: &[&str] = &["one", "two", "three"];
/// let list = SelectionList::new(items);
/// assert_eq!(list.len(), 3);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct SelectionList<S> {
    source: S,
}

impl<S: ListSource> SelectionList<S> {
    /// Creates a list over `source`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::SelectionList;
    ///
    /// let list = SelectionList::new(vec!["a".to_string(), "b".to_string()]);
    /// assert_eq!(list.len(), 2);
    /// ```
    #[must_use]
    pub const fn new(source: S) -> Self {
        Self { source }
    }

    /// The number of rows in the source.
    #[must_use]
    pub fn len(&self) -> usize {
        self.source.len()
    }

    /// Whether the source has no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.source.is_empty()
    }
}

/// The retained state of a [`SelectionList`]: the selected index and the first
/// visible row.
///
/// Framework-owned, keyed by identity (ADR 0002), so selection and scroll survive
/// across frames while the spec (and its borrowed source) is rebuilt each frame.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::selection_list::SelectionListState;
///
/// let state = SelectionListState::default();
/// assert_eq!(state.selected(), 0);
/// assert_eq!(state.offset(), 0);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SelectionListState {
    selected: usize,
    offset: usize,
}

impl SelectionListState {
    /// The selected row index.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// The first visible row index (the scroll offset).
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Sets the selected row index programmatically.
    ///
    /// The controlled-selection surface a widget command drives (slice 6): the
    /// app moves the selection with
    /// `update.widget::<SelectionList<_>>(path, |s| s.select(i))` — resetting to
    /// the top after a filter, jumping to a search hit. The index is re-clamped
    /// into `0..len` at the next render (as event-time movement already is), and
    /// scroll-into-view follows, so an out-of-range `i` is corrected rather than
    /// dangling.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::selection_list::SelectionListState;
    ///
    /// let mut state = SelectionListState::default();
    /// state.select(3);
    /// assert_eq!(state.selected(), 3);
    /// ```
    pub fn select(&mut self, index: usize) {
        self.selected = index;
    }

    /// Clamps the selection into `0..len` (or 0 when empty).
    ///
    /// Called at render time so a source that shrank between frames never leaves
    /// the selection dangling past the end.
    fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    /// Adjusts `offset` so the selected row is within a window of `height` rows,
    /// given `len` total rows. The scroll-into-view math, applied each render.
    fn scroll_into_view(&mut self, height: usize, len: usize) {
        if height == 0 || len == 0 {
            self.offset = 0;
            return;
        }
        // Never scroll past the point where the last row sits at the bottom.
        let max_offset = len.saturating_sub(height);
        if self.selected < self.offset {
            // Selection is above the window: bring it to the top.
            self.offset = self.selected;
        } else if self.selected >= self.offset + height {
            // Selection is below the window: bring it to the bottom row.
            self.offset = self.selected - height + 1;
        }
        self.offset = self.offset.min(max_offset);
    }
}

impl<S: ListSource> Widget for SelectionList<S> {
    type State = SelectionListState;

    fn render(&self, state: &mut SelectionListState, ctx: &mut RenderCtx<'_>) {
        ctx.focusable(true);
        let len = self.source.len();
        state.clamp(len);

        let height = usize::from(ctx.size().height);
        state.scroll_into_view(height, len);

        let text_style = ctx.style(Role::Text);
        let selected_style = if ctx.is_focused() {
            ctx.style(Role::Highlight)
        } else {
            ctx.style(Role::Muted)
        };

        // Paint only the visible window: offset .. offset + height, clamped to len.
        let end = (state.offset + height).min(len);
        for (row, index) in (state.offset..end).enumerate() {
            let Ok(y) = u16::try_from(row) else { break };
            let style = if index == state.selected {
                selected_style
            } else {
                text_style
            };
            ctx.set_string(Position::new(0, y), &self.source.item(index), style);
        }
    }

    fn handle(
        state: &mut SelectionListState,
        event: &InputEvent,
        ctx: &mut HandleCtx<'_>,
    ) -> Handled {
        // Mouse: a left press on a visible row selects it; the wheel moves the
        // selection one row per notch (the natural list gestures). The row is the
        // click's offset from the widget's area top, plus the scroll offset.
        if let Some(mouse) = event.as_mouse() {
            return handle_mouse(state, ctx, mouse);
        }
        let Some(key) = event.as_key() else {
            return Handled::No;
        };
        if key.modifiers.ctrl || key.modifiers.alt {
            return Handled::No;
        }
        // The source is not available at event time (the spec is gone), so
        // movement clamps against a "no upper bound" here and render re-clamps to
        // the real length. The last-known selection is what the app saw; moving up
        // is always safe, moving down is re-clamped on the next render.
        match key.key {
            Key::Up => move_selection(state, ctx, Movement::Up),
            Key::Down => move_selection(state, ctx, Movement::Down),
            Key::Home => move_selection(state, ctx, Movement::Home),
            Key::End => move_selection(state, ctx, Movement::End),
            Key::Enter => {
                ctx.emit(Outcome::Activated);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

/// Handles a mouse event for the list: a left press selects the clicked row; the
/// wheel moves the selection one row per notch.
///
/// The clicked row is the pointer's row minus the widget's area top, plus the
/// scroll offset — the visible window is `offset .. offset + height`, so a click
/// on the `k`-th visible row selects item `offset + k`. Movement past the source
/// length is re-clamped at the next render, as with key movement. Every mouse
/// event over the list is consumed; a bare release (`Up`) or a non-left button is
/// ignored (not handled), so it can fall through to the app.
fn handle_mouse(
    state: &mut SelectionListState,
    ctx: &mut HandleCtx<'_>,
    mouse: &rabbitui_core::input::MouseEvent,
) -> Handled {
    match mouse.kind {
        MouseKind::Down if mouse.button == MouseButton::Left => {
            let top = ctx.area().origin.y;
            let row = mouse.position.y.saturating_sub(top);
            let index = state.offset.saturating_add(usize::from(row));
            let before = state.selected;
            state.select(index);
            if state.selected != before {
                ctx.emit(Outcome::Selected(index));
            }
            Handled::Yes
        }
        MouseKind::Scroll(lines) => {
            let before = state.selected;
            if lines > 0 {
                // Scroll down: advance the selection, re-clamped at render.
                state.selected = state
                    .selected
                    .saturating_add(usize::from(lines.unsigned_abs()));
            } else if lines < 0 {
                state.selected = state
                    .selected
                    .saturating_sub(usize::from(lines.unsigned_abs()));
            }
            if state.selected != before {
                ctx.emit(Outcome::Selected(state.selected));
            }
            Handled::Yes
        }
        _ => Handled::No,
    }
}

/// A selection movement requested by a key.
#[derive(Debug, Clone, Copy)]
enum Movement {
    Up,
    Down,
    Home,
    End,
}

/// Applies `movement` to the selection, emitting [`Outcome::Selected`] if the
/// index changed. Always consumes the key (a clamped no-op is still handled).
///
/// End without the source length cannot compute the last index at event time, so
/// it is expressed as a large jump that the next render clamps to `len - 1`; the
/// [`Outcome::Selected`] it emits carries that provisional index, and the app
/// reads the authoritative selection from the widget state after re-render if it
/// needs the exact row. In the todo example this is immaterial because the app
/// tracks selection through the state, not the outcome payload.
fn move_selection(
    state: &mut SelectionListState,
    ctx: &mut HandleCtx<'_>,
    movement: Movement,
) -> Handled {
    let before = state.selected;
    match movement {
        Movement::Up => state.selected = state.selected.saturating_sub(1),
        Movement::Down => state.selected = state.selected.saturating_add(1),
        Movement::Home => state.selected = 0,
        // A sentinel the render clamps down to the true last row.
        Movement::End => state.selected = usize::MAX,
    }
    if state.selected != before {
        ctx.emit(Outcome::Selected(state.selected));
    }
    Handled::Yes
}

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::input::{InputEvent, Key};
    use rabbitui_core::outcome::Outcome;
    use rabbitui_core::theme::{Role, Theme};
    use rabbitui_core::widget::{HandleCtx, Handled, Phase, RenderCtx, Widget};

    use super::{ListSource, SelectionList, SelectionListState};

    fn items() -> Vec<String> {
        (0..10).map(|i| format!("item{i}")).collect()
    }

    fn dispatch(state: &mut SelectionListState, key: Key) -> (Handled, Vec<Outcome>) {
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let handled = {
            let mut ctx = HandleCtx::new(
                Phase::Bubble,
                Rect::default(),
                &mut outcomes,
                &mut request_focus,
            );
            <SelectionList<Vec<String>>>::handle(state, &InputEvent::key(key), &mut ctx)
        };
        (handled, outcomes)
    }

    /// Renders a list of `data` into a buffer of `height` rows, re-clamping the
    /// state, and returns the buffer.
    fn render(
        list: &SelectionList<Vec<String>>,
        state: &mut SelectionListState,
        height: u16,
        focused: bool,
    ) -> Buffer {
        let mut buffer = Buffer::new(Size::new(8, height));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(8, height)), focused);
        list.render(state, &mut ctx);
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
    fn slice_and_vec_sources_agree() {
        let slice: &[&str] = &["a", "b", "c"];
        assert_eq!(slice.len(), 3);
        assert_eq!(&*slice.item(2), "c");
        let owned = vec!["x".to_string(), "y".to_string()];
        assert_eq!(ListSource::len(&owned), 2);
        assert_eq!(&*owned.item(0), "x");
    }

    #[test]
    fn down_and_up_move_and_emit_selected() {
        let mut state = SelectionListState::default();
        let (handled, outcomes) = dispatch(&mut state, Key::Down);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(state.selected(), 1);
        assert_eq!(outcomes, vec![Outcome::Selected(1)]);
        let (_h, outcomes) = dispatch(&mut state, Key::Up);
        assert_eq!(state.selected(), 0);
        assert_eq!(outcomes, vec![Outcome::Selected(0)]);
    }

    #[test]
    fn up_at_top_is_clamped_and_emits_nothing() {
        let mut state = SelectionListState::default();
        let (handled, outcomes) = dispatch(&mut state, Key::Up);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(state.selected(), 0);
        assert!(outcomes.is_empty());
    }

    #[test]
    fn down_past_end_is_reclamped_on_render() {
        let list = SelectionList::new(items()); // 10 rows
        let mut state = SelectionListState::default();
        // Push selection well past the end at event time.
        for _ in 0..20 {
            dispatch(&mut state, Key::Down);
        }
        assert_eq!(state.selected(), 20);
        // Render clamps to the real last index.
        render(&list, &mut state, 5, true);
        assert_eq!(state.selected(), 9);
    }

    #[test]
    fn end_selects_last_after_render_clamp() {
        let list = SelectionList::new(items());
        let mut state = SelectionListState::default();
        dispatch(&mut state, Key::End);
        render(&list, &mut state, 5, true);
        assert_eq!(state.selected(), 9);
    }

    #[test]
    fn home_selects_first() {
        let mut state = SelectionListState {
            selected: 5,
            offset: 3,
        };
        let (_h, outcomes) = dispatch(&mut state, Key::Home);
        assert_eq!(state.selected(), 0);
        assert_eq!(outcomes, vec![Outcome::Selected(0)]);
    }

    #[test]
    fn enter_activates() {
        let mut state = SelectionListState::default();
        let (handled, outcomes) = dispatch(&mut state, Key::Enter);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(outcomes, vec![Outcome::Activated]);
    }

    fn dispatch_mouse(
        state: &mut SelectionListState,
        event: InputEvent,
        area: Rect,
    ) -> (Handled, Vec<Outcome>) {
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let handled = {
            let mut ctx = HandleCtx::new(Phase::Bubble, area, &mut outcomes, &mut request_focus);
            <SelectionList<Vec<String>>>::handle(state, &event, &mut ctx)
        };
        (handled, outcomes)
    }

    #[test]
    fn click_selects_the_clicked_row_and_emits_selected() {
        use rabbitui_core::input::{MouseButton, MouseEvent, MouseKind};
        // The list occupies rows 2..6; offset 0. A click on absolute row 4 is the
        // third visible row → index 2.
        let area = Rect::new(Position::new(0, 2), Size::new(8, 4));
        let mut state = SelectionListState::default();
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::new(0, 4),
        ));
        let (handled, outcomes) = dispatch_mouse(&mut state, click, area);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(state.selected(), 2);
        assert_eq!(outcomes, vec![Outcome::Selected(2)]);
    }

    #[test]
    fn click_respects_scroll_offset() {
        use rabbitui_core::input::{MouseButton, MouseEvent, MouseKind};
        let area = Rect::new(Position::new(0, 0), Size::new(8, 3));
        // Offset 5: the top visible row is item 5. Clicking the second visible row
        // selects item 6.
        let mut state = SelectionListState {
            selected: 5,
            offset: 5,
        };
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::new(0, 1),
        ));
        let (_h, outcomes) = dispatch_mouse(&mut state, click, area);
        assert_eq!(state.selected(), 6);
        assert_eq!(outcomes, vec![Outcome::Selected(6)]);
    }

    #[test]
    fn wheel_moves_selection() {
        use rabbitui_core::input::{MouseButton, MouseEvent, MouseKind};
        let area = Rect::new(Position::ORIGIN, Size::new(8, 5));
        let mut state = SelectionListState {
            selected: 3,
            offset: 0,
        };
        // Wheel down advances the selection.
        let down = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Scroll(1),
            MouseButton::None,
            Position::ORIGIN,
        ));
        let (handled, outcomes) = dispatch_mouse(&mut state, down, area);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(state.selected(), 4);
        assert_eq!(outcomes, vec![Outcome::Selected(4)]);
        // Wheel up retreats it.
        let up = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Scroll(-1),
            MouseButton::None,
            Position::ORIGIN,
        ));
        let (_h, outcomes) = dispatch_mouse(&mut state, up, area);
        assert_eq!(state.selected(), 3);
        assert_eq!(outcomes, vec![Outcome::Selected(3)]);
    }

    #[test]
    fn scroll_into_view_math() {
        // 10 rows, window of 3.
        // Selecting row 5 while offset 0: window must move so 5 is the bottom row.
        let mut state = SelectionListState {
            selected: 5,
            ..Default::default()
        };
        state.scroll_into_view(3, 10);
        assert_eq!(state.offset(), 3); // rows 3,4,5 visible
        // Selecting row 2 (above the window): window moves up to top at 2.
        state.selected = 2;
        state.scroll_into_view(3, 10);
        assert_eq!(state.offset(), 2);
        // Selecting the last row: offset clamps to len-height = 7.
        state.selected = 9;
        state.scroll_into_view(3, 10);
        assert_eq!(state.offset(), 7);
    }

    #[test]
    fn renders_only_visible_rows_and_highlights_selection() {
        let list = SelectionList::new(items());
        let mut state = SelectionListState {
            selected: 5,
            offset: 0,
        };
        // Height 3: scroll-into-view brings rows 3,4,5 into view.
        let buffer = render(&list, &mut state, 3, true);
        assert_eq!(state.offset(), 3);
        assert_eq!(row(&buffer, 0), "item3");
        assert_eq!(row(&buffer, 1), "item4");
        assert_eq!(row(&buffer, 2), "item5");
        // The selected row (item5, last visible) is highlighted when focused.
        let selected_style = buffer.get(Position::new(0, 2)).unwrap().style;
        assert_eq!(selected_style, Theme::default().style(Role::Highlight));
        // A non-selected visible row uses Text.
        let other = buffer.get(Position::new(0, 0)).unwrap().style;
        assert_eq!(other, Theme::default().style(Role::Text));
    }

    #[test]
    fn selected_row_is_muted_when_unfocused() {
        let list = SelectionList::new(items());
        let mut state = SelectionListState {
            selected: 1,
            offset: 0,
        };
        let buffer = render(&list, &mut state, 5, false);
        let selected_style = buffer.get(Position::new(0, 1)).unwrap().style;
        assert_eq!(selected_style, Theme::default().style(Role::Muted));
    }

    #[test]
    fn select_sets_index_and_render_clamps_and_scrolls() {
        let list = SelectionList::new(items()); // 10 rows
        let mut state = SelectionListState::default();
        // Select a mid-list row programmatically (a widget command would do this).
        state.select(7);
        assert_eq!(state.selected(), 7);
        let buffer = render(&list, &mut state, 3, true);
        // Scroll-into-view brought row 7 to the bottom of a 3-row window.
        assert_eq!(state.offset(), 5);
        assert_eq!(row(&buffer, 2), "item7");
        // An out-of-range selection is re-clamped at render, like event movement.
        state.select(99);
        render(&list, &mut state, 3, true);
        assert_eq!(state.selected(), 9);
    }

    #[test]
    fn empty_source_renders_nothing_and_clamps_to_zero() {
        let list = SelectionList::new(Vec::<String>::new());
        let mut state = SelectionListState {
            selected: 3,
            offset: 2,
        };
        let buffer = render(&list, &mut state, 4, true);
        assert_eq!(state.selected(), 0);
        assert_eq!(state.offset(), 0);
        assert_eq!(row(&buffer, 0), "");
    }
}
