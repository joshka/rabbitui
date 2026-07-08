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

    /// Returns this color blended toward black by `amount` (`0.0` = unchanged,
    /// `1.0` = black), preserving hue — a darker tone of the same color.
    ///
    /// Defined for [`Rgb`](Color::Rgb) only; a palette [`Ansi`](Color::Ansi) index
    /// has no reliable same-hue shade, so it is returned unchanged (callers that
    /// need contrast should detect the no-op and pair with a fixed color).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::Color;
    ///
    /// assert_eq!(Color::Rgb(200, 100, 50).darken(0.5), Color::Rgb(100, 50, 25));
    /// assert_eq!(Color::CYAN.darken(0.5), Color::CYAN); // Ansi: unchanged
    /// ```
    #[must_use]
    pub fn darken(self, amount: f32) -> Self {
        match self {
            Color::Rgb(r, g, b) => {
                let factor = 1.0 - amount.clamp(0.0, 1.0);
                Color::Rgb(
                    scale_channel(r, factor),
                    scale_channel(g, factor),
                    scale_channel(b, factor),
                )
            }
            other => other,
        }
    }

    /// Returns this color blended toward white by `amount` (`0.0` = unchanged,
    /// `1.0` = white), preserving hue — a lighter tone of the same color.
    ///
    /// [`Rgb`](Color::Rgb) only; an [`Ansi`](Color::Ansi) index is returned
    /// unchanged (see [`darken`](Color::darken)).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::Color;
    ///
    /// assert_eq!(Color::Rgb(100, 100, 100).lighten(0.5), Color::Rgb(178, 178, 178));
    /// ```
    #[must_use]
    pub fn lighten(self, amount: f32) -> Self {
        match self {
            Color::Rgb(r, g, b) => {
                let a = amount.clamp(0.0, 1.0);
                Color::Rgb(lift_channel(r, a), lift_channel(g, a), lift_channel(b, a))
            }
            other => other,
        }
    }
}

/// Scales one channel toward `0` by `factor` (`factor` in `0.0..=1.0`).
fn scale_channel(channel: u8, factor: f32) -> u8 {
    (f32::from(channel) * factor).round().clamp(0.0, 255.0) as u8
}

