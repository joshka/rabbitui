//! Layout: splitting areas with a constraint vocabulary.
//!
//! Per `docs/adr/0004-layout.md`: no constraint solver, no flexbox — direct
//! arithmetic with exact division, so fractional splits never leave a one-cell
//! gap. This module starts with the row/column split primitive; intrinsic
//! measurement (`desired_height(width)`) joins it in a later slice.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::geometry::{Rect, Size};
//! use rabbitui_core::layout::{Constraint, split_rows};
//!
//! let area = Rect::from_size(Size::new(80, 24));
//! let [status, body, input] =
//!     split_rows(area, [Constraint::Length(1), Constraint::Fill(1), Constraint::Length(3)]);
//! assert_eq!(status.size.height, 1);
//! assert_eq!(body.size.height, 20);
//! assert_eq!(input.size.height, 3);
//! ```

use crate::geometry::{Position, Rect, Size};

/// How much of an axis a region should take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Constraint {
    /// Exactly this many cells (clipped if the area is too small).
    Length(u16),
    /// A weighted share of the space left after every [`Length`] is taken.
    /// Shares divide exactly: `Fill(1), Fill(1), Fill(1)` over any height
    /// covers every cell with no gap.
    ///
    /// [`Length`]: Constraint::Length
    Fill(u16),
}

/// Splits `area` into `N` horizontal bands, top to bottom.
///
/// `Length` constraints are satisfied first, in order, clipped when the area
/// runs out. The remaining rows divide among `Fill` constraints by weight
/// using cumulative exact arithmetic: band boundaries are computed as rounded
/// cumulative shares, so the bands always tile the remainder exactly.
#[must_use]
pub fn split_rows<const N: usize>(area: Rect, constraints: [Constraint; N]) -> [Rect; N] {
    let heights = split_lengths(area.size.height, &constraints);
    let mut y = area.origin.y;
    heights.map(|height| {
        let rect = Rect::new(
            Position::new(area.origin.x, y),
            Size::new(area.size.width, height),
        );
        y = y.saturating_add(height);
        rect
    })
}

/// Splits `area` into `N` vertical bands, left to right.
///
/// Identical rules to [`split_rows`], applied to columns.
#[must_use]
pub fn split_columns<const N: usize>(area: Rect, constraints: [Constraint; N]) -> [Rect; N] {
    let widths = split_lengths(area.size.width, &constraints);
    let mut x = area.origin.x;
    widths.map(|width| {
        let rect = Rect::new(
            Position::new(x, area.origin.y),
            Size::new(width, area.size.height),
        );
        x = x.saturating_add(width);
        rect
    })
}

/// Centers a `width` × `height` region inside `area`, clamped to fit.
///
/// The returned rectangle is centered on both axes with any odd remainder
/// biased to the top-left (integer division), and never larger than `area`: a
/// requested size exceeding the area is clamped to the area's extent on that
/// axis. This is the "put a panel in the middle of the screen at a sensible
/// size" primitive — pair it with [`inset`] for padded content.
///
/// # Examples
///
/// ```
/// use rabbitui_core::geometry::{Rect, Size};
/// use rabbitui_core::layout::center;
///
/// let screen = Rect::from_size(Size::new(80, 24));
/// let dialog = center(screen, 40, 10);
/// assert_eq!(dialog.size, Size::new(40, 10));
/// assert_eq!(dialog.origin.x, 20); // (80 - 40) / 2
/// assert_eq!(dialog.origin.y, 7); //  (24 - 10) / 2
///
/// // A request larger than the area clamps to the area.
/// let clamped = center(screen, 200, 100);
/// assert_eq!(clamped.size, Size::new(80, 24));
/// assert_eq!(clamped.origin, screen.origin);
/// ```
#[must_use]
pub fn center(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.size.width);
    let height = height.min(area.size.height);
    let x = area.origin.x + (area.size.width - width) / 2;
    let y = area.origin.y + (area.size.height - height) / 2;
    Rect::new(Position::new(x, y), Size::new(width, height))
}

