//! A stateless multi-line text label, plain or styled-span.

use std::borrow::Cow;

use rabbitui_core::accessibility::SemanticRole;
use rabbitui_core::geometry::Position;
use rabbitui_core::style::Style;
use rabbitui_core::text::Span;
use rabbitui_core::theme::Role;
use rabbitui_core::widget::{RenderContext, Widget};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// What a [`Text`] shows: either a plain string or a sequence of styled
/// [`Span`]s.
///
/// `Content` is what lets one `Text` widget serve both the plain label
/// (`Text::new("ready")`) and the flagship's multi-style live tail (bold
/// headings, dim code, an accent status), where slice 8 had to fall back to
/// monochrome because the widget-side `Text` could not carry spans. Both arms
/// hold a [`Cow`] so a caller can borrow existing data (a `&str`, a committed
/// `&[Span]`) or hand over owned content without the widget forcing a clone.
///
/// The two arms differ only in whether the text carries per-run styling; both
/// flow through the *same* grapheme+style iterator for paint and wrap (see
/// [`Text`]'s soft-wrap docs), so styled text wraps exactly like plain text —
/// the fix for "styling pops at commit" in the flagship.
///
/// # Examples
///
/// ```
/// use rabbitui_core::style::{Color, Style};
/// use rabbitui_core::text::Span;
/// use rabbitui_widgets::text::Content;
///
/// // A plain string is `Plain` content.
/// let plain: Content<'_> = "hello".into();
/// assert_eq!(plain.to_plain_string(), "hello");
///
/// // A vector of spans is `Spans` content.
/// let spans: Content<'_> =
///     vec![Span::styled("ok", Style::new().fg(Color::GREEN))].into();
/// assert_eq!(spans.to_plain_string(), "ok");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content<'a> {
    /// A single unstyled run: the widget's resolved [`Role`]/[`Style`] paints all
    /// of it.
    Plain(Cow<'a, str>),
    /// A sequence of styled runs: each [`Span`]'s style layers over the widget's
    /// resolved default (an empty span style resolves to exactly the default).
    Spans(Cow<'a, [Span]>),
}

impl Content<'_> {
    /// The content's text with span styling flattened away — the concatenation of
    /// every run's text, `'\n'`s preserved.
    ///
    /// Useful for measuring, testing, and the plain-text fallback; the paint path
    /// uses the styled iterator instead.
    #[must_use]
    pub fn to_plain_string(&self) -> String {
        match self {
            Content::Plain(text) => text.as_ref().to_string(),
            Content::Spans(spans) => spans.iter().map(|span| span.text.as_str()).collect(),
        }
    }
}

impl<'a> From<&'a str> for Content<'a> {
    fn from(text: &'a str) -> Self {
        Content::Plain(Cow::Borrowed(text))
    }
}

impl From<String> for Content<'_> {
    fn from(text: String) -> Self {
        Content::Plain(Cow::Owned(text))
    }
}

impl<'a> From<&'a String> for Content<'a> {
    fn from(text: &'a String) -> Self {
        Content::Plain(Cow::Borrowed(text.as_str()))
    }
}

impl<'a> From<Cow<'a, str>> for Content<'a> {
    fn from(text: Cow<'a, str>) -> Self {
        Content::Plain(text)
    }
}

impl From<Vec<Span>> for Content<'_> {
    fn from(spans: Vec<Span>) -> Self {
        Content::Spans(Cow::Owned(spans))
    }
}

impl<'a> From<&'a [Span]> for Content<'a> {
    fn from(spans: &'a [Span]) -> Self {
        Content::Spans(Cow::Borrowed(spans))
    }
}

impl From<Span> for Content<'_> {
    fn from(span: Span) -> Self {
        Content::Spans(Cow::Owned(vec![span]))
    }
}

