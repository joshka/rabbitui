//! Loading [`Theme`]s from TOML files (behind the `themes` feature).
//!
//! Per `docs/adr/0007-styling-theming.md`, file loading and hot reload live in
//! the **facade**, not core: core stays dependency-free while the facade takes a
//! `toml` dependency to parse theme files. A theme file is a `[roles]` table
//! mapping each [`Role`] name to a small style string:
//!
//! ```toml
//! [roles]
//! surface   = "#cdd6f4 on #1e1e2e"
//! text      = "#cdd6f4"
//! accent    = "#cba6f7, bold"
//! danger    = "#f38ba8, bold underline"
//! highlight = "#11111b on #cba6f7"
//! ```
//!
//! # The grammar
//!
//! Each value is `FG [on BG] [, ATTR...]`:
//!
//! - `FG` and `BG` are `#rrggbb` hex colors (six hex digits, leading `#`).
//! - `on BG` is optional; without it the role sets only a foreground.
//! - after a comma, a space-separated list of attributes: `bold`, `dim`,
//!   `italic`, `underline`, `reversed`, `strikethrough`.
//!
//! Roles a file omits keep the base theme's style (partial override, ADR 0007),
//! and the load starts from a supplied base ([`Theme::default`] or a preset).
//! Any parse problem is returned as a [`ThemeError`] — **never a panic** — naming
//! the file, the offending role, and what was wrong.
//!
//! # Examples
//!
//! ```
//! use rabbitui::theme::parse_theme;
//! use rabbitui_core::style::{Attrs, Color};
//! use rabbitui_core::theme::{Role, Theme};
//!
//! let toml = r##"
//!     [roles]
//!     accent = "#cba6f7, bold"
//! "##;
//! let theme = parse_theme(toml, Theme::default()).unwrap();
//! assert_eq!(theme.style(Role::Accent).fg, Some(Color::Rgb(0xcb, 0xa6, 0xf7)));
//! assert!(theme.style(Role::Accent).attrs.contains(Attrs::BOLD));
//! ```

use std::fmt;
use std::path::{Path, PathBuf};

use rabbitui_core::style::{Attrs, Color, Style};
use rabbitui_core::theme::{Role, Theme};

/// What went wrong loading or parsing a theme.
///
/// Returned rather than panicked (ADR 0007 / slice-4 design note: "parse errors
/// returned as a proper error type, never panic"). Every variant names enough to
/// find and fix the problem.
#[derive(Debug)]
#[non_exhaustive]
pub enum ThemeError {
    /// Reading the theme file failed (missing, unreadable).
    Io {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The file was not valid TOML.
    Toml(toml::de::Error),
    /// The `[roles]` table was missing entirely.
    MissingRolesTable,
    /// A `[roles]` key is not a known role name.
    UnknownRole(String),
    /// A role's value was not a string (e.g. a number or table).
    NotAString(String),
    /// A role's style string could not be parsed.
    BadStyle {
        /// The role whose value was malformed.
        role: String,
        /// A human-readable reason.
        reason: String,
    },
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeError::Io { path, source } => {
                write!(f, "reading theme file {}: {source}", path.display())
            }
            ThemeError::Toml(error) => write!(f, "invalid theme TOML: {error}"),
            ThemeError::MissingRolesTable => {
                write!(f, "theme file has no [roles] table")
            }
            ThemeError::UnknownRole(name) => {
                write!(f, "unknown role {name:?} (expected one of the documented roles)")
            }
            ThemeError::NotAString(name) => {
                write!(f, "role {name:?} must be a style string like \"#rrggbb on #rrggbb, bold\"")
            }
            ThemeError::BadStyle { role, reason } => {
                write!(f, "role {role:?}: {reason}")
            }
        }
    }
}

impl std::error::Error for ThemeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ThemeError::Io { source, .. } => Some(source),
            ThemeError::Toml(error) => Some(error),
            _ => None,
        }
    }
}

/// Loads a theme file at `path`, layering its roles over `base`.
///
/// Reads the file, then defers to [`parse_theme`]. Roles the file omits keep
/// `base`'s style. Errors are returned, not panicked.
///
/// # Errors
///
/// Returns [`ThemeError::Io`] if the file cannot be read, [`ThemeError::Toml`]
/// for malformed TOML, or the parse errors [`parse_theme`] reports.
pub fn load_theme(path: impl AsRef<Path>, base: Theme) -> Result<Theme, ThemeError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path)
        .map_err(|source| ThemeError::Io { path: path.to_path_buf(), source })?;
    parse_theme(&text, base)
}