/// Shrinks `area` inward by `margin` cells on every side.
///
/// A uniform inset: the origin moves in by `margin` on each axis and the size
/// loses `2 * margin`. When the area is too small to inset (twice the margin
/// exceeds a dimension) that dimension collapses to zero rather than
/// underflowing, so the result is always a valid — possibly empty — rectangle
/// inside `area`. This is the "breathing room" primitive for content inside a
/// container (see [`center`]).
///
/// # Examples
///
/// ```
/// use rabbitui_core::geometry::{Position, Rect, Size};
/// use rabbitui_core::layout::inset;
///
/// let area = Rect::new(Position::new(2, 1), Size::new(20, 10));
/// let inner = inset(area, 2);
/// assert_eq!(inner.origin, Position::new(4, 3));
/// assert_eq!(inner.size, Size::new(16, 6));
///
/// // Too small to inset: the axis collapses instead of underflowing.
/// let tiny = inset(Rect::from_size(Size::new(3, 3)), 2);
/// assert_eq!(tiny.size, Size::new(0, 0));
/// ```
#[must_use]
pub fn inset(area: Rect, margin: u16) -> Rect {
    let inset_both = margin.saturating_mul(2);
    let width = area.size.width.saturating_sub(inset_both);
    let height = area.size.height.saturating_sub(inset_both);
    // Only advance the origin if there is room; a collapsed axis stays put.
    let x = if width == 0 {
        area.origin.x
    } else {
        area.origin.x.saturating_add(margin)
    };
    let y = if height == 0 {
        area.origin.y
    } else {
        area.origin.y.saturating_add(margin)
    };
    Rect::new(Position::new(x, y), Size::new(width, height))
}

