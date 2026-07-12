//! Shared text helpers for the widget catalog.
//!
//! One home for the grapheme-safe, display-width truncation both [`Panel`] title
//! framing and [`Table`] cell/column clipping need, so there is a single copy of
//! the width-advance rule rather than a twin in each widget (the one-width-oracle
//! principle, ADR 0012). When `rabbitui-core` grows the mode-aware width-oracle
//! module ADR 0012 calls for, this moves behind it and the local `unicode-width`
//! dependency here goes away.
//!
//! [`Panel`]: crate::Panel
//! [`Table`]: crate::Table

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Returns the longest prefix of `text` whose display width does not exceed
/// `max`, split on grapheme boundaries so a wide grapheme never straddles the
/// limit (it is dropped whole rather than bisected).
///
/// Each grapheme advances by its display width clamped to `1..=2`: a wide
/// cluster counts as two cells, and a zero-width cluster (a lone combining mark)
/// counts as one so it can never make the prefix silently longer than it paints.
/// This matches the clip [`set_string`](rabbitui_core::widget::RenderContext::set_string)
/// applies at an area edge, so a widget can pre-truncate to a *column* narrower
/// than its area and get the same right-edge behavior.
pub(crate) fn truncate_to_width(text: &str, max: usize) -> &str {
    let mut width = 0usize;
    let mut end = 0usize;
    for grapheme in text.graphemes(true) {
        let advance = UnicodeWidthStr::width(grapheme).clamp(1, 2);
        if width + advance > max {
            break;
        }
        width += advance;
        end += grapheme.len();
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::truncate_to_width;

    #[test]
    fn narrow_prefix_is_returned_whole() {
        assert_eq!(truncate_to_width("abcdef", 3), "abc");
        assert_eq!(truncate_to_width("abc", 10), "abc");
        assert_eq!(truncate_to_width("abc", 0), "");
    }

    #[test]
    fn a_wide_grapheme_never_straddles_the_limit() {
        // "a"(1) + "世"(2) fills 3; a 3-cell budget keeps both, a 2-cell budget
        // drops the wide cluster whole rather than showing half of it.
        assert_eq!(truncate_to_width("a世", 3), "a世");
        assert_eq!(truncate_to_width("a世", 2), "a");
        assert_eq!(truncate_to_width("世", 1), "");
    }
}
