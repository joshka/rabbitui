//! A stateless multi-line text label.

use rabbitui_core::geometry::Position;
use rabbitui_core::style::Style;
use rabbitui_core::theme::Role;
use rabbitui_core::widget::{RenderCtx, Widget};

/// A run of text painted line by line, one row per `'\n'`-separated line.
///
/// `Text` is the simplest conforming widget: stateless (`State = ()`), holding
/// borrowed content and an optional [`Role`] override. It splits its content on
/// `'\n'` and paints each line on its own row from the top of its area; lines and
/// rows past the area are clipped by the [`RenderCtx`], never wrapped (wrapping is
/// layout's job, not this widget's — `docs/adr/0004-layout.md`).
///
/// By default the text paints in the theme's [`Role::Text`] style (ADR 0007:
/// widgets reference roles, not colors). [`role`](Self::role) re-tags it to a
/// different semantic role — [`Role::Muted`] for a hint, [`Role::Danger`] for an
/// error — and the active theme resolves the concrete style. [`style`](Self::style)
/// remains as an escape hatch for a literal [`Style`] when no role fits.
///
/// # Examples
///
/// ```
/// use rabbitui_core::theme::Role;
/// use rabbitui_widgets::Text;
///
/// // A plain label in the theme's text role.
/// let label = Text::new("ready");
///
/// // A muted, multi-line hint.
/// let hint = Text::new("line one\nline two").role(Role::Muted);
/// assert_eq!(hint.content(), "line one\nline two");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Text<'a> {
    content: &'a str,
    style: Appearance,
}

/// How a [`Text`] resolves its paint style: a semantic role (resolved against the
/// active theme) or a literal [`Style`] override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Appearance {
    /// Resolve this role against the theme at render time.
    Role(Role),
    /// Paint exactly this style, ignoring the theme.
    Style(Style),
}

impl<'a> Text<'a> {
    /// Creates a text widget showing `content` in the theme's [`Role::Text`]
    /// style.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::Text;
    ///
    /// let text = Text::new("hello");
    /// assert_eq!(text.content(), "hello");
    /// ```
    #[must_use]
    pub const fn new(content: &'a str) -> Self {
        Self { content, style: Appearance::Role(Role::Text) }
    }

    /// Tags the text with a semantic [`Role`], resolved against the active theme.
    ///
    /// The idiomatic way to style text (ADR 0007): name what it *means* and let
    /// the theme pick the color. Overrides any prior [`role`](Self::role) or
    /// [`style`](Self::style).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::theme::Role;
    /// use rabbitui_widgets::Text;
    ///
    /// let error = Text::new("disk full").role(Role::Danger);
    /// assert_eq!(error.content(), "disk full");
    /// ```
    #[must_use]
    pub const fn role(mut self, role: Role) -> Self {
        self.style = Appearance::Role(role);
        self
    }

    /// Sets a literal [`Style`] applied to every cell, bypassing the theme.
    ///
    /// An escape hatch for a one-off style no role captures; prefer
    /// [`role`](Self::role) so the text tracks theme changes. Overrides any prior
    /// [`role`](Self::role) or `style`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::{Color, Style};
    /// use rabbitui_widgets::Text;
    ///
    /// let warning = Style::new().fg(Color::YELLOW).bold();
    /// let text = Text::new("warning").style(warning);
    /// assert_eq!(text.get_style(), Some(warning));
    /// ```
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = Appearance::Style(style);
        self
    }

    /// The text content this widget shows.
    #[must_use]
    pub const fn content(&self) -> &'a str {
        self.content
    }

    /// The literal style override, if one was set with [`style`](Self::style),
    /// or `None` if the text resolves through a [`Role`].
    ///
    /// Named `get_style` because [`style`](Self::style) is the builder setter.
    #[must_use]
    pub const fn get_style(&self) -> Option<Style> {
        match self.style {
            Appearance::Style(style) => Some(style),
            Appearance::Role(_) => None,
        }
    }
}

