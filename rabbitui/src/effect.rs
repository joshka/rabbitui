//! Async effects: commands and the runtime that runs them (ADR 0005).
//!
//! ADR 0005 makes effects **commands only** — a [`Cmd<M>`] is a future or a
//! stream the runtime spawns, whose messages re-enter the one serialized
//! `update` as [`Event::Message`](crate::app::Event::Message). There is no
//! subscription primitive: a recurring timer is a command that re-arms, a
//! long-lived source is a [`Cmd::stream`] that yields (Bubble Tea `ade8203c`).
//!
//! The runtime is an [`Effects<M>`] struct, deliberately **separable from the
//! render loop**: it owns the spawn tables, the group-abort registry, and the
//! result mailbox, and nothing in it touches a terminal. The event loop
//! (`crate::app::run`) holds one and calls [`spawn`](Effects::spawn) /
//! [`recv`](Effects::recv); the unit tests drive the same `Effects` under
//! `#[tokio::test]` without a tty, so async semantics are tested below the loop
//! where they are pure (ADR 0005's "the async boundary lives at the edges").
//!
//! # Groups: cancel-previous
//!
//! [`Cmd::group`] tags a command with a name; spawning into a group **aborts the
//! group's previous task** before starting the new one — Textual's
//! `@work(exclusive=True)`, the debounced-search pattern. Ungrouped commands run
//! to completion. When rapid input spawns a new grouped fetch each keystroke,
//! only the last survives; the aborted ones never deliver a message.
//!
//! # Panics are contained, not swallowed
//!
//! An effect runs on a tokio task, so a panic in it is caught at the task
//! boundary; the runtime surfaces it as
//! [`Event::EffectFailed`](crate::app::Event::EffectFailed) carrying the group
//! name (if any) and the panic text, and the app keeps running. This is the ADR
//! 0005 "panics in effect tasks are caught and surfaced as messages" clause.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use futures_core::Stream;
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinHandle};

/// A failed effect: the group it was spawned into (if any) and the failure text.
///
/// Delivered as [`Event::EffectFailed`](crate::app::Event::EffectFailed) when an
/// effect task panics. The framework contains the panic at the tokio task
/// boundary — it never unwinds the loop — and reports it honestly rather than
/// swallowing it, so a broken effect is a visible message the app can log or
/// surface, not a silent stall.
///
/// # Examples
///
/// ```
/// use rabbitui::effect::EffectError;
///
/// let error = EffectError::new(Some("search".to_string()), "boom".to_string());
/// assert_eq!(error.group(), Some("search"));
/// assert_eq!(error.message(), "boom");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectError {
    group: Option<String>,
    message: String,
}

impl EffectError {
    /// Builds an effect error from an optional group name and a failure message.
    #[must_use]
    pub fn new(group: Option<String>, message: String) -> Self {
        Self { group, message }
    }

    /// The group the failed effect was spawned into, if any.
    #[must_use]
    pub fn group(&self) -> Option<&str> {
        self.group.as_deref()
    }

    /// The failure text (a panic's payload, or a generic message).
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for EffectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.group {
            Some(group) => write!(f, "effect in group {group:?} failed: {}", self.message),
            None => write!(f, "effect failed: {}", self.message),
        }
    }
}

impl std::error::Error for EffectError {}

/// A boxed future producing one message.
type BoxFuture<M> = Pin<Box<dyn Future<Output = M> + Send>>;

/// A boxed stream producing many messages.
type BoxStream<M> = Pin<Box<dyn Stream<Item = M> + Send>>;

/// What a [`Cmd`] carries: a single future, a stream of messages, or a request
/// to cancel a group's live task without replacing it.
enum Kind<M> {
    /// A future yielding exactly one message.
    Future(BoxFuture<M>),
    /// A stream yielding zero or more messages, ending when the stream does.
    Stream(BoxStream<M>),
    /// Abort the named group's live task, if any, and start nothing.
    Cancel,
}

