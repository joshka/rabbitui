//! The minimal application loop.
//!
//! [`run`] is the walking-skeleton facade over the event loop (ADR 0005): it
//! owns the terminal, drives update → view → diff → render, and restores the
//! terminal on every exit path. The app supplies plain owned state, a
//! synchronous `update` that folds events into that state, and a synchronous
//! `view` that paints the state into a buffer.
//!
//! # A pre-declared-frame API, replaced in slice 2
//!
//! This is deliberately the smallest loop that renders and quits. It takes a
//! single `view` closure that paints straight into a [`Buffer`], with no
//! widgets, identity, frame facts, layout, timers, effects, or frame
//! scheduling — none of the declared-frame contract (ADR 0001) exists yet.
//! Slice 2 replaces this `run` signature with the declared-frame builder and a
//! coalescing scheduler; treat this API as provisional.
//!
//! # Examples
//!
//! A one-line app that quits on the next event:
//!
//! ```no_run
//! use std::ops::ControlFlow;
//!
//! use rabbitui::app::{self, Event};
//! use rabbitui_core::buffer::Buffer;
//! use rabbitui_core::geometry::Position;
//! use rabbitui_core::style::Style;
//!
//! # async fn demo() -> rabbitui::app::Result<()> {
//! app::run(
//!     (),
//!     |_state: &mut (), _event: Event| ControlFlow::Break(()),
//!     |_state: &(), buffer: &mut Buffer| {
//!         buffer.set_string(Position::ORIGIN, "hi", Style::new());
//!     },
//! )
//! .await
//! # }
//! ```

use std::ops::ControlFlow;

use qwertty::InputEvent;
use rabbitui_core::buffer::Buffer;
use rabbitui_core::geometry::Size;

use crate::render;
use crate::terminal::Terminal;

pub use crate::terminal::{Error, Result};

/// An event delivered to the app's `update` function.
///
/// # Substrate gap: resize is polled, not pushed
///
/// qwertty has no resize event yet (`docs/adr/0012-terminal-substrate.md`), so
/// [`run`] polls the terminal size once per loop iteration and synthesizes
/// [`Event::Resize`] when it changes. This means a resize is only observed on
/// the next input event, not the instant the window changes; when qwertty gains
/// a resize signal this becomes push-based with no change to this enum.
///
/// # Examples
///
/// ```
/// use rabbitui::app::Event;
/// use rabbitui_core::geometry::Size;
///
/// let event = Event::Resize(Size::new(80, 24));
/// assert!(matches!(event, Event::Resize(_)));
/// ```
#[derive(Debug, Clone)]
pub enum Event {
    /// An input event decoded by the substrate (a key, control byte, …).
    Input(InputEvent),
    /// The terminal was resized to this new size, detected by polling.
    Resize(Size),
}

/// Runs the application loop until `update` returns [`ControlFlow::Break`].
///
/// The loop opens the terminal, renders an initial full frame, then repeats:
/// wait for one input event; poll the terminal size and, if it changed, resize
/// the buffers (a full repaint) and deliver [`Event::Resize`] to `update`;
/// deliver the [`Event::Input`] to `update`; if `update` asked to break, close
/// the terminal and return; otherwise paint the new state with `view` into the
/// back buffer, diff it against the front buffer, render the difference, and
/// swap the buffers.
///
/// `update` and `view` are strictly synchronous — no `.await` — matching ADR
/// 0005's synchronous core; only the loop edges (input, render) are async. On
/// the orderly break path the terminal is closed explicitly; a panic is caught
/// by the restore hook installed when the terminal opened.
///
/// # Errors
///
/// Returns an error if opening the terminal, reading input, polling the size,
/// rendering, or closing the terminal fails.
///
/// # Examples
///
/// A counter that increments on every event and quits at three:
///
/// ```no_run
/// use std::ops::ControlFlow;
///
/// use rabbitui::app::{self, Event};
/// use rabbitui_core::buffer::Buffer;
/// use rabbitui_core::geometry::Position;
/// use rabbitui_core::style::Style;
///
/// # async fn demo() -> rabbitui::app::Result<()> {
/// app::run(
///     0u32,
///     |count: &mut u32, _event: Event| {
///         *count += 1;
///         if *count >= 3 { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
///     },
///     |count: &u32, buffer: &mut Buffer| {
///         buffer.set_string(Position::ORIGIN, &count.to_string(), Style::new());
///     },
/// )
/// .await
/// # }
/// ```
pub async fn run<S>(
    mut state: S,
    mut update: impl FnMut(&mut S, Event) -> ControlFlow<()>,
    view: impl Fn(&S, &mut Buffer),
) -> Result<()> {
    let mut terminal = Terminal::open().await?;
    let mut size = terminal.size()?;

    // Front buffer: what the terminal currently shows. Back buffer: what the
    // next frame will show. The initial front is blank, so the first diff is a
    // full paint of everything `view` writes.
    let mut front = Buffer::new(size);
    let mut back = Buffer::new(size);
    view(&state, &mut back);
    render::render(&mut terminal, &back.diff(&front)).await?;
    std::mem::swap(&mut front, &mut back);

    loop {
        let input = terminal.next_event().await?;

        // Poll for a resize (substrate has no resize event; see `Event`). On a
        // change, resize both buffers to blank so the next diff is a full
        // repaint, then deliver the resize to `update`.
        let new_size = terminal.size()?;
        if new_size != size {
            size = new_size;
            front.resize(size);
            back.resize(size);
            if let ControlFlow::Break(()) = update(&mut state, Event::Resize(size)) {
                return terminal.close().await;
            }
        }

        if let ControlFlow::Break(()) = update(&mut state, Event::Input(input)) {
            return terminal.close().await;
        }

        // Repaint the back buffer from scratch, diff against the front, render
        // the difference, and swap. Widgets always paint into a fresh buffer;
        // the diff computes the damage (ADR 0003).
        back.resize(size);
        view(&state, &mut back);
        render::render(&mut terminal, &back.diff(&front)).await?;
        std::mem::swap(&mut front, &mut back);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_event_carries_the_new_size() {
        let event = Event::Resize(Size::new(120, 40));
        match event {
            Event::Resize(size) => assert_eq!(size, Size::new(120, 40)),
            Event::Input(_) => panic!("expected a resize event"),
        }
    }
}
