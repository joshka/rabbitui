//! A streaming log-follower: the rabbitui column of the four-framework
//! comparison (`docs/plans/arc5-field.md` item 3).
//!
//! A simulated log source pushes lines over time; a filter [`TextInput`] narrows
//! the visible list; Tab moves focus between the filter and the list; and
//! selecting a line opens a detail modal on a [`Frame::layer`] showing its full
//! record. It exercises the four things the field report says differentiate TUI
//! frameworks:
//!
//! - **Streaming** — a [`Cmd::stream`] timer (like the flagship's spinner) emits
//!   a new [`LogEntry`] every ~700ms, appended to the app's owned log.
//! - **A filter input** — a [`TextInput`]; its `Changed` outcome updates the
//!   filter, and the visible list is recomputed each frame (case-insensitive
//!   substring over level + message).
//! - **Focus** — Tab / Shift-Tab cycle focus between the filter and the list
//!   (the runtime drives this on unconsumed Tab). The focused region's panel
//!   border highlights.
//! - **A detail modal** — Enter (or a click) on a selected row opens a centered
//!   modal on its own z-layer showing the entry's timestamp, level, target, and
//!   full message; Esc or Ctrl-D closes it.
//!
//! Run with `cd comparisons/rabbitui && cargo run`. Tab to the list, arrow to a
//! line, Enter to inspect it, Esc to close; type in the filter to narrow;
//! Ctrl-C or `q` (list focused) to quit.
//!
//! # Alt-screen, not inline
//!
//! This is a *browse* app: you scroll a growing list and open modals over it, so
//! it wants the full viewport as a stable canvas — [`Mode::AltScreen`] (the
//! default). The inline/scrollback question the field report raises is answered
//! deliberately: a log *follower* you filter and inspect is an alt-screen app,
//! whereas a log *emitter* that commits lines to native scrollback is the
//! inline case the `stream` example covers. Noted as a design axis, not a gap.

use std::collections::VecDeque;
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
use rabbitui_core::layout::{Constraint, center, split_rows};
use rabbitui_core::outcome::Outcome;
use rabbitui_core::theme::Role;
use rabbitui_widgets::{Panel, SelectionList, Text, TextInput};

/// The most log entries the app retains. A follower keeps a bounded live window,
/// not an unbounded history — the source runs forever, so an unbounded `Vec`
/// would grow without limit.
const MAX_ENTRIES: usize = 500;

/// The severity of a log line. Ordered for a stable, tiny domain; each maps to a
/// theme [`Role`] so the list re-skins with the theme rather than hard-coding
/// colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    /// The fixed-width label shown in a row (`INFO `, `WARN `, …).
    fn label(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        }
    }

    /// The theme role a row at this level paints in.
    fn role(self) -> Role {
        match self {
            Level::Debug => Role::Muted,
            Level::Info => Role::Text,
            Level::Warn => Role::Warning,
            Level::Error => Role::Danger,
        }
    }
}

/// One simulated log record: everything the list row and the detail modal show.
#[derive(Debug, Clone)]
struct LogEntry {
    /// A monotonic sequence number, standing in for a timestamp.
    seq: u64,
    level: Level,
    /// The emitting subsystem (a `tracing` target analogue).
    target: &'static str,
    message: String,
}

impl LogEntry {
    /// The one-line list rendering: `#seq LEVEL target  message`.
    fn row(&self) -> String {
        format!(
            "#{:<4} {} {:<9} {}",
            self.seq,
            self.level.label(),
            self.target,
            self.message
        )
    }

    /// Whether this entry matches a lowercased filter needle (empty ⇒ matches
    /// all). Matches over the level label, target, and message so `error` or a
    /// subsystem name both narrow the list.
    fn matches(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        self.level.label().to_ascii_lowercase().contains(needle)
            || self.target.to_ascii_lowercase().contains(needle)
            || self.message.to_ascii_lowercase().contains(needle)
    }
}

/// A message an effect produces, re-entering the loop as [`Event::Message`].
#[derive(Debug, Clone)]
enum Msg {
    /// The log source emitted a new entry.
    Line(LogEntry),
}

/// Which region currently holds focus. Tracked in app state because the *view*
/// has no way to read the framework's focus verdict (see the friction note:
/// `Frame` exposes no `is_focused`), yet the panels want to highlight the
/// focused region. Updated from `update.is_focused(...)` on every event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Filter,
    List,
}

