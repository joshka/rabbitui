//! Semantic theme roles and the role → [`Style`] map.
//!
//! Per `docs/adr/0007-styling-theming.md`: rabbitui styling has two layers — the
//! typed [`Style`] value everything resolves to, and a fixed vocabulary of
//! semantic *role tokens* that widgets reference instead of hard-coding colors.
//! A [`Theme`] maps each [`Role`] to a concrete [`Style`], resolved
//! framework-side during the paint pass. Widgets ask for a role through
//! [`RenderCtx::style`](crate::widget::RenderCtx::style); they never name a color
//! directly, so re-skinning the whole app is a single theme swap (ADR 0007's
//! "framework-resolved styling, not per-widget charity").
//!
//! The role set is a **closed** enum in v1 — the committed vocabulary the
//! built-in catalog needs. Expanding it later is additive; renaming is breaking
//! (ADR 0007 consequences). Presets ship as `const fn`s returning a fully
//! populated [`Theme`]; [`catppuccin_mocha`] is the first, alongside the
//! restrained dark [`Theme::default`]. Capability degradation (truecolor → 256 →
//! 16) is deferred until the capability probe exists (ADR 0012); a `Theme` stores
//! its [`Style`]s as authored.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::style::Color;
//! use rabbitui_core::theme::{Role, Theme};
//!
//! let theme = Theme::default();
//! // Every role resolves to some style; the accent carries a foreground color.
//! assert!(theme.style(Role::Accent).fg.is_some());
//!
//! // Presets are plain data, swappable wholesale.
//! let mocha = Theme::catppuccin_mocha();
//! assert_eq!(mocha.style(Role::Surface).bg, Some(Color::Rgb(0x1e, 0x1e, 0x2e)));
//! ```

use crate::style::{Color, Style};

/// A semantic style role: what a piece of UI *means*, not what color it is.
///
/// Widgets reference a role and let the active [`Theme`] decide the concrete
/// [`Style`]. The set is closed for v1 (ADR 0007): these nine roles cover the
/// built-in catalog. `Role` is `Copy` and cheap to pass around.
///
/// # Examples
///
/// ```
/// use rabbitui_core::theme::Role;
///
/// // Roles are plain, copyable tokens.
/// let role = Role::Danger;
/// assert_eq!(role, Role::Danger);
/// assert_ne!(Role::Danger, Role::Success);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Role {
    /// The app background — the base surface most content sits on.
    Surface,
    /// Primary foreground text.
    Text,
    /// De-emphasized text: hints, disabled labels, secondary detail.
    Muted,
    /// The accent color: the app's primary brand/interaction hue.
    Accent,
    /// A success state (a completed task, a passing check).
    Success,
    /// A warning state (a recoverable problem, a caution).
    Warning,
    /// A danger/error state (a destructive action, a failure).
    Danger,
    /// A border or separator between regions.
    Border,
    /// The highlight for the focused or selected element.
    Highlight,
}

impl Role {
    /// Every role, in declaration order.
    ///
    /// Useful for building or validating a full theme (a file loader can check
    /// it set — or defaulted — every role).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::theme::Role;
    ///
    /// assert_eq!(Role::ALL.len(), 9);
    /// assert_eq!(Role::ALL[0], Role::Surface);
    /// ```
    pub const ALL: [Role; 9] = [
        Role::Surface,
        Role::Text,
        Role::Muted,
        Role::Accent,
        Role::Success,
        Role::Warning,
        Role::Danger,
        Role::Border,
        Role::Highlight,
    ];

    /// The role's stable lowercase name, as used in theme files (`role = "…"`).
    ///
    /// This is the identifier the facade's file grammar keys on; keeping it here
    /// means the name and the variant never drift.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::theme::Role;
    ///
    /// assert_eq!(Role::Highlight.name(), "highlight");
    /// assert_eq!(Role::from_name("danger"), Some(Role::Danger));
    /// assert_eq!(Role::from_name("nope"), None);
    /// ```
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Role::Surface => "surface",
            Role::Text => "text",
            Role::Muted => "muted",
            Role::Accent => "accent",
            Role::Success => "success",
            Role::Warning => "warning",
            Role::Danger => "danger",
            Role::Border => "border",
            Role::Highlight => "highlight",
        }
    }

    /// The role named by `name`, or `None` if it is not a known role.
    ///
    /// The inverse of [`name`](Self::name); the facade's theme parser uses it to
    /// turn a `[roles]` table key into a [`Role`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|role| role.name() == name)
    }
}

