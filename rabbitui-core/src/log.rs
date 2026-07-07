//! The runtime-free logging seam: a bounded ring buffer of formatted log
//! records, shared by handle.
//!
//! Per `docs/design/arc2b-measurement-scroll.md`, rabbitui integrates `tracing`
//! behind a facade feature — but the *widget* that renders the log tail
//! ([`LogOverlay`]) must not drag `tracing` (or the facade, or an async runtime)
//! into the widgets crate. This module is the seam that keeps that boundary
//! clean: it defines the **core-side handle type** both sides share.
//!
//! - The facade's `rabbitui::log::Collector` is a `tracing_subscriber::Layer`
//!   that formats each event into a [`LogRecord`] and pushes it into a
//!   [`LogHandle`] the runtime owns. That is the only place `tracing` appears.
//! - `rabbitui-widgets`' `LogOverlay` takes a `&LogHandle`, reads its tail, and
//!   paints it. It depends only on this crate — no `tracing`, no facade.
//!
//! The handle is an `Arc<Mutex<VecDeque<LogRecord>>>` with a fixed capacity: a
//! push past capacity drops the oldest record (a ring buffer). At TUI event
//! rates a `Mutex` is cheap enough (the design note's explicit call), and the
//! `Arc` lets the runtime keep one clone while the view borrows another to paint.
//!
//! [`LogOverlay`]: https://docs.rs/rabbitui-widgets/latest/rabbitui_widgets/struct.LogOverlay.html
//!
//! # Examples
//!
//! ```
//! use rabbitui_core::log::{Level, LogHandle, LogRecord};
//!
//! let handle = LogHandle::with_capacity(2);
//! handle.push(LogRecord::new(Level::Info, "app", "starting"));
//! handle.push(LogRecord::new(Level::Warn, "app", "low disk"));
//! handle.push(LogRecord::new(Level::Error, "app", "boom")); // evicts "starting"
//!
//! // The ring keeps only the last two records.
//! let tail = handle.snapshot();
//! assert_eq!(tail.len(), 2);
//! assert_eq!(tail[0].message, "low disk");
//! assert_eq!(tail[1].message, "boom");
//! ```

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// The severity of a log record.
///
/// A closed, `tracing`-free mirror of `tracing::Level`, ordered least-to-most
/// severe so a filter or a "WARN and above" flush can compare with `>=`. The
/// facade maps `tracing::Level` onto this at collection time, so the widgets
/// crate never sees `tracing`.
///
/// # Examples
///
/// ```
/// use rabbitui_core::log::Level;
///
/// // More severe compares greater.
/// assert!(Level::Error > Level::Warn);
/// assert!(Level::Warn >= Level::Warn);
/// assert_eq!(Level::Error.as_str(), "ERROR");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// The most verbose level: fine-grained tracing.
    Trace,
    /// Debugging detail.
    Debug,
    /// Ordinary informational events.
    Info,
    /// A recoverable problem worth surfacing.
    Warn,
    /// An error: something failed.
    Error,
}

impl Level {
    /// The level's fixed uppercase name, as shown in an overlay line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

/// One formatted log event: its level, target, message, and a monotonic
/// sequence number.
///
/// The [`Collector`] formats a `tracing` event into this plain record at capture
/// time — the widget never re-derives anything from a live event. `seq` is a
/// per-handle counter (a "frame counter"-style stamp, per the design note's
/// "timestamp from a frame counter or Instant"): it orders records and lets a UI
/// tell new lines from old without pulling in a clock.
///
/// [`Collector`]: https://docs.rs/rabbitui/latest/rabbitui/log/struct.Collector.html
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    /// The event's severity.
    pub level: Level,
    /// The event's `tracing` target (usually the module path).
    pub target: String,
    /// The formatted message text.
    pub message: String,
    /// A monotonically increasing stamp assigned when the record entered the
    /// ring (0 for a record built directly, before it is pushed).
    pub seq: u64,
}

impl LogRecord {
    /// Builds a record with `seq` unset (0); [`LogHandle::push`] stamps it on the
    /// way into the ring.
    #[must_use]
    pub fn new(level: Level, target: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level,
            target: target.into(),
            message: message.into(),
            seq: 0,
        }
    }
}

/// A bounded, shareable ring buffer of [`LogRecord`]s.
///
/// Cloning is cheap (an `Arc` bump) and every clone views the same ring, so the
/// runtime holds one clone for the [`Collector`] to write through while the view
/// holds another to paint the overlay. A push past the capacity evicts the
/// oldest record.
///
/// The `Mutex` never guards long work — a push or a tail read — so lock
/// contention at terminal event rates is negligible (the design note's explicit
/// "cheap enough at TUI event rates").
#[derive(Debug, Clone)]
pub struct LogHandle {
    inner: Arc<Mutex<Ring>>,
}

/// The inner ring: the bounded deque plus the running sequence counter.
#[derive(Debug)]
struct Ring {
    records: VecDeque<LogRecord>,
    capacity: usize,
    next_seq: u64,
}

/// The default ring capacity when [`LogHandle::new`] is used — enough tail to be
/// useful in an overlay without unbounded growth.
pub const DEFAULT_CAPACITY: usize = 1024;

