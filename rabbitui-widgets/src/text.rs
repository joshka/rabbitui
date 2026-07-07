//! A stateless multi-line text label.

use rabbitui_core::geometry::Position;
use rabbitui_core::style::Style;
use rabbitui_core::widget::{RenderCtx, Widget};

/// A run of text painted line by line, one row per `'\n'`-separated line.
///
/// `Text` is the simplest conforming widget: stateless (`State = ()`), holding
/// borrowed content and a [`Style`]. It splits its content on `'\n'` and paints
/// each line on its own row from the top of its area; lines and rows past the
/// area are clipped by the [`RenderCtx`], never wrapped (wrapping is layout's
/// job, not this widget's — `docs/adr/0004-layout.md`).
///
/// # Examples
///
/// ```
/// use rabbitui_core::style::{Color, Style};
/// use rabbitui_widgets::Text;
///
/// // A plain label.
/// let label = Text::new("ready");
///
/// // A styled, multi-line label built with the builder.
/// let banner = Text::new("line one\nline two").style(Style::new().fg(Color::GREEN).bold());
/// assert_eq!(banner.content(), "line one\nline two");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Text<'a> {
    content: &'a str,
    style: Style,
}

impl<'a> Text<'a> {
    /// Creates a text widget showing `content` in the default style.
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
        Self { content, style: Style::new() }
    }

    /// Sets the style applied to every cell of the text.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::{Color, Style};
    /// use rabbitui_widgets::Text;
    ///
    /// let warning = Style::new().fg(Color::YELLOW).bold();
    /// let text = Text::new("warning").style(warning);
    /// assert_eq!(text.get_style(), warning);
    /// ```
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The text content this widget shows.
    #[must_use]
    pub const fn content(&self) -> &'a str {
        self.content
    }

    /// The style applied to the text.
    ///
    /// Named `get_style` because [`style`](Self::style) is the builder setter,
    /// following the same setter/getter split as [`Style`] itself (`fg` the
    /// field, `fg(..)` the builder).
    #[must_use]
    pub const fn get_style(&self) -> Style {
        self.style
    }
}

impl Widget for Text<'_> {
    type State = ();

    fn render(&self, (): &mut (), ctx: &mut RenderCtx<'_>) {
        for (row, line) in self.content.split('\n').enumerate() {
            let Ok(y) = u16::try_from(row) else { break };
            // Rows past the area's bottom are no-ops; stop once we're below it.
            if y >= ctx.area().size.height {
                break;
            }
            ctx.set_string(Position::new(0, y), line, self.style);
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
        assert_eq!(text.get_style().fg, Some(Color::RED));
    }

    #[test]
    fn renders_a_single_line_from_the_origin() {
        let mut buffer = Buffer::new(Size::new(10, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(10, 1)));
        Text::new("hello").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "hello");
    }

    #[test]
    fn splits_on_newline_one_row_per_line() {
        let mut buffer = Buffer::new(Size::new(10, 3));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(10, 3)));
        Text::new("one\ntwo\nthree").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "one");
        assert_eq!(row(&buffer, 1), "two");
        assert_eq!(row(&buffer, 2), "three");
    }

    #[test]
    fn lines_past_the_bottom_are_clipped() {
        // Two rows of area, three lines of content: the third is dropped.
        let mut buffer = Buffer::new(Size::new(10, 2));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(10, 2)));
        Text::new("one\ntwo\nthree").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "one");
        assert_eq!(row(&buffer, 1), "two");
    }

    #[test]
    fn long_lines_clip_at_the_right_edge() {
        let mut buffer = Buffer::new(Size::new(3, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(3, 1)));
        Text::new("abcdef").render(&mut (), &mut ctx);
        // The ctx clips to the 3-wide area; the rest is dropped, not wrapped.
        assert_eq!(row(&buffer, 0), "abc");
    }

    #[test]
    fn style_applies_to_every_painted_cell() {
        let mut buffer = Buffer::new(Size::new(5, 1));
        let style = Style::new().fg(Color::GREEN);
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(5, 1)));
        Text::new("ab").style(style).render(&mut (), &mut ctx);
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().style, style);
        assert_eq!(buffer.get(Position::new(1, 0)).unwrap().style, style);
    }

    #[test]
    fn empty_content_paints_one_blank_line() {
        // "" splits into a single empty line; nothing is painted, no panic.
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(4, 1)));
        Text::new("").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "");
    }
}
