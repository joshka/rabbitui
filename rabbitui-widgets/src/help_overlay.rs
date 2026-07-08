//! A help overlay generated from a [`Keymap`](rabbitui_core::keymap::Keymap):
//! two aligned columns (chord, action label) in a titled [`Panel`], meant to sit
//! in a [`Frame::layer`](rabbitui_core::frame::Frame::layer).
//!
//! `HelpOverlay` is the framework generalization of the flagship's in-app
//! `view_help` (`rabbitui-agent/src/app.rs`), the widget half of Arc 4 §3
//! (`docs/plans/arc4-spine.md`). Where the flagship hand-rolled the layout into
//! its `view`, this widget encapsulates it: give it the keymap's help rows and
//! it paints a two-column reference card that fits its area.
//!
//! # Generated from the one table
//!
//! The rows come straight from
//! [`Keymap::help_rows`](rabbitui_core::keymap::Keymap::help_rows) — there is no
//! hand-maintained list. Change a binding in the keymap and this overlay
//! follows, because both dispatch and help read the same table. The overlay
//! either borrows those rows directly ([`HelpOverlay::new`]) or builds them from
//! a keymap and a label function ([`HelpOverlay::from_keymap`]).
//!
//! # Display-only — it takes NO focus
//!
//! The overlay holds no focusable widget and calls `focusable(false)`: it is a
//! passive reference card, not a control. A [`Panel`] is chrome and panics if an
//! app tries to focus it (the declare-then-focus contract), so **the app routes
//! the close keys itself** at the app level — the overlay never steals input.
//! This matches the flagship, which routes Esc / the Help chord / Ctrl-C around
//! the overlay while the composer keeps focus underneath.
//!
//! # Responsive / fit-the-area
//!
//! Like the flagship's `view_help`, everything derives from the render area, so
//! a resize just recomputes. The chord column is padded to the widest chord so
//! the action labels align. When the area is too short for every row, the list
//! truncates with an "…and N more" summary on the last inner row rather than
//! clipping the panel chrome — and a long row clips at the right edge rather
//! than overflowing.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::geometry::{Rect, Size};
//! use rabbitui_core::id::key;
//! use rabbitui_core::keymap::{Binding, Chord, Keymap};
//! # use rabbitui_core::store::StateStore;
//! use rabbitui_widgets::HelpOverlay;
//!
//! #[derive(Clone, Copy, PartialEq, Eq)]
//! enum Action { Quit, Help }
//! fn label(a: Action) -> &'static str {
//!     match a { Action::Quit => "quit", Action::Help => "toggle this help" }
//! }
//!
//! static QUIT: &[Chord] = &[Chord::ctrl('c')];
//! static HELP: &[Chord] = &[Chord::ctrl('/'), Chord::ctrl('g')];
//! static BINDINGS: &[Binding<Action>] =
//!     &[Binding::new(Action::Quit, QUIT), Binding::new(Action::Help, HELP)];
//! let keymap = Keymap::new(BINDINGS);
//!
//! # let mut buffer = rabbitui_core::buffer::Buffer::new(Size::new(40, 8));
//! # let mut store = StateStore::new();
//! # store.begin_frame();
//! # let mut frame = Frame::new(&mut buffer, &mut store);
//! // On an overlay layer, over the app's base view.
//! frame.layer(key("help"), |overlay| {
//!     let area = Rect::from_size(Size::new(40, 8));
//!     overlay.widget(key("panel"), area, &HelpOverlay::from_keymap(&keymap, label));
//! });
//! # let _ = frame.finish();
//! # store.end_frame();
//! ```

use rabbitui_core::a11y::SemanticRole;
use rabbitui_core::geometry::{Position, Rect};
use rabbitui_core::keymap::Keymap;
use rabbitui_core::theme::Role;
use rabbitui_core::widget::{RenderCtx, Widget};

use crate::Panel;

/// The default gap between the chord column and the action column, in cells.
/// (Matches the flagship's `COLUMN_GAP`.)
const COLUMN_GAP: usize = 2;

/// The default panel title. The `Esc` hint documents the close key the *app*
/// routes (the overlay takes no focus and consumes nothing itself).
const DEFAULT_TITLE: &str = "keys — Esc to close";

