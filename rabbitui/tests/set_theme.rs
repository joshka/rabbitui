//! Runtime theme switching: `TestApp::set_theme` models the facade's
//! `Update::set_theme` applied before the next frame, so a widget re-resolves its
//! roles against the new theme on the following render.

use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::{Position, Size};
use rabbitui_core::id::key;
use rabbitui_core::style::Color;
use rabbitui_core::theme::Theme;
use rabbitui_testing::TestApp;
use rabbitui_widgets::Panel;

/// A borderless panel fills its whole area with the `Surface` role, so cell (1,1)
/// carries the theme's surface background.
fn view(_: &(), frame: &mut Frame<'_>) {
    let area = frame.area();
    frame.widget(key("panel"), area, &Panel::new().border(false));
}

#[test]
fn set_theme_reresolves_role_colors_on_the_next_frame() {
    let mut app = TestApp::new(Size::new(6, 3), ()).with_theme(Theme::dark());
    app.render(view);
    let dark = app.buffer().get(Position::new(1, 1)).unwrap().style.bg;

    app.set_theme(Theme::catppuccin_mocha());
    app.render(view);
    let mocha = app.buffer().get(Position::new(1, 1)).unwrap().style.bg;

    assert_ne!(dark, mocha, "switching theme changes the surface fill");
    assert_eq!(
        mocha,
        Some(Color::Rgb(0x1e, 0x1e, 0x2e)),
        "the catppuccin base color"
    );
}
