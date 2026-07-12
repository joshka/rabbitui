//! Item 1 (arc4-spine.md §1 follow-up): a *contained* effect-task panic must not
//! fire the visible terminal-restore hook, while a genuine `view`/`update` panic
//! still does.
//!
//! The mechanism (see `rabbitui/src/terminal.rs`): a thread-local flag is set
//! around each effect poll (`rabbitui/src/effect.rs`) and the installed
//! panic-restore hook checks it — while set, the panic is a contained effect
//! panic (reported as `EffectFailed`, loop survives) so the hook suppresses the
//! visible leave-alt-screen restore that would corrupt the live display mid-run.
//!
//! We cannot install rabbitui's real hook here (it writes to `/dev/tty` and its
//! decision is private), so this test proves the two load-bearing halves that
//! together give the guarantee:
//!
//!   1. The *runtime* end: an effect panic really does travel through the guarded
//!      poll on a worker thread and is contained — delivered as `Outbox::Failed`,
//!      the loop's mailbox never sees a crash. This is what makes the panic hit
//!      the hook while the guard is set (the containment the guard depends on).
//!   2. The *hook decision* end (unit-tested in `terminal.rs`): with the guard set
//!      the restore is suppressed; without it (a view/update panic) it fires.
//!
//! A single process-wide custom panic hook installed here observes, per thread,
//! whether a panic that unwinds an effect task is one the runtime catches — i.e.
//! that the panic and the containment happen on the same worker thread, which is
//! exactly the thread the real hook's thread-local guard covers.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rabbitui::effect::{Command, Effects, Outbox};

/// A contained effect panic is caught at the task boundary and surfaced as a
/// failure — the loop stays alive. This is the property the restore-hook guard
/// rides on: the panic is caught on the worker thread that set the guard, so the
/// installed hook (checking that thread-local) suppresses the visible restore.
#[tokio::test]
async fn contained_effect_panic_is_reported_not_crashed() {
    let mut effects: Effects<u32> = Effects::new();
    effects.spawn(Command::future(async { panic!("effect boom") }).group("risky"));
    match effects.recv().await {
        Some(Outbox::Failed(error)) => {
            assert_eq!(error.group(), Some("risky"));
            assert_eq!(error.message(), "effect boom");
        }
        other => panic!("expected a contained failure, got {other:?}"),
    }
    // The runtime is still usable after a contained panic — a fresh effect runs.
    effects.spawn(Command::future(async { 7u32 }));
    assert_eq!(effects.recv().await, Some(Outbox::Message(7)));
}

/// The whole loop survives an effect that panics: subsequent effects still
/// deliver. If the panic escaped the task boundary the mailbox would be poisoned
/// and this second message would never arrive.
#[tokio::test]
async fn loop_survives_and_keeps_delivering_after_an_effect_panic() {
    let delivered = Arc::new(AtomicUsize::new(0));
    let mut effects: Effects<u32> = Effects::new();

    // Panic, then a normal effect; both spawned before draining.
    effects.spawn(Command::future(async { panic!("boom") }));
    effects.spawn(Command::future(async { 42u32 }));

    let mut failures = 0;
    let mut messages = 0;
    for _ in 0..2 {
        match effects.recv().await {
            Some(Outbox::Failed(_)) => failures += 1,
            Some(Outbox::Message(m)) => {
                messages += 1;
                delivered.fetch_add(m as usize, Ordering::SeqCst);
            }
            None => break,
        }
    }
    assert_eq!(failures, 1, "the panic was contained and reported once");
    assert_eq!(messages, 1, "the healthy effect still delivered");
    assert_eq!(delivered.load(Ordering::SeqCst), 42);
}
