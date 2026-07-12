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
//!     role: rabbitui_core::accessibility::SemanticRole::None,
//! });
//! facts.push(FactEntry {
//!     id: b,
//!     parent: WidgetId::ROOT,
//!     area: Rect::new(Position::new(0, 1), Size::new(4, 1)),
//!     focusable: false,
//!     layer: 0,
//!     role: rabbitui_core::accessibility::SemanticRole::None,
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
    /// The widget's accessibility role, if it declared one via
    /// [`RenderContext::semantic_role`](crate::widget::RenderContext::semantic_role)
    /// ([`SemanticRole::None`](crate::accessibility::SemanticRole::None) otherwise).
    /// Recorded for a future accessibility exporter;
    /// nothing consumes it yet (ADR arc4 §5). The accessible *label* is a separate
    /// side table ([`FrameFacts::label`]), since it is an owned string.
    pub role: crate::accessibility::SemanticRole,
}

/// A widget's request to be scrolled into view, recorded as a fact.
///
/// Per the slice-7 design note this is **plumbing only**: a widget calls
/// [`RenderContext::request_visibility`](crate::widget::RenderContext::request_visibility)
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
    /// Devtools `id → source-name` side table (ADR arc4 §6): the leaf
    /// [`key`](crate::id::key) name each widget was declared under, captured only
    /// when the `devtools` feature is on. Combined with [`path_to`](Self::path_to)
    /// it yields the human path-of-names the facts inspector renders. Empty (and
    /// the field absent from equality's perspective) in release builds.
    #[cfg(feature = "devtools")]
    names: std::collections::HashMap<WidgetId, &'static str>,
    /// Accessibility `id → label` side table (ADR arc4 §5): the accessible name a
    /// widget declared via
    /// [`RenderContext::label`](crate::widget::RenderContext::label), for a future accessibility
    /// exporter. A side table (not a [`FactEntry`] field) so labels can be owned
    /// strings while `FactEntry` stays [`Copy`]. Present in every build — accessibility is
    /// not a devtools-only concern.
    labels: std::collections::HashMap<WidgetId, String>,
}

