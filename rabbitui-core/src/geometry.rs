//! Screen geometry in cell units.
//!
//! Positions are zero-based, column-major (`x` is the column, `y` is the row),
//! measured in terminal cells from the top-left corner of the drawing surface.

/// A position on the drawing surface, in cells, zero-based.
///
/// # Examples
///
/// ```
/// use rabbitui_core::geometry::Position;
///
/// let top_left = Position::ORIGIN;
/// let below = Position::new(0, 1);
/// assert_eq!(top_left.x, below.x);
/// assert_eq!(top_left.y + 1, below.y);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Position {
    /// Column, zero-based from the left edge.
    pub x: u16,
    /// Row, zero-based from the top edge.
    pub y: u16,
}

impl Position {
    /// The top-left corner.
    pub const ORIGIN: Self = Self { x: 0, y: 0 };

    /// Creates a position from a column and row.
    #[must_use]
    pub const fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }
}

/// A size in cells.
///
/// # Examples
///
/// ```
/// use rabbitui_core::geometry::Size;
///
/// let size = Size::new(80, 24);
/// assert_eq!(size.area(), 1920);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Size {
    /// Width in columns.
    pub width: u16,
    /// Height in rows.
    pub height: u16,
}

impl Size {
    /// Creates a size from a width and height.
    #[must_use]
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    /// The number of cells this size covers.
    #[must_use]
    pub const fn area(self) -> u32 {
        self.width as u32 * self.height as u32
    }
}
