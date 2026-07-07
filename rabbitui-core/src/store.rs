//! The framework-owned per-widget state store.
//!
//! Per `docs/adr/0002-widget-identity.md`: widgets are short-lived specs, so
//! anything that must survive frames — focus, scroll offsets, cursors,
//! collapsed flags, caches — lives here, keyed by [`WidgetId`]. Apps never
//! touch this store directly; the frame lends each widget `&mut` access to its
//! own slice during render.
//!
//! State for widgets that stop being declared is dropped after a grace period
//! of absent frames, so a widget that disappears briefly (a collapsed panel,
//! a tab switch) keeps its state, while state for genuinely removed widgets
//! does not leak.

use std::any::Any;
use std::collections::HashMap;

use crate::id::WidgetId;

/// How many frames a widget may go undeclared before its state is dropped.
const DEFAULT_GRACE_FRAMES: u64 = 60;

/// Per-widget retained state, keyed by identity.
///
/// # Examples
///
/// ```
/// use rabbitui_core::id::{WidgetId, key};
/// use rabbitui_core::store::StateStore;
///
/// #[derive(Default)]
/// struct ScrollState {
///     offset: u16,
/// }
///
/// let mut store = StateStore::new();
/// let id = WidgetId::ROOT.child(key("list"));
///
/// store.begin_frame();
/// store.get_or_default::<ScrollState>(id).offset = 5;
/// store.end_frame();
///
/// store.begin_frame();
/// assert_eq!(store.get_or_default::<ScrollState>(id).offset, 5);
/// store.end_frame();
/// ```
#[derive(Debug, Default)]
pub struct StateStore {
    entries: HashMap<WidgetId, Entry>,
    frame: u64,
    grace_frames: u64,
}

struct Entry {
    state: Box<dyn Any>,
    last_seen: u64,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry").field("last_seen", &self.last_seen).finish_non_exhaustive()
    }
}

impl StateStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self { entries: HashMap::new(), frame: 0, grace_frames: DEFAULT_GRACE_FRAMES }
    }

    /// Marks the start of a frame. Called by the runtime, once per frame,
    /// before any widget renders.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// Returns the retained state for `id`, creating it if absent.
    ///
    /// Requesting the same id twice within one frame is a duplicate-identity
    /// bug (two widgets declared with the same key path); debug builds panic,
    /// release builds share the state.
    ///
    /// If the type stored for `id` differs from `S` (the key path was reused
    /// for a different widget type), the state is reset to `S::default()`.
    pub fn get_or_default<S: Default + 'static>(&mut self, id: WidgetId) -> &mut S {
        let frame = self.frame;
        let entry = self
            .entries
            .entry(id)
            .and_modify(|entry| {
                debug_assert_ne!(
                    entry.last_seen, frame,
                    "duplicate WidgetId {id:?}: two widgets declared with the same key path \
                     in one frame"
                );
                entry.last_seen = frame;
                if !entry.state.is::<S>() {
                    entry.state = Box::new(S::default());
                }
            })
            .or_insert_with(|| Entry { state: Box::new(S::default()), last_seen: frame });
        entry.state.downcast_mut::<S>().expect("state type ensured above")
    }

    /// Lends type-erased `&mut` access to the state already stored for `id`.
    ///
    /// Unlike [`get_or_default`](Self::get_or_default) this neither creates state
    /// nor needs the concrete type: it is how the router reaches a widget's
    /// retained state during event dispatch, where only the erased handler thunk
    /// (which knows the type) will downcast it. Returns `None` if `id` holds no
    /// state (it was never declared, or its state was dropped).
    ///
    /// This does **not** touch `last_seen`: dispatch happens between frames and
    /// must not be mistaken for a re-declaration.
    #[must_use]
    pub fn get_dyn_mut(&mut self, id: WidgetId) -> Option<&mut dyn Any> {
        self.entries.get_mut(&id).map(|entry| entry.state.as_mut())
    }

    /// Marks the end of a frame and drops state for widgets that have not
    /// been declared within the grace period.
    pub fn end_frame(&mut self) {
        let cutoff = self.frame.saturating_sub(self.grace_frames);
        self.entries.retain(|_, entry| entry.last_seen >= cutoff);
    }

    /// The number of widgets currently holding retained state.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if no widget holds retained state.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::key;

    #[derive(Default)]
    struct Counter(u32);

    fn id(name: &str) -> WidgetId {
        WidgetId::ROOT.child(key(name))
    }

    #[test]
    fn state_persists_across_frames() {
        let mut store = StateStore::new();
        store.begin_frame();
        store.get_or_default::<Counter>(id("a")).0 = 7;
        store.end_frame();
        store.begin_frame();
        assert_eq!(store.get_or_default::<Counter>(id("a")).0, 7);
    }

    #[test]
    fn undeclared_state_drops_after_grace() {
        let mut store = StateStore::new();
        store.grace_frames = 2;
        store.begin_frame();
        store.get_or_default::<Counter>(id("a")).0 = 7;
        store.end_frame();
        for _ in 0..3 {
            store.begin_frame();
            store.end_frame();
        }
        assert!(store.is_empty());
        store.begin_frame();
        assert_eq!(store.get_or_default::<Counter>(id("a")).0, 0);
    }

    #[test]
    fn type_change_resets_state() {
        #[derive(Default)]
        struct Other(#[allow(dead_code)] bool);

        let mut store = StateStore::new();
        store.begin_frame();
        store.get_or_default::<Counter>(id("a")).0 = 7;
        store.end_frame();
        store.begin_frame();
        let _ = store.get_or_default::<Other>(id("a"));
        store.end_frame();
        store.begin_frame();
        assert_eq!(store.get_or_default::<Counter>(id("a")).0, 0);
    }

    #[test]
    #[should_panic(expected = "duplicate WidgetId")]
    #[cfg(debug_assertions)]
    fn duplicate_declaration_panics_in_debug() {
        let mut store = StateStore::new();
        store.begin_frame();
        let _ = store.get_or_default::<Counter>(id("a"));
        let _ = store.get_or_default::<Counter>(id("a"));
    }
}