/// Resolves constraint lengths along one axis of `total` cells.
fn split_lengths<const N: usize>(total: u16, constraints: &[Constraint; N]) -> [u16; N] {
    let mut lengths = [0u16; N];
    let mut remaining = total;

    // Pass 1: fixed lengths, clipped in order as space runs out.
    for (length, constraint) in lengths.iter_mut().zip(constraints) {
        if let Constraint::Length(want) = constraint {
            *length = (*want).min(remaining);
            remaining -= *length;
        }
    }

    // Pass 2: divide the remainder among fills by cumulative exact shares.
    // boundary_i = round(cum_weight_i * remaining / total_weight) guarantees
    // the bands tile `remaining` with no gap and no overflow.
    let total_weight: u32 = constraints
        .iter()
        .map(|c| {
            if let Constraint::Fill(w) = c {
                u32::from(*w)
            } else {
                0
            }
        })
        .sum();
    if total_weight == 0 {
        return lengths;
    }
    let mut cum_weight: u32 = 0;
    let mut previous_boundary: u16 = 0;
    for (length, constraint) in lengths.iter_mut().zip(constraints) {
        if let Constraint::Fill(weight) = constraint {
            cum_weight += u32::from(*weight);
            let boundary = ((u32::from(remaining) * cum_weight + total_weight / 2) / total_weight)
                .min(u32::from(remaining)) as u16;
            *length = boundary - previous_boundary;
            previous_boundary = boundary;
        }
    }
    lengths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(height: u16) -> Rect {
        Rect::from_size(Size::new(10, height))
    }

    #[test]
    fn lengths_then_fill() {
        let [a, b, c] = split_rows(
            area(24),
            [
                Constraint::Length(1),
                Constraint::Fill(1),
                Constraint::Length(3),
            ],
        );
        assert_eq!((a.size.height, b.size.height, c.size.height), (1, 20, 3));
        assert_eq!(b.origin.y, 1);
        assert_eq!(c.origin.y, 21);
    }

    #[test]
    fn fills_tile_exactly_with_no_gap() {
        // 3 equal fills over 10 rows: 3+4+3 or similar — must sum to 10.
        let [a, b, c] = split_rows(
            area(10),
            [
                Constraint::Fill(1),
                Constraint::Fill(1),
                Constraint::Fill(1),
            ],
        );
        assert_eq!(a.size.height + b.size.height + c.size.height, 10);
        assert!(a.size.height.abs_diff(c.size.height) <= 1);
    }

    #[test]
    fn weighted_fills() {
        let [a, b] = split_rows(area(30), [Constraint::Fill(2), Constraint::Fill(1)]);
        assert_eq!((a.size.height, b.size.height), (20, 10));
    }

    #[test]
    fn lengths_clip_when_area_too_small() {
        let [a, b, c] = split_rows(
            area(4),
            [
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Fill(1),
            ],
        );
        assert_eq!((a.size.height, b.size.height, c.size.height), (3, 1, 0));
    }

    #[test]
    fn columns_split_the_other_axis() {
        let base = Rect::from_size(Size::new(9, 5));
        let [l, r] = split_columns(base, [Constraint::Fill(1), Constraint::Fill(2)]);
        assert_eq!((l.size.width, r.size.width), (3, 6));
        assert_eq!(r.origin.x, 3);
        assert_eq!(l.size.height, 5);
    }

    #[test]
    fn center_centers_within_the_area() {
        let screen = Rect::from_size(Size::new(80, 24));
        let inner = center(screen, 40, 10);
        assert_eq!(inner.size, Size::new(40, 10));
        assert_eq!(inner.origin, Position::new(20, 7));
    }

    #[test]
    fn center_respects_a_non_origin_area() {
        let area = Rect::new(Position::new(10, 5), Size::new(20, 8));
        let inner = center(area, 10, 4);
        // Centered within the offset area: x = 10 + (20-10)/2, y = 5 + (8-4)/2.
        assert_eq!(inner.origin, Position::new(15, 7));
        assert_eq!(inner.size, Size::new(10, 4));
    }

    #[test]
    fn center_clamps_an_oversized_request() {
        let screen = Rect::from_size(Size::new(30, 10));
        let inner = center(screen, 200, 200);
        assert_eq!(inner.size, Size::new(30, 10));
        assert_eq!(inner.origin, Position::ORIGIN);
    }

    #[test]
    fn center_biases_odd_remainder_to_top_left() {
        // A 5-wide region in a 10-wide area leaves 5 to split: 2 left, 3 right.
        let inner = center(Rect::from_size(Size::new(10, 10)), 5, 5);
        assert_eq!(inner.origin, Position::new(2, 2));
    }

    #[test]
    fn inset_shrinks_every_side_uniformly() {
        let area = Rect::new(Position::new(2, 1), Size::new(20, 10));
        let inner = inset(area, 2);
        assert_eq!(inner.origin, Position::new(4, 3));
        assert_eq!(inner.size, Size::new(16, 6));
    }

    #[test]
    fn inset_of_zero_is_identity() {
        let area = Rect::new(Position::new(3, 4), Size::new(6, 7));
        assert_eq!(inset(area, 0), area);
    }

    #[test]
    fn inset_collapses_rather_than_underflowing() {
        // Margin 2 on a 3×3 area: both axes want to lose 4 cells from 3.
        let inner = inset(Rect::from_size(Size::new(3, 3)), 2);
        assert_eq!(inner.size, Size::new(0, 0));
        // The origin stays put on a collapsed axis (no underflow, no drift).
        assert_eq!(inner.origin, Position::ORIGIN);
    }

    #[test]
    fn inset_collapses_one_axis_independently() {
        // Wide but short: width survives the inset, height collapses.
        let area = Rect::new(Position::new(1, 1), Size::new(20, 3));
        let inner = inset(area, 2);
        assert_eq!(inner.size, Size::new(16, 0));
        assert_eq!(inner.origin, Position::new(3, 1));
    }
}