/// An async effect the app hands to the runtime: a future or a stream of
/// messages, optionally tagged with a cancel-previous group (ADR 0005).
///
/// Issued through [`Update::spawn`](crate::app::Update::spawn), buffered like a
/// commit, and drained by the runtime after `update` returns. The runtime spawns
/// it; its messages arrive back in the loop as
/// [`Event::Message`](crate::app::Event::Message) in completion order (command
/// completion is unspecified — ADR 0005).
///
/// # Examples
///
/// A one-shot future and a debounced, grouped one:
///
/// ```
/// use std::time::Duration;
///
/// use rabbitui::effect::Cmd;
///
/// // One message after some async work.
/// let load: Cmd<u32> = Cmd::future(async { 42 });
///
/// // A debounced search: spawning another into "search" aborts this one.
/// let search: Cmd<String> =
///     Cmd::timeout(Duration::from_millis(300), || "results".to_string()).group("search");
/// let _ = (load, search);
/// ```
#[must_use = "a Cmd does nothing until it is spawned via Update::spawn"]
pub struct Cmd<M> {
    kind: Kind<M>,
    group: Option<String>,
}

impl<M: Send + 'static> Cmd<M> {
    /// A command that awaits `future` and delivers its one message.
    ///
    /// The single effect primitive (ADR 0005): all other constructors are sugar
    /// over this or over [`stream`](Self::stream).
    pub fn future(future: impl Future<Output = M> + Send + 'static) -> Self {
        Self { kind: Kind::Future(Box::pin(future)), group: None }
    }

    /// A command that yields every message `stream` produces, ending when the
    /// stream does.
    ///
    /// A "subscription" is just a long stream the app chose to start (ADR 0005):
    /// a clock ticker, a websocket, a token feed. The bound is
    /// [`futures_core::Stream`] so any stream type composes without pulling in a
    /// stream-combinator dependency.
    pub fn stream(stream: impl Stream<Item = M> + Send + 'static) -> Self {
        Self { kind: Kind::Stream(Box::pin(stream)), group: None }
    }

    /// A command that waits `duration`, then delivers `f()`'s message.
    ///
    /// Sugar over [`future`](Self::future) for the common timer case — a delayed
    /// action, a debounce tail, a retry backoff. `f` runs after the delay, on the
    /// effect task.
    pub fn timeout(duration: Duration, f: impl FnOnce() -> M + Send + 'static) -> Self {
        Self::future(async move {
            tokio::time::sleep(duration).await;
            f()
        })
    }

    /// A command that **aborts** the named group's live task without starting a
    /// replacement — the stream-stop primitive (ADR 0005 / slice-7 carry-forward).
    ///
    /// [`group`](Self::group) starts a new task *and* cancels the group's previous
    /// one; `cancel_group` is the missing half: it cancels the group's task and
    /// starts nothing. This is how a toggled subscription (a clock ticker, a live
    /// feed) is stopped on demand rather than left running with its messages
    /// ignored. Spawning a `cancel_group("clock")` after `Cmd::stream(...).group("clock")`
    /// stops the stream for good; the aborted task delivers no further messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use rabbitui::effect::Cmd;
    ///
    /// // Later: stop the ticker started under the "clock" group.
    /// let stop: Cmd<u32> = Cmd::cancel_group("clock");
    /// let _ = stop;
    /// ```
    pub fn cancel_group(name: impl Into<String>) -> Self {
        Self { kind: Kind::Cancel, group: Some(name.into()) }
    }

    /// Tags this command with a **cancel-previous** group.
    ///
    /// Spawning a command into a group aborts the group's previous task before
    /// starting this one (Textual `@work(exclusive=True)`). This is the
    /// debounced-search idiom: bind each keystroke to a grouped fetch and only
    /// the last completes. Ungrouped commands (the default) always run to
    /// completion.
    pub fn group(mut self, name: impl Into<String>) -> Self {
        self.group = Some(name.into());
        self
    }
}

/// The effect runtime: spawn tables, group-abort registry, and the result
/// mailbox (ADR 0005), separable from the render loop.
///
/// The loop owns one `Effects<M>` and drives it with [`spawn`](Self::spawn) (as
/// it drains each update's buffered commands) and [`recv`](Self::recv) /
/// [`try_recv`](Self::try_recv) (as one of the `select!` arms). It touches no
/// terminal, so the unit tests exercise the same struct headless under
/// `#[tokio::test]`.
///
/// `M` is the app's message type. Results — one per future, many per stream, and
/// one [`EffectError`] per panicking task — arrive on the mailbox as
/// [`Outbox<M>`] items.
#[derive(Debug)]
pub struct Effects<M> {
    /// The sender cloned into every spawned task; results land on `rx`.
    tx: mpsc::UnboundedSender<Outbox<M>>,
    /// The mailbox the loop drains.
    rx: mpsc::UnboundedReceiver<Outbox<M>>,
    /// The live task per group name, for cancel-previous.
    groups: HashMap<String, AbortHandle>,
}

