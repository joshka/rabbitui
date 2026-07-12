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
//!   [`Command::stream`] over a hand-rolled 1-second interval, spawned into the
//!   `"clock"` group — whose messages update a live clock line. Toggling it off
//!   spawns a [`Command::cancel_group`] that aborts the stream for good (the
//!   stream-stop primitive, ADR 0005 / slice 7), rather than leaving it running
//!   with its ticks ignored.
//! - **A widget command.** `Ctrl-L` clears the input via
//!   `update.widget::<TextInput>(…, |s| s.clear())`, applied between frames.
//! - **Failures, surfaced.** `Ctrl-E` simulates an *expected* failure (an error
//!   value), shown in a dismissible [`ErrorBanner`] overlay — the recommended
//!   failure UX. Separately, an *unexpected* effect-task panic is contained and
//!   arrives as [`Event::EffectFailed`] (the same handler feeds the banner), so a
//!   bug in an effect never crashes the loop. Expected failures are values; panics
//!   are the safety net — see `docs/design/error-story.md`.
//!
//! Run with `cargo run --example fetch`. Type to search; watch the completed
//! counter lag far behind your keystrokes; press `t` to toggle the clock; `Ctrl-L`
//! to clear; `Ctrl-E` to surface a failure; `~` to toggle the debug **log
//! overlay**; `q` to quit.
//!
//! Note (substrate gap): the input is reached via Tab; while it is focused it
//! consumes printable keys, so app-level `t`/`q`/`~` require Tab-ing focus away
//! first. `Ctrl-L` works while focused (TextInput leaves ctrl chords for the app).
//!
//! # The log overlay (Arc 2B logging seam)
//!
//! This example demonstrates rabbitui's tracing integration. The app supplies a
//! shared [`LogHandle`] to [`App::log_handle`]; the runtime's collector writes
//! every `tracing::info!` / `warn!` into that ring, and `~` toggles a
//! [`LogOverlay`] declared into a `frame.layer` that renders the ring's tail. The
//! `update` emits a couple of traces (a search start, a clock toggle) so the
//! overlay shows real content.

use std::ops::ControlFlow;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_core::Stream;
use rabbitui::App;
use rabbitui::app::{Event, Update};
use rabbitui::effect::Command;
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::{Constraint, center, split_rows};
use rabbitui_core::log::LogHandle;
use rabbitui_core::outcome::Outcome;
use rabbitui_core::theme::Role;
use rabbitui_core::widget::Widget as _;
use rabbitui_widgets::{ErrorBanner, LogOverlay, Panel, SelectionList, Text, TextInput};

/// A message an effect produces, re-entering the loop as [`Event::Message`].
#[derive(Debug, Clone)]
enum Message {
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
    /// The shared log ring the runtime's tracing collector writes into; the
    /// overlay renders its tail. A clone lives in the runtime too (via
    /// [`App::log_handle`]), so both view the same events.
    logs: LogHandle,
    /// Whether the debug log overlay is toggled on (the `~` key).
    show_logs: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Fetch::default();
    // Share the app's log ring with the runtime so the collector's events land
    // where the overlay reads them. `.tracing(true)` forces the collector on even
    // in a release build of the example.
    let logs = app.logs.clone();
    App::new(app, update, view)
        .log_handle(logs)
        .tracing(true)
        .run()
        .await?;
    Ok(())
}

/// Folds one update into the app: spawn searches, toggle the ticker, clear the
/// input, and absorb effect results.
fn update(app: &mut Fetch, update: Update<'_, Message>) -> ControlFlow<()> {
    // A keystroke that changed the input spawns a new debounced fetch. The
    // `group("search")` aborts the previous fetch, so only the last-typed query
    // completes — the cancel-previous guarantee.
    if let Some(Outcome::Changed(value)) = update.outcome_for(&[key("input")]) {
        app.draft = value.clone();
        let query = app.draft.clone();
        tracing::info!(query = %query, "search started");
        update.spawn(fake_fetch(query).group("search"));
    }

    // Dismissing the error banner (Enter/Space/click) clears the failure.
    if update.outcome_for(&[key("errlayer"), key("banner")]) == Some(&Outcome::Dismissed) {
        app.last_error = None;
    }

    // Effect results re-enter here as messages.
    match update.event() {
        Event::Message(Message::Results { query, rows }) => {
            app.completed += 1;
            // Only display results for the query still in the box (belt-and-braces;
            // cancel-previous already makes stale results rare).
            if *query == app.draft {
                app.results = rows.clone();
            }
        }
        Event::Message(Message::Tick(n)) => app.ticks = *n,
        Event::EffectFailed(error) => {
            tracing::warn!(error = %error, "effect failed");
            app.last_error = Some(error.to_string());
        }
        _ => {}
    }

    // App-level key bindings on keys no focused widget consumed. `Ctrl-L` clears
    // the field even while it is focused (TextInput ignores ctrl chords).
    if let Event::Input(input) = update.event()
        && let Some(k) = input.as_key()
    {
        if k.key == Key::Char('l') && k.modifiers.ctrl {
            update.widget::<TextInput>(&[key("input")], |state| state.clear());
            app.draft.clear();
            app.results.clear();
        }
        // Ctrl-E simulates an operation that fails in an expected way: it sets a
        // domain error, surfaced in a dismissible ErrorBanner (the recommended
        // failure UX). Expected failures are error *values*, not panics — see
        // the `EffectFailed` handler above for the panic safety net.
        if k.key == Key::Char('e') && k.modifiers.ctrl {
            app.last_error = Some("could not reach the search backend (simulated)".to_string());
        }
        match k.key {
            // Toggle the clock ticker stream on/off. Guarded: the search
            // input consumes printables while focused (Update::consumed).
            Key::Char('t') if !k.modifiers.ctrl && !update.consumed() => {
                app.ticking = !app.ticking;
                tracing::info!(ticking = app.ticking, "clock toggled");
                if app.ticking {
                    // Start the ticker under the "clock" group so it can be
                    // aborted on demand.
                    update.spawn(
                        Command::stream(Ticker::every(Duration::from_secs(1))).group("clock"),
                    );
                } else {
                    // Stop it for good: cancel_group aborts the stream task
                    // without replacing it (the stream-stop primitive).
                    update.spawn(Command::cancel_group("clock"));
                }
            }
            // Toggle the debug log overlay. Guarded on `!consumed()` so it does
            // not fire while `~` is typed into the focused search field.
            Key::Char('~') if !update.consumed() => {
                app.show_logs = !app.show_logs;
            }
            Key::Char('q') if !update.consumed() => return ControlFlow::Break(()),
            Key::Escape => return ControlFlow::Break(()),
            _ => {}
        }
    }

    ControlFlow::Continue(())
}

