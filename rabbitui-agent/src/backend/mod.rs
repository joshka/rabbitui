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
/// `content` is a list of typed content blocks, serialized to match the
/// Anthropic wire exactly (`content` as an array of `{"type": ..., ...}`
/// objects). This same serialization *is* the session-persistence JSONL, so the
/// on-disk format and the request body share one shape. `user`/`assistant`
/// still produce a single [`ContentBlock::Text`] so existing call sites and
/// equality-by-constructor tests keep working (slice 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Who authored the message.
    pub role: Role,
    /// The message's content blocks, in wire order.
    pub content: Vec<ContentBlock>,
}

impl ChatMessage {
    /// A user message with a single text block.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: content.into(),
            }],
        }
    }

    /// An assistant message with a single text block.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: content.into(),
            }],
        }
    }

    /// An assistant message carrying arbitrary content blocks (thinking, text,
    /// tool_use) — the shape a `tool_use` turn replays as.
    #[must_use]
    pub fn assistant_blocks(content: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::Assistant,
            content,
        }
    }

    /// A user-role message of `tool_result` blocks — the single message that
    /// carries all results for a turn's (possibly parallel) tool calls.
    #[must_use]
    pub fn tool_results(content: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::User,
            content,
        }
    }

    /// The message's text: the concatenation of its [`ContentBlock::Text`]
    /// blocks (used for titles, the transcript, and the demo backend). Non-text
    /// blocks contribute nothing.
    #[must_use]
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// One content block of a [`ChatMessage`], serialized to the Anthropic wire.
///
/// The `#[serde(tag = "type")]` discriminant plus `rename_all = "snake_case"`
/// yields exactly the wire's `{"type": "text" | "thinking" | "tool_use" |
/// "tool_result", ...}` object shapes. A `thinking` block carries its opaque
/// `signature`, which must be replayed **verbatim** on the continuation request
/// after a tool-use turn (the API rejects modified thinking blocks) — see the
/// tool-continuation loop in `app.rs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Prose (assistant output or a user prompt).
    Text {
        /// The block's text.
        text: String,
    },
    /// A summarized-thinking block; `signature` is replayed verbatim.
    Thinking {
        /// The thinking text.
        thinking: String,
        /// The opaque signature the API returns and requires back unchanged.
        #[serde(default)]
        signature: String,
    },
    /// The model's request to run a tool.
    ToolUse {
        /// The tool-call id, echoed on the matching `tool_result`.
        id: String,
        /// The tool's name.
        name: String,
        /// The tool's JSON input.
        input: serde_json::Value,
    },
    /// The result of running a tool, returned in a user message.
    ToolResult {
        /// The `id` of the `tool_use` this answers.
        tool_use_id: String,
        /// The result (or error) text.
        content: String,
        /// Whether the tool call failed (or was denied).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_error: bool,
    },
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
    /// The signature that closes a thinking block. The wire sends this as a
    /// `signature_delta` right before the block stops; it must be replayed
    /// verbatim on the continuation request after a tool-use turn.
    ThinkingSignatureDelta {
        /// The (opaque) signature text.
        signature: String,
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

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    /// Round-trips a message through JSON and asserts it matches an exact wire
    /// value, then that it deserializes back unchanged.
    fn assert_wire(message: &ChatMessage, expected: Value) {
        let serialized = serde_json::to_value(message).expect("serializes");
        assert_eq!(serialized, expected, "wire JSON must match exactly");
        let back: ChatMessage = serde_json::from_value(serialized).expect("round-trips");
        assert_eq!(&back, message, "deserializes back to the same message");
    }

    #[test]
    fn text_block_matches_the_wire() {
        assert_wire(
            &ChatMessage::user("hello"),
            json!({"role": "user", "content": [{"type": "text", "text": "hello"}]}),
        );
    }

    #[test]
    fn thinking_block_carries_its_signature() {
        let message = ChatMessage::assistant_blocks(vec![ContentBlock::Thinking {
            thinking: "let me think".to_string(),
            signature: "sig-xyz".to_string(),
        }]);
        assert_wire(
            &message,
            json!({
                "role": "assistant",
                "content": [{
                    "type": "thinking",
                    "thinking": "let me think",
                    "signature": "sig-xyz"
                }]
            }),
        );
    }

    #[test]
    fn tool_use_block_matches_the_wire() {
        let message = ChatMessage::assistant_blocks(vec![ContentBlock::ToolUse {
            id: "toolu_1".to_string(),
            name: "read_file".to_string(),
            input: json!({"path": "a.rs"}),
        }]);
        assert_wire(
            &message,
            json!({
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "read_file",
                    "input": {"path": "a.rs"}
                }]
            }),
        );
    }

    #[test]
    fn tool_result_block_omits_is_error_when_false() {
        let ok = ChatMessage::tool_results(vec![ContentBlock::ToolResult {
            tool_use_id: "toolu_1".to_string(),
            content: "ok".to_string(),
            is_error: false,
        }]);
        // A successful result never serializes `is_error` — matching the wire,
        // where `is_error` is only sent on failures.
        assert_wire(
            &ok,
            json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_1",
                    "content": "ok"
                }]
            }),
        );
    }

    #[test]
    fn tool_result_block_includes_is_error_when_true() {
        let err = ChatMessage::tool_results(vec![ContentBlock::ToolResult {
            tool_use_id: "toolu_2".to_string(),
            content: "user denied this tool call".to_string(),
            is_error: true,
        }]);
        assert_wire(
            &err,
            json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_2",
                    "content": "user denied this tool call",
                    "is_error": true
                }]
            }),
        );
    }

    #[test]
    fn text_concatenates_only_text_blocks() {
        let message = ChatMessage::assistant_blocks(vec![
            ContentBlock::Thinking {
                thinking: "hmm".to_string(),
                signature: "s".to_string(),
            },
            ContentBlock::Text {
                text: "answer".to_string(),
            },
            ContentBlock::ToolUse {
                id: "t".to_string(),
                name: "read_file".to_string(),
                input: Value::Null,
            },
        ]);
        assert_eq!(message.text(), "answer");
    }
}
