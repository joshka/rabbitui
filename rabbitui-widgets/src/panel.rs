//! A container-look backdrop: a filled surface with an optional border, title,
//! and inner padding.
//!
//! `Panel` is the visual primitive apps reach for to make a view look like a
//! *designed application* rather than text flush against the top-left corner: it
//! fills its area with a background, optionally frames it with a light
//! box-drawing border, writes a title into the top border, and reserves uniform
//! inner padding for content.
//!
//! # The pre-composition (backdrop) pattern
//!
//! rabbitui widgets cannot nest yet — a widget has no children (the catalog arc
//! adds composition; `docs/design/middle-piece-audit.md` gap #4). So `Panel` is
//! a **backdrop**, not a container: it paints the frame and *nothing inside it*.
//! An app declares the panel first, computes the content region itself with
//! [`Panel::inner`], and then declares its contents into that inner area:
//!
//! ```
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::geometry::{Rect, Size};
//! use rabbitui_core::id::key;
//! # use rabbitui_core::store::StateStore;
//! use rabbitui_widgets::{Panel, Text};
//!
//! # let mut buffer = rabbitui_core::buffer::Buffer::new(Size::new(30, 8));
//! # let mut store = StateStore::new();
//! # store.begin_frame();
//! # let mut frame = Frame::new(&mut buffer, &mut store);
//! let area = Rect::from_size(Size::new(30, 8));
//! let panel = Panel::new().title("Form").padding(1);
//! // 1. declare the backdrop into the outer area…
//! frame.widget(key("panel"), area, &panel);
//! // 2. …then compute the inner area and declare contents into it.
//! let inner = Panel::inner(area, &panel);
//! frame.widget(key("body"), inner, &Text::new("content"));
//! # let _ = frame.finish();
//! # store.end_frame();
//! ```
//!
//! Because [`inner`](Panel::inner) is a pure function of the outer area and the
//! panel's own configuration (border on/off, padding), the app can call it
//! without re-deriving the arithmetic. When the catalog grows real children this
//! same panel becomes a container and the manual `inner` step disappears — the
//! backdrop pattern is the honest interim, and its one friction (the caller owns
//! the two-step declare) is exactly the pressure that motivates composition.
//!
//! # Styling
//!
//! Every surface references a [`Role`], never a raw color (ADR 0007): the fill is
//! [`Role::Surface`] by default, the border [`Role::Border`], and a *focused*
//! panel's border switches to [`Role::Accent`] so focus reads at the
//! container level. The title paints in the border's style. Swap the theme and
//! the whole panel re-skins.
//!
//! `Panel` is stateless (`State = ()`) and never focusable — it is chrome. To
//! show a panel as focused (because the widget *inside* it holds focus), the app
//! passes [`focused(true)`](Panel::focused); the panel does not read framework
//! focus itself, since it declares no focusable and would never be the focus
//! target.

use rabbitui_core::geometry::{Position, Rect, Size};
use rabbitui_core::layout::inset;
use rabbitui_core::style::Style;
use rabbitui_core::theme::Role;
use rabbitui_core::widget::{RenderCtx, Widget};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The light box-drawing set `Panel` frames with.
mod box_chars {
    pub const TOP_LEFT: &str = "┌";
    pub const TOP_RIGHT: &str = "┐";
    pub const BOTTOM_LEFT: &str = "└";
    pub const BOTTOM_RIGHT: &str = "┘";
    pub const HORIZONTAL: &str = "─";
    pub const VERTICAL: &str = "│";
}

/// A backdrop widget: a filled surface with an optional border, title, and inner
/// padding, meant to sit *behind* content declared into its [`inner`](Self::inner)
/// area.
///
/// See the module docs for the pre-composition pattern this widget serves.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::Panel;
///
/// // A bordered, titled panel with one cell of inner padding.
/// let panel = Panel::new().title("Details").padding(1);
/// assert_eq!(panel.get_title(), Some("Details"));
/// assert!(panel.has_border());
/// assert_eq!(panel.get_padding(), 1);
///
/// // A borderless wash of surface color — a subtle backdrop, no frame.
/// let backdrop = Panel::new().border(false);
/// assert!(!backdrop.has_border());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panel<'a> {
    /// The optional title drawn into the top border.
    title: Option<&'a str>,
    /// Whether to draw the box-drawing border.
    border: bool,
    /// Uniform inner padding, in cells, inside the border.
    padding: u16,
    /// The role the background fill resolves against.
    fill: Role,
    /// Whether to paint the border with the focused-panel highlight role.
    focused: bool,
}

