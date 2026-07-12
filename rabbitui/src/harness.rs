//! A headless pump harness that drives an app's **real** [`App::run`](crate::App::run)
//! loop over a [`qwertty::FakeDevice`] socketpair, parsing the bytes it emits with
//! a [`VtScreen`] so tests can assert on the screen a terminal would show.
//!
//! [`rabbitui_testing::TestApp`] drives the reducer, not the run loop, so bug
//! classes that live in the loop — a declare-then-focus panic raised from the real
//! `update`, inline-commit timing, mode-switch tail duplication — pass every
//! reducer test and only show on hardware
//! (`docs/design/fakedevice-e2e-harness.md`). This harness runs the loop itself,
//! so those classes become CI-catchable. It is generic over the app's run future
//! (not its message type or shape), so closure apps, trait apps, and the flagship
//! all drive through it identically.
//!
//! Behind the `harness` cargo feature (it pulls in `rabbitui-testing`); enable it
//! in a consumer's dev-dependencies to reach the pump from another crate's tests:
//!
//! ```toml
//! [dev-dependencies]
//! rabbitui = { workspace = true, features = ["harness"] }
//! ```
//!
//! ## Why a pump, not a spawn
//!
//! The loop keeps a `StateStore` (a `Box<dyn Any>` per widget) live across every
//! `.await`, so its future is `!Send` and cannot go on `tokio::spawn`. Instead the
//! [`Harness`] owns the pinned future and *pumps* it on a current-thread runtime:
//! each step polls the app once (biased) against a short timer, then drains the
//! socket into the screen. Assertions wait for a rendered marker to appear (with a
//! cap) rather than sleeping a fixed amount — deterministic without a real clock.
//!
//! ## Example
//!
//! ```no_run
//! use std::future::Future;
//!
//! use rabbitui::App;
//! use rabbitui::harness::Harness;
//!
//! # async fn demo(app: impl App + 'static) {
//! // `launch_with` opens the FakeDevice pair internally and hands the device to
//! // the builder, which returns the run future — so the device never escapes.
//! let mut harness = Harness::launch_with(|device| app.run_over_device(device), 80, 24);
//! assert!(harness.wait_for("some rendered marker").await);
//! harness.feed(b"q");
//! let _ = harness.join().await;
//! # }
//! ```

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use qwertty::{FakeDevice, FakeTerminal};
use rabbitui_testing::vt::VtScreen;

use crate::app::Result;

/// The number of pump steps a `wait_*` helper polls before giving up.
const WAIT_CAP: usize = 500;
/// How long each pump step lets the app future run before draining output.
const TICK: Duration = Duration::from_millis(2);

/// Drives an app's run loop over a [`FakeDevice`] on the current thread.
///
/// Generic over the app's **run future** `F` — not its message type or its shape
/// — so it holds only the pinned future and stays agnostic to how the app was
/// built. Construct it with [`launch_with`](Self::launch_with).
pub struct Harness<F: Future<Output = Result<()>>> {
    /// The app's pinned run future, pumped one poll at a time.
    app: Pin<Box<F>>,
    /// The test side of the socketpair: reads the bytes the app wrote.
    terminal: FakeTerminal,
    /// The parsed screen (visible tail plus scrollback) for assertions.
    pub screen: VtScreen,
    /// The loop's result once it has exited; `None` while it is still running.
    done: Option<Result<()>>,
}

impl<F: Future<Output = Result<()>>> Harness<F> {
    /// Opens a [`FakeDevice`] pair, hands the device to `build` (which returns the
    /// app's run future), and wraps it over a `cols`×`rows` [`VtScreen`].
    ///
    /// The device is created inside so it never escapes the harness: the flagship
    /// passes `|device| build_app(backend).run_over_device(device)`. `cols`/`rows`
    /// must match the [`FakeDevice`]'s reported size (80×24 by default).
    ///
    /// # Panics
    ///
    /// Panics if the fake device pair cannot be opened.
    pub fn launch_with(build: impl FnOnce(FakeDevice) -> F, cols: u16, rows: u16) -> Self {
        let (device, terminal) = FakeDevice::open().expect("open fake device");
        Self {
            app: Box::pin(build(device)),
            terminal,
            screen: VtScreen::new(cols, rows),
            done: None,
        }
    }

    /// Advances the app by one step: poll it once (unless already exited), then
    /// drain whatever it wrote into the screen.
    pub async fn pump_once(&mut self) {
        if self.done.is_none() {
            tokio::select! {
                biased;
                result = self.app.as_mut() => self.done = Some(result),
                () = tokio::time::sleep(TICK) => {}
            }
        } else {
            tokio::time::sleep(TICK).await;
        }
        let bytes = self.terminal.output().expect("read fake terminal output");
        if !bytes.is_empty() {
            self.screen.feed(&bytes);
        }
    }

    /// Pumps until the **visible** screen contains `needle`, or the cap elapses.
    ///
    /// Returns whether it appeared. Asserts against [`VtScreen::contents`] (the
    /// live tail), not scrollback — use [`wait_until`](Self::wait_until) with
    /// [`VtScreen::all_lines`] to wait on committed history.
    pub async fn wait_for(&mut self, needle: &str) -> bool {
        self.wait_until(|screen| screen.contents().contains(needle))
            .await
    }

    /// Pumps until the **visible** screen no longer contains `needle` (the inverse
    /// of [`wait_for`](Self::wait_for)), or the cap elapses.
    ///
    /// Returns whether it is gone — for asserting an overlay closed or a spinner
    /// settled.
    pub async fn wait_while(&mut self, needle: &str) -> bool {
        self.wait_until(|screen| !screen.contents().contains(needle))
            .await
    }

    /// Pumps until `predicate` holds against the screen, or the cap elapses.
    ///
    /// The general form behind [`wait_for`](Self::wait_for)/[`wait_while`](Self::wait_while):
    /// the predicate takes `&mut VtScreen` so it can read scrollback via
    /// [`VtScreen::all_lines`] (which needs `&mut`). Returns whether it held.
    pub async fn wait_until(&mut self, mut predicate: impl FnMut(&mut VtScreen) -> bool) -> bool {
        for _ in 0..WAIT_CAP {
            if predicate(&mut self.screen) {
                return true;
            }
            self.pump_once().await;
        }
        predicate(&mut self.screen)
    }

    /// Feeds raw input bytes the app will read as terminal input.
    ///
    /// # Panics
    ///
    /// Panics if the bytes cannot be written to the fake terminal.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.terminal
            .feed_input(bytes)
            .expect("feed fake terminal input");
    }

    /// Pumps until the loop exits, returning its result.
    ///
    /// # Panics
    ///
    /// Panics if the loop does not exit within the pump cap.
    pub async fn join(mut self) -> Result<()> {
        for _ in 0..WAIT_CAP {
            if let Some(result) = self.done.take() {
                return result;
            }
            self.pump_once().await;
        }
        panic!("app loop did not exit within the cap");
    }
}