/// A run of text painted line by line, one row per `'\n'`-separated line.
///
/// `Text` is the simplest conforming widget: stateless (`State = ()`), holding
/// borrowed or owned [`Content`] and an optional [`Role`] override. It splits its
/// content on `'\n'` and paints each line on its own row from the top of its
/// area; lines and rows past the area are clipped by the [`RenderContext`], never
/// wrapped unless [`wrap`](Self::wrap) is on.
///
/// # Plain and styled content
///
/// `Text::new` accepts anything convertible into [`Content`]: a `&str`/`String`
/// (plain) or a `Vec<Span>`/`&[Span]` (styled). Plain content paints entirely in
/// the widget's resolved style; styled content layers each span's style over that
/// resolved default, so a span carrying only `bold` keeps the role's color and
/// adds bold, and an empty span style paints as the plain default. Paint and wrap
/// share **one** grapheme+style iterator, so styled text wraps identically to
/// plain text — the flagship's monochrome live tail and "styling pops at commit"
/// strain both close here.
///
/// By default the text paints in the theme's [`Role::Text`] style (ADR 0007:
/// widgets reference roles, not colors). [`role`](Self::role) re-tags it to a
/// different semantic role — [`Role::Muted`] for a hint, [`Role::Danger`] for an
/// error — and the active theme resolves the concrete style. [`style`](Self::style)
/// remains as an escape hatch for a literal [`Style`] when no role fits.
///
/// # Soft wrap
///
/// By default long lines clip at the right edge (wrapping is layout's job, ADR
/// 0004). [`wrap(true)`](Self::wrap) opts a `Text` into **grapheme-correct soft
/// wrap** to its area width: each `'\n'`-separated line is broken into as many
/// display rows as it needs, preferring whitespace boundaries and falling back
/// to a grapheme break for a word longer than the area. Wrapping uses the same
/// width oracle the buffer uses, so a wide (CJK/emoji) grapheme is never split
/// across the boundary — including at a span boundary in styled content.
///
/// # Measurement
///
/// [`desired_height`](Widget::desired_height) reports the line count: the number
/// of `'\n'`-separated lines when unwrapped, or the total wrapped-row count at the
/// given width when [`wrap`](Self::wrap) is on. A scroll container stacks and
/// virtualizes on this.
///
/// # Examples
///
/// ```
/// use rabbitui_core::style::{Color, Style};
/// use rabbitui_core::text::Span;
/// use rabbitui_core::theme::Role;
/// use rabbitui_widgets::Text;
///
/// // A plain label in the theme's text role.
/// let label = Text::new("ready");
///
/// // A muted, multi-line hint.
/// let hint = Text::new("line one\nline two").role(Role::Muted);
/// assert_eq!(hint.content().to_plain_string(), "line one\nline two");
///
/// // A styled-span line: a green "ok" then a plain " done".
/// let status = Text::new(vec![
///     Span::styled("ok", Style::new().fg(Color::GREEN).bold()),
///     Span::raw(" done"),
/// ]);
///
/// // A soft-wrapped paragraph.
/// let para = Text::new("a long paragraph that wraps").wrap(true);
/// assert!(para.is_wrapped());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text<'a> {
    content: Content<'a>,
    style: Appearance,
    /// Whether long lines soft-wrap to the area width (grapheme-correct) rather
    /// than clip at the right edge.
    wrap: bool,
    /// When the content is taller than the area, whether to keep the *last* rows
    /// visible (anchor to the bottom) rather than the first. Off by default, so
    /// overflow clips at the bottom as usual; on, it clips at the top — a live
    /// tail view, e.g. a streaming preview that should always show newest text.
    anchor_bottom: bool,
}

