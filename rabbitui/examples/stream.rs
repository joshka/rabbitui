//! Inline mode + a fake streaming transcript: the slice-5 flagship.
//!
//! Demonstrates ADR 0013's inline invariant — an append-once scrollback commit
//! channel plus a bounded live tail — and runtime mode switching. Run with
//! `cargo run --example stream` and **scroll up in your terminal**: committed log
//! lines accumulate in *native* scrollback (with working selection and copy),
//! while the live tail stays pinned to the bottom and never grows past its bound.
//!
//! # Controls
//!
//! - `Ctrl-N` — commit the next numbered log line into scrollback (an update-time
//!   [`Update::commit`], so it happens exactly once per press).
//! - `m` — toggle between [`Mode::Inline`] and [`Mode::AltScreen`] live, via
//!   [`Update::set_mode`]. In alt-screen the transcript is a full-screen buffer;
//!   switch back to inline and the committed history is back in scrollback.
//! - The input is auto-focused; type and press Enter to commit your own line.
//! - `Ctrl-T` — toggle inline/alt-screen; `Ctrl-C` quits.
//! - `q` — quit.
//!
//! # A timer-free demo
//!
//! Real streaming (a timer emitting lines on a schedule) is slice 6; this v1 is
//! keypress-driven so it needs no async effects. Each `n` stands in for one
//! arrival of streamed output.
//!
//! Note (substrate gap): the input is only reachable via Tab (qwertty decodes no
//! Shift-Tab yet); while it is focused it consumes printable keys, so press Tab
//! again to cycle focus away before using `n`/`m`/`q`. See `rabbitui::input`.

use std::ops::ControlFlow;

use rabbitui::App;
use rabbitui::app::{Config, Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::mode::Mode;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::theme::Role;
use rabbitui_widgets::{Text, TextInput};

/// The bounded live-tail height, in rows: one input row, one status row, one hint
/// row. Everything above is committed history the terminal owns.
const TAIL_HEIGHT: u16 = 3;

/// The app's owned state: how many lines have been committed, whether we are
/// currently inline, the input draft, and its generation (bumped to clear the
/// input after a submit — the slice-4 uncontrolled-input workaround).
struct Stream {
    committed: u32,
    inline: bool,
    draft: String,
    input_generation: u64,
}

impl Default for Stream {
    fn default() -> Self {
        Self {
            committed: 0,
            inline: true,
            draft: String::new(),
            input_generation: 0,
        }
    }
}

impl App for Stream {
    /// Folds one update into the app: commit lines, toggle mode, quit.
    fn update(&mut self, update: Update<'_>) -> ControlFlow<()> {
        // Track the input draft on every edit; a submit (Enter) commits it.
        if let Some(Outcome::Changed(value)) = update.outcome_for(&[input_key(self)]) {
            self.draft = value.clone();
        }
        if update.outcome_for(&[input_key(self)]) == Some(&Outcome::Submitted) {
            let line = self.draft.trim().to_string();
            if !line.is_empty() {
                self.committed += 1;
                update.commit(format!("{:>3}  {line}", self.committed));
            }
            // Re-key the input to clear it, and reset the tracked draft.
            self.input_generation += 1;
            self.draft.clear();
        }

        // App-level bindings fire only on keys no focused widget consumed (the
        // input eats printables while focused — Update::consumed is the guard).
        // The input is the only focusable, so auto-focus means it is ALWAYS
        // focused and printable bindings would never fire — hence ctrl-chords,
        // which text inputs pass through (user rule: printable bindings must not
        // fight text boxes).
        if let Event::Input(input) = update.event() {
            let (key, ctrl) = match input.as_key() {
                Some(k) => (Some(k.key), k.modifiers.ctrl),
                None => (None, false),
            };
            match key {
                // Commit the next numbered log line into native scrollback.
                Some(Key::Char('n')) if ctrl => {
                    self.committed += 1;
                    update.commit(format!("{:>3}  log line", self.committed));
                }
                // Toggle inline ↔ alt-screen live.
                Some(Key::Char('t')) if ctrl => {
                    self.inline = !self.inline;
                    update.set_mode(if self.inline {
                        Mode::inline(TAIL_HEIGHT)
                    } else {
                        Mode::AltScreen
                    });
                }
                Some(Key::Char('c')) if ctrl => return ControlFlow::Break(()),
                Some(Key::Char('q') | Key::Escape) if !update.consumed() => {
                    return ControlFlow::Break(());
                }
                _ => {}
            }
        }

        ControlFlow::Continue(())
    }

    /// Declares the live tail: an input, a status line, and a hint.
    ///
    /// In inline mode the frame's area is the bounded tail (the runtime sizes
    /// the buffer to `TAIL_HEIGHT`); in alt-screen it is the whole viewport. The
    /// same declaration works in both — the tail rows pin to the top of
    /// whatever area the mode provides.
    ///
    /// This is an inline-mode example, so the tail is *not* wrapped in a panel:
    /// the committed scrollback above belongs to the terminal, and a border
    /// around a bottom-pinned strip that shares its top edge with native
    /// history would read wrong. Styling stays inside the tail, via theme roles.
    fn view(&self, frame: &mut Frame<'_>) {
        let [input_row, status_row, hint_row] = frame.rows([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ]);

        frame.widget(
            input_key(self),
            input_row,
            &TextInput::new().placeholder("Tab, type, Enter…"),
        );

        let mode = if self.inline { "inline" } else { "alt-screen" };
        let status = format!("[{mode}]  {} committed", self.committed);
        frame.widget(
            key("status"),
            status_row,
            &Text::new(&status).role(Role::Success),
        );

        let hint = "type + Enter: commit   Ctrl-N: log line   Ctrl-T: mode   Ctrl-C: quit";
        frame.widget(key("hint"), hint_row, &Text::new(hint).role(Role::Muted));
    }

    fn config(&self) -> Config {
        Config::new().mode(Mode::inline(TAIL_HEIGHT))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Stream::default().run().await?;
    Ok(())
}

/// The input's key for this frame, carrying the generation so a submit re-keys
/// (and clears) it.
fn input_key(app: &Stream) -> rabbitui_core::id::Key {
    key("input").index(usize::try_from(app.input_generation).unwrap_or(usize::MAX))
}
