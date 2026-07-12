//! A virtualized, index-selected table over a pluggable [`TableSource`].

use std::borrow::Cow;

use rabbitui_core::accessibility::SemanticRole;
use rabbitui_core::geometry::Position;
use rabbitui_core::input::{InputEvent, Key, MouseButton, MouseKind};
use rabbitui_core::layout::Constraint;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::theme::Role;
use rabbitui_core::widget::{HandleContext, Handled, RenderContext, Widget};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A lazy, cell-addressable source of table rows.
///
/// The seam for virtualization (ADR 0008): [`Table`] is generic over this trait
/// and only ever asks for the cells it paints, so a large or streaming backend
/// never materializes every row. `Vec<Vec<String>>` and `&[Vec<String>]`
/// implement it eagerly out of the box; a columnar or database-backed source
/// implements the same two methods without touching the widget. This mirrors
/// [`ListSource`](crate::ListSource): a user who knows one knows the other.
/// Selection is by index (v1); durable keyed selection is deferred with the list.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::TableSource;
///
/// let rows = vec![vec!["Ada".to_string(), "36".to_string()]];
/// assert_eq!(TableSource::len(&rows), 1);
/// assert_eq!(&*rows.cell(0, 1), "36");
/// // Out-of-range coordinates yield an empty cell rather than panicking.
/// assert_eq!(&*rows.cell(9, 9), "");
/// ```
pub trait TableSource {
    /// The number of rows available.
    fn len(&self) -> usize;

    /// The text of the cell at `row`, `col`.
    ///
    /// Returns a [`Cow`] so an eager source can borrow (`&str` slices) while a
    /// computed source can own (a formatted cell). The widget calls it **only
    /// for the cells it paints** — the visible window intersected with the
    /// declared columns — so a million-row source still costs one screenful.
    /// Out-of-range `row`/`col` should return an empty cell.
    fn cell(&self, row: usize, col: usize) -> Cow<'_, str>;

    /// Whether the source has no rows. Provided; override only if cheaper.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl TableSource for Vec<Vec<String>> {
    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn cell(&self, row: usize, col: usize) -> Cow<'_, str> {
        self.get(row)
            .and_then(|r| r.get(col))
            .map_or(Cow::Borrowed(""), |s| Cow::Borrowed(s.as_str()))
    }
}

impl TableSource for &[Vec<String>] {
    fn len(&self) -> usize {
        <[Vec<String>]>::len(self)
    }

    fn cell(&self, row: usize, col: usize) -> Cow<'_, str> {
        self.get(row)
            .and_then(|r| r.get(col))
            .map_or(Cow::Borrowed(""), |s| Cow::Borrowed(s.as_str()))
    }
}

/// A [`TableSource`] computed on demand from a row count and a per-cell formatter.
///
/// The escape hatch for backing a table with *borrowed custom data* without
/// materializing a `Vec<Vec<String>>` every frame. The formatter is called only
/// for the cells the widget paints, so a filtered view of a million-row source
/// still costs one screenful — the virtualization the widget promises, preserved
/// through the app's own cell formatting.
///
/// Build one with [`table_from_fn`] (a raw row count + `(row, col)` closure) or
/// the [`table_rows_with`] sugar (a borrowed slice + a `(&T, col) -> String`
/// closure). These are named `table_*` because the crate root already re-exports
/// the list's `from_fn`/`rows_with`.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::{TableSource, table_from_fn};
///
/// let src = table_from_fn(3, |row, col| format!("r{row}c{col}"));
/// assert_eq!(src.len(), 3);
/// assert_eq!(&*src.cell(1, 2), "r1c2");
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TableFromFn<F> {
    rows: usize,
    format: F,
}

impl<F> TableSource for TableFromFn<F>
where
    F: Fn(usize, usize) -> String,
{
    fn len(&self) -> usize {
        self.rows
    }

    fn cell(&self, row: usize, col: usize) -> Cow<'_, str> {
        if row < self.rows {
            Cow::Owned((self.format)(row, col))
        } else {
            Cow::Borrowed("")
        }
    }
}

/// Builds a [`TableSource`] of `rows` rows, formatting cell `(row, col)` on demand.
///
/// The general lazy source: the widget calls `format` only for the cells it
/// paints. Use this when the cell text is derived from data the app already
/// owns, to avoid allocating a `Vec<Vec<String>>` each frame. For the common
/// "borrowed slice + formatter" shape, prefer [`table_rows_with`].
///
/// # Examples
///
/// ```
/// use rabbitui_core::layout::Constraint;
/// use rabbitui_widgets::{Column, Table, table_from_fn};
///
/// let columns = vec![Column::new("#", Constraint::Length(8))];
/// let table = Table::new(table_from_fn(1_000_000, |row, _col| format!("line {row}")), columns);
/// assert_eq!(table.len(), 1_000_000);
/// ```
#[must_use]
pub const fn table_from_fn<F>(rows: usize, format: F) -> TableFromFn<F>
where
    F: Fn(usize, usize) -> String,
{
    TableFromFn { rows, format }
}

