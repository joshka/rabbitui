//! The bridge: render a ratatui widget, copy its cells into a rabbitui frame.
//!
//! This is ADR 0010's chosen mechanism (Option C, §Decision.3):
//! render-into-`Buffer`-and-copy-cells. [`render_ratatui`] and
//! [`render_ratatui_stateful`] allocate a ratatui [`Buffer`](ratatui::buffer::Buffer) the size of the
//! target area, invoke the widget's paint step into it, then copy every cell —
//! grapheme plus converted style — into the rabbitui [`RenderContext`] at the same
//! coordinate. No ratatui `Terminal`, backend, or draw loop is involved.
//!
//! # What crosses the bridge
//!
//! Cells and nothing else (ADR 0010 §Decision.5). Identity, focus, hit regions,
//! cursor candidates, and outcomes are *not* produced for bridged content: a
//! bridged ratatui widget is an inert rectangle of styled cells. Styling arrives
//! pre-resolved to concrete colors and is not re-themed on a theme switch.
//!
//! # Wide graphemes
//!
//! ratatui and rabbitui share the skip-cell convention for double-width
//! graphemes (ADR 0003, ADR 0010 §Context.3): the wide cluster lives in a lead
//! cell and the following cell is a continuation. ratatui marks the
//! continuation by storing no symbol (its `Cell::symbol()` then reads as a
//! space); rabbitui marks it with an empty symbol. The copy bridges the two by
//! *width*: when a lead cell holds a width-2 grapheme, rabbitui's
//! [`RenderContext::set_string`] writes both the lead and its continuation, and the
//! bridge skips the ratatui continuation cell so it does not paint a stray space
//! over the right half of the glyph.

use rabbitui_core::geometry::Position;
use rabbitui_core::widget::RenderContext;
use ratatui::buffer::Buffer as RatBuffer;
use ratatui::layout::Rect as RatRect;
use ratatui::widgets::{StatefulWidget, Widget};
use unicode_width::UnicodeWidthStr;

use crate::style::convert_style;

/// Renders a ratatui [`Widget`] into the rabbitui frame's current widget area.
///
/// Allocates a ratatui [`Buffer`](ratatui::buffer::Buffer) covering `ctx`'s
/// area, paints `widget` into it, and copies each resulting cell into `ctx`.
/// The widget is consumed (ratatui's `Widget::render` takes `self` by value).
/// Painting is clipped to the area by `ctx`; an empty area paints nothing.
///
/// This is the drawing escape hatch of ADR 0010: any `ratatui::Widget` — a
/// third-party chart, a canvas, a `Paragraph` — becomes an inert rectangle of
/// styled cells inside a rabbitui frame. It carries no focus, hit region, or
/// outcome (§Decision.5); wrap it in a rabbitui widget if you need those.
///
/// # Examples
///
/// ```
/// use rabbitui_core::buffer::Buffer;
/// use rabbitui_core::geometry::{Position, Rect, Size};
/// use rabbitui_core::widget::RenderContext;
/// use rabbitui_ratatui::render_ratatui;
/// use ratatui::widgets::Paragraph;
///
/// let mut buffer = Buffer::new(Size::new(5, 1));
/// let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(5, 1)), false);
/// render_ratatui(Paragraph::new("hi"), &mut ctx);
/// assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "h");
/// ```
pub fn render_ratatui<W: Widget>(widget: W, ctx: &mut RenderContext<'_>) {
    let Some(mut rat_buffer) = area_buffer(ctx) else {
        return;
    };
    let rat_area = *rat_buffer.area();
    widget.render(rat_area, &mut rat_buffer);
    copy_cells(&rat_buffer, ctx);
}