/// A display-only help overlay: a titled panel of two aligned columns
/// (`chord   action`), generated from a keymap's bindings.
///
/// Holds its rows as `(chord-column, label)` pairs — either borrowed from a
/// prepared [`help_rows`](rabbitui_core::keymap::Keymap::help_rows) slice, or
/// built once by [`from_keymap`](Self::from_keymap). Stateless (`State = ()`) and
/// never focusable; see the module docs for why the app owns the close keys.
///
/// # Examples
///
/// ```
/// use rabbitui_widgets::HelpOverlay;
///
/// let rows = vec![("Ctrl-T".to_string(), "toggle mode")];
/// let overlay = HelpOverlay::new(&rows).title("shortcuts");
/// assert_eq!(overlay.get_title(), "shortcuts");
/// assert_eq!(overlay.len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct HelpOverlay<'a> {
    /// The `(chord-column, action-label)` rows, in display order.
    rows: Cow<'a, [(String, &'a str)]>,
    /// The panel title.
    title: &'a str,
    /// The gap between the chord and action columns.
    gap: usize,
}

impl<'a> HelpOverlay<'a> {
    /// A help overlay over pre-built `rows`, the output of
    /// [`Keymap::help_rows`](rabbitui_core::keymap::Keymap::help_rows).
    ///
    /// Borrows the rows, so keep the `help_rows` result alive for the render.
    /// The rows are `(chord-column, action-label)` — the chord column already
    /// joins an action's chords with ` / `.
    #[must_use]
    pub fn new(rows: &'a [(String, &'a str)]) -> Self {
        Self {
            rows: Cow::Borrowed(rows),
            title: DEFAULT_TITLE,
            gap: COLUMN_GAP,
        }
    }

    /// A help overlay built from `keymap` and a `label` function, owning its
    /// rows.
    ///
    /// The convenience constructor: it calls
    /// [`Keymap::help_rows`](rabbitui_core::keymap::Keymap::help_rows) for you
    /// and holds the result, so a view can build the overlay inline without a
    /// separate `let rows = …`. To exclude some actions (e.g. modal-only ones),
    /// build the keymap over a narrower binding slice first.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::keymap::{Binding, Chord, Keymap};
    /// use rabbitui_widgets::HelpOverlay;
    ///
    /// #[derive(Clone, Copy, PartialEq, Eq)]
    /// enum Action { Quit }
    /// fn label(a: Action) -> &'static str { match a { Action::Quit => "quit" } }
    ///
    /// static QUIT: &[Chord] = &[Chord::ctrl('c')];
    /// static BINDINGS: &[Binding<Action>] = &[Binding::new(Action::Quit, QUIT)];
    /// let overlay = HelpOverlay::from_keymap(&Keymap::new(BINDINGS), label);
    /// assert_eq!(overlay.len(), 1);
    /// ```
    #[must_use]
    pub fn from_keymap<A: Copy>(
        keymap: &Keymap<'_, A>,
        label: impl Fn(A) -> &'static str,
    ) -> HelpOverlay<'static> {
        // `help_rows` yields owned `(String, &'static str)` pairs; the labels are
        // `'static` (the flagship's `Action::label` returns `&'static str`), so
        // the built overlay is `HelpOverlay<'static>` — it owns its rows and
        // borrows nothing from the caller.
        let rows = keymap.help_rows(label);
        HelpOverlay {
            rows: Cow::Owned(rows),
            title: DEFAULT_TITLE,
            gap: COLUMN_GAP,
        }
    }

    /// Sets the panel title (defaults to `"keys — Esc to close"`).
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    /// Sets the gap, in cells, between the chord and action columns (default 2).
    #[must_use]
    pub const fn column_gap(mut self, gap: usize) -> Self {
        self.gap = gap;
        self
    }

    /// The configured panel title.
    #[must_use]
    pub const fn get_title(&self) -> &str {
        self.title
    }

    /// The number of binding rows the overlay lists.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the overlay has no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The width, in cells, of the widest chord column across all rows.
    fn widest_chord(&self) -> usize {
        self.rows.iter().map(|(chord, _)| chord.len()).max().unwrap_or(0)
    }
}

