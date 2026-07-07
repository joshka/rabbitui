//! The substrate seam.
//!
//! This module is the single place rabbitui touches qwertty
//! (`docs/adr/0012-terminal-substrate.md`): everything above it works in
//! rabbitui's own types. [`Terminal`] owns the session for the lifetime of the
//! app and guarantees restoration on every exit path — orderly [`close`],
//! drop, and panic.
//!
//! [`close`]: Terminal::close

use std::io::Write as _;
use std::sync::Once;

use qwertty::{CommandBuffer, InputEvent, ProtocolPosition, TokioTerminalSession, commands};
use rabbitui_core::geometry::{Position, Size};
use rabbitui_core::style::Style;

use crate::encode;

/// Errors reported by the application loop.
///
/// Mostly the terminal substrate's error ([`Error::Terminal`], wrapping
/// [`qwertty::Error`]), plus [`Error::Theme`] for a theme file that cannot be
/// loaded or parsed (ADR 0007's file loading lives in the facade). A
/// `From<qwertty::Error>` lets the seam's `?` keep working unchanged.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// An error from the terminal substrate (I/O, decoding, size query).
    Terminal(qwertty::Error),
    /// A theme file could not be loaded or parsed. Carries a rendered message so
    /// this type does not depend on the `themes` feature being enabled.
    Theme(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Terminal(error) => write!(f, "{error}"),
            Error::Theme(message) => write!(f, "theme error: {message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Terminal(error) => Some(error),
            Error::Theme(_) => None,
        }
    }
}

impl From<qwertty::Error> for Error {
    fn from(error: qwertty::Error) -> Self {
        Error::Terminal(error)
    }
}

/// A specialized result for application operations.
pub type Result<T> = std::result::Result<T, Error>;

static PANIC_RESTORE_HOOK: Once = Once::new();

/// Exclusive ownership of the interactive terminal.
///
/// Opening a `Terminal` enters raw mode and the alternate screen and hides the
/// cursor; [`close`](Self::close) undoes all of it in order. If the program
/// panics or the value is dropped without `close`, a best-effort restore
/// sequence is written directly to `/dev/tty` so the user's shell comes back
/// usable — the guarantee every framework in the research survey eventually
/// learned to make first.
#[derive(Debug)]
pub struct Terminal {
    session: Option<TokioTerminalSession>,
}

impl Terminal {
    /// Opens the interactive terminal: raw mode, alternate screen, hidden
    /// cursor, cleared screen.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal device cannot be opened or configured.
    pub async fn open() -> Result<Self> {
        PANIC_RESTORE_HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                restore_directly();
                previous(info);
            }));
        });

        let mut session = TokioTerminalSession::open()?;
        session.bytes(encode::ENTER_ALT_SCREEN).await?;
        session.command(commands::cursor::hide()).await?;
        session.command(commands::screen::clear()).await?;
        session.flush().await?;
        Ok(Self { session: Some(session) })
    }

    /// The current terminal size in cells.
    ///
    /// # Errors
    ///
    /// Returns an error if the size query fails.
    pub fn size(&self) -> Result<Size> {
        let size = self.session().size()?;
        Ok(Size::new(size.columns(), size.rows()))
    }

    /// Writes `text` at `position` (zero-based cells) in `style`.
    ///
    /// Output is buffered; call [`flush`](Self::flush) to make it visible.
    ///
    /// # Errors
    ///
    /// Returns an error if writing to the session buffer fails.
    pub async fn print_styled(
        &mut self,
        position: Position,
        text: &str,
        style: Style,
    ) -> Result<()> {
        let protocol =
            ProtocolPosition::new(position.y.saturating_add(1), position.x.saturating_add(1));
        let session = self.session_mut();
        session.command(commands::cursor::move_to(protocol)).await?;
        session.bytes(encode::sgr(style)).await?;
        session.text(text).await?;
        session.bytes(encode::SGR_RESET).await?;
        Ok(())
    }

    /// Writes a pre-encoded frame to the terminal and flushes it.
    ///
    /// The renderer (`render.rs`) builds a whole frame's bytes — cursor moves,
    /// SGR runs, text, and synchronized-output framing — into a
    /// [`CommandBuffer`] and hands it here in one call, so the cursor and mode
    /// encoding stays behind this substrate seam.
    ///
    /// # Errors
    ///
    /// Returns an error if writing or flushing fails.
    pub(crate) async fn write_frame(&mut self, frame: CommandBuffer) -> Result<()> {
        let session = self.session_mut();
        session.bytes(frame.into_bytes()).await?;
        session.flush().await?;
        Ok(())
    }

    /// Flushes buffered output to the terminal.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying write fails.
    pub async fn flush(&mut self) -> Result<()> {
        self.session_mut().flush().await?;
        Ok(())
    }

    /// Waits for the next input event.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the terminal fails.
    pub async fn next_event(&mut self) -> Result<InputEvent> {
        Ok(self.session_mut().next_event().await?)
    }

    /// Restores the terminal (leave alternate screen, reset styles, show the
    /// cursor, cooked mode) and releases it.
    ///
    /// # Errors
    ///
    /// Returns an error if restoration writes fail; the terminal state is
    /// still restored on a best-effort basis.
    pub async fn close(mut self) -> Result<()> {
        let mut session = self.session.take().expect("session present until close");
        session.bytes(encode::SGR_RESET).await?;
        session.command(commands::cursor::show()).await?;
        session.bytes(encode::LEAVE_ALT_SCREEN).await?;
        session.flush().await?;
        session.leave().await?;
        Ok(())
    }

    fn session(&self) -> &TokioTerminalSession {
        self.session.as_ref().expect("session present until close")
    }

    fn session_mut(&mut self) -> &mut TokioTerminalSession {
        self.session.as_mut().expect("session present until close")
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // `close` already ran the orderly path. Otherwise (early return,
        // panic unwind) fall back to the direct restore; the session's own
        // Drop then restores cooked mode.
        if self.session.is_some() {
            restore_directly();
        }
    }
}

/// Writes the restore-of-last-resort sequence straight to the controlling
/// terminal, bypassing session buffering. Safe to call at any time, including
/// from a panic hook; all errors are deliberately ignored.
fn restore_directly() {
    if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        let _ = tty.write_all(encode::RESTORE);
        let _ = tty.flush();
    }
}
