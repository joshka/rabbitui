//! Visual styles for terminal cells.
//!
//! A [`Style`] is a partial description: unset fields mean "leave unchanged /
//! inherit". Concrete colors degrade at render time according to negotiated
//! terminal capabilities (truecolor → 256 → 16); widgets normally reference
//! semantic theme roles rather than constructing styles directly, but this is
//! the type everything resolves to.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::style::{Attrs, Color, Style};
//!
//! let emphasis = Style::new().fg(Color::Rgb(0xfa, 0xb3, 0x87)).bold().italic();
//! assert!(emphasis.attrs.contains(Attrs::BOLD | Attrs::ITALIC));
//! assert_eq!(emphasis.bg, None);
//! ```

/// A terminal color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    /// The terminal's default foreground or background color.
    Reset,
    /// One of the 16 base ANSI colors (0–7 normal, 8–15 bright).
    Ansi(u8),
    /// An indexed color from the 256-color palette.
    Indexed(u8),
    /// A 24-bit truecolor value.
    Rgb(u8, u8, u8),
}

impl Color {
    /// ANSI black (index 0).
    pub const BLACK: Self = Self::Ansi(0);
    /// ANSI red (index 1).
    pub const RED: Self = Self::Ansi(1);
    /// ANSI green (index 2).
    pub const GREEN: Self = Self::Ansi(2);
    /// ANSI yellow (index 3).
    pub const YELLOW: Self = Self::Ansi(3);
    /// ANSI blue (index 4).
    pub const BLUE: Self = Self::Ansi(4);
    /// ANSI magenta (index 5).
    pub const MAGENTA: Self = Self::Ansi(5);
    /// ANSI cyan (index 6).
    pub const CYAN: Self = Self::Ansi(6);
    /// ANSI white (index 7).
    pub const WHITE: Self = Self::Ansi(7);
}

/// A set of text attributes (bold, italic, …), stored as a bitset.
///
/// # Examples
///
/// ```
/// use rabbitui_core::style::Attrs;
///
/// let attrs = Attrs::BOLD | Attrs::UNDERLINE;
/// assert!(attrs.contains(Attrs::BOLD));
/// assert!(!attrs.contains(Attrs::ITALIC));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Attrs(u16);

impl Attrs {
    /// No attributes.
    pub const NONE: Self = Self(0);
    /// Bold / increased intensity (SGR 1).
    pub const BOLD: Self = Self(1 << 0);
    /// Dim / decreased intensity (SGR 2).
    pub const DIM: Self = Self(1 << 1);
    /// Italic (SGR 3).
    pub const ITALIC: Self = Self(1 << 2);
    /// Underline (SGR 4).
    pub const UNDERLINE: Self = Self(1 << 3);
    /// Reverse video (SGR 7).
    pub const REVERSED: Self = Self(1 << 4);
    /// Crossed out (SGR 9).
    pub const STRIKETHROUGH: Self = Self(1 << 5);

    /// Returns true if every attribute in `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns true if no attributes are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl core::ops::BitOr for Attrs {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for Attrs {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// A partial visual style: colors and attributes to apply to text.
///
/// `None` colors mean "leave the terminal's current color in place".
///
/// # Examples
///
/// ```
/// use rabbitui_core::style::{Color, Style};
///
/// let warning = Style::new().fg(Color::YELLOW).bold();
/// let plain = Style::new();
/// assert_ne!(warning, plain);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Style {
    /// Foreground color, if set.
    pub fg: Option<Color>,
    /// Background color, if set.
    pub bg: Option<Color>,
    /// Text attributes.
    pub attrs: Attrs,
}

impl Style {
    /// Creates an empty style that changes nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            fg: None,
            bg: None,
            attrs: Attrs::NONE,
        }
    }

    /// Sets the foreground color.
    #[must_use]
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    /// Sets the background color.
    #[must_use]
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// Adds the bold attribute.
    #[must_use]
    pub const fn bold(mut self) -> Self {
        self.attrs = Attrs(self.attrs.0 | Attrs::BOLD.0);
        self
    }

    /// Adds the dim attribute.
    #[must_use]
    pub const fn dim(mut self) -> Self {
        self.attrs = Attrs(self.attrs.0 | Attrs::DIM.0);
        self
    }

    /// Adds the italic attribute.
    #[must_use]
    pub const fn italic(mut self) -> Self {
        self.attrs = Attrs(self.attrs.0 | Attrs::ITALIC.0);
        self
    }

    /// Adds the underline attribute.
    #[must_use]
    pub const fn underline(mut self) -> Self {
        self.attrs = Attrs(self.attrs.0 | Attrs::UNDERLINE.0);
        self
    }

    /// Adds the reverse-video attribute.
    #[must_use]
    pub const fn reversed(mut self) -> Self {
        self.attrs = Attrs(self.attrs.0 | Attrs::REVERSED.0);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attrs_combine_and_query() {
        let attrs = Attrs::BOLD | Attrs::ITALIC;
        assert!(attrs.contains(Attrs::BOLD));
        assert!(attrs.contains(Attrs::ITALIC));
        assert!(!attrs.contains(Attrs::UNDERLINE));
        assert!(Attrs::NONE.is_empty());
    }

    #[test]
    fn style_builder_sets_fields() {
        let style = Style::new().fg(Color::RED).bg(Color::BLACK).bold();
        assert_eq!(style.fg, Some(Color::RED));
        assert_eq!(style.bg, Some(Color::BLACK));
        assert!(style.attrs.contains(Attrs::BOLD));
    }
}
