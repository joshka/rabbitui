//! Frame facts: the queryable record a render produces alongside cells.
//!
//! Per `docs/adr/0001-programming-model.md` and
//! `docs/adr/0006-input-focus-events.md`: rendering emits *facts* — for each
//! declared widget, where it landed, who its scope parent is, and whether it
//! can take focus. Input then routes against the **previous** frame's facts
//! (one-frame-stale, immaterial at terminal event rates; ADR 0001). Facts are
//! plain data, so the same record drives focus traversal, hit-testing, and the
//! headless test harness.
//!
//! [`FrameFacts`] preserves declaration order, which doubles as paint order and
//! last-declared-wins z-order *within a layer*. Layers (slice 7, ADR 0003 delta)
//! add a coarser z-order on top: each entry carries a [`layer`](FactEntry::layer)
//! (base = 0, incremented per nested [`Frame::layer`](crate::frame::Frame::layer)
//! declaration), and hit-testing and focus traversal prefer the topmost layer.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::facts::{FactEntry, FrameFacts};
//! use rabbitui_core::geometry::{Position, Rect, Size};
//! use rabbitui_core::id::{WidgetId, key};
//!
//! let a = WidgetId::ROOT.child(key("a"));
//! let b = WidgetId::ROOT.child(key("b"));
//!
//! let mut facts = FrameFacts::new();
//! facts.push(FactEntry {
//!     id: a,
//!     parent: WidgetId::ROOT,
//!     area: Rect::new(Position::ORIGIN, Size::new(4, 1)),
//!     focusable: true,
//!     layer: 0,
//! });
//! facts.push(FactEntry {
//!     id: b,
//!     parent: WidgetId::ROOT,
//!     area: Rect::new(Position::new(0, 1), Size::new(4, 1)),
//!     focusable: false,
//!     layer: 0,
//! });
//!
//! assert_eq!(facts.get(a).unwrap().parent, WidgetId::ROOT);
//! // Only `a` is focusable, so it is the sole entry in focus order.
//! let order: Vec<_> = facts.focus_order().map(|e| e.id).collect();
//! assert_eq!(order, vec![a]);
//! ```

use crate::geometry::{Position, Rect};
use crate::id::WidgetId;

/// One widget's facts for a single frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FactEntry {
    /// The widget's composed identity.
    pub id: WidgetId,
    /// The identity of the widget's declaration-scope parent, for the
    /// capture/bubble routing path. The frame root is its own parent.
    pub parent: WidgetId,
    /// The widget's area in absolute buffer coordinates.
    pub area: Rect,
    /// Whether the widget can hold keyboard focus.
    pub focusable: bool,
    /// The overlay layer this widget was declared in. Base = 0; each nested
    /// [`Frame::layer`](crate::frame::Frame::layer) increments it. Hit-testing
    /// prefers the highest layer and focus traversal is restricted to it
    /// (slice 7, ADR 0003 delta).
    pub layer: u8,
}

/// A widget's request to be scrolled into view, recorded as a fact.
///
/// Per the slice-7 design note this is **plumbing only**: a widget calls
/// [`RenderCtx::request_visibility`](crate::widget::RenderCtx::request_visibility)
/// with an area-relative rectangle it wants revealed, the frame records it here
/// keyed by the widget's identity, and a future scrollable container consumes it.
/// No generic container exists yet, so the fact is recorded and queryable
/// ([`FrameFacts::visibility_requests`]) but nothing acts on it this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityRequest {
    /// The requesting widget's identity.
    pub id: WidgetId,
    /// The rectangle to reveal, in absolute buffer coordinates (the frame
    /// resolves the widget's area-relative request against its area).
    pub area: Rect,
}

/// The facts collected while declaring one frame.
///
/// Entries are held in declaration order (= paint order). `FrameFacts` is the
/// input side of the declared frame: the runtime keeps the last frame's facts
/// and routes the next event against them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameFacts {
    entries: Vec<FactEntry>,
    visibility: Vec<VisibilityRequest>,
}

