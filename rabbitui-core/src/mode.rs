//! Screen modes: the two peer render targets (ADR 0013).
//!
//! rabbitui renders into one of two screen regions over the *same* widget tree
//! (`docs/adr/0013-screen-modes.md`): the alternate screen — a dedicated
//! full-screen buffer the terminal discards on exit, the classic full-app model
//! — or **inline**, a bounded live tail at the bottom of the primary screen plus
//! an append-once commit channel into native scrollback above it. Neither is
//! privileged; both are declared the same way and selectable at startup
//! (`App::mode`) and switchable at runtime (`Update::set_mode`).
//!
//! This type is deliberately dep-free and lives in core so the facade's engines,
//! the runtime, and the test harness all name the same mode without a facade
//! dependency.
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::mode::Mode;
//!
//! // The default is the robust full-screen floor.
//! assert_eq!(Mode::default(), Mode::AltScreen);
//!
//! // Inline caps the live region at `max_height` rows; overflow commits into
//! // native scrollback above it.
//! let inline = Mode::inline(3);
//! assert_eq!(inline.max_height(), Some(3));
//! assert!(inline.is_inline());
//! ```

/// Which screen region the runtime renders into.
///
/// The two variants are the peer render modes of ADR 0013. [`AltScreen`] is the
/// default and the robust floor for full-screen apps; [`Inline`] serves the
/// coding-agent workload with terminal-native scrollback, selection, and copy.
///
/// [`AltScreen`]: Mode::AltScreen
/// [`Inline`]: Mode::Inline
///
/// # Examples
///
/// ```
/// use rabbitui_core::mode::Mode;
///
/// let mode = Mode::Inline { max_height: 5 };
/// assert_eq!(mode.max_height(), Some(5));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mode {
    /// The alternate screen: a dedicated full-screen buffer the app owns entirely
    /// and the terminal discards on exit. App output does not enter scrollback.
    /// The default (ADR 0013's robust full-screen floor).
    #[default]
    AltScreen,
    /// Inline: a bounded live tail at the bottom of the primary screen. Finalized
    /// content is committed append-once into native scrollback above the tail;
    /// the live tail never exceeds `max_height` (further bounded by the content
    /// and the viewport height at render time — ADR 0013's live-tail invariant).
    Inline {
        /// The maximum height of the live tail, in rows. The rendered tail is
        /// `min(content_height, max_height, viewport_height)`.
        max_height: u16,
    },
}

impl Mode {
    /// An [`Inline`](Mode::Inline) mode with the given live-tail `max_height`.
    ///
    /// A terse constructor for the common inline case.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::mode::Mode;
    ///
    /// assert_eq!(Mode::inline(4), Mode::Inline { max_height: 4 });
    /// ```
    #[must_use]
    pub const fn inline(max_height: u16) -> Self {
        Self::Inline { max_height }
    }

    /// Returns true if this is [`AltScreen`](Mode::AltScreen).
    #[must_use]
    pub const fn is_alt_screen(self) -> bool {
        matches!(self, Self::AltScreen)
    }

    /// Returns true if this is [`Inline`](Mode::Inline).
    #[must_use]
    pub const fn is_inline(self) -> bool {
        matches!(self, Self::Inline { .. })
    }

    /// The live-tail `max_height` for [`Inline`](Mode::Inline), or `None` for
    /// [`AltScreen`](Mode::AltScreen).
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui_core::mode::Mode;
    ///
    /// assert_eq!(Mode::inline(2).max_height(), Some(2));
    /// assert_eq!(Mode::AltScreen.max_height(), None);
    /// ```
    #[must_use]
    pub const fn max_height(self) -> Option<u16> {
        match self {
            Self::AltScreen => None,
            Self::Inline { max_height } => Some(max_height),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_alt_screen() {
        assert_eq!(Mode::default(), Mode::AltScreen);
        assert!(Mode::default().is_alt_screen());
    }

    #[test]
    fn inline_carries_max_height() {
        let mode = Mode::inline(7);
        assert!(mode.is_inline());
        assert_eq!(mode.max_height(), Some(7));
    }

    #[test]
    fn alt_screen_has_no_max_height() {
        assert_eq!(Mode::AltScreen.max_height(), None);
        assert!(!Mode::AltScreen.is_inline());
    }
}