/// How a [`Text`] resolves its default paint style: a semantic role (resolved
/// against the active theme) or a literal [`Style`] override. Styled spans layer
/// over whichever this resolves to.
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
    /// `content` is anything convertible into [`Content`]: a `&str`/`String` for a
    /// plain label, or a `Vec<Span>`/`&[Span]` for styled runs. `Text::new("str")`
    /// stays source-compatible with the plain constructor.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::Text;
    ///
    /// let text = Text::new("hello");
    /// assert_eq!(text.content().to_plain_string(), "hello");
    /// ```
    #[must_use]
    pub fn new(content: impl Into<Content<'a>>) -> Self {
        Self {
            content: content.into(),
            style: Appearance::Role(Role::Text),
            wrap: false,
            anchor_bottom: false,
        }
    }

    /// Tags the text with a semantic [`Role`], resolved against the active theme.
    ///
    /// The idiomatic way to style text (ADR 0007): name what it *means* and let
    /// the theme pick the color. For styled-span content this sets the *default*
    /// each span layers over. Overrides any prior [`role`](Self::role) or
    /// [`style`](Self::style).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::theme::Role;
    /// use rabbitui_widgets::Text;
    ///
    /// let error = Text::new("disk full").role(Role::Danger);
    /// assert_eq!(error.content().to_plain_string(), "disk full");
    /// ```
    #[must_use]
    pub const fn role(mut self, role: Role) -> Self {
        self.style = Appearance::Role(role);
        self
    }

    /// Sets a literal [`Style`] applied as the default, bypassing the theme.
    ///
    /// An escape hatch for a one-off style no role captures; prefer
    /// [`role`](Self::role) so the text tracks theme changes. For styled-span
    /// content this is the base each span layers over. Overrides any prior
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

    /// Enables or disables grapheme-correct soft wrap to the area width.
    ///
    /// With `wrap` off (the default) long lines clip at the right edge; with it
    /// on, each line is broken into as many rows as it needs, preferring
    /// whitespace and never splitting a wide grapheme across the boundary. See
    /// the type docs for the wrap contract.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::Text;
    ///
    /// let wrapped = Text::new("wrap me").wrap(true);
    /// assert!(wrapped.is_wrapped());
    /// ```
    #[must_use]
    pub const fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Whether soft wrap is enabled (see [`wrap`](Self::wrap)).
    #[must_use]
    pub const fn is_wrapped(&self) -> bool {
        self.wrap
    }

    /// Anchors overflow to the bottom: when the content is taller than the area,
    /// the *last* rows stay visible and the top is clipped, instead of the
    /// default (first rows visible, bottom clipped).
    ///
    /// Use it for a fixed-height live tail — a streaming preview, a log pane —
    /// where the newest lines matter most. With content that fits the area it has
    /// no effect.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::Text;
    ///
    /// let tail = Text::new("many\nlines").wrap(true).anchor_bottom(true);
    /// assert!(tail.is_anchored_bottom());
    /// ```
    #[must_use]
    pub const fn anchor_bottom(mut self, anchor_bottom: bool) -> Self {
        self.anchor_bottom = anchor_bottom;
        self
    }

    /// Whether overflow anchors to the bottom (see [`anchor_bottom`](Self::anchor_bottom)).
    #[must_use]
    pub const fn is_anchored_bottom(&self) -> bool {
        self.anchor_bottom
    }

    /// The [`Content`] this widget shows.
    #[must_use]
    pub const fn content(&self) -> &Content<'a> {
        &self.content
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

    /// The resolved default style: the literal override, or the role resolved
    /// against `ctx`'s theme. Spans layer over this.
    fn base_style(&self, ctx: &RenderContext<'_>) -> Style {
        match self.style {
            Appearance::Role(role) => ctx.style(role),
            Appearance::Style(style) => style,
        }
    }

    /// The logical lines of this content: each a run of `(grapheme, style)` pairs,
    /// split on `'\n'`. `base` is the resolved default each span's style layers
    /// over. This is the *one* iterator both paint and wrap consume.
    fn styled_lines(&self, base: Style) -> Vec<Vec<StyledGrapheme<'_>>> {
        let mut lines: Vec<Vec<StyledGrapheme<'_>>> = vec![Vec::new()];
        match &self.content {
            Content::Plain(text) => extend_lines(&mut lines, text, base),
            Content::Spans(spans) => {
                for span in spans.iter() {
                    // Each span's style layers over the resolved base.
                    extend_lines(&mut lines, &span.text, span.style.merge_over(base));
                }
            }
        }
        lines
    }
}

