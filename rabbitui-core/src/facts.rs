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
//! (until layers land, ADR 0006 §5) approximates z-order for hit-testing.
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
//! });
//! facts.push(FactEntry {
//!     id: b,
//!     parent: WidgetId::ROOT,
//!     area: Rect::new(Position::new(0, 1), Size::new(4, 1)),
//!     focusable: false,
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
}

/// The facts collected while declaring one frame.
///
/// Entries are held in declaration order (= paint order). `FrameFacts` is the
/// input side of the declared frame: the runtime keeps the last frame's facts
/// and routes the next event against them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameFacts {
    entries: Vec<FactEntry>,
}

impl FrameFacts {
    /// Creates an empty facts record.
    #[must_use]
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Records one widget's facts. Called by [`Frame`](crate::frame::Frame) as
    /// each widget is declared, so entries accumulate in declaration order.
    pub fn push(&mut self, entry: FactEntry) {
        self.entries.push(entry);
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
    /// "Topmost" is the **last** declared containing entry, since declaration
    /// order approximates z-order until layers land (ADR 0006 §5). This is the
    /// hit-test mouse routing will use in a later slice.
    #[must_use]
    pub fn hit(&self, position: Position) -> Option<&FactEntry> {
        self.entries.iter().rev().find(|entry| entry.area.contains(position))
    }

    /// The focusable entries in declaration order — the tab-traversal order.
    ///
    /// Traversal derives from facts each frame (ADR 0006 §2), not from a
    /// retained tab-index attribute.
    pub fn focus_order(&self) -> impl Iterator<Item = &FactEntry> {
        self.entries.iter().filter(|entry| entry.focusable)
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
            let Some(entry) = self.get(current) else { break };
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
        FactEntry { id, parent, area, focusable }
    }

    #[test]
    fn get_returns_declared_entry() {
        let mut facts = FrameFacts::new();
        let a = id("a");
        facts.push(entry(a, WidgetId::ROOT, Rect::from_size(Size::new(2, 1)), true));
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
