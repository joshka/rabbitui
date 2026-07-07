//! The built-in widget catalog for rabbitui.
//!
//! Widgets here implement the one contract from [`rabbitui_core::widget`]: a
//! short-lived spec that renders against framework-retained per-identity state
//! (`docs/adr/0008-widget-contract.md`). The catalog is runtime-free — it
//! depends only on [`rabbitui_core`], never on tokio or a terminal — so widgets
//! are testable headlessly through `rabbitui-testing` (`docs/adr/0009-testing.md`).
//!
//! The catalog grows a widget at a time, slice by slice (`ROADMAP.md`). Slice 2
//! starts it with [`Text`], the stateless multi-line label the counter and
//! hello examples paint through.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::style::{Color, Style};
//! use rabbitui_widgets::Text;
//!
//! let title = Text::new("Hello, rabbitui!").style(Style::new().fg(Color::GREEN).bold());
//! assert_eq!(title.content(), "Hello, rabbitui!");
//! ```

pub mod text;

pub use text::Text;
