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

use qwertty::{InputEvent, ProtocolPosition, TokioTerminalSession, commands};
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
/// Opening a `Terminal` enters raw mode only; the active render **engine**
/// (`crate::engine`) drives screen setup — entering/leaving the alternate screen
/// or the inline live region — by producing bytes this type merely writes (ADR
/// 0013's pure-engine split). If the program panics or the value is dropped
/// without [`close`](Self::close), a best-effort restore sequence is written
/// directly to `/dev/tty` — and it *unconditionally* leaves the alternate screen,
/// whichever mode was active, so a panic never strands the user there. That
/// guarantee every framework in the research survey eventually learned to make
/// first.
#[derive(Debug)]
pub struct Terminal {
    session: Option<TokioTerminalSession>,
}

impl Terminal {
    /// Opens the interactive terminal in raw mode.
    ///
    /// Screen setup (alternate screen, cursor visibility, clearing, or the inline
    /// live region) is the engine's job now — [`run`](crate::app::run) writes the
    /// engine's mode-entry bytes as its first frame. This only installs the
    /// panic-restore hook and puts the tty in raw mode.
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

        // Resolve the controlling terminal's real device path instead of the
        // `/dev/tty` alias: on macOS, kqueue (and therefore tokio's AsyncFd)
        // rejects the alias device with EINVAL, while the underlying pty path
        // (`/dev/ttysNNN`) registers fine. Filed upstream in the qwertty
        // requirements handover; until qwertty resolves it internally, this
        // seam does. Falls back to the alias where no std stream is a tty.
        let session = match controlling_tty_path() {
            Some(path) => TokioTerminalSession::open_path(path)?,
            None => TokioTerminalSession::open()?,
        };
        Ok(Self { session: Some(session) })
    }

    /// Writes raw `bytes` to the terminal and flushes them.
    ///
    /// The single primitive the render engines drive through: an engine produces
    /// a whole frame's (or mode transition's) bytes and this writes them. All
    /// cursor, mode, and SGR encoding stays in the engines/encoder behind this
    /// substrate seam.
    ///
    /// # Errors
    ///
    /// Returns an error if writing or flushing fails.
    pub async fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let session = self.session_mut();
        session.bytes(bytes).await?;
        session.flush().await?;
        Ok(())
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

    /// Restores the terminal and releases it (leaves raw mode).
    ///
    /// The active engine has already written its own teardown frame (leave alt
    /// screen, or drop below the inline tail; reset styles; show the cursor)
    /// through [`write_bytes`](Self::write_bytes) before `run` calls this. As an
    /// unconditional backstop this still emits leave-alt-screen, reset, and
    /// show-cursor — leaving the alt screen is a harmless no-op when inline, so
    /// the invariant "RESTORE always leaves the alt screen" holds regardless of
    /// mode — then leaves raw mode.
    ///
    /// # Errors
    ///
    /// Returns an error if restoration writes fail; the terminal state is
    /// still restored on a best-effort basis.
    pub async fn close(mut self) -> Result<()> {
        let mut session = self.session.take().expect("session present until close");
        // Disable mouse reporting unconditionally (harmless if never enabled), so
        // the shell does not inherit mouse capture. The engine's leave frame has
        // already disabled it if it was on; this backstop matches RESTORE.
        session.bytes(encode::DISABLE_MOUSE).await?;
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

/// The real path of the controlling terminal (e.g. `/dev/ttys003`), resolved
/// via `ttyname` on the first standard stream that is a terminal.
fn controlling_tty_path() -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let fds: [&dyn std::os::fd::AsFd; 3] = [&stdin, &stdout, &stderr];
    for fd in fds {
        if let Ok(name) = rustix::termios::ttyname(fd, Vec::new()) {
            let bytes = name.into_bytes();
            return Some(std::ffi::OsString::from_vec(bytes).into());
        }
    }
    None
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
