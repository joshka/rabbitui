//! The substrate seam.
//!
//! This module is the single place rabbitui touches qwertty
//! (`docs/adr/0012-terminal-substrate.md`): everything above it works in
//! rabbitui's own types. [`Terminal`] owns the session for the lifetime of the
//! app and guarantees restoration on every exit path — orderly [`close`],
//! drop, and panic.
//!
//! [`close`]: Terminal::close

use std::cell::Cell;
use std::io::Write as _;
use std::sync::Once;

use qwertty::{Event, ProtocolPosition, TokioTerminalSession, commands};
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

thread_local! {
    /// Set on a thread while it is polling a *contained* effect task (ADR 0005:
    /// effect panics are caught at the tokio task boundary and surfaced as
    /// [`Event::EffectFailed`](crate::app::Event::EffectFailed), never unwinding
    /// the loop). The panic-restore hook (`restore_directly` via the installed
    /// hook) checks this flag: while it is set, a panic on this thread is a
    /// *contained* effect panic — the loop stays alive and will repaint — so the
    /// hook must NOT write the visible leave-alt-screen restore sequence, which
    /// would corrupt the live display mid-run. A genuine panic in `view`/`update`
    /// (the main loop thread, where this flag is never set) still restores.
    ///
    /// A `Cell<bool>` is enough: effect tasks are polled to a panic point without
    /// re-entering the guard (no nesting across tasks on one thread mid-panic).
    static IN_EFFECT_POLL: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` with the current thread marked as polling a contained effect task,
/// restoring the previous flag afterward.
///
/// The effect runtime (`crate::effect`) wraps each spawned task's poll in this so
/// the panic-restore hook can tell a *contained* effect panic (which the runtime
/// reports as a message and survives) from a genuine `view`/`update` panic (which
/// must visibly restore the terminal). See [`IN_EFFECT_POLL`].
pub(crate) fn with_effect_poll_guard<R>(f: impl FnOnce() -> R) -> R {
    struct Guard(bool);
    impl Drop for Guard {
        fn drop(&mut self) {
            IN_EFFECT_POLL.with(|flag| flag.set(self.0));
        }
    }
    let previous = IN_EFFECT_POLL.with(|flag| flag.replace(true));
    let _guard = Guard(previous);
    f()
}

/// Whether the current thread is inside a contained effect poll — the signal the
/// restore hook uses to suppress the visible restore for a contained panic.
fn in_effect_poll() -> bool {
    IN_EFFECT_POLL.with(Cell::get)
}

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
                // A *contained* effect-task panic (ADR 0005) is caught at the
                // tokio task boundary and reported as `EffectFailed`; the loop
                // survives and repaints. Writing the visible leave-alt-screen
                // restore here would corrupt the live display mid-run, so skip it
                // when this thread is inside an effect poll. A genuine `view` /
                // `update` panic runs on the main loop thread, where the flag is
                // never set, and still restores. `panic = "abort"` is unaffected:
                // there the process dies and the Terminal Drop / OS teardown apply.
                handle_panic_restore(restore_directly);
                previous(info);
            }));
        });

        // Open through qwertty's own controlling-terminal acquisition. It dups the
        // inherited read-write stdin — the descriptor kqueue accepts on macOS —
        // falling back to the resolved device path, then the `/dev/tty` alias
        // (`session.acquisition()` reports which branch won). This resolves the
        // macOS EINVAL that once forced our hand-rolled `ttyname → open_path`
        // workaround here (qwertty substrate-status; requirements doc "Resolution
        // — qwertty sync" #3), so the seam no longer resolves the device itself.
        let session = TokioTerminalSession::open()?;
        Ok(Self {
            session: Some(session),
        })
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
    pub async fn next_event(&mut self) -> Result<Event> {
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
    /// This byte backstop is **kept** even though qwertty's session leave/drop
    /// replays its own mode ledger: rabbitui enters the alternate screen by
    /// writing raw bytes through the *engine* (ADR 0013's pure-engine split), not
    /// through qwertty's ledger API, so qwertty's ledger never recorded — and so
    /// never undoes — our alt-screen entry. Its restore is therefore not
    /// equivalent to ours; the belt-and-suspenders leave-alt-screen stays.
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

/// The panic-hook decision: run `restore` only for a genuine panic, never for a
/// *contained* effect-task panic.
///
/// Split out from the installed hook so the containment rule is unit-testable
/// without writing to a real `/dev/tty` (the test passes a counting closure).
/// The rule: if this thread is inside an effect poll ([`in_effect_poll`]), the
/// panic is caught and reported as `EffectFailed` and the loop repaints — so
/// suppress the visible restore. Otherwise (a `view`/`update` panic on the main
/// loop thread) restore the terminal, the whole panic-safety contract.
fn handle_panic_restore(restore: impl Fn()) {
    if in_effect_poll() {
        return;
    }
    restore();
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{handle_panic_restore, in_effect_poll, with_effect_poll_guard};

    #[test]
    fn effect_poll_guard_sets_and_clears_the_flag() {
        assert!(!in_effect_poll(), "flag starts clear");
        with_effect_poll_guard(|| assert!(in_effect_poll(), "flag set inside the guard"));
        assert!(!in_effect_poll(), "flag restored after the guard");
    }

    #[test]
    fn guard_restores_previous_flag_even_on_panic() {
        // A panic inside the guarded closure must still restore the previous flag
        // (the guard's Drop runs during unwind), so a later poll on the same
        // reused worker thread is not left wedged as "in effect poll".
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_effect_poll_guard(|| {
                assert!(in_effect_poll());
                panic!("boom");
            });
        }));
        assert!(result.is_err(), "the panic propagated");
        assert!(!in_effect_poll(), "flag cleared after an unwinding guard");
    }

    #[test]
    fn contained_effect_panic_does_not_restore_but_a_real_panic_does() {
        // The hook's decision, exercised with a counting closure standing in for
        // the real `/dev/tty` restore. Inside an effect poll: suppressed. Outside
        // (a view/update panic on the main thread): the restore fires.
        let restores = Cell::new(0u32);
        let restore = || restores.set(restores.get() + 1);

        // Contained effect panic: the guard is set, so no visible restore.
        with_effect_poll_guard(|| handle_panic_restore(restore));
        assert_eq!(
            restores.get(),
            0,
            "a contained effect panic must not restore"
        );

        // A genuine view/update panic: the flag is clear, so restore fires.
        handle_panic_restore(restore);
        assert_eq!(restores.get(), 1, "a real panic must restore the terminal");
    }
}