/// Appends `text`'s graphemes (all painted in `style`) into `lines`, splitting on
/// `'\n'`: a newline starts a fresh line, so a run spanning several logical lines
/// tiles across them. The graphemes borrow from `text`, so `lines` inherits its
/// lifetime.
fn extend_lines<'a>(lines: &mut Vec<Vec<StyledGrapheme<'a>>>, text: &'a str, style: Style) {
    let mut first_line = true;
    for line_text in text.split('\n') {
        if !first_line {
            lines.push(Vec::new());
        }
        first_line = false;
        let current = lines.last_mut().expect("at least one line");
        for grapheme in line_text.graphemes(true) {
            current.push(StyledGrapheme { grapheme, style });
        }
    }
}

/// One grapheme cluster paired with the resolved style that paints it — the atom
/// the shared paint/wrap iterator yields.
#[derive(Debug, Clone, Copy)]
struct StyledGrapheme<'a> {
    grapheme: &'a str,
    style: Style,
}

impl StyledGrapheme<'_> {
    /// The display width of this grapheme, clamped to the terminal's 1–2 cell
    /// range (the buffer's width oracle).
    fn width(&self) -> usize {
        UnicodeWidthStr::width(self.grapheme).clamp(1, 2)
    }
}

impl Widget for Text<'_> {
    type State = ();

    fn render(&self, (): &mut (), ctx: &mut RenderContext<'_>) {
        // A11y groundwork (ADR arc4 §5): a static label announced by its text.
        ctx.semantic_role(SemanticRole::Label);
        ctx.label(self.content.to_plain_string());
        let base = self.base_style(ctx);
        let area = ctx.area().size;
        if area.height == 0 || area.width == 0 {
            return;
        }
        let lines = self.styled_lines(base);

        let display_rows: Vec<Vec<StyledGrapheme<'_>>> = if self.wrap {
            lines
                .iter()
                .flat_map(|line| wrap_line(line, area.width))
                .collect()
        } else {
            lines
        };

        // Anchoring to the bottom drops the leading overflow rows so the last
        // `area.height` rows land at the top of the area; the default keeps the
        // first rows and clips the tail.
        let start = if self.anchor_bottom {
            display_rows.len().saturating_sub(usize::from(area.height))
        } else {
            0
        };
        for (y, row) in display_rows[start..].iter().enumerate() {
            let Ok(y) = u16::try_from(y) else { break };
            if y >= area.height {
                break;
            }
            paint_row(ctx, y, row);
        }
    }

    fn desired_height(&self, (): &(), width: u16) -> u16 {
        let lines = self.styled_lines(Style::new());
        let rows = if self.wrap {
            lines
                .iter()
                .map(|line| wrap_line(line, width).len())
                .sum::<usize>()
        } else {
            lines.len()
        };
        u16::try_from(rows).unwrap_or(u16::MAX)
    }
}

/// Paints one display row of styled graphemes at `y`, advancing the column by
/// each grapheme's display width (so a wide grapheme's continuation cell is left
/// to the buffer, exactly as `set_string` handles it).
fn paint_row(ctx: &mut RenderContext<'_>, y: u16, row: &[StyledGrapheme<'_>]) {
    let mut x: u16 = 0;
    let width = ctx.area().size.width;
    for cell in row {
        if x >= width {
            break;
        }
        ctx.set_string(Position::new(x, y), cell.grapheme, cell.style);
        x = x.saturating_add(u16::try_from(cell.width()).unwrap_or(1));
    }
}

