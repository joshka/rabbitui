//! Declared-frame forms: a label-aligned stack of fields, laid out for you.
//!
//! rabbitui widgets do not nest (a [`Panel`](crate::Panel) is a backdrop, not a
//! container), so a form is **not a container widget** — it is a *declaration
//! helper*, exactly like [`ScrollScope`](rabbitui_core::scroll::ScrollScope):
//! [`form`](fn@form) opens a scope, and [`FormScope::field`] declares one field row at a
//! time, tracking a cursor down the area so the caller never does the layout
//! arithmetic.
//!
//! ```text
//! form(frame, key("login"), area, label_width(["User", "Password"]), |form| {
//!     form.field(key("user"), FieldSpec::new("User").required(), &user_input);
//!     form.field(key("pass"), FieldSpec::new("Password").error(pass_err), &pass_input);
//!     form.buttons(|frame, row| frame.widget(key("submit"), row, &submit));
//! });
//! ```
//!
//! # What the form does and does not own
//!
//! The form owns **layout** — a right-aligned label column, the input beside it,
//! an error line below when one is set, and a running cursor — and nothing else.
//! It never validates: validation is app-land by contract (ADR 0001, the
//! framework never owns app state). The app validates on a field's
//! [`Changed`](rabbitui_core::outcome::Outcome::Changed) /
//! [`Submitted`](rabbitui_core::outcome::Outcome::Submitted) outcome and passes
//! the message back in through [`FieldSpec::error`]; the form only *displays* it,
//! in [`Role::Danger`]. A required field renders a `*` marker (also
//! [`Role::Danger`]) next to its label; that is a hint, not a rule — the form
//! never blocks on it.
//!
//! # Focus and traversal
//!
//! Fields are ordinary declared widgets, so focus needs nothing new: Tab order
//! falls out of the frame facts in **declaration order** — the order of the
//! [`field`](FormScope::field) calls. The input is declared under the `key` you
//! pass, so the app reads its outcomes at the same path
//! (`[form_key, field_key]`).
//!
//! # Row accounting
//!
//! Each [`field`](FormScope::field) consumes the input widget's
//! [`desired_height`](rabbitui_core::widget::Widget::desired_height) rows, one
//! more row when an error is shown, and a trailing blank row so consecutive
//! fields breathe without a manual gap. [`gap`](FormScope::gap) inserts an extra
//! break (before a button row, say), and [`buttons`](FormScope::buttons) reserves
//! one row.

use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::{Position, Rect, Size};
use rabbitui_core::id::{Key, key as child_key};
use rabbitui_core::theme::Role;
use rabbitui_core::widget::Widget;
use unicode_width::UnicodeWidthStr;

use crate::Text;

/// Columns of blank space between the label column and the input column.
const LABEL_GAP: u16 = 2;

/// The declaration of one form field: its label, an optional error to show
/// below it, and whether it is marked required.
///
/// Built fluently and handed to [`FormScope::field`] alongside the input widget.
/// The error is borrowed (`Option<&str>`) because it is app-owned validation
/// state the form only displays — see the [module docs](self) on the app-land
/// contract.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::form::FieldSpec;
///
/// let plain = FieldSpec::new("Name");
/// let flagged = FieldSpec::new("Email").required().error(Some("must contain @"));
/// assert_eq!(flagged.label(), "Email");
/// assert!(flagged.is_required());
/// assert_eq!(flagged.get_error(), Some("must contain @"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSpec<'a> {
    label: &'a str,
    error: Option<&'a str>,
    required: bool,
}

impl<'a> FieldSpec<'a> {
    /// Creates a field spec for `label`, with no error and not required.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::form::FieldSpec;
    ///
    /// let spec = FieldSpec::new("Name");
    /// assert_eq!(spec.label(), "Name");
    /// assert!(!spec.is_required());
    /// ```
    #[must_use]
    pub const fn new(label: &'a str) -> Self {
        Self {
            label,
            error: None,
            required: false,
        }
    }

    /// Sets the error message shown below the field, or clears it with `None`.
    ///
    /// The app passes its own validation result here; the form displays the
    /// message in [`Role::Danger`] and never judges it (ADR 0001). Passing
    /// `None` (the default) shows no error line and reclaims the row.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::form::FieldSpec;
    ///
    /// let spec = FieldSpec::new("Email").error(Some("required"));
    /// assert_eq!(spec.get_error(), Some("required"));
    /// ```
    #[must_use]
    pub const fn error(mut self, error: Option<&'a str>) -> Self {
        self.error = error;
        self
    }

