//! The widget gallery: every catalog widget and every role, on one scrollable
//! screen — a living style guide that doubles as a visual regression.
//!
//! The theme is chosen at startup from the `GALLERY_THEME` environment variable
//! (`dark` (default), `catppuccin`, `nord`, `dracula`), so the betamax tape set
//! renders the same layout under each theme with no runtime theme-switch API. A
//! runtime `Update::set_theme` would let number keys switch live; that is a
//! deferred framework item (Arc 4), not needed for the regression tapes.
//!
//! The body is a virtualized [`ScrollView`](rabbitui_core::scroll) column: each
//! showcase is one scroll item, measured at its own height, so the gallery both
//! *shows* the scroll widget and *is* the scroll widget's visual test. Focus
//! starts on the scroll; Tab cycles it through the focusable widgets (buttons,
//! input, list, collapsibles). `q` / Ctrl-C quit.
//!
//! Run with `cargo run --example gallery`, or e.g.
//! `GALLERY_THEME=nord cargo run --example gallery`.

use std::ops::ControlFlow;

use rabbitui::App;
use rabbitui::app::{Config, Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::scroll::ScrollScope;
use rabbitui_core::spacing;
use rabbitui_core::style::Style;
use rabbitui_core::text::Span;
use rabbitui_core::theme::{Role, Theme};
use rabbitui_widgets::{Button, Collapsible, Panel, SelectionList, Text, TextInput};

/// The list shown in the `SelectionList` showcase.
const OPTIONS: [&str; 4] = [
    "First option",
    "Second option",
    "Third option",
    "Fourth option",
];

/// A paragraph long enough to demonstrate soft wrap.
const PARAGRAPH: &str = "This paragraph is wrapped to the panel width with grapheme-correct soft \
wrap: words break at boundaries, an overlong token hard-breaks, and wide graphemes never split.";

/// The gallery's tiny state: just the theme name, for the header and footer.
struct Gallery {
    /// The active theme's name, shown in the chrome.
    theme_name: &'static str,
}

/// Resolves the startup theme from `GALLERY_THEME`.
fn theme_from_env() -> (Theme, &'static str) {
    match std::env::var("GALLERY_THEME").as_deref() {
        Ok("catppuccin" | "mocha") => (Theme::catppuccin_mocha(), "catppuccin_mocha"),
        Ok("nord") => (Theme::nord(), "nord"),
        Ok("dracula") => (Theme::dracula(), "dracula"),
        _ => (Theme::default(), "dark"),
    }
}

impl App for Gallery {
    /// Number keys 1–4 switch theme live; `q` / Ctrl-C quit. Tab traversal is a
    /// framework default. All guarded by `consumed()`, so a digit typed into the
    /// focused input edits it rather than switching the theme.
    fn update(&mut self, update: Update<'_>) -> ControlFlow<()> {
        if let Event::Input(input) = update.event()
            && !update.consumed()
            && let Some(press) = input.as_key()
        {
            match press.key {
                Key::Char('1') => switch(self, &update, Theme::dark(), "dark"),
                Key::Char('2') => {
                    switch(self, &update, Theme::catppuccin_mocha(), "catppuccin_mocha")
                }
                Key::Char('3') => switch(self, &update, Theme::nord(), "nord"),
                Key::Char('4') => switch(self, &update, Theme::dracula(), "dracula"),
                Key::Char('q') if !press.modifiers.ctrl => return ControlFlow::Break(()),
                Key::Char('c') if press.modifiers.ctrl => return ControlFlow::Break(()),
                _ => {}
            }
        }
        ControlFlow::Continue(())
    }

    /// The whole gallery: a titled panel wrapping the scroll, plus a footer.
    fn view(&self, frame: &mut Frame<'_>) {
        let [body, footer] = frame.rows([Constraint::Fill(1), Constraint::Length(1)]);

        let title = format!("rabbitui gallery · {}", self.theme_name);
        let panel = Panel::new()
            .title(&title)
            .padding(spacing::PANEL_PADDING)
            .focused(true);
        frame.widget(key("panel"), body, &panel);
        let inner = Panel::inner(body, &panel);
        frame.scroll(key("scroll"), inner, showcase);

        let footer_hint = "1–4: theme   Tab: focus   ↑/↓/PgUp/PgDn: scroll   q: quit";
        frame.widget(
            key("footer"),
            footer,
            &Text::new(footer_hint).role(Role::Muted),
        );
    }

    fn config(&self) -> Config {
        let (theme, _) = theme_from_env();
        Config::new().theme(theme)
    }
}

/// Applies a live theme switch and records its name for the chrome.
fn switch(app: &mut Gallery, update: &Update<'_>, theme: Theme, name: &'static str) {
    update.set_theme(theme);
    app.theme_name = name;
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (_, theme_name) = theme_from_env();
    Gallery { theme_name }.run().await?;
    Ok(())
}

/// Declares every showcase item into the scroll column.
fn showcase(scroll: &mut ScrollScope<'_, '_>) {
    section(scroll, "text", "Text");
    scroll.item(
        key("t-body"),
        &Text::new("Primary body text (Role::Text).").role(Role::Text),
    );
    scroll.item(
        key("t-muted"),
        &Text::new("Muted secondary text.").role(Role::Muted),
    );
    scroll.item(
        key("t-accent"),
        &Text::new("Accented text.").role(Role::Accent),
    );
    scroll.item(
        key("t-spans"),
        &Text::new(vec![
            Span::raw("Styled spans: "),
            Span::styled("bold", Style::new().bold()),
            Span::raw(", "),
            Span::styled("italic", Style::new().italic()),
            Span::raw(", "),
            Span::styled("underline", Style::new().underline()),
            Span::raw("."),
        ]),
    );
    scroll.item(
        key("t-wrap"),
        &Text::new(PARAGRAPH).wrap(true).role(Role::Text),
    );

    section(scroll, "buttons", "Button");
    scroll.item(key("b-primary"), &Button::new("Primary action"));
    scroll.item(key("b-secondary"), &Button::new("Secondary action"));

    section(scroll, "input", "TextInput");
    scroll.item(
        key("input"),
        &TextInput::new().placeholder("Type here, then Tab away…"),
    );

    section(scroll, "list", "SelectionList");
    scroll.item(key("list"), &SelectionList::new(&OPTIONS[..]));

    section(scroll, "collapsible", "Collapsible");
    scroll.item(
        key("c-open"),
        &Collapsible::new(
            "Expanded section",
            "Its body is visible until you collapse it.",
        )
        .default_collapsed(false),
    );
    scroll.item(
        key("c-closed"),
        &Collapsible::new(
            "Collapsed section",
            "Hidden until you expand it with Enter or a click.",
        )
        .default_collapsed(true),
    );

    section(scroll, "roles", "Roles");
    for (index, role) in Role::ALL.into_iter().enumerate() {
        scroll.item(
            key("role").index(index),
            &Text::new(format!(" {} ", role.name())).role(role),
        );
    }
}

/// A section header: a blank spacer then a muted label.
fn section(scroll: &mut ScrollScope<'_, '_>, id: &str, label: &str) {
    scroll.item(
        key(&format!("gap-{id}")),
        &Text::new(" ".repeat(usize::from(spacing::GAP))),
    );
    scroll.item(
        key(&format!("label-{id}")),
        &Text::new(format!("— {label} —")).role(Role::Muted),
    );
}
