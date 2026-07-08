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
/// hashes: cheap to copy, compare, and compose, at the cost that — in release
/// builds — the original string is not recoverable (diagnostics carry the id,
/// not the name).
///
/// # Devtools name capture
///
/// Behind the default-on-in-dev `devtools` feature (ADR arc4 §6), a `Key` also
/// carries the source name it was built from (interned to a `&'static str` so the
/// key stays `Copy`), so the frame can record an `id → name` side table
/// ([`FrameFacts`](crate::facts::FrameFacts)) that the inspector renders as human
/// paths. Identity and hashing stay **hash-only** (see the manual
/// [`PartialEq`]/[`Hash`] impls), so the captured name never affects routing,
/// equality, or the composed [`WidgetId`]. A `cargo build --release` (feature off)
/// is the exact zero-cost FNV-only `Key` — one `u64`, no string, `key` a `const fn`.
#[derive(Debug, Clone, Copy)]
pub struct Key {
    hash: u64,
    /// The source name, captured only under `devtools` for the inspector's
    /// `id → name` table. Never part of identity or hashing.
    #[cfg(feature = "devtools")]
    name: &'static str,
}

// Identity and hashing are **hash-only** in every build: the captured name is a
// diagnostic, not part of the key's meaning, so two keys are equal iff their FNV
// hashes are (matching the release-build `Key(u64)` exactly). This keeps the
// `devtools` build behaviorally identical to release for routing and equality.
impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}
impl Eq for Key {}
impl core::hash::Hash for Key {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

/// Creates a [`Key`] from a name.
///
/// Names need only be unique among siblings; the full identity is the path of
/// keys from the root ([`WidgetId`]).
///
/// The signature (`&str`) and the FNV hash are identical in every build — the
/// `devtools` capture is additive and never changes a key's identity. In release
/// (feature off) this is a zero-cost `const fn`: one FNV fold, no allocation.
#[cfg(not(feature = "devtools"))]
#[must_use]
pub const fn key(name: &str) -> Key {
    Key {
        hash: fnv1a(FNV_OFFSET, name.as_bytes()),
    }
}

/// Creates a [`Key`] from a name, additionally retaining the name for the
/// devtools inspector's `id → name` table (ADR arc4 §6).
///
/// Same signature and same FNV hash as the release `const fn` (above), so every
/// existing call site — including dynamic ones like `key(&format!("row-{i}"))` —
/// compiles unchanged. The retained name is interned to a process-wide
/// `&'static str` (deduplicated by content) so `Key` stays `Copy` and the frame
/// can record it without lifetime plumbing; this small dev-only cost is compiled
/// out entirely in release. Not `const`: interning runs at first construction.
#[cfg(feature = "devtools")]
#[must_use]
pub fn key(name: &str) -> Key {
    Key {
        hash: fnv1a(FNV_OFFSET, name.as_bytes()),
        name: devtools::intern(name),
    }
}

/// Devtools-only name interner: dedups source names to `&'static str` so a `Key`
/// can carry one and stay `Copy`. Dev-only; compiled out in release.
#[cfg(feature = "devtools")]
mod devtools {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};

    fn table() -> &'static Mutex<HashSet<&'static str>> {
        static TABLE: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
        TABLE.get_or_init(|| Mutex::new(HashSet::new()))
    }

    /// Returns a `'static` copy of `name`, leaking it at most once per distinct
    /// value. Widget names come from a small, fixed set of declaration sites, so
    /// the interned set is bounded in practice.
    pub(crate) fn intern(name: &str) -> &'static str {
        let mut set = table().lock().expect("name interner poisoned");
        if let Some(existing) = set.get(name) {
            return existing;
        }
        let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
        set.insert(leaked);
        leaked
    }
}

impl Key {
    /// Derives a distinct key for the `i`-th item of a collection.
    ///
    /// Prefer a stable domain identifier over a position when items reorder:
    /// `key("rows").index(row.id)` keeps state attached to the row, not the
    /// slot.
    ///
    /// The derived key keeps the base key's captured name (under `devtools`): the
    /// composed id already disambiguates rows, and the inspector shows the base
    /// name (`rows`) for each — the human-meaningful part.
    #[must_use]
    pub const fn index(self, i: usize) -> Key {
        Key {
            hash: fnv1a(self.hash, &i.to_le_bytes()),
            #[cfg(feature = "devtools")]
            name: self.name,
        }
    }

    /// The FNV hash this key composes with — the sole identity input.
    pub(crate) const fn hash(self) -> u64 {
        self.hash
    }

    /// The source name captured under `devtools`, for the frame's `id → name`
    /// side table. Only present when the feature is on.
    #[cfg(feature = "devtools")]
    #[must_use]
    pub(crate) const fn name(self) -> &'static str {
        self.name
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
        Self(fnv1a(self.0, &key.hash().to_le_bytes()))
    }

    /// The raw composed hash, for the devtools facts dump to print when a widget
    /// carries no captured name (a bare, unnamed id).
    #[cfg(feature = "devtools")]
    #[must_use]
    pub fn raw_for_dump(self) -> u64 {
        self.0
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

    #[cfg(not(feature = "devtools"))]
    #[test]
    fn key_is_const_constructible() {
        // Release builds keep `key` a `const fn` for compile-time key constants.
        const SEARCH: Key = key("search");
        assert_eq!(SEARCH, key("search"));
    }

    #[test]
    fn key_round_trips_by_name_in_every_build() {
        // The identity guarantee holds regardless of const-ness or capture.
        assert_eq!(key("search"), key("search"));
    }

    #[test]
    fn equality_is_hash_only_in_every_build() {
        // Two keys are equal iff their FNV hashes match — the captured name (under
        // devtools) is a diagnostic and never enters identity, so the devtools
        // build behaves exactly like the release `Key(u64)`.
        assert_eq!(key("a"), key("a"));
        assert_ne!(key("a"), key("b"));
        // A derived key differs from its base by hash regardless of the shared name.
        assert_ne!(key("rows"), key("rows").index(0));
    }

    #[cfg(feature = "devtools")]
    #[test]
    fn devtools_captures_source_name() {
        assert_eq!(key("sidebar").name(), "sidebar");
        // A derived key keeps the base name; the composed id disambiguates rows.
        assert_eq!(key("rows").index(3).name(), "rows");
    }
}
