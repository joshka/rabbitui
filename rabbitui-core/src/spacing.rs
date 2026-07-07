//! Spacing design tokens.
//!
//! Named layout constants so the examples and apps share one vocabulary for gaps
//! and insets instead of scattering magic numbers — the layout counterpart to the
//! theme's [`Role`](crate::theme::Role)s (Arc 2A). These are the *defaults*; a view
//! is free to use its own values where a design genuinely needs them. Deliberately
//! a flat set of constants, not a density-scaling system — a compact/comfortable
//! mode is speculative until a real app asks for it.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::layout::Constraint;
//! use rabbitui_core::spacing;
//!
//! // A section, a gap, then a footer band.
//! let rows = [
//!     Constraint::Fill(1),
//!     Constraint::Length(spacing::GAP),
//!     Constraint::Length(1),
//! ];
//! assert_eq!(rows[1], Constraint::Length(1));
//! ```

/// The standard gap between sibling sections or panels, in rows or columns.
pub const GAP: u16 = 1;

/// The inner padding a [`Panel`]-style container holds around its content.
///
/// [`Panel`]: crate::widget::Widget
pub const PANEL_PADDING: u16 = 1;

/// The gap between a form field's label and its input.
pub const FORM_LABEL_GAP: u16 = 1;

/// The margin an overlay (modal, palette) insets from the screen edge.
pub const OVERLAY_MARGIN: u16 = 2;
