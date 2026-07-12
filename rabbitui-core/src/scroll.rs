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
//! - **Anchor scrolling** (`docs/plans/wave-b2-virtualization.md`). The scroll
//!   position is an *anchor* — `(item index, rows of it hidden above the
//!   viewport top)` — retained by the scope's identity, not an absolute row
//!   offset. An absolute offset needs the summed height of everything above it
//!   (O(n) with variable heights); an anchor needs only the heights of items
//!   actually walked, so every frame costs O(viewport), never O(items).
//! - **Measure cache.** Item heights near the anchor window are re-measured
//!   every frame (so a visible item that changes height relayouts immediately);
//!   heights measured for *distant* walks (an End jump, a scroll-into-view
//!   target) are cached per item and reused until the width changes or
//!   [`ScrollState::invalidate`] is called. See that method for the rules.
//! - **Virtualization by construction.** Only items intersecting the viewport
//!   render and declare facts; everything else is at most measured. A
//!   million-item scroll paints one screenful and measures one windowful.
//! - **Partial items.** A top-clipped item is declared with a hidden-top mask
//!   ([`RenderContext::with_hidden_top`](crate::widget::RenderContext::with_hidden_top),
//!   `docs/design/render-space.md`): it renders its full logical extent and the
//!   *bottom* rows land on screen — never the wrong (top) slice. Its recorded
//!   fact carries the clipped visible rect, so hit-testing acts on what is on
//!   screen.
//! - **Focus + input.** The scope declares itself a focusable widget whose handler
//!   consumes Up/Down/PageUp/PageDown/Home/End and the mouse wheel (scroll first;
//!   selection is the item's business). Movement is queued as pending rows and
//!   applied at the next render, where item heights are known; nested scrolls:
//!   the inner scope is the routing target, so it wins; an unconsumed event
//!   bubbles to the outer.
//! - **Scrollbar.** When content overflows the viewport, a one-column scrollbar
//!   paints in the right column: a [`Role::Border`] track with a
//!   [`Role::Muted`] thumb. Thumb size
//!   and position are the *item-fraction approximation* (`emitted / len` and
//!   `anchor / (len - emitted)`) — exact for uniform heights, proportional
//!   otherwise; a true total height is never computed.
//! - **Scroll-into-view.** A child's
//!   [`request_visibility`](crate::widget::RenderContext::request_visibility) is
//!   recorded as a fact this frame; the scope maps it to the requesting item and
//!   adjusts the anchor **next frame** to reveal it.

use std::cell::RefCell;
use std::rc::Rc;

use crate::frame::Frame;
use crate::geometry::{Position, Rect, Size};
use crate::id::{Key, WidgetId};
use crate::input::{InputEvent, Key as InputKey, MouseKind};
use crate::theme::Role;
use crate::widget::{HandleContext, Handled, RenderContext, Widget};

/// A scroll anchor: the first (partially) visible item and how many of its
/// rows are hidden above the viewport top.
type Anchor = (usize, u16);

/// A half-open item-index range a settle walk found unmeasured; the caller
/// measures it and retries.
type NeededRange = (usize, usize);

/// Slack added to walk bounds and measure ranges: covers the overscan
/// neighbors and bounds backward walks against pathological zero-height runs
/// (a run longer than this anchors conservatively — a strictly larger window —
/// rather than walking O(n)).
const WALK_SLACK: usize = 32;

/// The lazily-filled per-item height cache backing a scroll scope's anchor
/// walks.
///
/// `heights` is indexed by item position and sized to the source length —
/// a `Vec<Option<u16>>` deliberately, not a map: at a million items this is
/// ~4 MB, and the dense layout is what keeps the walk loops cache-friendly
/// (the adjudicated tradeoff, `docs/plans/wave-b2-virtualization.md`).
/// `width` is the validity key: heights depend on wrap width, so a width
/// change clears everything.
#[derive(Debug, Default)]
struct MeasureCache {
    /// The width every cached height was measured at; a change clears all.
    width: u16,
    /// Cached heights by item index; `None` = not yet measured at this width.
    heights: Vec<Option<u16>>,
}

impl MeasureCache {
    /// Keys the cache to `width`, clearing every height if it changed.
    fn set_width(&mut self, width: u16) {
        if width != self.width {
            self.heights.clear();
            self.width = width;
        }
    }

    /// Drops every cached height, keeping the width key.
    fn clear(&mut self) {
        self.heights.clear();
    }

    /// Resizes to the source length: appended items start unmeasured, removed
    /// items drop. (An insertion or removal *before* the end shifts indices —
    /// that is what [`ScrollState::invalidate`] is for.)
    fn resize(&mut self, len: usize) {
        self.heights.resize(len, None);
    }

    /// The cached height of item `i`, if measured at the current width.
    fn cached(&self, i: usize) -> Option<u16> {
        self.heights.get(i).copied().flatten()
    }

    /// The height of item `i`, measuring (and caching) on a miss.
    fn get(&mut self, i: usize, measure: impl FnOnce() -> u16) -> u16 {
        if i >= self.heights.len() {
            self.heights.resize(i + 1, None);
        }
        if let Some(height) = self.heights[i] {
            return height;
        }
        let height = measure();
        self.heights[i] = Some(height);
        height
    }

    /// Records a just-measured height for item `i`, replacing any cached one
    /// (the fresh-window path: visible items re-measure every frame).
    fn set(&mut self, i: usize, height: u16) {
        if i >= self.heights.len() {
            self.heights.resize(i + 1, None);
        }
        self.heights[i] = Some(height);
    }
}

/// The retained state of a scroll scope: the anchor, the measure cache, the
/// geometry the handler needs for paging, and the movement queued since the
/// last render.
///
/// Framework-owned, keyed by the scope's identity (ADR 0002). The anchor is
/// `(anchor_item, anchor_offset)`: the first (partially) visible item and how
/// many of its rows are hidden above the viewport top. Event handlers queue
/// movement into the `pending_*` fields — normalizing an anchor needs item
/// heights, and those are only measurable at render — so a key press takes
/// effect on the next frame, exactly as the old offset clamp did.
#[derive(Debug, Clone, Default)]
pub struct ScrollState {
    /// The first (partially) visible item.
    anchor_item: usize,
    /// Rows of the anchor item hidden above the viewport top (always less
    /// than the item's height after a render).
    anchor_offset: u16,
    /// The viewport height (rows) recorded at the last render, for page math.
    viewport_height: u16,
    /// The number of items declared at the last render, for the scrollbar and
    /// End clamping.
    item_count: usize,
    /// Whether content overflowed at the last render (the scrollbar column is
    /// reserved). Used as the width guess for the next frame's measures.
    scrollbar: bool,
    /// Rows of movement queued by the handler, applied at the next render
    /// (positive scrolls down).
    pending_scroll: i32,
    /// An End jump queued by the handler, applied at the next render.
    pending_end: bool,
    /// An item a child asked to reveal, consumed on the next render.
    pending_reveal: Option<usize>,
    /// The per-item height cache. Behind a shared handle so cloning the state
    /// (the container-state seam does, every frame) stays O(1) instead of
    /// copying a million-entry vector.
    cache: Rc<RefCell<MeasureCache>>,
}

impl ScrollState {
    /// The scroll anchor: `(item index, rows of it hidden above the viewport
    /// top)`. `(0, 0)` is scrolled to the top.
    #[must_use]
    pub fn anchor(&self) -> (usize, u16) {
        (self.anchor_item, self.anchor_offset)
    }

