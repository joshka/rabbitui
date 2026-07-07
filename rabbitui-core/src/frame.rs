//! The declared frame: where an app describes one frame of UI.
//!
//! Per `docs/adr/0001-programming-model.md`, the app's view function receives
//! a [`Frame`] and declares widgets into it by key. The frame composes
//! identities ([`WidgetId`]s) from the declaration path, lends each widget its
//! retained state from the [`StateStore`], and paints into the target buffer
//! through clipped [`RenderCtx`]s. From slice 3 it also collects frame facts
//! (hit regions, focus order) as it goes.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::buffer::Buffer;
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::geometry::{Position, Size};
//! use rabbitui_core::id::key;
//! use rabbitui_core::layout::Constraint;
//! use rabbitui_core::store::StateStore;
//! use rabbitui_core::style::Style;
//! use rabbitui_core::widget::{RenderCtx, Widget};
//!
//! struct Label<'a>(&'a str);
//! impl Widget for Label<'_> {
//!     type State = ();
//!     fn render(&self, _state: &mut (), ctx: &mut RenderCtx<'_>) {
//!         ctx.set_string(Position::ORIGIN, self.0, Style::new());
//!     }
//! }
//!
//! let mut buffer = Buffer::new(Size::new(20, 3));
//! let mut store = StateStore::new();
//! let mut frame = Frame::new(&mut buffer, &mut store);
//!
//! let [title, _body] = frame.rows([Constraint::Length(1), Constraint::Fill(1)]);
//! frame.widget(key("title"), title, &Label("hello"));
//! # drop(frame);
//! assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "h");
//! ```

use crate::buffer::Buffer;
use crate::geometry::Rect;
use crate::id::{Key, WidgetId};
use crate::layout::{Constraint, split_columns, split_rows};
use crate::store::StateStore;
use crate::widget::{RenderCtx, Widget};

/// One frame under declaration.
///
/// Created by the runtime once per frame around the app's view function. The
/// frame does not retain anything itself — identity-keyed state lives in the
/// store, and the painted cells live in the buffer.
#[derive(Debug)]
pub struct Frame<'a> {
    buffer: &'a mut Buffer,
    store: &'a mut StateStore,
    /// Identity of the current declaration parent; child widgets compose
    /// their ids under it.
    parent: WidgetId,
}

impl<'a> Frame<'a> {
    /// Begins a frame over `buffer` with retained state in `store`.
    ///
    /// The runtime is responsible for calling [`StateStore::begin_frame`]
    /// before and [`StateStore::end_frame`] after the view function.
    #[must_use]
    pub fn new(buffer: &'a mut Buffer, store: &'a mut StateStore) -> Self {
        Self { buffer, store, parent: WidgetId::ROOT }
    }

    /// The full drawable area of this frame.
    #[must_use]
    pub fn area(&self) -> Rect {
        Rect::from_size(self.buffer.size())
    }

    /// Splits the frame's full area into horizontal bands
    /// (see [`split_rows`]).
    #[must_use]
    pub fn rows<const N: usize>(&self, constraints: [Constraint; N]) -> [Rect; N] {
        split_rows(self.area(), constraints)
    }

    /// Splits the frame's full area into vertical bands
    /// (see [`split_columns`]).
    #[must_use]
    pub fn columns<const N: usize>(&self, constraints: [Constraint; N]) -> [Rect; N] {
        split_columns(self.area(), constraints)
    }

    /// Declares a widget: composes its identity from `key` under the current
    /// parent, lends it its retained state, and renders it into `area`.
    pub fn widget<W: Widget>(&mut self, key: Key, area: Rect, widget: &W) {
        let id = self.parent.child(key);
        let state = self.store.get_or_default::<W::State>(id);
        let mut ctx = RenderCtx::new(self.buffer, area);
        widget.render(state, &mut ctx);
    }

    /// Declares a container scope: widgets declared inside `scope` compose
    /// their identities under `key`, so a reusable view function gets a
    /// distinct identity subtree per call site.
    pub fn scoped(&mut self, key: Key, scope: impl FnOnce(&mut Frame<'_>)) {
        let mut child = Frame {
            buffer: self.buffer,
            store: self.store,
            parent: self.parent.child(key),
        };
        scope(&mut child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Position, Size};
    use crate::id::key;
    use crate::style::Style;

    #[derive(Default)]
    struct CountState {
        renders: u32,
    }

    struct Probe;

    impl Widget for Probe {
        type State = CountState;
        fn render(&self, state: &mut CountState, ctx: &mut RenderCtx<'_>) {
            state.renders += 1;
            ctx.set_string(Position::ORIGIN, &state.renders.to_string(), Style::new());
        }
    }

    #[test]
    fn state_persists_across_frames_by_key() {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut store = StateStore::new();
        for _ in 0..3 {
            store.begin_frame();
            let mut frame = Frame::new(&mut buffer, &mut store);
            frame.widget(key("probe"), frame.area(), &Probe);
            store.end_frame();
        }
        assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "3");
    }

    #[test]
    fn scoped_keys_are_distinct_identities() {
        let mut buffer = Buffer::new(Size::new(8, 2));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        let [top, bottom] = frame.rows([Constraint::Length(1), Constraint::Length(1)]);
        // The same inner key under two scopes: two widgets, two states —
        // no duplicate-id panic.
        frame.scoped(key("left"), |f| f.widget(key("probe"), top, &Probe));
        frame.scoped(key("right"), |f| f.widget(key("probe"), bottom, &Probe));
        store.end_frame();
        assert_eq!(store.len(), 2);
    }
}
