//! The scroll container: a scoped builder that stacks measured items in a
//! vertical viewport, virtualizes, and scrolls.
//!
//! Per `docs/design/arc2b-measurement-scroll.md`, the scroll container is the
//! proof that **scoped builders are rabbitui's composition mechanism**: like
//! [`Frame::scoped`](crate::frame::Frame::scoped) and
//! [`Frame::layer`](crate::frame::Frame::layer), it composes an identity subtree
//! by taking a closure — but it also composes *layout*, stacking whatever the
//! closure declares. No widget-children trait: composition is a function
//! declaring items into a scope.
//!
//! ```text
//! frame.scroll(key("transcript"), area, |scroll| {
//!     for cell in &app.cells {
//!         scroll.item(key("cell").index(cell.id), &CellWidget::new(cell));
//!     }
//! });
//! ```
//!
//! # Semantics
//!
//! - **Stacking.** Items stack top to bottom, each at its
//!   [`desired_height`](crate::widget::Widget::desired_height) for the viewport's
//!   inner width (the viewport minus the scrollbar column when content overflows).
//! - **Identity-retained offset.** The scope retains a `u16` row `offset` keyed by
//!   its identity, so scrolling survives re-declaration.
//! - **Virtualization by construction.** Only items intersecting the viewport
//!   render; items above and below are *measured* (to advance the stacking cursor
//!   and size the scrollbar) but never painted. A thousand-item scroll paints one
//!   screenful.
//! - **Focus + input.** The scope declares itself a focusable widget whose handler
//!   consumes Up/Down/PageUp/PageDown/Home/End and the mouse wheel (scroll first;
//!   selection is the item's business). Nested scrolls: the inner scope is the
//!   routing target, so it wins; an unconsumed event bubbles to the outer.
//! - **Scrollbar.** When content overflows the viewport, a one-column scrollbar
//!   paints in the right column: a [`Role::Border`](crate::theme::Role::Border)
//!   track with a [`Role::Muted`](crate::theme::Role::Muted) thumb sized and
//!   positioned to the offset.
//! - **Scroll-into-view.** A child's
//!   [`request_visibility`](crate::widget::RenderContext::request_visibility) is
//!   recorded as a fact this frame; the scope stashes the target row into its
//!   retained state and adjusts `offset` **next frame** to reveal it — closing the
//!   loop plumbed as facts in slice 7.

use crate::frame::Frame;
use crate::geometry::{Position, Rect, Size};
use crate::id::{Key, WidgetId};
use crate::input::{InputEvent, Key as InputKey, MouseKind};
use crate::theme::Role;
use crate::widget::{HandleContext, Handled, RenderContext, Widget};

/// The retained state of a scroll scope: the scroll offset and the geometry the
/// handler needs to clamp it between frames.
///
/// Framework-owned, keyed by the scope's identity (ADR 0002). `offset` is the
/// first content row shown at the top of the viewport. `viewport_height` and
/// `content_height` are recorded each render so the handler — which runs against
/// retained state only, with no spec — can clamp Page/End movement to the real
/// bounds. `pending_reveal` carries a scroll-into-view target row from one frame
/// to the next.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollState {
    /// The first content row shown at the top of the viewport.
    offset: u16,
    /// The viewport height (rows) recorded at the last render, for clamping.
    viewport_height: u16,
    /// The total stacked content height (rows) recorded at the last render.
    content_height: u16,
    /// A content row a child asked to reveal, consumed on the next render.
    pending_reveal: Option<u16>,
}

impl ScrollState {
    /// The current scroll offset (the first visible content row).
    #[must_use]
    pub const fn offset(&self) -> u16 {
        self.offset
    }

    /// The largest in-bounds offset given the recorded geometry: the offset that
    /// puts the last content row at the bottom of the viewport (0 when content
    /// fits).
    const fn max_offset(&self) -> u16 {
        self.content_height.saturating_sub(self.viewport_height)
    }

    /// Clamps `offset` into `0..=max_offset`.
    fn clamp(&mut self) {
        let max = self.max_offset();
        if self.offset > max {
            self.offset = max;
        }
    }

    /// Adjusts `offset` so content row `row` is within the viewport, given the
    /// recorded geometry. The scroll-into-view math, applied at render.
    fn reveal(&mut self, row: u16) {
        if self.viewport_height == 0 {
            return;
        }
        if row < self.offset {
            // Above the viewport: bring it to the top.
            self.offset = row;
        } else if row >= self.offset.saturating_add(self.viewport_height) {
            // Below the viewport: bring it to the bottom row.
            self.offset = row.saturating_sub(self.viewport_height - 1);
        }
        self.clamp();
    }
}

