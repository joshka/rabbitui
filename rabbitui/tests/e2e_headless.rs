//! End-to-end tests that drive the **real** [`App::run`] loop headlessly.
//!
//! `TestApp` (in `rabbitui-testing`) drives the reducer, not the run loop, so
//! bug classes that live in the loop — a declare-then-focus panic raised from the
//! real `update` closure, inline-commit timing, mode-switch tail duplication —
//! passed every existing test and only showed on hardware
//! (`docs/design/fakedevice-e2e-harness.md`). These tests run the loop over a
//! [`qwertty::FakeDevice`] socketpair and assert on the bytes it emits, parsed by
//! a `VtScreen`, so those classes become CI-catchable.
//!
//! The pump machinery itself is the promoted [`rabbitui::harness::Harness`] (behind
//! the `harness` feature, enabled here via the crate's self dev-dep) — its module
//! doc carries the "why a pump, not a spawn" explanation and an example. These
//! tests only supply the app and the assertions.

use std::future::Future;
use std::ops::ControlFlow;

use rabbitui::app::{Event, Update};
use rabbitui::effect::Command;
use rabbitui::harness::Harness;
use rabbitui::{App, from_fn};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_widgets::Text;

/// Builds a harness over an 80x24 fake device (the `FakeDevice` default size)
/// driving a closure app.
fn harness<S, U, V>(
    state: S,
    update: U,
    view: V,
) -> Harness<impl Future<Output = rabbitui::app::Result<()>>>
where
    S: 'static,
    U: FnMut(&mut S, Update<'_>) -> ControlFlow<()> + 'static,
    V: Fn(&S, &mut Frame<'_>) + 'static,
{
    Harness::launch_with(
        |device| from_fn(state, update, view).run_over_device(device),
        80,
        24,
    )
}

/// Builds a harness over a trait-shaped [`App`] (a struct implementing the trait
/// directly), for tests that exercise the defaulted lifecycle hooks
/// ([`App::init`], [`App::global`]) that closure apps cannot override.
fn harness_app<A, M>(app: A) -> Harness<impl Future<Output = rabbitui::app::Result<()>>>
where
    A: App<M> + 'static,
    M: Send + 'static,
{
    Harness::launch_with(|device| app.run_over_device(device), 80, 24)
}

/// State for the marker app: how many `Started` and input events it has seen.
struct Counts {
    started: u32,
    keys: u32,
}

/// An app that renders its `Started`/input counts and quits on `q`. One app
/// exercises the whole seam: startup delivery, input routing into `update`, a
/// repaint per event, and a clean quit teardown over the fake device.
fn counts_app() -> Harness<impl Future<Output = rabbitui::app::Result<()>>> {
    harness(
        Counts {
            started: 0,
            keys: 0,
        },
        |state: &mut Counts, update: Update<'_>| {
            match update.event() {
                Event::Started => state.started += 1,
                Event::Input(input) => {
                    if input.as_key().map(|k| k.key) == Some(rabbitui_core::input::Key::Char('q')) {
                        return ControlFlow::Break(());
                    }
                    state.keys += 1;
                }
                _ => {}
            }
            ControlFlow::Continue(())
        },
        |state: &Counts, frame: &mut Frame<'_>| {
            let area = frame.area();
            let line = format!("s={} k={}", state.started, state.keys);
            frame.widget(key("line"), area, &Text::new(line));
        },
    )
}

/// `Event::Started` fires exactly once, before any input, and the loop paints it.
#[tokio::test]
async fn started_fires_once_before_input() {
    let mut app = counts_app();

    // The startup tick lands before we feed anything: s=1, and no input yet.
    assert!(
        app.wait_for("s=1 k=0").await,
        "expected the startup paint, got:\n{}",
        app.screen.contents()
    );

    // A key routes into `update` and repaints — and Started does NOT fire again.
    app.feed(b"a");
    assert!(
        app.wait_for("s=1 k=1").await,
        "expected the post-input repaint, got:\n{}",
        app.screen.contents()
    );
    assert!(
        !app.screen.contents().contains("s=2"),
        "Started must fire exactly once, got:\n{}",
        app.screen.contents()
    );
}