impl LogHandle {
    /// A handle with the [`DEFAULT_CAPACITY`] ring.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// A handle whose ring holds at most `capacity` records (at least one).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Ring {
                records: VecDeque::new(),
                capacity: capacity.max(1),
                next_seq: 0,
            })),
        }
    }

    /// Pushes `record` into the ring, stamping its [`seq`](LogRecord::seq) and
    /// evicting the oldest record if the ring is full.
    ///
    /// Takes `record` by value and returns nothing: the ring owns it thereafter.
    pub fn push(&self, mut record: LogRecord) {
        // A poisoned lock means a prior holder panicked mid-push; recover the
        // guard and keep logging rather than propagate the panic into the runtime.
        let mut ring = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        record.seq = ring.next_seq;
        ring.next_seq = ring.next_seq.wrapping_add(1);
        if ring.records.len() == ring.capacity {
            ring.records.pop_front();
        }
        ring.records.push_back(record);
    }

    /// The number of records currently in the ring.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().records.len()
    }

    /// Whether the ring holds no records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().records.is_empty()
    }

    /// The ring's fixed capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.lock().capacity
    }

    /// A cloned snapshot of every record in the ring, oldest first.
    ///
    /// Copies out under the lock so a caller (the overlay's paint) reads a stable
    /// view without holding the mutex while it iterates.
    #[must_use]
    pub fn snapshot(&self) -> Vec<LogRecord> {
        self.lock().records.iter().cloned().collect()
    }

    /// The last `n` records, oldest first — the tail an overlay renders.
    ///
    /// Returns every record when `n` exceeds the ring length.
    #[must_use]
    pub fn tail(&self, n: usize) -> Vec<LogRecord> {
        let ring = self.lock();
        let skip = ring.records.len().saturating_sub(n);
        ring.records.iter().skip(skip).cloned().collect()
    }

    /// The records at or above `min_level`, oldest first — the close-flush set
    /// (WARN and above survives terminal restore).
    #[must_use]
    pub fn drain_at_least(&self, min_level: Level) -> Vec<LogRecord> {
        self.lock()
            .records
            .iter()
            .filter(|record| record.level >= min_level)
            .cloned()
            .collect()
    }

    /// Locks the ring, recovering from poisoning (see [`push`](Self::push)).
    fn lock(&self) -> std::sync::MutexGuard<'_, Ring> {
        self.inner.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}

impl Default for LogHandle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_stamps_a_monotonic_seq() {
        let handle = LogHandle::with_capacity(8);
        handle.push(LogRecord::new(Level::Info, "t", "a"));
        handle.push(LogRecord::new(Level::Info, "t", "b"));
        let tail = handle.snapshot();
        assert_eq!(tail[0].seq, 0);
        assert_eq!(tail[1].seq, 1);
    }

    #[test]
    fn ring_evicts_oldest_past_capacity() {
        let handle = LogHandle::with_capacity(2);
        for msg in ["a", "b", "c", "d"] {
            handle.push(LogRecord::new(Level::Info, "t", msg));
        }
        let tail = handle.snapshot();
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].message, "c");
        assert_eq!(tail[1].message, "d");
        // seq keeps counting even as records are evicted.
        assert_eq!(tail[1].seq, 3);
    }

    #[test]
    fn tail_returns_the_last_n_oldest_first() {
        let handle = LogHandle::with_capacity(16);
        for i in 0..5 {
            handle.push(LogRecord::new(Level::Info, "t", i.to_string()));
        }
        let tail = handle.tail(2);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].message, "3");
        assert_eq!(tail[1].message, "4");
        // Asking for more than held returns everything.
        assert_eq!(handle.tail(100).len(), 5);
    }

    #[test]
    fn drain_at_least_filters_by_severity() {
        let handle = LogHandle::with_capacity(16);
        handle.push(LogRecord::new(Level::Debug, "t", "dbg"));
        handle.push(LogRecord::new(Level::Info, "t", "info"));
        handle.push(LogRecord::new(Level::Warn, "t", "warn"));
        handle.push(LogRecord::new(Level::Error, "t", "err"));
        let warn_plus = handle.drain_at_least(Level::Warn);
        let messages: Vec<_> = warn_plus.iter().map(|r| r.message.as_str()).collect();
        assert_eq!(messages, ["warn", "err"]);
    }

    #[test]
    fn clones_share_one_ring() {
        let a = LogHandle::with_capacity(4);
        let b = a.clone();
        a.push(LogRecord::new(Level::Info, "t", "via a"));
        // The clone sees the push: it is the same ring.
        assert_eq!(b.len(), 1);
        assert_eq!(b.snapshot()[0].message, "via a");
    }

    #[test]
    fn capacity_is_at_least_one() {
        let handle = LogHandle::with_capacity(0);
        assert_eq!(handle.capacity(), 1);
        handle.push(LogRecord::new(Level::Info, "t", "a"));
        handle.push(LogRecord::new(Level::Info, "t", "b"));
        assert_eq!(handle.len(), 1);
        assert_eq!(handle.snapshot()[0].message, "b");
    }

    #[test]
    fn levels_order_least_to_most_severe() {
        assert!(Level::Trace < Level::Debug);
        assert!(Level::Debug < Level::Info);
        assert!(Level::Info < Level::Warn);
        assert!(Level::Warn < Level::Error);
    }
}
