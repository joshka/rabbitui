//! A single-line, uncontrolled text input with grapheme-correct editing.

use rabbitui_core::geometry::Position;
use rabbitui_core::input::{InputEvent, Key};
use rabbitui_core::outcome::Outcome;
use rabbitui_core::style::Style;
use rabbitui_core::theme::Role;
use rabbitui_core::widget::{HandleCtx, Handled, RenderCtx, Widget};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A single-line text field the user types into.
///
/// # Uncontrolled by design (ADR 0001 delta, slice-4 design note)
///
/// Under the thunk model a handler runs without the spec, so it cannot read an
/// app-owned value at event time. The value is therefore **retained state**
/// (uncontrolled): `TextInput::new()` takes no value, the widget owns the edited
/// string in its [`TextInputState`], and the app learns it through
/// [`Outcome::Changed`] on every edit and [`Outcome::Submitted`] on Enter.
/// Programmatic set/clear is now a **widget command** (slice 6): the app forces
/// the value with `update.widget::<TextInput>(path, |s| s.clear())` (or
/// [`set_value`](TextInputState::set_value)), applied between frames. This
/// replaces slice 4's re-keying workaround and folds back the ADR 0001 delta —
/// the value stays uncontrolled at *event* time (races are impossible) but is
/// controllable at *command* time (the app owns clears and sets).
///
/// # Editing
///
/// All editing is grapheme-cluster correct via the same width oracle the buffer
/// uses, so accents, emoji, and wide CJK behave. The cursor is a **byte offset**
/// held at a grapheme boundary. Bindings (only while focused): printable chars
/// insert at the cursor; Backspace deletes the grapheme before it; Delete the one
/// after; Left/Right move by one grapheme; Home/End jump to the ends; Enter
/// submits. The field scrolls horizontally to keep the cursor in view, and paints
/// the cursor as a reversed cell (the hardware-cursor path via facts is a later
/// slice). Every key the field does not use returns [`Handled::No`], and when
/// unfocused it consumes nothing.
///
/// Styling is by role: [`Role::Text`] for typed text, [`Role::Muted`] for the
/// placeholder, [`Role::Highlight`] for the cursor cell.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::TextInput;
///
/// let query = TextInput::new().placeholder("Search…");
/// assert_eq!(query.get_placeholder(), Some("Search…"));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextInput<'a> {
    placeholder: Option<&'a str>,
}

impl<'a> TextInput<'a> {
    /// Creates an empty text input with no placeholder.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::TextInput;
    ///
    /// let input = TextInput::new();
    /// assert_eq!(input.get_placeholder(), None);
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self { placeholder: None }
    }

    /// Sets the placeholder shown, in [`Role::Muted`], while the field is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::TextInput;
    ///
    /// let input = TextInput::new().placeholder("name");
    /// assert_eq!(input.get_placeholder(), Some("name"));
    /// ```
    #[must_use]
    pub const fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = Some(placeholder);
        self
    }

    /// The placeholder text, if one was set.
    #[must_use]
    pub const fn get_placeholder(&self) -> Option<&'a str> {
        self.placeholder
    }
}

/// The retained state of a [`TextInput`]: the edited value, the cursor, and the
/// horizontal scroll offset.
///
/// Owned by the framework's state store, keyed by the widget's identity (ADR
/// 0002), so the value survives across frames while the spec is rebuilt each
/// frame. Apps do not construct this; the store defaults it on first appearance.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::text_input::TextInputState;
///
/// let mut state = TextInputState::default();
/// state.insert_str("café");
/// assert_eq!(state.value(), "café");
/// // The cursor sits at the end, one past the last grapheme (in bytes).
/// assert_eq!(state.cursor(), "café".len());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInputState {
    value: String,
    /// Byte offset of the cursor, always on a grapheme boundary of `value`.
    cursor: usize,
    /// The first displayed column (in cells), advanced to keep the cursor in
    /// view. Recomputed each render against the area width.
    scroll: u16,
}

