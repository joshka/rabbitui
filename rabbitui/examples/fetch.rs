//! Async effects: a fake search-as-you-type app — the slice-6 flagship.
//!
//! Demonstrates ADR 0005's commands-only effect model end to end:
//!
//! - **Debounced search (cancel-previous).** Every keystroke in the [`TextInput`]
//!   spawns a `group("search")` command — a simulated ~300ms fetch producing
//!   result items. Spawning into a group aborts the group's previous task
//!   (Textual `@work(exclusive=True)`), so **rapid typing completes far fewer
//!   fetches than keystrokes**; a completed-fetch counter proves it.
//! - **A stream "subscription".** `t` toggles a clock ticker — a
//!   [`Cmd::stream`] over a hand-rolled 1-second interval, spawned into the
//!   `"clock"` group — whose messages update a live clock line. Toggling it off
//!   spawns a [`Cmd::cancel_group`] that aborts the stream for good (the
//!   stream-stop primitive, ADR 0005 / slice 7), rather than leaving it running
//!   with its ticks ignored.
//! - **A widget command.** `Ctrl-L` clears the input via
//!   `update.widget::<TextInput>(…, |s| s.clear())`, applied between frames.
//! - **Contained failures.** Effect panics arrive as [`Event::EffectFailed`] and
//!   are shown on the status line, never crashing the loop.
//!
//! Run with `cargo run --example fetch`. Type to search; watch the completed
//! counter lag far behind your keystrokes; press `t` to toggle the clock; `Ctrl-L`
//! to clear; `q` to quit.
//!
//! Note (substrate gap): the input is reached via Tab; while it is focused it
//! consumes printable keys, so app-level `t`/`q` require Tab-ing focus away first.
//! `Ctrl-L` works while focused (TextInput leaves ctrl chords for the app).

use std::ops::ControlFlow;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_core::Stream;
use rabbitui::App;
use rabbitui::app::{Event, Update};
use rabbitui::effect::Cmd;
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::Constraint;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::style::{Color, Style};
use rabbitui_widgets::{SelectionList, Text, TextInput};

/// A message an effect produces, re-entering the loop as [`Event::Message`].
#[derive(Debug, Clone)]
enum Msg {
    /// A search for `query` completed, yielding these result rows.
    Results { query: String, rows: Vec<String> },
    /// The ticker fired; carries a monotonically increasing tick count.
    Tick(u64),
}

/// The app's owned state.
#[derive(Default)]
struct Fetch {
    /// The current input draft, tracked from `Changed` outcomes.
    draft: String,
    /// The most recent completed search's results.
    results: Vec<String>,
    /// How many searches have *completed* — far fewer than keystrokes typed,
    /// because cancel-previous aborts superseded fetches.
    completed: u64,
    /// Whether the clock ticker stream is running.
    ticking: bool,
    /// The latest tick count from the ticker.
    ticks: u64,
    /// The last effect failure, shown on the status line (contained, not fatal).
    last_error: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    App::new(Fetch::default(), update, view).run().await?;
    Ok(())
}