/// Feeding the quit chord exits the loop cleanly over the fake device — the
/// Break → teardown → leave path that only a real loop exercises.
#[tokio::test]
async fn quit_chord_exits_the_loop_cleanly() {
    let mut app = counts_app();
    assert!(app.wait_for("s=1 k=0").await, "app should have started");

    app.feed(b"q");
    let result = app.join().await;
    assert!(result.is_ok(), "clean quit expected, got: {result:?}");
}

// ---------------------------------------------------------------------------
// Lifecycle-hook e2e tests (trait-shaped apps over the same FakeDevice harness).
// ---------------------------------------------------------------------------

/// The effect message the init app seeds itself with.
#[derive(Debug, Clone)]
enum Msg {
    /// Delivered by the command `App::init` returns — with no input at all.
    Seeded,
}

/// A trait-shaped app whose `init` returns a `Command::future` yielding
/// [`Msg::Seeded`]. Its `view` renders a distinctive marker only once seeded, so
/// the marker appearing proves the init command ran at startup — before any
/// input. Quits on `q`.
#[derive(Default)]
struct InitApp {
    seeded: bool,
}

impl App<Msg> for InitApp {
    fn init(&mut self) -> Command<Msg> {
        // Spawned once at startup, before `Event::Started`. Its message re-enters
        // the loop with no keypress required.
        Command::future(async { Msg::Seeded })
    }

    fn update(&mut self, update: Update<'_, Msg>) -> ControlFlow<()> {
        match update.event() {
            Event::Message(Msg::Seeded) => self.seeded = true,
            Event::Input(input) if input.as_key().map(|k| k.key) == Some(Key::Char('q')) => {
                return ControlFlow::Break(());
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let line = if self.seeded {
            "INIT-SEEDED"
        } else {
            "waiting"
        };
        frame.widget(key("line"), frame.area(), &Text::new(line));
    }
}

/// `App::init`'s command is spawned at startup and its message arrives with no
/// input fed — the self-starting path (dogfood finding #1) exercised end to end.
#[tokio::test]
async fn init_cmd_arrives_before_input() {
    let mut app = harness_app(InitApp::default());

    // No input is ever fed here: the marker can only appear because init spawned
    // its command at startup and the resulting message flipped the flag.
    assert!(
        app.wait_for("INIT-SEEDED").await,
        "init command's message should arrive with no input, got:\n{}",
        app.screen.contents()
    );

    // Quit cleanly to exercise the teardown path.
    app.feed(b"q");
    let result = app.join().await;
    assert!(result.is_ok(), "clean quit expected, got: {result:?}");
}

/// A trait-shaped app whose `update` always early-returns (a "modal" is open), so
/// it can never quit on its own. The quit chord (Ctrl-C) lives in `global`, which
/// runs before `update` for every event — proving the hook fires even when
/// `update` would swallow the event.
struct ModalApp;

impl App for ModalApp {
    fn global(&mut self, update: &Update<'_>) -> ControlFlow<()> {
        // Ctrl-C (raw byte `\x03`) decodes to `Char('c')` + the Ctrl modifier.
        if let Event::Input(input) = update.event()
            && let Some(k) = input.as_key()
            && k.key == Key::Char('c')
            && k.modifiers.ctrl
        {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }

    fn update(&mut self, _update: Update<'_>) -> ControlFlow<()> {
        // The modal is open: bail before reaching any quit branch. `update` alone
        // can never break, so a clean exit must come from `global`.
        ControlFlow::Continue(())
    }

    fn view(&self, frame: &mut Frame<'_>) {
        frame.widget(key("line"), frame.area(), &Text::new("modal open"));
    }
}

/// `global` returning `Break` quits the loop even though `update` early-returns
/// before any quit branch — the app-wide-chord path (ADR 0006) end to end.
#[tokio::test]
async fn global_break_quits_even_when_update_would_return_early() {
    let mut app = harness_app(ModalApp);
    assert!(app.wait_for("modal open").await, "app should have started");

    // Ctrl-C: `update` would swallow it (always Continue), but `global` runs first
    // and breaks, so the loop tears down cleanly.
    app.feed(b"\x03");
    let result = app.join().await;
    assert!(
        result.is_ok(),
        "global-driven quit expected, got: {result:?}"
    );
}
