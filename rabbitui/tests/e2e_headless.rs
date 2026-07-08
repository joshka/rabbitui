//! End-to-end tests that drive the **real** [`App::run`] loop headlessly.
//!
//! `TestApp` (in `rabbitui-testing`) drives the reducer, not the run loop, so
//! bug classes that live in the loop — a declare-then-focus panic raised from the
//! real `update` closure, inline-commit timing, mode-switch tail duplication —
//! passed every existing test and only showed on hardware
//! (`docs/design/fakedevice-e2e-harness.md`). These tests run the loop over a
//! [`qwertty::FakeDevice`] socketpair and assert on the bytes it emits, parsed by
//! [`VtScreen`], so those classes become CI-catchable.
//!
//! ## Why a pump, not a spawn
//!
//! The loop keeps a `StateStore` (a `Box<dyn Any>` per widget) live across every
//! `.await`, so its future is `!Send` and cannot go on `tokio::spawn`. Instead the
//! [`Harness`] owns the pinned future and *pumps* it on a current-thread runtime:
//! each step polls the app once (biased) against a short timer, then drains the
//! socket into the screen. Assertions wait for a rendered marker to appear (with a
//! cap) rather than sleeping a fixed amount — deterministic without a real clock.

use std::future::Future;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::time::Duration;

use qwertty::{FakeDevice, FakeTerminal};
use rabbitui::App;
use rabbitui::app::{Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_testing::vt::VtScreen;
use rabbitui_widgets::Text;

/// Drives an app's run loop over a fake device on the current thread.
struct Harness<F: Future<Output = rabbitui::app::Result<()>>> {
    app: Pin<Box<F>>,
    terminal: FakeTerminal,
    screen: VtScreen,
    /// The loop's result once it has exited; `None` while it is still running.
    done: Option<rabbitui::app::Result<()>>,
}

impl<F: Future<Output = rabbitui::app::Result<()>>> Harness<F> {
    /// Advances the app by one step: poll it once (unless already exited), then
    /// drain whatever it wrote into the screen.
    async fn pump_once(&mut self) {
        if self.done.is_none() {
            tokio::select! {
                biased;
                result = self.app.as_mut() => self.done = Some(result),
                () = tokio::time::sleep(Duration::from_millis(2)) => {}
            }
        } else {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let bytes = self.terminal.output().expect("read fake terminal output");
        if !bytes.is_empty() {
            self.screen.feed(&bytes);
        }
    }

    /// Pumps until the rendered screen contains `needle`, or a ~1s cap elapses.
    async fn wait_for(&mut self, needle: &str) -> bool {
        for _ in 0..500 {
            if self.screen.contents().contains(needle) {
                return true;
            }
            self.pump_once().await;
        }
        self.screen.contents().contains(needle)
    }

    /// Feeds raw input bytes the app will read as terminal input.
    fn feed(&mut self, bytes: &[u8]) {
        self.terminal
            .feed_input(bytes)
            .expect("feed fake terminal input");
    }

    /// Pumps until the loop exits, returning its result (or panicking on a cap).
    async fn join(mut self) -> rabbitui::app::Result<()> {
        for _ in 0..500 {
            if let Some(result) = self.done.take() {
                return result;
            }
            self.pump_once().await;
        }
        panic!("app loop did not exit within the cap");
    }
}

/// Builds a harness over an 80x24 fake device (the `FakeDevice` default size).
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
    let (device, terminal) = FakeDevice::open().expect("open fake device");
    let app = App::new(state, update, view);
    Harness {
        app: Box::pin(app.run_over_device(device)),
        terminal,
        screen: VtScreen::new(80, 24),
        done: None,
    }
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
