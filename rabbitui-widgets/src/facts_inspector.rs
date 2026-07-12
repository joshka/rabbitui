//! A devtools overlay that renders the current frame's facts tree (ADR arc4 §7).
//!
//! `FactsInspector` is the on-screen half of the devtools facts inspector; the
//! off-screen half is [`facts::dump`](rabbitui_core::facts::dump), which writes
//! the **same** lines to the log seam. Given the previous frame's
//! [`FrameFacts`] — every declared widget's id
//! path, area, layer, focusability, visibility request, and the focused id — it
//! paints them one per row inside a themed [`Panel`], newest declaration order top
//! to bottom (= paint / z order). Read-only in v1: no pick-to-highlight.
//!
//! # Devtools-gated, borrows core facts only
//!
//! Behind the crate's `devtools` feature (which turns on `rabbitui-core/devtools`
//! so the `id → name` side table exists). Like [`LogOverlay`](crate::LogOverlay),
//! it takes a *core* type — `&FrameFacts` — not a facade type, so the widgets
//! crate stays runtime-free (ADR 0011) and the inspector is testable headlessly by
//! building facts directly.
//!
//! # Wiring
//!
//! An app toggles it with a chord (the gallery/flagship use Ctrl-D) and declares
//! it into a top [`Frame::layer`](rabbitui_core::frame::Frame::layer), passing the
//! facts the runtime kept from the *previous* frame and the current focus. Because
//! it reads last frame's facts, it never sees itself.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::facts::FrameFacts;
//! use rabbitui_widgets::FactsInspector;
//!
//! let facts = FrameFacts::new();
//! let inspector = FactsInspector::new(&facts).focus(None).title("facts");
//! assert_eq!(inspector.get_title(), "facts");
//! ```

use rabbitui_core::facts::FrameFacts;
use rabbitui_core::geometry::{Position, Size};
use rabbitui_core::id::WidgetId;
use rabbitui_core::theme::Role;
use rabbitui_core::widget::{RenderContext, Widget};

use crate::Panel;

/// A read-only overlay that paints a [`FrameFacts`] tree in a themed panel.
///
/// Borrows the facts (the runtime keeps the previous frame's); a per-frame spec
/// like every widget, stateless (`State = ()`) and never focusable — a passive
/// diagnostic, not a control. See the module docs for layout and wiring.
#[derive(Debug, Clone)]
pub struct FactsInspector<'a> {
    /// The facts tree to render (the previous frame's, from the runtime).
    facts: &'a FrameFacts,
    /// The currently-focused id, marked `[F]` in the listing.
    focus: Option<WidgetId>,
    /// The panel title (defaults to an entry count).
    title: Option<&'a str>,
}

impl<'a> FactsInspector<'a> {
    /// Creates an inspector over `facts`, titled with a live entry count and with
    /// no focus marker until [`focus`](Self::focus) sets one.
    #[must_use]
    pub fn new(facts: &'a FrameFacts) -> Self {
        Self {
            facts,
            focus: None,
            title: None,
        }
    }

    /// Sets the focused id, so its row is marked `[F]` in the listing.
    #[must_use]
    pub const fn focus(mut self, focus: Option<WidgetId>) -> Self {
        self.focus = focus;
        self
    }

    /// Sets a fixed panel title, replacing the default entry count.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// The configured title, or `"facts"` when the default count title is used.
    #[must_use]
    pub fn get_title(&self) -> &str {
        self.title.unwrap_or("facts")
    }
}

impl Widget for FactsInspector<'_> {
    type State = ();

    fn render(&self, (): &mut (), ctx: &mut RenderContext<'_>) {
        // A passive readout: never a focus target.
        ctx.focusable(false);

        let size = ctx.size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        // 1. The backdrop: a titled, bordered panel matching the catalog chrome.
        let count = self.facts.len();
        let count_title = format!("{count} facts");
        let title = self.title.unwrap_or(&count_title);
        let panel = Panel::new().title(title);
        panel.render(&mut (), ctx);

        let inner = Panel::inner(rect(size), &panel);
        if inner.size.width == 0 || inner.size.height == 0 {
            return;
        }

        // 2. The lines: the exact `dump_lines` format the log seam uses, so the
        //    overlay and `facts::dump` read identically. One entry per row, clipped
        //    to the inner height; the RenderContext clips each line at the right edge.
        let lines = self.facts.dump_lines(self.focus);
        let text = ctx.style(Role::Text);
        let accent = ctx.style(Role::Accent);
        let rows = usize::from(inner.size.height);
        for (row, line) in lines.iter().take(rows).enumerate() {
            let y = inner.origin.y + u16::try_from(row).unwrap_or(u16::MAX);
            // A focused row (its marker is `[F]`) is tinted with Accent so it stands
            // out; every other row is plain Text.
            let style = if line.starts_with("[F]") {
                accent
            } else {
                text
            };
            ctx.set_string(Position::new(inner.origin.x, y), line, style);
        }
    }
}

