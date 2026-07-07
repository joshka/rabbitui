//! A built-in demo backend.
//!
//! Generates a deterministic scripted response seeded from the latest user prompt
//! — the slice-8 example's simulated agent, re-expressed as [`StreamEvent`]s over
//! the [`Backend`] contract. It exists so `cargo run --bin rabbit` does something
//! before slice 2 lands the real Anthropic backend and makes that the default.

use std::time::Duration;

use crate::backend::replay::stream_turn;
use crate::backend::{Backend, ChatRequest, EventStream, Role, StopReason, StreamEvent, Usage};

/// A backend that scripts a canned markdown response per prompt.
#[derive(Debug, Clone)]
pub struct DemoBackend {
    /// The inter-event delay, so streaming paces visibly.
    delay: Duration,
}

impl Default for DemoBackend {
    fn default() -> Self {
        Self {
            delay: Duration::from_millis(80),
        }
    }
}

impl Backend for DemoBackend {
    fn send(&mut self, request: ChatRequest) -> EventStream {
        let topic = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)
            .map_or("your request", |message| message.content.trim());
        stream_turn(demo_turn(topic), Some(self.delay))
    }
}

/// The scripted event turn for `topic`: chunked markdown prose with a heading,
/// bold, a bullet list, inline code, and a short thinking preamble.
fn demo_turn(topic: &str) -> Vec<StreamEvent> {
    let text = |s: &str| StreamEvent::TextDelta { text: s.to_string() };
    let think = |s: &str| StreamEvent::ThinkingDelta { text: s.to_string() };
    vec![
        think("Reading the request and sketching an approach before I answer."),
        text(&format!("## Working on: {topic}\n\n")),
        text("Let me start by looking at the relevant "),
        text("code and the **test suite**.\n\n"),
        text("Everything checks out. The change touches:\n\n"),
        text("- the `core::text` span type\n"),
        text("- the inline engine's per-span SGR\n\n"),
        text("Done — this is the built-in demo backend; slice 2 wires the real API."),
        StreamEvent::MessageDone {
            stop_reason: StopReason::EndTurn,
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
            },
        },
    ]
}