/// Builds a [`TableSource`] over a borrowed slice of `T`, formatting each visible
/// cell with `format`.
///
/// The ergonomic form of [`table_from_fn`] for the dominant case: an app holds a
/// slice of some record type and wants one text cell per `(record, column)`
/// without cloning them into a `Vec<Vec<String>>`. The slice is borrowed for as
/// long as the returned source lives (a single frame), and `format` runs only
/// for the painted cells. Mirrors [`rows_with`](crate::selection_list::rows_with).
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::{TableSource, table_rows_with};
///
/// struct Person { name: &'static str, age: u32 }
/// let people = [Person { name: "Ada", age: 36 }, Person { name: "Alan", age: 41 }];
/// let src = table_rows_with(&people, |p, col| match col {
///     0 => p.name.to_string(),
///     _ => p.age.to_string(),
/// });
/// assert_eq!(src.len(), 2);
/// assert_eq!(&*src.cell(1, 0), "Alan");
/// assert_eq!(&*src.cell(0, 1), "36");
/// ```
pub fn table_rows_with<T, F>(
    rows: &[T],
    format: F,
) -> TableFromFn<impl Fn(usize, usize) -> String + '_>
where
    F: Fn(&T, usize) -> String + 'static,
{
    table_from_fn(rows.len(), move |row, col| format(&rows[row], col))
}

/// A single table column: a header label and a width [`Constraint`].
///
/// Column widths are resolved every frame from the widget's area width using the
/// same cumulative exact-share arithmetic as
/// [`split_columns`](rabbitui_core::layout::split_columns): [`Constraint::Length`]
/// columns take their fixed width first (clipped in order when space runs out),
/// then [`Constraint::Fill`] columns divide the remainder by weight with no gap.
///
/// # Examples
///
/// ```
/// use rabbitui_core::layout::Constraint;
/// use rabbitui_widgets::Column;
///
/// let name = Column::new("Name", Constraint::Fill(1));
/// let age = Column::new("Age", Constraint::Length(4));
/// let _ = (name, age);
/// ```
#[derive(Debug, Clone)]
pub struct Column {
    header: Cow<'static, str>,
    constraint: Constraint,
}

impl Column {
    /// Creates a column with `header` and a width `constraint`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::layout::Constraint;
    /// use rabbitui_widgets::Column;
    ///
    /// let col = Column::new("Name", Constraint::Fill(1));
    /// let _ = col;
    /// ```
    pub fn new(header: impl Into<Cow<'static, str>>, constraint: Constraint) -> Self {
        Self {
            header: header.into(),
            constraint,
        }
    }
}

/// A virtualized, index-selected table of cells over a pluggable [`TableSource`].
///
/// Rows are uniform height (one terminal row each; variable-height rows are out
/// of scope for v1). A pinned header occupies row 0 of the area and never
/// scrolls; the body virtualizes a window over the source and paints **only the
/// visible rows** (`offset .. offset + body_height`), calling
/// [`TableSource::cell`] only for the cells it paints — so a million-row source
/// costs one screenful. The API mirrors [`SelectionList`](crate::SelectionList):
/// same naming, same state shape, same outcome vocabulary.
///
/// Bindings (only while focused): Up/Down move the selection one row;
/// PageUp/PageDown move it by the recorded body height; Home/End jump to the
/// first/last row; Enter activates the selected row. The table keeps the
/// selection visible by adjusting its offset (scroll-into-view inside the
/// widget).
///
/// Outcomes: [`Outcome::Selected`] carrying the new index whenever the selection
/// moves, and [`Outcome::Activated`] on Enter.
///
/// The selected row paints in [`Role::Highlight`] when the table is focused and
/// [`Role::Accent`] when it is not; other rows use [`Role::Text`].
/// (Accent, not Muted: the selection must never be the dimmest row.)
///
/// # Column gutter
///
/// Each cell's text truncates to `width - 1` of its column, leaving one blank
/// gutter column so neighboring cells never abut — except the **last** column,
/// which uses its full width. Header labels use the full column width.
///
/// # Examples
///
/// ```
/// use rabbitui_core::layout::Constraint;
/// use rabbitui_widgets::{Column, Table};
///
/// let columns = vec![
///     Column::new("Name", Constraint::Fill(1)),
///     Column::new("Age", Constraint::Length(4)),
/// ];
/// let rows = vec![vec!["Ada".to_string(), "36".to_string()]];
/// let table = Table::new(rows, columns);
/// assert_eq!(table.len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct Table<S> {
    source: S,
    columns: Vec<Column>,
    /// Placeholder shown *in place of the body rows* when the source is empty.
    /// When set, an empty table renders this text in [`Role::Muted`] on the first
    /// body row and stays declared and focusable under one key (dogfood finding
    /// #6); the header still paints. `None` keeps the header-only behavior.
    empty_text: Option<Cow<'static, str>>,
}

