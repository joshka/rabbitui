//! A dismissible error banner — the recommended surface for a recoverable failure.
//!
//! An opaque, [`Danger`](Role::Danger)-bordered box with a title in its top border,
//! a word-wrapped message, and a dismiss hint in its bottom border. Declare it on
//! a [`Frame::layer`](rabbitui_core::frame::Frame::layer) so it overlays the app;
//! it fills its own background (so content behind it does not show through) and is
//! focusable, emitting [`Outcome::Dismissed`] on Enter, Space, or a click — the app
//! clears its error state in response, and the banner simply is not declared next
//! frame.
//!
//! This is the widget the "when things fail" story recommends for an
//! `Event::EffectFailed` (from the `rabbitui` facade) or any app-level error.
//!
//! # Examples
//!
//! ```
//! use rabbitui_widgets::ErrorBanner;
//!
//! let banner = ErrorBanner::new("the request timed out").title("Network error");
//! assert_eq!(banner.message(), "the request timed out");
//! ```

use rabbitui_core::geometry::{Position, Size};
use rabbitui_core::input::{InputEvent, Key, MouseButton, MouseKind};
use rabbitui_core::outcome::Outcome;
use rabbitui_core::style::Style;
use rabbitui_core::theme::Role;
use rabbitui_core::widget::{HandleCtx, Handled, RenderCtx, Widget};

/// The hint shown in the bottom border.
const HINT: &str = " Enter: dismiss ";

/// A dismissible error banner.
#[derive(Debug, Clone, Copy)]
pub struct ErrorBanner<'a> {
    /// The message body.
    message: &'a str,
    /// The title shown in the top border.
    title: &'a str,
}

impl<'a> ErrorBanner<'a> {
    /// A banner showing `message`, titled `Error`.
    #[must_use]
    pub const fn new(message: &'a str) -> Self {
        Self {
            message,
            title: "Error",
        }
    }

    /// Sets the title shown in the top border (default `Error`).
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    /// The message body.
    #[must_use]
    pub const fn message(&self) -> &'a str {
        self.message
    }

    /// The title.
    #[must_use]
    pub const fn get_title(&self) -> &'a str {
        self.title
    }

    /// The message wrapped to `inner_width` columns (word boundaries, then a hard
    /// break for an overlong word). ASCII-oriented — adequate for error prose; a
    /// grapheme-correct pass can replace it when a message needs it.
    fn wrap(&self, inner_width: u16) -> Vec<String> {
        let width = usize::from(inner_width).max(1);
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        for word in self.message.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.chars().count() + 1 + word.chars().count() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(std::mem::take(&mut current));
                current.push_str(word);
            }
            // Hard-break a single word longer than the line.
            while current.chars().count() > width {
                let head: String = current.chars().take(width).collect();
                current = current.chars().skip(width).collect();
                lines.push(head);
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    }
}

