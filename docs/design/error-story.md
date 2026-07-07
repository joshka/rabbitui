# When things fail — the error story

How a rabbitui app handles failure, across three layers: bugs (let them crash), expected failures
(error values surfaced in the UI), and the effect safety net (contained panics). Arc 4 item 1.

## 1. Bugs in `view` / `update`: let them crash

A panic in your `view` or `update` is a bug, and the right response is to crash — loudly. rabbitui
does **not** `catch_unwind` your app code: catching a bug leaves the app running with corrupted state
assumptions and hides the defect. What rabbitui guarantees instead is that a crash never strands the
terminal: the panic-restore hook (installed in `terminal.rs`) restores cooked mode and the primary
screen on every unwind path, so a panic drops the user back to a clean shell with a readable
backtrace. Fix the bug; don't swallow it.

## 2. Expected failures are error _values_, not panics

A network call that times out, a file that isn't there, an API that returns 500 — these are not bugs,
they are outcomes. Model them as data: give your message type an error variant (or carry a
`Result` into it) and handle it in `update` like any other message.

```rust
enum Msg {
    Loaded(Data),
    Failed(String), // an expected failure, carried as a value
}

// in the effect:
Cmd::future(async move {
    match fetch().await {
        Ok(data) => Msg::Loaded(data),
        Err(error) => Msg::Failed(error.to_string()),
    }
})

// in update:
if let Event::Message(Msg::Failed(error)) = update.event() {
    app.last_error = Some(error.clone());
}
```

Then surface `app.last_error` in the UI. The recommended surface is the [`ErrorBanner`] widget on a
top layer (see §4). This is the clean path — no stderr noise, no terminal disruption, fully under
your control, dismissible. `examples/fetch.rs` demonstrates it (`Ctrl-E`).

## 3. The effect safety net: contained panics → `Event::EffectFailed`

Despite §2, an effect task might still _panic_ — a genuine bug in the effect's own code, an
`unwrap` that shouldn't have been there. rabbitui contains it: the effect runner catches the panic
and delivers it to `update` as `Event::EffectFailed(EffectError)` (carrying the effect's group and a
message) rather than letting it unwind the whole loop. So one buggy effect does not take down the
app. Handle it the same way as an expected failure — record it, surface it, keep going:

```rust
if let Event::EffectFailed(error) = update.event() {
    app.last_error = Some(error.to_string());
}
```

This is a **safety net for bugs**, not a control-flow channel — reach for §2's error values for
anything you expect. Two reasons:

- **It is noisy.** A panic fires Rust's default panic hook first, which prints the panic message and
  backtrace to stderr — over your UI. Contained or not, the user sees the panic text.
- **Known limitation (finding, 2026-07-07): a contained effect panic still fires the terminal-restore
  hook.** The panic-restore hook is global, so it runs for a caught effect-task panic too, writing
  leave-alt-screen bytes to the tty _while the app is still running_ — which drops the app out of the
  alternate screen and corrupts the live display until the next full repaint. The loop survives and
  the `ErrorBanner` still renders, but the screen is disturbed. **Follow-up (Arc 4):** guard the
  restore hook against effect-task panics (a thread-local set around the effect poll that the hook
  checks), so the safety net is clean. Until then: prefer error values (§2); reserve panics for bugs
  you want to see and fix.

## 4. The `ErrorBanner` widget

[`ErrorBanner`](../../rabbitui-widgets/src/error_banner.rs) is the recommended failure surface: an
opaque, `Danger`-bordered box with the title in its top border, a word-wrapped message, and a
dismiss hint in its bottom border. Declare it on a [`Frame::layer`] (so it overlays the app and
captures focus); it is focusable and emits [`Outcome::Dismissed`] on Enter, Space, or a click. Clear
your error state in response and it simply is not declared next frame.

```rust
if let Some(error) = &app.last_error {
    let banner = ErrorBanner::new(error).title("Something went wrong");
    let width = area.size.width.saturating_sub(4).clamp(10, 50);
    let height = banner.desired_height(&(), width).min(area.size.height);
    frame.layer(key("errlayer"), |overlay| {
        overlay.widget(key("banner"), center(area, width, height), &banner);
    });
}
// in update:
if update.outcome_for(&[key("errlayer"), key("banner")]) == Some(&Outcome::Dismissed) {
    app.last_error = None;
}
```

## Summary

| Failure kind                         | Response                                                                                  |
| ------------------------------------ | ----------------------------------------------------------------------------------------- |
| Bug in `view`/`update`               | Let it crash; the restore hook cleans the terminal. Don't `catch_unwind`.                 |
| Expected failure (network, I/O, API) | An error _value_ in a message; surface in an `ErrorBanner`.                               |
| Bug in an effect task                | Contained as `Event::EffectFailed`; surface it, keep going. Noisy — not for control flow. |

[`ErrorBanner`]: ../../rabbitui-widgets/src/error_banner.rs
[`Frame::layer`]: ../../rabbitui-core/src/frame.rs
[`Outcome::Dismissed`]: ../../rabbitui-core/src/outcome.rs