impl<S: TableSource> Table<S> {
    /// Creates a table over `source` with `columns`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::layout::Constraint;
    /// use rabbitui_widgets::{Column, Table};
    ///
    /// let columns = vec![Column::new("Name", Constraint::Fill(1))];
    /// let table = Table::new(vec![vec!["Ada".to_string()]], columns);
    /// assert_eq!(table.len(), 1);
    /// ```
    #[must_use]
    pub const fn new(source: S, columns: Vec<Column>) -> Self {
        Self {
            source,
            columns,
            empty_text: None,
        }
    }

    /// Sets the placeholder shown when the table has zero rows.
    ///
    /// With an empty text set, an empty [`Table`] renders the given message (in
    /// [`Role::Muted`]) on the first body row instead of showing only the header,
    /// and stays declared and focusable under its own key. An app can therefore
    /// keep the table under a single stable identity across the empty↔populated
    /// boundary rather than swapping to a separate placeholder widget — which
    /// drops the table's focus and can trigger the declare-then-command panic
    /// (dogfood finding #6).
    ///
    /// Focus and selection on an empty table: the table remains focusable, the
    /// selection clamps to `0`, and no [`Outcome::Selected`] is emitted while
    /// empty. Movement keys are still consumed but move nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::layout::Constraint;
    /// use rabbitui_widgets::{Column, Table};
    ///
    /// let columns = vec![Column::new("Name", Constraint::Fill(1))];
    /// let table = Table::new(Vec::<Vec<String>>::new(), columns).empty_text("no rows");
    /// assert!(table.is_empty());
    /// ```
    #[must_use]
    pub fn empty_text(mut self, text: impl Into<Cow<'static, str>>) -> Self {
        self.empty_text = Some(text.into());
        self
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

/// The retained state of a [`Table`]: the selected row, the first visible row,
/// and the body height recorded at the last render.
///
/// Framework-owned, keyed by identity (ADR 0002), so selection and scroll survive
/// across frames while the spec (and its borrowed source) is rebuilt each frame.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::TableState;
///
/// let state = TableState::default();
/// assert_eq!(state.selected(), 0);
/// assert_eq!(state.offset(), 0);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TableState {
    selected: usize,
    offset: usize,
    /// The body height (area height minus the one header row) recorded at the
    /// last render, so a PageUp/PageDown at event time — when the area is not
    /// available — knows a page size. Recorded geometry, like `ScrollState`'s
    /// viewport fields; `0` (never rendered) is treated as `1`.
    page: u16,
}

impl TableState {
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
    /// The controlled-selection surface a widget command drives: the app moves
    /// the selection with
    /// `update.widget::<Table<_>>(path, |s| s.select(i))` — resetting to the top
    /// after a filter, jumping to a search hit. The index is re-clamped into
    /// `0..len` at the next render (as event-time movement already is), and
    /// scroll-into-view follows, so an out-of-range `i` is corrected rather than
    /// dangling.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::TableState;
    ///
    /// let mut state = TableState::default();
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

    /// Adjusts `offset` so the selected row is within a window of `height` body
    /// rows, given `len` total rows. The same scroll-into-view math as
    /// [`SelectionListState`](crate::selection_list::SelectionListState); `height`
    /// is the **body** height (area height minus the pinned header row).
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

impl<S: TableSource> Widget for Table<S> {
    type State = TableState;