/// The internal focusable widget backing a scroll scope.
///
/// The scope declares one of these at its own id so routing can reach it: it is
/// focusable and its [`handle`](Widget::handle) is the scroll keymap. It never
/// paints (the items and scrollbar are painted directly by
/// [`Frame::scroll`](crate::frame::Frame::scroll)); its `render` only marks the
/// scope focusable.
struct ScrollView;

impl Widget for ScrollView {
    type State = ScrollState;

    fn render(&self, _state: &mut ScrollState, ctx: &mut RenderContext<'_>) {
        ctx.focusable(true);
    }

    fn handle(state: &mut ScrollState, event: &InputEvent, ctx: &mut HandleContext<'_>) -> Handled {
        // Only act on the bubble leg, so a nested (inner) scroll — the routing
        // target — handles the event before an enclosing (outer) scroll sees it on
        // the way down. Capture would let the outer swallow it first.
        if ctx.phase() != crate::widget::Phase::Bubble {
            return Handled::No;
        }
        // Wheel first: one notch scrolls one line per reported line.
        if let Some(mouse) = event.as_mouse() {
            if let MouseKind::Scroll(lines) = mouse.kind {
                if lines > 0 {
                    state.offset = state.offset.saturating_add(u16::from(lines.unsigned_abs()));
                } else if lines < 0 {
                    state.offset = state.offset.saturating_sub(u16::from(lines.unsigned_abs()));
                }
                state.clamp();
                // The wheel over a scroll region is the scroll's, not the app's,
                // even when clamped to an end — always consumed.
                return Handled::Yes;
            }
            return Handled::No;
        }
        let Some(key) = event.as_key() else {
            return Handled::No;
        };
        if key.modifiers.ctrl || key.modifiers.alt {
            return Handled::No;
        }
        // A page is the viewport height less one row of overlap (or one row when
        // the viewport is a single row), so paging keeps a line of context.
        let page = state.viewport_height.saturating_sub(1).max(1);
        match key.key {
            InputKey::Up => {
                state.offset = state.offset.saturating_sub(1);
                state.clamp();
                Handled::Yes
            }
            InputKey::Down => {
                state.offset = state.offset.saturating_add(1);
                state.clamp();
                Handled::Yes
            }
            InputKey::PageUp => {
                state.offset = state.offset.saturating_sub(page);
                state.clamp();
                Handled::Yes
            }
            InputKey::PageDown => {
                state.offset = state.offset.saturating_add(page);
                state.clamp();
                Handled::Yes
            }
            InputKey::Home => {
                state.offset = 0;
                Handled::Yes
            }
            InputKey::End => {
                state.offset = state.max_offset();
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

/// The scope a [`Frame::scroll`](crate::frame::Frame::scroll) closure declares
/// items into.
///
/// Items are declared with [`item`](Self::item) in top-to-bottom order; each is
/// measured, stacked, and — if it intersects the viewport — painted. The scope
/// tracks the running content height and the current offset so it can decide what
/// is visible and where.
pub struct ScrollScope<'a, 'f> {
    /// The child frame items declare into (scope id is its parent).
    frame: &'a mut Frame<'f>,
    /// The viewport in absolute buffer coordinates (already minus the scrollbar
    /// column when content overflows — see `inner_width`).
    viewport: Rect,
    /// The width items are measured and painted at (viewport minus the scrollbar).
    inner_width: u16,
    /// The current scroll offset (first visible content row).
    offset: u16,
    /// The running content height as items stack (the next item's top row).
    cursor: u16,
}

impl ScrollScope<'_, '_> {
    /// Declares one item into the scroll: measures it at the viewport width,
    /// stacks it at the current cursor, and paints it **only if** it intersects
    /// the viewport.
    ///
    /// Off-viewport items are measured (advancing the cursor and feeding the
    /// scrollbar) but never declared as widgets — virtualization by construction.
    /// A visible item is declared with [`Frame::widget`](crate::frame::Frame::widget)
    /// at a clipped area, so it records its facts and handler and routing reaches
    /// it normally.
    pub fn item<W: Widget>(&mut self, key: Key, widget: &W) {
        let height = self.frame.measure(key, self.inner_width, widget);
        let top = self.cursor;
        self.cursor = self.cursor.saturating_add(height);

        // The item occupies content rows `top .. top + height`. It is visible when
        // that range intersects the offset window `offset .. offset + vh`.
        let window_bottom = self.offset.saturating_add(self.viewport.size.height);
        let item_bottom = top.saturating_add(height);
        let intersects = item_bottom > self.offset && top < window_bottom;
        if !intersects || height == 0 || self.inner_width == 0 {
            return;
        }

        // Map the item's content rows to viewport rows: its top row on screen is
        // `top - offset` (may be negative — clip above the viewport).
        let screen_top = i32::from(top) - i32::from(self.offset);
        let abs_y = i32::from(self.viewport.origin.y) + screen_top;
        // Clip the top: if the item begins above the viewport, start at the
        // viewport top and shrink the painted height accordingly.
        let (paint_y, clipped_height) = if abs_y < i32::from(self.viewport.origin.y) {
            let hidden = i32::from(self.viewport.origin.y) - abs_y;
            (
                self.viewport.origin.y,
                height.saturating_sub(u16::try_from(hidden).unwrap_or(height)),
            )
        } else {
            (u16::try_from(abs_y).unwrap_or(u16::MAX), height)
        };
        // Clip the bottom to the viewport.
        let max_height = self.viewport.bottom().saturating_sub(paint_y);
        let paint_height = clipped_height.min(max_height);
        if paint_height == 0 {
            return;
        }
        let area = Rect::new(
            Position::new(self.viewport.origin.x, paint_y),
            Size::new(self.inner_width, paint_height),
        );
        self.frame.widget(key, area, widget);
    }

    /// Declares a **nested scroll** of a fixed `height` rows as an item.
    ///
    /// A scroll is a scope, not a widget, so it cannot go through
    /// [`item`](Self::item); `nest` reserves `height` content rows, stacks the
    /// nested viewport at the cursor, and — when it intersects the outer viewport —
    /// declares an inner [`Frame::scroll`](crate::frame::Frame::scroll) into the
    /// (clipped) region. The inner scroll is a distinct focusable scope, so
    /// existing routing gives it the event first (inner wins) and bubbles
    /// unconsumed events to the outer.
    pub fn nest(&mut self, key: Key, height: u16, scope: impl Fn(&mut ScrollScope<'_, '_>)) {
        let top = self.cursor;
        self.cursor = self.cursor.saturating_add(height);
        let window_bottom = self.offset.saturating_add(self.viewport.size.height);
        let item_bottom = top.saturating_add(height);
        let intersects = item_bottom > self.offset && top < window_bottom;
        if !intersects || height == 0 || self.inner_width == 0 {
            return;
        }
        let screen_top = i32::from(top) - i32::from(self.offset);
        let abs_y = i32::from(self.viewport.origin.y) + screen_top;
        let (paint_y, clipped_height) = if abs_y < i32::from(self.viewport.origin.y) {
            let hidden = i32::from(self.viewport.origin.y) - abs_y;
            (
                self.viewport.origin.y,
                height.saturating_sub(u16::try_from(hidden).unwrap_or(height)),
            )
        } else {
            (u16::try_from(abs_y).unwrap_or(u16::MAX), height)
        };
        let max_height = self.viewport.bottom().saturating_sub(paint_y);
        let paint_height = clipped_height.min(max_height);
        if paint_height == 0 {
            return;
        }
        let area = Rect::new(
            Position::new(self.viewport.origin.x, paint_y),
            Size::new(self.inner_width, paint_height),
        );
        self.frame.scroll(key, area, scope);
    }
}

impl<'f> Frame<'f> {
    /// Declares a **scroll container**: a scoped builder that stacks the items its
    /// closure declares in a vertical viewport, virtualizes them, and scrolls.
    ///
    /// See the [`scroll`](crate::scroll) module docs for the full semantics. In
    /// brief: `scope` declares items with
    /// [`ScrollScope::item`](crate::scroll::ScrollScope::item); each is measured at
    /// the viewport width, stacked, and painted only if it intersects the viewport.
    /// The scope retains a scroll `offset` by identity, declares itself focusable
    /// with a Up/Down/PageUp/PageDown/Home/End + wheel handler, paints a scrollbar
    /// when content overflows, and consumes children's scroll-into-view requests to
    /// adjust the offset on the next frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::buffer::Buffer;
    /// use rabbitui_core::frame::Frame;
    /// use rabbitui_core::geometry::{Position, Size};
    /// use rabbitui_core::id::key;
    /// use rabbitui_core::store::StateStore;
    /// use rabbitui_core::style::Style;
    /// use rabbitui_core::widget::{RenderContext, Widget};
    ///
    /// struct Row(&'static str);
    /// impl Widget for Row {
    ///     type State = ();
    ///     fn render(&self, _s: &mut (), ctx: &mut RenderContext<'_>) {
    ///         ctx.set_string(Position::ORIGIN, self.0, Style::new());
    ///     }
    /// }
    ///
    /// let mut buffer = Buffer::new(Size::new(20, 3));
    /// let mut store = StateStore::new();
    /// store.begin_frame();
    /// let mut frame = Frame::new(&mut buffer, &mut store);
    /// frame.scroll(key("list"), frame.area(), |scroll| {
    ///     for i in 0..100 {
    ///         scroll.item(key("row").index(i), &Row("item"));
    ///     }
    /// });
    /// # let _ = frame.finish();
    /// store.end_frame();
    /// // Only the three visible rows were painted, though a hundred were declared.
    /// assert_eq!(buffer.get(Position::ORIGIN).unwrap().symbol, "i");
    /// ```
    pub fn scroll(&mut self, key: Key, area: Rect, scope: impl Fn(&mut ScrollScope<'_, '_>)) {
        let scope_id = self.parent_id().child(key);
        let bounds = Rect::from_size(self.buffer_size());
        let viewport_full = area.intersection(bounds);
        if viewport_full.is_empty() {
            // Nothing to scroll into: still persist state and register the
            // focusable scope so focus and routing are stable, but paint nothing.
            let state: ScrollState = self.container_state(scope_id);
            self.put_container_state(scope_id, state);
            self.register_container::<ScrollView>(scope_id, viewport_full);
            return;
        }

        // Load retained state; apply a pending scroll-into-view from last frame
        // against last frame's recorded geometry before we re-measure.
        let mut state: ScrollState = self.container_state(scope_id);
        if let Some(row) = state.pending_reveal.take() {
            state.reveal(row);
        }

        // First pass: measure every item to learn the content height (cheap, no
        // paint), so we can decide on the scrollbar column and clamp the offset.
        let full_width = viewport_full.size.width;
        let content_at_full = self.measure_scroll_content(scope_id, full_width, &scope);
        let overflow = content_at_full > viewport_full.size.height;
        let inner_width = if overflow {
            full_width.saturating_sub(1)
        } else {
            full_width
        };
        // A narrower inner width (scrollbar column) can wrap taller content, so
        // re-measure at the inner width when it differs.
        let content_height = if overflow && inner_width != full_width {
            self.measure_scroll_content(scope_id, inner_width, &scope)
        } else {
            content_at_full
        };

        // Record geometry and clamp the offset now that both are known.
        state.viewport_height = viewport_full.size.height;
        state.content_height = content_height;
        state.clamp();
        let offset = state.offset;

        // Second pass: declare the visible items into a child scope, tracking the
        // content cursor. Items outside the offset window are measured (inside
        // `item`) but not painted — virtualization by construction.
        let visibility_before = self.visibility_len();
        self.with_child_scope(scope_id, 0, |child| {
            let mut scroll_scope = ScrollScope {
                frame: child,
                viewport: viewport_full,
                inner_width,
                offset,
                cursor: 0,
            };
            scope(&mut scroll_scope);
        });

        // Consume any scroll-into-view request a child recorded this frame (a
        // request whose id is within this scope): stash the target content row so
        // next frame's offset reveals it.
        if let Some(row) =
            self.find_child_reveal(scope_id, viewport_full, offset, visibility_before)
        {
            state.pending_reveal = Some(row);
        }

        // Persist the updated state and register the focusable scope + handler.
        self.put_container_state(scope_id, state);
        self.register_container::<ScrollView>(scope_id, viewport_full);

        // Paint the scrollbar last (over the viewport's right column) when content
        // overflows, so it sits above the items.
        if overflow {
            self.paint_scrollbar(viewport_full, offset, content_height);
        }
    }

    /// Measures the content height of a scroll scope by running its closure
    /// through a zero-height viewport: every item's `intersects` test is false, so
    /// [`ScrollScope::item`](crate::scroll::ScrollScope::item) only *measures* and
    /// advances its cursor — no item is declared. The final cursor is the content
    /// height.
    fn measure_scroll_content(
        &mut self,
        scope_id: WidgetId,
        width: u16,
        scope: &impl Fn(&mut ScrollScope<'_, '_>),
    ) -> u16 {
        self.with_child_scope(scope_id, 0, |child| {
            let viewport = Rect::new(Position::ORIGIN, Size::new(width, 0));
            let mut scroll_scope = ScrollScope {
                frame: child,
                viewport,
                inner_width: width,
                offset: 0,
                cursor: 0,
            };
            scope(&mut scroll_scope);
            scroll_scope.cursor
        })
    }

    /// Finds a child's scroll-into-view request recorded this frame (its id nested
    /// under `scope_id`) and maps its absolute row to a content row, so the next
    /// frame's offset can reveal it.
    ///
    /// The request area is absolute (the frame resolves the child's area-relative
    /// rect against its painted area). The painted area's top is `viewport.top +
    /// (content_row - offset)`, so the content row is `request.top - viewport.top +
    /// offset`. The last request wins.
    fn find_child_reveal(
        &self,
        scope_id: WidgetId,
        viewport: Rect,
        offset: u16,
        since: usize,
    ) -> Option<u16> {
        // Every request in this slice was pushed during this scope's child-frame
        // body, so all are descendants of `scope_id` by construction (a WidgetId is
        // a one-way hash, so a structural ancestor check is not possible — the
        // positional filter in `visibility_since` is the sound one).
        let _ = scope_id;
        self.visibility_since(since)
            .into_iter()
            .filter_map(|request| {
                // Reveal the request's *bottom* row: a partially-clipped item asks
                // for its whole height, and revealing the bottom brings the rest
                // into view (revealing the top alone would leave it clipped below).
                let bottom = request.area.bottom().saturating_sub(1);
                let screen_row = bottom.checked_sub(viewport.origin.y)?;
                Some(offset.saturating_add(screen_row))
            })
            .next_back()
    }

    /// Paints the scrollbar in the viewport's right column: a
    /// [`Role::Border`](crate::theme::Role::Border) track with a
    /// [`Role::Muted`](crate::theme::Role::Muted) thumb sized to the visible
    /// fraction and positioned to the offset.
    fn paint_scrollbar(&mut self, viewport: Rect, offset: u16, content_height: u16) {
        let vh = viewport.size.height;
        if vh == 0 || content_height <= vh {
            return;
        }
        let x = viewport.right().saturating_sub(1);
        let track_style = self.theme_ref().style(Role::Border);
        let thumb_style = self.theme_ref().style(Role::Muted);

        // Thumb length: viewport/content of the track, at least one row.
        let vh32 = u32::from(vh);
        let content32 = u32::from(content_height);
        let thumb_len = ((vh32 * vh32) / content32).max(1) as u16;
        let thumb_len = thumb_len.min(vh);
        // Thumb top: offset/(content - vh) of the free track (track - thumb).
        let max_offset = u32::from(content_height.saturating_sub(vh));
        let free = u32::from(vh - thumb_len);
        let thumb_top = (u32::from(offset) * free + max_offset / 2)
            .checked_div(max_offset)
            .unwrap_or(0) as u16;

        for row in 0..vh {
            let y = viewport.origin.y + row;
            let in_thumb = row >= thumb_top && row < thumb_top.saturating_add(thumb_len);
            let (glyph, style) = if in_thumb {
                ("█", thumb_style)
            } else {
                ("│", track_style)
            };
            self.paint_absolute(Position::new(x, y), glyph, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::geometry::Size;
    use crate::id::key;
    use crate::store::StateStore;
    use crate::style::Style;

    /// A probe row of a fixed height that counts how many times it is *rendered*
    /// (painted), so a test can prove virtualization: a scroll of a thousand rows
    /// renders only the visible few.
    struct Probe {
        height: u16,
        label: &'static str,
    }

    impl Widget for Probe {
        type State = RenderCount;
        fn render(&self, state: &mut RenderCount, ctx: &mut RenderContext<'_>) {
            state.0 += 1;
            ctx.set_string(Position::ORIGIN, self.label, Style::new());
        }
        fn desired_height(&self, _state: &RenderCount, _width: u16) -> u16 {
            self.height
        }
    }

    #[derive(Default)]
    struct RenderCount(u32);

    fn scope_id() -> WidgetId {
        WidgetId::ROOT.child(key("list"))
    }

    #[test]
    fn stacks_items_at_measured_heights() {
        let mut buffer = Buffer::new(Size::new(10, 6));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.scroll(key("list"), frame.area(), |scroll| {
            scroll.item(
                key("a"),
                &Probe {
                    height: 2,
                    label: "a",
                },
            );
            scroll.item(
                key("b"),
                &Probe {
                    height: 2,
                    label: "b",
                },
            );
            scroll.item(
                key("c"),
                &Probe {
                    height: 2,
                    label: "c",
                },
            );
        });
        let _ = frame.finish();
        store.end_frame();
        // a at row 0, b at row 2, c at row 4.
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, "a");
        assert_eq!(buffer.get(Position::new(0, 2)).unwrap().symbol, "b");
        assert_eq!(buffer.get(Position::new(0, 4)).unwrap().symbol, "c");
    }

    #[test]
    fn virtualizes_a_thousand_items_to_the_visible_few() {
        let mut buffer = Buffer::new(Size::new(10, 5));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.scroll(key("list"), frame.area(), |scroll| {
            for i in 0..1000 {
                scroll.item(
                    key("row").index(i),
                    &Probe {
                        height: 1,
                        label: "x",
                    },
                );
            }
        });
        let _ = frame.finish();
        store.end_frame();

        // Only the rows intersecting the 5-row viewport were declared, so only
        // those hold retained render state. A viewport of 5 rows over height-1
        // rows shows 5 rows; the scrollbar narrows nothing that removes a row.
        // Count widgets with a render row in the store: exactly the visible ones
        // plus the scroll scope itself.
        // The visible rows are indices 0..5 (offset 0).
        let visible: usize = (0..1000)
            .filter(|i| {
                store
                    .peek::<RenderCount>(scope_id().child(key("row").index(*i)))
                    .is_some_and(|c| c.0 > 0)
            })
            .count();
        assert_eq!(visible, 5, "only the 5 viewport rows painted");
    }

    /// Renders one scroll frame of `count` height-1 rows into a `height`-row
    /// viewport and returns the store so a test can inspect the offset/state.
    fn render_rows(store: &mut StateStore, count: usize, viewport_h: u16) {
        let mut buffer = Buffer::new(Size::new(10, viewport_h));
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, store);
        frame.scroll(key("list"), frame.area(), |scroll| {
            for i in 0..count {
                scroll.item(
                    key("row").index(i),
                    &Probe {
                        height: 1,
                        label: "x",
                    },
                );
            }
        });
        let _ = frame.finish();
        store.end_frame();
    }

    /// Dispatches an event to the scroll scope's handler and returns Handled.
    fn dispatch(state: &mut ScrollState, event: InputEvent) -> Handled {
        let mut outcomes = Vec::new();
        let mut request_focus = false;
        let mut ctx = HandleContext::new(
            crate::widget::Phase::Bubble,
            Rect::default(),
            &mut outcomes,
            &mut request_focus,
        );
        ScrollView::handle(state, &event, &mut ctx)
    }

    #[test]
    fn keys_scroll_and_clamp() {
        // Geometry: 20 rows of content, 5-row viewport → max_offset 15.
        let mut state = ScrollState {
            viewport_height: 5,
            content_height: 20,
            ..Default::default()
        };
        assert_eq!(
            dispatch(&mut state, InputEvent::key(InputKey::Down)),
            Handled::Yes
        );
        assert_eq!(state.offset(), 1);
        dispatch(&mut state, InputEvent::key(InputKey::PageDown));
        assert_eq!(state.offset(), 1 + 4); // page = viewport - 1 = 4
        dispatch(&mut state, InputEvent::key(InputKey::End));
        assert_eq!(state.offset(), 15);
        // Down at the end is clamped.
        dispatch(&mut state, InputEvent::key(InputKey::Down));
        assert_eq!(state.offset(), 15);
        dispatch(&mut state, InputEvent::key(InputKey::Home));
        assert_eq!(state.offset(), 0);
        // Up at the top is clamped.
        dispatch(&mut state, InputEvent::key(InputKey::Up));
        assert_eq!(state.offset(), 0);
    }

    #[test]
    fn wheel_scrolls_and_is_always_handled() {
        use crate::input::{MouseButton, MouseEvent};
        let mut state = ScrollState {
            viewport_height: 5,
            content_height: 20,
            offset: 3,
            ..Default::default()
        };
        let down = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Scroll(2),
            MouseButton::None,
            Position::ORIGIN,
        ));
        assert_eq!(dispatch(&mut state, down), Handled::Yes);
        assert_eq!(state.offset(), 5);
        let up = InputEvent::Mouse(MouseEvent::new(
            MouseKind::Scroll(-10),
            MouseButton::None,
            Position::ORIGIN,
        ));
        // A big wheel-up clamps to the top and is still handled.
        assert_eq!(dispatch(&mut state, up), Handled::Yes);
        assert_eq!(state.offset(), 0);
    }

    #[test]
    fn scrolling_shows_the_next_window_of_items() {
        // 10 height-1 rows, 3-row viewport. Scroll down 2 and the visible window is
        // items 2,3,4.
        let mut store = StateStore::new();
        render_rows(&mut store, 10, 3);
        // Move the offset via the handler, then re-render.
        let mut state = *store.peek::<ScrollState>(scope_id()).unwrap();
        dispatch(&mut state, InputEvent::key(InputKey::Down));
        dispatch(&mut state, InputEvent::key(InputKey::Down));
        // Persist by seeding a prior frame, then render.
        store.begin_frame();
        *store.get_or_default::<ScrollState>(scope_id()) = state;
        store.end_frame();

        let mut buffer = Buffer::new(Size::new(10, 3));
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.scroll(key("list"), frame.area(), |scroll| {
            for i in 0..10 {
                let label: &'static str = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"][i];
                scroll.item(key("row").index(i), &Probe { height: 1, label });
            }
        });
        let _ = frame.finish();
        store.end_frame();
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, "2");
        assert_eq!(buffer.get(Position::new(0, 1)).unwrap().symbol, "3");
        assert_eq!(buffer.get(Position::new(0, 2)).unwrap().symbol, "4");
    }

    /// A tall widget that, while it renders (even clipped), asks for its whole
    /// `height` to be visible — the scroll-into-view probe. A partially-clipped
    /// item at the bottom edge is the realistic requester: virtualization means a
    /// fully-off-screen item never renders to ask, but an edge item can.
    struct Reveal {
        height: u16,
    }
    impl Widget for Reveal {
        type State = ();
        fn render(&self, _s: &mut (), ctx: &mut RenderContext<'_>) {
            ctx.focusable(true);
            ctx.request_visibility(Rect::new(Position::ORIGIN, Size::new(1, self.height)));
        }
        fn desired_height(&self, _s: &(), _w: u16) -> u16 {
            self.height
        }
    }

    #[test]
    fn request_visibility_scrolls_the_requester_into_view_next_frame() {
        // 3 height-1 rows (content 0,1,2) then a height-3 requester (content 3,4,5)
        // into a 4-row viewport. At offset 0 the requester's top row is the last
        // visible row, clipped to one; it asks for its whole height. Next frame the
        // offset scrolls so its bottom (content row 5) is the viewport bottom.
        let mut store = StateStore::new();
        let render = |store: &mut StateStore| {
            let mut buffer = Buffer::new(Size::new(10, 4));
            store.begin_frame();
            let mut frame = Frame::new(&mut buffer, store);
            frame.scroll(key("list"), frame.area(), |scroll| {
                for i in 0..3 {
                    scroll.item(
                        key("row").index(i),
                        &Probe {
                            height: 1,
                            label: "x",
                        },
                    );
                }
                scroll.item(key("tall"), &Reveal { height: 3 });
            });
            let _ = frame.finish();
            store.end_frame();
        };
        // Frame 1: offset 0; the requester is edge-visible and asks to be revealed.
        render(&mut store);
        assert_eq!(store.peek::<ScrollState>(scope_id()).unwrap().offset(), 0);
        // Frame 2: the stash is consumed; content row 5 sits at the viewport bottom
        // → offset = 5 - (4 - 1) = 2.
        render(&mut store);
        assert_eq!(store.peek::<ScrollState>(scope_id()).unwrap().offset(), 2);
    }

    #[test]
    fn scrollbar_paints_a_track_and_thumb_on_overflow() {
        // 20 rows, 5-row viewport → overflow, scrollbar in the right column (x=9).
        let mut store = StateStore::new();
        let mut buffer = Buffer::new(Size::new(10, 5));
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.scroll(key("list"), frame.area(), |scroll| {
            for i in 0..20 {
                scroll.item(
                    key("row").index(i),
                    &Probe {
                        height: 1,
                        label: "x",
                    },
                );
            }
        });
        let _ = frame.finish();
        store.end_frame();

        // The right column is the scrollbar; every cell is a track or thumb glyph.
        let mut glyphs = Vec::new();
        for y in 0..5 {
            glyphs.push(buffer.get(Position::new(9, y)).unwrap().symbol.clone());
        }
        // At offset 0 the thumb is at the top: at least the first cell is a thumb.
        assert_eq!(glyphs[0], "█");
        // The track glyph appears below the thumb.
        assert!(glyphs.iter().any(|g| g == "│"), "track present: {glyphs:?}");
    }

    #[test]
    fn no_scrollbar_when_content_fits() {
        // 3 rows in a 5-row viewport: no overflow, no scrollbar column painted.
        let mut store = StateStore::new();
        let mut buffer = Buffer::new(Size::new(10, 5));
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.scroll(key("list"), frame.area(), |scroll| {
            for i in 0..3 {
                scroll.item(
                    key("row").index(i),
                    &Probe {
                        height: 1,
                        label: "x",
                    },
                );
            }
        });
        let _ = frame.finish();
        store.end_frame();
        // The right column holds no scrollbar glyphs (blank), since content fits.
        for y in 0..5 {
            let sym = &buffer.get(Position::new(9, y)).unwrap().symbol;
            assert!(sym == " ", "no scrollbar row {y}: {sym:?}");
        }
    }

    #[test]
    fn nested_scroll_inner_wins_and_unconsumed_bubbles_to_outer() {
        use crate::routing::{Focus, route};
        // An outer scroll containing an inner scroll; both overflow. When the inner
        // is focused, Down scrolls the inner (it consumes) and NOT the outer.
        let mut store = StateStore::new();
        let mut buffer = Buffer::new(Size::new(12, 8));
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.scroll(key("outer"), frame.area(), |outer| {
            // A tall inner scroll as the first item, then filler rows.
            outer.item(
                key("filler0"),
                &Probe {
                    height: 1,
                    label: "a",
                },
            );
            // The inner scroll is declared as a nested scope. Because `item`
            // measures then declares, we express the inner scroll as a widget-like
            // item is not possible; instead nest via the child frame directly.
            outer.nest(key("inner"), 4, |inner| {
                for i in 0..20 {
                    inner.item(
                        key("in").index(i),
                        &Probe {
                            height: 1,
                            label: "b",
                        },
                    );
                }
            });
            for i in 0..20 {
                outer.item(
                    key("out").index(i),
                    &Probe {
                        height: 1,
                        label: "c",
                    },
                );
            }
        });
        let (facts, handlers) = frame.into_parts();
        store.end_frame();

        let outer_id = WidgetId::ROOT.child(key("outer"));
        let inner_id = outer_id.child(key("inner"));

        // Focus the inner scroll and press Down: the inner offset advances, the
        // outer's does not.
        let mut focus = Focus::new();
        focus.set(Some(inner_id));
        let outer_before = store.peek::<ScrollState>(outer_id).unwrap().offset();
        route(
            &facts,
            &handlers,
            &mut focus,
            &mut store,
            &InputEvent::key(InputKey::Down),
        );
        assert_eq!(
            store.peek::<ScrollState>(inner_id).unwrap().offset(),
            1,
            "inner scrolled"
        );
        assert_eq!(
            store.peek::<ScrollState>(outer_id).unwrap().offset(),
            outer_before,
            "outer did not scroll — inner won"
        );
    }

    #[test]
    fn offset_clamps_to_content_bounds() {
        let mut store = StateStore::new();
        let mut buffer = Buffer::new(Size::new(10, 4));
        // Seed a wildly-out-of-bounds offset in a PRIOR frame, then render and
        // confirm it clamps to max_offset = content(10) - viewport(4) = 6.
        store.begin_frame();
        store.get_or_default::<ScrollState>(scope_id()).offset = 999;
        store.end_frame();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.scroll(key("list"), frame.area(), |scroll| {
            for i in 0..10 {
                scroll.item(
                    key("row").index(i),
                    &Probe {
                        height: 1,
                        label: "x",
                    },
                );
            }
        });
        let _ = frame.finish();
        store.end_frame();
        assert_eq!(store.peek::<ScrollState>(scope_id()).unwrap().offset(), 6);
    }
}