/// A result on the effect mailbox: a message an effect produced, or a failure.
///
/// The runtime maps these into loop events — a [`Message`](Self::Message)
/// becomes [`Event::Message`](crate::app::Event::Message), a
/// [`Failed`](Self::Failed) becomes
/// [`Event::EffectFailed`](crate::app::Event::EffectFailed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outbox<M> {
    /// A message an effect delivered.
    Message(M),
    /// An effect task panicked; this carries its group and failure text.
    Failed(EffectError),
}

impl<M: Send + 'static> Effects<M> {
    /// Creates an idle runtime with an empty mailbox and no groups.
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self { tx, rx, groups: HashMap::new() }
    }

    /// Spawns `cmd` onto the runtime, applying cancel-previous for a grouped one.
    ///
    /// A future spawns one task delivering one message; a stream spawns one task
    /// forwarding each item until it ends. If `cmd` names a group, the group's
    /// previous task (if still live) is aborted first, so only the newest grouped
    /// command survives — the debounced-search guarantee. A task that panics
    /// sends an [`EffectError`] (with the group name) instead of crashing.
    pub fn spawn(&mut self, cmd: Cmd<M>) {
        let Cmd { kind, group } = cmd;

        // A cancel-group command aborts the group's live task and starts nothing.
        // Removing the entry lets the group be re-armed later by a fresh spawn.
        if let Kind::Cancel = kind {
            if let Some(name) = group {
                if let Some(previous) = self.groups.remove(&name) {
                    previous.abort();
                }
            }
            return;
        }

        let tx = self.tx.clone();
        let group_for_error = group.clone();

        // The task body: run the effect and forward its messages. Panics inside
        // the future/stream are caught by the tokio task boundary; the join
        // watcher below turns a panicked join into an `EffectError`.
        let handle: JoinHandle<()> = match kind {
            Kind::Future(future) => tokio::spawn(async move {
                let message = future.await;
                // A closed receiver means the loop is gone; dropping is correct.
                let _ = tx.send(Outbox::Message(message));
            }),
            Kind::Stream(mut stream) => tokio::spawn(async move {
                use std::task::Poll;
                std::future::poll_fn(move |cx| {
                    loop {
                        match stream.as_mut().poll_next(cx) {
                            Poll::Ready(Some(message)) => {
                                if tx.send(Outbox::Message(message)).is_err() {
                                    return Poll::Ready(());
                                }
                            }
                            Poll::Ready(None) => return Poll::Ready(()),
                            Poll::Pending => return Poll::Pending,
                        }
                    }
                })
                .await;
            }),
            // Handled above with an early return; the match cannot reach here.
            Kind::Cancel => unreachable!("Kind::Cancel is handled before spawning a task"),
        };

        // Watch the task: on a panic, surface an `EffectError` on the mailbox so
        // the loop delivers `Event::EffectFailed` and the app stays alive.
        let abort = self.watch(handle, group_for_error);

        // Cancel-previous: register (and abort the predecessor) for a group.
        if let Some(name) = group {
            if let Some(previous) = self.groups.insert(name, abort) {
                previous.abort();
            }
        }
    }

    /// Spawns a watcher that turns a panicked join into an [`EffectError`] on the
    /// mailbox, and returns the effect task's abort handle for group tracking.
    ///
    /// An orderly finish (or a deliberate group abort) sends nothing; only a
    /// genuine panic is reported. The watcher itself holds no state the loop must
    /// drain — it lives exactly as long as the effect task.
    fn watch(&self, handle: JoinHandle<()>, group: Option<String>) -> AbortHandle {
        let abort = handle.abort_handle();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match handle.await {
                Ok(()) => {}
                Err(join) if join.is_cancelled() => {
                    // A cancel-previous abort is expected; report nothing.
                }
                Err(join) => {
                    let message = panic_message(join);
                    let _ = tx.send(Outbox::Failed(EffectError::new(group, message)));
                }
            }
        });
        abort
    }

    /// Awaits the next mailbox item (a message or a failure), or `None` once every
    /// sender is gone.
    ///
    /// One arm of the loop's `select!`. Because `Effects` holds its own `tx`, this
    /// never returns `None` in a running loop — there is always a sender — so the
    /// arm parks until an effect produces something.
    pub async fn recv(&mut self) -> Option<Outbox<M>> {
        self.rx.recv().await
    }

    /// Drains one already-arrived mailbox item without awaiting, or `None` if the
    /// mailbox is momentarily empty.
    ///
    /// The frame-coalescing drain (ADR 0005 / tui2): after handling one event the
    /// loop `try_recv`s in a tight biased loop, absorbing a burst of stream
    /// messages into one render instead of one render per message. See the
    /// `flood` test.
    pub fn try_recv(&mut self) -> Option<Outbox<M>> {
        self.rx.try_recv().ok()
    }

    /// The number of groups currently tracking a live-or-finished task.
    ///
    /// A probe for tests; the loop never needs it.
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }
}