/// Parses a theme from TOML `text`, layering its `[roles]` over `base`.
///
/// The pure, I/O-free half of [`load_theme`], exposed for testing the grammar
/// directly. Every role the table names is validated and applied over `base`;
/// omitted roles keep `base`'s style.
///
/// # Errors
///
/// Returns a [`ThemeError`] for malformed TOML, a missing `[roles]` table, an
/// unknown role name, a non-string value, or a malformed style string.
pub fn parse_theme(text: &str, base: Theme) -> Result<Theme, ThemeError> {
    let document: toml::Table = text.parse().map_err(ThemeError::Toml)?;
    let roles = document
        .get("roles")
        .ok_or(ThemeError::MissingRolesTable)?
        .as_table()
        .ok_or(ThemeError::MissingRolesTable)?;

    let mut theme = base;
    for (name, value) in roles {
        let role = Role::from_name(name).ok_or_else(|| ThemeError::UnknownRole(name.clone()))?;
        let spec = value.as_str().ok_or_else(|| ThemeError::NotAString(name.clone()))?;
        let style = parse_style(spec)
            .map_err(|reason| ThemeError::BadStyle { role: name.clone(), reason })?;
        theme.set(role, style);
    }
    Ok(theme)
}

/// Parses one style string (`FG [on BG] [, ATTR...]`) into a [`Style`].
///
/// The grammar's core, split out so both the loader and its tests exercise the
/// same code. Returns a human-readable reason on failure.
fn parse_style(spec: &str) -> Result<Style, String> {
    // Split colors from attributes at the first comma.
    let (colors, attr_part) = match spec.split_once(',') {
        Some((colors, attrs)) => (colors.trim(), Some(attrs.trim())),
        None => (spec.trim(), None),
    };

    if colors.is_empty() {
        return Err("empty style (expected at least a \"#rrggbb\" foreground)".to_string());
    }

    let mut style = Style::new();
    // Colors: `FG` or `FG on BG`.
    let mut words = colors.split_whitespace();
    let fg = words.next().ok_or_else(|| "missing foreground color".to_string())?;
    style = style.fg(parse_color(fg)?);
    if let Some(word) = words.next() {
        if word != "on" {
            return Err(format!("expected \"on\" before a background color, found {word:?}"));
        }
        let bg = words
            .next()
            .ok_or_else(|| "\"on\" must be followed by a background color".to_string())?;
        style = style.bg(parse_color(bg)?);
        if let Some(extra) = words.next() {
            return Err(format!("unexpected trailing text after the background color: {extra:?}"));
        }
    }

    if let Some(attrs) = attr_part {
        for attr in attrs.split_whitespace() {
            style = apply_attr(style, attr)?;
        }
    }
    Ok(style)
}

/// Parses a `#rrggbb` hex color.
fn parse_color(word: &str) -> Result<Color, String> {
    let hex = word
        .strip_prefix('#')
        .ok_or_else(|| format!("color {word:?} must start with '#'"))?;
    if hex.len() != 6 {
        return Err(format!("color {word:?} must be #rrggbb (six hex digits)"));
    }
    let component = |range: std::ops::Range<usize>| {
        u8::from_str_radix(&hex[range], 16)
            .map_err(|_| format!("color {word:?} has non-hex digits"))
    };
    Ok(Color::Rgb(component(0..2)?, component(2..4)?, component(4..6)?))
}

/// Adds one named attribute to `style`.
fn apply_attr(style: Style, attr: &str) -> Result<Style, String> {
    Ok(match attr {
        "bold" => style.bold(),
        "dim" => style.dim(),
        "italic" => style.italic(),
        "underline" => style.underline(),
        "reversed" => style.reversed(),
        "strikethrough" => add_strikethrough(style),
        other => {
            return Err(format!(
                "unknown attribute {other:?} (expected bold, dim, italic, underline, \
                 reversed, or strikethrough)"
            ));
        }
    })
}

