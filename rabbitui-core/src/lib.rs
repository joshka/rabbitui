//! Core types for the rabbitui terminal UI framework.
//!
//! This crate holds the runtime-free foundation: geometry, styles, and (as later
//! slices land) the cell buffer, widget identity, frame facts, and the widget
//! contract. It has no dependencies and never touches an async runtime or a
//! terminal device — see `docs/adr/0011-crate-layout.md`.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::style::{Color, Style};
//!
//! let style = Style::new().fg(Color::GREEN).bold();
//! assert_eq!(style.fg, Some(Color::GREEN));
//! assert!(style.attrs.contains(rabbitui_core::style::Attrs::BOLD));
//! ```

pub mod geometry;
pub mod style;
