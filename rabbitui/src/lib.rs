//! rabbitui — a terminal user interface framework.
//!
//! This crate is the user-facing facade: it owns the async runtime integration,
//! the terminal session, and (as later slices land) the event loop, declared
//! frames, and widgets. Runtime-free building blocks live in [`rabbitui_core`].
//!
//! The crate is under construction, slice by slice — see `ROADMAP.md`. What
//! exists today is the substrate seam ([`Terminal`]) and enough escape-sequence
//! encoding to draw styled text and restore the terminal reliably.
//!
//! # Examples
//!
//! Draw a styled line in the alternate screen and wait for a key
//! (see `examples/smoke.rs` for the full program):
//!
//! ```no_run
//! use rabbitui::Terminal;
//! use rabbitui_core::geometry::Position;
//! use rabbitui_core::style::{Color, Style};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let mut terminal = Terminal::open().await?;
//! terminal.print_styled(Position::new(2, 1), "hello", Style::new().fg(Color::GREEN).bold()).await?;
//! terminal.flush().await?;
//! terminal.next_event().await?;
//! terminal.close().await?;
//! # Ok(())
//! # }
//! ```

pub use rabbitui_core as core;

pub mod app;
mod encode;
pub mod input;
mod render;
mod terminal;
#[cfg(feature = "themes")]
pub mod theme;

pub use app::App;
pub use terminal::Terminal;