impl Widget for Text<'_> {
    type State = ();

    fn render(&self, (): &mut (), ctx: &mut RenderCtx<'_>) {
        let style = match self.style {
            Appearance::Role(role) => ctx.style(role),
            Appearance::Style(style) => style,
        };
        for (row, line) in self.content.split('\n').enumerate() {
            let Ok(y) = u16::try_from(row) else { break };
            // Rows past the area's bottom are no-ops; stop once we're below it.
            if y >= ctx.area().size.height {
                break;
            }
            ctx.set_string(Position::new(0, y), line, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::style::{Color, Style};
    use rabbitui_core::widget::{RenderCtx, Widget};

    use super::Text;

    /// Reads a row of a buffer back as a trailing-trimmed string.
    fn row(buffer: &Buffer, y: u16) -> String {
        let mut line = String::new();
        for x in 0..buffer.size().width {
            line.push_str(&buffer.get(Position::new(x, y)).unwrap().symbol);
        }
        line.trim_end().to_string()
    }

    #[test]
    fn builder_sets_content_and_style() {
        let text = Text::new("hi").style(Style::new().fg(Color::RED).bold());
        assert_eq!(text.content(), "hi");
        assert_eq!(text.get_style().unwrap().fg, Some(Color::RED));
    }

    #[test]
    fn new_defaults_to_the_text_role_style() {
        use rabbitui_core::theme::{Role, Theme};
        // A default Text has no literal override; it resolves Role::Text.
        assert_eq!(Text::new("x").get_style(), None);
        let mut buffer = Buffer::new(Size::new(2, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(2, 1)), false);
        Text::new("x").render(&mut (), &mut ctx);
        assert_eq!(
            buffer.get(Position::ORIGIN).unwrap().style,
            Theme::default().style(Role::Text),
        );
    }

    #[test]
    fn role_resolves_against_the_active_theme() {
        use rabbitui_core::theme::{Role, Theme};
        let theme = Theme::catppuccin_mocha();
        let mut buffer = Buffer::new(Size::new(3, 1));
        let mut ctx =
            RenderCtx::new_themed(&mut buffer, Rect::from_size(Size::new(3, 1)), false, &theme);
        Text::new("hi").role(Role::Danger).render(&mut (), &mut ctx);
        assert_eq!(buffer.get(Position::ORIGIN).unwrap().style, theme.style(Role::Danger));
    }

    #[test]
    fn renders_a_single_line_from_the_origin() {
        let mut buffer = Buffer::new(Size::new(10, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(10, 1)), false);
        Text::new("hello").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "hello");
    }

    #[test]
    fn splits_on_newline_one_row_per_line() {
        let mut buffer = Buffer::new(Size::new(10, 3));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(10, 3)), false);
        Text::new("one\ntwo\nthree").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "one");
        assert_eq!(row(&buffer, 1), "two");
        assert_eq!(row(&buffer, 2), "three");
    }

    #[test]
    fn lines_past_the_bottom_are_clipped() {
        // Two rows of area, three lines of content: the third is dropped.
        let mut buffer = Buffer::new(Size::new(10, 2));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(10, 2)), false);
        Text::new("one\ntwo\nthree").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "one");
        assert_eq!(row(&buffer, 1), "two");
    }

    #[test]
    fn long_lines_clip_at_the_right_edge() {
        let mut buffer = Buffer::new(Size::new(3, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(3, 1)), false);
        Text::new("abcdef").render(&mut (), &mut ctx);
        // The ctx clips to the 3-wide area; the rest is dropped, not wrapped.
        assert_eq!(row(&buffer, 0), "abc");
    }

    #[test]
    fn style_applies_to_every_painted_cell() {
        let mut buffer = Buffer::new(Size::new(5, 1));
        let style = Style::new().fg(Color::GREEN);
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(5, 1)), false);
        Text::new("ab").style(style).render(&mut (), &mut ctx);
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().style, style);
        assert_eq!(buffer.get(Position::new(1, 0)).unwrap().style, style);
    }

    #[test]
    fn empty_content_paints_one_blank_line() {
        // "" splits into a single empty line; nothing is painted, no panic.
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(4, 1)), false);
        Text::new("").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "");
    }
}
