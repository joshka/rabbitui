//! A debug log overlay: renders the tail of a [`LogHandle`] ring in a themed
//! panel, meant to sit in a [`Frame::layer`](rabbitui_core::frame::Frame::layer).
//!
//! `LogOverlay` is the render half of the logging seam
//! (`docs/design/arc2b-measurement-scroll.md`). The framework collects `tracing`
//! events into a bounded [`LogHandle`] ring (the facade's `rabbitui::log`); this
//! widget reads that ring's tail and paints it as a translucent-looking debug
//! panel toggled by an app key (the examples use `~`).
//!
//! # The core-side handle keeps `tracing` out of this crate
//!
//! The overlay takes a [`LogHandle`] — the **core** ring-buffer handle — not a
//! `tracing` subscriber or any facade type. That is deliberate: `rabbitui-widgets`
//! depends only on [`rabbitui_core`] (ADR 0011), and dragging `tracing` in would
//! break that. The facade's `Collector` writes formatted [`LogRecord`]s into the
//! ring; this widget reads them back. Neither side needs the other's crate — they
//! meet at the core handle. So the overlay is fully testable here by pushing
//! records into a handle directly, with no subscriber and no async runtime.
//!
//! # Layout
//!
//! The overlay fills its area with a [`Panel`]-style backdrop (a
//! [`Role::Surface`] fill, a [`Role::Border`] frame, an "N logs" title) and paints
//! the last rows of the ring inside it, one record per row, newest at the bottom.
//! Each line reads `LEVEL target: message`, the level tinted by severity
//! ([`Role::Danger`] for errors, [`Role::Warning`] for warnings,
//! [`Role::Muted`] for the rest) so a warning is visible at a glance. Longer tails
//! than fit are clipped to the last visible rows; the message is clipped at the
//! right edge.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::geometry::{Rect, Size};
//! use rabbitui_core::id::key;
//! use rabbitui_core::log::{Level, LogHandle, LogRecord};
//! # use rabbitui_core::store::StateStore;
//! use rabbitui_widgets::LogOverlay;
//!
//! let logs = LogHandle::with_capacity(256);
//! logs.push(LogRecord::new(Level::Info, "app", "fetch started"));
//! logs.push(LogRecord::new(Level::Warn, "app", "slow response"));
//!
//! # let mut buffer = rabbitui_core::buffer::Buffer::new(Size::new(40, 8));
//! # let mut store = StateStore::new();
//! # store.begin_frame();
//! # let mut frame = Frame::new(&mut buffer, &mut store);
//! // Declared into an overlay layer, at the bottom of the screen.
//! frame.layer(key("logs"), |overlay| {
//!     let area = Rect::from_size(Size::new(40, 8));
//!     overlay.widget(key("panel"), area, &LogOverlay::new(&logs));
//! });
//! # let _ = frame.finish();
//! # store.end_frame();
//! ```

use rabbitui_core::geometry::{Position, Size};
use rabbitui_core::log::{Level, LogHandle, LogRecord};
use rabbitui_core::style::Style;
use rabbitui_core::theme::Role;
use rabbitui_core::widget::{RenderCtx, Widget};

use crate::Panel;

/// A debug overlay that paints the tail of a [`LogHandle`] in a themed panel.
///
/// Borrows the handle (a cheap `Arc` clone lives in the runtime); the widget is a
/// per-frame spec like every other, so it re-reads the ring's tail each render.
/// Stateless (`State = ()`) and never focusable — it is a passive readout, not a
/// control. See the module docs for the layout and the seam rationale.
///
/// # Examples
///
/// ```
/// use rabbitui_core::log::LogHandle;
/// use rabbitui_widgets::LogOverlay;
///
/// let logs = LogHandle::new();
/// let overlay = LogOverlay::new(&logs).title("debug log");
/// assert_eq!(overlay.get_title(), "debug log");
/// ```
#[derive(Debug, Clone)]
pub struct LogOverlay<'a> {
    /// The ring whose tail this overlay renders.
    handle: &'a LogHandle,
    /// The panel title (defaults to a record count).
    title: Option<&'a str>,
}