    /// Marks the field required, rendering a `*` marker next to its label.
    ///
    /// The marker is a visual hint only — the form does not enforce it (the app
    /// owns validation). Paints in [`Role::Danger`], like the error line.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::form::FieldSpec;
    ///
    /// assert!(FieldSpec::new("User").required().is_required());
    /// ```
    #[must_use]
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// The field's label text.
    #[must_use]
    pub const fn label(&self) -> &'a str {
        self.label
    }

    /// The error message set on this field, if any.
    #[must_use]
    pub const fn get_error(&self) -> Option<&'a str> {
        self.error
    }

    /// Whether the field is marked required.
    #[must_use]
    pub const fn is_required(&self) -> bool {
        self.required
    }
}

/// The label column width for a set of labels: the widest label's display width.
///
/// The [`form`](fn@form) entry takes a caller-supplied `label_width` (single-pass layout,
/// so the width must be known before the fields declare); this helper computes
/// it from the labels up front, keeping the call site free of layout arithmetic.
/// Widths are Unicode display widths, so wide (CJK) labels are measured
/// correctly.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::form::label_width;
///
/// assert_eq!(label_width(["User", "Password"]), 8); // "Password"
/// assert_eq!(label_width(["A", "bb", "ccc"]), 3);
/// ```
#[must_use]
pub fn label_width<'a>(labels: impl IntoIterator<Item = &'a str>) -> u16 {
    labels
        .into_iter()
        .map(|label| u16::try_from(UnicodeWidthStr::width(label)).unwrap_or(u16::MAX))
        .max()
        .unwrap_or(0)
}

/// The scope a [`form`](fn@form) closure declares fields into.
///
/// Fields are declared top to bottom with [`field`](Self::field); the scope
/// tracks a cursor down the form's area so each call lands under the previous
/// one. [`gap`](Self::gap) and [`buttons`](Self::buttons) insert a break and a
/// trailing control row. See the [module docs](self) for the layout contract.
pub struct FormScope<'a, 'f> {
    /// The child frame fields declare into (the form scope's id is its parent).
    frame: &'a mut Frame<'f>,
    /// The form's left edge (absolute buffer x).
    left: u16,
    /// The form's right edge, exclusive (absolute buffer x).
    right: u16,
    /// The label column width (display cells), from the caller.
    label_width: u16,
    /// The next free row (absolute buffer y); advances as items are declared.
    cursor_y: u16,
    /// The number of fields declared so far, for unique label/error keys.
    index: usize,
}

impl FormScope<'_, '_> {
    /// The absolute x where the input column begins.
    fn input_x(&self) -> u16 {
        self.left
            .saturating_add(self.label_width)
            .saturating_add(LABEL_GAP)
    }

    /// The width available to the input column.
    fn input_width(&self) -> u16 {
        self.right.saturating_sub(self.input_x())
    }

