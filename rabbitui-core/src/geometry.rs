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

/// A rectangular region of the drawing surface, in cells.
///
/// # Examples
///
/// ```
/// use rabbitui_core::geometry::{Position, Rect, Size};
///
/// let area = Rect::new(Position::new(2, 1), Size::new(10, 4));
/// assert!(area.contains(Position::new(2, 1)));
/// assert!(!area.contains(Position::new(12, 1)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    /// The top-left corner.
    pub origin: Position,
    /// The extent from the origin.
    pub size: Size,
}

impl Rect {
    /// Creates a rectangle from its top-left corner and size.
    #[must_use]
    pub const fn new(origin: Position, size: Size) -> Self {
        Self { origin, size }
    }

    /// A rectangle covering `size` cells from the top-left of the surface.
    #[must_use]
    pub const fn from_size(size: Size) -> Self {
        Self {
            origin: Position::ORIGIN,
            size,
        }
    }

    /// The first column inside the rectangle.
    #[must_use]
    pub const fn left(self) -> u16 {
        self.origin.x
    }

    /// One past the last column inside the rectangle.
    #[must_use]
    pub const fn right(self) -> u16 {
        self.origin.x.saturating_add(self.size.width)
    }

    /// The first row inside the rectangle.
    #[must_use]
    pub const fn top(self) -> u16 {
        self.origin.y
    }

    /// One past the last row inside the rectangle.
    #[must_use]
    pub const fn bottom(self) -> u16 {
        self.origin.y.saturating_add(self.size.height)
    }

    /// Returns true if the rectangle covers zero cells.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.size.width == 0 || self.size.height == 0
    }

    /// Returns true if `position` lies inside the rectangle.
    #[must_use]
    pub const fn contains(self, position: Position) -> bool {
        position.x >= self.left()
            && position.x < self.right()
            && position.y >= self.top()
            && position.y < self.bottom()
    }

    /// The largest rectangle contained in both `self` and `other`; an empty
    /// rectangle when they do not overlap.
    #[must_use]
    pub fn intersection(self, other: Self) -> Self {
        let left = self.left().max(other.left());
        let top = self.top().max(other.top());
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Self {
            origin: Position::new(left, top),
            size: Size::new(right.saturating_sub(left), bottom.saturating_sub(top)),
        }
    }
}
