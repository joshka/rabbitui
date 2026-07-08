//! A focusable push button.

use rabbitui_core::a11y::SemanticRole;
use rabbitui_core::geometry::Position;
use rabbitui_core::input::{InputEvent, Key, MouseButton, MouseKind};
use rabbitui_core::outcome::Outcome;
use rabbitui_core::style::{Color, Style};
use rabbitui_core::theme::Role;
use rabbitui_core::widget::{HandleCtx, Handled, RenderCtx, Widget};

/// A single-line push button: a label that takes focus and activates on Enter
/// or Space.
///
/// `Button` is rabbitui's first interactive widget and the smallest proof of the
/// slice-3 machinery: it declares itself focusable (so it joins tab traversal),
/// paints its focus state (so focus is visible), and emits [`Outcome::Activated`]
/// from its handler when pressed (so the app learns it was clicked, via
/// `update`). It is stateless — `State = ()` — because focus lives in the
/// framework, not the widget (ADR 0006).
///
/// Styling is by role (ADR 0007): the label paints in [`Role::Text`] when
/// unfocused and [`Role::Highlight`] when focused, both resolved against the
/// active theme, so a focused button stands out in whatever palette is loaded.
/// [`style`](Self::style) overrides the *unfocused* style with a literal
/// [`Style`] for a one-off button no role captures; the focused style still
/// comes from [`Role::Highlight`].
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
    /// A literal override for the unfocused style; `None` resolves [`Role::Text`].
    style: Option<Style>,
    /// Whether to paint as a solid, filled chip (a tonal fill with a same-hue
    /// label) rather than plain role-colored text. See [`filled`](Self::filled).
    filled: bool,
}

impl<'a> Button<'a> {
    /// Creates a button showing `label` styled by role: [`Role::Text`] unfocused,
    /// [`Role::Highlight`] focused.
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
        Self {
            label,
            style: None,
            filled: false,
        }
    }

    /// Paints the button as a solid, filled chip instead of plain colored text.
    ///
    /// The whole button area fills with a darker tone of its role color and the
    /// label sits centered on it in the full role color — the same hue at two
    /// lightnesses, which reads as a tactile solid button (the technique statusbar
    /// segments use). Focus brightens the fill (via [`Role::Highlight`]) and bolds
    /// the label. On a palette (`Ansi`) theme with no same-hue shade, it falls back
    /// to the role color filled with a dark label, still solid.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_widgets::Button;
    ///
    /// let allow = Button::new("Allow").filled(true);
    /// assert!(allow.is_filled());
    /// ```
    #[must_use]
    pub const fn filled(mut self, filled: bool) -> Self {
        self.filled = filled;
        self
    }

    /// Whether the button paints as a solid filled chip (see [`filled`](Self::filled)).
    #[must_use]
    pub const fn is_filled(&self) -> bool {
        self.filled
    }

    /// Overrides the button's *unfocused* style with a literal [`Style`].
    ///
    /// An escape hatch when no role fits; the focused style still comes from
    /// [`Role::Highlight`] so focus stays visible. Prefer theming the roles over
    /// setting this.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::{Color, Style};
    /// use rabbitui_widgets::Button;
    ///
    /// let danger = Style::new().fg(Color::RED).bold();
    /// let button = Button::new("Delete").style(danger);
    /// assert_eq!(button.get_style(), Some(danger));
    /// ```
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }

    /// The button's label.
    #[must_use]
    pub const fn label(&self) -> &'a str {
        self.label
    }

    /// The literal unfocused-style override, if one was set with
    /// [`style`](Self::style), or `None` if the button resolves [`Role::Text`].
    #[must_use]
    pub const fn get_style(&self) -> Option<Style> {
        self.style
    }

    /// Paints the solid-chip form: a tonal fill across the whole area with the
    /// label centered on it in the same hue (see [`filled`](Self::filled)).
    fn render_filled(&self, ctx: &mut RenderCtx<'_>) {
        let role = if ctx.is_focused() {
            Role::Highlight
        } else {
            Role::Accent
        };
        let base = ctx.style(role).fg.unwrap_or(Color::WHITE);
        let (fill, ink) = tonal_pair(base);

        let size = ctx.area().size;
        // Fill every cell of the button with the darker tone.
        let blank = " ".repeat(size.width as usize);
        let fill_style = Style::new().bg(fill);
        for y in 0..size.height {
            ctx.set_string(Position::new(0, y), &blank, fill_style);
        }
        // Center the label; a transparent bg lets it compose over the fill.
        let label_width = self.label.chars().count() as u16;
        let x = size.width.saturating_sub(label_width) / 2;
        let y = size.height / 2;
        let mut ink_style = Style::new().fg(ink);
        if ctx.is_focused() {
            ink_style = ink_style.bold();
        }
        ctx.set_string(Position::new(x, y), self.label, ink_style);
    }
}