impl<'a> Panel<'a> {
    /// A default panel: a [`Role::Surface`] fill, a border, no title, no padding.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::Panel;
    ///
    /// let panel = Panel::new();
    /// assert!(panel.has_border());
    /// assert_eq!(panel.get_title(), None);
    /// assert_eq!(panel.get_padding(), 0);
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self {
            title: None,
            border: true,
            padding: 0,
            fill: Role::Surface,
            focused: false,
        }
    }

    /// Sets the title drawn into the top border.
    ///
    /// The title is written after the top-left corner, in the border's style,
    /// clipped to the available top-border width. It is only visible when the
    /// border is on.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Enables or disables the box-drawing border (on by default).
    ///
    /// With the border off the panel is a bare wash of its [`fill`](Self::fill)
    /// role; [`inner`](Self::inner) then only subtracts padding, not a frame.
    #[must_use]
    pub const fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Sets the uniform inner padding, in cells, reserved inside the border.
    ///
    /// Padding shrinks the [`inner`](Self::inner) area on every side. It is
    /// applied inside the border (or directly inside the area, when borderless).
    #[must_use]
    pub const fn padding(mut self, padding: u16) -> Self {
        self.padding = padding;
        self
    }

    /// Sets the [`Role`] the background fill resolves against ([`Role::Surface`]
    /// by default).
    ///
    /// An escape hatch for a panel that should read as a distinct surface (a
    /// sidebar, a highlighted region); most panels keep the default.
    #[must_use]
    pub const fn fill(mut self, fill: Role) -> Self {
        self.fill = fill;
        self
    }

    /// Marks the panel as focused, painting its border in [`Role::Accent`].
    ///
    /// The app passes `true` when the content *inside* the panel holds focus, so
    /// the container reads as active. A panel never takes focus itself (it is
    /// chrome, `focusable(false)`), so it cannot read framework focus and relies
    /// on this flag instead.
    #[must_use]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// The title, if one was set.
    #[must_use]
    pub const fn get_title(&self) -> Option<&'a str> {
        self.title
    }

    /// Whether the border is drawn.
    #[must_use]
    pub const fn has_border(&self) -> bool {
        self.border
    }

    /// The uniform inner padding.
    #[must_use]
    pub const fn get_padding(&self) -> u16 {
        self.padding
    }

    /// Whether the panel paints its border in the focused-highlight role.
    #[must_use]
    pub const fn is_focused(&self) -> bool {
        self.focused
    }

    /// The content area inside `area` for this panel: `area` minus the border (a
    /// one-cell frame when [`has_border`](Self::has_border)) and minus the
    /// panel's uniform padding.
    ///
    /// This is the pre-composition seam (see the module docs): the app declares
    /// the panel into `area`, then declares its contents into `Panel::inner(area,
    /// &panel)`. The result is clamped — a panel too small for its border and
    /// padding yields an empty inner area rather than underflowing.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::geometry::{Position, Rect, Size};
    /// use rabbitui_widgets::Panel;
    ///
    /// let area = Rect::from_size(Size::new(20, 10));
    ///
    /// // Bordered, no padding: inner loses one cell on every side.
    /// let inner = Panel::inner(area, &Panel::new());
    /// assert_eq!(inner.origin, Position::new(1, 1));
    /// assert_eq!(inner.size, Size::new(18, 8));
    ///
    /// // Bordered + padding 1: inner loses two cells on every side.
    /// let inner = Panel::inner(area, &Panel::new().padding(1));
    /// assert_eq!(inner.origin, Position::new(2, 2));
    /// assert_eq!(inner.size, Size::new(16, 6));
    ///
    /// // Borderless: only padding is subtracted.
    /// let inner = Panel::inner(area, &Panel::new().border(false).padding(2));
    /// assert_eq!(inner.origin, Position::new(2, 2));
    /// assert_eq!(inner.size, Size::new(16, 6));
    /// ```
    #[must_use]
    pub fn inner(area: Rect, panel: &Panel<'_>) -> Rect {
        let bordered = if panel.border { inset(area, 1) } else { area };
        inset(bordered, panel.padding)
    }
}