impl Widget for HelpOverlay<'_> {
    type State = ();

    fn desired_height(&self, (): &(), _width: u16) -> u16 {
        // Panel chrome (border, no padding here) is 2 rows; one row per binding.
        let chrome = 2u16;
        let rows = u16::try_from(self.rows.len()).unwrap_or(u16::MAX);
        chrome.saturating_add(rows).max(chrome + 1)
    }

    fn render(&self, (): &mut (), ctx: &mut RenderCtx<'_>) {
        // Display-only: never a focus target (focusing the Panel would panic).
        ctx.focusable(false);
        // A11y groundwork (ADR arc4 §5): a modal-like reference readout.
        ctx.semantic_role(SemanticRole::Dialog);

        let size = ctx.size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        // 1. The backdrop: a titled, bordered panel so the overlay matches the
        //    rest of the catalog's chrome and re-skins with the theme.
        let panel = Panel::new().title(self.title);
        panel.render(&mut (), ctx);
        let inner = Panel::inner(Rect::from_size(size), &panel);
        if inner.size.width == 0 || inner.size.height == 0 {
            return;
        }

        // 2. Fit the rows to the inner area (the flagship's fit-the-area rule):
        //    reserve the last inner row for an "…and N more" summary when the
        //    list is too tall to show every binding.
        let capacity = inner.size.height;
        let total = u16::try_from(self.rows.len()).unwrap_or(u16::MAX);
        let truncated = total > capacity;
        let listed = if truncated {
            usize::from(capacity.saturating_sub(1))
        } else {
            self.rows.len()
        };

        // 3. Two aligned columns: pad the chord to the widest chord, then the gap,
        //    then the label. `set_string` writes at a position relative to the
        //    widget area and clips at the right edge, so a long row truncates
        //    rather than overflowing (the flagship's fit-the-area rule).
        let widest = self.widest_chord();
        let gap = " ".repeat(self.gap);
        // The whole row is Accent so the chord reads as the actionable part
        // (matches the flagship); the panel already painted the surface behind it.
        let accent = ctx.style(Role::Accent);
        for (row, (chord, label)) in self.rows.iter().take(listed).enumerate() {
            let y = inner.origin.y + u16::try_from(row).unwrap_or(u16::MAX);
            let line = format!("{chord:<widest$}{gap}{label}");
            ctx.set_string(Position::new(inner.origin.x, y), &line, accent);
        }

        if truncated {
            let more = self.rows.len() - listed;
            let y = inner.origin.y + u16::try_from(listed).unwrap_or(u16::MAX);
            let muted = ctx.style(Role::Muted);
            ctx.set_string(
                Position::new(inner.origin.x, y),
                &format!("…and {more} more"),
                muted,
            );
        }
    }
}

use std::borrow::Cow;

#[cfg(test)]
mod tests {
    use rabbitui_core::buffer::Buffer;
    use rabbitui_core::geometry::{Position, Rect, Size};
    use rabbitui_core::input::Key;
    use rabbitui_core::keymap::{Binding, Chord, Keymap};
    use rabbitui_core::theme::Theme;
    use rabbitui_core::widget::{RenderCtx, Widget};

    use super::HelpOverlay;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Action {
        ToggleMode,
        Help,
        Quit,
    }

