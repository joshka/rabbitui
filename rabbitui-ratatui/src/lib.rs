//! Render ratatui widgets inside a rabbitui frame, via a cell-copy bridge.
//!
//! This is the single optional interop crate of `docs/adr/0010-ratatui-interop.md`:
//! the *only* place rabbitui's and ratatui's type systems meet (ADR 0011).
//! `rabbitui-core` never mentions ratatui; a rabbitui app that uses no ratatui
//! widget pulls in none of this. Adding it buys day-one access to the ratatui
//! widget zoo — charts, canvases, `Paragraph`, `Block`, third-party widgets — as
//! a **drawing escape hatch** inside a rabbitui app.
//!
//! # The mechanism: render into a `Buffer`, copy the cells out
//!
//! ADR 0010 chose Option C (§Decision.3): the bridge allocates a ratatui
//! [`Buffer`](ratatui::buffer::Buffer) the size of the target area, calls the
//! ratatui widget's paint step into it, and copies each cell — grapheme plus
//! converted style — into a rabbitui frame at the same coordinate. No ratatui
//! `Terminal`, backend, or draw loop is involved. Because ADR 0003 kept the two
//! cell models convertible *by construction*, the copy is a total per-cell field
//! map (see [`style`]), not a re-render or a semantic translation.
//!
//! Two entry points:
//!
//! - [`render_ratatui`] / [`render_ratatui_stateful`] — the imperative bridge,
//!   called inside a native widget's `render`.
//! - [`RatatuiWidget`] — a declarative wrapper implementing rabbitui's
//!   [`Widget`](rabbitui_core::widget::Widget), dropped into
//!   [`Frame::widget`](rabbitui_core::frame::Frame::widget) by key.
//!
//! # What the bridge does *not* carry (ADR 0010 §Decision.5)
//!
//! Cells and nothing else. A bridged ratatui widget is an inert rectangle of
//! styled cells: no identity, no focus, no hit region, no cursor candidate, no
//! outcome. Its styles arrive pre-resolved to concrete colors and are *not*
//! re-themed on a theme switch. ratatui `StatefulWidget` state stays
//! caller-owned for one frame and does not enter rabbitui's per-identity store.
//! Make bridged content interactive by wrapping it in a native rabbitui widget
//! that owns the facts.
//!
//! # ratatui version and features
//!
//! Tracks ratatui `0.30` with `default-features = false` — the bridge only paints
//! into a `Buffer` it owns, so it needs no backend (`crossterm`/`termion`/
//! `termwiz`) and no `Terminal`. It enables `std` and `all-widgets`. This leaf
//! crate absorbs ratatui's release cadence and its higher MSRV (rustc 1.88),
//! insulated from `rabbitui-core` (rustc 1.85) by the crate boundary
//! (ADR 0010 §Neutral, ADR 0011).
//!
//! # Examples
//!
//! Declare a bordered ratatui `Block` next to native rabbitui widgets:
//!
//! ```
//! use rabbitui_core::buffer::Buffer;
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::geometry::Size;
//! use rabbitui_core::id::key;
//! use rabbitui_core::store::StateStore;
//! use rabbitui_ratatui::RatatuiWidget;
//! use ratatui::widgets::Block;
//!
//! let mut buffer = Buffer::new(Size::new(12, 4));
//! let mut store = StateStore::new();
//! store.begin_frame();
//! let mut frame = Frame::new(&mut buffer, &mut store);
//! frame.widget(key("panel"), frame.area(), &RatatuiWidget::new(Block::bordered()));
//! # let _ = frame.finish();
//! store.end_frame();
//! ```

pub mod adapter;
pub mod bridge;
pub mod style;

pub use adapter::RatatuiWidget;
pub use bridge::{render_ratatui, render_ratatui_stateful};
pub use style::{convert_color, convert_modifier, convert_style};