/// The app's owned state.
struct App_ {
    /// The bounded live window of received entries, oldest first.
    entries: VecDeque<LogEntry>,
    /// The current filter draft, tracked from the input's `Changed` outcomes.
    filter: String,
    /// Whether the detail modal is open, and if so which *entry seq* it shows.
    /// Storing the seq (not the visible index) keeps the modal pinned to its
    /// entry as new lines arrive and shift indices underneath it.
    detail: Option<u64>,
    /// Whether a focus request into the modal is still owed (set when it opens,
    /// cleared once honored — the form.rs declare-then-focus handshake).
    focus_modal: bool,
    /// Whether the source is paused (Ctrl-P). A paused follower stops appending
    /// so you can read without the list moving under you.
    paused: bool,
    /// The last-known focused region, mirrored from `update.is_focused` so the
    /// view can highlight the focused panel.
    focus: Focus,
}

impl Default for App_ {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            filter: String::new(),
            detail: None,
            focus_modal: false,
            paused: false,
            focus: Focus::Filter,
        }
    }
}

impl App_ {
    /// The entries currently passing the filter, oldest first, cloned for the
    /// list source. (The list borrows a `Vec<String>`; see the friction note on
    /// rebuilding this every frame.)
    fn visible(&self) -> Vec<&LogEntry> {
        let needle = self.filter.trim().to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|entry| entry.matches(&needle))
            .collect()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    App::new(App_::default(), update, view)
        .mouse(true)
        .run()
        .await?;
    Ok(())
}

/// Folds one update into the app.
fn update(app: &mut App_, update: Update<'_, Msg>) -> ControlFlow<()> {
    // Start the log source at launch via the one-shot `Event::Started` hook
    // (dogfood finding #1) — no lazy `started` flag, no "press a key to begin".
    if matches!(update.event(), Event::Started) {
        update.spawn(Cmd::stream(LogSource::new()).group("source"));
    }

    // Global chords, checked FIRST so an early `return` in a later branch (the
    // modal captures input and returns) can never strand them — dogfood finding
    // #7. Ctrl-C quits from anywhere; text inputs pass it through, so it works
    // while the filter is focused. Checking here once replaces the copy that
    // previously had to live in both the modal branch and the base bindings.
    if let Event::Input(input) = update.event() {
        if let Some(k) = input.as_key() {
            if k.key == Key::Char('c') && k.modifiers.ctrl {
                return ControlFlow::Break(());
            }
        }
    }

    // Mirror the framework's focus verdict into app state so the view can
    // highlight the focused region (the view itself cannot read focus).
    if update.is_focused(&[key("filter")]) {
        app.focus = Focus::Filter;
    } else if update.is_focused(&[key("list")]) {
        app.focus = Focus::List;
    }

    // A new streamed line: append, bound the window, and (if the modal is open on
    // an entry that just fell off the front) close it rather than dangle. While
    // paused, the line is dropped on the floor (the source keeps ticking; a
    // cancel-then-respawn would also work but this keeps the seq monotonic).
    if let Event::Message(Msg::Line(entry)) = update.event() {
        if !app.paused {
            app.entries.push_back(entry.clone());
            while app.entries.len() > MAX_ENTRIES {
                let dropped = app.entries.pop_front();
                if let (Some(dropped), Some(open)) = (dropped, app.detail) {
                    if dropped.seq == open {
                        app.detail = None;
                    }
                }
            }
        }
    }

    // Track the filter draft on every edit.
    if let Some(Outcome::Changed(value)) = update.outcome_for(&[key("filter")]) {
        app.filter = value.clone();
        // A narrower filter can strand the selection past the new end; reset the
        // widget's own selection to the top so the highlight is always on a
        // visible row. The list now renders its own empty state and stays
        // declared under `key("list")` even with zero matches, so this command
        // always lands — no is-empty guard needed (the finding-#4 footgun is gone).
        update.widget::<SelectionList<Vec<String>>>(&[key("list")], |state| state.select(0));
    }

    if app.detail.is_some() {
        // Modal is open: Esc or Ctrl-D closes it (Ctrl-C already handled globally
        // above). Everything beneath is inert because the layer captures input
        // (ADR 0003).
        if let Event::Input(input) = update.event() {
            if let Some(k) = input.as_key() {
                let close = k.key == Key::Escape || (k.key == Key::Char('d') && k.modifiers.ctrl);
                if close {
                    app.detail = None;
                }
            }
        }
        // Honor the one-shot focus request into the modal's close button so the
        // overlay actually holds focus (a display-only layer takes none).
        if app.focus_modal {
            update.focus(&[key("modal"), key("close")]);
            app.focus_modal = false;
        }
        // The modal's Close button (Enter/Space/click) also closes.
        if update.outcome_for(&[key("modal"), key("close")]) == Some(&Outcome::Activated) {
            app.detail = None;
        }
        return ControlFlow::Continue(());
    }

    // Base view: Enter (or a click) on the list activates the selected row and
    // opens its detail modal. The selected index is read straight from the list's
    // own state (dogfood finding #2 — no app-side mirror), resolved against the
    // current filtered view.
    if update.outcome_for(&[key("list")]) == Some(&Outcome::Activated) {
        let selected = update
            .widget_state::<SelectionList<Vec<String>>>(&[key("list")])
            .map_or(0, |state| state.selected());
        if let Some(entry) = app.visible().into_iter().nth(selected) {
            app.detail = Some(entry.seq);
            app.focus_modal = true;
        }
    }

    // App-level key bindings, on keys no focused widget consumed. The filter
    // input eats printables while focused, so app printable bindings are
    // `consumed()`-guarded. (Ctrl-C quit is handled once, globally, at the top.)
    if let Event::Input(input) = update.event() {
        if let Some(k) = input.as_key() {
            // Ctrl-P toggles the source pause (works while the filter is focused).
            if k.key == Key::Char('p') && k.modifiers.ctrl {
                app.paused = !app.paused;
            }
            match k.key {
                Key::Char('q') if !k.modifiers.ctrl && !update.consumed() => {
                    return ControlFlow::Break(());
                }
                _ => {}
            }
        }
    }

    ControlFlow::Continue(())
}

