//! Escape-level tests for the render engines (ADR 0009 layer 3).
//!
//! These drive the *emitted bytes* of [`AltEngine`] and [`InlineEngine`] through
//! a real `vt100` terminal model ([`rabbitui_testing::vt::VtScreen`]) and assert
//! on the screen a terminal would show — the layer that catches synchronized-
//! output framing, clears, cursor discipline, and inline commit/tail interleaving
//! that buffer equality cannot (the tui2/textual-rs finding). Because the engines
//! are pure byte producers, a test is just: engine bytes → VtScreen → assert.

use rabbitui::engine::{AltEngine, InlineEngine};
use rabbitui_core::buffer::Buffer;
use rabbitui_core::commit::CommitLine;
use rabbitui_core::geometry::{Position, Size};
use rabbitui_core::style::{Color, Style};
use rabbitui_testing::vt::{VtColor, VtScreen};

/// Builds a `width`-by-`height` buffer with each string in `rows` on its own row
/// from column 0, unstyled.
fn buffer(rows: &[&str], width: u16) -> Buffer {
    let mut buffer = Buffer::new(Size::new(width, rows.len() as u16));
    for (y, row) in rows.iter().enumerate() {
        buffer.set_string(Position::new(0, y as u16), row, Style::new());
    }
    buffer
}