/// Folds one update into the app: spawn searches, toggle the ticker, clear the
/// input, and absorb effect results.
fn update(app: &mut Fetch, update: Update<'_, Msg>) -> ControlFlow<()> {
    // A keystroke that changed the input spawns a new debounced fetch. The
    // `group("search")` aborts the previous fetch, so only the last-typed query
    // completes — the cancel-previous guarantee.
    if let Some(Outcome::Changed(value)) = update.outcome_for(&[key("input")]) {
        app.draft = value.clone();
        let query = app.draft.clone();
        update.spawn(fake_fetch(query).group("search"));
    }

    // Effect results re-enter here as messages.
    match update.event() {
        Event::Message(Msg::Results { query, rows }) => {
            app.completed += 1;
            // Only display results for the query still in the box (belt-and-braces;
            // cancel-previous already makes stale results rare).
            if *query == app.draft {
                app.results = rows.clone();
            }
        }
        Event::Message(Msg::Tick(n)) => app.ticks = *n,
        Event::EffectFailed(error) => {
            app.last_error = Some(error.to_string());
        }
        _ => {}
    }

    // App-level key bindings on keys no focused widget consumed. `Ctrl-L` clears
    // the field even while it is focused (TextInput ignores ctrl chords).
    if let Event::Input(input) = update.event() {
        if let Some(k) = input.as_key() {
            if k.key == Key::Char('l') && k.modifiers.ctrl {
                update.widget::<TextInput>(&[key("input")], |state| state.clear());
                app.draft.clear();
                app.results.clear();
            }
            match k.key {
                // Toggle the clock ticker stream on/off.
                Key::Char('t') if !k.modifiers.ctrl => {
                    app.ticking = !app.ticking;
                    if app.ticking {
                        // Start the ticker under the "clock" group so it can be
                        // aborted on demand.
                        update.spawn(Cmd::stream(Ticker::every(Duration::from_secs(1))).group("clock"));
                    } else {
                        // Stop it for good: cancel_group aborts the stream task
                        // without replacing it (the stream-stop primitive).
                        update.spawn(Cmd::cancel_group("clock"));
                    }
                }
                Key::Char('q') | Key::Escape => return ControlFlow::Break(()),
                _ => {}
            }
        }
    }

    ControlFlow::Continue(())
}

/// Declares the input, the results list, and the status/clock/hint lines.
fn view(app: &Fetch, frame: &mut Frame<'_>) {
    let [input_row, list_area, clock_row, status_row, hint_row] = frame.rows([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);

    frame.widget(key("input"), input_row, &TextInput::new().placeholder("Tab, then search…"));
    frame.widget(key("results"), list_area, &SelectionList::new(app.results.clone()));

    let clock = if app.ticking {
        format!("clock: tick {}", app.ticks)
    } else {
        "clock: off (press t)".to_string()
    };
    frame.widget(key("clock"), clock_row, &Text::new(&clock).style(Style::new().fg(Color::CYAN)));

    let status = match &app.last_error {
        Some(error) => format!("error: {error}"),
        None => format!("{} fetches completed", app.completed),
    };
    frame.widget(
        key("status"),
        status_row,
        &Text::new(&status).style(Style::new().fg(Color::GREEN).bold()),
    );

    let hint = Style::new().fg(Color::Indexed(245)).italic();
    frame.widget(
        key("hint"),
        hint_row,
        &Text::new("Tab: focus  type: search  Ctrl-L: clear  t: clock  q: quit").style(hint),
    );
}

/// A simulated ~300ms fetch that returns a few result rows for `query`.
///
/// A real app would `await` a network call here; the sleep stands in for it. The
/// point is the *shape*: an async future producing one message, spawned into the
/// `search` group so a newer keystroke aborts it mid-flight.
fn fake_fetch(query: String) -> Cmd<Msg> {
    Cmd::future(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let rows = if query.trim().is_empty() {
            Vec::new()
        } else {
            (1..=5).map(|n| format!("{query} result {n}")).collect()
        };
        Msg::Results { query, rows }
    })
}

/// A hand-rolled interval stream over [`tokio::time::Interval`] — the ~15-line
/// ticker the slice-6 design calls for, no `tokio-stream` dependency.
///
/// It polls the interval's `poll_tick`, emitting an incrementing count each time
/// it fires. The stream never ends (a clock runs forever); the app stops caring
/// about its ticks when the ticker is toggled off.
struct Ticker {
    interval: tokio::time::Interval,
    count: u64,
}

impl Ticker {
    /// A ticker firing every `period`.
    fn every(period: Duration) -> Self {
        Self { interval: tokio::time::interval(period), count: 0 }
    }
}

impl Stream for Ticker {
    type Item = Msg;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Msg>> {
        let this = self.get_mut();
        match this.interval.poll_tick(cx) {
            Poll::Ready(_) => {
                this.count += 1;
                Poll::Ready(Some(Msg::Tick(this.count)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
