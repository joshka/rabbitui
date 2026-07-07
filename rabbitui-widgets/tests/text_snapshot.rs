//! Buffer-snapshot test for the [`Text`] widget.
//!
//! Per `docs/adr/0009-testing.md`, every catalog widget carries a
//! buffer-snapshot test (the escape-level vt100 test joins in slice 5). This
//! drives `Text` through the headless [`TestApp`] and snapshots the rendered
//! multi-line output against `tests/snapshots/`.

use rabbitui_core::geometry::Size;
use rabbitui_core::id::key;
use rabbitui_core::style::{Color, Style};
use rabbitui_testing::{TestApp, assert_snapshot};
use rabbitui_widgets::Text;

#[test]
fn multi_line_text_snapshot() {
    let mut app = TestApp::new(Size::new(20, 4), ());
    app.render(|(), frame| {
        let content = "Hello, rabbitui!\nline two\nline three";
        let text = Text::new(content).style(Style::new().fg(Color::GREEN).bold());
        frame.widget(key("text"), frame.area(), &text);
    });
    assert_snapshot!("text_multi_line", app.buffer_text());
}

#[test]
fn soft_wrap_text_snapshot() {
    // A paragraph and a run of wide graphemes, soft-wrapped to a narrow area, so
    // the snapshot pins word-boundary wrapping and wide-grapheme handling.
    let mut app = TestApp::new(Size::new(12, 6), ());
    app.render(|(), frame| {
        let content =
            "the quick brown fox jumps over\n世界語のテスト";
        let text = Text::new(content).wrap(true);
        frame.widget(key("wrapped"), frame.area(), &text);
    });
    assert_snapshot!("text_soft_wrap", app.buffer_text());
}
