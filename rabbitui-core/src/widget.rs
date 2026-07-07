//! The widget contract.
//!
//! Per `docs/adr/0001-programming-model.md` and `docs/adr/0008-widget-contract.md`:
//! a widget is a short-lived *spec* — a plain value describing what to show —
//! rendered against framework-retained per-identity state. Specs are built
//! fresh every frame from app data; anything that must survive the frame
//! (scroll, cursor, focus) lives in the framework's state store and is lent to
//! the widget as `&mut` during render.
//!
//! Widgets paint through a [`RenderCtx`], which owns clipping to the widget's
//! area (and, from slice 3, collects frame facts: hit regions, focus entries,
//! cursor candidates).
//!
//! # Examples
//!
//! A minimal stateless widget:
//!
//! ```
//! use rabbitui_core::geometry::Position;
//! use rabbitui_core::style::Style;
//! use rabbitui_core::widget::{RenderCtx, Widget};
//!
//! struct Label<'a>(&'a str);
//!
//! impl Widget for Label<'_> {
//!     type State = ();
//!
//!     fn render(&self, _state: &mut (), ctx: &mut RenderCtx<'_>) {
//!         ctx.set_string(Position::ORIGIN, self.0, Style::new());
//!     }
//! }
//! ```

use crate::buffer::Buffer;
use crate::geometry::{Position, Rect};
use crate::style::Style;

/// A widget spec: a per-frame description of one widget, rendered against its
/// retained state.
///
/// `State` is the widget's framework-retained state — `()` for stateless
/// widgets. It must implement `Default` (the state a widget has the first
/// frame it appears) and is kept across frames by identity
/// (`docs/adr/0002-widget-identity.md`).
pub trait Widget {
    /// Framework-retained state for this widget kind.
    type State: Default + 'static;

    /// Paints the widget into its area and updates retained state.
    fn render(&self, state: &mut Self::State, ctx: &mut RenderCtx<'_>);
}

/// The surface a widget paints through: its area of the buffer, pre-clipped.
///
/// Positions passed to paint methods are relative to the widget's own area;
/// painting outside the area is clipped, never an error.
#[derive(Debug)]
pub struct RenderCtx<'a> {
    buffer: &'a mut Buffer,
    /// The widget's area in buffer coordinates, already clipped to the buffer.
    area: Rect,
}

impl<'a> RenderCtx<'a> {
    /// Creates a context painting into `area` of `buffer`.
    ///
    /// `area` is clipped to the buffer's bounds; a fully out-of-bounds area
    /// yields a context whose paints are all no-ops.
    #[must_use]
    pub fn new(buffer: &'a mut Buffer, area: Rect) -> Self {
        let bounds = Rect::from_size(buffer.size());
        let area = area.intersection(bounds);
        Self { buffer, area }
    }

    /// The widget's area size (relative coordinates run from the origin to
    /// this size).
    #[must_use]
    pub fn area(&self) -> Rect {
        Rect::from_size(self.area.size)
    }

    /// Writes `text` at `position` (relative to the widget's area) in
    /// `style`, clipped to the area's right edge.
    pub fn set_string(&mut self, position: Position, text: &str, style: Style) {
        if position.y >= self.area.size.height || position.x >= self.area.size.width {
            return;
        }
        let absolute = Position::new(
            self.area.origin.x + position.x,
            self.area.origin.y + position.y,
        );
        let max_width = usize::from(self.area.size.width - position.x);
        self.buffer.set_stringn(absolute, text, style, max_width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Size;

    #[test]
    fn paints_relative_to_area() {
        let mut buffer = Buffer::new(Size::new(10, 3));
        let area = Rect::new(Position::new(2, 1), Size::new(5, 1));
        let mut ctx = RenderCtx::new(&mut buffer, area);
        ctx.set_string(Position::new(1, 0), "hi", Style::new());
        assert_eq!(buffer.get(Position::new(3, 1)).unwrap().symbol, "h");
        assert_eq!(buffer.get(Position::new(4, 1)).unwrap().symbol, "i");
    }

    #[test]
    fn clips_to_area_not_buffer() {
        let mut buffer = Buffer::new(Size::new(10, 3));
        let area = Rect::new(Position::new(2, 1), Size::new(3, 1));
        let mut ctx = RenderCtx::new(&mut buffer, area);
        ctx.set_string(Position::ORIGIN, "abcdef", Style::new());
        // "abc" fits the 3-wide area; "def" is clipped even though the buffer
        // continues.
        assert_eq!(buffer.get(Position::new(4, 1)).unwrap().symbol, "c");
        assert_eq!(buffer.get(Position::new(5, 1)).unwrap().symbol, " ");
    }

    #[test]
    fn out_of_area_positions_are_no_ops() {
        let mut buffer = Buffer::new(Size::new(4, 2));
        let area = Rect::new(Position::ORIGIN, Size::new(4, 1));
        let mut ctx = RenderCtx::new(&mut buffer, area);
        ctx.set_string(Position::new(0, 5), "nope", Style::new());
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, " ");
    }

    #[test]
    fn area_outside_buffer_is_empty() {
        let mut buffer = Buffer::new(Size::new(4, 2));
        let area = Rect::new(Position::new(10, 10), Size::new(5, 5));
        let ctx = RenderCtx::new(&mut buffer, area);
        assert!(ctx.area().is_empty());
    }
}
