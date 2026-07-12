//! A rabbitui [`Widget`] wrapper around a ratatui widget.
//!
//! [`render_ratatui`] is the imperative bridge — call it
//! inside a widget's `render`. [`RatatuiWidget`] is the *declarative* one: it
//! wraps a ratatui widget so it implements rabbitui's [`Widget`] trait and drops
//! straight into [`Frame::widget`](rabbitui_core::frame::Frame::widget), keyed
//! like any native widget.
//!
//! # Why the ratatui widget must be `Clone`
//!
//! rabbitui's [`Widget::render`] takes `&self` (a spec is rendered without being
//! consumed), but ratatui's `Widget::render` takes `self` by value. The wrapper
//! bridges the two by holding a `Clone` ratatui widget and cloning it per frame
//! to hand ratatui the owned value it wants. Every first-party ratatui widget
//! (`Paragraph`, `Block`, `Gauge`, `List`, …) is `Clone`, and specs are rebuilt
//! every frame anyway (ADR 0001), so the clone is on the frame's existing cost
//! curve.
//!
//! # State is `()` — bridged content is inert
//!
//! [`RatatuiWidget`]'s `State` is `()`: a bridged widget carries no rabbitui
//! identity, focus, or outcome (ADR 0010 §Decision.5). For a ratatui
//! `StatefulWidget` whose state you own, use
//! [`render_ratatui_stateful`](crate::render_ratatui_stateful) inside a native
//! widget's `render` and keep the state in your app struct — that state cannot
//! live in rabbitui's per-identity store (§Consequences.Negative).

use rabbitui_core::widget::{RenderContext, Widget};
use ratatui::widgets::Widget as RatWidget;

use crate::render_ratatui;

/// Wraps a `Clone` ratatui [`Widget`](ratatui::widgets::Widget) as a rabbitui
/// [`Widget`], so it can be declared into a frame by key.
///
/// The wrapper renders through [`render_ratatui`], so all of that function's
/// contract holds: cells only, no focus, no hit region, styles pre-resolved and
/// not re-themed (ADR 0010 §Decision.5). Its `State` is `()`.
///
/// # Examples
///
/// ```
/// use rabbitui_core::buffer::Buffer;
/// use rabbitui_core::frame::Frame;
/// use rabbitui_core::geometry::Size;
/// use rabbitui_core::id::key;
/// use rabbitui_core::store::StateStore;
/// use rabbitui_ratatui::RatatuiWidget;
/// use ratatui::widgets::Block;
///
/// let mut buffer = Buffer::new(Size::new(10, 3));
/// let mut store = StateStore::new();
/// store.begin_frame();
/// let mut frame = Frame::new(&mut buffer, &mut store);
/// frame.widget(key("border"), frame.area(), &RatatuiWidget::new(Block::bordered()));
/// # let _ = frame.finish();
/// store.end_frame();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatatuiWidget<W> {
    widget: W,
}

impl<W: RatWidget + Clone> RatatuiWidget<W> {
    /// Wraps `widget` for declaration into a rabbitui frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_ratatui::RatatuiWidget;
    /// use ratatui::widgets::Paragraph;
    ///
    /// let wrapped = RatatuiWidget::new(Paragraph::new("hi"));
    /// let _ = wrapped;
    /// ```
    #[must_use]
    pub const fn new(widget: W) -> Self {
        Self { widget }
    }

    /// A reference to the wrapped ratatui widget.
    #[must_use]
    pub const fn inner(&self) -> &W {
        &self.widget
    }
}

impl<W: RatWidget + Clone> Widget for RatatuiWidget<W> {
    type State = ();

    fn render(&self, (): &mut (), ctx: &mut RenderContext<'_>) {
        // Clone to hand ratatui the owned value its by-value render wants; the
        // spec itself is untouched, so the same wrapper renders every frame.
        render_ratatui(self.widget.clone(), ctx);
    }
}

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::widget::{RenderContext, Widget};
    use ratatui::widgets::{Block, Paragraph};

    use super::RatatuiWidget;

    fn symbol(buffer: &Buffer, x: u16, y: u16) -> String {
        buffer.get(Position::new(x, y)).unwrap().symbol.clone()
    }

    #[test]
    fn wraps_and_renders_a_paragraph() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(4, 1)), false);
        RatatuiWidget::new(Paragraph::new("hey")).render(&mut (), &mut ctx);
        assert_eq!(symbol(&buffer, 0, 0), "h");
        assert_eq!(symbol(&buffer, 2, 0), "y");
    }

    #[test]
    fn inner_returns_the_wrapped_widget() {
        let block = Block::bordered();
        let wrapped = RatatuiWidget::new(block.clone());
        assert_eq!(wrapped.inner(), &block);
    }

    #[test]
    fn is_not_focusable_bridged_content_is_inert() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx = RenderContext::new(&mut buffer, Rect::from_size(Size::new(4, 1)), false);
        RatatuiWidget::new(Paragraph::new("x")).render(&mut (), &mut ctx);
        // The wrapper never declares itself focusable (ADR 0010 §Decision.5).
        assert!(!ctx.is_focusable());
    }
}
