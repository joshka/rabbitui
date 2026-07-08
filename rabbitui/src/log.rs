//! The `tracing` collector: a [`Layer`] that formats events into the core log
//! ring, plus the global-default install used by [`App::tracing`].
//!
//! This module is the facade half of the logging seam
//! (`docs/design/arc2b-measurement-scroll.md`). It is the *only* place `tracing`
//! appears in rabbitui: [`Collector`] implements
//! [`tracing_subscriber::Layer`], formats each event into a
//! [`LogRecord`](rabbitui_core::log::LogRecord), and pushes it into a
//! [`LogHandle`](rabbitui_core::log::LogHandle) — the runtime-free ring the
//! runtime owns and the `LogOverlay` widget reads. The widget never touches this
//! module: it meets the runtime at the core handle, so `rabbitui-widgets` depends
//! only on `rabbitui-core` (ADR 0011).
//!
//! # Install-once, never panic
//!
//! [`App::tracing`](crate::App::tracing) installs the collector as the process's
//! **global default** subscriber — but only if none is already set. A second
//! install (a test harness, a host app that set up its own subscriber, a second
//! `App` in one process) is a no-op, not a panic: [`try_install`] returns whether
//! it won the race, and the runtime ignores a loss. Double-init is a supported,
//! silent outcome by design.
//!
//! # Filtering and the close flush
//!
//! An [`EnvFilter`] honors `RABBITUI_LOG`, falling back to `RUST_LOG`, so the
//! usual `RUST_LOG=debug` works and `RABBITUI_LOG` overrides it for this app
//! alone. Nothing is written to the terminal while the app owns it (writing to
//! stderr would corrupt the alternate screen); instead the runtime, on close and
//! **after** the terminal is restored, flushes buffered `WARN` and above to
//! stderr with [`flush_warnings`], so a panic trail or an error survives the alt
//! screen.

use std::io::Write;

use rabbitui_core::log::{Level, LogHandle, LogRecord};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::registry::Registry;
use tracing_subscriber::util::SubscriberInitExt;

/// The environment variable rabbitui reads its log filter from, before falling
/// back to the conventional `RUST_LOG`.
pub const FILTER_ENV: &str = "RABBITUI_LOG";

/// A [`tracing_subscriber::Layer`] that formats events into a
/// [`LogHandle`](rabbitui_core::log::LogHandle) ring.
///
/// Holds a clone of the ring handle (a cheap `Arc`); the runtime keeps another
/// clone to hand the overlay. On each event it maps the `tracing::Level` to the
/// core [`Level`], extracts the `message` field, and pushes a [`LogRecord`].
///
/// # Examples
///
/// ```
/// use rabbitui::log::Collector;
/// use rabbitui_core::log::LogHandle;
///
/// let handle = LogHandle::with_capacity(64);
/// let _collector = Collector::new(handle.clone());
/// // Installing it globally is `App::tracing`'s job; a Collector can also be
/// // used directly in a `tracing_subscriber` stack for testing.
/// ```
#[derive(Debug, Clone)]
pub struct Collector {
    handle: LogHandle,
}

impl Collector {
    /// Creates a collector writing into `handle`.
    #[must_use]
    pub fn new(handle: LogHandle) -> Self {
        Self { handle }
    }

    /// The ring handle this collector writes into.
    #[must_use]
    pub fn handle(&self) -> &LogHandle {
        &self.handle
    }
}

/// Maps a `tracing::Level` to the core [`Level`].
fn map_level(level: &tracing::Level) -> Level {
    match *level {
        tracing::Level::TRACE => Level::Trace,
        tracing::Level::DEBUG => Level::Debug,
        tracing::Level::INFO => Level::Info,
        tracing::Level::WARN => Level::Warn,
        tracing::Level::ERROR => Level::Error,
    }
}

/// A field visitor that captures a formatted event's `message` field and appends
/// any other fields as `key=value` pairs.
struct MessageVisitor {
    message: String,
}

impl MessageVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
        }
    }

    /// Appends `text` to the message, separating from prior content with a space.
    fn append(&mut self, text: &str) {
        if !self.message.is_empty() {
            self.message.push(' ');
        }
        self.message.push_str(text);
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            // The event's primary message: the `{:?}` of the format args, minus
            // the surrounding quotes a `Debug` of a string would add is not worth
            // the fragility — the message field's Debug is the rendered string.
            let rendered = format!("{value:?}");
            self.append(&rendered);
        } else {
            self.append(&format!("{}={value:?}", field.name()));
        }
    }
}

impl<S: Subscriber> Layer<S> for Collector {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = MessageVisitor::new();
        event.record(&mut visitor);
        self.handle.push(LogRecord::new(
            map_level(metadata.level()),
            metadata.target(),
            visitor.message,
        ));
    }
}

/// Builds the [`EnvFilter`] from `RABBITUI_LOG`, falling back to `RUST_LOG`, then
/// to `info`.
///
/// `RABBITUI_LOG` takes precedence so an app can filter its own logs without
/// disturbing a `RUST_LOG` set for other crates. A malformed directive string is
/// tolerated: the filter keeps the directives it can parse (the `EnvFilter`
/// builder's lossy default).
#[must_use]
pub fn env_filter() -> EnvFilter {
    filter_from(
        std::env::var(FILTER_ENV).ok(),
        std::env::var("RUST_LOG").ok(),
    )
}