impl TextInputState {
    /// The current value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The cursor's byte offset (always at a grapheme boundary).
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Inserts `text` at the cursor and advances the cursor past it.
    ///
    /// A convenience used by the handler (one char at a time) and by tests
    /// (whole strings). `text` is inserted verbatim; grapheme correctness is a
    /// property of where the cursor may sit, which this preserves because the
    /// cursor is always on a boundary and insertion happens there.
    pub fn insert_str(&mut self, text: &str) {
        self.value.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    /// Clears the value and resets the cursor and scroll to the start.
    ///
    /// The programmatic reset a widget command drives (slice 6): the app clears
    /// the field on submit with
    /// `update.widget::<TextInput>(path, |s| s.clear())`, replacing the slice-4
    /// re-keying workaround. Because it is a controlled mutation the app owns the
    /// timing, not the field.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::text_input::TextInputState;
    ///
    /// let mut state = TextInputState::default();
    /// state.insert_str("draft");
    /// state.clear();
    /// assert_eq!(state.value(), "");
    /// assert_eq!(state.cursor(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
        self.scroll = 0;
    }

    /// Replaces the value with `value` and places the cursor at its end.
    ///
    /// The controlled-set companion to [`clear`](Self::clear): a widget command
    /// forces the field's content (a recalled draft, a completion). The cursor
    /// lands at the end (a grapheme boundary by construction); the scroll offset
    /// is recomputed at the next render to keep the cursor in view.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::text_input::TextInputState;
    ///
    /// let mut state = TextInputState::default();
    /// state.set_value("hello");
    /// assert_eq!(state.value(), "hello");
    /// assert_eq!(state.cursor(), "hello".len());
    /// ```
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.cursor = self.value.len();
        // The next render recomputes scroll against the area width; resetting it
        // here avoids a stale offset if the new value is shorter.
        self.scroll = 0;
    }

    /// Deletes the grapheme immediately before the cursor (Backspace), if any.
    ///
    /// Returns true if something was removed.
    fn delete_backward(&mut self) -> bool {
        let Some(prev) = self.prev_boundary(self.cursor) else {
            return false;
        };
        self.value.replace_range(prev..self.cursor, "");
        self.cursor = prev;
        true
    }

    /// Deletes the grapheme immediately after the cursor (Delete), if any.
    ///
    /// Returns true if something was removed.
    fn delete_forward(&mut self) -> bool {
        let Some(next) = self.next_boundary(self.cursor) else {
            return false;
        };
        self.value.replace_range(self.cursor..next, "");
        true
    }

    /// Moves the cursor one grapheme left; returns true if it moved.
    fn move_left(&mut self) -> bool {
        match self.prev_boundary(self.cursor) {
            Some(prev) => {
                self.cursor = prev;
                true
            }
            None => false,
        }
    }

    /// Moves the cursor one grapheme right; returns true if it moved.
    fn move_right(&mut self) -> bool {
        match self.next_boundary(self.cursor) {
            Some(next) => {
                self.cursor = next;
                true
            }
            None => false,
        }
    }

    /// Moves the cursor to the start; returns true if it moved.
    fn move_home(&mut self) -> bool {
        let moved = self.cursor != 0;
        self.cursor = 0;
        moved
    }

    /// Moves the cursor to the end; returns true if it moved.
    fn move_end(&mut self) -> bool {
        let end = self.value.len();
        let moved = self.cursor != end;
        self.cursor = end;
        moved
    }

    /// The byte offset of the grapheme boundary just before `at`, or `None` if
    /// `at` is at the start.
    fn prev_boundary(&self, at: usize) -> Option<usize> {
        self.value[..at]
            .grapheme_indices(true)
            .next_back()
            .map(|(index, _)| index)
    }

    /// The byte offset of the grapheme boundary just after `at`, or `None` if
    /// `at` is at the end.
    fn next_boundary(&self, at: usize) -> Option<usize> {
        self.value[at..]
            .graphemes(true)
            .next()
            .map(|first| at + first.len())
    }

    /// The display width, in cells, of the value up to the cursor — the cursor's
    /// column before scrolling.
    fn cursor_column(&self) -> u16 {
        u16::try_from(UnicodeWidthStr::width(&self.value[..self.cursor])).unwrap_or(u16::MAX)
    }
}