/// Adds strikethrough. [`Style`] has no `strikethrough` builder, so fold the
/// [`Attrs::STRIKETHROUGH`] bit in through the public attrs field.
fn add_strikethrough(mut style: Style) -> Style {
    style.attrs |= Attrs::STRIKETHROUGH;
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fg_only() {
        let style = parse_style("#cba6f7").unwrap();
        assert_eq!(style.fg, Some(Color::Rgb(0xcb, 0xa6, 0xf7)));
        assert_eq!(style.bg, None);
        assert!(style.attrs.is_empty());
    }

    #[test]
    fn parses_fg_on_bg() {
        let style = parse_style("#11111b on #cba6f7").unwrap();
        assert_eq!(style.fg, Some(Color::Rgb(0x11, 0x11, 0x1b)));
        assert_eq!(style.bg, Some(Color::Rgb(0xcb, 0xa6, 0xf7)));
    }

    #[test]
    fn parses_attributes() {
        let style = parse_style("#ffffff, bold underline").unwrap();
        assert!(style.attrs.contains(Attrs::BOLD | Attrs::UNDERLINE));
    }

    #[test]
    fn parses_fg_bg_and_attrs_together() {
        let style = parse_style("#000000 on #ffffff, bold reversed").unwrap();
        assert_eq!(style.fg, Some(Color::Rgb(0, 0, 0)));
        assert_eq!(style.bg, Some(Color::Rgb(0xff, 0xff, 0xff)));
        assert!(style.attrs.contains(Attrs::BOLD | Attrs::REVERSED));
    }

    #[test]
    fn all_attributes_are_recognized() {
        let style =
            parse_style("#ffffff, bold dim italic underline reversed strikethrough").unwrap();
        for attr in [
            Attrs::BOLD,
            Attrs::DIM,
            Attrs::ITALIC,
            Attrs::UNDERLINE,
            Attrs::REVERSED,
            Attrs::STRIKETHROUGH,
        ] {
            assert!(style.attrs.contains(attr));
        }
    }

    #[test]
    fn rejects_missing_hash() {
        let error = parse_style("cba6f7").unwrap_err();
        assert!(error.contains("must start with '#'"), "{error}");
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(parse_style("#abc").unwrap_err().contains("six hex digits"));
    }

    #[test]
    fn rejects_non_hex() {
        assert!(parse_style("#gggggg").unwrap_err().contains("non-hex"));
    }

    #[test]
    fn rejects_unknown_attribute() {
        let error = parse_style("#ffffff, sparkly").unwrap_err();
        assert!(error.contains("unknown attribute"), "{error}");
    }

    #[test]
    fn rejects_missing_on_keyword() {
        let error = parse_style("#ffffff #000000").unwrap_err();
        assert!(error.contains("expected \"on\""), "{error}");
    }

    #[test]
    fn parse_theme_layers_over_base() {
        let base = Theme::default();
        let before_text = base.style(Role::Text);
        let toml = "[roles]\naccent = \"#cba6f7, bold\"\n";
        let theme = parse_theme(toml, base).unwrap();
        // The named role is overridden…
        assert_eq!(theme.style(Role::Accent).fg, Some(Color::Rgb(0xcb, 0xa6, 0xf7)));
        // …and an unnamed role keeps the base.
        assert_eq!(theme.style(Role::Text), before_text);
    }

    #[test]
    fn parse_theme_reports_unknown_role() {
        let error = parse_theme("[roles]\nbogus = \"#ffffff\"\n", Theme::default()).unwrap_err();
        assert!(matches!(error, ThemeError::UnknownRole(ref name) if name == "bogus"));
        assert!(error.to_string().contains("unknown role"));
    }

    #[test]
    fn parse_theme_reports_missing_roles_table() {
        let error = parse_theme("[other]\nx = 1\n", Theme::default()).unwrap_err();
        assert!(matches!(error, ThemeError::MissingRolesTable));
    }

    #[test]
    fn parse_theme_reports_bad_style_with_role_name() {
        let error =
            parse_theme("[roles]\ndanger = \"not-a-color\"\n", Theme::default()).unwrap_err();
        match error {
            ThemeError::BadStyle { ref role, .. } => assert_eq!(role, "danger"),
            other => panic!("expected BadStyle, got {other:?}"),
        }
        assert!(error.to_string().contains("danger"));
    }

    #[test]
    fn parse_theme_reports_non_string_value() {
        let error = parse_theme("[roles]\naccent = 42\n", Theme::default()).unwrap_err();
        assert!(matches!(error, ThemeError::NotAString(ref n) if n == "accent"));
    }

    #[test]
    fn invalid_toml_is_reported_not_panicked() {
        let error = parse_theme("this is not = = toml", Theme::default()).unwrap_err();
        assert!(matches!(error, ThemeError::Toml(_)));
    }
}
