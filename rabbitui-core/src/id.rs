//! Stable widget identity.
//!
//! Per `docs/adr/0002-widget-identity.md`: every widget instance is addressed
//! by a [`WidgetId`] composed from user-chosen [`Key`]s along the declaration
//! path (Xilem-style id-paths, carried as data). Identity is what lets focus,
//! scroll offsets, and cursors survive frames in a model where widgets
//! themselves are short-lived specs.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::id::{Key, WidgetId, key};
//!
//! // The same key at the same path is the same widget, frame after frame.
//! let a = WidgetId::ROOT.child(key("sidebar")).child(key("list"));
//! let b = WidgetId::ROOT.child(key("sidebar")).child(key("list"));
//! assert_eq!(a, b);
//!
//! // List items derive per-row keys from one base key.
//! let row0 = key("rows").index(0);
//! let row1 = key("rows").index(1);
//! assert_ne!(row0, row1);
//! ```

/// A user-chosen name for a widget within its parent.
///
/// Create with [`key`]; derive per-item keys with [`Key::index`]. Keys are
/// hashes: cheap to copy, compare, and compose, at the cost that the original
/// string is not recoverable (diagnostics carry the id, not the name).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key(u64);

/// Creates a [`Key`] from a name.
///
/// Names need only be unique among siblings; the full identity is the path of
/// keys from the root ([`WidgetId`]).
#[must_use]
pub const fn key(name: &str) -> Key {
    Key(fnv1a(FNV_OFFSET, name.as_bytes()))
}

impl Key {
    /// Derives a distinct key for the `i`-th item of a collection.
    ///
    /// Prefer a stable domain identifier over a position when items reorder:
    /// `key("rows").index(row.id)` keeps state attached to the row, not the
    /// slot.
    #[must_use]
    pub const fn index(self, i: usize) -> Key {
        Key(fnv1a(self.0, &i.to_le_bytes()))
    }
}

/// The composed identity of a widget instance: its key path from the root.
///
/// Equal paths are equal identities across frames — that is the entire
/// mechanism by which framework-retained state (focus, scroll, cursor)
/// survives re-declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WidgetId(u64);

impl WidgetId {
    /// The identity of the frame root.
    pub const ROOT: Self = Self(FNV_OFFSET);

    /// The identity of the child of `self` named by `key`.
    #[must_use]
    pub const fn child(self, key: Key) -> Self {
        Self(fnv1a(self.0, &key.0.to_le_bytes()))
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a, chained: `state` is the running hash, `bytes` are folded in.
const fn fnv1a(state: u64, bytes: &[u8]) -> u64 {
    let mut hash = state;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_path_same_id() {
        assert_eq!(
            WidgetId::ROOT.child(key("a")),
            WidgetId::ROOT.child(key("a"))
        );
    }

    #[test]
    fn different_names_differ() {
        assert_ne!(key("a"), key("b"));
        assert_ne!(
            WidgetId::ROOT.child(key("a")),
            WidgetId::ROOT.child(key("b"))
        );
    }

    #[test]
    fn nesting_matters() {
        let flat = WidgetId::ROOT.child(key("ab"));
        let nested = WidgetId::ROOT.child(key("a")).child(key("b"));
        assert_ne!(flat, nested);
    }

    #[test]
    fn indexed_keys_differ_from_base_and_each_other() {
        let base = key("rows");
        assert_ne!(base, base.index(0));
        assert_ne!(base.index(0), base.index(1));
    }

    #[test]
    fn key_is_const_constructible() {
        const SEARCH: Key = key("search");
        assert_eq!(SEARCH, key("search"));
    }
}