impl Widget for TextInput<'_> {
    type State = TextInputState;

    fn render(&self, state: &mut TextInputState, ctx: &mut RenderCtx<'_>) {
        ctx.focusable(true);
        let width = ctx.size().width;
        if width == 0 {
            return;
        }

        // Keep the cursor visible: scroll right if it ran past the right edge,
        // left if it moved before the current offset. The cursor occupies one
        // cell, so the last visible cursor column is `scroll + width - 1`.
        let cursor_col = state.cursor_column();
        if cursor_col < state.scroll {
            state.scroll = cursor_col;
        } else if cursor_col >= state.scroll + width {
            state.scroll = cursor_col - width + 1;
        }

        let text_style = ctx.style(Role::Text);
        if state.value.is_empty() {
            if let Some(placeholder) = self.placeholder {
                if !ctx.is_focused() {
                    ctx.set_string(Position::ORIGIN, placeholder, ctx.style(Role::Muted));
                }
            }
        } else {
            // Paint the value shifted left by `scroll` cells. Walk graphemes,
            // tracking each one's starting column, and place those within the
            // visible window.
            paint_scrolled(ctx, &state.value, state.scroll, text_style);
        }

        // Paint the cursor as a reversed/highlight cell at its visible column.
        if ctx.is_focused() {
            let visible = cursor_col.saturating_sub(state.scroll);
            if visible < width {
                let under = grapheme_at(&state.value, state.cursor);
                let cursor_style = cursor_style(ctx.style(Role::Highlight));
                ctx.set_string(Position::new(visible, 0), under, cursor_style);
            }
        }
    }

    fn handle(state: &mut TextInputState, event: &InputEvent, ctx: &mut HandleCtx<'_>) -> Handled {
        // A mouse press over the field is left *unconsumed* so the router's
        // click-to-focus focuses this focusable field (slice-7 design note:
        // "TextInput Down → focus"). Placing the cursor at the clicked column is a
        // recorded later refinement; for now a click only focuses, and the cursor
        // stays where it was.
        let Some(key) = event.as_key() else {
            return Handled::No;
        };
        // Modifiers other than Shift are not text editing here; leave them for
        // the app (e.g. Ctrl-C to quit).
        if key.modifiers.ctrl || key.modifiers.alt {
            return Handled::No;
        }
        match key.key {
            Key::Char(ch) => {
                let mut buffer = [0u8; 4];
                state.insert_str(ch.encode_utf8(&mut buffer));
                ctx.emit(Outcome::Changed(state.value.clone()));
                Handled::Yes
            }
            Key::Backspace => edit(state, ctx, TextInputState::delete_backward),
            Key::Delete => edit(state, ctx, TextInputState::delete_forward),
            Key::Left => move_cursor(state, TextInputState::move_left),
            Key::Right => move_cursor(state, TextInputState::move_right),
            Key::Home => move_cursor(state, TextInputState::move_home),
            Key::End => move_cursor(state, TextInputState::move_end),
            Key::Enter => {
                ctx.emit(Outcome::Submitted);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

/// Applies an edit `op`; emits [`Outcome::Changed`] if it changed the value.
///
/// The field always *consumes* an edit key even when the value did not change
/// (Backspace at the start is still "handled"), so the key never falls through
/// to the app as a raw event while the field is focused.
fn edit(
    state: &mut TextInputState,
    ctx: &mut HandleCtx<'_>,
    op: fn(&mut TextInputState) -> bool,
) -> Handled {
    if op(state) {
        ctx.emit(Outcome::Changed(state.value.clone()));
    }
    Handled::Yes
}

/// Applies a cursor-move `op`; movement emits no outcome (the value is
/// unchanged) but still consumes the key so it does not double as app input.
fn move_cursor(state: &mut TextInputState, op: fn(&mut TextInputState) -> bool) -> Handled {
    op(state);
    Handled::Yes
}

/// Paints `value` into the context's row, shifted left by `scroll` cells, so the
/// visible window starts at display column `scroll`.
fn paint_scrolled(ctx: &mut RenderCtx<'_>, value: &str, scroll: u16, style: Style) {
    let mut column: u16 = 0;
    for grapheme in value.graphemes(true) {
        let advance = u16::try_from(UnicodeWidthStr::width(grapheme))
            .unwrap_or(1)
            .max(1);
        // Only paint graphemes whose start is at or past the scroll offset; a
        // wide grapheme half-scrolled off the left edge is dropped rather than
        // shown clipped.
        if column >= scroll {
            let x = column - scroll;
            ctx.set_string(Position::new(x, 0), grapheme, style);
        }
        column = column.saturating_add(advance);
    }
}

/// The grapheme at byte offset `at`, or a space if the cursor sits at the end.
///
/// The cursor cell shows whatever grapheme it covers (so a wide glyph stays wide
/// under the cursor); at end-of-text there is nothing to cover, so it shows a
/// space.
fn grapheme_at(value: &str, at: usize) -> &str {
    value[at..].graphemes(true).next().unwrap_or(" ")
}

/// The cursor cell's style: the highlight role plus reverse video, so the cursor
/// reads as a block regardless of the theme's highlight (v1 software cursor).
fn cursor_style(highlight: Style) -> Style {
    highlight.reversed()
}

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::input::{InputEvent, Key, KeyEvent, Modifiers};
    use rabbitui_core::outcome::Outcome;
    use rabbitui_core::style::Attrs;
    use rabbitui_core::widget::{HandleCtx, Handled, Phase, RenderCtx, Widget};

    use super::{TextInput, TextInputState};

    /// Dispatches one key to a fresh-or-given state, returning (handled, outcomes).
    fn dispatch(state: &mut TextInputState, event: InputEvent) -> (Handled, Vec<Outcome>) {
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let handled = {
            let mut ctx = HandleCtx::new(
                Phase::Bubble,
                Rect::default(),
                &mut outcomes,
                &mut request_focus,
            );
            TextInput::handle(state, &event, &mut ctx)
        };
        (handled, outcomes)
    }

    fn type_str(state: &mut TextInputState, text: &str) {
        for ch in text.chars() {
            dispatch(state, InputEvent::key(Key::Char(ch)));
        }
    }

    #[test]
    fn typing_inserts_and_emits_changed() {
        let mut state = TextInputState::default();
        let (handled, outcomes) = dispatch(&mut state, InputEvent::key(Key::Char('a')));
        assert_eq!(handled, Handled::Yes);
        assert_eq!(outcomes, vec![Outcome::Changed("a".to_string())]);
        assert_eq!(state.value(), "a");
        assert_eq!(state.cursor(), 1);
    }

    #[test]
    fn enter_submits_without_clearing() {
        let mut state = TextInputState::default();
        type_str(&mut state, "hi");
        let (handled, outcomes) = dispatch(&mut state, InputEvent::key(Key::Enter));
        assert_eq!(handled, Handled::Yes);
        assert_eq!(outcomes, vec![Outcome::Submitted]);
        // Submitted leaves the value in place; the app decides whether to clear.
        assert_eq!(state.value(), "hi");
    }

    #[test]
    fn backspace_deletes_grapheme_before_cursor() {
        // "héllo": the accent is a combining sequence in "é" here as a single
        // scalar, but exercise the boundary logic with a real multi-byte char.
        let mut state = TextInputState::default();
        type_str(&mut state, "héllo");
        assert_eq!(state.value(), "héllo");
        // Cursor at end; backspace removes "o".
        let (_h, outcomes) = dispatch(&mut state, InputEvent::key(Key::Backspace));
        assert_eq!(state.value(), "héll");
        assert_eq!(outcomes, vec![Outcome::Changed("héll".to_string())]);
    }

    #[test]
    fn backspace_over_multibyte_removes_whole_grapheme() {
        let mut state = TextInputState::default();
        type_str(&mut state, "aé");
        // Cursor after "é" (3 bytes: 'a' + 2-byte 'é'). One backspace removes é.
        assert_eq!(state.cursor(), "aé".len());
        dispatch(&mut state, InputEvent::key(Key::Backspace));
        assert_eq!(state.value(), "a");
        assert_eq!(state.cursor(), 1);
    }

    #[test]
    fn backspace_over_combining_accent_grapheme() {
        // "e" + U+0301 combining acute = one grapheme, two scalars, three bytes.
        let mut state = TextInputState::default();
        state.insert_str("e\u{0301}");
        assert_eq!(state.value().chars().count(), 2);
        // A single backspace removes the whole grapheme cluster.
        dispatch(&mut state, InputEvent::key(Key::Backspace));
        assert_eq!(state.value(), "");
    }

    #[test]
    fn backspace_over_emoji_removes_whole_cluster() {
        let mut state = TextInputState::default();
        // A family emoji is one grapheme built of several scalars joined by ZWJ.
        let family = "👨‍👩‍👧";
        state.insert_str(family);
        assert!(family.len() > 4);
        dispatch(&mut state, InputEvent::key(Key::Backspace));
        assert_eq!(state.value(), "");
    }

    #[test]
    fn delete_removes_grapheme_after_cursor() {
        let mut state = TextInputState::default();
        type_str(&mut state, "abc");
        state.cursor = 0;
        dispatch(&mut state, InputEvent::key(Key::Delete));
        assert_eq!(state.value(), "bc");
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn left_right_move_by_grapheme_over_wide_cjk() {
        let mut state = TextInputState::default();
        type_str(&mut state, "世界"); // two wide graphemes, 3 bytes each.
        assert_eq!(state.cursor(), "世界".len());
        // Left twice returns to the start, one grapheme at a time.
        dispatch(&mut state, InputEvent::key(Key::Left));
        assert_eq!(state.cursor(), "世".len());
        dispatch(&mut state, InputEvent::key(Key::Left));
        assert_eq!(state.cursor(), 0);
        // Left again at the start is a no-op (still consumed).
        let (handled, _o) = dispatch(&mut state, InputEvent::key(Key::Left));
        assert_eq!(handled, Handled::Yes);
        assert_eq!(state.cursor(), 0);
        // Right moves forward by a whole wide grapheme.
        dispatch(&mut state, InputEvent::key(Key::Right));
        assert_eq!(state.cursor(), "世".len());
    }

    #[test]
    fn home_and_end_jump_to_the_ends() {
        let mut state = TextInputState::default();
        type_str(&mut state, "hello");
        dispatch(&mut state, InputEvent::key(Key::Home));
        assert_eq!(state.cursor(), 0);
        dispatch(&mut state, InputEvent::key(Key::End));
        assert_eq!(state.cursor(), 5);
    }

    #[test]
    fn insert_in_the_middle() {
        let mut state = TextInputState::default();
        type_str(&mut state, "ac");
        dispatch(&mut state, InputEvent::key(Key::Left)); // between a and c
        dispatch(&mut state, InputEvent::key(Key::Char('b')));
        assert_eq!(state.value(), "abc");
    }

    #[test]
    fn ctrl_and_alt_keys_are_not_consumed() {
        let mut state = TextInputState::default();
        let ctrl_c = InputEvent::Key(KeyEvent::new(Key::Char('c')).ctrl());
        let (handled, outcomes) = dispatch(&mut state, ctrl_c);
        assert_eq!(handled, Handled::No);
        assert!(outcomes.is_empty());
        assert_eq!(state.value(), "");
        // A plain shift-char still types.
        let shift_a = InputEvent::Key(
            KeyEvent::new(Key::Char('A')).with_modifiers(Modifiers::NONE.with_shift()),
        );
        let (handled, _o) = dispatch(&mut state, shift_a);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(state.value(), "A");
    }

    /// Renders a state into a fresh buffer of `width` and returns it.
    fn render_state(state: &mut TextInputState, width: u16, focused: bool) -> Buffer {
        let mut buffer = Buffer::new(Size::new(width, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(width, 1)), focused);
        TextInput::new().render(state, &mut ctx);
        buffer
    }

    fn row(buffer: &Buffer) -> String {
        let mut line = String::new();
        for x in 0..buffer.size().width {
            line.push_str(&buffer.get(Position::new(x, 0)).unwrap().symbol);
        }
        line.trim_end().to_string()
    }

    #[test]
    fn cursor_cell_is_reversed_when_focused() {
        let mut state = TextInputState::default();
        type_str(&mut state, "ab");
        state.cursor = 0;
        let buffer = render_state(&mut state, 5, true);
        // The cursor sits on 'a' at column 0, painted reversed.
        assert!(
            buffer
                .get(Position::new(0, 0))
                .unwrap()
                .style
                .attrs
                .contains(Attrs::REVERSED)
        );
        // A non-cursor cell is not reversed.
        assert!(
            !buffer
                .get(Position::new(1, 0))
                .unwrap()
                .style
                .attrs
                .contains(Attrs::REVERSED)
        );
    }

    #[test]
    fn no_cursor_painted_when_unfocused() {
        let mut state = TextInputState::default();
        type_str(&mut state, "ab");
        let buffer = render_state(&mut state, 5, false);
        for x in 0..2 {
            assert!(
                !buffer
                    .get(Position::new(x, 0))
                    .unwrap()
                    .style
                    .attrs
                    .contains(Attrs::REVERSED)
            );
        }
    }

    #[test]
    fn placeholder_shows_only_when_empty_and_unfocused() {
        let mut empty = TextInputState::default();
        let mut buffer = Buffer::new(Size::new(10, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(10, 1)), false);
        TextInput::new()
            .placeholder("type…")
            .render(&mut empty, &mut ctx);
        assert_eq!(row(&buffer), "type…");
    }

    #[test]
    fn scroll_keeps_cursor_visible_past_right_edge() {
        let mut state = TextInputState::default();
        type_str(&mut state, "abcdefgh"); // 8 cells, cursor at end (col 8).
        // Width 4: the last visible column is scroll+3; to show col 8 the scroll
        // becomes 8 - 4 + 1 = 5, so the window shows "fgh" then the cursor cell.
        let buffer = render_state(&mut state, 4, true);
        // Visible text (before cursor) starts at "f".
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, "f");
        assert_eq!(state.scroll, 5);
    }

    #[test]
    fn scroll_returns_left_when_cursor_moves_back() {
        let mut state = TextInputState::default();
        type_str(&mut state, "abcdefgh");
        render_state(&mut state, 4, true);
        assert_eq!(state.scroll, 5);
        // Home: cursor to 0, next render scrolls back to 0.
        state.cursor = 0;
        let buffer = render_state(&mut state, 4, true);
        assert_eq!(state.scroll, 0);
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, "a");
    }

    #[test]
    fn clear_resets_value_cursor_and_scroll() {
        let mut state = TextInputState::default();
        type_str(&mut state, "abcdefgh");
        render_state(&mut state, 4, true);
        assert_eq!(state.scroll, 5);
        state.clear();
        assert_eq!(state.value(), "");
        assert_eq!(state.cursor(), 0);
        assert_eq!(state.scroll, 0);
    }

    #[test]
    fn set_value_replaces_and_places_cursor_at_end() {
        let mut state = TextInputState::default();
        type_str(&mut state, "old");
        state.set_value("hello");
        assert_eq!(state.value(), "hello");
        assert_eq!(state.cursor(), "hello".len());
    }

    #[test]
    fn mouse_press_is_unconsumed_so_router_click_to_focus_applies() {
        use rabbitui_core::geometry::Position;
        use rabbitui_core::input::{MouseButton, MouseEvent, MouseKind};
        // The field does not consume a click (cursor placement is deferred); it
        // returns Handled::No so the router focuses this focusable field.
        let mut state = TextInputState::default();
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::ORIGIN,
        ));
        let (handled, outcomes) = dispatch(&mut state, click);
        assert_eq!(handled, Handled::No);
        assert!(outcomes.is_empty());
    }

    #[test]
    fn unfocused_handler_is_still_only_reached_via_focus() {
        // The handler consumes editing keys; the router only invokes it when the
        // field is the focus target, so "consumes only when focused" is enforced
        // by routing. Here we assert the handler itself does not, e.g., treat an
        // unmapped key as handled.
        let mut state = TextInputState::default();
        let (handled, _o) = dispatch(&mut state, InputEvent::key(Key::Tab));
        assert_eq!(handled, Handled::No);
    }
}