impl FrameFacts {
    /// Creates an empty facts record.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            visibility: Vec::new(),
        }
    }

    /// Records one widget's facts. Called by [`Frame`](crate::frame::Frame) as
    /// each widget is declared, so entries accumulate in declaration order.
    pub fn push(&mut self, entry: FactEntry) {
        self.entries.push(entry);
    }

    /// Records a widget's scroll-into-view request (slice-7 plumbing).
    ///
    /// Called by [`Frame`](crate::frame::Frame) when a widget invokes
    /// [`RenderCtx::request_visibility`](crate::widget::RenderCtx::request_visibility).
    /// The requests are queryable through [`visibility_requests`](Self::visibility_requests);
    /// no container consumes them yet.
    pub fn push_visibility(&mut self, request: VisibilityRequest) {
        self.visibility.push(request);
    }

    /// The scroll-into-view requests recorded this frame, in declaration order.
    pub fn visibility_requests(&self) -> impl Iterator<Item = &VisibilityRequest> {
        self.visibility.iter()
    }

    /// The highest layer any entry declared this frame (0 when there are no
    /// entries, or none declared above the base).
    #[must_use]
    pub fn top_layer(&self) -> u8 {
        self.entries
            .iter()
            .map(|entry| entry.layer)
            .max()
            .unwrap_or(0)
    }

    /// True if no facts were recorded (an empty frame, or before the first
    /// render).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The number of recorded entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Every entry in declaration order.
    pub fn iter(&self) -> impl Iterator<Item = &FactEntry> {
        self.entries.iter()
    }

    /// The facts for `id`, if it was declared this frame.
    ///
    /// The first matching entry wins; a well-formed frame has no duplicate ids
    /// (the store `debug_assert!`s on duplicates during render).
    #[must_use]
    pub fn get(&self, id: WidgetId) -> Option<&FactEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// The topmost entry whose area contains `position`, or `None`.
    ///
    /// "Topmost" prefers the **highest layer** first (an overlay swallows clicks
    /// over the base beneath it), then the **last** declared containing entry
    /// within that layer, since declaration order is last-wins z-order inside a
    /// layer. This is the hit-test mouse routing uses (ADR 0006 §5, slice-7
    /// layers).
    #[must_use]
    pub fn hit(&self, position: Position) -> Option<&FactEntry> {
        // Scan from the highest layer down; within a layer prefer the last
        // declared (topmost) containing entry.
        self.entries
            .iter()
            .filter(|entry| entry.area.contains(position))
            .max_by_key(|entry| entry.layer)
    }

    /// The focusable entries of the **topmost layer**, in declaration order —
    /// the tab-traversal order.
    ///
    /// Traversal derives from facts each frame (ADR 0006 §2), not from a
    /// retained tab-index attribute. When a modal declares a higher layer,
    /// traversal is restricted to it (containment): Tab cycles only the modal's
    /// focusables while it exists, and reconciles back to the base when it
    /// disappears (slice-7 layers).
    pub fn focus_order(&self) -> impl Iterator<Item = &FactEntry> {
        let top = self.top_layer();
        self.entries
            .iter()
            .filter(move |entry| entry.focusable && entry.layer == top)
    }

    /// The path from the root to `id` (inclusive), following parent links.
    ///
    /// The result is ordered root → target, the capture direction; reverse it
    /// for the bubble direction. Returns an empty vector if `id` is not present.
    /// The walk is bounded by [`len`](Self::len) to stay finite even if a parent
    /// link is malformed (a widget cannot be its own ancestor in a well-formed
    /// frame; the root is its own parent, which terminates the walk).
    #[must_use]
    pub fn path_to(&self, id: WidgetId) -> Vec<WidgetId> {
        if self.get(id).is_none() {
            return Vec::new();
        }
        let mut path = vec![id];
        let mut current = id;
        // Bound the walk to guarantee termination regardless of link shape.
        for _ in 0..self.entries.len() {
            let Some(entry) = self.get(current) else {
                break;
            };
            if entry.parent == current {
                // Reached a root (self-parent); the path is complete.
                break;
            }
            path.push(entry.parent);
            current = entry.parent;
        }
        path.reverse();
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Size;
    use crate::id::key;

    fn id(name: &str) -> WidgetId {
        WidgetId::ROOT.child(key(name))
    }

    fn entry(id: WidgetId, parent: WidgetId, area: Rect, focusable: bool) -> FactEntry {
        FactEntry {
            id,
            parent,
            area,
            focusable,
            layer: 0,
        }
    }

    fn layered(
        id: WidgetId,
        parent: WidgetId,
        area: Rect,
        focusable: bool,
        layer: u8,
    ) -> FactEntry {
        FactEntry {
            id,
            parent,
            area,
            focusable,
            layer,
        }
    }

    #[test]
    fn get_returns_declared_entry() {
        let mut facts = FrameFacts::new();
        let a = id("a");
        facts.push(entry(
            a,
            WidgetId::ROOT,
            Rect::from_size(Size::new(2, 1)),
            true,
        ));
        assert_eq!(facts.get(a).unwrap().id, a);
        assert!(facts.get(id("missing")).is_none());
    }

    #[test]
    fn focus_order_is_declaration_order_focusable_only() {
        let mut facts = FrameFacts::new();
        let a = id("a");
        let b = id("b");
        let c = id("c");
        facts.push(entry(a, WidgetId::ROOT, Rect::default(), true));
        facts.push(entry(b, WidgetId::ROOT, Rect::default(), false));
        facts.push(entry(c, WidgetId::ROOT, Rect::default(), true));
        let order: Vec<_> = facts.focus_order().map(|e| e.id).collect();
        assert_eq!(order, vec![a, c]);
    }

    #[test]
    fn hit_prefers_last_declared() {
        let mut facts = FrameFacts::new();
        let a = id("a");
        let b = id("b");
        let area = Rect::from_size(Size::new(4, 4));
        facts.push(entry(a, WidgetId::ROOT, area, false));
        facts.push(entry(b, WidgetId::ROOT, area, false));
        // Overlapping areas: the later declaration wins (topmost).
        assert_eq!(facts.hit(Position::new(1, 1)).unwrap().id, b);
        assert!(facts.hit(Position::new(9, 9)).is_none());
    }

    #[test]
    fn path_to_walks_parent_links_root_to_target() {
        let mut facts = FrameFacts::new();
        let scope = WidgetId::ROOT.child(key("scope"));
        let leaf = scope.child(key("leaf"));
        facts.push(entry(scope, WidgetId::ROOT, Rect::default(), false));
        facts.push(entry(leaf, scope, Rect::default(), true));
        assert_eq!(facts.path_to(leaf), vec![WidgetId::ROOT, scope, leaf]);
    }

    #[test]
    fn hit_prefers_highest_layer_over_last_declared() {
        // A base entry declared last would win by declaration order, but an
        // overlay on a higher layer sits on top and takes the click.
        let mut facts = FrameFacts::new();
        let base = id("base");
        let modal = id("modal");
        let area = Rect::from_size(Size::new(4, 4));
        facts.push(layered(modal, WidgetId::ROOT, area, false, 1));
        facts.push(layered(base, WidgetId::ROOT, area, false, 0));
        // Even though `base` was declared later, the higher layer wins.
        assert_eq!(facts.hit(Position::new(1, 1)).unwrap().id, modal);
    }

    #[test]
    fn focus_order_is_restricted_to_the_top_layer() {
        let mut facts = FrameFacts::new();
        let base = id("base");
        let ok = id("ok");
        let cancel = id("cancel");
        facts.push(layered(base, WidgetId::ROOT, Rect::default(), true, 0));
        facts.push(layered(ok, WidgetId::ROOT, Rect::default(), true, 1));
        facts.push(layered(cancel, WidgetId::ROOT, Rect::default(), true, 1));
        // A modal (layer 1) exists, so traversal never reaches the base focusable.
        let order: Vec<_> = facts.focus_order().map(|e| e.id).collect();
        assert_eq!(order, vec![ok, cancel]);
        assert_eq!(facts.top_layer(), 1);
    }

    #[test]
    fn visibility_requests_are_recorded_in_order() {
        let mut facts = FrameFacts::new();
        let a = id("a");
        let b = id("b");
        facts.push_visibility(VisibilityRequest {
            id: a,
            area: Rect::from_size(Size::new(2, 1)),
        });
        facts.push_visibility(VisibilityRequest {
            id: b,
            area: Rect::from_size(Size::new(3, 1)),
        });
        let ids: Vec<_> = facts
            .visibility_requests()
            .map(|request| request.id)
            .collect();
        assert_eq!(ids, vec![a, b]);
    }

    #[test]
    fn path_to_absent_is_empty() {
        let facts = FrameFacts::new();
        assert!(facts.path_to(id("nope")).is_empty());
    }

    #[test]
    fn path_to_root_self_parent_terminates() {
        // A widget parented directly at ROOT, where ROOT itself is not declared:
        // the path stops at the declared entry's parent.
        let mut facts = FrameFacts::new();
        let a = id("a");
        facts.push(entry(a, WidgetId::ROOT, Rect::default(), true));
        assert_eq!(facts.path_to(a), vec![WidgetId::ROOT, a]);
    }
}
