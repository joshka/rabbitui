//! The built-in widget catalog for rabbitui.
//!
//! Widgets here implement the one contract from [`rabbitui_core::widget`]: a
//! short-lived spec that renders against framework-retained per-identity state
//! (`docs/adr/0008-widget-contract.md`). The catalog is runtime-free — it
//! depends only on [`rabbitui_core`], never on tokio or a terminal — so widgets
//! are testable headlessly through `rabbitui-testing` (`docs/adr/0009-testing.md`).
//!
//! The catalog grows a widget at a time, slice by slice (`ROADMAP.md`):
//!
//! - [`Text`] — a stateless multi-line label. Styled by [`Role`], defaulting to
//!   [`Role::Text`].
//! - [`Button`] — a focusable push button: [`Role::Text`] normally,
//!   [`Role::Highlight`] when focused, emitting
//!   [`Outcome::Activated`](rabbitui_core::outcome::Outcome::Activated) on Enter
//!   or Space.
//! - [`TextInput`] — a single-line, uncontrolled text field with
//!   grapheme-correct editing, emitting
//!   [`Outcome::Changed`](rabbitui_core::outcome::Outcome::Changed) and
//!   [`Outcome::Submitted`](rabbitui_core::outcome::Outcome::Submitted).
//! - [`SelectionList`] — a virtualized, index-selected list over a
//!   [`ListSource`], emitting
//!   [`Outcome::Selected`](rabbitui_core::outcome::Outcome::Selected) and
//!   [`Outcome::Activated`](rabbitui_core::outcome::Outcome::Activated).
//! - [`Collapsible`] — a header + body disclosure cell whose collapsed state is
//!   retained by identity; Enter or a header click toggles it, emitting
//!   [`Outcome::Toggled`](rabbitui_core::outcome::Outcome::Toggled). The
//!   transcript's alt-screen cell (slice 8).
//! - `LogOverlay` — a debug readout that renders the tail of a
//!   [`LogHandle`](rabbitui_core::log::LogHandle) ring in a themed panel, meant
//!   for a [`Frame::layer`](rabbitui_core::frame::Frame::layer). Behind the
//!   `tracing` feature (matching the facade), but it depends only on the *core*
//!   handle — no `tracing` in this crate (the logging seam,
//!   `docs/design/arc2b-measurement-scroll.md`).
//! - [`Panel`] — a container-look *backdrop*: a role-filled surface with an
//!   optional light-box border, a title in the top border, and inner padding.
//!   Because widgets cannot nest yet, it paints behind content declared into its
//!   [`Panel::inner`] area (the pre-composition pattern; the catalog arc turns it
//!   into a real container).
//!
//! Every widget references semantic [`Role`]s rather than hard-coded colors, so
//! the active [`Theme`] re-skins the whole catalog (ADR 0007).
//!
//! [`Role`]: rabbitui_core::theme::Role
//! [`Role::Text`]: rabbitui_core::theme::Role::Text
//! [`Role::Highlight`]: rabbitui_core::theme::Role::Highlight
//! [`Theme`]: rabbitui_core::theme::Theme
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::theme::Role;
//! use rabbitui_widgets::Text;
//!
//! let title = Text::new("Hello, rabbitui!").role(Role::Accent);
//! assert_eq!(title.content().to_plain_string(), "Hello, rabbitui!");
//! ```

pub mod button;
pub mod collapsible;
#[cfg(feature = "tracing")]
pub mod log_overlay;
pub mod panel;
pub mod selection_list;
pub mod text;
pub mod text_input;

pub use button::Button;
pub use collapsible::Collapsible;
#[cfg(feature = "tracing")]
pub use log_overlay::LogOverlay;
pub use panel::Panel;
pub use selection_list::{ListSource, SelectionList};
pub use text::{Content, Text};
pub use text_input::TextInput;