/// Lifts one channel toward `255` by `amount` (`amount` in `0.0..=1.0`).
fn lift_channel(channel: u8, amount: f32) -> u8 {
    let channel = f32::from(channel);
    (channel + (255.0 - channel) * amount)
        .round()
        .clamp(0.0, 255.0) as u8
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

    /// Every defined attribute set — the mask [`Not`](core::ops::Not) complements
    /// over, so a complement never yields an undefined flag.
    pub const ALL: Self = Self(
        Self::BOLD.0
            | Self::DIM.0
            | Self::ITALIC.0
            | Self::UNDERLINE.0
            | Self::REVERSED.0
            | Self::STRIKETHROUGH.0,
    );

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

    /// Returns `self` with every attribute in `other` also set.
    ///
    /// The value companion to [`BitOrAssign`](core::ops::BitOrAssign) for a
    /// `const` / builder context: `attrs.insert(Attrs::BOLD)` adds a flag without
    /// a mutable binding. Inserting a flag already present is a no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::Attrs;
    ///
    /// let attrs = Attrs::BOLD.insert(Attrs::ITALIC);
    /// assert!(attrs.contains(Attrs::BOLD | Attrs::ITALIC));
    /// ```
    #[must_use]
    pub const fn insert(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns `self` with every attribute in `other` cleared.
    ///
    /// The complement of [`insert`](Self::insert): `attrs.remove(Attrs::BOLD)`
    /// drops a flag, leaving the rest untouched. Removing a flag that is not set
    /// is a no-op. This closes the flagship's hand-rolled `remove` (the markdown
    /// renderer rebuilt the set from the known flags because `Attrs` had only
    /// `|`).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::Attrs;
    ///
    /// let attrs = (Attrs::BOLD | Attrs::ITALIC).remove(Attrs::BOLD);
    /// assert!(attrs.contains(Attrs::ITALIC));
    /// assert!(!attrs.contains(Attrs::BOLD));
    /// ```
    #[must_use]
    pub const fn remove(self, other: Self) -> Self {
        Self(self.0 & !other.0)
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

impl core::ops::BitAnd for Attrs {
    type Output = Self;

    /// The intersection: the attributes set in *both* operands.
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl core::ops::BitAndAssign for Attrs {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl core::ops::Not for Attrs {
    type Output = Self;

    /// The complement over the defined attribute flags.
    ///
    /// Only the bits backing the six defined attributes can ever be set, so the
    /// complement is masked to them — `!attrs` never yields a phantom flag, and
    /// `attrs & !other` removes exactly `other` (the identity [`remove`] uses).
    ///
    /// [`remove`]: Attrs::remove
    fn not(self) -> Self {
        Self(!self.0 & Self::ALL.0)
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

    /// Layers `self` over `base`: `self`'s set fields win, `base` fills the rest.
    ///
    /// The composition a span uses against its widget's default (Arc 2B styled
    /// `Text`): a span carrying only `bold` over a `Role::Text` base keeps the
    /// role's foreground and adds bold; a span with an explicit `fg` overrides the
    /// role's. Colors are override-if-set (`self.fg` if `Some`, else `base.fg`);
    /// attributes **union**, so a bold span over an italic role is both. An empty
    /// `Style::new()` therefore resolves to exactly `base`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::{Attrs, Color, Style};
    ///
    /// let base = Style::new().fg(Color::GREEN).italic();
    /// // A span that only asks for bold keeps the base's green + italic, adds bold.
    /// let resolved = Style::new().bold().merge_over(base);
    /// assert_eq!(resolved.fg, Some(Color::GREEN));
    /// assert!(resolved.attrs.contains(Attrs::BOLD | Attrs::ITALIC));
    /// // An empty style resolves to the base unchanged.
    /// assert_eq!(Style::new().merge_over(base), base);
    /// ```
    #[must_use]
    pub const fn merge_over(self, base: Self) -> Self {
        Self {
            fg: match self.fg {
                Some(color) => Some(color),
                None => base.fg,
            },
            bg: match self.bg {
                Some(color) => Some(color),
                None => base.bg,
            },
            attrs: Attrs(self.attrs.0 | base.attrs.0),
        }
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

    #[test]
    fn insert_adds_a_flag() {
        let attrs = Attrs::BOLD.insert(Attrs::ITALIC);
        assert!(attrs.contains(Attrs::BOLD));
        assert!(attrs.contains(Attrs::ITALIC));
        // Inserting a present flag is a no-op.
        assert_eq!(attrs.insert(Attrs::BOLD), attrs);
    }

    #[test]
    fn remove_clears_a_flag_and_leaves_the_rest() {
        let attrs = (Attrs::BOLD | Attrs::ITALIC | Attrs::UNDERLINE).remove(Attrs::ITALIC);
        assert!(attrs.contains(Attrs::BOLD));
        assert!(attrs.contains(Attrs::UNDERLINE));
        assert!(!attrs.contains(Attrs::ITALIC));
        // Removing an absent flag is a no-op.
        assert_eq!(attrs.remove(Attrs::STRIKETHROUGH), attrs);
    }

    #[test]
    fn bitand_is_the_intersection() {
        let left = Attrs::BOLD | Attrs::ITALIC;
        let right = Attrs::ITALIC | Attrs::UNDERLINE;
        assert_eq!(left & right, Attrs::ITALIC);
        let mut acc = left;
        acc &= right;
        assert_eq!(acc, Attrs::ITALIC);
    }

    #[test]
    fn merge_over_layers_set_fields_and_unions_attrs() {
        let base = Style::new().fg(Color::GREEN).italic();
        // Only bold set: base fg/italic survive, bold is added.
        let resolved = Style::new().bold().merge_over(base);
        assert_eq!(resolved.fg, Some(Color::GREEN));
        assert!(resolved.attrs.contains(Attrs::BOLD | Attrs::ITALIC));
        // An explicit fg overrides the base's.
        let over = Style::new().fg(Color::RED).merge_over(base);
        assert_eq!(over.fg, Some(Color::RED));
        // An empty style resolves to exactly the base.
        assert_eq!(Style::new().merge_over(base), base);
    }

    #[test]
    fn not_complements_only_defined_flags() {
        // The complement of NONE is every defined flag; complementing again
        // returns to NONE — the involution holds because Not masks to ALL.
        assert_eq!(!Attrs::NONE, Attrs::ALL);
        assert_eq!(!Attrs::ALL, Attrs::NONE);
        // `attrs & !other` is exactly `attrs.remove(other)`.
        let attrs = Attrs::BOLD | Attrs::ITALIC;
        assert_eq!(attrs & !Attrs::BOLD, attrs.remove(Attrs::BOLD));
    }
}
