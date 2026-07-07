//! End-to-end snapshots of ratatui widgets bridged into a rabbitui frame.
//!
//! Per `docs/adr/0009-testing.md`, output correctness is proven by driving the
//! real path. These tests declare ratatui widgets through [`RatatuiWidget`] into
//! a headless [`TestApp`] — the same [`StateStore`]/[`Frame`] path the runtime
//! uses — and snapshot both the glyphs and a per-cell style legend, so a change
//! in the bridged *appearance* (border glyphs, gauge fill, converted colors) is
//! caught, not just its text.
//!
//! [`StateStore`]: rabbitui_core::store::StateStore
//! [`Frame`]: rabbitui_core::frame::Frame

use rabbitui_core::buffer::Buffer;
use rabbitui_core::geometry::{Position, Size};
use rabbitui_core::id::key;
use rabbitui_core::style::{Attrs, Color};
use rabbitui_ratatui::RatatuiWidget;
use rabbitui_testing::{TestApp, assert_snapshot};
use ratatui::style::{Color as RatColor, Modifier, Style as RatStyle};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, Paragraph};

/// Renders `buffer` as glyph rows followed by a legend: for each non-blank cell,
/// its position and a terse style description — the same shape the widgets
/// crate's snapshots use, so a bridged widget reads like a native one.
fn styled_snapshot(buffer: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buffer.size().height {
        for x in 0..buffer.size().width {
            let cell = buffer.get(Position::new(x, y)).unwrap();
            out.push_str(if cell.symbol.is_empty() {
                " "
            } else {
                &cell.symbol
            });
        }
        out.push('\n');
    }
    out.push_str("---\n");
    for y in 0..buffer.size().height {
        for x in 0..buffer.size().width {
            let cell = buffer.get(Position::new(x, y)).unwrap();
            if cell.symbol == " " && cell.style == Default::default() {
                continue;
            }
            out.push_str(&format!(
                "({x},{y}) {:?} = {}\n",
                cell.symbol,
                describe(&cell.style)
            ));
        }
    }
    out
}

/// A terse, stable description of a style: fg/bg colors and any attributes.
fn describe(style: &rabbitui_core::style::Style) -> String {
    let mut parts = Vec::new();
    if let Some(color) = style.fg {
        parts.push(format!("fg={}", color_name(color)));
    }
    if let Some(color) = style.bg {
        parts.push(format!("bg={}", color_name(color)));
    }
    for (attr, name) in [
        (Attrs::BOLD, "bold"),
        (Attrs::DIM, "dim"),
        (Attrs::ITALIC, "italic"),
        (Attrs::UNDERLINE, "underline"),
        (Attrs::REVERSED, "reversed"),
        (Attrs::STRIKETHROUGH, "strikethrough"),
    ] {
        if style.attrs.contains(attr) {
            parts.push(name.to_string());
        }
    }
    if parts.is_empty() {
        "default".to_string()
    } else {
        parts.join(" ")
    }
}

fn color_name(color: Color) -> String {
    match color {
        Color::Reset => "reset".to_string(),
        Color::Ansi(n) => format!("ansi{n}"),
        Color::Indexed(n) => format!("idx{n}"),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
    }
}

/// A bordered `Block` with a styled `Paragraph` inside, bridged through the
/// adapter: the snapshot pins the border glyphs and the converted styles that
/// crossed the bridge.
#[test]
fn bordered_block_through_the_bridge() {
    let mut app = TestApp::new(Size::new(20, 4), ());
    app.render(|_s, frame| {
        let panel = Paragraph::new(Line::from(vec![
            Span::raw("hi "),
            Span::styled(
                "cyan",
                RatStyle::default()
                    .fg(RatColor::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .block(Block::bordered().title("panel"));
        frame.widget(key("panel"), frame.area(), &RatatuiWidget::new(panel));
    });
    assert_snapshot!("bordered_block", styled_snapshot(app.buffer()));
}

/// A ratatui `Gauge` bridged through the adapter, driven by app state: the
/// snapshot pins the gauge fill and its converted colors.
#[test]
fn gauge_through_the_bridge() {
    let mut app = TestApp::new(Size::new(16, 3), 60u16);
    app.render(|percent, frame| {
        let gauge = Gauge::default()
            .block(Block::bordered())
            .gauge_style(RatStyle::default().fg(RatColor::Green))
            .percent(*percent);
        frame.widget(key("gauge"), frame.area(), &RatatuiWidget::new(gauge));
    });
    assert_snapshot!("gauge", styled_snapshot(app.buffer()));
}

/// The adapter drives cleanly through the TestApp render/re-render cycle, and
/// its content re-renders from fresh app state each frame (specs are rebuilt
/// every frame, ADR 0001) — so a state change moves the bridged gauge.
#[test]
fn adapter_rerenders_from_state_through_testapp() {
    let mut app = TestApp::new(Size::new(10, 1), 0u16);
    let view = |percent: &u16, frame: &mut rabbitui_core::frame::Frame<'_>| {
        // A 10-wide gauge with no block: the filled run is drawn with the block
        // glyph "█", and its length tracks the percentage — the clean signal
        // that the bridged widget re-rendered from fresh app state.
        let gauge = Gauge::default()
            .gauge_style(RatStyle::default().fg(RatColor::Blue))
            .percent(*percent);
        frame.widget(key("g"), frame.area(), &RatatuiWidget::new(gauge));
    };
    // Count filled ("█") cells in the row.
    let filled = |buffer: &Buffer| {
        (0..buffer.size().width)
            .filter(|&x| buffer.get(Position::new(x, 0)).unwrap().symbol == "█")
            .count()
    };
    app.render(view);
    // 0%: the fill covers no full cells.
    assert_eq!(
        filled(app.buffer()),
        0,
        "0% gauge should have no filled cells"
    );

    // Fold state to 100% and re-render: the fill now covers cells the empty
    // gauge did not (the centered "100%" label sits over the middle, so not
    // every cell is a block glyph — the point is the bridged widget re-rendered
    // from the new state, growing the fill).
    app.send(|percent| *percent = 100, view);
    assert!(
        filled(app.buffer()) > 0,
        "100% gauge should show a fill the 0% gauge did not"
    );
}