/// A mapping from every [`Role`] to a concrete [`Style`].
///
/// A `Theme` is plain data: an array indexed by role, so lookup is a bounds-free
/// array read. Build one with [`Theme::default`] or a preset, then override
/// individual roles with [`set`](Self::set) (the facade's file loader does
/// exactly this — start from a base theme, apply the roles the file names, leave
/// the rest as the base).
///
/// # Examples
///
/// ```
/// use rabbitui_core::style::{Color, Style};
/// use rabbitui_core::theme::{Role, Theme};
///
/// let mut theme = Theme::default();
/// theme.set(Role::Accent, Style::new().fg(Color::Rgb(0xff, 0x00, 0x88)));
/// assert_eq!(theme.style(Role::Accent).fg, Some(Color::Rgb(0xff, 0x00, 0x88)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    styles: [Style; Role::ALL.len()],
}

impl Theme {
    /// The restrained dark default — an active theme with zero configuration.
    ///
    /// Deliberately understated (ADR 0007's "pretty by default" without shouting):
    /// ANSI-anchored colors so it degrades cleanly on any terminal, with a bright
    /// accent and a reversed-ish highlight. Presets like
    /// [`catppuccin_mocha`](Self::catppuccin_mocha) replace it wholesale.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::theme::{Role, Theme};
    ///
    /// let theme = Theme::default();
    /// assert!(theme.style(Role::Text).fg.is_some());
    /// ```
    #[must_use]
    pub const fn dark() -> Self {
        // ANSI-anchored so it survives on a 16-color terminal with no
        // degradation pass. Muted uses DIM; the highlight is the accent hue used
        // as a background for the focused element.
        let mut styles = [Style::new(); Role::ALL.len()];
        styles[role_index(Role::Surface)] = Style::new().bg(Color::Reset);
        styles[role_index(Role::Text)] = Style::new().fg(Color::Reset);
        styles[role_index(Role::Muted)] = Style::new().fg(Color::Ansi(8)).dim();
        styles[role_index(Role::Accent)] = Style::new().fg(Color::CYAN);
        styles[role_index(Role::Success)] = Style::new().fg(Color::GREEN);
        styles[role_index(Role::Warning)] = Style::new().fg(Color::YELLOW);
        styles[role_index(Role::Danger)] = Style::new().fg(Color::RED);
        styles[role_index(Role::Border)] = Style::new().fg(Color::Ansi(8));
        styles[role_index(Role::Highlight)] =
            Style::new().fg(Color::BLACK).bg(Color::CYAN);
        Self { styles }
    }

    /// The Catppuccin Mocha preset (ADR 0007's "presets in v0.1").
    ///
    /// Truecolor values from the published Catppuccin Mocha palette. Stored as
    /// authored; degradation to 256/16 colors is applied at render time once the
    /// capability probe lands (ADR 0012), not here.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::Color;
    /// use rabbitui_core::theme::{Role, Theme};
    ///
    /// let mocha = Theme::catppuccin_mocha();
    /// // Mauve accent, base surface.
    /// assert_eq!(mocha.style(Role::Accent).fg, Some(Color::Rgb(0xcb, 0xa6, 0xf7)));
    /// ```
    #[must_use]
    pub const fn catppuccin_mocha() -> Self {
        // Palette: base #1e1e2e, text #cdd6f4, overlay1 #7f849c (muted),
        // mauve #cba6f7 (accent), green #a6e3a1, yellow #f9e2af, red #f38ba8,
        // surface2 #585b70 (border), crust #11111b on mauve (highlight).
        const BASE: Color = Color::Rgb(0x1e, 0x1e, 0x2e);
        const TEXT: Color = Color::Rgb(0xcd, 0xd6, 0xf4);
        const OVERLAY1: Color = Color::Rgb(0x7f, 0x84, 0x9c);
        const MAUVE: Color = Color::Rgb(0xcb, 0xa6, 0xf7);
        const GREEN: Color = Color::Rgb(0xa6, 0xe3, 0xa1);
        const YELLOW: Color = Color::Rgb(0xf9, 0xe2, 0xaf);
        const RED: Color = Color::Rgb(0xf3, 0x8b, 0xa8);
        const SURFACE2: Color = Color::Rgb(0x58, 0x5b, 0x70);
        const CRUST: Color = Color::Rgb(0x11, 0x11, 0x1b);

        let mut styles = [Style::new(); Role::ALL.len()];
        styles[role_index(Role::Surface)] = Style::new().fg(TEXT).bg(BASE);
        styles[role_index(Role::Text)] = Style::new().fg(TEXT);
        styles[role_index(Role::Muted)] = Style::new().fg(OVERLAY1);
        styles[role_index(Role::Accent)] = Style::new().fg(MAUVE);
        styles[role_index(Role::Success)] = Style::new().fg(GREEN);
        styles[role_index(Role::Warning)] = Style::new().fg(YELLOW);
        styles[role_index(Role::Danger)] = Style::new().fg(RED);
        styles[role_index(Role::Border)] = Style::new().fg(SURFACE2);
        styles[role_index(Role::Highlight)] = Style::new().fg(CRUST).bg(MAUVE);
        Self { styles }
    }

