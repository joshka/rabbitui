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
//! - [`Table`] — a virtualized, index-selected table of cells over a
//!   [`TableSource`] (the virtualization seam: cells are fetched only for the
//!   painted window), with a pinned header and per-column width constraints,
//!   emitting [`Outcome::Selected`](rabbitui_core::outcome::Outcome::Selected)
//!   and [`Outcome::Activated`](rabbitui_core::outcome::Outcome::Activated).
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
//! - [`HelpOverlay`] — a display-only reference card generated from a
//!   [`Keymap`](rabbitui_core::keymap::Keymap): two aligned columns (chord,
//!   action label) in a titled [`Panel`], meant for a
//!   [`Frame::layer`](rabbitui_core::frame::Frame::layer). Takes no focus — the
//!   app routes the close keys itself (Arc 4 §3, the keybinding layer).
//! - [`Panel`] — a container-look *backdrop*: a role-filled surface with an
//!   optional light-box border, a title in the top border, and inner padding.
//!   Because widgets cannot nest yet, it paints behind content declared into its
//!   [`Panel::inner`] area (the pre-composition pattern; the catalog arc turns it
//!   into a real container).
//! - [`form`](fn@form) — not a widget but a *declaration helper* (widgets do not
//!   nest): [`form`](fn@form) opens a scope and [`FormScope`] lays out
//!   label-aligned fields down an area, displaying app-supplied validation errors
//!   in [`Role::Danger`](rabbitui_core::theme::Role::Danger). Validation stays
//!   app-land (ADR 0001); the form only displays. The
//!   [`ScrollScope`](rabbitui_core::scroll::ScrollScope) pattern applied to forms.
//! - `FactsInspector` — a read-only devtools overlay that renders the previous
//!   frame's [`FrameFacts`](rabbitui_core::facts::FrameFacts) tree (id path, area,
//!   layer, focusable, visibility, focus marker) in a themed panel, matching the
//!   [`facts::dump`](rabbitui_core::facts::dump) log format. Behind the `devtools`
//!   feature (ADR arc4 §7); reads only the *core* facts, so no runtime is pulled in.
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
pub mod error_banner;
#[cfg(feature = "devtools")]
pub mod facts_inspector;
pub mod form;
pub mod help_overlay;
#[cfg(feature = "tracing")]
pub mod log_overlay;
pub mod panel;
pub mod selection_list;
pub mod table;
pub mod text;
pub mod text_input;
mod text_util;

pub use button::Button;
pub use collapsible::Collapsible;
pub use error_banner::ErrorBanner;
#[cfg(feature = "devtools")]
pub use facts_inspector::FactsInspector;
pub use form::{FieldSpec, FormScope, form, label_width};
pub use help_overlay::HelpOverlay;
#[cfg(feature = "tracing")]
pub use log_overlay::LogOverlay;
pub use panel::Panel;
pub use selection_list::{FromFn, ListSource, SelectionList, from_fn, rows_with};
pub use table::{
    Column, Table, TableFromFn, TableSource, TableState, table_from_fn, table_rows_with,
};
pub use text::{Content, Text};
pub use text_input::TextInput;