    fn label(action: Action) -> &'static str {
        match action {
            Action::ToggleMode => "toggle inline / browse mode",
            Action::Help => "toggle this help",
            Action::Quit => "quit",
        }
    }

    static TOGGLE: &[Chord] = &[Chord::ctrl('t')];
    static HELP: &[Chord] = &[Chord::ctrl('/'), Chord::ctrl('g')];
    static QUIT: &[Chord] = &[Chord::ctrl('c')];
    static BINDINGS: &[Binding<Action>] = &[
        Binding::new(Action::ToggleMode, TOGGLE),
        Binding::new(Action::Help, HELP),
        Binding::new(Action::Quit, QUIT),
    ];

    fn keymap() -> Keymap<'static, Action> {
        Keymap::new(BINDINGS)
    }

    /// Renders an overlay into a fresh `size` buffer against `theme`.
    fn render(overlay: &HelpOverlay<'_>, size: Size, theme: &Theme) -> Buffer {
        let mut buffer = Buffer::new(size);
        let mut ctx = RenderCtx::new_themed(&mut buffer, Rect::from_size(size), false, theme);
        overlay.render(&mut (), &mut ctx);
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
    fn generated_from_keymap_lists_a_row_per_binding() {
        let overlay = HelpOverlay::from_keymap(&keymap(), label);
        assert_eq!(overlay.len(), 3);
    }

    #[test]
    fn renders_two_aligned_columns_inside_a_titled_panel() {
        let overlay = HelpOverlay::from_keymap(&keymap(), label);
        // Wide enough for the widest row; tall enough for all three + chrome.
        let buffer = render(&overlay, Size::new(48, 5), &Theme::default());
        let top = row(&buffer, 0);
        assert!(top.starts_with("┌ keys"), "title row: {top:?}");
        // The chord column is padded so labels align: the "Help" row's chord is
        // the widest ("Ctrl-/ / Ctrl-G"), so every label starts at the same col.
        let toggle = row(&buffer, 1);
        assert!(toggle.contains("Ctrl-T"), "toggle row: {toggle:?}");
        assert!(toggle.contains("toggle inline"), "toggle row: {toggle:?}");
        let help = row(&buffer, 2);
        assert!(help.contains("Ctrl-/ / Ctrl-G"), "help row: {help:?}");
        assert!(help.contains("toggle this help"), "help row: {help:?}");
    }

    #[test]
    fn columns_are_aligned_on_the_widest_chord() {
        let overlay = HelpOverlay::from_keymap(&keymap(), label);
        let buffer = render(&overlay, Size::new(48, 5), &Theme::default());
        // The label column starts at the same x on every row. Find where the
        // "Ctrl-T" toggle label begins vs the "Ctrl-C" quit label.
        let toggle = row(&buffer, 1);
        let quit = row(&buffer, 3);
        let toggle_label = toggle.find("toggle inline").unwrap();
        let quit_label = quit.find("quit").unwrap();
        assert_eq!(
            toggle_label, quit_label,
            "labels align: toggle@{toggle_label} quit@{quit_label}"
        );
    }

    #[test]
    fn a_short_area_truncates_with_an_and_more_summary() {
        let overlay = HelpOverlay::from_keymap(&keymap(), label);
        // 4 rows tall: 2 chrome + 2 inner. Three bindings don't fit → one row is
        // listed, the last inner row is the "…and N more" summary.
        let buffer = render(&overlay, Size::new(48, 4), &Theme::default());
        let first = row(&buffer, 1);
        assert!(first.contains("Ctrl-T"), "first listed row: {first:?}");
        let summary = row(&buffer, 2);
        assert!(summary.contains("…and 2 more"), "summary row: {summary:?}");
    }

    #[test]
    fn takes_no_focus() {
        // The overlay must declare no focusable — the app owns the close keys.
        let overlay = HelpOverlay::from_keymap(&keymap(), label);
        let mut buffer = Buffer::new(Size::new(40, 5));
        let mut ctx =
            RenderCtx::new(&mut buffer, Rect::from_size(Size::new(40, 5)), false);
        overlay.render(&mut (), &mut ctx);
        assert!(!ctx.is_focusable(), "the help overlay must not be focusable");
    }

    #[test]
    fn borrows_prepared_help_rows() {
        // The `new` constructor over a borrowed `help_rows` slice.
        let rows = keymap().help_rows(label);
        let overlay = HelpOverlay::new(&rows).title("shortcuts");
        assert_eq!(overlay.get_title(), "shortcuts");
        assert_eq!(overlay.len(), 3);
    }

    #[test]
    fn empty_keymap_still_paints_the_panel() {
        static NONE: &[Binding<Action>] = &[];
        let overlay = HelpOverlay::from_keymap(&Keymap::new(NONE), label);
        assert!(overlay.is_empty());
        // Wide enough for the framed title to fit.
        let buffer = render(&overlay, Size::new(28, 3), &Theme::default());
        assert!(row(&buffer, 0).starts_with("┌ keys"), "title: {:?}", row(&buffer, 0));
        assert_eq!(row(&buffer, 2), "└──────────────────────────┘");
    }

    #[test]
    fn a_bare_key_binding_renders_its_display_name() {
        // A bindings table with a bare Enter (a non-Ctrl chord) renders "Enter".
        static SEND: &[Chord] = &[Chord::bare(Key::Enter)];
        static ONE: &[Binding<Action>] = &[Binding::new(Action::ToggleMode, SEND)];
        let overlay = HelpOverlay::from_keymap(&Keymap::new(ONE), label);
        let buffer = render(&overlay, Size::new(40, 3), &Theme::default());
        assert!(row(&buffer, 1).contains("Enter"));
    }

    /// A snapshot of the whole overlay: an exact row-by-row picture proving the
    /// panel frame, title, aligned columns, and generated chords lay out as
    /// designed.
    #[test]
    fn overlay_snapshot() {
        let overlay = HelpOverlay::from_keymap(&keymap(), label);
        // 60 wide so the longest row fits without clipping at the right border.
        let buffer = render(&overlay, Size::new(60, 5), &Theme::catppuccin_mocha());
        assert_eq!(
            row(&buffer, 0),
            "┌ keys — Esc to close ─────────────────────────────────────┐"
        );
        // The chord column is padded to the widest chord ("Ctrl-/ / Ctrl-G" = 15),
        // then a two-space gap, so every label starts at the same column.
        assert_eq!(
            row(&buffer, 1),
            "│Ctrl-T           toggle inline / browse mode              │"
        );
        assert_eq!(
            row(&buffer, 2),
            "│Ctrl-/ / Ctrl-G  toggle this help                         │"
        );
        assert_eq!(
            row(&buffer, 3),
            "│Ctrl-C           quit                                     │"
        );
        assert_eq!(
            row(&buffer, 4),
            "└──────────────────────────────────────────────────────────┘"
        );
    }
}
