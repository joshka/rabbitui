//! A backend that replays recorded turns from a fixture.
//!
//! A fixture is a flat list of [`StreamEvent`]s; turns are delimited by
//! [`StreamEvent::MessageDone`]. Each [`Backend::send`] plays the events up to and
//! including the next `MessageDone`, so a multi-request conversation (an initial
//! turn, then an after-tool-result turn, …) replays across successive `send`
//! calls exactly as it was recorded. This is what makes the app's request loop —
//! including the tool loop — testable with no network.
//!
//! Fixtures load from JSONL (one event per line). Slice 2's real backend gains a
//! record mode that writes this same format, closing the loop.

use std::collections::VecDeque;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_core::Stream;

use super::{Backend, ChatRequest, EventStream, StopReason, StreamEvent, Usage};

/// A backend that replays scripted turns from a fixture.
#[derive(Debug, Clone)]
pub struct ReplayBackend {
    /// The recorded events, oldest first.
    events: Vec<StreamEvent>,
    /// How far through `events` the conversation has replayed.
    cursor: usize,
    /// An optional inter-event delay, for a live-feeling demo. `None` (the
    /// default) replays as fast as the runtime polls — what tests want.
    delay: Option<Duration>,
}

impl ReplayBackend {
    /// A replay backend over an in-memory event list.
    #[must_use]
    pub fn new(events: Vec<StreamEvent>) -> Self {
        Self {
            events,
            cursor: 0,
            delay: None,
        }
    }

    /// Sets an inter-event delay so streaming paces visibly (for the binary demo).
    #[must_use]
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// Parses a fixture from JSONL text (one [`StreamEvent`] per line; blank lines
    /// ignored).
    ///
    /// # Errors
    ///
    /// Returns the first line's parse error.
    pub fn from_jsonl(text: &str) -> Result<Self, serde_json::Error> {
        let events = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<StreamEvent>, _>>()?;
        Ok(Self::new(events))
    }

    /// Loads a fixture from a JSONL file.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the file cannot be read, or an invalid-data error
    /// wrapping the JSONL parse failure.
    pub fn from_path(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::from_jsonl(&text)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }

    /// The events of the next turn: from the cursor up to and including the next
    /// [`StreamEvent::MessageDone`]. Guarantees the turn ends with a `MessageDone`
    /// — if the fixture is exhausted or lacks one, a synthetic `EndTurn` is
    /// appended so the app always sees a turn close (and never hangs streaming).
    fn next_turn(&mut self) -> Vec<StreamEvent> {
        let mut turn = Vec::new();
        while let Some(event) = self.events.get(self.cursor) {
            self.cursor += 1;
            let done = matches!(event, StreamEvent::MessageDone { .. });
            turn.push(event.clone());
            if done {
                return turn;
            }
        }
        turn.push(StreamEvent::MessageDone {
            stop_reason: StopReason::EndTurn,
            usage: Usage::default(),
        });
        turn
    }
}

impl Backend for ReplayBackend {
    fn send(&mut self, _request: ChatRequest) -> EventStream {
        stream_turn(self.next_turn(), self.delay)
    }
}

/// Boxes one turn's events into an [`EventStream`] — the building block both the
/// replay backend and the demo backend hand to [`Backend::send`].
#[must_use]
pub fn stream_turn(events: Vec<StreamEvent>, delay: Option<Duration>) -> EventStream {
    Box::pin(ReplayStream::new(events, delay))
}

/// The stream one replayed turn produces: its events in order, each `Ok`, paced by
/// the optional delay. Mirrors the slice-8 example's hand-rolled `AgentStream`.
struct ReplayStream {
    /// The remaining events of this turn.
    events: VecDeque<StreamEvent>,
    /// The inter-event delay, if any.
    delay: Option<Duration>,
    /// A pending sleep, armed after each yielded event.
    pending: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl ReplayStream {
    fn new(events: Vec<StreamEvent>, delay: Option<Duration>) -> Self {
        Self {
            events: events.into(),
            delay,
            pending: None,
        }
    }
}

impl Stream for ReplayStream {
    type Item = Result<StreamEvent, super::BackendError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        // Wait out an armed inter-event delay before yielding the next event.
        if let Some(pending) = this.pending.as_mut() {
            match pending.as_mut().poll(cx) {
                Poll::Ready(()) => this.pending = None,
                Poll::Pending => return Poll::Pending,
            }
        }
        match this.events.pop_front() {
            Some(event) => {
                if let Some(delay) = this.delay {
                    this.pending = Some(Box::pin(tokio::time::sleep(delay)));
                }
                Poll::Ready(Some(Ok(event)))
            }
            None => Poll::Ready(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_jsonl_segments_turns_at_message_done() {
        let fixture = include_str!("../../tests/fixtures/greeting.jsonl");
        let mut backend = ReplayBackend::from_jsonl(fixture).expect("fixture parses");

        let turn = backend.next_turn();
        assert_eq!(turn.len(), 5, "the greeting fixture is one five-event turn");
        assert!(matches!(turn.last(), Some(StreamEvent::MessageDone { .. })));

        // Exhausted: the next turn is a synthetic end-of-turn so the app never hangs.
        let synthetic = backend.next_turn();
        assert_eq!(synthetic.len(), 1);
        assert!(matches!(
            synthetic[0],
            StreamEvent::MessageDone {
                stop_reason: StopReason::EndTurn,
                ..
            }
        ));
    }

    #[test]
    fn a_blank_fixture_yields_a_synthetic_end_turn() {
        let mut backend = ReplayBackend::new(Vec::new());
        let turn = backend.next_turn();
        assert!(matches!(turn.as_slice(), [StreamEvent::MessageDone { .. }]));
    }
}