/// Splits a role color into a `(fill, ink)` tonal pair: a darker fill with the
/// original color as the label ink, both the same hue. A palette (`Ansi`) color
/// has no same-hue shade, so it fills with the color itself and inks in black —
/// still a solid chip, just not tonal.
fn tonal_pair(base: Color) -> (Color, Color) {
    let fill = base.darken(0.55);
    if fill == base {
        (base, Color::BLACK)
    } else {
        (fill, base)
    }
}

impl Widget for Button<'_> {
    type State = ();

    fn render(&self, (): &mut (), ctx: &mut RenderCtx<'_>) {
        ctx.focusable(true);
        // A11y groundwork (ADR arc4 §5): a button, labelled by its caption.
        ctx.semantic_role(SemanticRole::Button);
        ctx.label(self.label);
        if self.filled {
            self.render_filled(ctx);
            return;
        }
        let style = if ctx.is_focused() {
            ctx.style(Role::Highlight)
        } else {
            self.style.unwrap_or_else(|| ctx.style(Role::Text))
        };
        ctx.set_string(Position::ORIGIN, self.label, style);
    }

    fn handle((): &mut (), event: &InputEvent, ctx: &mut HandleCtx<'_>) -> Handled {
        // A left-button press over the button activates it (click), mirroring
        // Enter/Space. The router has already resolved the hit region, so the
        // press need only be checked for button + kind.
        if let Some(mouse) = event.as_mouse() {
            if mouse.button == MouseButton::Left && mouse.kind == MouseKind::Down {
                ctx.emit(Outcome::Activated);
                return Handled::Yes;
            }
            return Handled::No;
        }
        let Some(key) = event.as_key() else {
            return Handled::No;
        };
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
    use rabbitui_core::style::Style;
    use rabbitui_core::theme::{Role, Theme};
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
        assert_eq!(button.get_style(), Some(style));
    }

    #[test]
    fn filled_button_paints_a_solid_tonal_fill() {
        let theme = Theme::catppuccin_mocha();
        let mut buffer = Buffer::new(Size::new(11, 1));
        let mut ctx =
            RenderCtx::new_themed(&mut buffer, Rect::from_size(Size::new(11, 1)), false, &theme);
        Button::new("OK").filled(true).render(&mut (), &mut ctx);

        // Every cell carries the darker fill — a solid chip, not bare text; the
        // label cells keep it too (transparent-paint composition).
        let accent = theme.style(Role::Accent).fg.expect("accent has a color");
        let fill = accent.darken(0.55);
        for x in 0..11 {
            assert_eq!(
                cell_style(&buffer, x).bg,
                Some(fill),
                "cell {x} should carry the tonal fill"
            );
        }
        // The label is centered ("OK" width 2 in width 11 → starts at col 4) and
        // inked in the base hue.
        assert_eq!(buffer.get(Position::new(4, 0)).unwrap().symbol, "O");
        assert_eq!(cell_style(&buffer, 4).fg, Some(accent));
    }

    #[test]
    fn renders_label_in_text_role_when_unfocused() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(4, 1)), false);
        Button::new("Go").render(&mut (), &mut ctx);
        assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "G");
        assert_eq!(cell_style(&buffer, 0), Theme::default().style(Role::Text));
    }

    #[test]
    fn renders_highlight_role_when_focused() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(4, 1)), true);
        Button::new("Go").render(&mut (), &mut ctx);
        assert_eq!(
            cell_style(&buffer, 0),
            Theme::default().style(Role::Highlight)
        );
    }

    #[test]
    fn literal_style_overrides_unfocused_but_focus_uses_highlight() {
        let base = Style::new().fg(rabbitui_core::style::Color::RED).bold();
        // Unfocused: the literal override wins.
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(4, 1)), false);
        Button::new("Go").style(base).render(&mut (), &mut ctx);
        assert_eq!(cell_style(&buffer, 0), base);
        // Focused: highlight role still applies, so focus stays visible.
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(Size::new(4, 1)), true);
        Button::new("Go").style(base).render(&mut (), &mut ctx);
        assert_eq!(
            cell_style(&buffer, 0),
            Theme::default().style(Role::Highlight)
        );
    }

    fn dispatch(event: InputEvent) -> (Handled, Vec<Outcome>) {
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let handled = {
            let mut ctx = HandleCtx::new(
                Phase::Bubble,
                Rect::default(),
                &mut outcomes,
                &mut request_focus,
            );
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

    #[test]
    fn left_click_activates() {
        use rabbitui_core::geometry::Position;
        use rabbitui_core::input::{MouseButton, MouseEvent, MouseKind};
        let click = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Down,
            MouseButton::Left,
            Position::ORIGIN,
        ));
        let (handled, outcomes) = dispatch(click);
        assert_eq!(handled, Handled::Yes);
        assert_eq!(outcomes, vec![Outcome::Activated]);
    }

    #[test]
    fn mouse_release_does_not_activate() {
        use rabbitui_core::geometry::Position;
        use rabbitui_core::input::{MouseButton, MouseEvent, MouseKind};
        let release = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Up,
            MouseButton::Left,
            Position::ORIGIN,
        ));
        let (handled, outcomes) = dispatch(release);
        assert_eq!(handled, Handled::No);
        assert!(outcomes.is_empty());
    }
}
