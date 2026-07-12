//! Flagship end-to-end tests over the real run loop.
//!
//! These drive the **actual** [`Agent::update`]/[`Agent::view`] loop over a
//! [`qwertty::FakeDevice`] via the promoted [`rabbitui::harness::Harness`] pump,
//! parsing the emitted bytes with its `VtScreen`. `TestApp` (the reducer-level
//! harness the slice tests use) never runs the loop, so the three bug classes
//! that motivated the FakeDevice harness only showed on hardware. Each test here
//! reads as a user story and turns one of those classes into a permanent CI guard:
//!
//! 1. [`help_overlay_opens_closes_and_loop_survives`] — the declare-then-focus
//!    panic (a non-focusable overlay must never request focus).
//! 2. [`tool_turn_settles_to_terminal_glyph_in_scrollback`] — the scrollback
//!    freeze (a Tool cell committed at its Pending glyph never settles).
//! 3. [`mode_toggle_leaves_one_tail`] — the inline↔alt toggle duplicating the tail.
//!
//! Determinism follows the existing suite: never a bare sleep — always
//! `wait_for`/`wait_while`/`wait_until` a rendered marker.

use rabbitui::App as _;
use rabbitui::harness::Harness;
use rabbitui_agent::app::{Agent, build_app};
use rabbitui_agent::backend::replay::ReplayBackend;

// Raw input bytes for the chords, each citing its `keymap.rs` binding so a reader
// can trace the byte back to the action without decoding it by hand.
/// `Action::Help`'s works-today alias, Ctrl-G (`0x07`); close with a second press,
/// not Esc (lone-Esc decoding is a live qwertty item).
const HELP: &[u8] = b"\x07";
/// `Action::ToggleMode`, Ctrl-T (`0x14`).
const TOGGLE: &[u8] = b"\x14";
/// `Action::Allow`, a bare `y` (guarded: fires only on a key no widget consumed).
const ALLOW: &[u8] = b"y";
/// `Action::Quit`, Ctrl-C (`0x03`) — quits from anywhere via `global`.
const QUIT: &[u8] = b"\x03";
/// `Action::Send`, a bare Enter — submits the composer draft.
const ENTER: &[u8] = b"\r";

/// A stable substring of the composer's generated key-hint footer
/// (`hint_line` in `app.rs`: "…Ctrl-G: help…"): the "app has started and painted"
/// marker every test waits on first.
const FOOTER: &str = "Ctrl-G: help";

/// Launches the flagship over a `FakeDevice`, driving `backend` through the real
/// run loop. 80×24 matches the `FakeDevice` default (and the modal/overlay layout
/// is sized against it).
fn launch(
    backend: ReplayBackend,
) -> Harness<impl std::future::Future<Output = rabbitui::app::Result<()>>> {
    Harness::launch_with(
        |device| build_app("test-model", Box::new(backend)).run_over_device(device),
        80,
        24,
    )
}

/// Opening the help overlay and closing it again must not panic the loop, and the
/// loop must keep running afterwards.
///
/// The overlay is display-only: it holds no focusable widget, so it must **not**
/// request focus into itself — doing so would fail the declare-then-focus contract
/// and panic the real `update` (the bug this guards). We prove the loop survived
/// by typing into the composer *after* closing help and seeing the echo.
#[tokio::test]
async fn help_overlay_opens_closes_and_loop_survives() {
    // Idle backend: no turn ever fires (we never submit), so the app just sits at
    // the composer.
    let mut app = launch(ReplayBackend::new(Vec::new()));
    assert!(
        app.wait_for(FOOTER).await,
        "the composer footer should paint at startup, got:\n{}",
        app.screen.contents()
    );

    // Open help (Ctrl-G): the overlay's title card appears.
    app.feed(HELP);
    assert!(
        app.wait_for("keys").await,
        "help overlay should open on Ctrl-G, got:\n{}",
        app.screen.contents()
    );

    // Close it with a second Ctrl-G (not Esc): the title card goes away.
    app.feed(HELP);
    assert!(
        app.wait_while("keys").await,
        "help overlay should close on a second Ctrl-G, got:\n{}",
        app.screen.contents()
    );

    // The loop is still alive: a printable lands in the composer and echoes.
    app.feed(b"ping");
    assert!(
        app.wait_for("ping").await,
        "composer should echo input after help closed — the loop survived, got:\n{}",
        app.screen.contents()
    );

    // Clean teardown via the quit chord.
    app.feed(QUIT);
    assert!(app.join().await.is_ok(), "clean quit expected");
}