impl FrameFacts {
    /// Creates an empty facts record.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            visibility: Vec::new(),
            #[cfg(feature = "devtools")]
            names: std::collections::HashMap::new(),
            labels: std::collections::HashMap::new(),
        }
    }

    /// Records a widget's accessible label (ADR arc4 §5).
    ///
    /// Called by [`Frame`](crate::frame::Frame) when a widget declares one via
    /// [`RenderContext::label`](crate::widget::RenderContext::label). Queryable through
    /// [`label`](Self::label); nothing consumes it yet — this is the accessibility
    /// groundwork the exporter will read.
    pub fn record_label(&mut self, id: WidgetId, label: impl Into<String>) {
        self.labels.insert(id, label.into());
    }

    /// The accessible label `id` declared this frame, if any.
    #[must_use]
    pub fn label(&self, id: WidgetId) -> Option<&str> {
        self.labels.get(&id).map(String::as_str)
    }

    /// The accessibility role `id` declared this frame, or
    /// [`SemanticRole::None`](crate::accessibility::SemanticRole::None) when absent.
    #[must_use]
    pub fn role(&self, id: WidgetId) -> crate::accessibility::SemanticRole {
        self.get(id)
            .map_or(crate::accessibility::SemanticRole::None, |e| e.role)
    }

    /// Records the source name a widget was declared under (devtools only).
    ///
    /// Called by [`Frame`](crate::frame::Frame) as it composes each id, so the
    /// inspector can render `id → name` paths. A no-op in release builds (the
    /// method still exists for a uniform call site, but the feature-gated map is
    /// absent, so this compiles to nothing).
    #[cfg(feature = "devtools")]
    pub fn record_name(&mut self, id: WidgetId, name: &'static str) {
        self.names.insert(id, name);
    }

    /// The source name `id` was declared under, if devtools captured it.
    ///
    /// Returns `None` in release builds or for an id declared without a name (a
    /// composed scope id that was never a widget). See [`name_path`](Self::name_path)
    /// for the full human path.
    #[cfg(feature = "devtools")]
    #[must_use]
    pub fn name(&self, id: WidgetId) -> Option<&'static str> {
        self.names.get(&id).copied()
    }

    /// The human path-of-names from the root to `id` (devtools only): the leaf
    /// name of each ancestor that carried one, root → target.
    ///
    /// Built from [`path_to`](Self::path_to) (the id path) resolved through the
    /// `id → name` table. Ancestors with no captured name (bare scope ids) are
    /// skipped, so `["sidebar", "list", "row"]` reads as the declaration path a
    /// developer wrote. Empty when `id` is absent or nothing on its path was named.
    #[cfg(feature = "devtools")]
    #[must_use]
    pub fn name_path(&self, id: WidgetId) -> Vec<&'static str> {
        self.path_to(id)
            .into_iter()
            .filter_map(|step| self.name(step))
            .collect()
    }

    /// Records one widget's facts. Called by [`Frame`](crate::frame::Frame) as
    /// each widget is declared, so entries accumulate in declaration order.
    pub fn push(&mut self, entry: FactEntry) {
        self.entries.push(entry);
    }

    /// Records a widget's scroll-into-view request (slice-7 plumbing).
    ///
    /// Called by [`Frame`](crate::frame::Frame) when a widget invokes
    /// [`RenderContext::request_visibility`](crate::widget::RenderContext::request_visibility).
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

    /// One human-readable line per entry describing the frame's facts tree — the
    /// shared format the [`facts::dump`](dump) log seam and the widgets-crate
    /// `FactsInspector` both render (ADR arc4 §7). Devtools only.
    ///
    /// Each line reads:
    ///
    /// ```text
    /// [F] sidebar/list  L0  focusable  area=1,0 20x10  vis=0,0 4x1
    /// ```
    ///
    /// — a focus marker (`[F]` for the focused id, `[ ]` otherwise), the human
    /// path-of-names ([`name_path`](Self::name_path), joined by `/`, falling back
    /// to the raw id when unnamed), the layer, a `focusable` tag when the widget
    /// can hold focus, the absolute area, and a `vis=` clause when the widget
    /// requested visibility this frame. Entries are listed in declaration order,
    /// which is paint / z order. `focus` is the currently-focused id, if any.
    #[cfg(feature = "devtools")]
    #[must_use]
    pub fn dump_lines(&self, focus: Option<WidgetId>) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| {
                let marker = if focus == Some(entry.id) {
                    "[F]"
                } else {
                    "[ ]"
                };
                let names = self.name_path(entry.id);
                let path = if names.is_empty() {
                    format!("#{:016x}", entry.id.raw_for_dump())
                } else {
                    names.join("/")
                };
                let focusable = if entry.focusable { "  focusable" } else { "" };
                let area = &entry.area;
                let mut line = format!(
                    "{marker} {path}  L{}{focusable}  area={},{} {}x{}",
                    entry.layer, area.origin.x, area.origin.y, area.size.width, area.size.height,
                );
                // A11y facts (ADR arc4 §5), when the widget declared them.
                if entry.role != crate::accessibility::SemanticRole::None {
                    line.push_str(&format!("  role={}", entry.role.as_str()));
                }
                if let Some(label) = self.labels.get(&entry.id) {
                    line.push_str(&format!("  label={label:?}"));
                }
                if let Some(request) = self.visibility.iter().find(|r| r.id == entry.id) {
                    let v = &request.area;
                    line.push_str(&format!(
                        "  vis={},{} {}x{}",
                        v.origin.x, v.origin.y, v.size.width, v.size.height
                    ));
                }
                line
            })
            .collect()
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