/// Declares the whole view: a filter row, the log list panel, a status/hint
/// footer, and — when open — the detail modal layer.
fn view(app: &App_, frame: &mut Frame<'_>) {
    let [filter_area, list_area, status_row, hint_row] = frame.rows([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);

    // Focus is mirrored into app state (the view cannot read it from the frame).
    let filter_focused = app.focus == Focus::Filter;
    let list_focused = app.focus == Focus::List;

    // Filter row: a label plus the input, side by side.
    let [label_col, input_col] =
        split_rows_horizontal(filter_area, [Constraint::Length(9), Constraint::Fill(1)]);
    frame.widget(
        key("filter_label"),
        label_col,
        &Text::new("Filter: ").role(if filter_focused {
            Role::Accent
        } else {
            Role::Muted
        }),
    );
    frame.widget(
        key("filter"),
        input_col,
        &TextInput::new().placeholder("type to filter (Tab to switch focus)…"),
    );

    // The log list, inside a focus-aware panel. The panel is chrome (never
    // focusable); it reflects the *list's* focus so the border highlights when
    // the list holds focus.
    let visible = app.visible();
    let count = format!(" logs ({}/{}) ", visible.len(), app.entries.len());
    let panel = Panel::new().title(&count).padding(1).focused(list_focused);
    frame.widget(key("list_panel"), list_area, &panel);
    let inner = Panel::inner(list_area, &panel);

    // The list is backed by a lazy source that borrows `visible` and formats only
    // the painted rows — no per-frame `Vec<String>`. It renders its own empty state
    // (built-in `empty_text`), so it always stays declared under `key("list")`:
    // no `key("empty")` swap, so focus and the deferred `select(0)` command never
    // hit an absent widget.
    let empty = if app.entries.is_empty() {
        "waiting for logs…"
    } else {
        "no lines match the filter"
    };
    let source = rabbitui_widgets::rows_with(&visible, |entry| entry.row());
    frame.widget(
        key("list"),
        inner,
        &SelectionList::new(source).empty_text(empty),
    );

    // Status line: the source state and selection, in a role that reads the state.
    let (status, status_role) = if app.paused {
        (
            "source paused (Ctrl-P to resume)".to_string(),
            Role::Warning,
        )
    } else {
        (
            format!("streaming — {} received", app.entries.len()),
            Role::Success,
        )
    };
    frame.widget(
        key("status"),
        status_row,
        &Text::new(&status).role(status_role),
    );

    frame.widget(
        key("hint"),
        hint_row,
        &Text::new("Tab: focus   ↑↓: select   Enter: detail   Ctrl-P: pause   Ctrl-C/q: quit")
            .role(Role::Muted),
    );

    // The detail modal, on its own z-layer over the list (the form.rs pattern).
    // While declared, Tab cycles only its Close button and clicks over it never
    // reach the base beneath.
    if let Some(seq) = app.detail {
        if let Some(entry) = app.entries.iter().find(|entry| entry.seq == seq) {
            view_detail_modal(frame, entry);
        }
    }
}

/// Declares the detail modal for `entry` on a centered, focused layer.
fn view_detail_modal(frame: &mut Frame<'_>, entry: &LogEntry) {
    let full = frame.area();
    let width = full.size.width.saturating_sub(6).clamp(20, 72);
    let height = 10u16.min(full.size.height);
    let modal_area = center(full, width, height);

    frame.layer(key("modal"), |modal| {
        let modal_panel = Panel::new().title(" log detail ").padding(1).focused(true);
        modal.widget(key("bg"), modal_area, &modal_panel);
        let inner = Panel::inner(modal_area, &modal_panel);

        let [
            seq_row,
            level_row,
            target_row,
            _gap,
            msg_label,
            msg_row,
            _spacer,
            close_row,
        ] = split_rows(
            inner,
            [
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ],
        );

        modal.widget(
            key("d_seq"),
            seq_row,
            &Text::new(format!("seq:    #{}", entry.seq)).role(Role::Text),
        );
        modal.widget(
            key("d_level"),
            level_row,
            &Text::new(format!("level:  {}", entry.level.label().trim())).role(entry.level.role()),
        );
        modal.widget(
            key("d_target"),
            target_row,
            &Text::new(format!("target: {}", entry.target)).role(Role::Accent),
        );
        modal.widget(
            key("msg_label"),
            msg_label,
            &Text::new("message:").role(Role::Muted),
        );
        modal.widget(
            key("d_msg"),
            msg_row,
            &Text::new(&entry.message).role(Role::Text).wrap(true),
        );
        modal.widget(key("close"), close_row, &CloseButton);
    });
}

/// A focusable "Close" affordance for the modal: the modal must hold a focusable
/// widget or the layer takes no focus (the flagship's lesson — a Panel is not
/// focusable). Reuses [`Role::Highlight`] when focused. Kept tiny inline rather
/// than pulling the whole `Button` styling contract, since it only needs
/// Enter/Space/click → close.
struct CloseButton;

impl rabbitui_core::widget::Widget for CloseButton {
    type State = ();

    fn render(&self, _state: &mut (), ctx: &mut rabbitui_core::widget::RenderCtx<'_>) {
        ctx.focusable(true);
        let role = if ctx.is_focused() {
            Role::Highlight
        } else {
            Role::Accent
        };
        ctx.set_string(
            rabbitui_core::geometry::Position::ORIGIN,
            "[ Close (Esc) ]",
            ctx.style(role),
        );
    }

    fn handle(
        _state: &mut (),
        event: &rabbitui_core::input::InputEvent,
        ctx: &mut rabbitui_core::widget::HandleCtx<'_>,
    ) -> rabbitui_core::widget::Handled {
        use rabbitui_core::input::{Key, MouseButton, MouseKind};
        use rabbitui_core::widget::Handled;
        if let Some(mouse) = event.as_mouse() {
            if mouse.button == MouseButton::Left && mouse.kind == MouseKind::Down {
                ctx.emit(Outcome::Activated);
                return Handled::Yes;
            }
            return Handled::No;
        }
        let Some(k) = event.as_key() else {
            return Handled::No;
        };
        match k.key {
            Key::Enter | Key::Char(' ') => {
                ctx.emit(Outcome::Activated);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

/// A horizontal split — [`split_rows`] with the axis swapped. rabbitui exposes
/// `split_columns`; this wraps it so the two call sites read consistently. (The
/// name is a local shim; see the friction note.)
fn split_rows_horizontal<const N: usize>(
    area: rabbitui_core::geometry::Rect,
    constraints: [Constraint; N],
) -> [rabbitui_core::geometry::Rect; N] {
    rabbitui_core::layout::split_columns(area, constraints)
}

/// The simulated log source: a stream that emits a new [`LogEntry`] every
/// ~700ms, cycling through a small script of realistic-looking lines. This is
/// the streaming primitive the field report calls for — a `Cmd::stream` timer,
/// exactly like the flagship's spinner ticker.
struct LogSource {
    interval: tokio::time::Interval,
    seq: u64,
}

impl LogSource {
    fn new() -> Self {
        Self {
            interval: tokio::time::interval(Duration::from_millis(700)),
            seq: 0,
        }
    }

    /// The next scripted entry for sequence `seq` (deterministic, so the demo is
    /// reproducible frame to frame).
    fn entry(seq: u64) -> LogEntry {
        // A small rotating script: enough variety to make the filter and the
        // level roles visibly do something.
        const SCRIPT: &[(Level, &str, &str)] = &[
            (Level::Info, "http", "GET /api/logs 200 in 12ms"),
            (Level::Debug, "cache", "hit for key user:42 profile"),
            (Level::Info, "auth", "session refreshed for user 42"),
            (
                Level::Warn,
                "http",
                "slow response: GET /api/report took 1420ms",
            ),
            (Level::Info, "worker", "job queued: reindex batch 7"),
            (
                Level::Error,
                "db",
                "connection reset by peer; retrying (1/3)",
            ),
            (Level::Debug, "cache", "evicted 12 stale entries"),
            (Level::Info, "worker", "job done: reindex batch 7 in 3.1s"),
            (
                Level::Warn,
                "auth",
                "rate limit near threshold for 10.0.0.7",
            ),
            (Level::Info, "http", "POST /api/logs 201 in 8ms"),
        ];
        let (level, target, message) = SCRIPT[(seq as usize) % SCRIPT.len()];
        LogEntry {
            seq,
            level,
            target,
            message: message.to_string(),
        }
    }
}

impl Stream for LogSource {
    type Item = Msg;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Msg>> {
        let this = self.get_mut();
        match this.interval.poll_tick(cx) {
            Poll::Ready(_) => {
                let entry = LogSource::entry(this.seq);
                this.seq += 1;
                Poll::Ready(Some(Msg::Line(entry)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(seq: u64, level: Level, target: &'static str, message: &str) -> LogEntry {
        LogEntry {
            seq,
            level,
            target,
            message: message.to_string(),
        }
    }

    #[test]
    fn empty_filter_matches_everything() {
        let e = entry(1, Level::Info, "http", "GET /api");
        assert!(e.matches(""));
    }

    #[test]
    fn filter_matches_message_case_insensitively() {
        let e = entry(1, Level::Info, "http", "GET /API/logs");
        assert!(e.matches("api"));
        assert!(!e.matches("post"));
    }

    #[test]
    fn filter_matches_level_and_target() {
        let e = entry(1, Level::Error, "db", "connection reset");
        assert!(e.matches("error"));
        assert!(e.matches("db"));
    }

    #[test]
    fn visible_narrows_to_matching_entries() {
        let mut app = App_::default();
        app.entries
            .push_back(entry(1, Level::Info, "http", "GET /api"));
        app.entries.push_back(entry(2, Level::Error, "db", "reset"));
        app.entries
            .push_back(entry(3, Level::Info, "http", "POST /api"));
        app.filter = "http".to_string();
        let seqs: Vec<u64> = app.visible().iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![1, 3]);
    }

    #[test]
    fn visible_with_blank_filter_is_all_in_order() {
        let mut app = App_::default();
        for i in 0..5 {
            app.entries.push_back(entry(i, Level::Debug, "cache", "x"));
        }
        app.filter = "   ".to_string();
        assert_eq!(app.visible().len(), 5);
        assert_eq!(app.visible()[0].seq, 0);
    }

    #[test]
    fn row_format_is_stable() {
        let e = entry(7, Level::Warn, "http", "slow");
        assert_eq!(e.row(), "#7    WARN  http      slow");
    }

    #[test]
    fn scripted_source_cycles_levels() {
        // The script rotates, so the same modulo index yields the same entry.
        let a = LogSource::entry(0);
        let b = LogSource::entry(10);
        assert_eq!(a.target, b.target);
        assert_eq!(a.level, b.level);
    }
}