/// A tool-use turn, once allowed, must settle its Tool cell to the terminal `✓`
/// glyph in committed scrollback — never freeze at the Pending `…` glyph.
///
/// The fixture stops on `tool_use` with a `list_dir(".")` call (chosen because it
/// succeeds against any cwd, so the cell reaches `Ok`/`✓`). The inline engine
/// commits each cell to native scrollback exactly once and cannot rewrite it, so a
/// cell committed while still Pending would strand its `…` glyph there forever —
/// the freeze bug. We assert the committed history shows `✓` and never the frozen
/// `…` for that cell.
#[tokio::test]
async fn tool_turn_settles_to_terminal_glyph_in_scrollback() {
    let backend = ReplayBackend::from_jsonl(include_str!("fixtures/tool_turn.jsonl"))
        .expect("fixture parses");
    let mut app = launch(backend);
    assert!(app.wait_for(FOOTER).await, "app should start");

    // Submit a prompt: this fires the backend turn, which stops on tool_use.
    app.feed(b"list the directory");
    app.feed(ENTER);

    // The confirmation modal comes up with its Allow/Deny affordances.
    assert!(
        app.wait_for("Allow (y)").await,
        "approval modal should open on the tool_use turn, got:\n{}",
        app.screen.contents()
    );
    assert!(
        app.screen.contents().contains("Deny (n)"),
        "the modal shows both affordances, got:\n{}",
        app.screen.contents()
    );

    // Allow it: the tool runs (list_dir succeeds) and the cell settles to Ok.
    app.feed(ALLOW);
    let settled = app
        .wait_until(|screen| {
            screen
                .all_lines()
                .iter()
                .any(|line| line.contains("✓ list_dir"))
        })
        .await;
    assert!(
        settled,
        "the Tool cell should settle to ✓ in scrollback, got:\n{:#?}",
        app.screen.all_lines()
    );
    // The freeze bug's exact shape: a cell committed at its Pending glyph. It must
    // never appear in committed history.
    assert!(
        !app.screen
            .all_lines()
            .iter()
            .any(|line| line.contains("… list_dir")),
        "the Tool cell must not freeze at its Pending glyph, got:\n{:#?}",
        app.screen.all_lines()
    );

    app.feed(QUIT);
    assert!(app.join().await.is_ok(), "clean quit expected");
}

/// Toggling inline → alt-screen browse → inline must leave exactly one live tail:
/// the footer hint appears once, not duplicated.
///
/// The earlier bug left a second copy of the tail chrome after a round-trip
/// through alt-screen. We count the footer hint's occurrences in the visible
/// screen after returning to inline; it must be exactly one.
#[tokio::test]
async fn mode_toggle_leaves_one_tail() {
    let mut app = launch(ReplayBackend::new(Vec::new()));
    assert!(
        app.wait_for(FOOTER).await,
        "app should start in inline mode"
    );

    // Toggle to alt-screen browse: only that mode's status chrome shows this.
    app.feed(TOGGLE);
    assert!(
        app.wait_for("alt-screen browse").await,
        "Ctrl-T should switch to alt-screen browse, got:\n{}",
        app.screen.contents()
    );

    // Toggle back to inline.
    app.feed(TOGGLE);
    assert!(
        app.wait_for("[inline]").await,
        "a second Ctrl-T should return to inline, got:\n{}",
        app.screen.contents()
    );

    // Exactly one live tail: the footer hint is not duplicated.
    let footer_count = app.screen.contents().matches(FOOTER).count();
    assert_eq!(
        footer_count,
        1,
        "the tail should appear exactly once after a mode round-trip, saw {footer_count}:\n{}",
        app.screen.contents()
    );

    app.feed(QUIT);
    assert!(app.join().await.is_ok(), "clean quit expected");
}

// A tiny compile-time anchor that `build_app` returns the flagship `Agent`, so the
// shared `main`/test construction path stays typed as the flagship (not a trait
// object) — if Wave A reshapes the app value, this fails to compile here first.
const _: fn(Box<dyn rabbitui_agent::backend::Backend>) -> Agent = |backend| build_app("m", backend);
