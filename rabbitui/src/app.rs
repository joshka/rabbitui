//! The minimal application loop.
//!
//! [`run`] is the walking-skeleton facade over the event loop (ADR 0005): it
//! owns the terminal, drives update → view → diff → render, and restores the
//! terminal on every exit path. The app supplies plain owned state, a
//! synchronous `update` that folds events into that state, and a synchronous
//! `view` that declares the state's UI into a [`Frame`].
//!
//! # The declared frame
//!
//! `view` receives a [`Frame`] (`docs/adr/0001-programming-model.md`), not a
//! bare buffer: it declares widgets by key into the frame, which composes their
//! identities, lends each its framework-retained state from the loop's
//! [`StateStore`], and paints them into the back buffer. The state store lives
//! across iterations, so a widget's scroll offset, cursor, or other retained
//! state survives frame to frame by identity. The loop clears the back buffer
//! to blank before every `view` call — widgets declare everything each frame,
//! so nothing carries over except through the store. This *is* the declared
//! frame; the frame *facts* it collects (hit regions, focus order, outcomes)
//! arrive in slice 3, but the contract is already the one every widget renders
//! through.
//!
//! # Examples
//!
//! A one-line app that quits on the next event:
//!
//! ```no_run
//! use std::ops::ControlFlow;
//!
//! use rabbitui::app::{self, Event};
//! use rabbitui_core::frame::Frame;
//! use rabbitui_core::id::key;
//! use rabbitui_widgets::Text;
//!
//! # async fn demo() -> rabbitui::app::Result<()> {
//! app::run(
//!     (),
//!     |_state: &mut (), _event: Event| ControlFlow::Break(()),
//!     |_state: &(), frame: &mut Frame<'_>| {
//!         frame.widget(key("greeting"), frame.area(), &Text::new("hi"));
//!     },
//! )
//! .await
//! # }
//! ```

use std::ops::ControlFlow;

use qwertty::InputEvent;
use rabbitui_core::buffer::Buffer;
use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::Size;
use rabbitui_core::store::StateStore;

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
/// The loop owns a [`StateStore`] across iterations and brackets each `view`
/// call in [`StateStore::begin_frame`] / [`StateStore::end_frame`], building a
/// [`Frame`] over the back buffer and the store. The back buffer is cleared to
/// blank before every `view` call: widgets declare everything each frame, and
/// the double-buffer diff turns the fresh paint into minimal damage (ADR 0003).
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
/// use rabbitui_core::frame::Frame;
/// use rabbitui_core::id::key;
/// use rabbitui_widgets::Text;
///
/// # async fn demo() -> rabbitui::app::Result<()> {
/// app::run(
///     0u32,
///     |count: &mut u32, _event: Event| {
///         *count += 1;
///         if *count >= 3 { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
///     },
///     |count: &u32, frame: &mut Frame<'_>| {
///         let text = count.to_string();
///         frame.widget(key("count"), frame.area(), &Text::new(&text));
///     },
/// )
/// .await
/// # }
/// ```
pub async fn run<S>(
    mut state: S,
    mut update: impl FnMut(&mut S, Event) -> ControlFlow<()>,
    view: impl Fn(&S, &mut Frame<'_>),
) -> Result<()> {
    let mut terminal = Terminal::open().await?;
    let mut size = terminal.size()?;

    // Front buffer: what the terminal currently shows. Back buffer: what the
    // next frame will show. The initial front is blank, so the first diff is a
    // full paint of everything `view` declares. The state store persists across
    // iterations so widget-retained state (scroll, cursor) survives by identity.
    let mut front = Buffer::new(size);
    let mut back = Buffer::new(size);
    let mut store = StateStore::new();
    draw(&mut back, &mut store, &state, &view);
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
        // the difference, and swap. Widgets always declare into a fresh buffer;
        // the diff computes the damage (ADR 0003).
        back.reset();
        draw(&mut back, &mut store, &state, &view);
        render::render(&mut terminal, &back.diff(&front)).await?;
        std::mem::swap(&mut front, &mut back);
    }
}

/// Declares one frame: brackets `view` in the store's frame lifecycle and
/// builds a [`Frame`] over `buffer` and `store` for it to declare into.
///
/// The caller has already cleared (or resized) `buffer` to blank, matching the
/// declared-frame rule that widgets re-declare everything each frame.
fn draw<S>(
    buffer: &mut Buffer,
    store: &mut StateStore,
    state: &S,
    view: &impl Fn(&S, &mut Frame<'_>),
) {
    store.begin_frame();
    {
        let mut frame = Frame::new(buffer, store);
        view(state, &mut frame);
    }
    store.end_frame();
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