/// Soft-wraps one logical line (a run of styled graphemes) to `width` display
/// cells, returning the display rows in order.
///
/// Grapheme-correct and width-aware: a row accumulates graphemes until the next
/// would exceed `width`, preferring to break at the last whitespace so words stay
/// intact. A single word wider than the area is broken at a grapheme boundary (no
/// infinite loop, no split wide grapheme). Styling rides along untouched — the
/// break logic sees widths, not text — so a wide grapheme at a span boundary is
/// kept whole with its own style. A `width` of zero yields one empty row so an
/// empty area still advances a line.
fn wrap_line<'a>(line: &[StyledGrapheme<'a>], width: u16) -> Vec<Vec<StyledGrapheme<'a>>> {
    if width == 0 {
        return vec![Vec::new()];
    }
    let width = usize::from(width);

    let mut rows: Vec<Vec<StyledGrapheme<'a>>> = Vec::new();
    let mut current: Vec<StyledGrapheme<'a>> = Vec::new();
    let mut current_width = 0usize;
    // The index (in `current`) just after the last whitespace break candidate,
    // and the display width up to that point.
    let mut last_space: Option<(usize, usize)> = None;

    for cell in line {
        let advance = cell.width();
        // A grapheme that would overflow the row closes the row first.
        if current_width + advance > width && !current.is_empty() {
            match last_space {
                // Break after the last space: everything up to it stays; the
                // remainder (with leading spaces trimmed) carries to the next row.
                Some((split, _)) => {
                    let remainder: Vec<StyledGrapheme<'a>> = current
                        .split_off(split)
                        .into_iter()
                        .skip_while(|g| g.grapheme.chars().all(char::is_whitespace))
                        .collect();
                    rows.push(std::mem::take(&mut current));
                    current_width = remainder.iter().map(StyledGrapheme::width).sum();
                    current = remainder;
                }
                // No break candidate (a single long word): hard-break here.
                None => {
                    rows.push(std::mem::take(&mut current));
                    current_width = 0;
                }
            }
            last_space = None;
        }
        let is_space = cell.grapheme.chars().all(char::is_whitespace);
        // Drop whitespace that would lead a row (a break already consumed the
        // word separator, so the next row starts at the next word).
        if is_space && current.is_empty() {
            continue;
        }
        if is_space {
            // Record the break candidate after this space.
            last_space = Some((current.len() + 1, current_width + advance));
        }
        current.push(*cell);
        current_width += advance;
    }
    rows.push(current);
    rows
}

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::style::{Attributes, Color, Style};
    use rabbitui_core::text::Span;
    use rabbitui_core::widget::{RenderContext, Widget};

    use super::{Content, Text};

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
        assert_eq!(text.content().to_plain_string(), "hi");
        assert_eq!(text.get_style().unwrap().fg, Some(Color::RED));
    }

    #[test]
    fn new_accepts_str_string_and_spans() {
        assert!(matches!(Text::new("x").content(), Content::Plain(_)));
        assert!(matches!(
            Text::new(String::from("x")).content(),
            Content::Plain(_)
        ));
        assert!(matches!(
            Text::new(vec![Span::raw("x")]).content(),
            Content::Spans(_)
        ));
    }

    #[test]
    fn new_defaults_to_the_text_role_style() {
        use rabbitui_core::theme::{Role, Theme};
        // A default Text has no literal override; it resolves Role::Text.
        assert_eq!(Text::new("x").get_style(), None);
        let mut buffer = Buffer::new(Size::new(2, 1));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(2, 1)), false);
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
            RenderContext::new_themed(&mut buffer, Rect::from_size(Size::new(3, 1)), false, &theme);
        Text::new("hi").role(Role::Danger).render(&mut (), &mut ctx);
        assert_eq!(
            buffer.get(Position::ORIGIN).unwrap().style,
            theme.style(Role::Danger)
        );
    }

    #[test]
    fn renders_a_single_line_from_the_origin() {
        let mut buffer = Buffer::new(Size::new(10, 1));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(10, 1)), false);
        Text::new("hello").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "hello");
    }

    #[test]
    fn splits_on_newline_one_row_per_line() {
        let mut buffer = Buffer::new(Size::new(10, 3));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(10, 3)), false);
        Text::new("one\ntwo\nthree").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "one");
        assert_eq!(row(&buffer, 1), "two");
        assert_eq!(row(&buffer, 2), "three");
    }

    #[test]
    fn lines_past_the_bottom_are_clipped() {
        let mut buffer = Buffer::new(Size::new(10, 2));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(10, 2)), false);
        Text::new("one\ntwo\nthree").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "one");
        assert_eq!(row(&buffer, 1), "two");
    }

    #[test]
    fn anchor_bottom_keeps_the_last_rows_when_content_overflows() {
        let mut buffer = Buffer::new(Size::new(10, 2));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(10, 2)), false);
        Text::new("one\ntwo\nthree")
            .anchor_bottom(true)
            .render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "two");
        assert_eq!(row(&buffer, 1), "three");
    }

    #[test]
    fn anchor_bottom_is_a_no_op_when_content_fits() {
        let mut buffer = Buffer::new(Size::new(10, 3));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(10, 3)), false);
        Text::new("one\ntwo")
            .anchor_bottom(true)
            .render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "one");
        assert_eq!(row(&buffer, 1), "two");
    }

    #[test]
    fn long_lines_clip_at_the_right_edge() {
        let mut buffer = Buffer::new(Size::new(3, 1));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(3, 1)), false);
        Text::new("abcdef").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "abc");
    }

    #[test]
    fn style_applies_to_every_painted_cell() {
        let mut buffer = Buffer::new(Size::new(5, 1));
        let style = Style::new().fg(Color::GREEN);
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(5, 1)), false);
        Text::new("ab").style(style).render(&mut (), &mut ctx);
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().style, style);
        assert_eq!(buffer.get(Position::new(1, 0)).unwrap().style, style);
    }

    #[test]
    fn empty_content_paints_one_blank_line() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(4, 1)), false);
        Text::new("").render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "");
    }

    // --- styled-span content ---

    #[test]
    fn spans_paint_each_run_in_its_own_style() {
        let mut buffer = Buffer::new(Size::new(10, 1));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(10, 1)), false);
        Text::new(vec![
            Span::styled("ok", Style::new().fg(Color::GREEN).bold()),
            Span::styled(" no", Style::new().fg(Color::RED)),
        ])
        .render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "ok no");
        // "ok" is green+bold; " no" is red.
        let ok = buffer.get(Position::new(0, 0)).unwrap();
        assert_eq!(ok.style.fg, Some(Color::GREEN));
        assert!(ok.style.attrs.contains(Attributes::BOLD));
        assert_eq!(
            buffer.get(Position::new(3, 0)).unwrap().style.fg,
            Some(Color::RED)
        );
    }

    #[test]
    fn empty_span_style_resolves_to_the_widget_default() {
        use rabbitui_core::theme::{Role, Theme};
        // A raw (unstyled) span under a Danger role paints in Danger — the role
        // default merges under the empty span style.
        let theme = Theme::catppuccin_mocha();
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx =
            RenderContext::new_themed(&mut buffer, Rect::from_size(Size::new(4, 1)), false, &theme);
        Text::new(vec![Span::raw("err")])
            .role(Role::Danger)
            .render(&mut (), &mut ctx);
        assert_eq!(
            buffer.get(Position::ORIGIN).unwrap().style,
            theme.style(Role::Danger)
        );
    }

    #[test]
    fn span_attrs_layer_over_the_role_default() {
        use rabbitui_core::theme::{Role, Theme};
        // A span carrying only `bold` keeps the role's foreground and adds bold.
        let theme = Theme::catppuccin_mocha();
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx =
            RenderContext::new_themed(&mut buffer, Rect::from_size(Size::new(4, 1)), false, &theme);
        Text::new(vec![Span::styled("hi", Style::new().bold())])
            .role(Role::Accent)
            .render(&mut (), &mut ctx);
        let cell = buffer.get(Position::ORIGIN).unwrap();
        assert_eq!(cell.style.fg, theme.style(Role::Accent).fg);
        assert!(cell.style.attrs.contains(Attributes::BOLD));
    }

    #[test]
    fn spans_split_on_embedded_newlines() {
        let mut buffer = Buffer::new(Size::new(10, 2));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(10, 2)), false);
        Text::new(vec![Span::raw("a\nb"), Span::raw("c")]).render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "a");
        assert_eq!(row(&buffer, 1), "bc");
    }

    // --- wrap ---

    #[test]
    fn wrap_builder_toggles_the_flag() {
        assert!(!Text::new("x").is_wrapped());
        assert!(Text::new("x").wrap(true).is_wrapped());
    }

    #[test]
    fn wrap_breaks_at_word_boundaries() {
        let mut buffer = Buffer::new(Size::new(10, 4));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(10, 4)), false);
        Text::new("the quick brown fox")
            .wrap(true)
            .render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "the quick");
        assert_eq!(row(&buffer, 1), "brown fox");
        assert_eq!(row(&buffer, 2), "");
    }

    #[test]
    fn wrap_hard_breaks_a_word_longer_than_the_area() {
        let mut buffer = Buffer::new(Size::new(5, 3));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(5, 3)), false);
        Text::new("abcdefghijkl")
            .wrap(true)
            .render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "abcde");
        assert_eq!(row(&buffer, 1), "fghij");
        assert_eq!(row(&buffer, 2), "kl");
    }

    #[test]
    fn wrap_keeps_wide_graphemes_whole_at_the_boundary() {
        let mut buffer = Buffer::new(Size::new(4, 2));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(4, 2)), false);
        Text::new("世界語").wrap(true).render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "世界");
        assert_eq!(row(&buffer, 1), "語");
    }

    #[test]
    fn wrap_keeps_wide_graphemes_whole_across_a_span_boundary() {
        // Two spans, each one wide CJK grapheme, then a third: styled content must
        // wrap exactly like plain, never straddling the edge at the span seam.
        let mut buffer = Buffer::new(Size::new(4, 2));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(4, 2)), false);
        Text::new(vec![
            Span::styled("世", Style::new().fg(Color::GREEN)),
            Span::styled("界", Style::new().fg(Color::RED)),
            Span::styled("語", Style::new().fg(Color::BLUE)),
        ])
        .wrap(true)
        .render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "世界");
        assert_eq!(row(&buffer, 1), "語");
        // Styling survives the wrap: the second grapheme (red) is on row 0, the
        // third (blue) wrapped to row 1.
        assert_eq!(
            buffer.get(Position::new(2, 0)).unwrap().style.fg,
            Some(Color::RED)
        );
        assert_eq!(
            buffer.get(Position::new(0, 1)).unwrap().style.fg,
            Some(Color::BLUE)
        );
    }

    #[test]
    fn wrap_preserves_explicit_newlines() {
        let mut buffer = Buffer::new(Size::new(20, 3));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(20, 3)), false);
        Text::new("one\ntwo three")
            .wrap(true)
            .render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "one");
        assert_eq!(row(&buffer, 1), "two three");
    }

    #[test]
    fn styled_wrap_keeps_per_span_styling() {
        // "aaa bbb" styled: green word, red word. Wrapped at width 3, styling
        // survives on both rows.
        let mut buffer = Buffer::new(Size::new(3, 2));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(3, 2)), false);
        Text::new(vec![
            Span::styled("aaa ", Style::new().fg(Color::GREEN)),
            Span::styled("bbb", Style::new().fg(Color::RED)),
        ])
        .wrap(true)
        .render(&mut (), &mut ctx);
        assert_eq!(row(&buffer, 0), "aaa");
        assert_eq!(row(&buffer, 1), "bbb");
        assert_eq!(
            buffer.get(Position::new(0, 0)).unwrap().style.fg,
            Some(Color::GREEN)
        );
        assert_eq!(
            buffer.get(Position::new(0, 1)).unwrap().style.fg,
            Some(Color::RED)
        );
    }

    // --- measurement ---

    #[test]
    fn desired_height_counts_lines_unwrapped() {
        assert_eq!(Text::new("a\nb\nc").desired_height(&(), 10), 3);
        assert_eq!(Text::new("single").desired_height(&(), 10), 1);
    }

    #[test]
    fn desired_height_counts_wrapped_rows() {
        // "the quick brown fox" at width 10 wraps into two rows.
        let text = Text::new("the quick brown fox").wrap(true);
        assert_eq!(text.desired_height(&(), 10), 2);
    }

    #[test]
    fn desired_height_of_spans_counts_flattened_lines() {
        let text = Text::new(vec![Span::raw("a\nb"), Span::raw("c")]);
        assert_eq!(text.desired_height(&(), 10), 2);
    }
}