    fn render(&self, state: &mut TableState, ctx: &mut RenderContext<'_>) {
        ctx.focusable(true);
        // A11y groundwork (ADR arc4 §5): there is no SemanticRole::Table variant
        // yet, so a table declares as a List (a selectable set of rows). Adding a
        // Table variant to `rabbitui-core` is out of this lane.
        ctx.semantic_role(SemanticRole::List);

        let len = self.source.len();
        state.clamp(len);

        let size = ctx.size();
        // The body is everything below the one pinned header row.
        let body_height = size.height.saturating_sub(1);
        state.page = body_height;
        let body_height_usize = usize::from(body_height);
        state.scroll_into_view(body_height_usize, len);

        // Column widths recomputed each frame from the area width.
        let constraints: Vec<Constraint> = self.columns.iter().map(|c| c.constraint).collect();
        let widths = column_widths(size.width, &constraints);

        // Header: row 0, always painted (even when empty), never scrolls. Uses the
        // full column width (no gutter).
        let header_style = ctx.style(Role::Muted).bold();
        let mut x = 0u16;
        for (column, width) in self.columns.iter().zip(&widths) {
            if *width > 0 {
                let text = truncate_to_width(&column.header, usize::from(*width));
                ctx.set_string(Position::new(x, 0), text, header_style);
            }
            x = x.saturating_add(*width);
        }

        // Empty state: with a placeholder set, paint it on the first body row and
        // return. The header still painted above; the widget stays declared and
        // focusable (set above), so an app keeps the table under one key across
        // the empty↔populated boundary (dogfood finding #6). Selection is already
        // clamped to 0 by `clamp(len)` above.
        if len == 0 {
            if let Some(text) = &self.empty_text
                && body_height > 0
            {
                let style = ctx.style(Role::Muted);
                ctx.set_string(Position::new(0, 1), text, style);
            }
            return;
        }

        let text_style = ctx.style(Role::Text);
        let selected_style = if ctx.is_focused() {
            ctx.style(Role::Highlight)
        } else {
            ctx.style(Role::Accent)
        };

        // Paint only the visible window: offset .. offset + body_height, clamped to
        // len. Cells are fetched only for painted (row, col) pairs — the
        // virtualization property.
        let last_col = self.columns.len().saturating_sub(1);
        let end = (state.offset + body_height_usize).min(len);
        for (screen_row, row_index) in (state.offset..end).enumerate() {
            // +1 for the header row above the body.
            let Ok(y) = u16::try_from(screen_row + 1) else {
                break;
            };
            let style = if row_index == state.selected {
                selected_style
            } else {
                text_style
            };
            let mut x = 0u16;
            for (col_index, width) in widths.iter().enumerate() {
                // Gutter: reserve one blank column, except the last column, which
                // uses its full width.
                let max = if col_index == last_col {
                    usize::from(*width)
                } else {
                    usize::from(width.saturating_sub(1))
                };
                if max > 0 {
                    let cell = self.source.cell(row_index, col_index);
                    let text = truncate_to_width(&cell, max);
                    ctx.set_string(Position::new(x, y), text, style);
                }
                x = x.saturating_add(*width);
            }
        }
    }

    fn desired_height(&self, _state: &TableState, _width: u16) -> u16 {
        // Header row plus one row per source row: a table's honest intrinsic
        // height. A container clamps this to its viewport, and the table
        // virtualizes internally (it only ever paints the visible window), so a
        // million-row source still reports 1 + a million but costs one screenful.
        // An empty table asks for the header row plus, when a placeholder is set,
        // one more row for the placeholder text (dogfood finding #6).
        let len = self.source.len();
        if len == 0 {
            return if self.empty_text.is_some() { 2 } else { 1 };
        }
        u16::try_from(len.saturating_add(1)).unwrap_or(u16::MAX)
    }

