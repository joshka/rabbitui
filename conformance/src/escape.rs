//! Named escape sequences the corpus fixtures are built from.
//!
//! Keeping the raw control bytes behind names does two things: the fixtures in
//! [`crate::corpus`] read like the escape sequences they encode, and the exact
//! byte spellings the harness treats as significant (sync-output framing, the
//! alt-screen enter, the SGR reset, erase-to-end-of-display) live in one place so
//! the spec's byte-level assertions and the runner agree on them.

/// Synchronized-output begin — DEC private mode 2026 set (`CSI ? 2026 h`).
///
/// Wraps a frame so the terminal presents the whole update atomically; degrades to
/// a no-op on terminals that do not implement it (spec §4.5).
pub const SYNC_BEGIN: &[u8] = b"\x1b[?2026h";

/// Synchronized-output end — DEC private mode 2026 reset (`CSI ? 2026 l`).
pub const SYNC_END: &[u8] = b"\x1b[?2026l";

/// Alternate-screen enter (`CSI ? 1049 h`). Hides the primary screen and its
/// scrollback; the commit-flush ordering guarantee (spec §6) is that pending
/// commit bytes precede this sequence.
pub const ALT_ENTER: &[u8] = b"\x1b[?1049h";

/// SGR reset (`CSI 0 m`). Must immediately precede every erase (the `BCE-RESET`
/// invariant, spec §4.4 / §10.1) so the erase does not inherit an active
/// background and flood vacated cells with it.
pub const SGR_RESET: &[u8] = b"\x1b[0m";

/// Erase-to-end-of-display (`CSI 0 J`, also spelled `CSI J`). Clears from the
/// cursor to the bottom of the screen — the shrink-clear of spec §4.4.
pub const ERASE_TO_END: &[u8] = b"\x1b[0J";

/// The short spelling of erase-to-end-of-display (`CSI J`), which some renderers
/// emit; the harness treats it as equivalent to [`ERASE_TO_END`].
pub const ERASE_TO_END_SHORT: &[u8] = b"\x1b[J";

/// The index where `needle` first occurs in `haystack`, if at all.
#[must_use]
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}