/// Declares the input, the results list, and the status/clock/hint lines inside
/// a centered, focused panel.
fn view(app: &Fetch, frame: &mut Frame<'_>) {
    let full = frame.area();
    let width = full.size.width.min(60);
    let height = full.size.height.saturating_sub(4).clamp(12, 26);
    let area = center(full, width, height);
    let panel = Panel::new().title("fetch").padding(1).focused(true);
    frame.widget(key("panel"), area, &panel);

    let inner = Panel::inner(area, &panel);
    let [input_row, list_area, clock_row, status_row, hint_row] = split_rows(
        inner,
        [
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
    );

    frame.widget(
        key("input"),
        input_row,
        &TextInput::new().placeholder("Tab, then search…"),
    );
    frame.widget(
        key("results"),
        list_area,
        &SelectionList::new(app.results.clone()),
    );

    let clock = if app.ticking {
        format!("clock: tick {}", app.ticks)
    } else {
        "clock: off (press t)".to_string()
    };
    frame.widget(
        key("clock"),
        clock_row,
        &Text::new(&clock).role(Role::Accent),
    );

    // Failures surface in the ErrorBanner overlay below, so the status line always
    // reports the completed-fetch count (the cancel-previous proof).
    let status = format!("{} fetches completed", app.completed);
    frame.widget(
        key("status"),
        status_row,
        &Text::new(&status).role(Role::Success),
    );

    frame.widget(
        key("hint"),
        hint_row,
        &Text::new("search   Ctrl-L: clear   Ctrl-E: fail   t/~ (list): clock/logs   Ctrl-C: quit")
            .role(Role::Muted),
    );

    // The debug log overlay: a themed panel over the bottom third of the screen,
    // declared into its own layer so it sits above the app and (per ADR 0003's
    // layer semantics) contains its own input. It renders the shared log ring's
    // tail — the tracing events the update emits.
    if app.show_logs {
        let log_h = full.size.height.saturating_sub(2).clamp(3, 10);
        let log_area = split_rows(full, [Constraint::Fill(1), Constraint::Length(log_h)])[1];
        frame.layer(key("logs"), |overlay| {
            overlay.widget(key("overlay"), log_area, &LogOverlay::new(&app.logs));
        });
    }

    // A contained effect failure surfaces here, in a dismissible ErrorBanner on its
    // own top layer (which captures focus, so Enter/Space dismisses it). Clearing
    // `last_error` on the Dismissed outcome stops declaring it next frame.
    if let Some(error) = &app.last_error {
        let banner = ErrorBanner::new(error).title("Effect failed");
        let width = full.size.width.saturating_sub(4).clamp(10, 50);
        let height = banner.desired_height(&(), width).min(full.size.height);
        let banner_area = center(full, width, height);
        frame.layer(key("errlayer"), |overlay| {
            overlay.widget(key("banner"), banner_area, &banner);
        });
    }
}

/// A simulated ~300ms fetch that returns a few result rows for `query`.
///
/// A real app would `await` a network call here; the sleep stands in for it. The
/// point is the *shape*: an async future producing one message, spawned into the
/// `search` group so a newer keystroke aborts it mid-flight.
fn fake_fetch(query: String) -> Command<Message> {
    Command::future(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let rows = if query.trim().is_empty() {
            Vec::new()
        } else {
            (1..=5).map(|n| format!("{query} result {n}")).collect()
        };
        Message::Results { query, rows }
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
        Self {
            interval: tokio::time::interval(period),
            count: 0,
        }
    }
}

impl Stream for Ticker {
    type Item = Message;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Message>> {
        let this = self.get_mut();
        match this.interval.poll_tick(cx) {
            Poll::Ready(_) => {
                this.count += 1;
                Poll::Ready(Some(Message::Tick(this.count)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