    fn handle(state: &mut TableState, event: &InputEvent, ctx: &mut HandleContext<'_>) -> Handled {
        // Mouse: a left press on a body row selects it; the wheel moves the
        // selection one row per notch. A press on the pinned header row is
        // consumed but selects nothing.
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
        // the real length. Moving up is always safe; moving down is re-clamped on
        // the next render.
        match key.key {
            Key::Up => move_selection(state, ctx, Movement::Up),
            Key::Down => move_selection(state, ctx, Movement::Down),
            Key::PageUp => move_selection(state, ctx, Movement::PageUp),
            Key::PageDown => move_selection(state, ctx, Movement::PageDown),
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

/// Resolves per-column widths along a row of `total` cells.
///
/// The slice-based equivalent of `rabbitui_core::layout`'s `split_lengths` (the
/// source of this algorithm): [`Constraint::Length`] columns are satisfied first,
/// in order, clipped as space runs out; the remainder divides among
/// [`Constraint::Fill`] columns by cumulative exact shares so the columns tile
/// `total` with no gap and no overflow. `split_lengths` is const-generic over
/// arrays; a table needs runtime-length columns, so the arithmetic is replicated
/// here on slices (a coordinator follow-up may lift a slice variant into
/// `layout.rs`).
fn column_widths(total: u16, constraints: &[Constraint]) -> Vec<u16> {
    let mut lengths = vec![0u16; constraints.len()];
    let mut remaining = total;

    // Pass 1: fixed lengths, clipped in order as space runs out.
    for (length, constraint) in lengths.iter_mut().zip(constraints) {
        if let Constraint::Length(want) = constraint {
            *length = (*want).min(remaining);
            remaining -= *length;
        }
    }

    // Pass 2: divide the remainder among fills by cumulative exact shares.
    let total_weight: u32 = constraints
        .iter()
        .map(|c| {
            if let Constraint::Fill(w) = c {
                u32::from(*w)
            } else {
                0
            }
        })
        .sum();
    if total_weight == 0 {
        return lengths;
    }
    let mut cum_weight: u32 = 0;
    let mut previous_boundary: u16 = 0;
    for (length, constraint) in lengths.iter_mut().zip(constraints) {
        if let Constraint::Fill(weight) = constraint {
            cum_weight += u32::from(*weight);
            let boundary = ((u32::from(remaining) * cum_weight + total_weight / 2) / total_weight)
                .min(u32::from(remaining)) as u16;
            *length = boundary - previous_boundary;
            previous_boundary = boundary;
        }
    }
    lengths
}

/// Returns the longest prefix of `text` whose display width does not exceed
/// `max`, split on grapheme boundaries so a wide grapheme never straddles the
/// limit.
///
/// The same shape as `panel.rs`'s private twin (a coordinator may unify these
/// into one width oracle later): walk graphemes, advance by the display width
/// clamped to `1..=2`, and cut before the grapheme that would exceed `max` — so a
/// wide grapheme straddling the limit is dropped whole.
fn truncate_to_width(text: &str, max: usize) -> &str {
    let mut width = 0usize;
    let mut end = 0usize;
    for grapheme in text.graphemes(true) {
        let advance = UnicodeWidthStr::width(grapheme).clamp(1, 2);
        if width + advance > max {
            break;
        }
        width += advance;
        end += grapheme.len();
    }
    &text[..end]
}

/// Handles a mouse event for the table: a left press selects the clicked body
/// row; the wheel moves the selection one row per notch.
///
/// The clicked row is the pointer's row minus the widget's area top minus the one
/// header row, plus the scroll offset — the visible body is `offset .. offset +
/// body_height`, so a click on the `k`-th visible body row selects item
/// `offset + k`. A press on the header row (offset 0 from the area top) is
/// consumed but selects nothing. Movement past the source length is re-clamped at
/// the next render, as with key movement. Every mouse press/scroll over the table
/// is consumed; a bare release (`Up`) or a non-left button falls through.
fn handle_mouse(
    state: &mut TableState,
    ctx: &mut HandleContext<'_>,
    mouse: &rabbitui_core::input::MouseEvent,
) -> Handled {
    match mouse.kind {
        MouseKind::Down if mouse.button == MouseButton::Left => {
            let top = ctx.area().origin.y;
            let relative = mouse.position.y.saturating_sub(top);
            // Row 0 is the pinned header: consume the click but select nothing.
            if relative == 0 {
                return Handled::Yes;
            }
            let row = relative - 1;
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
    PageUp,
    PageDown,
    Home,
    End,
}

/// Applies `movement` to the selection, emitting [`Outcome::Selected`] if the
/// index changed. Always consumes the key (a clamped no-op is still handled).
///
/// PageUp/PageDown move by the body height recorded at the last render
/// (`state.page`, defaulting to `1` before the first render). End without the
/// source length cannot compute the last index at event time, so it is expressed
/// as a large jump that the next render clamps to `len - 1`; the emitted
/// [`Outcome::Selected`] carries that provisional index, and the app reads the
/// authoritative selection from the widget state after re-render if it needs the
/// exact row.
fn move_selection(
    state: &mut TableState,
    ctx: &mut HandleContext<'_>,
    movement: Movement,
) -> Handled {
    let before = state.selected;
    let page = usize::from(state.page.max(1));
    match movement {
        Movement::Up => state.selected = state.selected.saturating_sub(1),
        Movement::Down => state.selected = state.selected.saturating_add(1),
        Movement::PageUp => state.selected = state.selected.saturating_sub(page),
        Movement::PageDown => state.selected = state.selected.saturating_add(page),
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
    use std::cell::Cell;
    use std::rc::Rc;

    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::input::{InputEvent, Key, MouseButton, MouseEvent, MouseKind};
    use rabbitui_core::layout::Constraint;
    use rabbitui_core::outcome::Outcome;
    use rabbitui_core::theme::{Role, Theme};
    use rabbitui_core::widget::{HandleContext, Handled, Phase, RenderContext, Widget};

    use super::{Column, Table, TableSource, TableState, table_from_fn, table_rows_with};

    fn columns() -> Vec<Column> {
        vec![
            Column::new("Name", Constraint::Fill(1)),
            Column::new("Age", Constraint::Length(4)),
        ]
    }

    fn data() -> Vec<Vec<String>> {
        (0..10)
            .map(|i| vec![format!("name{i}"), format!("{}", 20 + i)])
            .collect()
    }

    fn table() -> Table<Vec<Vec<String>>> {
        Table::new(data(), columns())
    }

    fn render<S: TableSource>(
        table: &Table<S>,
        state: &mut TableState,
        size: Size,
        focused: bool,
    ) -> Buffer {
        let mut buffer = Buffer::new(size);
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(size), focused);
        table.render(state, &mut ctx);
        buffer
    }

    fn row(buffer: &Buffer, y: u16) -> String {
        let mut line = String::new();
        for x in 0..buffer.size().width {
            line.push_str(&buffer.get(Position::new(x, y)).unwrap().symbol);
        }
        line.trim_end().to_string()
    }

    fn dispatch(state: &mut TableState, key: Key) -> (Handled, Vec<Outcome>) {
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let handled = {
            let mut ctx = HandleContext::new(
                Phase::Bubble,
                Rect::default(),
                &mut outcomes,
                &mut request_focus,
            );
            <Table<Vec<Vec<String>>>>::handle(state, &InputEvent::key(key), &mut ctx)
        };
        (handled, outcomes)
    }

    fn dispatch_mouse(
        state: &mut TableState,
        event: InputEvent,
        area: Rect,
    ) -> (Handled, Vec<Outcome>) {
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let handled = {
            let mut ctx =
                HandleContext::new(Phase::Bubble, area, &mut outcomes, &mut request_focus);
            <Table<Vec<Vec<String>>>>::handle(state, &event, &mut ctx)
        };
        (handled, outcomes)
    }

    // 1. Header at row 0 in Muted+bold; first data rows below it.
    #[test]
    fn renders_header_then_body_rows() {
        let table = table();
        let mut state = TableState::default();
        // 20 wide: Fill takes 16, Length(4) takes 4.
        let buffer = render(&table, &mut state, Size::new(20, 6), true);
        assert_eq!(row(&buffer, 0), "Name            Age");
        assert_eq!(row(&buffer, 1), "name0           20");
        assert_eq!(row(&buffer, 2), "name1           21");
        // The header is Muted + bold.
        let header_style = buffer.get(Position::new(0, 0)).unwrap().style;
        assert_eq!(header_style, Theme::default().style(Role::Muted).bold());
    }

    // 2. Header stays pinned while the body scrolls.
    #[test]
    fn header_stays_pinned_while_body_scrolls() {
        let table = table();
        // Height 4 → body height 3. Select the last row: window becomes 7,8,9.
        let mut state = TableState::default();
        state.select(9);
        let buffer = render(&table, &mut state, Size::new(20, 4), true);
        assert_eq!(state.offset(), 7);
        // Header still at row 0.
        assert_eq!(row(&buffer, 0), "Name            Age");
        // Body window moved to rows 7,8,9.
        assert_eq!(row(&buffer, 1), "name7           27");
        assert_eq!(row(&buffer, 3), "name9           29");
    }

    // 3. Selection movement and clamp.
    #[test]
    fn down_and_up_move_and_clamp() {
        let table = table();
        let mut state = TableState::default();
        let (handled, outcomes) = dispatch(&mut state, Key::Down);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(state.selected(), 1);
        assert_eq!(outcomes, vec![Outcome::Selected(1)]);
        // Up at top is a clamped no-op that emits nothing.
        let mut top = TableState::default();
        let (handled, outcomes) = dispatch(&mut top, Key::Up);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(top.selected(), 0);
        assert!(outcomes.is_empty());
        // Down past the end is re-clamped at render.
        let mut state = TableState::default();
        for _ in 0..20 {
            dispatch(&mut state, Key::Down);
        }
        assert_eq!(state.selected(), 20);
        render(&table, &mut state, Size::new(20, 6), true);
        assert_eq!(state.selected(), 9);
        // End sentinel clamps to the last row at render; Home returns to the first.
        let mut state = TableState::default();
        dispatch(&mut state, Key::End);
        assert_eq!(state.selected(), usize::MAX);
        render(&table, &mut state, Size::new(20, 6), true);
        assert_eq!(state.selected(), 9);
        dispatch(&mut state, Key::Home);
        assert_eq!(state.selected(), 0);
    }

    // 4. PageDown/PageUp move by the recorded page (body height).
    #[test]
    fn page_keys_move_by_recorded_body_height() {
        let table = table();
        let mut state = TableState::default();
        // Render height 4 → body height 3 is recorded as the page.
        render(&table, &mut state, Size::new(20, 4), true);
        let (handled, outcomes) = dispatch(&mut state, Key::PageDown);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(state.selected(), 3);
        assert_eq!(outcomes, vec![Outcome::Selected(3)]);
        let (_h, outcomes) = dispatch(&mut state, Key::PageUp);
        assert_eq!(state.selected(), 0);
        assert_eq!(outcomes, vec![Outcome::Selected(0)]);
    }

    // 5. Window virtualization: cell() called only for the painted window.
    struct Counting {
        rows: usize,
        cols: usize,
        calls: Cell<u32>,
    }

    impl TableSource for Counting {
        fn len(&self) -> usize {
            self.rows
        }

        fn cell(&self, row: usize, col: usize) -> std::borrow::Cow<'_, str> {
            assert!(row < self.rows && col < self.cols);
            self.calls.set(self.calls.get() + 1);
            std::borrow::Cow::Owned(format!("r{row}c{col}"))
        }
    }

    #[test]
    fn cell_calls_are_bounded_by_the_window() {
        let source = Counting {
            rows: 10_000,
            cols: 2,
            calls: Cell::new(0),
        };
        let table = Table::new(source, columns());
        let mut state = TableState::default();
        // Height 6 → body height 5. Bound: (body_height + 1) * columns.
        render(&table, &mut state, Size::new(20, 6), true);
        let body_height = 5u32;
        let bound = (body_height + 1) * 2;
        assert!(
            table.source.calls.get() <= bound,
            "cell() called {} times, bound {bound}",
            table.source.calls.get()
        );

        // A 1_000_000-row lazy source stays O(window) too — count the formatter.
        let calls = Rc::new(Cell::new(0u32));
        let counter = Rc::clone(&calls);
        let source = table_from_fn(1_000_000, move |r, c| {
            counter.set(counter.get() + 1);
            format!("r{r}c{c}")
        });
        let table = Table::new(source, columns());
        let mut state = TableState::default();
        render(&table, &mut state, Size::new(20, 6), true);
        assert!(
            calls.get() <= bound,
            "formatter called {} times",
            calls.get()
        );
    }

    // 6. Empty state: placeholder row, focusable, desired_height 2.
    #[test]
    fn empty_state_paints_placeholder_and_stays_focusable() {
        let table = Table::new(Vec::<Vec<String>>::new(), columns()).empty_text("no rows");
        let mut state = TableState {
            selected: 3,
            offset: 2,
            page: 0,
        };
        let mut buffer = Buffer::new(Size::new(20, 4));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(20, 4)), true);
        table.render(&mut state, &mut ctx);
        assert!(ctx.is_focusable());
        // Header still paints on row 0; placeholder on row 1 in Muted.
        assert_eq!(row(&buffer, 0), "Name            Age");
        assert_eq!(row(&buffer, 1), "no rows");
        let style = buffer.get(Position::new(0, 1)).unwrap().style;
        assert_eq!(style, Theme::default().style(Role::Muted));
        // Selection/offset clamp to zero.
        assert_eq!(state.selected(), 0);
        assert_eq!(state.offset(), 0);
        // desired_height is header + placeholder = 2.
        assert_eq!(table.desired_height(&TableState::default(), 20), 2);
        // Without a placeholder it is header only = 1.
        let bare = Table::new(Vec::<Vec<String>>::new(), columns());
        assert_eq!(bare.desired_height(&TableState::default(), 20), 1);
    }

    #[test]
    fn empty_movement_is_a_safe_no_op_and_emits_no_selected() {
        let table = Table::new(Vec::<Vec<String>>::new(), columns()).empty_text("empty");
        let mut state = TableState::default();
        let (handled, _outcomes) = dispatch(&mut state, Key::Down);
        assert_eq!(handled, Handled::Yes);
        // Render re-clamps to zero; no out-of-range selection survives.
        render(&table, &mut state, Size::new(20, 4), true);
        assert_eq!(state.selected(), 0);
    }

    // 7. Outcomes: Selected on move, Activated on Enter.
    #[test]
    fn outcomes_selected_on_move_activated_on_enter() {
        let mut state = TableState::default();
        let (_h, outcomes) = dispatch(&mut state, Key::Down);
        assert_eq!(outcomes, vec![Outcome::Selected(1)]);
        let (handled, outcomes) = dispatch(&mut state, Key::Enter);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(outcomes, vec![Outcome::Activated]);
    }

    // 8. Click selects the clicked body row; header click selects nothing; wheel moves.
    #[test]
    fn click_selects_body_row_respecting_offset() {
        // Area at y=2, height 5 → header at row 2, body rows at 3,4,5,6.
        let area = Rect::new(Position::new(0, 2), Size::new(20, 5));
        let mut state = TableState {
            selected: 0,
            offset: 0,
            page: 4,
        };
        // Click absolute row 4 → relative 2 → body row 1 → index offset+1 = 1.
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::new(0, 4),
        ));
        let (handled, outcomes) = dispatch_mouse(&mut state, click, area);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(state.selected(), 1);
        assert_eq!(outcomes, vec![Outcome::Selected(1)]);

        // A click respecting an offset: offset 5, body row 1 → index 6.
        let mut state = TableState {
            selected: 5,
            offset: 5,
            page: 4,
        };
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::new(0, 4),
        ));
        let (_h, outcomes) = dispatch_mouse(&mut state, click, area);
        assert_eq!(state.selected(), 6);
        assert_eq!(outcomes, vec![Outcome::Selected(6)]);
    }