impl Default for Panel<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Panel<'_> {
    type State = ();

    fn desired_height(&self, (): &(), _width: u16) -> u16 {
        // A panel is a backdrop with no measurable content of its own (the caller
        // declares content into its `inner` area). Its intrinsic height is the
        // chrome overhead it needs to read as an empty frame: the border (two rows
        // when bordered) plus padding on both edges, at least one row.
        let border: u16 = if self.border { 2 } else { 0 };
        let padding = self.padding.saturating_mul(2);
        border.saturating_add(padding).max(1)
    }

    fn render(&self, (): &mut (), ctx: &mut RenderCtx<'_>) {
        // A panel is chrome: never a focus target.
        ctx.focusable(false);

        let size = ctx.size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        // 1. Fill the whole area with the surface background, row by row.
        let fill_style = ctx.style(self.fill);
        let blank = " ".repeat(usize::from(size.width));
        for y in 0..size.height {
            ctx.set_string(Position::new(0, y), &blank, fill_style);
        }

        if !self.border {
            return;
        }

        // 2. Draw the border. A focused panel highlights its frame.
        let border_style = if self.focused {
            // Accent, not Highlight: Highlight carries a background, which
            // paints the whole border as a thick colored band (user report).
            ctx.style(Role::Accent)
        } else {
            ctx.style(Role::Border)
        };
        draw_border(ctx, size, border_style);

        // 3. Draw the title into the top border, after the top-left corner.
        if let Some(title) = self.title {
            draw_title(ctx, size, title, border_style);
        }
    }
}

/// Paints the box-drawing frame around a `size`-cell area in `style`.
fn draw_border(ctx: &mut RenderCtx<'_>, size: Size, style: Style) {
    use box_chars::{
        BOTTOM_LEFT, BOTTOM_RIGHT, HORIZONTAL, TOP_LEFT, TOP_RIGHT, VERTICAL,
    };
    let last_x = size.width - 1;
    let last_y = size.height - 1;

    // A degenerate one-cell-thin panel has no room for a full box; fall back to a
    // horizontal or vertical run so it still reads as chrome, not garbage.
    if size.height == 1 {
        let run = HORIZONTAL.repeat(usize::from(size.width));
        ctx.set_string(Position::ORIGIN, &run, style);
        return;
    }
    if size.width == 1 {
        for y in 0..size.height {
            ctx.set_string(Position::new(0, y), VERTICAL, style);
        }
        return;
    }

    // Corners.
    ctx.set_string(Position::new(0, 0), TOP_LEFT, style);
    ctx.set_string(Position::new(last_x, 0), TOP_RIGHT, style);
    ctx.set_string(Position::new(0, last_y), BOTTOM_LEFT, style);
    ctx.set_string(Position::new(last_x, last_y), BOTTOM_RIGHT, style);

    // Top and bottom edges between the corners.
    let horizontal = HORIZONTAL.repeat(usize::from(size.width - 2));
    ctx.set_string(Position::new(1, 0), &horizontal, style);
    ctx.set_string(Position::new(1, last_y), &horizontal, style);

    // Left and right edges between the corners.
    for y in 1..last_y {
        ctx.set_string(Position::new(0, y), VERTICAL, style);
        ctx.set_string(Position::new(last_x, y), VERTICAL, style);
    }
}