impl<'a> LogOverlay<'a> {
    /// Creates an overlay over `handle`, titled with a live record count.
    #[must_use]
    pub fn new(handle: &'a LogHandle) -> Self {
        Self {
            handle,
            title: None,
        }
    }

    /// Sets a fixed panel title, replacing the default record count.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// The configured title, or `"logs"` when the default count title is used.
    ///
    /// A builder-inspection accessor for tests; the rendered title is the record
    /// count unless [`title`](Self::title) set an explicit one.
    #[must_use]
    pub fn get_title(&self) -> &str {
        self.title.unwrap_or("logs")
    }

    /// The role a record's level tag paints in: danger for errors, warning for
    /// warnings, muted for everything quieter.
    fn level_role(level: Level) -> Role {
        match level {
            Level::Error => Role::Danger,
            Level::Warn => Role::Warning,
            Level::Trace | Level::Debug | Level::Info => Role::Muted,
        }
    }
}

impl Widget for LogOverlay<'_> {
    type State = ();

    fn render(&self, (): &mut (), ctx: &mut RenderCtx<'_>) {
        // A passive readout: never a focus target.
        ctx.focusable(false);

        let size = ctx.size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        // 1. The backdrop. A titled, bordered panel drawn by the Panel widget so
        //    the overlay matches the rest of the catalog's chrome and re-skins with
        //    the theme. The title is either the caller's or a live count.
        let count = self.handle.len();
        let count_title = format!("{count} logs");
        let title = self.title.unwrap_or(&count_title);
        let panel = Panel::new().title(title);
        panel.render(&mut (), ctx);

        let inner = Panel::inner(area_rect(size), &panel);
        if inner.size.width == 0 || inner.size.height == 0 {
            return;
        }

        // 2. The tail. Read the last `inner.height` records and paint them oldest
        //    at the top, newest at the bottom — a chat-log reading order.
        let rows = usize::from(inner.size.height);
        let tail = self.handle.tail(rows);
        let muted = ctx.style(Role::Muted);
        let text_style = ctx.style(Role::Text);
        for (row, record) in tail.iter().enumerate() {
            let y = inner.origin.y + u16::try_from(row).unwrap_or(u16::MAX);
            paint_record(
                ctx,
                Position::new(inner.origin.x, y),
                inner.size.width,
                record,
                ctx.style(Self::level_role(record.level)),
                muted,
                text_style,
            );
        }
    }
}

/// The full rectangle of a `size`-cell area, for `Panel::inner`.
fn area_rect(size: Size) -> rabbitui_core::geometry::Rect {
    rabbitui_core::geometry::Rect::from_size(size)
}