/// Renders a ratatui [`StatefulWidget`] into the frame's current widget area,
/// threading caller-owned `state` for this one paint.
///
/// The stateful analog of [`render_ratatui`]. ratatui `StatefulWidget` state
/// (a `ListState` scroll offset, a table selection) is owned by the *caller* and
/// lent for one frame — it does **not** enter rabbitui's per-identity state store
/// (ADR 0010 §Consequences.Negative). Keep the `State` in your app struct and
/// pass `&mut` to it each frame, exactly as a plain ratatui app would.
///
/// # Examples
///
/// ```
/// use rabbitui_core::buffer::Buffer;
/// use rabbitui_core::geometry::{Rect, Size};
/// use rabbitui_core::widget::RenderContext;
/// use rabbitui_ratatui::render_ratatui_stateful;
/// use ratatui::widgets::{List, ListState};
///
/// let mut buffer = Buffer::new(Size::new(8, 3));
/// let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(8, 3)), false);
/// let mut state = ListState::default();
/// state.select(Some(1));
/// let list = List::new(["a", "b", "c"]);
/// render_ratatui_stateful(list, &mut state, &mut ctx);
/// ```
pub fn render_ratatui_stateful<W: StatefulWidget>(
    widget: W,
    state: &mut W::State,
    ctx: &mut RenderContext<'_>,
) {
    let Some(mut rat_buffer) = area_buffer(ctx) else {
        return;
    };
    let rat_area = *rat_buffer.area();
    widget.render(rat_area, &mut rat_buffer, state);
    copy_cells(&rat_buffer, ctx);
}

/// Allocates a ratatui [`Buffer`](ratatui::buffer::Buffer) covering `ctx`'s area,
/// or `None` if the area is empty (nothing to paint).
///
/// The buffer's own [`Rect`](ratatui::layout::Rect) starts at the origin — the
/// widget paints in area-relative coordinates, matching how [`copy_cells`] reads
/// them back and how [`RenderContext::set_string`] expects relative positions.
fn area_buffer(ctx: &RenderContext<'_>) -> Option<RatBuffer> {
    let size = ctx.size();
    if size.width == 0 || size.height == 0 {
        return None;
    }
    Some(RatBuffer::empty(RatRect::new(
        0,
        0,
        size.width,
        size.height,
    )))
}

