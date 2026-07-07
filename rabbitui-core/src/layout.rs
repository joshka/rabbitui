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
        let rect = Rect::new(Position::new(area.origin.x, y), Size::new(area.size.width, height));
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
        let rect = Rect::new(Position::new(x, area.origin.y), Size::new(width, area.size.height));
        x = x.saturating_add(width);
        rect
    })
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
    let total_weight: u32 =
        constraints.iter().map(|c| if let Constraint::Fill(w) = c { u32::from(*w) } else { 0 }).sum();
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
        let [a, b, c] =
            split_rows(area(24), [Constraint::Length(1), Constraint::Fill(1), Constraint::Length(3)]);
        assert_eq!((a.size.height, b.size.height, c.size.height), (1, 20, 3));
        assert_eq!(b.origin.y, 1);
        assert_eq!(c.origin.y, 21);
    }

    #[test]
    fn fills_tile_exactly_with_no_gap() {
        // 3 equal fills over 10 rows: 3+4+3 or similar — must sum to 10.
        let [a, b, c] =
            split_rows(area(10), [Constraint::Fill(1), Constraint::Fill(1), Constraint::Fill(1)]);
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
        let [a, b, c] =
            split_rows(area(4), [Constraint::Length(3), Constraint::Length(3), Constraint::Fill(1)]);
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
}