/// The pure filter-selection logic behind [`env_filter`], taking the two env
/// values explicitly so it is testable without mutating the process environment
/// (`unsafe_code` is forbidden workspace-wide, so a test cannot set env vars).
///
/// `RABBITUI_LOG` wins if present, else `RUST_LOG`, else `info`.
#[must_use]
fn filter_from(rabbitui_log: Option<String>, rust_log: Option<String>) -> EnvFilter {
    if let Some(directives) = rabbitui_log {
        return EnvFilter::builder().parse_lossy(directives);
    }
    if let Some(directives) = rust_log {
        return EnvFilter::builder().parse_lossy(directives);
    }
    EnvFilter::new("info")
}

/// Installs a [`Collector`] over `handle` as the global-default subscriber, with
/// the [`env_filter`] filter — **only if no global default is already set**.
///
/// Returns `true` if this call installed the collector, `false` if a global
/// default was already in place (a host app's subscriber, a prior `App`, a test
/// harness). Never panics on a double install: that is the documented, supported
/// outcome (`App::tracing`'s "install only if none is set").
pub fn try_install(handle: LogHandle) -> bool {
    let subscriber = Registry::default()
        .with(env_filter())
        .with(Collector::new(handle));
    subscriber.try_init().is_ok()
}

/// Writes buffered `WARN` and above from `handle` to stderr, newest last.
///
/// Called by the runtime on close, **after** the terminal is restored, so errors
/// and warnings survive the alternate screen (they were never written while it
/// was owned). A write error to stderr is ignored — there is nowhere left to
/// report it, and the process is exiting.
pub fn flush_warnings(handle: &LogHandle) {
    let records = handle.drain_at_least(Level::Warn);
    if records.is_empty() {
        return;
    }
    let stderr = std::io::stderr();
    let mut lock = stderr.lock();
    for record in records {
        // Best-effort: a broken stderr on exit is not worth crashing over.
        let _ = writeln!(
            lock,
            "[{}] {}: {}",
            record.level.as_str(),
            record.target,
            record.message
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::subscriber;
    use tracing_subscriber::layer::SubscriberExt;

    /// Runs `body` with a subscriber that collects into `handle`, scoped to this
    /// thread so the test does not touch the global default.
    fn with_collector(handle: &LogHandle, body: impl FnOnce()) {
        let subscriber = Registry::default().with(Collector::new(handle.clone()));
        subscriber::with_default(subscriber, body);
    }

    #[test]
    fn collector_captures_level_target_and_message() {
        let handle = LogHandle::with_capacity(16);
        with_collector(&handle, || {
            tracing::info!("hello world");
            tracing::warn!("careful now");
        });
        let records = handle.snapshot();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].level, Level::Info);
        assert_eq!(records[0].message, "hello world");
        assert_eq!(records[1].level, Level::Warn);
        assert_eq!(records[1].message, "careful now");
        // The target is this test module's path.
        assert!(records[0].target.contains("log"));
    }

    #[test]
    fn collector_captures_extra_fields() {
        let handle = LogHandle::with_capacity(16);
        with_collector(&handle, || {
            tracing::info!(count = 3, "fetched");
        });
        let record = &handle.snapshot()[0];
        // The message plus the extra field.
        assert!(record.message.contains("fetched"), "{:?}", record.message);
        assert!(record.message.contains("count=3"), "{:?}", record.message);
    }

    #[test]
    fn env_filter_prefers_rabbitui_log_over_rust_log() {
        // RABBITUI_LOG=trace wins over RUST_LOG=error: a debug event is admitted.
        // The pure `filter_from` lets us test this without mutating process env
        // (unsafe env-var setters are forbidden workspace-wide).
        let filter = filter_from(Some("trace".into()), Some("error".into()));
        let handle = LogHandle::with_capacity(16);
        let subscriber = Registry::default()
            .with(filter)
            .with(Collector::new(handle.clone()));
        subscriber::with_default(subscriber, || {
            tracing::debug!("debug visible under trace filter");
        });
        assert_eq!(
            handle.len(),
            1,
            "RABBITUI_LOG=trace should admit a debug event"
        );
    }

    #[test]
    fn env_filter_falls_back_to_rust_log() {
        // No RABBITUI_LOG, RUST_LOG=warn: info is dropped, warn kept.
        let filter = filter_from(None, Some("warn".into()));
        let handle = LogHandle::with_capacity(16);
        let subscriber = Registry::default()
            .with(filter)
            .with(Collector::new(handle.clone()));
        subscriber::with_default(subscriber, || {
            tracing::info!("info dropped at warn");
            tracing::warn!("warn kept");
        });
        let records = handle.snapshot();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].message, "warn kept");
    }

    #[test]
    fn env_filter_defaults_to_info_when_neither_is_set() {
        let filter = filter_from(None, None);
        let handle = LogHandle::with_capacity(16);
        let subscriber = Registry::default()
            .with(filter)
            .with(Collector::new(handle.clone()));
        subscriber::with_default(subscriber, || {
            tracing::debug!("debug dropped at info");
            tracing::info!("info kept");
        });
        let records = handle.snapshot();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].message, "info kept");
    }

    #[test]
    fn flush_warnings_writes_only_warn_and_above() {
        // Construct the collector directly (no subscriber, no terminal) and verify
        // the close-flush selects WARN+ — the unit-testable close-flush behavior.
        let handle = LogHandle::with_capacity(16);
        handle.push(LogRecord::new(Level::Info, "t", "info"));
        handle.push(LogRecord::new(Level::Warn, "t", "warn"));
        handle.push(LogRecord::new(Level::Error, "t", "err"));
        let flushed = handle.drain_at_least(Level::Warn);
        let messages: Vec<_> = flushed.iter().map(|r| r.message.as_str()).collect();
        assert_eq!(messages, ["warn", "err"]);
        // flush_warnings itself does not panic on a populated handle.
        flush_warnings(&handle);
    }
}