impl<M: Send + 'static> Default for Effects<M> {
    fn default() -> Self {
        Self::new()
    }
}

/// Extracts a readable message from a panicked task's [`JoinError`].
///
/// tokio does not surface the panic payload directly; it re-wraps it, so the best
/// portable text is the join error's own `Display`, which reads
/// "task N panicked". A downcast of the payload would need `into_panic`, which
/// consumes the error; the `Display` is stable and enough for the honest report.
///
/// [`JoinError`]: tokio::task::JoinError
fn panic_message(join: tokio::task::JoinError) -> String {
    // `into_panic` gives the payload; try to render a `&str`/`String` payload,
    // else fall back to the join error's own text.
    if join.is_panic() {
        let payload = join.into_panic();
        if let Some(text) = payload.downcast_ref::<&'static str>() {
            return (*text).to_string();
        }
        if let Some(text) = payload.downcast_ref::<String>() {
            return text.clone();
        }
        return "effect task panicked".to_string();
    }
    join.to_string()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A hand-rolled stream over a `Vec`, to test `Cmd::stream` without a
    /// stream-combinator dependency.
    struct VecStream<M> {
        items: std::collections::VecDeque<M>,
    }

    impl<M> VecStream<M> {
        fn new(items: impl IntoIterator<Item = M>) -> Self {
            Self { items: items.into_iter().collect() }
        }
    }

    impl<M: Unpin> Stream for VecStream<M> {
        type Item = M;
        fn poll_next(
            mut self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<M>> {
            std::task::Poll::Ready(self.items.pop_front())
        }
    }

    #[tokio::test]
    async fn future_delivers_one_message() {
        let mut effects = Effects::new();
        effects.spawn(Cmd::future(async { 7u32 }));
        assert_eq!(effects.recv().await, Some(Outbox::Message(7)));
    }

    #[tokio::test]
    async fn stream_delivers_every_item_then_ends() {
        let mut effects = Effects::new();
        effects.spawn(Cmd::stream(VecStream::new([1u32, 2, 3])));
        assert_eq!(effects.recv().await, Some(Outbox::Message(1)));
        assert_eq!(effects.recv().await, Some(Outbox::Message(2)));
        assert_eq!(effects.recv().await, Some(Outbox::Message(3)));
        // The stream ended; nothing more is queued (a poll would park, so we only
        // assert the three items arrived).
        assert!(effects.try_recv().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_delivers_after_the_delay() {
        let mut effects = Effects::new();
        effects.spawn(Cmd::timeout(Duration::from_millis(300), || "done"));
        // With the clock paused, nothing has fired yet.
        assert!(effects.try_recv().is_none());
        tokio::time::advance(Duration::from_millis(300)).await;
        assert_eq!(effects.recv().await, Some(Outbox::Message("done")));
    }

    #[tokio::test(start_paused = true)]
    async fn group_cancel_previous_drops_the_first_result() {
        let mut effects = Effects::new();
        // First grouped fetch: slow.
        effects.spawn(Cmd::timeout(Duration::from_millis(300), || "first").group("search"));
        // A second into the same group aborts the first before it can fire.
        effects.spawn(Cmd::timeout(Duration::from_millis(300), || "second").group("search"));
        tokio::time::advance(Duration::from_millis(300)).await;
        // Only the second survives; the first's result never arrives.
        assert_eq!(effects.recv().await, Some(Outbox::Message("second")));
        assert!(effects.try_recv().is_none());
        assert_eq!(effects.group_count(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_group_aborts_the_live_task_without_replacing_it() {
        let mut effects = Effects::new();
        // Start a grouped fetch, then cancel the group before it fires.
        effects.spawn(Cmd::timeout(Duration::from_millis(300), || "result").group("search"));
        assert_eq!(effects.group_count(), 1);
        effects.spawn(Cmd::<&str>::cancel_group("search"));
        // The group is gone and nothing replaced the task.
        assert_eq!(effects.group_count(), 0);
        tokio::time::advance(Duration::from_millis(300)).await;
        // The aborted task delivers no message.
        assert!(effects.try_recv().is_none());
    }

    #[tokio::test]
    async fn cancel_group_of_an_absent_group_is_a_no_op() {
        let mut effects = Effects::<&str>::new();
        // Cancelling a group that was never spawned into does nothing, cleanly.
        effects.spawn(Cmd::cancel_group("nope"));
        assert_eq!(effects.group_count(), 0);
        assert!(effects.try_recv().is_none());
    }

    #[tokio::test]
    async fn ungrouped_commands_all_complete() {
        let mut effects = Effects::new();
        effects.spawn(Cmd::future(async { 1u32 }));
        effects.spawn(Cmd::future(async { 2u32 }));
        let mut got = vec![
            unwrap_message(effects.recv().await),
            unwrap_message(effects.recv().await),
        ];
        got.sort_unstable();
        assert_eq!(got, vec![1, 2]);
    }

    #[tokio::test]
    async fn panic_is_contained_and_reported_with_group() {
        let mut effects = Effects::new();
        effects.spawn(Cmd::<u32>::future(async { panic!("boom") }).group("risky"));
        match effects.recv().await {
            Some(Outbox::Failed(error)) => {
                assert_eq!(error.group(), Some("risky"));
                assert_eq!(error.message(), "boom");
            }
            other => panic!("expected a contained failure, got {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn completion_order_is_by_finish_not_spawn() {
        let mut effects = Effects::new();
        // Spawn slow-then-fast; the fast one must arrive first.
        effects.spawn(Cmd::timeout(Duration::from_millis(300), || "slow"));
        effects.spawn(Cmd::timeout(Duration::from_millis(100), || "fast"));
        tokio::time::advance(Duration::from_millis(100)).await;
        assert_eq!(effects.recv().await, Some(Outbox::Message("fast")));
        tokio::time::advance(Duration::from_millis(200)).await;
        assert_eq!(effects.recv().await, Some(Outbox::Message("slow")));
    }

    #[tokio::test]
    async fn flood_of_stream_messages_drains_in_one_burst() {
        // A stream emits far more messages than a loop would render; the biased
        // `try_recv` drain absorbs the whole burst without awaiting per message.
        let mut effects = Effects::new();
        const N: u32 = 1000;
        effects.spawn(Cmd::stream(VecStream::new(0..N)));
        // Await the first to know the task has started producing…
        let first = unwrap_message(effects.recv().await);
        let mut count = 1;
        let mut last = first;
        // …then drain everything already queued in a tight loop, re-awaiting when
        // the mailbox momentarily empties, until all N have arrived.
        while count < N {
            match effects.try_recv() {
                Some(Outbox::Message(m)) => {
                    last = m;
                    count += 1;
                }
                Some(Outbox::Failed(e)) => panic!("unexpected failure: {e}"),
                None => {
                    // Give the producer task a chance to enqueue more.
                    tokio::task::yield_now().await;
                }
            }
        }
        assert_eq!(count, N);
        assert_eq!(last, N - 1);
    }

    fn unwrap_message<M>(item: Option<Outbox<M>>) -> M {
        match item {
            Some(Outbox::Message(m)) => m,
            _ => panic!("expected a message"),
        }
    }
}