/// Copies every cell of `rat_buffer` into `ctx`, converting style and honoring
/// the shared wide-grapheme skip-cell convention.
///
/// Each row is walked left to right. A cell's grapheme and converted style are
/// written through [`RenderContext::set_string`]; when that grapheme is wide
/// (advances two columns), `set_string` also writes rabbitui's continuation
/// cell, so the following ratatui cell — ratatui's own continuation — is skipped
/// rather than painted as a stray space over the glyph's right half.
fn copy_cells(rat_buffer: &RatBuffer, ctx: &mut RenderContext<'_>) {
    let area = *rat_buffer.area();
    for y in 0..area.height {
        let mut x = 0;
        while x < area.width {
            let cell = &rat_buffer[(x, y)];
            let symbol = cell.symbol();
            let style = convert_style(cell);
            ctx.set_string(Position::new(x, y), symbol, style);
            // A wide grapheme owns the next column too: rabbitui's set_string
            // has already written its continuation, so skip ratatui's. Clamp to
            // 1..=2: a zero-width or empty symbol still advances one column, so
            // the walk can never stall.
            let advance = UnicodeWidthStr::width(symbol).clamp(1, 2) as u16;
            x += advance;
        }
    }
}

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::style::{Attributes, Color};
    use rabbitui_core::widget::RenderContext;
    use ratatui::style::{Color as RatColor, Modifier, Style as RatStyle};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Paragraph};

    use super::{render_ratatui, render_ratatui_stateful};

    fn symbol(buffer: &Buffer, x: u16, y: u16) -> String {
        buffer.get(Position::new(x, y)).unwrap().symbol.clone()
    }

    #[test]
    fn copies_plain_text_grapheme_for_grapheme() {
        let mut buffer = Buffer::new(Size::new(5, 1));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(5, 1)), false);
        render_ratatui(Paragraph::new("hi!"), &mut ctx);
        assert_eq!(symbol(&buffer, 0, 0), "h");
        assert_eq!(symbol(&buffer, 1, 0), "i");
        assert_eq!(symbol(&buffer, 2, 0), "!");
    }

    #[test]
    fn copies_fg_bg_and_attributes() {
        let mut buffer = Buffer::new(Size::new(3, 1));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(3, 1)), false);
        let style = RatStyle::default()
            .fg(RatColor::Green)
            .bg(RatColor::Blue)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC);
        render_ratatui(
            Paragraph::new(Line::from(Span::styled("ok", style))),
            &mut ctx,
        );
        let cell = buffer.get(Position::ORIGIN).unwrap();
        assert_eq!(cell.symbol, "o");
        assert_eq!(cell.style.fg, Some(Color::Ansi(2)));
        assert_eq!(cell.style.bg, Some(Color::Ansi(4)));
        assert!(
            cell.style
                .attrs
                .contains(Attributes::BOLD | Attributes::ITALIC)
        );
    }

    #[test]
    fn wide_grapheme_becomes_lead_plus_continuation() {
        let mut buffer = Buffer::new(Size::new(6, 1));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(6, 1)), false);
        // A CJK lead (width 2) followed by a narrow grapheme.
        render_ratatui(Paragraph::new("世x"), &mut ctx);
        assert_eq!(symbol(&buffer, 0, 0), "世");
        // The cell after the wide lead is a rabbitui continuation cell (empty),
        // not a stray space copied from ratatui's own continuation.
        assert!(buffer.get(Position::new(1, 0)).unwrap().is_continuation());
        // The next grapheme lands after the wide cluster, in sync with ratatui's
        // two-column advance.
        assert_eq!(symbol(&buffer, 2, 0), "x");
    }

    #[test]
    fn bordered_block_paints_corners_and_edges() {
        let mut buffer = Buffer::new(Size::new(4, 3));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(4, 3)), false);
        render_ratatui(Block::bordered(), &mut ctx);
        assert_eq!(symbol(&buffer, 0, 0), "┌");
        assert_eq!(symbol(&buffer, 3, 0), "┐");
        assert_eq!(symbol(&buffer, 0, 2), "└");
        assert_eq!(symbol(&buffer, 3, 2), "┘");
        assert_eq!(symbol(&buffer, 1, 0), "─");
        assert_eq!(symbol(&buffer, 0, 1), "│");
    }

    #[test]
    fn empty_area_paints_nothing() {
        let mut buffer = Buffer::new(Size::new(4, 2));
        // A zero-size area: the context clips everything to nothing.
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(0, 0)), false);
        render_ratatui(Paragraph::new("nope"), &mut ctx);
        // The buffer is untouched — still all blank.
        assert_eq!(symbol(&buffer, 0, 0), " ");
    }

    #[test]
    fn paints_relative_to_a_shifted_area() {
        let mut buffer = Buffer::new(Size::new(8, 3));
        let area = Rect::new(Position::new(2, 1), Size::new(4, 1));
        let mut ctx = RenderContext::new(&mut buffer, area, false);
        render_ratatui(Paragraph::new("ab"), &mut ctx);
        // Painting is offset by the area origin, exactly like a native widget.
        assert_eq!(symbol(&buffer, 2, 1), "a");
        assert_eq!(symbol(&buffer, 3, 1), "b");
        assert_eq!(symbol(&buffer, 0, 0), " ");
    }

    #[test]
    fn stateful_widget_uses_caller_owned_state() {
        use ratatui::widgets::{List, ListState};

        let mut buffer = Buffer::new(Size::new(6, 3));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(6, 3)), false);
        let mut state = ListState::default();
        state.select(Some(1));
        let list = List::new(["a", "b", "c"])
            .highlight_style(RatStyle::default().add_modifier(Modifier::REVERSED));
        render_ratatui_stateful(list, &mut state, &mut ctx);
        // The three rows are present; the selected (second) row carries the
        // reversed highlight the caller-owned state drove.
        assert_eq!(symbol(&buffer, 0, 0), "a");
        assert_eq!(symbol(&buffer, 0, 1), "b");
        assert_eq!(symbol(&buffer, 0, 2), "c");
        assert!(
            buffer
                .get(Position::new(0, 1))
                .unwrap()
                .style
                .attrs
                .contains(Attributes::REVERSED)
        );
    }
}