/// The full rectangle of a `size`-cell area, for [`Panel::inner`].
fn rect(size: Size) -> rabbitui_core::geometry::Rect {
    rabbitui_core::geometry::Rect::from_size(size)
}

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::facts::FrameFacts;
    use rabbitui_core::frame::Frame;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::id::{WidgetId, key};
    use rabbitui_core::store::StateStore;
    use rabbitui_core::theme::{Role, Theme};
    use rabbitui_core::widget::{RenderContext, Widget};

    use super::FactsInspector;

    /// A focusable leaf, so the facts carry a focus target and the `focusable` tag.
    struct Focusable;
    impl Widget for Focusable {
        type State = ();
        fn render(&self, _s: &mut (), ctx: &mut RenderContext<'_>) {
            ctx.focusable(true);
        }
    }
    struct Passive;
    impl Widget for Passive {
        type State = ();
        fn render(&self, _s: &mut (), _ctx: &mut RenderContext<'_>) {}
    }

    /// Declares a small realistic frame and returns its facts (with devtools names).
    fn sample_facts() -> FrameFacts {
        let mut buffer = Buffer::new(Size::new(40, 6));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.widget(
            key("banner"),
            Rect::new(Position::ORIGIN, Size::new(40, 1)),
            &Passive,
        );
        frame.scoped(key("sidebar"), |f| {
            f.widget(
                key("list"),
                Rect::new(Position::new(0, 1), Size::new(8, 4)),
                &Focusable,
            );
        });
        let facts = frame.finish();
        store.end_frame();
        facts
    }

    /// Renders the inspector into a fresh `size` buffer against `theme`.
    fn render(facts: &FrameFacts, focus: Option<WidgetId>, size: Size, theme: &Theme) -> Buffer {
        let mut buffer = Buffer::new(size);
        let mut ctx = RenderContext::new_themed(&mut buffer, Rect::from_size(size), false, theme);
        FactsInspector::new(facts)
            .focus(focus)
            .render(&mut (), &mut ctx);
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

    /// The inner text of a bordered row: drop the left/right border cells and
    /// trailing pad, so the assertion is on the rendered line content, not on the
    /// exact panel width's padding.
    fn inner_row(buffer: &Buffer, y: u16) -> String {
        let w = buffer.size().width;
        let mut line = String::new();
        for x in 1..w.saturating_sub(1) {
            line.push_str(&buffer.get(Position::new(x, y)).unwrap().symbol);
        }
        line.trim_end().to_string()
    }

    #[test]
    fn snapshot_renders_the_facts_tree_in_a_titled_panel() {
        // A panel wide enough that neither fact line is clipped.
        let facts = sample_facts();
        let list = WidgetId::ROOT.child(key("sidebar")).child(key("list"));
        let buffer = render(
            &facts,
            Some(list),
            Size::new(50, 4),
            &Theme::catppuccin_mocha(),
        );
        // The titled top border and matching bottom border.
        assert!(row(&buffer, 0).starts_with("┌ 2 facts "));
        assert!(row(&buffer, 0).ends_with('┐'));
        assert_eq!(row(&buffer, 3), format!("└{}┘", "─".repeat(48)));
        // The two facts, in declaration (= paint) order: the focused list is marked.
        assert_eq!(inner_row(&buffer, 1), "[ ] banner  L0  area=0,0 40x1");
        assert_eq!(
            inner_row(&buffer, 2),
            "[F] sidebar/list  L0  focusable  area=0,1 8x4"
        );
    }

    #[test]
    fn focused_row_is_accent_tinted() {
        let facts = sample_facts();
        let list = WidgetId::ROOT.child(key("sidebar")).child(key("list"));
        let theme = Theme::catppuccin_mocha();
        let buffer = render(&facts, Some(list), Size::new(50, 4), &theme);
        // Row 2 is the focused list entry: its first inner cell paints in Accent.
        let cell = buffer.get(Position::new(1, 2)).unwrap();
        assert_eq!(cell.symbol, "[");
        assert_eq!(cell.style.fg, theme.style(Role::Accent).fg);
        // Row 1 (the unfocused banner) paints in the plain Text role instead.
        let banner = buffer.get(Position::new(1, 1)).unwrap();
        assert_eq!(banner.style.fg, theme.style(Role::Text).fg);
    }

    #[test]
    fn empty_facts_still_paint_the_panel() {
        let facts = FrameFacts::new();
        let buffer = render(&facts, None, Size::new(16, 3), &Theme::default());
        assert!(row(&buffer, 0).starts_with("┌ 0 facts"));
        assert_eq!(row(&buffer, 2), "└──────────────┘");
    }
}