/// Writes `title` into the top border, framed by a space on each side, clipped
/// to the room between the corners.
fn draw_title(ctx: &mut RenderCtx<'_>, size: Size, title: &str, style: Style) {
    // The room between the two corners: width - 2. Nothing fits below 3 wide.
    if size.width < 3 {
        return;
    }
    let available = usize::from(size.width - 2);
    // Frame the title with spaces so it doesn't touch the corners; if there is no
    // room for the framing, fall back to the bare title.
    let framed = format!(" {title} ");
    let text = if UnicodeWidthStr::width(framed.as_str()) <= available {
        framed
    } else {
        title.to_string()
    };
    // Truncate to the run between the corners so a long title never clobbers the
    // top-right corner: `set_string` only clips at the buffer/area edge, which
    // would land on the corner cell.
    let clipped = truncate_to_width(&text, available);
    ctx.set_string(Position::new(1, 0), clipped, style);
}

/// Returns the longest prefix of `text` whose display width does not exceed
/// `max`, split on grapheme boundaries so a wide grapheme never straddles the
/// limit.
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

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::theme::{Role, Theme};
    use rabbitui_core::widget::{RenderCtx, Widget};

    use super::Panel;

    /// Renders a panel into a fresh `size` buffer against `theme`.
    fn render(panel: &Panel<'_>, size: Size, theme: &Theme) -> Buffer {
        let mut buffer = Buffer::new(size);
        let mut ctx =
            RenderCtx::new_themed(&mut buffer, Rect::from_size(size), false, theme);
        panel.render(&mut (), &mut ctx);
        buffer
    }

    /// Reads a row back as a trailing-trimmed string.
    fn row(buffer: &Buffer, y: u16) -> String {
        let mut line = String::new();
        for x in 0..buffer.size().width {
            line.push_str(&buffer.get(Position::new(x, y)).unwrap().symbol);
        }
        line.trim_end().to_string()
    }

    #[test]
    fn builder_records_configuration() {
        let panel = Panel::new()
            .title("T")
            .padding(2)
            .border(true)
            .fill(Role::Accent)
            .focused(true);
        assert_eq!(panel.get_title(), Some("T"));
        assert_eq!(panel.get_padding(), 2);
        assert!(panel.has_border());
        assert!(panel.is_focused());
    }

    #[test]
    fn default_is_a_bordered_untitled_surface_panel() {
        let panel = Panel::default();
        assert!(panel.has_border());
        assert_eq!(panel.get_title(), None);
        assert_eq!(panel.get_padding(), 0);
    }

    #[test]
    fn inner_subtracts_border_and_padding() {
        let area = Rect::from_size(Size::new(20, 10));
        // Bordered, no padding.
        let inner = Panel::inner(area, &Panel::new());
        assert_eq!(inner.origin, Position::new(1, 1));
        assert_eq!(inner.size, Size::new(18, 8));
        // Bordered + padding 1.
        let inner = Panel::inner(area, &Panel::new().padding(1));
        assert_eq!(inner.origin, Position::new(2, 2));
        assert_eq!(inner.size, Size::new(16, 6));
        // Borderless + padding 2.
        let inner = Panel::inner(area, &Panel::new().border(false).padding(2));
        assert_eq!(inner.origin, Position::new(2, 2));
        assert_eq!(inner.size, Size::new(16, 6));
    }

    #[test]
    fn desired_height_is_the_chrome_overhead() {
        use rabbitui_core::widget::Widget;
        // Bordered, no padding: two border rows.
        assert_eq!(Panel::new().desired_height(&(), 20), 2);
        // Bordered + padding 1: two border rows + two padding rows.
        assert_eq!(Panel::new().padding(1).desired_height(&(), 20), 4);
        // Borderless, no padding: at least one row.
        assert_eq!(Panel::new().border(false).desired_height(&(), 20), 1);
        // Borderless + padding 2: four padding rows.
        assert_eq!(Panel::new().border(false).padding(2).desired_height(&(), 20), 4);
    }

    #[test]
    fn inner_clamps_when_too_small() {
        // A 2×2 area cannot even hold its border's inner region.
        let inner = Panel::inner(Rect::from_size(Size::new(2, 2)), &Panel::new().padding(1));
        assert_eq!(inner.size, Size::new(0, 0));
    }

    #[test]
    fn renders_a_full_border_box() {
        let buffer = render(&Panel::new(), Size::new(6, 3), &Theme::default());
        assert_eq!(row(&buffer, 0), "┌────┐");
        assert_eq!(row(&buffer, 1), "│    │");
        assert_eq!(row(&buffer, 2), "└────┘");
    }

    #[test]
    fn renders_a_title_in_the_top_border() {
        let buffer = render(&Panel::new().title("Hi"), Size::new(10, 3), &Theme::default());
        assert_eq!(row(&buffer, 0), "┌ Hi ────┐");
        assert_eq!(row(&buffer, 2), "└────────┘");
    }

    #[test]
    fn a_long_title_clips_to_the_top_border() {
        // Title wider than the run: framing is dropped, then it clips to width-2.
        let buffer = render(
            &Panel::new().title("a very long title"),
            Size::new(8, 3),
            &Theme::default(),
        );
        // The corners survive; the title fills the six-cell run between them.
        let top = row(&buffer, 0);
        assert!(top.starts_with('┌'));
        assert!(top.ends_with('┐'));
    }

    #[test]
    fn border_off_fills_but_draws_no_frame() {
        let buffer = render(&Panel::new().border(false), Size::new(4, 2), &Theme::default());
        // No box-drawing glyphs anywhere.
        for y in 0..2 {
            let line = row(&buffer, y);
            assert!(!line.contains('┌') && !line.contains('│') && !line.contains('─'));
        }
    }

    #[test]
    fn fill_paints_the_surface_background_on_every_cell() {
        let theme = Theme::catppuccin_mocha();
        let buffer = render(&Panel::new().border(false), Size::new(3, 2), &theme);
        let surface = theme.style(Role::Surface);
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(buffer.get(Position::new(x, y)).unwrap().style, surface);
            }
        }
    }

    #[test]
    fn focused_border_uses_the_highlight_role() {
        let theme = Theme::catppuccin_mocha();
        let buffer = render(&Panel::new().focused(true), Size::new(4, 3), &theme);
        // The top-left corner glyph carries the highlight style, not the border.
        let corner = buffer.get(Position::ORIGIN).unwrap();
        assert_eq!(corner.symbol, "┌");
        assert_eq!(corner.style, theme.style(Role::Accent));
    }

    #[test]
    fn unfocused_border_uses_the_border_role() {
        let theme = Theme::catppuccin_mocha();
        let buffer = render(&Panel::new(), Size::new(4, 3), &theme);
        let corner = buffer.get(Position::ORIGIN).unwrap();
        assert_eq!(corner.style, theme.style(Role::Border));
    }

    #[test]
    fn one_row_panel_draws_a_horizontal_run() {
        let buffer = render(&Panel::new(), Size::new(5, 1), &Theme::default());
        assert_eq!(row(&buffer, 0), "─────");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        // A zero-width area renders nothing and does not panic.
        let buffer = render(&Panel::new(), Size::new(0, 3), &Theme::default());
        assert_eq!(buffer.size(), Size::new(0, 3));
    }

    /// A themed snapshot: a titled, padded, focused panel rendered against the
    /// Catppuccin Mocha theme, checked as an exact row-by-row picture.
    #[test]
    fn themed_snapshot() {
        let panel = Panel::new().title("Settings").padding(1).focused(true);
        let buffer = render(&panel, Size::new(16, 6), &Theme::catppuccin_mocha());
        assert_eq!(row(&buffer, 0), "┌ Settings ────┐");
        assert_eq!(row(&buffer, 1), "│              │");
        assert_eq!(row(&buffer, 2), "│              │");
        assert_eq!(row(&buffer, 3), "│              │");
        assert_eq!(row(&buffer, 4), "│              │");
        assert_eq!(row(&buffer, 5), "└──────────────┘");
        // The frame is highlighted; the fill is the surface; both come from theme
        // roles, so a theme swap re-skins the whole panel.
        let theme = Theme::catppuccin_mocha();
        assert_eq!(
            buffer.get(Position::ORIGIN).unwrap().style,
            theme.style(Role::Accent)
        );
        assert_eq!(
            buffer.get(Position::new(1, 1)).unwrap().style,
            theme.style(Role::Surface)
        );
    }
}