    #[test]
    fn click_on_header_row_selects_nothing() {
        let area = Rect::new(Position::new(0, 2), Size::new(20, 5));
        let mut state = TableState {
            selected: 3,
            offset: 0,
            page: 4,
        };
        // Absolute row 2 is the header row.
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::new(0, 2),
        ));
        let (handled, outcomes) = dispatch_mouse(&mut state, click, area);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(state.selected(), 3);
        assert!(outcomes.is_empty());
    }

    #[test]
    fn wheel_moves_selection() {
        let area = Rect::new(Position::ORIGIN, Size::new(20, 5));
        let mut state = TableState {
            selected: 3,
            offset: 0,
            page: 4,
        };
        let down = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Scroll(1),
            MouseButton::None,
            Position::ORIGIN,
        ));
        let (handled, outcomes) = dispatch_mouse(&mut state, down, area);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(state.selected(), 4);
        assert_eq!(outcomes, vec![Outcome::Selected(4)]);
        let up = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Scroll(-1),
            MouseButton::None,
            Position::ORIGIN,
        ));
        let (_h, outcomes) = dispatch_mouse(&mut state, up, area);
        assert_eq!(state.selected(), 3);
        assert_eq!(outcomes, vec![Outcome::Selected(3)]);
    }

    // 9. Column truncation on narrow widths, gutter, and wide graphemes.
    #[test]
    fn cells_truncate_to_column_width_minus_gutter() {
        // Two columns: Length(5) then Length(5). Width 10.
        let cols = vec![
            Column::new("A", Constraint::Length(5)),
            Column::new("B", Constraint::Length(5)),
        ];
        let rows = vec![vec!["abcdefghij".to_string(), "klmnopqrst".to_string()]];
        let table = Table::new(rows, cols);
        let mut state = TableState::default();
        let buffer = render(&table, &mut state, Size::new(10, 2), true);
        // First column (not last): width 5 with a 1-col gutter → 4 chars "abcd".
        // Last column: full width 5 → "klmno".
        assert_eq!(row(&buffer, 1), "abcd klmno");
    }

    #[test]
    fn wide_graphemes_do_not_straddle_the_column_edge() {
        // Column width 4 with gutter → max 3 display columns. A wide char at the
        // boundary is dropped whole rather than bisected.
        let cols = vec![
            Column::new("A", Constraint::Length(4)),
            Column::new("B", Constraint::Length(4)),
        ];
        // "a" (1) + "世" (2) fills 3; the next "界" (2) would exceed and is dropped.
        let rows = vec![vec!["a世界".to_string(), "x".to_string()]];
        let table = Table::new(rows, cols);
        let mut state = TableState::default();
        let buffer = render(&table, &mut state, Size::new(8, 2), true);
        // First column shows "a世" (3 cols) then a gutter; last column "x".
        assert_eq!(row(&buffer, 1), "a世 x");
    }

    // 10. table_from_fn / table_rows_with adapters format only in range.
    #[test]
    fn table_from_fn_formats_only_within_bounds() {
        let src = table_from_fn(3, |r, c| format!("r{r}c{c}"));
        assert_eq!(src.len(), 3);
        assert!(!src.is_empty());
        assert_eq!(&*src.cell(0, 0), "r0c0");
        assert_eq!(&*src.cell(2, 1), "r2c1");
        // Out-of-range row yields an empty cell without calling the formatter.
        assert_eq!(&*src.cell(3, 0), "");
    }

    #[test]
    fn table_rows_with_backs_a_table_by_borrowed_custom_type() {
        struct Person {
            name: &'static str,
            age: u32,
        }
        let people = [
            Person {
                name: "Ada",
                age: 36,
            },
            Person {
                name: "Alan",
                age: 41,
            },
        ];
        let src = table_rows_with(&people, |p, col| match col {
            0 => p.name.to_string(),
            _ => p.age.to_string(),
        });
        assert_eq!(src.len(), 2);
        assert_eq!(&*src.cell(1, 0), "Alan");
        assert_eq!(&*src.cell(0, 1), "36");
        // And it renders through the widget like any other source.
        let table = Table::new(src, columns());
        let mut state = TableState::default();
        let buffer = render(&table, &mut state, Size::new(20, 4), true);
        assert_eq!(row(&buffer, 1), "Ada             36");
    }

    // 11. desired_height is 1 + len (header + rows).
    #[test]
    fn desired_height_is_one_plus_len() {
        let table = table(); // 10 rows
        assert_eq!(table.desired_height(&TableState::default(), 20), 11);
    }

    #[test]
    fn slice_source_agrees_with_vec_source() {
        let owned = data();
        let slice: &[Vec<String>] = &owned;
        assert_eq!(TableSource::len(&slice), 10);
        assert_eq!(&*slice.cell(1, 0), "name1");
        assert_eq!(&*slice.cell(100, 0), "");
    }

    #[test]
    fn selected_row_is_accent_when_unfocused() {
        let table = table();
        let mut state = TableState {
            selected: 1,
            offset: 0,
            page: 0,
        };
        let buffer = render(&table, &mut state, Size::new(20, 6), false);
        // Row 1 is selected → its body row (y = 2) uses Accent when unfocused.
        let style = buffer.get(Position::new(0, 2)).unwrap().style;
        assert_eq!(style, Theme::default().style(Role::Accent));
    }

    #[test]
    fn select_sets_index_and_render_clamps_and_scrolls() {
        let table = table(); // 10 rows
        let mut state = TableState::default();
        state.select(7);
        assert_eq!(state.selected(), 7);
        // Height 4 → body height 3; scroll-into-view brings row 7 to the bottom.
        let buffer = render(&table, &mut state, Size::new(20, 4), true);
        assert_eq!(state.offset(), 5);
        assert_eq!(row(&buffer, 3), "name7           27");
        // An out-of-range selection is re-clamped at render.
        state.select(99);
        render(&table, &mut state, Size::new(20, 4), true);
        assert_eq!(state.selected(), 9);
    }
}