/// Writes the frame's facts tree to the log seam — the non-visual half of the
/// devtools inspector (ADR arc4 §7). Devtools only.
///
/// Pushes one [`LogRecord`](crate::log::LogRecord) per entry (the exact lines
/// [`FrameFacts::dump_lines`] produces, so the log and the on-screen
/// `FactsInspector` read identically) at [`Level::Debug`](crate::log::Level::Debug)
/// under the `rabbitui::facts` target, into the same [`LogHandle`](crate::log::LogHandle)
/// ring the `LogOverlay` renders. A one-shot, read-only diagnostic an app wires to
/// a chord next to the inspector toggle.
///
/// # Examples
///
/// ```
/// use rabbitui_core::facts::{self, FrameFacts};
/// use rabbitui_core::log::LogHandle;
///
/// let facts = FrameFacts::new();
/// let logs = LogHandle::with_capacity(64);
/// facts::dump(&facts, None, &logs); // empty frame: nothing to log
/// assert_eq!(logs.len(), 0);
/// ```
#[cfg(feature = "devtools")]
pub fn dump(facts: &FrameFacts, focus: Option<WidgetId>, handle: &crate::log::LogHandle) {
    use crate::log::{Level, LogRecord};
    for line in facts.dump_lines(focus) {
        handle.push(LogRecord::new(Level::Debug, "rabbitui::facts", line));
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
            role: crate::accessibility::SemanticRole::None,
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
            role: crate::accessibility::SemanticRole::None,
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

    #[cfg(feature = "devtools")]
    #[test]
    fn dump_lines_render_the_facts_tree_in_the_shared_format() {
        use crate::buffer::Buffer;
        use crate::frame::Frame;
        use crate::geometry::Position;
        use crate::store::StateStore;
        use crate::widget::{RenderContext, Widget};

        // A focusable leaf so the dump shows the `focusable` tag and a focus marker.
        struct Focusable;
        impl Widget for Focusable {
            type State = ();
            fn render(&self, _s: &mut (), ctx: &mut RenderContext<'_>) {
                ctx.focusable(true);
            }
        }
        struct Passive;
        impl Widget for Passive {
            type State = ();
            fn render(&self, _s: &mut (), _ctx: &mut RenderContext<'_>) {}
        }

        let mut buffer = Buffer::new(Size::new(20, 6));
        let mut store = StateStore::new();
        store.begin_frame();
        let mut frame = Frame::new(&mut buffer, &mut store);
        frame.widget(
            key("banner"),
            Rect::new(Position::ORIGIN, Size::new(20, 1)),
            &Passive,
        );
        frame.scoped(key("sidebar"), |f| {
            f.widget(
                key("list"),
                Rect::new(Position::new(0, 1), Size::new(6, 4)),
                &Focusable,
            );
        });
        let facts = frame.finish();
        store.end_frame();

        // Focus the list, so its line carries the `[F]` marker.
        let list = WidgetId::ROOT.child(key("sidebar")).child(key("list"));
        let lines = facts.dump_lines(Some(list));
        assert_eq!(
            lines,
            vec![
                "[ ] banner  L0  area=0,0 20x1".to_string(),
                "[F] sidebar/list  L0  focusable  area=0,1 6x4".to_string(),
            ]
        );
    }

    #[cfg(feature = "devtools")]
    #[test]
    fn dump_writes_one_debug_record_per_entry_to_the_log_seam() {
        let mut facts = FrameFacts::new();
        let a = id("a");
        facts.push(entry(
            a,
            WidgetId::ROOT,
            Rect::from_size(Size::new(2, 1)),
            true,
        ));
        facts.record_name(a, "a");
        let handle = crate::log::LogHandle::with_capacity(16);
        dump(&facts, Some(a), &handle);
        let records = handle.snapshot();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].target, "rabbitui::facts");
        assert!(records[0].message.starts_with("[F] a  L0  focusable"));
    }

    #[cfg(feature = "devtools")]
    #[test]
    fn name_path_resolves_declaration_names_root_to_target() {
        let mut facts = FrameFacts::new();
        let scope = WidgetId::ROOT.child(key("panel"));
        let leaf = scope.child(key("ok"));
        facts.push(entry(scope, WidgetId::ROOT, Rect::default(), false));
        facts.push(entry(leaf, scope, Rect::default(), true));
        facts.record_name(scope, "panel");
        facts.record_name(leaf, "ok");
        // ROOT carries no name, so it is skipped; the human path is the scope then leaf.
        assert_eq!(facts.name_path(leaf), vec!["panel", "ok"]);
        assert_eq!(facts.name(leaf), Some("ok"));
        assert_eq!(facts.name(WidgetId::ROOT.child(key("absent"))), None);
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