    /// Declares one field row: a right-aligned label, the input widget beside it,
    /// and — when [`FieldSpec::error`] is set — an error line below.
    ///
    /// `key` scopes the input, so the app reads its outcomes at
    /// `[form_key, key]`; declaration order is Tab order. The input is measured at
    /// the input column width and declared at its
    /// [`desired_height`](rabbitui_core::widget::Widget::desired_height), so a
    /// multi-line input (or one that grows) lays out correctly. The cursor then
    /// advances past the input, the error line (if any), and a trailing blank row.
    pub fn field<W: Widget>(&mut self, key: Key, spec: FieldSpec<'_>, widget: &W) {
        let row_y = self.cursor_y;
        let input_x = self.input_x();
        let input_width = self.input_width();
        let height = self.frame.measure(key, input_width, widget).max(1);

        // The label, right-aligned within the label column [left, left+width).
        let label_cells = u16::try_from(UnicodeWidthStr::width(spec.label)).unwrap_or(u16::MAX);
        let draw_width = label_cells.min(self.label_width);
        let label_x = self.left + (self.label_width - draw_width);
        self.frame.widget(
            child_key("__form_label").index(self.index),
            Rect::new(Position::new(label_x, row_y), Size::new(draw_width, 1)),
            &Text::new(spec.label),
        );

        // The required marker sits just past the label column, in the gap.
        if spec.required {
            self.frame.widget(
                child_key("__form_marker").index(self.index),
                Rect::new(
                    Position::new(self.left + self.label_width, row_y),
                    Size::new(1, 1),
                ),
                &Text::new("*").role(Role::Danger),
            );
        }

        // The input widget, filling the rest of the row at its desired height.
        self.frame.widget(
            key,
            Rect::new(
                Position::new(input_x, row_y),
                Size::new(input_width, height),
            ),
            widget,
        );

        let mut consumed = height;

        // The error line, one row below the input, in Danger.
        if let Some(error) = spec.error {
            self.frame.widget(
                child_key("__form_error").index(self.index),
                Rect::new(
                    Position::new(input_x, row_y + height),
                    Size::new(input_width, 1),
                ),
                &Text::new(error).role(Role::Danger),
            );
            consumed += 1;
        }

        // A trailing blank row so consecutive fields breathe.
        self.cursor_y = self.cursor_y.saturating_add(consumed).saturating_add(1);
        self.index += 1;
    }

    /// Inserts a vertical gap of `rows` blank rows before the next item.
    ///
    /// Fields already leave one blank row beneath themselves; use `gap` for a
    /// larger break, e.g. before a [`buttons`](Self::buttons) row.
    pub fn gap(&mut self, rows: u16) {
        self.cursor_y = self.cursor_y.saturating_add(rows);
    }

    /// Declares a trailing control row (Submit / Cancel, say) and hands the
    /// closure the row's [`Rect`] to declare buttons into.
    ///
    /// The row spans the input column — aligned under the fields — and is one row
    /// tall. The closure declares its own widgets (typically right-aligned within
    /// the row) directly on the form's frame, so their keys live under the form
    /// scope like the fields' do. The cursor advances one row.
    pub fn buttons(&mut self, f: impl FnOnce(&mut Frame<'_>, Rect)) {
        let input_x = self.input_x();
        let row = Rect::new(
            Position::new(input_x, self.cursor_y),
            Size::new(self.input_width(), 1),
        );
        f(self.frame, row);
        self.cursor_y = self.cursor_y.saturating_add(1);
    }
}

/// Declares a **form**: a scope that stacks label-aligned fields down `area`.
///
/// Opens an identity scope under `key` (so field keys live at `[key, field_key]`)
/// and runs `f` against a [`FormScope`], which lays out each
/// [`field`](FormScope::field) for you — a right-aligned label column
/// `label_width` cells wide, the input beside it, and error lines below. Layout
/// is single-pass, so `label_width` is supplied up front; compute it with
/// [`label_width`] to keep the call site arithmetic-free. Returns the number of
/// rows the form consumed, so a caller can size or border the area around it.
///
/// Validation is the app's: the form displays [`FieldSpec::error`] messages but
/// never judges them (ADR 0001). See the [module docs](self).
///
/// # Examples
///
/// ```
/// use rabbitui_core::buffer::Buffer;
/// use rabbitui_core::frame::Frame;
/// use rabbitui_core::geometry::Size;
/// use rabbitui_core::id::key;
/// use rabbitui_core::store::StateStore;
/// use rabbitui_widgets::TextInput;
/// use rabbitui_widgets::form::{FieldSpec, form, label_width};
///
/// let mut buffer = Buffer::new(Size::new(40, 6));
/// let mut store = StateStore::new();
/// store.begin_frame();
/// let mut frame = Frame::new(&mut buffer, &mut store);
/// let area = frame.area();
/// let rows = form(
///     &mut frame,
///     key("login"),
///     area,
///     label_width(["User", "Password"]),
///     |form| {
///         form.field(key("user"), FieldSpec::new("User").required(), &TextInput::new());
///         form.field(key("pass"), FieldSpec::new("Password"), &TextInput::new());
///     },
/// );
/// assert_eq!(rows, 4); // two single-row fields, each with a trailing blank row
/// # let _ = frame.finish();
/// store.end_frame();
/// ```
pub fn form(
    frame: &mut Frame<'_>,
    key: Key,
    area: Rect,
    label_width: u16,
    f: impl FnOnce(&mut FormScope),
) -> u16 {
    let mut consumed = 0;
    frame.scoped(key, |child| {
        let mut scope = FormScope {
            frame: child,
            left: area.origin.x,
            right: area.origin.x.saturating_add(area.size.width),
            label_width,
            cursor_y: area.origin.y,
            index: 0,
        };
        f(&mut scope);
        consumed = scope.cursor_y.saturating_sub(area.origin.y);
    });
    consumed
}

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::facts::FrameFacts;
    use rabbitui_core::frame::Frame;
    use rabbitui_core::geometry::{Position, Size};
    use rabbitui_core::id::{WidgetId, key};
    use rabbitui_core::store::StateStore;
    use rabbitui_core::theme::{Role, Theme};

