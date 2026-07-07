//! A focusable push button.

use rabbitui_core::geometry::Position;
use rabbitui_core::input::{InputEvent, Key};
use rabbitui_core::outcome::Outcome;
use rabbitui_core::style::Style;
use rabbitui_core::widget::{HandleCtx, Handled, RenderCtx, Widget};

/// A single-line push button: a label that takes focus and activates on Enter
/// or Space.
///
/// `Button` is rabbitui's first interactive widget and the smallest proof of the
/// slice-3 machinery: it declares itself focusable (so it joins tab traversal),
/// paints reversed when focused (so focus is visible), and emits
/// [`Outcome::Activated`] from its handler when pressed (so the app learns it
/// was clicked, via `update`). It is stateless — `State = ()` — because focus
/// lives in the framework, not the widget (ADR 0006).
///
/// The label may carry its own [`Style`]; when the button is focused that style
/// gains the reversed attribute so the focused button stands out regardless of
/// its base colors.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::Button;
///
/// let ok = Button::new("OK");
/// assert_eq!(ok.label(), "OK");
/// ```
///
/// In a view, declared by key like any widget:
///
/// ```
/// use rabbitui_core::buffer::Buffer;
/// use rabbitui_core::frame::Frame;
/// use rabbitui_core::geometry::Size;
/// use rabbitui_core::id::key;
/// use rabbitui_core::store::StateStore;
/// use rabbitui_widgets::Button;
///
/// let mut buffer = Buffer::new(Size::new(6, 1));
/// let mut store = StateStore::new();
/// store.begin_frame();
/// let mut frame = Frame::new(&mut buffer, &mut store);
/// frame.widget(key("ok"), frame.area(), &Button::new("OK"));
/// # let _ = frame.finish();
/// store.end_frame();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Button<'a> {
    label: &'a str,
    style: Style,
}

impl<'a> Button<'a> {
    /// Creates a button showing `label` in the default style.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::Button;
    ///
    /// let button = Button::new("Save");
    /// assert_eq!(button.label(), "Save");
    /// ```
    #[must_use]
    pub const fn new(label: &'a str) -> Self {
        Self { label, style: Style::new() }
    }

    /// Sets the button's base style (its style when unfocused).
    ///
    /// When focused the button paints this style with the reversed attribute
    /// added, so focus is visible on top of whatever base style is set.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::{Color, Style};
    /// use rabbitui_widgets::Button;
    ///
    /// let danger = Style::new().fg(Color::RED).bold();
    /// let button = Button::new("Delete").style(danger);
    /// assert_eq!(button.get_style(), danger);
    /// ```
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The button's label.
    #[must_use]
    pub const fn label(&self) -> &'a str {
        self.label
    }

    /// The button's base style.
    ///
    /// Named `get_style` because [`style`](Self::style) is the builder setter,
    /// matching [`Style`]'s own field/builder split.
    #[must_use]
    pub const fn get_style(&self) -> Style {
        self.style
    }
}

impl Widget for Button<'_> {
    type State = ();

    fn render(&self, (): &mut (), ctx: &mut RenderCtx<'_>) {
        ctx.focusable(true);
        let style = if ctx.is_focused() { self.style.reversed() } else { self.style };
        ctx.set_string(Position::ORIGIN, self.label, style);
    }

    fn handle((): &mut (), event: &InputEvent, ctx: &mut HandleCtx<'_>) -> Handled {
        let Some(key) = event.as_key() else { return Handled::No };
        match key.key {
            Key::Enter | Key::Char(' ') => {
                ctx.emit(Outcome::Activated);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::input::{InputEvent, Key};
    use rabbitui_core::outcome::Outcome;
    use rabbitui_core::style::{Attrs, Style};
    use rabbitui_core::widget::{HandleCtx, Handled, Phase, RenderCtx, Widget};

    use super::Button;

    fn cell_style(buffer: &Buffer, x: u16) -> Style {
        buffer.get(Position::new(x, 0)).unwrap().style
    }

    #[test]
    fn builder_sets_label_and_style() {
        let style = Style::new().bold();
        let button = Button::new("Go").style(style);
        assert_eq!(button.label(), "Go");
        assert_eq!(button.get_style(), style);
    }

    #[test]
    fn renders_label_plainly_when_unfocused() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(4, 1)), false);
        Button::new("Go").render(&mut (), &mut ctx);
        assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "G");
        assert!(!cell_style(&buffer, 0).attrs.contains(Attrs::REVERSED));
    }

    #[test]
    fn renders_reversed_when_focused() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(4, 1)), true);
        Button::new("Go").render(&mut (), &mut ctx);
        assert!(cell_style(&buffer, 0).attrs.contains(Attrs::REVERSED));
    }

    fn dispatch(event: InputEvent) -> (Handled, Vec<Outcome>) {
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let handled = {
            let mut ctx =
                HandleCtx::new(Phase::Bubble, Rect::default(), &mut outcomes, &mut request_focus);
            Button::handle(&mut (), &event, &mut ctx)
        };
        (handled, outcomes)
    }

    #[test]
    fn enter_activates() {
        let (handled, outcomes) = dispatch(InputEvent::key(Key::Enter));
        assert_eq!(handled, Handled::Yes);
        assert_eq!(outcomes, vec![Outcome::Activated]);
    }

    #[test]
    fn space_activates() {
        let (handled, outcomes) = dispatch(InputEvent::key(Key::Char(' ')));
        assert_eq!(handled, Handled::Yes);
        assert_eq!(outcomes, vec![Outcome::Activated]);
    }

    #[test]
    fn other_keys_are_ignored() {
        let (handled, outcomes) = dispatch(InputEvent::key(Key::Char('x')));
        assert_eq!(handled, Handled::No);
        assert!(outcomes.is_empty());
    }
}