impl Widget for ErrorBanner<'_> {
    type State = ();

    fn render(&self, (): &mut (), ctx: &mut RenderCtx<'_>) {
        ctx.focusable(true);
        let size = ctx.size();
        if size.width < 2 || size.height < 2 {
            return;
        }

        // Resolve every role first, then paint (each `style` call borrows `ctx`).
        let surface = ctx.style(Role::Surface);
        let danger = ctx.style(Role::Danger);
        let muted = ctx.style(Role::Muted);
        let text = ctx.style(Role::Text);

        // Opaque backdrop: fill with the surface role so content behind the layer
        // does not show through.
        let blank = " ".repeat(usize::from(size.width));
        for y in 0..size.height {
            ctx.set_string(Position::new(0, y), &blank, surface);
        }

        draw_border(ctx, size, danger);
        draw_top_label(ctx, size, &format!(" ⚠ {} ", self.title), danger);
        draw_bottom_label(ctx, size, HINT, muted);

        // The wrapped message, inset one cell from the border, in the text role.
        let inner_width = size.width - 2;
        for (row, line) in self.wrap(inner_width).into_iter().enumerate() {
            let Ok(row) = u16::try_from(row + 1) else {
                break;
            };
            if row >= size.height - 1 {
                break;
            }
            ctx.set_string(Position::new(1, row), &line, text);
        }
    }

    fn desired_height(&self, (): &(), width: u16) -> u16 {
        let inner_width = width.saturating_sub(2).max(1);
        let lines = u16::try_from(self.wrap(inner_width).len()).unwrap_or(1);
        // message rows + top and bottom border.
        lines.saturating_add(2)
    }

    fn handle((): &mut (), event: &InputEvent, ctx: &mut HandleCtx<'_>) -> Handled {
        if let Some(mouse) = event.as_mouse() {
            if mouse.button == MouseButton::Left && mouse.kind == MouseKind::Down {
                ctx.emit(Outcome::Dismissed);
                return Handled::Yes;
            }
            return Handled::No;
        }
        let Some(key) = event.as_key() else {
            return Handled::No;
        };
        match key.key {
            Key::Enter | Key::Char(' ') => {
                ctx.emit(Outcome::Dismissed);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

// Box-drawing characters (kept local; Panel's set is private to that module).
const TOP_LEFT: &str = "┌";
const TOP_RIGHT: &str = "┐";
const BOTTOM_LEFT: &str = "└";
const BOTTOM_RIGHT: &str = "┘";
const HORIZONTAL: &str = "─";
const VERTICAL: &str = "│";

/// Draws a full box border in `style`.
fn draw_border(ctx: &mut RenderCtx<'_>, size: Size, style: Style) {
    let last_x = size.width - 1;
    let last_y = size.height - 1;
    ctx.set_string(Position::new(0, 0), TOP_LEFT, style);
    ctx.set_string(Position::new(last_x, 0), TOP_RIGHT, style);
    ctx.set_string(Position::new(0, last_y), BOTTOM_LEFT, style);
    ctx.set_string(Position::new(last_x, last_y), BOTTOM_RIGHT, style);
    let run = HORIZONTAL.repeat(usize::from(size.width - 2));
    ctx.set_string(Position::new(1, 0), &run, style);
    ctx.set_string(Position::new(1, last_y), &run, style);
    for y in 1..last_y {
        ctx.set_string(Position::new(0, y), VERTICAL, style);
        ctx.set_string(Position::new(last_x, y), VERTICAL, style);
    }
}

/// Writes `label` into the top border after the corner, clipped to fit.
fn draw_top_label(ctx: &mut RenderCtx<'_>, size: Size, label: &str, style: Style) {
    let max = usize::from(size.width.saturating_sub(2));
    let clipped: String = label.chars().take(max).collect();
    ctx.set_string(Position::new(1, 0), &clipped, style);
}

/// Writes `label` into the bottom border after the corner, clipped to fit.
fn draw_bottom_label(ctx: &mut RenderCtx<'_>, size: Size, label: &str, style: Style) {
    let max = usize::from(size.width.saturating_sub(2));
    let clipped: String = label.chars().take(max).collect();
    ctx.set_string(Position::new(1, size.height - 1), &clipped, style);
}

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::input::{InputEvent, Key, MouseButton, MouseEvent, MouseKind};
    use rabbitui_core::outcome::Outcome;
    use rabbitui_core::widget::{HandleCtx, Handled, Phase, RenderCtx, Widget};

    use super::ErrorBanner;

    /// The symbols of one buffer row, concatenated.
    fn row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.size().width)
            .map(|x| buffer.get(Position::new(x, y)).map_or(String::new(), |c| c.symbol.to_string()))
            .collect()
    }

    fn render(banner: &ErrorBanner<'_>, size: Size) -> Buffer {
        let mut buffer = Buffer::new(size);
        let mut ctx = RenderCtx::new(&mut buffer, Rect::from_size(size), false);
        banner.render(&mut (), &mut ctx);
        buffer
    }

    fn dispatch(event: InputEvent) -> (Handled, Vec<Outcome>) {
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let handled = {
            let mut ctx = HandleCtx::new(Phase::Bubble, Rect::default(), &mut outcomes, &mut request_focus);
            ErrorBanner::handle(&mut (), &event, &mut ctx)
        };
        (handled, outcomes)
    }

    #[test]
    fn builder_keeps_message_and_title() {
        let banner = ErrorBanner::new("boom").title("Network error");
        assert_eq!(banner.message(), "boom");
        assert_eq!(banner.get_title(), "Network error");
        assert_eq!(ErrorBanner::new("x").get_title(), "Error", "default title");
    }

    #[test]
    fn renders_a_bordered_box_with_title_message_and_hint() {
        let buffer = render(&ErrorBanner::new("disk full").title("Save failed"), Size::new(24, 3));
        let top = row(&buffer, 0);
        assert!(top.starts_with('┌') && top.ends_with('┐'), "top border: {top}");
        assert!(top.contains("Save failed"), "title in top border: {top}");
        assert!(row(&buffer, 1).contains("disk full"), "message row");
        let bottom = row(&buffer, 2);
        assert!(bottom.starts_with('└') && bottom.ends_with('┘'), "bottom border");
        assert!(bottom.contains("dismiss"), "dismiss hint in bottom border");
    }

    #[test]
    fn a_long_message_wraps_and_grows_the_height() {
        let banner = ErrorBanner::new("the network request timed out after thirty seconds");
        // Inner width 18 (box 20) wraps the message across several rows.
        let lines = banner.wrap(18);
        assert!(lines.len() > 1, "wraps: {lines:?}");
        assert!(lines.iter().all(|l| l.chars().count() <= 18), "no line exceeds width");
        assert_eq!(
            banner.desired_height(&(), 20),
            u16::try_from(lines.len()).unwrap() + 2,
            "height is wrapped lines plus two borders"
        );
    }

    #[test]
    fn enter_space_and_click_dismiss() {
        assert_eq!(dispatch(InputEvent::key(Key::Enter)), (Handled::Yes, vec![Outcome::Dismissed]));
        assert_eq!(dispatch(InputEvent::key(Key::Char(' '))), (Handled::Yes, vec![Outcome::Dismissed]));
        let click = InputEvent::Mouse(MouseEvent {
            kind: MouseKind::Down,
            button: MouseButton::Left,
            position: Position::new(1, 1),
            modifiers: rabbitui_core::input::Modifiers::default(),
        });
        assert_eq!(dispatch(click), (Handled::Yes, vec![Outcome::Dismissed]));
    }

    #[test]
    fn an_unrelated_key_is_not_handled() {
        assert_eq!(dispatch(InputEvent::key(Key::Char('x'))), (Handled::No, vec![]));
    }
}
