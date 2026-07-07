//! Themed buffer snapshots for the interactive widgets, focused and unfocused.
//!
//! Per `docs/adr/0009-testing.md`, every catalog widget carries a buffer-snapshot
//! test. Slice 4 adds [`TextInput`] and [`SelectionList`]; these drive each
//! through the headless [`TestApp`] under the Catppuccin Mocha theme and snapshot
//! both the rendered glyphs and a compact per-cell style legend, focused and
//! unfocused, so a change in *appearance* (not just text) is caught.

use rabbitui_core::buffer::Buffer;
use rabbitui_core::geometry::{Position, Size};
use rabbitui_core::id::{WidgetId, key};
use rabbitui_core::style::{Attrs, Color};
use rabbitui_core::theme::Theme;
use rabbitui_testing::{TestApp, assert_snapshot};
use rabbitui_widgets::{Collapsible, Panel, SelectionList, Text, TextInput};

/// Renders `buffer` as glyph rows followed by a legend: for each non-blank cell,
/// its position and a terse style description. This makes a themed snapshot
/// sensitive to color and attributes, not only text.
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

fn text_input_id() -> WidgetId {
    WidgetId::ROOT.child(key("input"))
}

fn list_id() -> WidgetId {
    WidgetId::ROOT.child(key("list"))
}

#[test]
fn text_input_focused_snapshot() {
    let mut app = TestApp::new(Size::new(12, 1), ()).with_theme(Theme::catppuccin_mocha());
    // Render once so the widget is declared, then focus it and type.
    let view = |_s: &(), frame: &mut rabbitui_core::frame::Frame<'_>| {
        frame.widget(
            key("input"),
            frame.area(),
            &TextInput::new().placeholder("search"),
        );
    };
    app.render(view);
    app.set_focus(Some(text_input_id()));
    app.render(view);
    app.send_key(rabbitui_core::input::Key::Char('h'));
    app.send_key(rabbitui_core::input::Key::Char('i'));
    app.render(view);
    assert_snapshot!("text_input_focused", styled_snapshot(app.buffer()));
}

#[test]
fn text_input_unfocused_placeholder_snapshot() {
    let mut app = TestApp::new(Size::new(12, 1), ()).with_theme(Theme::catppuccin_mocha());
    app.render(|_s, frame| {
        frame.widget(
            key("input"),
            frame.area(),
            &TextInput::new().placeholder("search"),
        );
    });
    assert_snapshot!("text_input_unfocused", styled_snapshot(app.buffer()));
}

#[test]
fn selection_list_focused_snapshot() {
    let items = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
    let mut app = TestApp::new(Size::new(8, 3), items).with_theme(Theme::catppuccin_mocha());
    let view = |items: &Vec<String>, frame: &mut rabbitui_core::frame::Frame<'_>| {
        frame.widget(
            key("list"),
            frame.area(),
            &SelectionList::new(items.clone()),
        );
    };
    app.render(view);
    app.set_focus(Some(list_id()));
    app.render(view);
    // Move selection down one so a non-default row is highlighted.
    app.send_key(rabbitui_core::input::Key::Down);
    app.render(view);
    assert_snapshot!("selection_list_focused", styled_snapshot(app.buffer()));
}

#[test]
fn selection_list_unfocused_snapshot() {
    let items = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
    let mut app = TestApp::new(Size::new(8, 3), items).with_theme(Theme::catppuccin_mocha());
    app.render(|items, frame| {
        frame.widget(
            key("list"),
            frame.area(),
            &SelectionList::new(items.clone()),
        );
    });
    assert_snapshot!("selection_list_unfocused", styled_snapshot(app.buffer()));
}

fn collapsible_id() -> WidgetId {
    WidgetId::ROOT.child(key("cell"))
}

/// A tool-style collapsible, default-collapsed: the snapshot pins the collapsed
/// header (marker + summary, no body).
#[test]
fn collapsible_collapsed_snapshot() {
    let mut app = TestApp::new(Size::new(24, 4), ()).with_theme(Theme::catppuccin_mocha());
    app.render(|_s, frame| {
        frame.widget(
            key("cell"),
            frame.area(),
            &Collapsible::new("ran cargo test — 396 passed", "…full output…")
                .default_collapsed(true),
        );
    });
    assert_snapshot!("collapsible_collapsed", styled_snapshot(app.buffer()));
}

/// The same cell, toggled open by Enter while focused: the snapshot pins the
/// expanded marker, the highlighted focused header, and the muted body — and that
/// the toggle survived by identity across re-renders.
#[test]
fn collapsible_expanded_focused_snapshot() {
    let mut app = TestApp::new(Size::new(24, 4), ()).with_theme(Theme::catppuccin_mocha());
    let view = |_s: &(), frame: &mut rabbitui_core::frame::Frame<'_>| {
        frame.widget(
            key("cell"),
            frame.area(),
            &Collapsible::new("ran cargo test — 396 passed", "test one\ntest two")
                .default_collapsed(true),
        );
    };
    app.render(view);
    app.set_focus(Some(collapsible_id()));
    app.render(view);
    // Enter toggles the default-collapsed cell open; the state is retained by id.
    app.send_key(rabbitui_core::input::Key::Enter);
    app.render(view);
    assert_snapshot!(
        "collapsible_expanded_focused",
        styled_snapshot(app.buffer())
    );
}

/// A titled, padded, focused [`Panel`] with content declared into its inner area
/// — the pre-composition backdrop pattern. The snapshot pins the box-drawing
/// frame, the title in the top border, the surface fill on every cell, and the
/// focused-highlight border role, all resolved through the theme.
#[test]
fn panel_titled_focused_snapshot() {
    let mut app = TestApp::new(Size::new(20, 6), ()).with_theme(Theme::catppuccin_mocha());
    app.render(|_s, frame| {
        let area = frame.area();
        let panel = Panel::new().title("Settings").padding(1).focused(true);
        frame.widget(key("panel"), area, &panel);
        // Content declared into the computed inner area (the backdrop pattern).
        let inner = Panel::inner(area, &panel);
        frame.widget(key("body"), inner, &Text::new("name: rabbitui"));
    });
    assert_snapshot!("panel_titled_focused", styled_snapshot(app.buffer()));
}

/// A borderless [`Panel`] — a bare wash of the surface role with padding but no
/// frame. The snapshot pins that no box-drawing glyphs appear and the fill still
/// covers every cell.
#[test]
fn panel_borderless_snapshot() {
    let mut app = TestApp::new(Size::new(16, 3), ()).with_theme(Theme::catppuccin_mocha());
    app.render(|_s, frame| {
        let area = frame.area();
        let panel = Panel::new().border(false).padding(1);
        frame.widget(key("panel"), area, &panel);
        let inner = Panel::inner(area, &panel);
        frame.widget(key("body"), inner, &Text::new("borderless"));
    });
    assert_snapshot!("panel_borderless", styled_snapshot(app.buffer()));
}
