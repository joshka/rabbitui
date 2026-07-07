//! Converting ratatui styles to rabbitui styles, cell by cell.
//!
//! ADR 0010 bets that the two style models "map field-for-field" because ADR
//! 0003 kept rabbitui's cell model convertible to ratatui's *by construction*.
//! This module is where that bet is cashed: a total, infallible per-cell
//! function from a ratatui [`Cell`](ratatui::buffer::Cell)'s style
//! ([`Color`](ratatui::style::Color) foreground/background plus a
//! [`Modifier`](ratatui::style::Modifier) bitset) to a rabbitui [`Style`].
//!
//! # Lossy corners (documented, never a panic)
//!
//! The map is total but not a bijection — ratatui can express a few things
//! rabbitui's [`Style`]/[`Attrs`] cannot. Per ADR 0010 §Decision.6 the bridge
//! *degrades* rather than withholds: it drops what has no analog and copies the
//! rest, silently, with the corners named here.
//!
//! - **`Modifier::SLOW_BLINK` / `Modifier::RAPID_BLINK`** — rabbitui has no
//!   blink attribute (blink is widely disabled in terminals and omitted from
//!   [`Attrs`]). Dropped.
//! - **`Modifier::HIDDEN`** — rabbitui has no conceal attribute. Dropped.
//! - **`underline_color`** — ratatui carries a *separate* underline color; a
//!   rabbitui [`Style`] has one foreground and one background and no underline
//!   color. Dropped (the `underline-color` cargo feature is left off so the
//!   field is always ratatui's default anyway).
//!
//! Everything else round-trips exactly: all four rabbitui [`Color`] variants
//! have a ratatui source, and the six attributes rabbitui models
//! (bold/dim/italic/underline/reversed/strikethrough) each have a `Modifier`
//! bit.

use rabbitui_core::style::{Attrs, Color, Style};
use ratatui::buffer::Cell;
use ratatui::style::{Color as RatColor, Modifier};

/// Converts a ratatui [`Color`](ratatui::style::Color) to a rabbitui [`Color`].
///
/// The map is total: every ratatui color has a rabbitui representation.
///
/// - `Reset` → [`Color::Reset`]
/// - the sixteen named colors (`Black`..=`White`, `DarkGray`, the `Light*`
///   set) → [`Color::Ansi`] with the color's ANSI index (0–15)
/// - `Indexed(n)` → [`Color::Indexed`]
/// - `Rgb(r, g, b)` → [`Color::Rgb`]
///
/// # Examples
///
/// ```
/// use rabbitui_core::style::Color;
/// use rabbitui_ratatui::style::convert_color;
/// use ratatui::style::Color as RatColor;
///
/// assert_eq!(convert_color(RatColor::Red), Color::Ansi(1));
/// assert_eq!(convert_color(RatColor::LightBlue), Color::Ansi(12));
/// assert_eq!(convert_color(RatColor::Rgb(1, 2, 3)), Color::Rgb(1, 2, 3));
/// assert_eq!(convert_color(RatColor::Reset), Color::Reset);
/// ```
#[must_use]
pub fn convert_color(color: RatColor) -> Color {
    match color {
        RatColor::Reset => Color::Reset,
        // The sixteen ANSI base colors, mapped to their palette indices so a
        // bridged widget's named colors land on the same 0–15 slots the encoder
        // (ADR 0012) degrades against.
        RatColor::Black => Color::Ansi(0),
        RatColor::Red => Color::Ansi(1),
        RatColor::Green => Color::Ansi(2),
        RatColor::Yellow => Color::Ansi(3),
        RatColor::Blue => Color::Ansi(4),
        RatColor::Magenta => Color::Ansi(5),
        RatColor::Cyan => Color::Ansi(6),
        RatColor::Gray => Color::Ansi(7),
        RatColor::DarkGray => Color::Ansi(8),
        RatColor::LightRed => Color::Ansi(9),
        RatColor::LightGreen => Color::Ansi(10),
        RatColor::LightYellow => Color::Ansi(11),
        RatColor::LightBlue => Color::Ansi(12),
        RatColor::LightMagenta => Color::Ansi(13),
        RatColor::LightCyan => Color::Ansi(14),
        RatColor::White => Color::Ansi(15),
        RatColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
        RatColor::Indexed(n) => Color::Indexed(n),
    }
}