/// Paints one record on its row: `LEVEL` in `level_style`, ` target: ` muted, then
/// the message in the text style, all clipped to `width`.
#[allow(clippy::too_many_arguments)]
fn paint_record(
    ctx: &mut RenderCtx<'_>,
    origin: Position,
    width: u16,
    record: &LogRecord,
    level_style: Style,
    muted: Style,
    text_style: Style,
) {
    // The three runs share one row, each starting where the last left off; the
    // RenderCtx clips at the area's right edge, so a long message never overruns.
    let mut x = origin.x;
    let level = record.level.as_str();
    ctx.set_string(Position::new(x, origin.y), level, level_style);
    x = x.saturating_add(u16::try_from(level.len()).unwrap_or(0));

    let target = format!(" {}: ", record.target);
    ctx.set_string(Position::new(x, origin.y), &target, muted);
    x = x.saturating_add(u16::try_from(target.chars().count()).unwrap_or(0));

    if x < origin.x.saturating_add(width) {
        ctx.set_string(Position::new(x, origin.y), &record.message, text_style);
    }
}

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::log::{Level, LogHandle, LogRecord};
    use rabbitui_core::theme::{Role, Theme};
    use rabbitui_core::widget::{RenderCtx, Widget};

    use super::LogOverlay;

    /// Renders an overlay over `handle` into a fresh `size` buffer against `theme`.
    fn render(handle: &LogHandle, size: Size, theme: &Theme) -> Buffer {
        let mut buffer = Buffer::new(size);
        let mut ctx = RenderCtx::new_themed(&mut buffer, Rect::from_size(size), false, theme);
        LogOverlay::new(handle).render(&mut (), &mut ctx);
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
    fn renders_the_tail_inside_a_titled_panel() {
        let handle = LogHandle::with_capacity(64);
        handle.push(LogRecord::new(Level::Info, "app", "started"));
        handle.push(LogRecord::new(Level::Warn, "app", "slow"));
        // A 24x4 panel: one border row top and bottom, two inner rows for the two
        // records.
        let buffer = render(&handle, Size::new(24, 4), &Theme::default());
        // The title carries the live count.
        let top = row(&buffer, 0);
        assert!(top.starts_with("┌ 2 logs"), "title row: {top:?}");
        // The two records, oldest first.
        assert!(row(&buffer, 1).contains("INFO app: started"));
        assert!(row(&buffer, 2).contains("WARN app: slow"));
    }

    #[test]
    fn a_full_ring_shows_only_the_last_visible_rows() {
        let handle = LogHandle::with_capacity(64);
        for i in 0..20 {
            handle.push(LogRecord::new(Level::Info, "t", format!("line {i}")));
        }
        // 30x4 → two inner rows: only the last two records paint.
        let buffer = render(&handle, Size::new(30, 4), &Theme::default());
        assert!(row(&buffer, 1).contains("line 18"));
        assert!(row(&buffer, 2).contains("line 19"));
    }

    #[test]
    fn level_tag_is_tinted_by_severity() {
        let theme = Theme::catppuccin_mocha();
        let handle = LogHandle::with_capacity(8);
        handle.push(LogRecord::new(Level::Error, "t", "boom"));
        let buffer = render(&handle, Size::new(30, 3), &theme);
        // The "ERROR" tag at the inner origin (1,1) takes the Danger foreground and
        // sits on the overlay panel's surface fill (transparent-paint composition).
        let cell = buffer.get(Position::new(1, 1)).unwrap();
        assert_eq!(cell.symbol, "E");
        assert_eq!(cell.style.fg, theme.style(Role::Danger).fg);
        assert_eq!(cell.style.bg, theme.style(Role::Surface).bg);
    }

    #[test]
    fn empty_ring_still_paints_the_panel() {
        let handle = LogHandle::new();
        let buffer = render(&handle, Size::new(16, 3), &Theme::default());
        // No records, but the framed panel is drawn and titled "0 logs".
        assert!(row(&buffer, 0).starts_with("┌ 0 logs"));
        assert_eq!(row(&buffer, 2), "└──────────────┘");
    }

    #[test]
    fn a_custom_title_replaces_the_count() {
        let handle = LogHandle::new();
        let theme = Theme::default();
        let mut buffer = Buffer::new(Size::new(20, 3));
        let mut ctx =
            RenderCtx::new_themed(&mut buffer, Rect::from_size(Size::new(20, 3)), false, &theme);
        LogOverlay::new(&handle).title("debug").render(&mut (), &mut ctx);
        assert!(row(&buffer, 0).starts_with("┌ debug"));
    }

    /// A snapshot of the whole overlay: an exact row-by-row picture proving the
    /// panel frame, title, and tinted tail lay out as designed.
    #[test]
    fn overlay_snapshot() {
        let handle = LogHandle::with_capacity(16);
        handle.push(LogRecord::new(Level::Info, "fetch", "start"));
        handle.push(LogRecord::new(Level::Warn, "fetch", "retry"));
        let buffer = render(&handle, Size::new(26, 4), &Theme::catppuccin_mocha());
        assert_eq!(row(&buffer, 0), "┌ 2 logs ────────────────┐");
        assert_eq!(row(&buffer, 1), "│INFO fetch: start       │");
        assert_eq!(row(&buffer, 2), "│WARN fetch: retry       │");
        assert_eq!(row(&buffer, 3), "└────────────────────────┘");
    }
}