    /// The concrete [`Style`] this theme maps `role` to.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::theme::{Role, Theme};
    ///
    /// let theme = Theme::catppuccin_mocha();
    /// let danger = theme.style(Role::Danger);
    /// assert!(danger.fg.is_some());
    /// ```
    #[must_use]
    pub const fn style(&self, role: Role) -> Style {
        self.styles[role_index(role)]
    }

    /// Overrides the style for a single `role`, leaving the rest unchanged.
    ///
    /// This is the partial-override primitive (ADR 0007's Brick-style partial
    /// merge): a theme file names only the roles it cares about, and the loader
    /// applies each over a base theme with this.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::style::{Color, Style};
    /// use rabbitui_core::theme::{Role, Theme};
    ///
    /// let mut theme = Theme::default();
    /// theme.set(Role::Warning, Style::new().fg(Color::Rgb(0xff, 0xaa, 0x00)).bold());
    /// assert!(theme.style(Role::Warning).attrs.contains(rabbitui_core::style::Attrs::BOLD));
    /// ```
    pub const fn set(&mut self, role: Role, style: Style) {
        self.styles[role_index(role)] = style;
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// The array index for a role — its position in [`Role::ALL`].
///
/// A `const fn` so [`Theme`]'s preset constructors and [`Theme::style`] are
/// `const`; the match is exhaustive, so it never panics.
const fn role_index(role: Role) -> usize {
    match role {
        Role::Surface => 0,
        Role::Text => 1,
        Role::Muted => 2,
        Role::Accent => 3,
        Role::Success => 4,
        Role::Warning => 5,
        Role::Danger => 6,
        Role::Border => 7,
        Role::Highlight => 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Attrs;

    #[test]
    fn role_index_matches_all_ordering() {
        for (index, role) in Role::ALL.into_iter().enumerate() {
            assert_eq!(role_index(role), index);
        }
    }

    #[test]
    fn name_round_trips_through_from_name() {
        for role in Role::ALL {
            assert_eq!(Role::from_name(role.name()), Some(role));
        }
        assert_eq!(Role::from_name("not-a-role"), None);
    }

    #[test]
    fn default_populates_every_role() {
        let theme = Theme::default();
        // No role is the wholly-empty style: default resolves each to something.
        for role in Role::ALL {
            let style = theme.style(role);
            assert!(
                style != Style::new() || role == Role::Surface,
                "role {role:?} resolved to an empty style",
            );
        }
    }

    #[test]
    fn set_overrides_only_the_named_role() {
        let mut theme = Theme::default();
        let before_text = theme.style(Role::Text);
        theme.set(Role::Accent, Style::new().fg(Color::RED).bold());
        assert_eq!(theme.style(Role::Accent).fg, Some(Color::RED));
        assert!(theme.style(Role::Accent).attrs.contains(Attrs::BOLD));
        // A different role is untouched.
        assert_eq!(theme.style(Role::Text), before_text);
    }

    #[test]
    fn catppuccin_mocha_uses_truecolor() {
        let mocha = Theme::catppuccin_mocha();
        assert_eq!(mocha.style(Role::Surface).bg, Some(Color::Rgb(0x1e, 0x1e, 0x2e)));
        assert_eq!(mocha.style(Role::Accent).fg, Some(Color::Rgb(0xcb, 0xa6, 0xf7)));
    }

    #[test]
    fn presets_differ_from_default() {
        assert_ne!(Theme::default(), Theme::catppuccin_mocha());
    }
}