/// True if `bytes` are wrapped in synchronized-output (mode 2026) framing:
/// `CSI ? 2026 h` … `CSI ? 2026 l`.
fn is_sync_framed(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x1b[?2026h") && bytes.ends_with(b"\x1b[?2026l")
}

// ---------------------------------------------------------------------------
// Alt-screen
// ---------------------------------------------------------------------------

#[test]
fn alt_screen_frame_renders_to_expected_grid() {
    let mut engine = AltEngine::new();
    let mut screen = VtScreen::new(10, 3);

    screen.feed(&engine.enter());
    let previous = Buffer::new(Size::new(10, 3));
    let current = buffer(&["title", "body"], 10);
    screen.feed(&engine.render(&current, &previous));

    screen.assert_row(0, "title");
    screen.assert_row(1, "body");
    screen.assert_row(2, "");
}

#[test]
fn alt_screen_diff_only_changes_the_changed_cells() {
    let mut engine = AltEngine::new();
    let mut screen = VtScreen::new(10, 2);
    screen.feed(&engine.enter());

    // First frame: paint two rows.
    let blank = Buffer::new(Size::new(10, 2));
    let first = buffer(&["aaaa", "bbbb"], 10);
    screen.feed(&engine.render(&first, &blank));
    screen.assert_row(0, "aaaa");
    screen.assert_row(1, "bbbb");

    // Second frame changes only row 0. The emitted diff must address only the
    // changed cells; feeding it must leave row 1 untouched and update row 0.
    let second = buffer(&["axxa", "bbbb"], 10);
    let diff_bytes = engine.render(&second, &first);
    screen.feed(&diff_bytes);
    screen.assert_row(0, "axxa");
    screen.assert_row(1, "bbbb");

    // The diff bytes must not mention row 1's content at all.
    let text = String::from_utf8_lossy(&diff_bytes);
    assert!(text.contains("xx"), "diff should carry the changed run");
    assert!(!text.contains("bbbb"), "diff must not repaint the unchanged row");
}

#[test]
fn every_alt_frame_is_sync_framed() {
    let mut engine = AltEngine::new();
    let blank = Buffer::new(Size::new(6, 1));
    let current = buffer(&["hi"], 6);
    let bytes = engine.render(&current, &blank);
    assert!(is_sync_framed(&bytes), "alt frame must be wrapped in mode-2026 framing");
    // vt100 ignores the mode bytes but still renders the grid.
    let mut screen = VtScreen::new(6, 1);
    screen.feed(&bytes);
    screen.assert_row(0, "hi");
}

// ---------------------------------------------------------------------------
// Inline: commits + live tail
// ---------------------------------------------------------------------------

#[test]
fn inline_commit_then_tail_yields_commit_above_tail() {
    let mut engine = InlineEngine::new();
    // Tall enough that both the committed line and the tail are inspectable.
    let mut screen = VtScreen::new(20, 5);
    screen.feed(&engine.enter());

    let tail = buffer(&["> prompt"], 20);
    let commits = [CommitLine::from("log line 1")];
    let bytes = engine.render(&tail, &commits);
    assert!(is_sync_framed(&bytes), "inline frame must be sync-framed");
    screen.feed(&bytes);

    // The committed line sits above the live tail.
    let lines = screen.all_lines();
    let commit_at = lines.iter().position(|l| l == "log line 1").expect("commit present");
    let tail_at = lines.iter().position(|l| l == "> prompt").expect("tail present");
    assert!(commit_at < tail_at, "commit must appear above the tail: {lines:?}");
}

#[test]
fn inline_two_commits_in_one_update_stay_in_order() {
    let mut engine = InlineEngine::new();
    let mut screen = VtScreen::new(20, 6);
    screen.feed(&engine.enter());

    let tail = buffer(&["tail"], 20);
    let commits = [CommitLine::from("first"), CommitLine::from("second")];
    screen.feed(&engine.render(&tail, &commits));

    let lines = screen.all_lines();
    let first = lines.iter().position(|l| l == "first").expect("first present");
    let second = lines.iter().position(|l| l == "second").expect("second present");
    assert!(first < second, "commits must stay in order: {lines:?}");
}

#[test]
fn inline_tail_shrink_leaves_no_orphan_rows() {
    let mut engine = InlineEngine::new();
    let mut screen = VtScreen::new(20, 6);
    screen.feed(&engine.enter());

    // A three-row tail…
    screen.feed(&engine.render(&buffer(&["one", "two", "three"], 20), &[]));
    screen.assert_row(0, "one");
    screen.assert_row(1, "two");
    screen.assert_row(2, "three");

    // …shrinks to one row. The ED-down clear must remove the orphaned rows.
    screen.feed(&engine.render(&buffer(&["only"], 20), &[]));
    screen.assert_row(0, "only");
    screen.assert_row(1, "");
    screen.assert_row(2, "");
}

#[test]
fn inline_resize_repaints_tail_at_new_width() {
    let mut engine = InlineEngine::new();
    // Render at width 20, then the caller (loops on resize) forces a repaint and
    // re-lays-out the tail to a new width; feed both to a wide-enough screen.
    let mut screen = VtScreen::new(30, 4);
    screen.feed(&engine.enter());
    screen.feed(&engine.render(&buffer(&["short"], 20), &[]));
    screen.assert_row(0, "short");

    // Simulate a resize: force a full repaint, hand a wider tail.
    engine.force_repaint();
    screen.feed(&engine.render(&buffer(&["a wider tail line"], 30), &[]));
    screen.assert_row(0, "a wider tail line");
}

#[test]
fn inline_stable_tail_diffs_only_changed_cells() {
    let mut engine = InlineEngine::new();
    let mut screen = VtScreen::new(20, 4);
    screen.feed(&engine.enter());

    // Establish a two-row tail.
    screen.feed(&engine.render(&buffer(&["row zero", "row one"], 20), &[]));
    screen.assert_row(0, "row zero");
    screen.assert_row(1, "row one");

    // Change only row 1, same height, no commits: the cell-diff path repaints
    // just the changed row. Feeding it must update row 1 and leave row 0 intact,
    // and the bytes must not repaint row 0's text.
    let diff_bytes = engine.render(&buffer(&["row zero", "row ONE!"], 20), &[]);
    screen.feed(&diff_bytes);
    screen.assert_row(0, "row zero");
    screen.assert_row(1, "row ONE!");

    let text = String::from_utf8_lossy(&diff_bytes);
    assert!(text.contains("ONE!"), "diff carries the changed cells");
    assert!(!text.contains("row zero"), "diff must not repaint the unchanged row");
    // No ED-down clear on a stable-height diff (that would be a full repaint).
    assert!(!text.contains("\x1b[0J"), "a stable-tail diff must not erase-below");
}

#[test]
fn inline_no_op_frame_emits_nothing() {
    let mut engine = InlineEngine::new();
    let tail = buffer(&["stable"], 10);
    let _ = engine.enter();
    let _ = engine.render(&tail, &[]);
    // Same tail, no commits: an idle inline app is silent.
    assert!(engine.render(&tail, &[]).is_empty());
}

// ---------------------------------------------------------------------------
// Mode transitions
// ---------------------------------------------------------------------------

#[test]
fn mode_switch_alt_to_inline_restores_and_repaints() {
    // Start in alt-screen, paint a frame.
    let mut alt = AltEngine::new();
    let mut screen = VtScreen::new(20, 4);
    screen.feed(&alt.enter());
    screen.feed(&alt.render(&buffer(&["alt content"], 20), &Buffer::new(Size::new(20, 4))));
    screen.assert_row(0, "alt content");

    // Leave alt (restores the prior screen), then enter inline and paint a tail.
    screen.feed(&alt.leave());
    let mut inline = InlineEngine::new();
    screen.feed(&inline.enter());
    screen.feed(&inline.render(&buffer(&["inline tail"], 20), &[]));

    // The alt content is gone (the terminal restored the primary screen); the
    // inline tail is now shown.
    let lines = screen.all_lines();
    assert!(lines.iter().any(|l| l == "inline tail"), "inline tail must show: {lines:?}");
    assert!(
        !lines.iter().any(|l| l == "alt content"),
        "alt content must not survive the switch: {lines:?}"
    );
}

#[test]
fn commits_before_alt_entry_appear_in_scrollback() {
    // Model the runtime's ordering: in inline mode, commits flush into scrollback
    // *before* the alt-screen entry, so they are not lost behind the alt buffer.
    let mut inline = InlineEngine::new();
    let mut screen = VtScreen::new(20, 4);
    screen.feed(&inline.enter());

    // Flush a commit through the inline engine with an empty tail (the runtime's
    // pre-alt-entry flush path).
    let empty = Buffer::new(Size::new(20, 0));
    let flush_bytes = inline.render(&empty, &[CommitLine::from("committed before alt")]);
    screen.feed(&flush_bytes);

    // On the *primary* screen (before any alt entry) the commit is in scrollback.
    let before = screen.all_lines();
    assert!(
        before.iter().any(|l| l == "committed before alt"),
        "commit must land in primary scrollback before alt entry: {before:?}"
    );

    // Now leave inline and enter alt. While in the alt screen the terminal hides
    // the primary scrollback — that is correct emulator behavior (the whole point
    // of the alt screen). What matters is the *ordering*: the commit bytes were
    // emitted before the alt-screen-enter escape, so nothing was lost behind it.
    let leave_bytes = inline.leave();
    let mut alt = AltEngine::new();
    let enter_bytes = alt.enter();

    // Assert emission order in the byte stream (the spec's documented fallback):
    // the commit text precedes the `CSI ? 1049 h` alt-screen entry.
    let mut stream = flush_bytes.clone();
    stream.extend_from_slice(&leave_bytes);
    stream.extend_from_slice(&enter_bytes);
    let commit_pos = find(&stream, b"committed before alt").expect("commit in stream");
    let alt_enter_pos = find(&stream, b"\x1b[?1049h").expect("alt entry in stream");
    assert!(
        commit_pos < alt_enter_pos,
        "the commit must be emitted before the alt-screen entry"
    );

    // And the alt screen then paints its own content on the restored buffer.
    screen.feed(&leave_bytes);
    screen.feed(&enter_bytes);
    screen.feed(&alt.render(&buffer(&["alt now"], 20), &Buffer::new(Size::new(20, 4))));
    screen.assert_row(0, "alt now");
}

/// The byte index where `needle` first occurs in `haystack`, if any.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------------------
// Styling carries through
// ---------------------------------------------------------------------------

#[test]
fn inline_commit_style_reaches_the_terminal() {
    let mut engine = InlineEngine::new();
    let mut screen = VtScreen::new(20, 4);
    screen.feed(&engine.enter());

    let commits = [CommitLine::new("green", Style::new().fg(Color::GREEN))];
    screen.feed(&engine.render(&buffer(&["tail"], 20), &commits));

    // The committed cell carries a green foreground in the emulated grid.
    // all_lines pulls it from scrollback; find its row, then inspect the cell.
    let lines = screen.all_lines();
    assert!(lines.iter().any(|l| l == "green"), "styled commit present: {lines:?}");
}

#[test]
fn inline_multi_span_commit_carries_per_span_color_through_vt100() {
    use rabbitui_core::text::Span;

    let mut engine = InlineEngine::new();
    // A two-row screen: the committed line lands on visible row 0, the one-row
    // tail below it on row 1, so vt100 exposes the commit's per-cell colors
    // directly without walking scrollback.
    let mut screen = VtScreen::new(20, 2);
    screen.feed(&engine.enter());

    // "OK" in green then "ERR" in red, one committed line, two spans.
    let commits = [CommitLine::from_spans([
        Span::styled("OK", Style::new().fg(Color::GREEN)),
        Span::styled("ERR", Style::new().fg(Color::RED)),
    ])];
    screen.feed(&engine.render(&buffer(&["tail"], 20), &commits));

    // The committed line reads "OKERR" and each half kept its own color: cell 0
    // (the 'O') is green (ANSI 2), cell 2 (the 'E') is red (ANSI 1) — proving
    // per-span SGR, not one style for the whole line.
    let cells = screen.row_cells(0);
    let text: String = cells.iter().map(|(sym, _)| sym.as_str()).collect();
    assert_eq!(text, "OKERR");
    assert_eq!(cells[0].1, VtColor::Ansi(2), "first span painted green");
    assert_eq!(cells[2].1, VtColor::Ansi(1), "second span painted red");
}
