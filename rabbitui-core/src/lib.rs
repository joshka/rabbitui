//! Core types for the rabbitui terminal UI framework.
//!
//! This crate holds the runtime-free foundation: geometry, styles, the cell
//! buffer, widget identity, the retained state store, layout, and the widget
//! contract. It never touches an async runtime or a terminal device — see
//! `docs/adr/0011-crate-layout.md`.
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

pub mod a11y;
pub mod buffer;
pub mod commit;
pub mod facts;
pub mod frame;
pub mod geometry;
pub mod id;
pub mod input;
pub mod layout;
pub mod log;
pub mod mode;
pub mod outcome;
pub mod pending;
pub mod routing;
pub mod scroll;
pub mod spacing;
pub mod store;
pub mod style;
pub mod text;
pub mod theme;
pub mod widget;
