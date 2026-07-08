//! The backend contract.
//!
//! Everything network-shaped sits behind [`Backend`] so the UI is testable
//! offline and the end-to-end tapes never touch the network. A backend turns a
//! [`ChatRequest`] into a stream of wire-neutral [`StreamEvent`]s; the app folds
//! those into transcript cells. Two implementations exist: [`replay`] (plays
//! recorded fixtures — this slice) and the real Anthropic HTTP backend (slice 2).
//!
//! The event vocabulary mirrors the Anthropic streaming wire closely enough that
//! slice 2 is a matter of parsing SSE into these variants — but it is the app's
//! type, not the wire's: the SSE shape does not leak past this module.

use std::pin::Pin;

use futures_core::Stream;
use serde::{Deserialize, Serialize};

pub mod anthropic;
pub mod replay;
pub mod sse;

/// A boxed event stream for one request/response turn.
///
/// `Send + 'static` because the app spawns it as an effect ([`Cmd::stream`]) that
/// outlives the `send` call — so [`Backend::send`] returns an owned stream, never
/// one borrowing the backend. The replay backend clones its scripted turn out;
/// slice 2's HTTP backend boxes a `reqwest` byte stream (both are `Send`).
///
/// [`Cmd::stream`]: rabbitui::effect::Cmd::stream
pub type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, BackendError>> + Send>>;

/// The turn a backend is asked to produce: the model and the conversation so far.
///
/// The API is stateless, so the whole history rides on every request. System
/// prompt, sampling, and other request knobs are the backend's own concern
/// (slice 2), not part of this app-facing request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatRequest {
    /// The model id (e.g. `claude-opus-4-8`).
    pub model: String,
    /// The conversation history, oldest first.
    pub messages: Vec<ChatMessage>,
}

/// One message in the conversation history.
///
/// Slice 1 carries text-only content; slices 2 and 4 grow this into content
/// blocks (thinking, tool_use, tool_result) as the wire requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Who authored the message.
    pub role: Role,
    /// The message text.
    pub content: String,
}

impl ChatMessage {
    /// A user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    /// An assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// The author of a [`ChatMessage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// A message from the user.
    User,
    /// A message from the assistant.
    Assistant,
}

/// One event in a single turn's response stream.
///
/// Wire-neutral: the same reducer folds replay and real events. Tagged for
/// fixture (de)serialization so a recorded turn is a readable JSONL file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// A delta of assistant prose (markdown source).
    TextDelta {
        /// The appended text.
        text: String,
    },
    /// A delta of summarized thinking.
    ThinkingDelta {
        /// The appended thinking text.
        text: String,
    },
    /// The model requested a tool call; `id` correlates the eventual result.
    ToolUseStart {
        /// The tool-call id (echoed back with the result).
        id: String,
        /// The tool's name.
        name: String,
    },
    /// A delta of the tool call's streamed JSON input.
    ToolUseInputDelta {
        /// The tool-call id this delta belongs to.
        id: String,
        /// The appended JSON fragment.
        json: String,
    },
    /// The tool call's input is complete.
    ToolUseStop {
        /// The tool-call id that finished.
        id: String,
    },
    /// The turn finished: why, and the token usage it cost.
    MessageDone {
        /// Why the turn stopped.
        stop_reason: StopReason,
        /// The tokens the turn consumed.
        usage: Usage,
    },
}

/// Why a turn stopped, mirroring the wire's `stop_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// The assistant finished naturally.
    EndTurn,
    /// The output hit the token cap.
    MaxTokens,
    /// The model wants a tool run before continuing.
    ToolUse,
    /// A safety classifier declined the request.
    Refusal,
    /// A server-tool loop paused; re-send to resume.
    PauseTurn,
    /// A custom stop sequence was hit.
    StopSequence,
}

/// The token usage a turn reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens processed at full input price.
    #[serde(default)]
    pub input_tokens: u64,
    /// Tokens the assistant generated.
    #[serde(default)]
    pub output_tokens: u64,
}

/// A backend failure, surfaced into the transcript as an error cell — never a
/// crash or a silent drop.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BackendError {
    /// The transport failed (network, connection, I/O).
    Transport(String),
    /// The API returned an error status with a rendered message.
    Api {
        /// The HTTP status code.
        status: u16,
        /// A human-readable message.
        message: String,
    },
    /// The response could not be decoded into events.
    Decode(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::Transport(message) => write!(f, "transport error: {message}"),
            BackendError::Api { status, message } => write!(f, "api error {status}: {message}"),
            BackendError::Decode(message) => write!(f, "decode error: {message}"),
        }
    }
}

impl std::error::Error for BackendError {}

/// A source of assistant turns.
///
/// One method: turn a request into a stream of events. The app owns everything
/// above it — history, the tool loop, persistence — so a backend is exactly the
/// network seam and nothing more.
pub trait Backend {
    /// Sends `request` and returns the response event stream.
    ///
    /// The stream is owned (`'static`) so the app can spawn it as an effect that
    /// outlives this call. Errors ride *inside* the stream as
    /// `Err(BackendError)` items rather than a failed return, so a mid-turn
    /// failure lands in the transcript in order with the prose before it.
    fn send(&mut self, request: ChatRequest) -> EventStream;
}