    /// Drops every cached item height, keeping the anchor. Heights re-measure
    /// on the next frame.
    ///
    /// **When to call it:** after editing content *away from the visible
    /// window* in a way that changes item heights or shifts item indices — an
    /// insertion or removal anywhere but the end, a collapse-all, a filter
    /// change. Visible items re-measure every frame regardless, and appending
    /// items needs nothing (new indices start unmeasured), but a stale cached
    /// height for a *distant* item would misplace jumps that walk it (End, a
    /// scroll-into-view, a large wheel delta).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::scroll::ScrollState;
    ///
    /// let mut state = ScrollState::default();
    /// // …the app removed an item far above the viewport…
    /// state.invalidate(); // distant heights re-measure on the next frame
    /// assert_eq!(state.anchor(), (0, 0)); // the anchor is kept
    /// ```
    pub fn invalidate(&mut self) {
        self.cache.borrow_mut().clear();
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
        // Wheel first: one notch scrolls one line per reported line. Movement
        // is queued in rows and applied at the next render, where the anchor
        // can be normalized against real item heights.
        if let Some(mouse) = event.as_mouse() {
            if let MouseKind::Scroll(lines) = mouse.kind {
                state.pending_scroll = state.pending_scroll.saturating_add(i32::from(lines));
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
        let page = i32::from(state.viewport_height.saturating_sub(1).max(1));
        match key.key {
            InputKey::Up => {
                state.pending_scroll = state.pending_scroll.saturating_sub(1);
                Handled::Yes
            }
            InputKey::Down => {
                state.pending_scroll = state.pending_scroll.saturating_add(1);
                Handled::Yes
            }
            InputKey::PageUp => {
                state.pending_scroll = state.pending_scroll.saturating_sub(page);
                Handled::Yes
            }
            InputKey::PageDown => {
                state.pending_scroll = state.pending_scroll.saturating_add(page);
                Handled::Yes
            }
            InputKey::Home => {
                state.anchor_item = 0;
                state.anchor_offset = 0;
                state.pending_scroll = 0;
                state.pending_end = false;
                Handled::Yes
            }
            InputKey::End => {
                state.pending_scroll = 0;
                state.pending_end = true;
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

// ---- Anchor walks (pure math over cached heights) ---------------------------
//
// Every walk touches O(viewport) heights. A walk that reaches an unmeasured
// item returns the range it needs as `Err`; the settle loop measures it and
// retries. Walks that could cross unbounded zero-height runs are bounded at
// viewport + WALK_SLACK items and anchor conservatively past that.

/// The forward range worth measuring when a walk missed at `i`.
fn forward_need(i: usize, viewport_height: u16) -> NeededRange {
    (
        i,
        i.saturating_add(usize::from(viewport_height) + WALK_SLACK + 1),
    )
}

/// The backward range worth measuring when a walk missed at `i`.
fn backward_need(i: usize, viewport_height: u16) -> NeededRange {
    (
        i.saturating_sub(usize::from(viewport_height) + WALK_SLACK),
        i + 1,
    )
}

/// Walks backward from the bottom of item `last` until `viewport_height` rows
/// are covered; the walk's end is the anchor that puts `last`'s bottom row on
/// the viewport's last row. This is the maximum in-bounds anchor when `last`
/// is the final item.
fn walk_back(
    cache: &MeasureCache,
    last: usize,
    viewport_height: u16,
) -> Result<Anchor, NeededRange> {
    let want = u32::from(viewport_height);
    let mut rows: u32 = 0;
    let mut i = last;
    loop {
        let height = u32::from(
            cache
                .cached(i)
                .ok_or_else(|| backward_need(i, viewport_height))?,
        );
        if rows + height >= want {
            return Ok((i, u16::try_from(rows + height - want).unwrap_or(u16::MAX)));
        }
        rows += height;
        if i == 0 {
            return Ok((0, 0));
        }
        // Bound the walk against zero-height runs: anchoring a little early
        // only widens the window.
        if last - i > usize::from(viewport_height) + WALK_SLACK {
            return Ok((i, 0));
        }
        i -= 1;
    }
}

/// Normalizes an anchor so its offset is within its item's height, walking
/// forward (the offset can exceed the item after a height shrank or a queued
/// scroll). Past the last item it pins inside the last item; the under-fill
/// clamp repairs the exact position.
fn normalize(
    cache: &MeasureCache,
    anchor: Anchor,
    len: usize,
    viewport_height: u16,
) -> Result<Anchor, NeededRange> {
    let (mut item, mut offset) = anchor;
    while item < len {
        let height = cache
            .cached(item)
            .ok_or_else(|| forward_need(item, viewport_height))?;
        if offset < height {
            return Ok((item, offset));
        }
        if item + 1 == len {
            return Ok((item, height.saturating_sub(1).min(offset)));
        }
        offset -= height;
        item += 1;
    }
    Ok((item.min(len.saturating_sub(1)), 0))
}

/// Rows of content visible from `anchor`, capped at `viewport_height` — the
/// under-fill probe (a full window returns exactly the viewport height).
fn fill_rows(
    cache: &MeasureCache,
    anchor: Anchor,
    viewport_height: u16,
    len: usize,
) -> Result<u16, NeededRange> {
    let want = u32::from(viewport_height);
    let mut rows: u32 = 0;
    let mut i = anchor.0;
    while rows < want && i < len {
        let height = u32::from(
            cache
                .cached(i)
                .ok_or_else(|| forward_need(i, viewport_height))?,
        );
        let visible = if i == anchor.0 {
            height.saturating_sub(u32::from(anchor.1))
        } else {
            height
        };
        rows += visible;
        i += 1;
    }
    Ok(u16::try_from(rows.min(want)).unwrap_or(viewport_height))
}

/// Shifts an anchor by `delta` rows (positive scrolls down), normalizing as it
/// walks. Overshoot past either end is left for the clamps.
fn shift(
    cache: &MeasureCache,
    anchor: Anchor,
    delta: i32,
    len: usize,
    viewport_height: u16,
) -> Result<Anchor, NeededRange> {
    if delta >= 0 {
        let mut extra = u32::from(anchor.1) + u32::try_from(delta).unwrap_or(0);
        let mut i = anchor.0;
        loop {
            let height = u32::from(
                cache
                    .cached(i)
                    .ok_or_else(|| forward_need(i, viewport_height))?,
            );
            if extra < height {
                return Ok((i, u16::try_from(extra).unwrap_or(u16::MAX)));
            }
            if i + 1 >= len {
                // Pinned inside the last item; the under-fill clamp repairs it.
                return Ok((i, u16::try_from(height.saturating_sub(1)).unwrap_or(0)));
            }
            extra -= height;
            i += 1;
        }
    } else {
        let mut back = delta.unsigned_abs();
        let mut i = anchor.0;
        let mut offset = u32::from(anchor.1);
        loop {
            if back <= offset {
                return Ok((i, u16::try_from(offset - back).unwrap_or(u16::MAX)));
            }
            if i == 0 {
                return Ok((0, 0));
            }
            back -= offset;
            i -= 1;
            offset = u32::from(
                cache
                    .cached(i)
                    .ok_or_else(|| backward_need(i, viewport_height))?,
            );
        }
    }
}

/// Moves `anchor` the minimum distance that makes `item` visible: above (or a
/// top-clipped anchor that fits) scrolls its top to the viewport top; below
/// scrolls its bottom to the viewport's last row; already fully visible is a
/// no-op. An item taller than the viewport keeps a bottom-anchored view — the
/// oscillation guard for widgets that re-request visibility every frame.
fn ensure_visible(
    cache: &MeasureCache,
    anchor: Anchor,
    item: usize,
    viewport_height: u16,
) -> Result<Anchor, NeededRange> {
    if item < anchor.0 {
        return Ok((item, 0));
    }
    if item == anchor.0 && anchor.1 > 0 {
        let height = cache
            .cached(item)
            .ok_or_else(|| forward_need(item, viewport_height))?;
        if height <= viewport_height {
            // Fits: show it whole from its top.
            return Ok((item, 0));
        }
        // Taller than the viewport: bring its bottom to the last row, or keep
        // the current view when the bottom is already on screen (no-op).
        let bottom_anchor = height - viewport_height;
        return Ok((item, anchor.1.max(bottom_anchor)));
    }
    // Walk from the anchor to the item's screen position; give up early (it is
    // certainly below) if the walk leaves the window or runs too long.
    let mut top = -i64::from(anchor.1);
    let mut i = anchor.0;
    while i < item {
        if top > i64::from(viewport_height)
            || i - anchor.0 > usize::from(viewport_height) + WALK_SLACK
        {
            return walk_back(cache, item, viewport_height);
        }
        let height = cache
            .cached(i)
            .ok_or_else(|| forward_need(i, viewport_height))?;
        top += i64::from(height);
        i += 1;
    }
    let height = cache
        .cached(item)
        .ok_or_else(|| forward_need(item, viewport_height))?;
    if top >= 0 && top + i64::from(height) <= i64::from(viewport_height) {
        return Ok(anchor); // already fully visible
    }
    walk_back(cache, item, viewport_height)
}

/// Applies the queued operations to `anchor` and clamps it in-bounds, given
/// the cached heights. `Ok` carries the settled anchor plus whether the whole
/// content fits the viewport (the scrollbar verdict); `Err` names an
/// unmeasured range the caller must fill before retrying.
fn settle(
    cache: &MeasureCache,
    len: usize,
    viewport_height: u16,
    anchor: Anchor,
    pending_scroll: i32,
    pending_end: bool,
    pending_reveal: Option<usize>,
) -> Result<(Anchor, bool), NeededRange> {
    if len == 0 || viewport_height == 0 {
        return Ok(((0, 0), true));
    }
    let mut anchor = if anchor.0 >= len {
        (len - 1, 0)
    } else {
        anchor
    };
    if pending_end {
        anchor = walk_back(cache, len - 1, viewport_height)?;
    }
    if let Some(item) = pending_reveal {
        anchor = ensure_visible(cache, anchor, item.min(len - 1), viewport_height)?;
    }
    if pending_scroll != 0 {
        anchor = shift(cache, anchor, pending_scroll, len, viewport_height)?;
    }
    // Normalize, then repair under-fill: fewer visible rows than the viewport
    // means the anchor is past the maximum (or everything fits).
    for _ in 0..3 {
        anchor = normalize(cache, anchor, len, viewport_height)?;
        let filled = fill_rows(cache, anchor, viewport_height, len)?;
        if filled >= viewport_height {
            return Ok((anchor, false));
        }
        if anchor == (0, 0) {
            return Ok((anchor, true));
        }
        let max_anchor = walk_back(cache, len - 1, viewport_height)?;
        anchor = anchor.min(max_anchor);
    }
    Ok((anchor, false))
}

/// Merges overlapping/touching ranges in place (sorted, half-open).
fn merge_ranges(ranges: &mut Vec<NeededRange>) {
    ranges.retain(|range| range.1 > range.0);
    ranges.sort_unstable();
    let mut merged: Vec<NeededRange> = Vec::with_capacity(ranges.len());
    for &(start, end) in ranges.iter() {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    *ranges = merged;
}

/// One item emitted by the declare pass: which item, where on screen (viewport
/// rows), and how many rows of it are visible. Feeds the reveal mapping and
/// the scrollbar's emitted count.
struct Emitted {
    item: usize,
    screen_top: u16,
    visible: u16,
}

/// Which of the two passes a [`ScrollScope`] is running.
enum ScopeMode {
    /// Measure candidate items into the cache; declare and paint nothing.
    /// `fresh` items re-measure unconditionally (the visible window must track
    /// height changes frame to frame); `ranges` items measure only on a cache
    /// miss (distant walk targets).
    Measure {
        fresh: NeededRange,
        ranges: Vec<NeededRange>,
        cursor: usize,
    },
    /// Emit the visible window at the settled anchor; heights come from the
    /// cache (the measure pass filled them this frame).
    Declare {
        anchor_item: usize,
        anchor_offset: u16,
        filled: u16,
        emitted: Vec<Emitted>,
        overscan_below_done: bool,
    },
}

/// The scope a [`Frame::scroll`](crate::frame::Frame::scroll) closure declares
/// items into.
///
/// Items are declared with [`item`](Self::item) in top-to-bottom order. The
/// closure runs more than once per frame — a measure pass (or several, while
/// the anchor settles) that only records heights, then a declare pass that
/// paints the visible window — so it must be a `Fn` and deterministic within
/// a frame.
pub struct ScrollScope<'a, 'f> {
    /// The child frame items declare into (scope id is its parent).
    frame: &'a mut Frame<'f>,
    /// The viewport in absolute buffer coordinates (declare pass only).
    viewport: Rect,
    /// The width items are measured and painted at (viewport minus the
    /// scrollbar column when content overflows).
    inner_width: u16,
    /// The scope's height cache (shared with the retained [`ScrollState`]).
    cache: Rc<RefCell<MeasureCache>>,
    /// The index of the next declared item (== items seen so far).
    index: usize,
    /// Which pass this scope run is.
    mode: ScopeMode,
}

impl ScrollScope<'_, '_> {
    /// Declares one item into the scroll.
    ///
    /// During a measure pass this at most measures the item
    /// ([`Frame::measure`], so the widget is never marked declared); during
    /// the declare pass an item intersecting the viewport is declared with
    /// [`Frame::widget`](crate::frame::Frame::widget) semantics at its clipped
    /// visible area — a top-clipped item carries a hidden-top mask so its
    /// *bottom* rows show. Everything else is skipped: virtualization by
    /// construction.
    pub fn item<W: Widget>(&mut self, key: Key, widget: &W) {
        let index = self.index;
        self.index += 1;
        let width = self.inner_width;
        match &mut self.mode {
            ScopeMode::Measure {
                fresh,
                ranges,
                cursor,
            } => {
                if index >= fresh.0 && index < fresh.1 {
                    let height = self.frame.measure(key, width, widget);
                    self.cache.borrow_mut().set(index, height);
                    return;
                }
                while *cursor < ranges.len() && ranges[*cursor].1 <= index {
                    *cursor += 1;
                }
                if *cursor < ranges.len() && ranges[*cursor].0 <= index {
                    let frame = &*self.frame;
                    self.cache
                        .borrow_mut()
                        .get(index, || frame.measure(key, width, widget));
                }
            }
            ScopeMode::Declare {
                anchor_item,
                anchor_offset,
                filled,
                emitted,
                overscan_below_done,
            } => {
                let viewport_height = self.viewport.size.height;
                if index + 1 == *anchor_item
                    || (*filled >= viewport_height && !*overscan_below_done)
                {
                    // Overscan: pre-measure one neighbor beyond each window
                    // edge so the next single-row wheel step costs no measure.
                    if *filled >= viewport_height {
                        *overscan_below_done = true;
                    }
                    let frame = &*self.frame;
                    self.cache
                        .borrow_mut()
                        .get(index, || frame.measure(key, width, widget));
                    return;
                }
                if index < *anchor_item || *filled >= viewport_height || width == 0 {
                    return;
                }
                let frame = &*self.frame;
                let height = self
                    .cache
                    .borrow_mut()
                    .get(index, || frame.measure(key, width, widget));
                if height == 0 {
                    return;
                }
                let hidden = if index == *anchor_item {
                    (*anchor_offset).min(height - 1)
                } else {
                    0
                };
                let visible = (height - hidden).min(viewport_height - *filled);
                let area = Rect::new(
                    Position::new(self.viewport.origin.x, self.viewport.origin.y + *filled),
                    Size::new(width, visible),
                );
                self.frame.widget_masked(key, area, hidden, widget);
                emitted.push(Emitted {
                    item: index,
                    screen_top: *filled,
                    visible,
                });
                *filled += visible;
            }
        }
    }

    /// Declares a **nested scroll** of a fixed `height` rows as an item.
    ///
    /// A scroll is a scope, not a widget, so it cannot go through
    /// [`item`](Self::item); `nest` reserves `height` content rows and — when
    /// visible — declares an inner [`Frame::scroll`](crate::frame::Frame::scroll)
    /// into the clipped region. The inner scroll is a distinct focusable scope,
    /// so existing routing gives it the event first (inner wins) and bubbles
    /// unconsumed events to the outer.
    ///
    /// A top-clipped nested scroll shrinks its viewport rather than masking:
    /// its content is itself scroll-positioned, so the hidden-top mask (an
    /// *item* mechanism) does not apply.
    pub fn nest(&mut self, key: Key, height: u16, scope: impl Fn(&mut ScrollScope<'_, '_>)) {
        let index = self.index;
        self.index += 1;
        match &mut self.mode {
            ScopeMode::Measure { .. } => {
                // A fixed height needs no measure closure; record it outright
                // so the anchor walks stack it correctly.
                self.cache.borrow_mut().set(index, height);
            }
            ScopeMode::Declare {
                anchor_item,
                anchor_offset,
                filled,
                emitted,
                overscan_below_done,
            } => {
                let viewport_height = self.viewport.size.height;
                self.cache.borrow_mut().set(index, height);
                if *filled >= viewport_height {
                    *overscan_below_done = true;
                    return;
                }
                if index < *anchor_item || height == 0 || self.inner_width == 0 {
                    return;
                }
                let hidden = if index == *anchor_item {
                    (*anchor_offset).min(height - 1)
                } else {
                    0
                };
                let visible = (height - hidden).min(viewport_height - *filled);
                let area = Rect::new(
                    Position::new(self.viewport.origin.x, self.viewport.origin.y + *filled),
                    Size::new(self.inner_width, visible),
                );
                self.frame.scroll(key, area, scope);
                emitted.push(Emitted {
                    item: index,
                    screen_top: *filled,
                    visible,
                });
                *filled += visible;
            }
        }
    }
}

impl<'f> Frame<'f> {
    /// Declares a **scroll container**: a scoped builder that stacks the items its
    /// closure declares in a vertical viewport, virtualizes them, and scrolls.
    ///
    /// See the [`scroll`](crate::scroll) module docs for the full semantics. In
    /// brief: `scope` declares items with
    /// [`ScrollScope::item`](crate::scroll::ScrollScope::item); the scope keeps an
    /// **anchor** — `(first visible item, rows of it hidden)` — by identity, measures
    /// only the items near the window (heights for distant jumps are cached, see
    /// [`ScrollState::invalidate`]), and paints only the visible window. It declares
    /// itself focusable with a Up/Down/PageUp/PageDown/Home/End + wheel handler,
    /// paints a scrollbar when content overflows, and consumes children's
    /// scroll-into-view requests to adjust the anchor on the next frame.
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
        let viewport = area.intersection(bounds);
        if viewport.is_empty() {
            // Nothing to scroll into: still persist state and register the
            // focusable scope so focus and routing are stable, but paint nothing.
            let state: ScrollState = self.container_state(scope_id);
            self.put_container_state(scope_id, state);
            self.register_container::<ScrollView>(scope_id, viewport);
            return;
        }

        let mut state: ScrollState = self.container_state(scope_id);
        let viewport_height = viewport.size.height;
        let full_width = viewport.size.width;

        // Take the operations queued since the last render; they apply to the
        // anchor as it stood when they were queued.
        let pending_scroll = std::mem::take(&mut state.pending_scroll);
        let pending_end = std::mem::take(&mut state.pending_end);
        let pending_reveal = state.pending_reveal.take();

        let original_anchor = (state.anchor_item, state.anchor_offset);
        let mut anchor = original_anchor;
        let mut scrollbar = state.scrollbar;
        let mut len = state.item_count;

        // Settle the anchor, flipping the scrollbar-column verdict at most
        // once (the width guess comes from last frame — sticky, so a
        // boundary-wrapping content cannot oscillate within a frame).
        for _flip in 0..2 {
            let width = full_width.saturating_sub(u16::from(scrollbar && full_width > 1));
            state.cache.borrow_mut().set_width(width);

            // The window range re-measures fresh every frame (a visible item
            // that changed height must relayout now); pending jumps add
            // cache-miss-only ranges for the walks they will take.
            let back = usize::try_from(pending_scroll.min(0).unsigned_abs()).unwrap_or(usize::MAX);
            let ahead = usize::try_from(pending_scroll.max(0)).unwrap_or(usize::MAX);
            let fresh = (
                original_anchor.0.saturating_sub(1 + back),
                original_anchor
                    .0
                    .saturating_add(usize::from(viewport_height) + 2)
                    .saturating_add(ahead),
            );
            let reach = usize::from(viewport_height) + WALK_SLACK + 1;
            let mut ranges: Vec<NeededRange> = Vec::new();
            if pending_end {
                ranges.push((len.saturating_sub(reach), len.saturating_add(1)));
            }
            if let Some(item) = pending_reveal {
                ranges.push((item.saturating_sub(reach), item.saturating_add(reach)));
            }

            let mut settled = (original_anchor.min((len.saturating_sub(1), 0)), false);
            for _attempt in 0..8 {
                merge_ranges(&mut ranges);
                len =
                    self.scroll_measure_pass(scope_id, width, &state.cache, fresh, &ranges, &scope);
                state.cache.borrow_mut().resize(len);
                let result = {
                    let cache = state.cache.borrow();
                    settle(
                        &cache,
                        len,
                        viewport_height,
                        original_anchor,
                        pending_scroll,
                        pending_end,
                        pending_reveal,
                    )
                };
                match result {
                    Ok(outcome) => {
                        settled = outcome;
                        break;
                    }
                    Err(needed) => ranges.push(needed),
                }
            }
            anchor = settled.0;
            let overflow = !settled.1;
            if overflow == scrollbar {
                break;
            }
            scrollbar = overflow;
        }

        let inner_width = full_width.saturating_sub(u16::from(scrollbar && full_width > 1));

        // Declare the visible window at the settled anchor.
        let visibility_before = self.visibility_len();
        let emitted = self.scroll_declare_pass(
            scope_id,
            viewport,
            inner_width,
            &state.cache,
            anchor,
            &scope,
        );

        // Consume any scroll-into-view request a child recorded during the
        // declare pass: map its top row back to the emitted item, and reveal
        // that item next frame. The last request wins.
        state.pending_reveal = self
            .visibility_since(visibility_before)
            .into_iter()
            .rev()
            .find_map(|request| {
                let screen_row = request.area.origin.y.checked_sub(viewport.origin.y)?;
                emitted
                    .iter()
                    .find(|entry| {
                        screen_row >= entry.screen_top
                            && screen_row < entry.screen_top + entry.visible
                    })
                    .map(|entry| entry.item)
            });

        // Persist the updated state and register the focusable scope + handler.
        state.anchor_item = anchor.0;
        state.anchor_offset = anchor.1;
        state.viewport_height = viewport_height;
        state.item_count = len;
        state.scrollbar = scrollbar;
        let emitted_count = emitted.len();
        self.put_container_state(scope_id, state);
        self.register_container::<ScrollView>(scope_id, viewport);

        // Paint the scrollbar last (over the viewport's right column) when
        // content overflows, so it sits above the items.
        if scrollbar {
            self.paint_scrollbar(viewport, anchor.0, len, emitted_count);
        }
    }

    /// Runs the scope closure in measure mode: items in `fresh` re-measure
    /// unconditionally, items in `ranges` fill cache misses, and nothing is
    /// declared. Returns the number of items the closure declared (the source
    /// length this frame).
    fn scroll_measure_pass(
        &mut self,
        scope_id: WidgetId,
        width: u16,
        cache: &Rc<RefCell<MeasureCache>>,
        fresh: NeededRange,
        ranges: &[NeededRange],
        scope: &impl Fn(&mut ScrollScope<'_, '_>),
    ) -> usize {
        self.with_child_scope(scope_id, 0, |child| {
            let mut scroll_scope = ScrollScope {
                frame: child,
                viewport: Rect::default(),
                inner_width: width,
                cache: Rc::clone(cache),
                index: 0,
                mode: ScopeMode::Measure {
                    fresh,
                    ranges: ranges.to_vec(),
                    cursor: 0,
                },
            };
            scope(&mut scroll_scope);
            scroll_scope.index
        })
    }

    /// Runs the scope closure in declare mode: emits the visible window at the
    /// settled `anchor` (heights come from the cache, filled by the measure
    /// pass) and returns the emitted items for the reveal mapping and the
    /// scrollbar.
    fn scroll_declare_pass(
        &mut self,
        scope_id: WidgetId,
        viewport: Rect,
        inner_width: u16,
        cache: &Rc<RefCell<MeasureCache>>,
        anchor: Anchor,
        scope: &impl Fn(&mut ScrollScope<'_, '_>),
    ) -> Vec<Emitted> {
        self.with_child_scope(scope_id, 0, |child| {
            let mut scroll_scope = ScrollScope {
                frame: child,
                viewport,
                inner_width,
                cache: Rc::clone(cache),
                index: 0,
                mode: ScopeMode::Declare {
                    anchor_item: anchor.0,
                    anchor_offset: anchor.1,
                    filled: 0,
                    emitted: Vec::new(),
                    overscan_below_done: false,
                },
            };
            scope(&mut scroll_scope);
            match scroll_scope.mode {
                ScopeMode::Declare { emitted, .. } => emitted,
                ScopeMode::Measure { .. } => unreachable!("declare pass keeps its mode"),
            }
        })
    }

    /// Paints the scrollbar in the viewport's right column: a
    /// [`Role::Border`](crate::theme::Role::Border) track with a
    /// [`Role::Muted`](crate::theme::Role::Muted) thumb sized and positioned by
    /// the **item-fraction approximation** — thumb length `emitted / len` of
    /// the track, thumb top `anchor_item / (len - emitted)` of the free track.
    /// Exact for uniform heights, proportional otherwise; no total content
    /// height is ever computed (that would be O(items)).
    fn paint_scrollbar(&mut self, viewport: Rect, anchor_item: usize, len: usize, emitted: usize) {
        let viewport_height = viewport.size.height;
        if viewport_height == 0 || len == 0 {
            return;
        }
        let x = viewport.right().saturating_sub(1);
        let track_style = self.theme_ref().style(Role::Border);
        let thumb_style = self.theme_ref().style(Role::Muted);

        let track = u64::from(viewport_height);
        let thumb_len = ((track * emitted as u64) / len as u64).max(1).min(track);
        let free = track - thumb_len;
        let denom = len.saturating_sub(emitted).max(1) as u64;
        let thumb_top = ((free * anchor_item as u64) / denom).min(free);
        let (thumb_top, thumb_len) = (thumb_top as u16, thumb_len as u16);

        for row in 0..viewport_height {
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
    use std::cell::Cell;

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

    /// A multi-row item that labels every logical row (`{label}{row}`), so a
    /// test can assert exactly *which slice* of a partially-visible item is on
    /// screen — the render-space wrong-slice pin.
    struct Lines {
        label: &'static str,
        height: u16,
    }

    impl Widget for Lines {
        type State = ();
        fn render(&self, _state: &mut (), ctx: &mut RenderContext<'_>) {
            for row in 0..ctx.size().height {
                ctx.set_string(
                    Position::new(0, row),
                    &format!("{}{row}", self.label),
                    Style::new(),
                );
            }
        }
        fn desired_height(&self, _state: &(), _width: u16) -> u16 {
            self.height
        }
    }

    /// A row whose `desired_height` increments a shared counter — the
    /// structural probe for the O(window) measure property and for cache
    /// invalidation.
    struct CountingRow {
        height: u16,
        measures: Rc<Cell<usize>>,
    }

    impl Widget for CountingRow {
        type State = ();
        fn render(&self, _state: &mut (), ctx: &mut RenderContext<'_>) {
            ctx.set_string(Position::ORIGIN, "x", Style::new());
        }
        fn desired_height(&self, _state: &(), _width: u16) -> u16 {
            self.measures.set(self.measures.get() + 1);
            self.height
        }
    }

    fn scope_id() -> WidgetId {
        WidgetId::ROOT.child(key("list"))
    }

    /// Renders one scroll frame of `size` with the given item declarations and
    /// returns the buffer.
    fn render_frame(
        store: &mut StateStore,
        size: Size,
        body: impl Fn(&mut ScrollScope<'_, '_>),
    ) -> Buffer {
        let mut buffer = Buffer::new(size);
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, store);
        frame.scroll(key("list"), frame.area(), body);
        let _ = frame.finish();
        store.end_frame();
        buffer
    }

    /// Reads a row of *content* cells — everything left of the scrollbar
    /// column — so text assertions are unaffected by the scrollbar glyphs.
    fn row(buffer: &Buffer, y: u16) -> String {
        let mut line = String::new();
        for x in 0..buffer.size().width.saturating_sub(1) {
            line.push_str(&buffer.get(Position::new(x, y)).unwrap().symbol);
        }
        line.trim_end().to_string()
    }

    fn anchor_of(store: &StateStore) -> (usize, u16) {
        store.peek::<ScrollState>(scope_id()).unwrap().anchor()
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

    /// Runs `event` through the retained handler state, persisting the result
    /// so the next render applies the queued movement.
    fn send(store: &mut StateStore, event: InputEvent) -> Handled {
        let mut state = store.peek::<ScrollState>(scope_id()).unwrap().clone();
        let handled = dispatch(&mut state, event);
        store.begin_frame();
        *store.get_or_default::<ScrollState>(scope_id()) = state;
        store.end_frame();
        handled
    }

    fn wheel(lines: i8) -> InputEvent {
        use crate::input::{MouseButton, MouseEvent};
        InputEvent::Mouse(MouseEvent::new(
            MouseKind::Scroll(lines),
            MouseButton::None,
            Position::ORIGIN,
        ))
    }

    fn uniform_rows(count: usize) -> impl Fn(&mut ScrollScope<'_, '_>) {
        move |scroll| {
            for i in 0..count {
                scroll.item(
                    key("row").index(i),
                    &Probe {
                        height: 1,
                        label: "x",
                    },
                );
            }
        }
    }

    #[test]
    fn stacks_items_at_measured_heights() {
        let mut store = StateStore::new();
        let buffer = render_frame(&mut store, Size::new(10, 6), |scroll| {
            for (label, height) in [("a", 2), ("b", 2), ("c", 2)] {
                scroll.item(key(label), &Probe { height, label });
            }
        });
        // a at row 0, b at row 2, c at row 4.
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, "a");
        assert_eq!(buffer.get(Position::new(0, 2)).unwrap().symbol, "b");
        assert_eq!(buffer.get(Position::new(0, 4)).unwrap().symbol, "c");
    }

    #[test]
    fn virtualizes_a_thousand_items_to_the_visible_few() {
        let mut store = StateStore::new();
        render_frame(&mut store, Size::new(10, 5), uniform_rows(1000));
        // Only items intersecting the 5-row viewport were *declared*, so only
        // those hold retained render state.
        let painted: usize = (0..1000)
            .filter(|i| {
                store
                    .peek::<RenderCount>(scope_id().child(key("row").index(*i)))
                    .is_some_and(|count| count.0 > 0)
            })
            .count();
        assert_eq!(painted, 5, "only the 5 viewport rows painted");
    }

    #[test]
    fn ten_thousand_items_declare_at_most_the_window() {
        // The virtualization property at 10k, asserted structurally (the plan's
        // core test 1): declared items per frame never exceed the viewport rows
        // plus the two overscan neighbors.
        let mut store = StateStore::new();
        render_frame(&mut store, Size::new(10, 5), uniform_rows(10_000));
        send(&mut store, wheel(3));
        render_frame(&mut store, Size::new(10, 5), uniform_rows(10_000));
        let declared: usize = (0..10_000)
            .filter(|i| {
                store
                    .peek::<RenderCount>(scope_id().child(key("row").index(*i)))
                    .is_some()
            })
            .count();
        // Two frames' windows (rows 0..5 and 3..8) plus overscan at most.
        assert!(declared <= 5 + 5 + 2, "declared {declared} items");
        assert!(declared >= 5);
    }

    #[test]
    fn one_hundred_thousand_items_measure_o_window() {
        // The measure half of the property: desired_height calls per frame are
        // O(window) — bounded by a constant near the viewport size — even when
        // the source is enormous. (The bench mirrors this at 1M.)
        const COUNT: usize = 100_000;
        let measures = Rc::new(Cell::new(0));
        let body = |measures: Rc<Cell<usize>>| {
            move |scroll: &mut ScrollScope<'_, '_>| {
                for i in 0..COUNT {
                    scroll.item(
                        key("row").index(i),
                        &CountingRow {
                            height: 1,
                            measures: Rc::clone(&measures),
                        },
                    );
                }
            }
        };
        let mut store = StateStore::new();
        // Frame 1 settles the scrollbar verdict (two measure passes).
        render_frame(&mut store, Size::new(10, 24), body(Rc::clone(&measures)));
        assert!(
            measures.get() <= 64,
            "first frame measured {}",
            measures.get()
        );
        // A steady scrolled frame re-measures only the fresh window.
        send(&mut store, wheel(3));
        measures.set(0);
        render_frame(&mut store, Size::new(10, 24), body(Rc::clone(&measures)));
        assert!(
            measures.get() <= 64,
            "steady frame measured {}",
            measures.get()
        );
        assert!(measures.get() > 0);
        assert_eq!(anchor_of(&store), (3, 0));
    }

    #[test]
    fn variable_heights_fill_the_window_with_the_right_slices() {
        // Heights cycle 1,2,3 (the plan's core test 2). After scrolling down 2
        // rows the anchor is (1, 1): the viewport must show item 1's *second*
        // row at its top — the wrong-slice bug would show its first.
        let body = |scroll: &mut ScrollScope<'_, '_>| {
            for (i, label) in ["a", "b", "c", "d", "e", "f"].iter().enumerate() {
                scroll.item(
                    key("item").index(i),
                    &Lines {
                        label,
                        height: (i % 3) as u16 + 1,
                    },
                );
            }
        };
        let mut store = StateStore::new();
        render_frame(&mut store, Size::new(10, 6), body);
        assert_eq!(anchor_of(&store), (0, 0));
        send(&mut store, wheel(2));
        let buffer = render_frame(&mut store, Size::new(10, 6), body);
        assert_eq!(anchor_of(&store), (1, 1));
        // Content rows: a:[0,1) b:[1,3) c:[3,6) d:[6,7) e:[7,9) f:[9,12).
        // Window rows 2..8 → b's row 1, all of c, d, then e's first row.
        assert_eq!(row(&buffer, 0), "b1", "top partial shows its BOTTOM row");
        assert_eq!(row(&buffer, 1), "c0");
        assert_eq!(row(&buffer, 2), "c1");
        assert_eq!(row(&buffer, 3), "c2");
        assert_eq!(row(&buffer, 4), "d0");
        assert_eq!(row(&buffer, 5), "e0");
    }

    #[test]
    fn top_partial_item_paints_its_bottom_rows_exactly() {
        // The minimal render-space pin: one 3-row item scrolled 2 rows up shows
        // exactly its last row on the first viewport row.
        let body = |scroll: &mut ScrollScope<'_, '_>| {
            scroll.item(
                key("tall"),
                &Lines {
                    label: "t",
                    height: 3,
                },
            );
            for i in 0..8 {
                scroll.item(
                    key("fill").index(i),
                    &Lines {
                        label: "f",
                        height: 1,
                    },
                );
            }
        };
        let mut store = StateStore::new();
        render_frame(&mut store, Size::new(10, 4), body);
        send(&mut store, wheel(2));
        let buffer = render_frame(&mut store, Size::new(10, 4), body);
        assert_eq!(anchor_of(&store), (0, 2));
        assert_eq!(
            row(&buffer, 0),
            "t2",
            "the sliced item shows row 2, not row 0"
        );
        assert_eq!(row(&buffer, 1), "f0");
    }

    #[test]
    fn scroll_by_normalizes_the_anchor_across_multi_row_items() {
        // Heights 3,1,2,3,1,2: +4 rows from the top crosses two items into the
        // third; -3 rows walks back across a whole item.
        let body = |scroll: &mut ScrollScope<'_, '_>| {
            for (i, height) in [3u16, 1, 2, 3, 1, 2, 3, 1, 2].iter().enumerate() {
                scroll.item(
                    key("item").index(i),
                    &Probe {
                        height: *height,
                        label: "x",
                    },
                );
            }
        };
        let mut store = StateStore::new();
        render_frame(&mut store, Size::new(10, 5), body);
        send(&mut store, wheel(4));
        render_frame(&mut store, Size::new(10, 5), body);
        assert_eq!(
            anchor_of(&store),
            (2, 0),
            "3+1 rows crossed, none into item 2"
        );
        send(&mut store, wheel(1));
        render_frame(&mut store, Size::new(10, 5), body);
        assert_eq!(anchor_of(&store), (2, 1));
        // Content row 5 minus 3 is row 2 — two rows into the first item.
        send(&mut store, wheel(-3));
        render_frame(&mut store, Size::new(10, 5), body);
        assert_eq!(anchor_of(&store), (0, 2), "backward walk re-normalizes");
    }

    #[test]
    fn scroll_past_the_end_clamps_to_the_back_filled_max_anchor() {
        // 10 items × 2 rows in a 5-row viewport: the maximum anchor back-fills
        // from the last item — (7, 1) puts item 9's bottom row on the last row.
        let body = |scroll: &mut ScrollScope<'_, '_>| {
            for i in 0..10 {
                scroll.item(
                    key("item").index(i),
                    &Lines {
                        label: ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"][i],
                        height: 2,
                    },
                );
            }
        };
        let mut store = StateStore::new();
        render_frame(&mut store, Size::new(10, 5), body);
        send(&mut store, wheel(100));
        let buffer = render_frame(&mut store, Size::new(10, 5), body);
        assert_eq!(anchor_of(&store), (7, 1));
        // The last item is fully visible at the bottom.
        assert_eq!(row(&buffer, 3), "j0");
        assert_eq!(row(&buffer, 4), "j1");
        // End reaches the same clamp; Home returns to the top.
        send(&mut store, InputEvent::key(InputKey::Home));
        render_frame(&mut store, Size::new(10, 5), body);
        assert_eq!(anchor_of(&store), (0, 0));
        send(&mut store, InputEvent::key(InputKey::End));
        render_frame(&mut store, Size::new(10, 5), body);
        assert_eq!(anchor_of(&store), (7, 1));
    }

    #[test]
    fn keys_scroll_and_clamp() {
        let mut store = StateStore::new();
        let body = uniform_rows(20);
        render_frame(&mut store, Size::new(10, 5), &body);
        assert_eq!(
            send(&mut store, InputEvent::key(InputKey::Down)),
            Handled::Yes
        );
        render_frame(&mut store, Size::new(10, 5), &body);
        assert_eq!(anchor_of(&store), (1, 0));
        // A page is the viewport height less one row of overlap.
        send(&mut store, InputEvent::key(InputKey::PageDown));
        render_frame(&mut store, Size::new(10, 5), &body);
        assert_eq!(anchor_of(&store), (5, 0));
        send(&mut store, InputEvent::key(InputKey::End));
        render_frame(&mut store, Size::new(10, 5), &body);
        assert_eq!(anchor_of(&store), (15, 0));
        // Down at the end is clamped.
        send(&mut store, InputEvent::key(InputKey::Down));
        render_frame(&mut store, Size::new(10, 5), &body);
        assert_eq!(anchor_of(&store), (15, 0));
        send(&mut store, InputEvent::key(InputKey::PageUp));
        render_frame(&mut store, Size::new(10, 5), &body);
        assert_eq!(anchor_of(&store), (11, 0));
        send(&mut store, InputEvent::key(InputKey::Home));
        render_frame(&mut store, Size::new(10, 5), &body);
        assert_eq!(anchor_of(&store), (0, 0));
        // Up at the top is clamped.
        send(&mut store, InputEvent::key(InputKey::Up));
        render_frame(&mut store, Size::new(10, 5), &body);
        assert_eq!(anchor_of(&store), (0, 0));
    }

    #[test]
    fn wheel_scrolls_and_is_always_handled() {
        let mut store = StateStore::new();
        let body = uniform_rows(20);
        render_frame(&mut store, Size::new(10, 5), &body);
        assert_eq!(send(&mut store, wheel(2)), Handled::Yes);
        render_frame(&mut store, Size::new(10, 5), &body);
        assert_eq!(anchor_of(&store), (2, 0));
        // A big wheel-up clamps to the top and is still handled — the wheel
        // over a scroll region is the scroll's even at an end stop.
        assert_eq!(send(&mut store, wheel(-10)), Handled::Yes);
        render_frame(&mut store, Size::new(10, 5), &body);
        assert_eq!(anchor_of(&store), (0, 0));
    }

    #[test]
    fn scrolling_shows_the_next_window_of_items() {
        // 10 height-1 rows, 3-row viewport. Scroll down 2 and the visible
        // window is items 2,3,4.
        let body = |scroll: &mut ScrollScope<'_, '_>| {
            for i in 0..10 {
                let label: &'static str = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"][i];
                scroll.item(key("row").index(i), &Probe { height: 1, label });
            }
        };
        let mut store = StateStore::new();
        render_frame(&mut store, Size::new(10, 3), body);
        send(&mut store, InputEvent::key(InputKey::Down));
        send(&mut store, InputEvent::key(InputKey::Down));
        let buffer = render_frame(&mut store, Size::new(10, 3), body);
        assert_eq!(buffer.get(Position::new(0, 0)).unwrap().symbol, "2");
        assert_eq!(buffer.get(Position::new(0, 1)).unwrap().symbol, "3");
        assert_eq!(buffer.get(Position::new(0, 2)).unwrap().symbol, "4");
    }

    /// A tall widget that, while it renders (even clipped), asks for its whole
    /// `height` to be visible — the scroll-into-view probe.
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
        // 3 height-1 rows then a height-3 requester into a 4-row viewport. At
        // the top the requester's first row is the last visible row; it asks
        // for its whole height, and the next frame anchors so its bottom sits
        // on the viewport's last row: anchor (2, 0).
        let body = |scroll: &mut ScrollScope<'_, '_>| {
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
        };
        let mut store = StateStore::new();
        render_frame(&mut store, Size::new(10, 4), body);
        assert_eq!(anchor_of(&store), (0, 0));
        render_frame(&mut store, Size::new(10, 4), body);
        assert_eq!(anchor_of(&store), (2, 0));
        // The reveal is stable: a further frame does not move the anchor.
        render_frame(&mut store, Size::new(10, 4), body);
        assert_eq!(anchor_of(&store), (2, 0));
    }

    #[test]
    fn request_visibility_scrolls_up_to_a_top_clipped_item() {
        // The other direction (the plan's core test 5): a top-clipped requester
        // that *fits* the viewport scrolls up to show itself whole.
        let body = |scroll: &mut ScrollScope<'_, '_>| {
            scroll.item(key("tall"), &Reveal { height: 3 });
            for i in 0..8 {
                scroll.item(
                    key("row").index(i),
                    &Probe {
                        height: 1,
                        label: "x",
                    },
                );
            }
        };
        let mut store = StateStore::new();
        render_frame(&mut store, Size::new(10, 4), body);
        send(&mut store, wheel(2));
        // The wheel wins this frame (the reveal maps to the clipped item)…
        render_frame(&mut store, Size::new(10, 4), body);
        assert_eq!(anchor_of(&store), (0, 2));
        // …and the clipped requester pulls the anchor back to its top.
        render_frame(&mut store, Size::new(10, 4), body);
        assert_eq!(anchor_of(&store), (0, 0));
    }

    #[test]
    fn zero_height_items_are_skipped_without_stalling() {
        let body = |scroll: &mut ScrollScope<'_, '_>| {
            for i in 0..12 {
                let height = if i % 2 == 0 { 0 } else { 1 };
                let label: &'static str = if i % 2 == 0 { "z" } else { "r" };
                scroll.item(key("row").index(i), &Probe { height, label });
            }
        };
        let mut store = StateStore::new();
        let buffer = render_frame(&mut store, Size::new(10, 3), body);
        assert_eq!(row(&buffer, 0), "r");
        send(&mut store, wheel(2));
        render_frame(&mut store, Size::new(10, 3), body);
        // Two rows down: past items 0..=4 (heights 0,1,0,1,0), into item 5.
        assert_eq!(anchor_of(&store), (5, 0));
    }

    #[test]
    fn empty_scroll_is_stable_and_safe() {
        let mut store = StateStore::new();
        let body = |_scroll: &mut ScrollScope<'_, '_>| {};
        let buffer = render_frame(&mut store, Size::new(10, 3), body);
        assert_eq!(anchor_of(&store), (0, 0));
        assert_eq!(row(&buffer, 0), "");
        assert_eq!(
            send(&mut store, InputEvent::key(InputKey::End)),
            Handled::Yes
        );
        render_frame(&mut store, Size::new(10, 3), body);
        assert_eq!(anchor_of(&store), (0, 0));
    }

    #[test]
    fn shrinking_content_clamps_the_anchor() {
        let mut store = StateStore::new();
        render_frame(&mut store, Size::new(10, 4), uniform_rows(100));
        send(&mut store, InputEvent::key(InputKey::End));
        render_frame(&mut store, Size::new(10, 4), uniform_rows(100));
        assert_eq!(anchor_of(&store), (96, 0));
        // The source shrinks to 10 items: the anchor clamps to the new max.
        render_frame(&mut store, Size::new(10, 4), uniform_rows(10));
        assert_eq!(anchor_of(&store), (6, 0));
        // And to the top when everything fits.
        render_frame(&mut store, Size::new(10, 4), uniform_rows(3));
        assert_eq!(anchor_of(&store), (0, 0));
    }

    #[test]
    fn anchor_clamps_to_content_bounds() {
        let mut store = StateStore::new();
        // Seed a wildly-out-of-bounds anchor in a PRIOR frame, then render and
        // confirm it clamps to the back-filled max anchor (6, 0).
        store.begin_frame();
        store.get_or_default::<ScrollState>(scope_id()).anchor_item = 999;
        store.end_frame();
        render_frame(&mut store, Size::new(10, 4), uniform_rows(10));
        assert_eq!(anchor_of(&store), (6, 0));
    }

    #[test]
    fn width_change_invalidates_the_cache() {
        // Distant heights cache across frames (an End jump measured the tail;
        // repeating it is nearly free) until the width changes, which clears
        // every height (the plan's core test 6).
        const COUNT: usize = 200;
        let measures = Rc::new(Cell::new(0usize));
        let body = |measures: Rc<Cell<usize>>| {
            move |scroll: &mut ScrollScope<'_, '_>| {
                for i in 0..COUNT {
                    scroll.item(
                        key("row").index(i),
                        &CountingRow {
                            height: 1,
                            measures: Rc::clone(&measures),
                        },
                    );
                }
            }
        };
        let mut store = StateStore::new();
        let size = Size::new(10, 5);
        render_frame(&mut store, size, body(Rc::clone(&measures)));

        // First End jump: the tail range is unmeasured — a cold walk.
        send(&mut store, InputEvent::key(InputKey::End));
        measures.set(0);
        render_frame(&mut store, size, body(Rc::clone(&measures)));
        let cold_end = measures.get();

        // Home, then End again at the same width: the tail is cached.
        send(&mut store, InputEvent::key(InputKey::Home));
        render_frame(&mut store, size, body(Rc::clone(&measures)));
        send(&mut store, InputEvent::key(InputKey::End));
        measures.set(0);
        render_frame(&mut store, size, body(Rc::clone(&measures)));
        let warm_end = measures.get();
        assert!(
            warm_end < cold_end,
            "cached tail: warm {warm_end} < cold {cold_end}"
        );

        // A width change clears the cache: the next End walk is cold again.
        send(&mut store, InputEvent::key(InputKey::Home));
        render_frame(&mut store, Size::new(14, 5), body(Rc::clone(&measures)));
        send(&mut store, InputEvent::key(InputKey::End));
        measures.set(0);
        render_frame(&mut store, Size::new(14, 5), body(Rc::clone(&measures)));
        let after_resize = measures.get();
        assert!(
            after_resize > warm_end,
            "width change re-measures: {after_resize} > {warm_end}"
        );
    }

    #[test]
    fn visible_items_re_measure_every_frame() {
        // A visible item that changes height relayouts immediately: the window
        // is fresh-measured each frame, never served stale from the cache.
        let grow = Rc::new(Cell::new(1u16));
        struct Growing {
            height: Rc<Cell<u16>>,
            label: &'static str,
        }
        impl Widget for Growing {
            type State = ();
            fn render(&self, _s: &mut (), ctx: &mut RenderContext<'_>) {
                for row in 0..ctx.size().height {
                    ctx.set_string(
                        Position::new(0, row),
                        &format!("{}{row}", self.label),
                        Style::new(),
                    );
                }
            }
            fn desired_height(&self, _s: &(), _w: u16) -> u16 {
                self.height.get()
            }
        }
        let body = |grow: Rc<Cell<u16>>| {
            move |scroll: &mut ScrollScope<'_, '_>| {
                scroll.item(
                    key("grow"),
                    &Growing {
                        height: Rc::clone(&grow),
                        label: "g",
                    },
                );
                for i in 0..10 {
                    scroll.item(
                        key("row").index(i),
                        &Lines {
                            label: "f",
                            height: 1,
                        },
                    );
                }
            }
        };
        let mut store = StateStore::new();
        let buffer = render_frame(&mut store, Size::new(10, 4), body(Rc::clone(&grow)));
        assert_eq!(row(&buffer, 0), "g0");
        assert_eq!(row(&buffer, 1), "f0");
        // The item grows (a disclosure expanded): the very next frame stacks
        // the fillers two rows further down.
        grow.set(2);
        let buffer = render_frame(&mut store, Size::new(10, 4), body(Rc::clone(&grow)));
        assert_eq!(row(&buffer, 0), "g0");
        assert_eq!(row(&buffer, 1), "g1");
        assert_eq!(row(&buffer, 2), "f0");
    }

    #[test]
    fn scrollbar_paints_a_track_and_thumb_on_overflow() {
        // 20 rows, 5-row viewport → overflow, scrollbar in the right column (x=9).
        let mut store = StateStore::new();
        let buffer = render_frame(&mut store, Size::new(10, 5), uniform_rows(20));
        let mut glyphs = Vec::new();
        for y in 0..5 {
            glyphs.push(buffer.get(Position::new(9, y)).unwrap().symbol.clone());
        }
        // At the top the thumb is at the top: the first cell is a thumb cell.
        assert_eq!(glyphs[0], "█");
        // The track glyph appears below the thumb.
        assert!(glyphs.iter().any(|g| g == "│"), "track present: {glyphs:?}");
    }

    #[test]
    fn scrollbar_thumb_reaches_the_bottom_at_the_end() {
        // The item-fraction approximation is exact for uniform heights: at the
        // End clamp the thumb's last cell is the track's last cell.
        let mut store = StateStore::new();
        render_frame(&mut store, Size::new(10, 5), uniform_rows(20));
        send(&mut store, InputEvent::key(InputKey::End));
        let buffer = render_frame(&mut store, Size::new(10, 5), uniform_rows(20));
        assert_eq!(anchor_of(&store), (15, 0));
        assert_eq!(buffer.get(Position::new(9, 4)).unwrap().symbol, "█");
        assert_eq!(buffer.get(Position::new(9, 0)).unwrap().symbol, "│");
    }

    #[test]
    fn no_scrollbar_when_content_fits() {
        // 3 rows in a 5-row viewport: no overflow, no scrollbar column painted.
        let mut store = StateStore::new();
        let buffer = render_frame(&mut store, Size::new(10, 5), uniform_rows(3));
        for y in 0..5 {
            let sym = &buffer.get(Position::new(9, y)).unwrap().symbol;
            assert!(sym == " ", "no scrollbar row {y}: {sym:?}");
        }
    }

    #[test]
    fn nested_scroll_inner_wins_and_unconsumed_bubbles_to_outer() {
        use crate::routing::{Focus, route};
        // An outer scroll containing an inner scroll; both overflow. When the
        // inner is focused, Down scrolls the inner (it consumes) and NOT the outer.
        let declare = |frame: &mut Frame<'_>| {
            let area = frame.area();
            frame.scroll(key("outer"), area, |outer| {
                outer.item(
                    key("filler0"),
                    &Probe {
                        height: 1,
                        label: "a",
                    },
                );
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
        };
        let mut store = StateStore::new();
        let mut buffer = Buffer::new(Size::new(12, 8));
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        declare(&mut frame);
        let (facts, handlers) = frame.into_parts();
        store.end_frame();

        let outer_id = WidgetId::ROOT.child(key("outer"));
        let inner_id = outer_id.child(key("inner"));

        // Focus the inner scroll and press Down: the inner queues movement,
        // the outer does not.
        let mut focus = Focus::new();
        focus.set(Some(inner_id));
        route(
            &facts,
            &handlers,
            &mut focus,
            &mut store,
            &InputEvent::key(InputKey::Down),
        );
        // Re-render so the queued movement is applied to the anchors.
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        declare(&mut frame);
        let _ = frame.finish();
        store.end_frame();
        assert_eq!(
            store.peek::<ScrollState>(inner_id).unwrap().anchor(),
            (1, 0),
            "inner scrolled"
        );
        assert_eq!(
            store.peek::<ScrollState>(outer_id).unwrap().anchor(),
            (0, 0),
            "outer did not scroll — inner won"
        );
    }

    #[test]
    fn invalidate_keeps_the_anchor_and_re_measures() {
        let measures = Rc::new(Cell::new(0usize));
        let body = |measures: Rc<Cell<usize>>| {
            move |scroll: &mut ScrollScope<'_, '_>| {
                for i in 0..50 {
                    scroll.item(
                        key("row").index(i),
                        &CountingRow {
                            height: 1,
                            measures: Rc::clone(&measures),
                        },
                    );
                }
            }
        };
        let mut store = StateStore::new();
        render_frame(&mut store, Size::new(10, 5), body(Rc::clone(&measures)));
        send(&mut store, InputEvent::key(InputKey::End));
        render_frame(&mut store, Size::new(10, 5), body(Rc::clone(&measures)));
        let anchor_before = anchor_of(&store);
        // The app edited distant content: it invalidates through the state.
        store.begin_frame();
        store.get_or_default::<ScrollState>(scope_id()).invalidate();
        store.end_frame();
        render_frame(&mut store, Size::new(10, 5), body(Rc::clone(&measures)));
        assert_eq!(
            anchor_of(&store),
            anchor_before,
            "anchor survives invalidate"
        );
    }
}