    use super::{FieldSpec, form, label_width};
    use crate::{Text, TextInput};

    /// Renders one form frame of `size`, returning the painted buffer, the frame
    /// facts, and the row count the form reported.
    fn render_form(
        store: &mut StateStore,
        size: Size,
        width: u16,
        body: impl FnOnce(&mut super::FormScope),
    ) -> (Buffer, FrameFacts, u16) {
        let mut buffer = Buffer::new(size);
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, store);
        let area = frame.area();
        let rows = form(&mut frame, key("form"), area, width, body);
        let facts = frame.finish();
        store.end_frame();
        (buffer, facts, rows)
    }

    /// Reads columns `x0..x1` of row `y` back as a trailing-trimmed string.
    fn cells(buffer: &Buffer, y: u16, x0: u16, x1: u16) -> String {
        let mut line = String::new();
        for x in x0..x1 {
            line.push_str(&buffer.get(Position::new(x, y)).unwrap().symbol);
        }
        line.trim_end().to_string()
    }

    /// The composed id of a field's input under the form scope.
    fn field_id(field: &str) -> WidgetId {
        WidgetId::ROOT.child(key("form")).child(key(field))
    }

    #[test]
    fn label_width_measures_the_widest_label() {
        assert_eq!(label_width(["User", "Password"]), 8);
        assert_eq!(label_width(["A", "bb", "ccc"]), 3);
        assert_eq!(label_width(Vec::<&str>::new()), 0);
    }

    #[test]
    fn labels_are_right_aligned_in_the_label_column() {
        // Labels of different widths in a column of width 4: both must end at the
        // same column (right edge = col 4), so a short label is left-padded.
        let mut store = StateStore::new();
        let (buffer, _facts, _rows) = render_form(&mut store, Size::new(20, 6), 4, |form| {
            form.field(key("a"), FieldSpec::new("A"), &TextInput::new());
            form.field(key("long"), FieldSpec::new("Long"), &TextInput::new());
        });
        // "A" right-aligned in [0,4) → three leading spaces, char at col 3.
        assert_eq!(cells(&buffer, 0, 0, 4), "   A");
        assert_eq!(buffer.get(Position::new(3, 0)).unwrap().symbol, "A");
        // "Long" fills the column exactly, ending at col 3 like "A".
        assert_eq!(cells(&buffer, 2, 0, 4), "Long");
        assert_eq!(buffer.get(Position::new(3, 2)).unwrap().symbol, "g");
    }

    #[test]
    fn error_line_appears_below_the_field_and_reclaims_its_row_when_absent() {
        // With an error: the message paints one row below the input, and the
        // field consumes an extra row (input 1 + error 1 + blank 1 = 3).
        let mut store = StateStore::new();
        let (buffer, _facts, rows) = render_form(&mut store, Size::new(30, 6), 5, |form| {
            form.field(
                key("email"),
                FieldSpec::new("Email").error(Some("must contain @")),
                &TextInput::new(),
            );
        });
        // Input is at col 5 + 2 = 7; the error line sits at row 1 from that col.
        assert_eq!(cells(&buffer, 1, 7, 30), "must contain @");
        assert_eq!(rows, 3);

        // Without an error: the row is blank and the field is one row shorter.
        let mut store = StateStore::new();
        let (buffer, _facts, rows) = render_form(&mut store, Size::new(30, 6), 5, |form| {
            form.field(key("email"), FieldSpec::new("Email"), &TextInput::new());
        });
        assert_eq!(cells(&buffer, 1, 7, 30), "");
        assert_eq!(rows, 2);
    }

    #[test]
    fn error_and_marker_paint_in_the_danger_role() {
        let theme = Theme::default();
        let mut store = StateStore::new();
        let (buffer, _facts, _rows) = render_form(&mut store, Size::new(30, 6), 5, |form| {
            form.field(
                key("email"),
                FieldSpec::new("Email").required().error(Some("bad")),
                &TextInput::new(),
            );
        });
        // The required marker sits just past the label column (col 5), in Danger.
        assert_eq!(buffer.get(Position::new(5, 0)).unwrap().symbol, "*");
        assert_eq!(
            buffer.get(Position::new(5, 0)).unwrap().style,
            theme.style(Role::Danger)
        );
        // The error text is Danger too.
        assert_eq!(
            buffer.get(Position::new(7, 1)).unwrap().style,
            theme.style(Role::Danger)
        );
    }

    #[test]
    fn focus_order_follows_declaration_order() {
        // Three fields declared top to bottom: the focusable inputs appear in the
        // frame's focus order in exactly that order (labels/errors are not
        // focusable, so they never enter it).
        let mut store = StateStore::new();
        let (_buffer, facts, _rows) = render_form(&mut store, Size::new(30, 12), 6, |form| {
            form.field(
                key("first"),
                FieldSpec::new("First").error(Some("oops")),
                &TextInput::new(),
            );
            form.field(key("second"), FieldSpec::new("Second"), &TextInput::new());
            form.field(
                key("third"),
                FieldSpec::new("Third").required(),
                &TextInput::new(),
            );
        });
        let order: Vec<WidgetId> = facts.focus_order().map(|entry| entry.id).collect();
        assert_eq!(
            order,
            vec![field_id("first"), field_id("second"), field_id("third")]
        );
    }

    #[test]
    fn desired_height_accounting_advances_the_cursor_correctly() {
        // A two-row input (a two-line Text) plus an error line plus a blank row =
        // 4 rows; the next field must land exactly below it.
        let mut store = StateStore::new();
        let (_buffer, facts, rows) = render_form(&mut store, Size::new(30, 12), 5, |form| {
            form.field(
                key("bio"),
                FieldSpec::new("Bio").error(Some("too long")),
                &Text::new("line one\nline two"),
            );
            form.field(key("name"), FieldSpec::new("Name"), &TextInput::new());
        });
        // Field 1: height 2 + error 1 + blank 1 = 4. Field 2 starts at row 4.
        let second = facts.get(field_id("name")).unwrap();
        assert_eq!(second.area.origin.y, 4);
        // Field 2: height 1 + blank 1 = 2. Total = 6.
        assert_eq!(rows, 6);
    }

    #[test]
    fn gap_and_buttons_advance_the_cursor() {
        let mut store = StateStore::new();
        let (_buffer, facts, rows) = render_form(&mut store, Size::new(30, 12), 5, |form| {
            form.field(key("name"), FieldSpec::new("Name"), &TextInput::new());
            form.gap(2);
            form.buttons(|frame, row| frame.widget(key("submit"), row, &Text::new("[ Submit ]")));
        });
        // Field (2 rows) + gap (2) = 4, so the button row is at row 4.
        let submit = facts
            .get(WidgetId::ROOT.child(key("form")).child(key("submit")))
            .unwrap();
        assert_eq!(submit.area.origin.y, 4);
        // The button row is aligned under the input column (label 5 + gap 2 = 7).
        assert_eq!(submit.area.origin.x, 7);
        assert_eq!(rows, 5);
    }

    #[test]
    fn input_is_declared_under_the_field_key() {
        // The app reads the input's outcomes at [form_key, field_key]; the input
        // fact must exist at exactly that composed id.
        let mut store = StateStore::new();
        let (_buffer, facts, _rows) = render_form(&mut store, Size::new(30, 6), 4, |form| {
            form.field(key("name"), FieldSpec::new("Name"), &TextInput::new());
        });
        let input = facts.get(field_id("name")).unwrap();
        assert!(
            input.focusable,
            "the input carries the field's focusability"
        );
        // Input column begins at label_width (4) + LABEL_GAP (2) = 6.
        assert_eq!(input.area.origin.x, 6);
    }
}