/// Converts a ratatui [`Modifier`] bitset to rabbitui [`Attrs`].
///
/// The six attributes rabbitui models are copied bit-for-bit; the three ratatui
/// modifiers with no rabbitui analog (`SLOW_BLINK`, `RAPID_BLINK`, `HIDDEN`) are
/// dropped — see the module docs. This never fails: an unrepresentable modifier
/// is skipped, not an error.
///
/// # Examples
///
/// ```
/// use rabbitui_core::style::Attrs;
/// use rabbitui_ratatui::style::convert_modifier;
/// use ratatui::style::Modifier;
///
/// let attrs = convert_modifier(Modifier::BOLD | Modifier::ITALIC);
/// assert!(attrs.contains(Attrs::BOLD | Attrs::ITALIC));
/// // Blink has no rabbitui analog and is dropped.
/// assert!(convert_modifier(Modifier::SLOW_BLINK).is_empty());
/// ```
#[must_use]
pub fn convert_modifier(modifier: Modifier) -> Attrs {
    let mut attrs = Attrs::NONE;
    if modifier.contains(Modifier::BOLD) {
        attrs |= Attrs::BOLD;
    }
    if modifier.contains(Modifier::DIM) {
        attrs |= Attrs::DIM;
    }
    if modifier.contains(Modifier::ITALIC) {
        attrs |= Attrs::ITALIC;
    }
    if modifier.contains(Modifier::UNDERLINED) {
        attrs |= Attrs::UNDERLINE;
    }
    if modifier.contains(Modifier::REVERSED) {
        attrs |= Attrs::REVERSED;
    }
    if modifier.contains(Modifier::CROSSED_OUT) {
        attrs |= Attrs::STRIKETHROUGH;
    }
    // SLOW_BLINK, RAPID_BLINK, HIDDEN: no rabbitui analog — dropped, not an
    // error (ADR 0010 §Decision.6).
    attrs
}

/// Converts a whole ratatui [`Cell`]'s style to a rabbitui [`Style`].
///
/// Foreground and background colors always convert (ratatui has no "unset"
/// color — the absence of a color is [`RatColor::Reset`]), so the result's `fg`
/// and `bg` are always `Some`. Attributes come from
/// [`convert_modifier`]. `underline_color` is not read (see the module docs).
///
/// # Examples
///
/// ```
/// use rabbitui_core::style::{Attrs, Color};
/// use rabbitui_ratatui::style::convert_style;
/// use ratatui::buffer::Cell;
///
/// let mut cell = Cell::new("x");
/// cell.set_fg(ratatui::style::Color::Green);
/// cell.modifier = ratatui::style::Modifier::BOLD;
/// let style = convert_style(&cell);
/// assert_eq!(style.fg, Some(Color::Ansi(2)));
/// assert!(style.attrs.contains(Attrs::BOLD));
/// ```
#[must_use]
pub fn convert_style(cell: &Cell) -> Style {
    Style {
        fg: Some(convert_color(cell.fg)),
        bg: Some(convert_color(cell.bg)),
        attrs: convert_modifier(cell.modifier),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_color_maps_to_its_ansi_index() {
        let pairs = [
            (RatColor::Black, 0),
            (RatColor::Red, 1),
            (RatColor::Green, 2),
            (RatColor::Yellow, 3),
            (RatColor::Blue, 4),
            (RatColor::Magenta, 5),
            (RatColor::Cyan, 6),
            (RatColor::Gray, 7),
            (RatColor::DarkGray, 8),
            (RatColor::LightRed, 9),
            (RatColor::LightGreen, 10),
            (RatColor::LightYellow, 11),
            (RatColor::LightBlue, 12),
            (RatColor::LightMagenta, 13),
            (RatColor::LightCyan, 14),
            (RatColor::White, 15),
        ];
        for (rat, index) in pairs {
            assert_eq!(convert_color(rat), Color::Ansi(index), "{rat:?}");
        }
    }

    #[test]
    fn indexed_and_rgb_and_reset_pass_through() {
        assert_eq!(convert_color(RatColor::Indexed(200)), Color::Indexed(200));
        assert_eq!(convert_color(RatColor::Rgb(9, 8, 7)), Color::Rgb(9, 8, 7));
        assert_eq!(convert_color(RatColor::Reset), Color::Reset);
    }

    #[test]
    fn each_modeled_modifier_maps_to_its_attr() {
        let pairs = [
            (Modifier::BOLD, Attrs::BOLD),
            (Modifier::DIM, Attrs::DIM),
            (Modifier::ITALIC, Attrs::ITALIC),
            (Modifier::UNDERLINED, Attrs::UNDERLINE),
            (Modifier::REVERSED, Attrs::REVERSED),
            (Modifier::CROSSED_OUT, Attrs::STRIKETHROUGH),
        ];
        for (modifier, attr) in pairs {
            assert!(convert_modifier(modifier).contains(attr), "{modifier:?}");
        }
    }

    #[test]
    fn unrepresentable_modifiers_are_dropped_not_panicked() {
        // Blink and hidden have no rabbitui analog; converting them yields no
        // attributes rather than erroring.
        assert!(convert_modifier(Modifier::SLOW_BLINK).is_empty());
        assert!(convert_modifier(Modifier::RAPID_BLINK).is_empty());
        assert!(convert_modifier(Modifier::HIDDEN).is_empty());
        // A mix keeps the representable half and drops the rest.
        let mixed = convert_modifier(Modifier::BOLD | Modifier::HIDDEN);
        assert!(mixed.contains(Attrs::BOLD));
        assert_eq!(mixed, Attrs::BOLD);
    }

    #[test]
    fn convert_style_reads_fg_bg_and_modifier() {
        let mut cell = Cell::new("z");
        cell.set_fg(RatColor::Red);
        cell.set_bg(RatColor::Blue);
        cell.modifier = Modifier::BOLD | Modifier::UNDERLINED;
        let style = convert_style(&cell);
        assert_eq!(style.fg, Some(Color::Ansi(1)));
        assert_eq!(style.bg, Some(Color::Ansi(4)));
        assert!(style.attrs.contains(Attrs::BOLD | Attrs::UNDERLINE));
    }
}
